//! Playwright sidecar 执行器(ROADMAP 1.4)。
//!
//! Rust 负责编排:定位 sidecar、发送 JSON 动作计划、读取 NDJSON 事件流并
//! 聚合为结构化证据。真实浏览器操作由 `executors/playwright-runner` 完成。
//! 未探测到 sidecar(如 CI 无 Node/Playwright)时,调用方降级到 HTTP 探测。

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use std::{env, io};

use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

/// 与 sidecar `runner.mjs` 的 PROTOCOL_VERSION 对齐。
pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Error)]
pub enum PlaywrightError {
    #[error("failed to start the Playwright runner (is Node.js installed?): {0}")]
    Spawn(#[source] io::Error),
    #[error("Playwright runner I/O failed: {0}")]
    Io(#[source] io::Error),
    #[error("Playwright runner timed out after {0:?}")]
    Timeout(Duration),
    #[error("browser setup failed: {0}")]
    Setup(String),
}

/// 已定位的 sidecar 位置。
#[derive(Debug, Clone)]
pub struct RunnerLocation {
    pub runner_js: PathBuf,
}

/// 发送给 sidecar 的动作计划。
#[derive(Debug, Clone, Serialize)]
pub struct BrowserPlanRequest {
    pub protocol_version: u32,
    pub base_url: String,
    pub timeout_ms: u64,
    pub screenshot_dir: String,
    pub actions: Vec<PlaywrightAction>,
}

/// 动作原语。探针只用 goto/wait_for_selector/screenshot;其余用于 1.5 的
/// 具体交互步骤,expect_hidden 用于 1.8 的筛选/隐藏类否定断言。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PlaywrightAction {
    Goto {
        url: String,
    },
    WaitForSelector {
        selector: String,
    },
    Click {
        selector: String,
    },
    Fill {
        selector: String,
        value: String,
    },
    Press {
        selector: String,
        key: String,
    },
    ExpectVisible {
        selector: String,
    },
    /// 断言元素不可见或不存在(用于筛选/隐藏类需求的否定断言)。
    ExpectHidden {
        selector: String,
    },
    ExpectText {
        selector: String,
        text: String,
    },
    Screenshot {
        name: String,
    },
    Eval {
        expression: String,
    },
}

impl PlaywrightAction {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Goto { .. } => "goto",
            Self::WaitForSelector { .. } => "wait_for_selector",
            Self::Click { .. } => "click",
            Self::Fill { .. } => "fill",
            Self::Press { .. } => "press",
            Self::ExpectVisible { .. } => "expect_visible",
            Self::ExpectHidden { .. } => "expect_hidden",
            Self::ExpectText { .. } => "expect_text",
            Self::Screenshot { .. } => "screenshot",
            Self::Eval { .. } => "eval",
        }
    }

    /// 动作的主要目标(selector / url / 名称),用于人类可读报告。
    pub fn target(&self) -> String {
        match self {
            Self::Goto { url } => url.clone(),
            Self::Screenshot { name } => name.clone(),
            Self::Eval { expression } => expression.clone(),
            Self::WaitForSelector { selector }
            | Self::Click { selector }
            | Self::ExpectVisible { selector }
            | Self::ExpectHidden { selector }
            | Self::Fill { selector, .. }
            | Self::Press { selector, .. }
            | Self::ExpectText { selector, .. } => selector.clone(),
        }
    }
}

/// sidecar 上报的事件(NDJSON,每行一个)。未知类型被忽略以便协议向前兼容。
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RunnerEvent {
    Started {
        protocol_version: u32,
    },
    ActionResult {
        index: usize,
        action: String,
        ok: bool,
        #[serde(default)]
        detail: Option<String>,
        #[serde(default)]
        path: Option<String>,
        /// 执行尝试次数(重试一次抗 flaky);缺省视为 1(ROADMAP 3.5)。
        #[serde(default)]
        attempts: Option<u32>,
    },
    Console {
        level: String,
        text: String,
    },
    PageError {
        message: String,
    },
    NetworkFailed {
        url: String,
        #[serde(default)]
        status: Option<u16>,
        #[serde(default)]
        failure: Option<String>,
    },
    Snapshot {
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        interactive: Vec<InteractiveElement>,
    },
    /// Playwright trace 归档路径(ROADMAP 3.5)。
    Trace {
        path: String,
    },
    Finished {
        ok: bool,
    },
    Fatal {
        message: String,
    },
    #[serde(other)]
    Unknown,
}

/// 聚合后的执行证据。
#[derive(Debug, Default, Serialize)]
pub struct PlaywrightOutcome {
    pub started: bool,
    pub protocol_version: Option<u32>,
    pub actions: Vec<ActionOutcome>,
    pub console: Vec<ConsoleMessage>,
    pub network_failures: Vec<NetworkFailure>,
    pub page_errors: Vec<String>,
    pub snapshot: Option<PageSnapshot>,
    /// 归档的 Playwright trace.zip 路径(ROADMAP 3.5)。
    pub trace_path: Option<String>,
    pub finished_ok: Option<bool>,
    pub fatal: Option<String>,
}

impl PlaywrightOutcome {
    /// 执行是否整体成功:sidecar 报告 finished.ok 且无 fatal。
    pub fn success(&self) -> bool {
        self.fatal.is_none() && self.finished_ok == Some(true)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ActionOutcome {
    pub index: usize,
    pub action: String,
    pub ok: bool,
    pub detail: Option<String>,
    pub screenshot_path: Option<String>,
    /// 执行尝试次数;>1 表示重试后才定案(抗 flaky,ROADMAP 3.5)。
    pub attempts: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConsoleMessage {
    pub level: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NetworkFailure {
    pub url: String,
    pub status: Option<u16>,
    pub failure: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PageSnapshot {
    pub title: Option<String>,
    pub interactive: Vec<InteractiveElement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractiveElement {
    pub tag: String,
    pub role: String,
    pub text: String,
    pub selector: String,
}

/// 定位 sidecar:要求 `runner.mjs` 存在且同目录已安装 `node_modules/playwright`。
/// 后者缺失(未 `npm install`)时视为不可用,调用方降级到 HTTP 探测。
pub fn detect_runner() -> Option<RunnerLocation> {
    // 逃生开关:强制走 HTTP 探测(用于对齐无 Node 的 CI、或用户显式禁用浏览器执行)。
    if env::var_os("SPECPROBE_NO_PLAYWRIGHT").is_some() {
        return None;
    }
    runner_candidates()
        .iter()
        .find_map(|candidate| resolve_runner(candidate))
}

fn runner_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(explicit) = env::var_os("SPECPROBE_PLAYWRIGHT_RUNNER") {
        candidates.push(PathBuf::from(explicit));
    }
    candidates.push(PathBuf::from("executors/playwright-runner/runner.mjs"));
    if let Ok(exe) = env::current_exe()
        && let Some(dir) = exe.parent()
    {
        // target/debug/specprobe.exe -> 仓库根
        candidates.push(dir.join("../../executors/playwright-runner/runner.mjs"));
    }
    candidates
}

fn resolve_runner(runner_js: &Path) -> Option<RunnerLocation> {
    if !runner_js.is_file() {
        return None;
    }
    let dir = runner_js.parent()?;
    if !dir.join("node_modules").join("playwright").is_dir() {
        return None;
    }
    Some(RunnerLocation {
        runner_js: runner_js.to_path_buf(),
    })
}

/// 定位 sidecar 目录(含 package.json),供 `setup-browser` 安装依赖。
pub fn runner_dir() -> Option<PathBuf> {
    runner_candidates().iter().find_map(|candidate| {
        let dir = candidate.parent()?;
        dir.join("package.json")
            .is_file()
            .then(|| dir.to_path_buf())
    })
}

/// 一键安装 sidecar 依赖:`npm install` + `npx playwright install chromium`。
/// 输出透传到终端。需要本机已装 Node.js。
pub async fn setup_runner() -> Result<(), PlaywrightError> {
    let dir = runner_dir().ok_or_else(|| {
        PlaywrightError::Setup(
            "executors/playwright-runner not found near the specprobe binary or working directory"
                .to_owned(),
        )
    })?;
    println!("Installing Playwright runner in {}", dir.display());
    run_setup_step(&dir, "npm", &["install"]).await?;
    run_setup_step(&dir, "npx", &["playwright", "install", "chromium"]).await?;
    println!("Playwright runner is ready.");
    Ok(())
}

async fn run_setup_step(dir: &Path, program: &str, args: &[&str]) -> Result<(), PlaywrightError> {
    println!("$ {program} {}", args.join(" "));
    // Windows 上 npm/npx 是 .cmd,必须经 cmd.exe 启动。
    let mut command = if cfg!(windows) {
        let mut command = Command::new("cmd");
        command.arg("/c").arg(program).args(args);
        command
    } else {
        let mut command = Command::new(program);
        command.args(args);
        command
    };
    let status = command
        .current_dir(dir)
        .status()
        .await
        .map_err(PlaywrightError::Spawn)?;
    if !status.success() {
        return Err(PlaywrightError::Setup(format!(
            "`{program}` exited with a non-zero status"
        )));
    }
    Ok(())
}

/// 运行 sidecar:发送计划、读取事件流、聚合证据。spawn/IO/超时失败返回 Err;
/// 只要读到事件流即返回 Ok(即便 outcome.fatal 有值),便于调用方保留部分证据。
pub async fn run_actions(
    location: &RunnerLocation,
    request: &BrowserPlanRequest,
    timeout: Duration,
) -> Result<PlaywrightOutcome, PlaywrightError> {
    let payload = json!(request).to_string();

    let mut child = Command::new("node")
        .arg(&location.runner_js)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(PlaywrightError::Spawn)?;

    let mut stdin = child.stdin.take().expect("stdin was piped");
    let stdout = child.stdout.take().expect("stdout was piped");

    let collect = async {
        stdin
            .write_all(payload.as_bytes())
            .await
            .map_err(PlaywrightError::Io)?;
        drop(stdin); // 关闭 stdin,sidecar 读到 EOF 后开始执行

        let mut lines = BufReader::new(stdout).lines();
        let mut raw = Vec::new();
        while let Some(line) = lines.next_line().await.map_err(PlaywrightError::Io)? {
            raw.push(line);
        }
        child.wait().await.map_err(PlaywrightError::Io)?;
        Ok::<_, PlaywrightError>(build_outcome(&raw))
    };

    match tokio::time::timeout(timeout, collect).await {
        Ok(result) => result,
        Err(_) => Err(PlaywrightError::Timeout(timeout)),
    }
}

/// 把 NDJSON 事件行聚合成结构化证据。非 JSON 行与未知事件被忽略。
fn build_outcome(lines: &[String]) -> PlaywrightOutcome {
    let mut outcome = PlaywrightOutcome::default();

    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(event) = serde_json::from_str::<RunnerEvent>(trimmed) else {
            continue;
        };
        match event {
            RunnerEvent::Started { protocol_version } => {
                outcome.started = true;
                outcome.protocol_version = Some(protocol_version);
            }
            RunnerEvent::ActionResult {
                index,
                action,
                ok,
                detail,
                path,
                attempts,
            } => outcome.actions.push(ActionOutcome {
                index,
                action,
                ok,
                detail,
                screenshot_path: path,
                attempts: attempts.unwrap_or(1).max(1),
            }),
            RunnerEvent::Console { level, text } => {
                outcome.console.push(ConsoleMessage { level, text })
            }
            RunnerEvent::PageError { message } => outcome.page_errors.push(message),
            RunnerEvent::NetworkFailed {
                url,
                status,
                failure,
            } => outcome.network_failures.push(NetworkFailure {
                url,
                status,
                failure,
            }),
            RunnerEvent::Snapshot { title, interactive } => {
                outcome.snapshot = Some(PageSnapshot { title, interactive })
            }
            RunnerEvent::Trace { path } => outcome.trace_path = Some(path),
            RunnerEvent::Finished { ok } => outcome.finished_ok = Some(ok),
            RunnerEvent::Fatal { message } => outcome.fatal = Some(message),
            RunnerEvent::Unknown => {}
        }
    }

    // sidecar 崩溃恢复(ROADMAP 3.5):事件流既无 finished 也无 fatal,说明 runner
    // 中途退出(崩溃 / 被杀)。把它显式记为 fatal,让调用方当作高severity失败,
    // 而不是静默地"未完成"。
    if outcome.finished_ok.is_none() && outcome.fatal.is_none() {
        outcome.fatal =
            Some("runner ended without a finish event (possible sidecar crash)".to_owned());
    }

    outcome
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{
        BrowserPlanRequest, PROTOCOL_VERSION, PlaywrightAction, build_outcome, resolve_runner,
    };
    use crate::testutil::temp_project;

    #[test]
    fn serializes_actions_as_tagged_json() {
        let request = BrowserPlanRequest {
            protocol_version: PROTOCOL_VERSION,
            base_url: "http://127.0.0.1:4173".to_owned(),
            timeout_ms: 10_000,
            screenshot_dir: "/tmp/run".to_owned(),
            actions: vec![
                PlaywrightAction::Goto {
                    url: "http://127.0.0.1:4173".to_owned(),
                },
                PlaywrightAction::Fill {
                    selector: "#task-input".to_owned(),
                    value: "  ".to_owned(),
                },
                PlaywrightAction::Screenshot {
                    name: "home".to_owned(),
                },
            ],
        };

        let value = serde_json::to_value(&request).expect("serialize");
        assert_eq!(value["protocol_version"], PROTOCOL_VERSION);
        assert_eq!(value["actions"][0]["type"], "goto");
        assert_eq!(value["actions"][1]["type"], "fill");
        assert_eq!(value["actions"][1]["selector"], "#task-input");
        assert_eq!(value["actions"][2]["type"], "screenshot");
    }

    #[test]
    fn builds_outcome_from_event_stream() {
        let lines = vec![
            r#"{"type":"started","protocol_version":1}"#.to_owned(),
            r#"{"type":"action_result","index":0,"action":"goto","ok":true,"detail":"navigated"}"#
                .to_owned(),
            r#"{"type":"console","level":"error","text":"Task API returned HTTP 500"}"#.to_owned(),
            r#"{"type":"network_failed","url":"http://x/api/tasks","status":500}"#.to_owned(),
            r#"{"type":"action_result","index":1,"action":"expect_visible","ok":true,"detail":"visible (passed on retry)","attempts":2}"#
                .to_owned(),
            r#"{"type":"action_result","index":2,"action":"screenshot","ok":true,"path":"/tmp/run/home.png"}"#
                .to_owned(),
            r##"{"type":"snapshot","title":"FocusBoard","interactive":[{"tag":"button","role":"button","text":"添加任务","selector":"#add-task-btn"}]}"##
                .to_owned(),
            r#"{"type":"trace","path":"/tmp/run/trace.zip"}"#.to_owned(),
            r#"{"type":"finished","ok":true}"#.to_owned(),
        ];

        let outcome = build_outcome(&lines);

        assert!(outcome.started);
        assert_eq!(outcome.protocol_version, Some(1));
        assert_eq!(outcome.actions.len(), 3);
        // 无 attempts 字段默认 1;重试过的动作 attempts=2。
        assert_eq!(outcome.actions[0].attempts, 1);
        assert_eq!(outcome.actions[1].attempts, 2);
        assert_eq!(
            outcome.actions[2].screenshot_path.as_deref(),
            Some("/tmp/run/home.png")
        );
        assert_eq!(outcome.console.len(), 1);
        assert_eq!(outcome.network_failures.len(), 1);
        assert_eq!(outcome.network_failures[0].status, Some(500));
        let snapshot = outcome.snapshot.as_ref().expect("snapshot present");
        assert_eq!(snapshot.title.as_deref(), Some("FocusBoard"));
        assert_eq!(snapshot.interactive[0].selector, "#add-task-btn");
        assert_eq!(outcome.trace_path.as_deref(), Some("/tmp/run/trace.zip"));
        assert_eq!(outcome.finished_ok, Some(true));
        assert!(outcome.success());
    }

    #[test]
    fn flags_missing_finish_as_crash() {
        // runner 启动并跑了一步,但没有 finished/fatal → 视为中途崩溃。
        let lines = vec![
            r#"{"type":"started","protocol_version":1}"#.to_owned(),
            r#"{"type":"action_result","index":0,"action":"goto","ok":true}"#.to_owned(),
        ];

        let outcome = build_outcome(&lines);

        assert!(outcome.started);
        assert!(outcome.finished_ok.is_none());
        assert!(
            outcome
                .fatal
                .as_deref()
                .is_some_and(|message| message.contains("crash"))
        );
        assert!(!outcome.success());
    }

    #[test]
    fn ignores_malformed_and_unknown_events() {
        let lines = vec![
            "not json".to_owned(),
            r#"{"type":"future_event","payload":1}"#.to_owned(),
            r#"{"type":"fatal","message":"boom"}"#.to_owned(),
        ];

        let outcome = build_outcome(&lines);

        assert!(!outcome.started);
        assert_eq!(outcome.fatal.as_deref(), Some("boom"));
        assert!(!outcome.success());
    }

    #[test]
    fn resolve_runner_requires_installed_playwright() {
        let root = temp_project("specprobe-pw-detect");
        let runner = root.join("runner.mjs");
        fs::write(&runner, "// runner").expect("write runner");

        // 缺少 node_modules/playwright 时不可用。
        assert!(resolve_runner(&runner).is_none());

        fs::create_dir_all(root.join("node_modules").join("playwright")).expect("mkdir");
        assert!(resolve_runner(&runner).is_some());

        fs::remove_dir_all(root).expect("cleanup");
    }
}
