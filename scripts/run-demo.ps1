param(
    [int]$Port = 4173
)

$ErrorActionPreference = "Stop"

$projectRoot = Split-Path -Parent $PSScriptRoot
$demoRoot = Join-Path $projectRoot "demo\buggy-task-board"
$requirements = Join-Path $demoRoot "REQUIREMENTS.md"
$reportRoot = Join-Path $projectRoot ".specprobe\demo-reports"
$cargoScript = Join-Path $PSScriptRoot "cargo-msvc.ps1"
$specprobe = Join-Path $projectRoot "target\debug\specprobe.exe"
$baseUrl = "http://127.0.0.1:$Port"

New-Item -ItemType Directory -Path $reportRoot -Force | Out-Null

Write-Host "[1/8] Building SpecProbe..."
& $cargoScript build
if ($LASTEXITCODE -ne 0) {
    throw "SpecProbe build failed."
}

function Write-SpecProbeReport {
    param(
        [string]$Name,
        [string[]]$Arguments
    )

    $output = & $specprobe @Arguments --json
    if ($LASTEXITCODE -ne 0) {
        throw "SpecProbe command failed while generating $Name."
    }
    $output | Set-Content -LiteralPath (Join-Path $reportRoot "$Name.json") -Encoding utf8
}

Write-Host "[2/8] Scanning demo project..."
Write-SpecProbeReport "01-scan" @("scan", $demoRoot)

Write-Host "[3/8] Parsing requirements..."
Write-SpecProbeReport "02-requirements" @("requirements", $requirements)

Write-Host "[4/8] Running offline AI analysis..."
Write-SpecProbeReport "03-ai-mock" @("ai", $requirements)

Write-Host "[5/8] Detecting launch command..."
Write-SpecProbeReport "04-launch-plan" @("launch", $demoRoot, "--dry-run")

Write-Host "[6/8] Starting FocusBoard..."
$server = Start-Process `
    -FilePath "node" `
    -ArgumentList @("server.js", "--port", $Port) `
    -WorkingDirectory $demoRoot `
    -PassThru `
    -WindowStyle Hidden

try {
    $ready = $false
    for ($attempt = 0; $attempt -lt 30; $attempt++) {
        try {
            $health = Invoke-WebRequest `
                -Uri "$baseUrl/health" `
                -TimeoutSec 1 `
                -UseBasicParsing
            if ($health.StatusCode -eq 200) {
                $ready = $true
                break
            }
        } catch {
            Start-Sleep -Milliseconds 200
        }
    }

    if (-not $ready) {
        throw "FocusBoard did not become ready at $baseUrl."
    }

    Write-Host "[7/8] Collecting browser and review evidence..."
    Write-SpecProbeReport "05-browser-home" @(
        "browser",
        $requirements,
        "--base-url",
        $baseUrl
    )
    Write-SpecProbeReport "06-review-broken-api" @(
        "review",
        $requirements,
        "--project",
        $demoRoot,
        "--base-url",
        "$baseUrl/api/tasks",
        "--execute",
        "--skip-launch"
    )

    Write-Host "[8/8] Generating patch proposals and regression checks..."
    Write-SpecProbeReport "07-proposals-broken-api" @(
        "propose",
        $requirements,
        "--project",
        $demoRoot,
        "--base-url",
        "$baseUrl/api/tasks",
        "--execute",
        "--skip-launch"
    )
} finally {
    if ($server -and -not $server.HasExited) {
        Stop-Process -Id $server.Id -Force
        Wait-Process -Id $server.Id -ErrorAction SilentlyContinue
    }
}

Write-Host ""
Write-Host "Demo complete. Reports: $reportRoot"
