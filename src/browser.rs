use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use thiserror::Error;

use crate::playwright::{
    BrowserPlanRequest, PROTOCOL_VERSION, PlaywrightAction, PlaywrightOutcome, RunnerLocation,
    detect_runner, run_actions,
};
use crate::requirements::{
    ExecutorHint, RequirementError, RequirementReport, TestAction, analyze_requirements,
};

const BODY_EXCERPT_LIMIT: usize = 2_000;
/// Playwright 总执行超时在每动作超时基础上的额外缓冲(秒)。
const PLAYWRIGHT_OVERHEAD_SECS: u64 = 10;

#[derive(Debug, Error)]
pub enum BrowserError {
    #[error(transparent)]
    Requirements(#[from] RequirementError),
    #[error("browser base URL is not supported: {0}")]
    UnsupportedUrl(String),
    #[error("failed to build the probe client: {0}")]
    Client(#[source] reqwest::Error),
    #[error("failed to probe {target}: {source}")]
    Probe {
        target: String,
        #[source]
        source: reqwest::Error,
    },
}

#[derive(Debug, Serialize)]
pub struct BrowserRunReport {
    pub requirement_source: String,
    pub base_url: String,
    pub backend: BrowserBackend,
    pub plan: BrowserActionPlan,
    pub execution: BrowserExecution,
    pub page: Option<PageProbeEvidence>,
    pub playwright: Option<PlaywrightEvidence>,
    pub diagnostics: Vec<BrowserDiagnostic>,
}

/// 本次执行采用的后端。`None` 表示 dry-run(仅生成计划)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserBackend {
    Playwright,
    HttpProbe,
    None,
}

/// Playwright 深度执行证据:归档目录与聚合的 sidecar 结果。
#[derive(Debug, Serialize)]
pub struct PlaywrightEvidence {
    pub run_dir: String,
    pub outcome: PlaywrightOutcome,
}

#[derive(Debug, Serialize)]
pub struct BrowserActionPlan {
    pub cases: Vec<BrowserCase>,
}

#[derive(Debug, Serialize)]
pub struct BrowserCase {
    pub id: String,
    pub requirement_id: String,
    pub title: String,
    pub source_executor_hint: ExecutorHint,
    pub actions: Vec<BrowserAction>,
    pub expected_result: String,
}

#[derive(Debug, Serialize)]
pub struct BrowserAction {
    pub action: BrowserActionKind,
    pub target: String,
    pub input: Option<String>,
    pub assertion: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserActionKind {
    OpenPage,
    WaitForReady,
    PerformInteraction,
    AssertVisibleResult,
    CollectEvidence,
}

#[derive(Debug, Serialize)]
pub struct BrowserExecution {
    pub attempted: bool,
    pub dry_run: bool,
    pub success: bool,
    pub duration_ms: u128,
    pub timeout_secs: u64,
}

#[derive(Debug, Serialize)]
pub struct PageProbeEvidence {
    pub url: String,
    pub status_code: Option<u16>,
    pub status_text: String,
    pub title: Option<String>,
    pub body_excerpt: String,
    pub response_bytes: usize,
}

#[derive(Debug, Serialize)]
pub struct BrowserDiagnostic {
    pub severity: BrowserDiagnosticSeverity,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserDiagnosticSeverity {
    Info,
    Warning,
    Error,
}

pub async fn run_browser_plan(
    requirements_path: &Path,
    base_url: &str,
    timeout_secs: u64,
    dry_run: bool,
) -> Result<BrowserRunReport, BrowserError> {
    let requirement_report = analyze_requirements(requirements_path)?;
    let plan = build_browser_action_plan(&requirement_report, base_url);
    let mut diagnostics = Vec::new();

    if plan.cases.is_empty() {
        diagnostics.push(BrowserDiagnostic {
            severity: BrowserDiagnosticSeverity::Warning,
            message: "No browser-suitable test cases were generated from the requirements."
                .to_owned(),
        });
    }

    if dry_run {
        diagnostics.push(BrowserDiagnostic {
            severity: BrowserDiagnosticSeverity::Info,
            message:
                "Browser plan was generated without probing the page because --dry-run was set."
                    .to_owned(),
        });
        return Ok(BrowserRunReport {
            requirement_source: requirement_report.source,
            base_url: base_url.to_owned(),
            backend: BrowserBackend::None,
            plan,
            execution: BrowserExecution {
                attempted: false,
                dry_run: true,
                success: true,
                duration_ms: 0,
                timeout_secs,
            },
            page: None,
            playwright: None,
            diagnostics,
        });
    }

    let start = Instant::now();
    let outcome = execute_plan(base_url, timeout_secs, &mut diagnostics).await;
    let duration_ms = start.elapsed().as_millis();

    Ok(BrowserRunReport {
        requirement_source: requirement_report.source,
        base_url: base_url.to_owned(),
        backend: outcome.backend,
        plan,
        execution: BrowserExecution {
            attempted: true,
            dry_run: false,
            success: outcome.success,
            duration_ms,
            timeout_secs,
        },
        page: outcome.page,
        playwright: outcome.playwright,
        diagnostics,
    })
}

struct ExecutionOutcome {
    backend: BrowserBackend,
    success: bool,
    page: Option<PageProbeEvidence>,
    playwright: Option<PlaywrightEvidence>,
}

/// 优先用 Playwright sidecar 深度执行;探测不到或执行失败则降级到 HTTP 探测。
async fn execute_plan(
    base_url: &str,
    timeout_secs: u64,
    diagnostics: &mut Vec<BrowserDiagnostic>,
) -> ExecutionOutcome {
    if let Some(location) = detect_runner() {
        match run_playwright(&location, base_url, timeout_secs).await {
            Ok(evidence) => return finish_playwright(evidence, diagnostics),
            Err(error) => diagnostics.push(BrowserDiagnostic {
                severity: BrowserDiagnosticSeverity::Warning,
                message: format!("Playwright runner failed ({error}); falling back to HTTP probe."),
            }),
        }
    } else {
        diagnostics.push(BrowserDiagnostic {
            severity: BrowserDiagnosticSeverity::Info,
            message: "Playwright runner not detected; using HTTP probe. Run `specprobe setup-browser` to enable full browser testing.".to_owned(),
        });
    }

    http_fallback(base_url, timeout_secs, diagnostics).await
}

fn finish_playwright(
    evidence: PlaywrightEvidence,
    diagnostics: &mut Vec<BrowserDiagnostic>,
) -> ExecutionOutcome {
    let success = evidence.outcome.success();
    if let Some(fatal) = &evidence.outcome.fatal {
        diagnostics.push(BrowserDiagnostic {
            severity: BrowserDiagnosticSeverity::Error,
            message: format!("Playwright runner reported a fatal error: {fatal}"),
        });
    }
    for action in evidence.outcome.actions.iter().filter(|action| !action.ok) {
        diagnostics.push(BrowserDiagnostic {
            severity: BrowserDiagnosticSeverity::Error,
            message: format!(
                "Action {} ({}) failed: {}",
                action.index,
                action.action,
                action.detail.as_deref().unwrap_or("no detail")
            ),
        });
    }
    for failure in &evidence.outcome.network_failures {
        diagnostics.push(BrowserDiagnostic {
            severity: BrowserDiagnosticSeverity::Error,
            message: format!(
                "Network failure: {}{}",
                failure.url,
                failure
                    .status
                    .map(|status| format!(" (HTTP {status})"))
                    .or_else(|| failure.failure.clone().map(|text| format!(" ({text})")))
                    .unwrap_or_default()
            ),
        });
    }
    for error in &evidence.outcome.page_errors {
        diagnostics.push(BrowserDiagnostic {
            severity: BrowserDiagnosticSeverity::Error,
            message: format!("Page error: {error}"),
        });
    }

    ExecutionOutcome {
        backend: BrowserBackend::Playwright,
        success,
        page: None,
        playwright: Some(evidence),
    }
}

async fn http_fallback(
    base_url: &str,
    timeout_secs: u64,
    diagnostics: &mut Vec<BrowserDiagnostic>,
) -> ExecutionOutcome {
    match probe_http_page(base_url, timeout_secs).await {
        Ok(page) => {
            let success = page
                .status_code
                .is_some_and(|status| (200..400).contains(&status));
            if !success {
                diagnostics.push(BrowserDiagnostic {
                    severity: BrowserDiagnosticSeverity::Error,
                    message: format!("Page returned non-success status: {}", page.status_text),
                });
            }
            ExecutionOutcome {
                backend: BrowserBackend::HttpProbe,
                success,
                page: Some(page),
                playwright: None,
            }
        }
        Err(error) => {
            diagnostics.push(BrowserDiagnostic {
                severity: BrowserDiagnosticSeverity::Error,
                message: error.to_string(),
            });
            ExecutionOutcome {
                backend: BrowserBackend::HttpProbe,
                success: false,
                page: None,
                playwright: None,
            }
        }
    }
}

/// 用 sidecar 打开页面并深度取证:导航、等待、截图,自动附带 console/网络/DOM 摘要。
/// 证据归档到 `.specprobe/runs/browser-<id>/`。
async fn run_playwright(
    location: &RunnerLocation,
    base_url: &str,
    timeout_secs: u64,
) -> Result<PlaywrightEvidence, crate::playwright::PlaywrightError> {
    let run_dir = PathBuf::from(".specprobe")
        .join("runs")
        .join(format!("browser-{}", unique_suffix()));
    let _ = fs::create_dir_all(&run_dir);
    let screenshot_dir = fs::canonicalize(&run_dir).unwrap_or_else(|_| run_dir.clone());

    let request = BrowserPlanRequest {
        protocol_version: PROTOCOL_VERSION,
        base_url: base_url.to_owned(),
        timeout_ms: timeout_secs.saturating_mul(1000).max(1000),
        screenshot_dir: normalize_path(&screenshot_dir),
        actions: vec![
            PlaywrightAction::Goto {
                url: base_url.to_owned(),
            },
            PlaywrightAction::WaitForSelector {
                selector: "body".to_owned(),
            },
            PlaywrightAction::Screenshot {
                name: "page".to_owned(),
            },
        ],
    };

    let overall = Duration::from_secs(timeout_secs.max(1).saturating_add(PLAYWRIGHT_OVERHEAD_SECS));
    let outcome = run_actions(location, &request, overall).await?;
    archive_outcome(&run_dir, &outcome);

    Ok(PlaywrightEvidence {
        run_dir: normalize_path(&run_dir),
        outcome,
    })
}

/// 把 console/network/snapshot 尽力归档为 JSON,失败不影响主流程。
fn archive_outcome(run_dir: &Path, outcome: &PlaywrightOutcome) {
    let files = [
        (
            "console.json",
            serde_json::to_string_pretty(&outcome.console),
        ),
        (
            "network.json",
            serde_json::to_string_pretty(&outcome.network_failures),
        ),
        (
            "snapshot.json",
            serde_json::to_string_pretty(&outcome.snapshot),
        ),
    ];
    for (name, content) in files {
        if let Ok(json) = content {
            let _ = fs::write(run_dir.join(name), json);
        }
    }
}

fn unique_suffix() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    format!("{millis}-{}", std::process::id())
}

fn normalize_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    value
        .strip_prefix(r"\\?\")
        .unwrap_or(&value)
        .replace('\\', "/")
}

fn build_browser_action_plan(
    requirement_report: &RequirementReport,
    base_url: &str,
) -> BrowserActionPlan {
    let cases = requirement_report
        .test_plan
        .cases
        .iter()
        .filter(|case| {
            matches!(
                case.executor_hint,
                ExecutorHint::Browser | ExecutorHint::Generic | ExecutorHint::ManualReview
            )
        })
        .map(|case| BrowserCase {
            id: format!("BROWSER-{}", case.id),
            requirement_id: case.requirement_id.clone(),
            title: case.title.clone(),
            source_executor_hint: case.executor_hint,
            actions: vec![
                BrowserAction {
                    action: BrowserActionKind::OpenPage,
                    target: base_url.to_owned(),
                    input: None,
                    assertion: Some("page responds with a successful HTTP status".to_owned()),
                },
                BrowserAction {
                    action: BrowserActionKind::WaitForReady,
                    target: "document".to_owned(),
                    input: None,
                    assertion: Some("page body is available for inspection".to_owned()),
                },
                BrowserAction {
                    action: BrowserActionKind::PerformInteraction,
                    target: action_target_for_case(case),
                    input: action_input_for_case(case),
                    assertion: None,
                },
                BrowserAction {
                    action: BrowserActionKind::AssertVisibleResult,
                    target: "page".to_owned(),
                    input: None,
                    assertion: Some(case.expected_result.clone()),
                },
                BrowserAction {
                    action: BrowserActionKind::CollectEvidence,
                    target: "browser".to_owned(),
                    input: None,
                    assertion: Some(
                        "collect page status, title and body excerpt; console errors and screenshot require Playwright backend".to_owned(),
                    ),
                },
            ],
            expected_result: case.expected_result.clone(),
        })
        .collect();

    BrowserActionPlan { cases }
}

fn action_target_for_case(case: &crate::requirements::TestCase) -> String {
    case.steps
        .iter()
        .find(|step| step.action == TestAction::PerformRequirementAction)
        .map(|step| step.target.clone())
        .unwrap_or_else(|| "page".to_owned())
}

fn action_input_for_case(case: &crate::requirements::TestCase) -> Option<String> {
    case.steps
        .iter()
        .find(|step| step.action == TestAction::PerformRequirementAction)
        .and_then(|step| step.input.clone())
}

/// 探测只支持 http/https;重定向跟随、超时、chunked 解码由 reqwest 处理。
fn is_supported_probe_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

async fn probe_http_page(url: &str, timeout_secs: u64) -> Result<PageProbeEvidence, BrowserError> {
    if !is_supported_probe_url(url) {
        return Err(BrowserError::UnsupportedUrl(url.to_owned()));
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs.max(1)))
        .user_agent(concat!("specprobe/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(BrowserError::Client)?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|source| BrowserError::Probe {
            target: url.to_owned(),
            source,
        })?;

    let status = response.status();
    let status_text = format!("{:?} {status}", response.version());
    let body_bytes = response
        .bytes()
        .await
        .map_err(|source| BrowserError::Probe {
            target: url.to_owned(),
            source,
        })?;
    let body = String::from_utf8_lossy(&body_bytes);

    Ok(PageProbeEvidence {
        url: url.to_owned(),
        status_code: Some(status.as_u16()),
        status_text,
        title: extract_title(&body),
        body_excerpt: excerpt(body.trim(), BODY_EXCERPT_LIMIT),
        response_bytes: body_bytes.len(),
    })
}

fn extract_title(body: &str) -> Option<String> {
    let lower = body.to_ascii_lowercase();
    let start = lower.find("<title>")? + "<title>".len();
    let end = lower[start..].find("</title>")? + start;
    Some(body[start..end].trim().to_owned())
}

fn excerpt(text: &str, limit: usize) -> String {
    let mut value = text.chars().take(limit).collect::<String>();
    if text.chars().count() > limit {
        value.push_str("...");
    }
    value
}

impl fmt::Display for BrowserActionKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::OpenPage => "open-page",
            Self::WaitForReady => "wait-for-ready",
            Self::PerformInteraction => "perform-interaction",
            Self::AssertVisibleResult => "assert-visible-result",
            Self::CollectEvidence => "collect-evidence",
        })
    }
}

impl fmt::Display for BrowserDiagnosticSeverity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        })
    }
}

impl fmt::Display for BrowserBackend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Playwright => "playwright",
            Self::HttpProbe => "http-probe",
            Self::None => "none",
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{extract_title, is_supported_probe_url, run_browser_plan};

    #[tokio::test]
    async fn browser_dry_run_builds_action_plan() {
        let root = temp_project("specprobe-browser-plan");
        let file = root.join("PRD.md");
        fs::write(&file, "- 页面应该显示登录成功提示。").expect("write requirement");

        let report = run_browser_plan(&file, "http://127.0.0.1:3000", 2, true)
            .await
            .expect("dry run succeeds");

        assert!(!report.execution.attempted);
        assert!(report.execution.success);
        assert_eq!(report.plan.cases.len(), 1);
        assert_eq!(report.plan.cases[0].actions.len(), 5);
        fs::remove_dir_all(root).expect("remove test project");
    }

    #[tokio::test]
    async fn http_probe_collects_status_title_and_body() {
        let root = temp_project("specprobe-browser-http");
        let file = root.join("PRD.md");
        fs::write(&file, "- 页面应该显示首页标题。").expect("write requirement");
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let port = listener.local_addr().expect("read local addr").port();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept connection");
            let mut buffer = [0_u8; 1024];
            let _ = stream.read(&mut buffer);
            let body = "<html><head><title>SpecProbe Demo</title></head><body>Hello</body></html>";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .expect("write response");
        });

        let report = run_browser_plan(&file, &format!("http://127.0.0.1:{port}/"), 3, false)
            .await
            .expect("browser probe succeeds");

        handle.join().expect("server thread joins");
        assert!(report.execution.success);
        assert_eq!(
            report.page.and_then(|page| page.title),
            Some("SpecProbe Demo".to_owned())
        );
        fs::remove_dir_all(root).expect("remove test project");
    }

    #[test]
    fn accepts_only_http_and_https_probe_urls() {
        assert!(is_supported_probe_url("http://127.0.0.1:3000"));
        assert!(is_supported_probe_url("https://example.com"));
        assert!(!is_supported_probe_url("ftp://example.com"));
        assert!(!is_supported_probe_url("file:///tmp/index.html"));
    }

    #[test]
    fn extracts_page_title() {
        assert_eq!(
            extract_title("<html><title>Hello</title></html>"),
            Some("Hello".to_owned())
        );
    }

    fn temp_project(prefix: &str) -> std::path::PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("{prefix}-{suffix}"));
        fs::create_dir_all(&root).expect("create temp project");
        root
    }
}
