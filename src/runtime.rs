use std::env;
use std::fmt;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use thiserror::Error;

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
    pub stdout_excerpt: String,
    pub stderr_excerpt: String,
    pub diagnostics: Vec<LaunchDiagnostic>,
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

pub fn launch_project(
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
    let run = run_command(&command, timeout_secs)?;
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
        stdout_excerpt: run.stdout_excerpt,
        stderr_excerpt: run.stderr_excerpt,
        diagnostics,
    })
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

fn run_command(command: &LaunchCommand, timeout_secs: u64) -> Result<CommandRun, RuntimeError> {
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
        .stderr(Stdio::from(stderr_file));

    let start = Instant::now();
    let mut child = process.spawn().map_err(|source| RuntimeError::Io {
        path: working_directory.clone(),
        source,
    })?;
    let timeout = Duration::from_secs(timeout_secs.max(1));
    let mut timed_out = false;
    let exit_status = loop {
        if let Some(status) = child.try_wait().map_err(|source| RuntimeError::Io {
            path: working_directory.clone(),
            source,
        })? {
            break status;
        }

        if start.elapsed() >= timeout {
            timed_out = true;
            child.kill().map_err(|source| RuntimeError::Io {
                path: working_directory.clone(),
                source,
            })?;
            break child.wait().map_err(|source| RuntimeError::Io {
                path: working_directory.clone(),
                source,
            })?;
        }

        thread::sleep(Duration::from_millis(50));
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

    if is_windows_script {
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
    }
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

fn unique_suffix() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    format!("{millis}-{}", std::process::id())
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
        launch_project, redact_sensitive_lines, run_command,
    };

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

    #[test]
    fn dry_run_reports_detected_command_without_execution() {
        let root = temp_project("specprobe-runtime-dry");
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
        )
        .expect("write cargo manifest");

        let report = launch_project(&root, 1, true).expect("dry run should succeed");

        assert_eq!(report.adapter, ProjectAdapterKind::Rust);
        assert!(!report.execution.attempted);
        assert!(report.execution.dry_run);
        fs::remove_dir_all(root).expect("remove test project");
    }

    #[test]
    fn run_command_captures_stdout() {
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

        let run = run_command(&command, 3).expect("command should run");

        assert!(run.execution.success);
        assert!(run.stdout_excerpt.contains("specprobe-runtime"));
        fs::remove_dir_all(root).expect("remove test project");
    }

    // 下面两个超时测试直接把系统临时目录当工作目录:被杀的 cmd/sh 可能留下
    // 孤儿子进程占用工作目录(进程树 kill 缺陷,ROADMAP 1.6),专属目录会删不掉。
    #[test]
    fn long_running_process_with_output_is_not_a_failure() {
        // Linux 上必须用外部 /bin/echo:dash 内建 echo 对文件是块缓冲,
        // SIGKILL 终止时缓冲未落盘,stdout 采集为空,long_running 判定不成立。
        let command = if cfg!(windows) {
            timeout_probe_command("echo specprobe-server & ping -n 30 127.0.0.1 > nul")
        } else {
            timeout_probe_command("/bin/echo specprobe-server; sleep 30")
        };

        let run = run_command(&command, 1).expect("command should run");

        assert!(run.execution.timed_out);
        assert!(run.execution.long_running);
        assert!(run.execution.success);
        assert!(run.stdout_excerpt.contains("specprobe-server"));
    }

    #[test]
    fn silent_timeout_stays_a_failure() {
        let command = if cfg!(windows) {
            timeout_probe_command("ping -n 30 127.0.0.1 > nul")
        } else {
            timeout_probe_command("sleep 30")
        };

        let run = run_command(&command, 1).expect("command should run");

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
