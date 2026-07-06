param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$CargoArgs
)

$ErrorActionPreference = "Stop"

function Find-VcVars64 {
    $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path -LiteralPath $vswhere) {
        $installation = & $vswhere -latest -products * `
            -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
            -property installationPath
        if ($installation) {
            $candidate = Join-Path $installation "VC\Auxiliary\Build\vcvars64.bat"
            if (Test-Path -LiteralPath $candidate) {
                return $candidate
            }
        }
    }

    foreach ($edition in @("Community", "Professional", "Enterprise", "BuildTools")) {
        $candidate = "C:\Program Files\Microsoft Visual Studio\2022\$edition\VC\Auxiliary\Build\vcvars64.bat"
        if (Test-Path -LiteralPath $candidate) {
            return $candidate
        }
    }

    throw "Visual Studio C++ build environment was not found."
}

$vcvars = Find-VcVars64
$environment = & cmd.exe /d /s /c "`"$vcvars`" >nul && set"
foreach ($line in $environment) {
    $name, $value = $line -split "=", 2
    if ($name -and $null -ne $value) {
        Set-Item -Path "Env:$name" -Value $value
    }
}

if (-not $CargoArgs) {
    $CargoArgs = @("build")
}

& cargo @CargoArgs
exit $LASTEXITCODE
