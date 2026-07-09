use std::env;
use std::fmt;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use thiserror::Error;
use tokio::process::{Child, Command};

const LOG_EXCERPT_LIMIT: usize = 4_000;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("project path does not exist: {0}")]
    NotFound(PathBuf),
    #[error("project path is not a directory: {0}")]
    NotDirectory(PathBuf),
    #[error("failed to inspect {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("no launch command could be detected for {0}")]
    NoLaunchCommand(PathBuf),
}

#[derive(Debug, Serialize)]
pub struct LaunchReport {
    pub project_root: String,
    pub adapter: ProjectAdapterKind,
    pub command: LaunchCommand,
    pub execution: LaunchExecution,
    /// 托管模式(ManagedApp)下的就绪探测结果;一次性 launch 为 None。
    pub readiness: Option<ReadinessReport>,
    pub stdout_excerpt: String,
    pub stderr_excerpt: String,
    pub diagnostics: Vec<LaunchDiagnostic>,
}

/// 被测服务的就绪探测结果。
#[derive(Debug, Clone, Serialize)]
pub struct ReadinessReport {
    /// 是否进行了探测(提供了 base_url 才探测)。
    pub probed: bool,
    pub ready: bool,
    pub waited_ms: u128,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectAdapterKind {
    Node,
    Rust,
    Python,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LaunchCommand {
    pub program: String,
    pub args: Vec<String>,
    pub working_directory: String,
    pub source: String,
    pub confidence: CommandConfidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandConfidence {
    High,
    Medium,
    Low,
}

#[derive(Debug, Serialize)]
pub struct LaunchExecution {
    pub attempted: bool,
    pub dry_run: bool,
    pub success: bool,
    pub timed_out: bool,
    /// 超时被终止前仍在运行且已产生输出的进程,视为长驻服务而非失败。
    /// 这是临时启发式,基于就绪探测的托管生命周期见 ROADMAP 1.6。
    pub long_running: bool,
    pub exit_code: Option<i32>,
    pub duration_ms: u128,
    pub timeout_secs: u64,
}

#[derive(Debug, Serialize)]
pub struct LaunchDiagnostic {
    pub severity: RuntimeDiagnosticSeverity,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeDiagnosticSeverity {
    Info,
    Warning,
    Error,
}

impl fmt::Display for ProjectAdapterKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Node => "node",
            Self::Rust => "rust",
            Self::Python => "python",
            Self::Unknown => "unknown",
        })
    }
}

impl fmt::Display for CommandConfidence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        })
    }
}

impl fmt::Display for RuntimeDiagnosticSeverity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        })
    }
}

struct CommandPlan {
    adapter: ProjectAdapterKind,
    command: LaunchCommand,
}

pub async fn launch_project(
    path: &Path,
    timeout_secs: u64,
    dry_run: bool,
) -> Result<LaunchReport, RuntimeError> {
    if !path.exists() {
        return Err(RuntimeError::NotFound(path.to_path_buf()));
    }
    if !path.is_dir() {
        return Err(RuntimeError::NotDirectory(path.to_path_buf()));
    }

    let root = fs::canonicalize(path).map_err(|source| RuntimeError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let Some(plan) = detect_launch_command(&root) else {
        return Err(RuntimeError::NoLaunchCommand(root));
    };

    if dry_run {
        return Ok(LaunchReport {
            project_root: display_path(&root),
            adapter: plan.adapter,
            command: plan.command,
            execution: LaunchExecution {
                attempted: false,
                dry_run: true,
                success: false,
                timed_out: false,
                long_running: false,
                exit_code: None,
                duration_ms: 0,
                timeout_secs,
            },
            readiness: None,
            stdout_excerpt: String::new(),
            stderr_excerpt: String::new(),
            diagnostics: vec![LaunchDiagnostic {
                severity: RuntimeDiagnosticSeverity::Info,
                message: "Launch command was detected but not executed because --dry-run was set."
                    .to_owned(),
            }],
        });
    }

    let command = plan.command.clone();
    let run = run_command(&command, timeout_secs).await?;
    let mut diagnostics = Vec::new();

    if run.execution.long_running {
        diagnostics.push(LaunchDiagnostic {
            severity: RuntimeDiagnosticSeverity::Info,
            message: "Process was still running with output when the timeout elapsed; treated as a long-running service, not a failure.".to_owned(),
        });
    } else {
        if run.execution.timed_out {
            diagnostics.push(LaunchDiagnostic {
                severity: RuntimeDiagnosticSeverity::Warning,
                message: "Process exceeded the timeout and was killed.".to_owned(),
            });
        }
        if !run.execution.success {
            diagnostics.push(LaunchDiagnostic {
                severity: RuntimeDiagnosticSeverity::Error,
                message: "Process did not exit successfully.".to_owned(),
            });
        }
    }

    Ok(LaunchReport {
        project_root: display_path(&root),
        adapter: plan.adapter,
        command,
        execution: run.execution,
        readiness: None,
        stdout_excerpt: run.stdout_excerpt,
        stderr_excerpt: run.stderr_excerpt,
        diagnostics,
    })
}

const READINESS_POLL_INTERVAL: Duration = Duration::from_millis(200);

/// 托管的被测服务:已启动并持有,可探测就绪、保持运行、优雅关停。
/// 取代"运行到退出或超时杀掉"的模型,用于 Web 服务器这类长驻进程。
pub struct ManagedApp {
    child: Child,
    pid: Option<u32>,
    log_dir: PathBuf,
    stdout_path: PathBuf,
    stderr_path: PathBuf,
    project_root: String,
    adapter: ProjectAdapterKind,
    command: LaunchCommand,
    started: Instant,
    readiness: Option<ReadinessReport>,
}

/// 启动被测服务并持有进程(不等待退出)。stdout/stderr 落盘,供关停时采集。
pub async fn start_app(path: &Path) -> Result<ManagedApp, RuntimeError> {
    if !path.exists() {
        return Err(RuntimeError::NotFound(path.to_path_buf()));
    }
    if !path.is_dir() {
        return Err(RuntimeError::NotDirectory(path.to_path_buf()));
    }
    let root = fs::canonicalize(path).map_err(|source| RuntimeError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let Some(plan) = detect_launch_command(&root) else {
        return Err(RuntimeError::NoLaunchCommand(root));
    };
    spawn_managed(display_path(&root), plan.adapter, plan.command)
}

fn spawn_managed(
    project_root: String,
    adapter: ProjectAdapterKind,
    command: LaunchCommand,
) -> Result<ManagedApp, RuntimeError> {
    let working_directory = PathBuf::from(&command.working_directory);
    let log_dir = env::temp_dir().join(format!("specprobe-app-{}", unique_suffix()));
    fs::create_dir_all(&log_dir).map_err(|source| RuntimeError::Io {
        path: log_dir.clone(),
        source,
    })?;
    let stdout_path = log_dir.join("stdout.log");
    let stderr_path = log_dir.join("stderr.log");
    let stdout_file = File::create(&stdout_path).map_err(|source| RuntimeError::Io {
        path: stdout_path.clone(),
        source,
    })?;
    let stderr_file = File::create(&stderr_path).map_err(|source| RuntimeError::Io {
        path: stderr_path.clone(),
        source,
    })?;

    let mut process = build_process(&command);
    process
        .current_dir(&working_directory)
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file))
        .kill_on_drop(true);

    let child = process.spawn().map_err(|source| RuntimeError::Io {
        path: working_directory,
        source,
    })?;
    let pid = child.id();

    Ok(ManagedApp {
        child,
        pid,
        log_dir,
        stdout_path,
        stderr_path,
        project_root,
        adapter,
        command,
        started: Instant::now(),
        readiness: None,
    })
}

impl ManagedApp {
    /// 轮询 base_url 直到收到任意 HTTP 响应(视为就绪)、进程退出或超时。
    /// base_url 为 None 时跳过探测,短暂等待让进程初始化。
    pub async fn wait_until_ready(
        &mut self,
        base_url: Option<&str>,
        timeout_secs: u64,
    ) -> ReadinessReport {
        let start = Instant::now();
        let timeout = Duration::from_secs(timeout_secs.max(1));

        let Some(url) = base_url else {
            tokio::time::sleep(READINESS_POLL_INTERVAL).await;
            let report = ReadinessReport {
                probed: false,
                ready: true,
                waited_ms: start.elapsed().as_millis(),
                detail: "No base URL provided; skipped readiness probe.".to_owned(),
            };
            self.readiness = Some(report.clone());
            return report;
        };

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .ok();

        let report = loop {
            if let Ok(Some(status)) = self.child.try_wait() {
                break ReadinessReport {
                    probed: true,
                    ready: false,
                    waited_ms: start.elapsed().as_millis(),
                    detail: format!(
                        "Process exited before becoming ready (exit code {:?}).",
                        status.code()
                    ),
                };
            }
            if let Some(client) = &client
                && client.get(url).send().await.is_ok()
            {
                break ReadinessReport {
                    probed: true,
                    ready: true,
                    waited_ms: start.elapsed().as_millis(),
                    detail: format!("Server responded at {url}."),
                };
            }
            if start.elapsed() >= timeout {
                break ReadinessReport {
                    probed: true,
                    ready: false,
                    waited_ms: start.elapsed().as_millis(),
                    detail: format!("Timed out after {timeout_secs}s waiting for {url}."),
                };
            }
            tokio::time::sleep(READINESS_POLL_INTERVAL).await;
        };
        self.readiness = Some(report.clone());
        report
    }

    /// 关停服务:杀进程树、回收、采集脱敏日志,返回 LaunchReport。
    pub async fn shutdown(mut self) -> LaunchReport {
        let was_running = matches!(self.child.try_wait(), Ok(None));
        if let Some(pid) = self.pid {
            kill_tree(&mut self.child, pid).await;
        } else {
            let _ = self.child.start_kill();
        }
        let exit_status = self.child.wait().await.ok();
        let duration_ms = self.started.elapsed().as_millis();
        let stdout_excerpt = read_redacted_excerpt(&self.stdout_path);
        let stderr_excerpt = read_redacted_excerpt(&self.stderr_path);
        let _ = fs::remove_dir_all(&self.log_dir);

        let ready = self
            .readiness
            .as_ref()
            .map(|report| report.ready)
            .unwrap_or(was_running);

        let mut diagnostics = Vec::new();
        match &self.readiness {
            Some(report) if report.probed && report.ready => diagnostics.push(LaunchDiagnostic {
                severity: RuntimeDiagnosticSeverity::Info,
                message: format!("Service became ready in {} ms.", report.waited_ms),
            }),
            Some(report) if report.probed => diagnostics.push(LaunchDiagnostic {
                severity: RuntimeDiagnosticSeverity::Error,
                message: report.detail.clone(),
            }),
            _ => {}
        }
        if !was_running {
            diagnostics.push(LaunchDiagnostic {
                severity: RuntimeDiagnosticSeverity::Error,
                message: "Service process was no longer running before shutdown.".to_owned(),
            });
        }

        LaunchReport {
            project_root: self.project_root,
            adapter: self.adapter,
            command: self.command,
            execution: LaunchExecution {
                attempted: true,
                dry_run: false,
                success: ready && was_running,
                timed_out: false,
                long_running: was_running,
                exit_code: exit_status.and_then(|status| status.code()),
                duration_ms,
                timeout_secs: 0,
            },
            readiness: self.readiness,
            stdout_excerpt,
            stderr_excerpt,
            diagnostics,
        }
    }
}

/// 杀进程树。Windows 用 taskkill /T;Unix 依赖 build_process 设置的进程组,
/// 负 PID 向整组发信号(解决 npm -> node 的孤儿进程泄漏)。
async fn kill_tree(child: &mut Child, pid: u32) {
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
    }
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .arg("-TERM")
            .arg(format!("-{pid}"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
    }
    let _ = child.start_kill();
}

fn detect_launch_command(root: &Path) -> Option<CommandPlan> {
    detect_node_command(root)
        .or_else(|| detect_rust_command(root))
        .or_else(|| detect_python_command(root))
}

fn detect_node_command(root: &Path) -> Option<CommandPlan> {
    let package_json = root.join("package.json");
    let contents = fs::read_to_string(package_json).ok()?;
    let document =
        serde_json::from_str::<serde_json::Value>(contents.trim_start_matches('\u{feff}')).ok()?;
    let scripts = document.get("scripts")?.as_object()?;
    let script_name = ["dev", "start", "serve", "preview", "test"]
        .into_iter()
        .find(|candidate| scripts.contains_key(*candidate))?;
    let package_manager = detect_package_manager(root);

    Some(CommandPlan {
        adapter: ProjectAdapterKind::Node,
        command: LaunchCommand {
            program: package_manager.program.to_owned(),
            args: package_manager.args_for(script_name),
            working_directory: display_path(root),
            source: format!("package.json scripts.{script_name}"),
            confidence: if script_name == "test" {
                CommandConfidence::Medium
            } else {
                CommandConfidence::High
            },
        },
    })
}

fn detect_rust_command(root: &Path) -> Option<CommandPlan> {
    root.join("Cargo.toml").is_file().then(|| CommandPlan {
        adapter: ProjectAdapterKind::Rust,
        command: LaunchCommand {
            program: "cargo".to_owned(),
            args: vec!["run".to_owned()],
            working_directory: display_path(root),
            source: "Cargo.toml".to_owned(),
            confidence: CommandConfidence::Medium,
        },
    })
}

fn detect_python_command(root: &Path) -> Option<CommandPlan> {
    let app = root.join("app.py");
    let main = root.join("main.py");
    let entry = if app.is_file() {
        Some("app.py")
    } else if main.is_file() {
        Some("main.py")
    } else {
        None
    }?;

    Some(CommandPlan {
        adapter: ProjectAdapterKind::Python,
        command: LaunchCommand {
            program: "python".to_owned(),
            args: vec![entry.to_owned()],
            working_directory: display_path(root),
            source: entry.to_owned(),
            confidence: CommandConfidence::Low,
        },
    })
}

struct PackageManager {
    program: &'static str,
    run_word: Option<&'static str>,
}

impl PackageManager {
    fn args_for(&self, script_name: &str) -> Vec<String> {
        match self.run_word {
            Some(run_word) => vec![run_word.to_owned(), script_name.to_owned()],
            None => vec![script_name.to_owned()],
        }
    }
}

fn detect_package_manager(root: &Path) -> PackageManager {
    if root.join("pnpm-lock.yaml").is_file() {
        PackageManager {
            program: "pnpm",
            run_word: Some("run"),
        }
    } else if root.join("yarn.lock").is_file() {
        PackageManager {
            program: "yarn",
            run_word: None,
        }
    } else if root.join("bun.lock").is_file() || root.join("bun.lockb").is_file() {
        PackageManager {
            program: "bun",
            run_word: Some("run"),
        }
    } else {
        PackageManager {
            program: "npm",
            run_word: Some("run"),
        }
    }
}

struct CommandRun {
    execution: LaunchExecution,
    stdout_excerpt: String,
    stderr_excerpt: String,
}

async fn run_command(
    command: &LaunchCommand,
    timeout_secs: u64,
) -> Result<CommandRun, RuntimeError> {
    let working_directory = PathBuf::from(&command.working_directory);
    let log_dir = env::temp_dir().join(format!("specprobe-run-{}", unique_suffix()));
    fs::create_dir_all(&log_dir).map_err(|source| RuntimeError::Io {
        path: log_dir.clone(),
        source,
    })?;
    let stdout_path = log_dir.join("stdout.log");
    let stderr_path = log_dir.join("stderr.log");

    let stdout_file = File::create(&stdout_path).map_err(|source| RuntimeError::Io {
        path: stdout_path.clone(),
        source,
    })?;
    let stderr_file = File::create(&stderr_path).map_err(|source| RuntimeError::Io {
        path: stderr_path.clone(),
        source,
    })?;

    let mut process = build_process(command);
    process
        .current_dir(&working_directory)
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file))
        .kill_on_drop(true);

    let start = Instant::now();
    let mut child = process.spawn().map_err(|source| RuntimeError::Io {
        path: working_directory.clone(),
        source,
    })?;
    let timeout = Duration::from_secs(timeout_secs.max(1));
    let mut timed_out = false;
    let exit_status = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(status) => status.map_err(|source| RuntimeError::Io {
            path: working_directory.clone(),
            source,
        })?,
        Err(_elapsed) => {
            timed_out = true;
            // 超时与自然退出存在竞争:对已退出进程 start_kill 返回 InvalidInput,忽略。
            if let Err(source) = child.start_kill()
                && source.kind() != io::ErrorKind::InvalidInput
            {
                return Err(RuntimeError::Io {
                    path: working_directory.clone(),
                    source,
                });
            }
            child.wait().await.map_err(|source| RuntimeError::Io {
                path: working_directory.clone(),
                source,
            })?
        }
    };

    let duration_ms = start.elapsed().as_millis();
    let stdout_excerpt = read_redacted_excerpt(&stdout_path);
    let stderr_excerpt = read_redacted_excerpt(&stderr_path);
    let _ = fs::remove_dir_all(&log_dir);

    let long_running = timed_out && (!stdout_excerpt.is_empty() || !stderr_excerpt.is_empty());

    Ok(CommandRun {
        execution: LaunchExecution {
            attempted: true,
            dry_run: false,
            success: (exit_status.success() && !timed_out) || long_running,
            timed_out,
            long_running,
            exit_code: exit_status.code(),
            duration_ms,
            timeout_secs,
        },
        stdout_excerpt,
        stderr_excerpt,
    })
}

fn build_process(command: &LaunchCommand) -> Command {
    let resolved = resolve_program(&command.program);
    let executable = resolved
        .as_deref()
        .unwrap_or_else(|| Path::new(&command.program));
    let is_windows_script = cfg!(windows)
        && executable
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
            });

    let process = if is_windows_script {
        let mut process = Command::new("cmd.exe");
        process
            .args(["/d", "/c"])
            .arg(executable)
            .args(&command.args);
        process
    } else {
        let mut process = Command::new(executable);
        process.args(&command.args);
        process
    };
    // Unix 上放入新进程组,使关停时可用负 PID 杀掉整棵进程树(如 npm -> node)。
    #[cfg(unix)]
    let process = {
        let mut process = process;
        process.process_group(0);
        process
    };
    process
}

fn resolve_program(program: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    let extensions: &[&str] = if cfg!(windows) {
        &[".exe", ".cmd", ".bat", ""]
    } else {
        &[""]
    };

    env::split_paths(&path)
        .flat_map(|directory| {
            extensions
                .iter()
                .map(move |extension| directory.join(format!("{program}{extension}")))
        })
        .find(|candidate| candidate.is_file())
}

fn read_redacted_excerpt(path: &Path) -> String {
    let Ok(contents) = fs::read_to_string(path) else {
        return String::new();
    };
    excerpt(&redact_sensitive_lines(&contents), LOG_EXCERPT_LIMIT)
}

fn redact_sensitive_lines(contents: &str) -> String {
    contents
        .lines()
        .map(|line| {
            let lower = line.to_ascii_lowercase();
            if ["api_key", "apikey", "token", "secret", "password"]
                .iter()
                .any(|needle| lower.contains(needle))
            {
                "[REDACTED LINE]"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn excerpt(text: &str, limit: usize) -> String {
    let mut value = text.chars().take(limit).collect::<String>();
    if text.chars().count() > limit {
        value.push_str("...");
    }
    value
}

// 仅毫秒 + 进程 ID 不够:同进程并发的 run_command(如并行测试)会在同一毫秒
// 拿到相同目录,互相截断日志文件并提前删除对方的目录,导致 stdout 采集为空。
static LOG_DIR_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn unique_suffix() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let sequence = LOG_DIR_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{millis}-{}-{sequence}", std::process::id())
}

fn display_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    value.strip_prefix(r"\\?\").unwrap_or(&value).to_owned()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        CommandConfidence, LaunchCommand, ProjectAdapterKind, detect_launch_command,
        launch_project, redact_sensitive_lines, run_command, spawn_managed,
    };
    use crate::testutil::{chat_response, spawn_chat_server};

    #[test]
    fn detects_node_dev_script() {
        let root = temp_project("specprobe-runtime-node");
        fs::write(
            root.join("package.json"),
            r#"{"scripts":{"dev":"vite --host 127.0.0.1","start":"node server.js"}}"#,
        )
        .expect("write package manifest");

        let plan = detect_launch_command(&root).expect("command should be detected");

        assert_eq!(plan.adapter, ProjectAdapterKind::Node);
        assert_eq!(plan.command.program, "npm");
        assert_eq!(plan.command.args, vec!["run", "dev"]);
        assert_eq!(plan.command.confidence, CommandConfidence::High);
        fs::remove_dir_all(root).expect("remove test project");
    }

    #[test]
    fn detects_node_manifest_with_utf8_bom() {
        let root = temp_project("specprobe-runtime-bom");
        fs::write(
            root.join("package.json"),
            "\u{feff}{\"scripts\":{\"start\":\"node server.js\"}}",
        )
        .expect("write package manifest");

        let plan = detect_launch_command(&root).expect("command should be detected");

        assert_eq!(plan.adapter, ProjectAdapterKind::Node);
        assert_eq!(plan.command.args, vec!["run", "start"]);
        fs::remove_dir_all(root).expect("remove test project");
    }

    #[tokio::test]
    async fn dry_run_reports_detected_command_without_execution() {
        let root = temp_project("specprobe-runtime-dry");
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
        )
        .expect("write cargo manifest");

        let report = launch_project(&root, 1, true)
            .await
            .expect("dry run should succeed");

        assert_eq!(report.adapter, ProjectAdapterKind::Rust);
        assert!(!report.execution.attempted);
        assert!(report.execution.dry_run);
        fs::remove_dir_all(root).expect("remove test project");
    }

    #[tokio::test]
    async fn run_command_captures_stdout() {
        let root = temp_project("specprobe-runtime-run");
        let command = if cfg!(windows) {
            LaunchCommand {
                program: "cmd".to_owned(),
                args: vec![
                    "/d".to_owned(),
                    "/c".to_owned(),
                    "echo specprobe-runtime".to_owned(),
                ],
                working_directory: root.display().to_string(),
                source: "test".to_owned(),
                confidence: CommandConfidence::High,
            }
        } else {
            LaunchCommand {
                program: "sh".to_owned(),
                args: vec!["-c".to_owned(), "echo specprobe-runtime".to_owned()],
                working_directory: root.display().to_string(),
                source: "test".to_owned(),
                confidence: CommandConfidence::High,
            }
        };

        let run = run_command(&command, 3).await.expect("command should run");

        assert!(run.execution.success);
        assert!(run.stdout_excerpt.contains("specprobe-runtime"));
        fs::remove_dir_all(root).expect("remove test project");
    }

    // 下面两个超时测试直接把系统临时目录当工作目录:被杀的 cmd/sh 可能留下
    // 孤儿子进程占用工作目录(进程树 kill 缺陷,ROADMAP 1.6),专属目录会删不掉。
    #[tokio::test]
    async fn long_running_process_with_output_is_not_a_failure() {
        // Linux 上必须用外部 /bin/echo:dash 内建 echo 对文件是块缓冲,
        // SIGKILL 终止时缓冲未落盘,stdout 采集为空,long_running 判定不成立。
        let command = if cfg!(windows) {
            timeout_probe_command("echo specprobe-server & ping -n 30 127.0.0.1 > nul")
        } else {
            timeout_probe_command("/bin/echo specprobe-server; sleep 30")
        };

        let run = run_command(&command, 1).await.expect("command should run");

        assert!(run.execution.timed_out);
        assert!(run.execution.long_running);
        assert!(run.execution.success);
        assert!(run.stdout_excerpt.contains("specprobe-server"));
    }

    #[tokio::test]
    async fn silent_timeout_stays_a_failure() {
        let command = if cfg!(windows) {
            timeout_probe_command("ping -n 30 127.0.0.1 > nul")
        } else {
            timeout_probe_command("sleep 30")
        };

        let run = run_command(&command, 1).await.expect("command should run");

        assert!(run.execution.timed_out);
        assert!(!run.execution.long_running);
        assert!(!run.execution.success);
    }

    fn timeout_probe_command(script: &str) -> LaunchCommand {
        let (program, mut args) = if cfg!(windows) {
            ("cmd".to_owned(), vec!["/d".to_owned(), "/c".to_owned()])
        } else {
            ("sh".to_owned(), vec!["-c".to_owned()])
        };
        args.push(script.to_owned());

        LaunchCommand {
            program,
            args,
            working_directory: std::env::temp_dir().display().to_string(),
            source: "test".to_owned(),
            confidence: CommandConfidence::High,
        }
    }

    #[test]
    fn redacts_sensitive_lines() {
        let redacted = redact_sensitive_lines("ok\nOPENAI_API_KEY=secret\npassword=abc");

        assert_eq!(redacted, "ok\n[REDACTED LINE]\n[REDACTED LINE]");
    }

    fn long_running_command() -> LaunchCommand {
        let (program, mut args) = if cfg!(windows) {
            ("cmd".to_owned(), vec!["/d".to_owned(), "/c".to_owned()])
        } else {
            ("sh".to_owned(), vec!["-c".to_owned()])
        };
        args.push(if cfg!(windows) {
            "ping -n 30 127.0.0.1 > nul".to_owned()
        } else {
            "sleep 30".to_owned()
        });
        LaunchCommand {
            program,
            args,
            working_directory: std::env::temp_dir().display().to_string(),
            source: "test".to_owned(),
            confidence: CommandConfidence::High,
        }
    }

    #[tokio::test]
    async fn managed_app_probes_ready_server_and_shuts_down() {
        let (url, handle) = spawn_chat_server(vec![(200, chat_response("{}"))]);
        let mut app = spawn_managed(
            "test".to_owned(),
            ProjectAdapterKind::Unknown,
            long_running_command(),
        )
        .expect("service starts");

        let readiness = app.wait_until_ready(Some(&url), 3).await;
        assert!(readiness.probed);
        assert!(readiness.ready);

        let report = app.shutdown().await;
        handle.join().expect("server thread joins");
        assert!(report.execution.success);
        assert!(report.readiness.is_some_and(|readiness| readiness.ready));
    }

    #[tokio::test]
    async fn managed_app_reports_unreachable_server() {
        let mut app = spawn_managed(
            "test".to_owned(),
            ProjectAdapterKind::Unknown,
            long_running_command(),
        )
        .expect("service starts");

        // 无人监听的端口:探测在超时后判定未就绪。
        let readiness = app.wait_until_ready(Some("http://127.0.0.1:1"), 1).await;
        assert!(readiness.probed);
        assert!(!readiness.ready);

        let report = app.shutdown().await;
        assert!(!report.execution.success);
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
