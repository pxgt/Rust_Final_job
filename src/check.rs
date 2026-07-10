//! 一键检查命令(ROADMAP 2.1):scan → 需求精解析 → 编排执行 → 浏览器 → 诊断。
//!
//! `check` 是面向用户的主入口,把分步子命令串成单条命令;安全边界保留——
//! 执行被测项目的启动命令前需交互确认(`--yes` 跳过)。确认回调可注入,
//! 便于测试与后续非交互环境处理(非 TTY / EOF 视为拒绝,安全默认)。

use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use serde::Serialize;

use crate::ai::AiProviderKind;
use crate::review::{ReviewError, ReviewOptions, ReviewReport, generate_review_report};
use crate::runtime::launch_project;
use crate::scanner::{ProjectProfile, scan_project};

#[derive(Debug, Clone)]
pub struct CheckOptions {
    /// 被测项目目录(默认也作为需求文档搜索处)。
    pub path: PathBuf,
    /// 需求文档或目录覆盖;None 时用 `path`。
    pub requirements: Option<PathBuf>,
    pub base_url: String,
    pub provider: AiProviderKind,
    pub cache_dir: Option<PathBuf>,
    /// 跳过启动命令确认。
    pub assume_yes: bool,
    pub launch_timeout_secs: u64,
    pub browser_timeout_secs: u64,
}

#[derive(Debug, Serialize)]
pub struct CheckReport {
    pub profile: ProjectProfile,
    /// 是否真实执行了启动与浏览器(false = 用户拒绝确认,降级为计划级)。
    pub executed: bool,
    pub review: ReviewReport,
}

#[derive(Debug, thiserror::Error)]
pub enum CheckError {
    #[error(transparent)]
    Scan(#[from] crate::scanner::ScanError),
    #[error(transparent)]
    Review(#[from] ReviewError),
}

/// 交互式入口:确认走真实终端(stderr 提问 + stdin 读答复)。
pub async fn run_check(options: CheckOptions) -> Result<CheckReport, CheckError> {
    run_check_with(options, prompt_on_terminal).await
}

/// 可注入确认回调的实现,便于测试。回调收到"将要执行的启动命令"描述,
/// 返回 true 表示允许执行。
pub async fn run_check_with(
    options: CheckOptions,
    confirm: impl Fn(&str) -> bool,
) -> Result<CheckReport, CheckError> {
    let profile = scan_project(&options.path)?;
    let executed = should_execute(&options, &confirm).await;

    let requirements_path = options
        .requirements
        .clone()
        .unwrap_or_else(|| options.path.clone());
    let review = generate_review_report(
        &requirements_path,
        ReviewOptions {
            project_path: options.path.clone(),
            base_url: options.base_url.clone(),
            provider: options.provider,
            cache_dir: options.cache_dir.clone(),
            execute: executed,
            skip_launch: false,
            skip_browser: false,
            launch_timeout_secs: options.launch_timeout_secs,
            browser_timeout_secs: options.browser_timeout_secs,
        },
    )
    .await?;

    Ok(CheckReport {
        profile,
        executed,
        review,
    })
}

/// 安全边界决策:`--yes` 直接执行;探测到启动命令(dry-run,不执行)则先确认;
/// 未检测到命令时没有可执行的项目代码,无需确认(浏览器仍可探测已运行的服务)。
async fn should_execute(options: &CheckOptions, confirm: &impl Fn(&str) -> bool) -> bool {
    if options.assume_yes {
        return true;
    }
    match launch_project(&options.path, options.launch_timeout_secs, true).await {
        Ok(report) => {
            let command_line = format!(
                "{} {}",
                report.command.program,
                report.command.args.join(" ")
            );
            confirm(&format!(
                "About to execute the project's launch command: `{}` (from {}).",
                command_line.trim(),
                report.command.source
            ))
        }
        Err(_) => true,
    }
}

/// 真实终端确认:stderr 提问,stdin 读一行;非 TTY / EOF / 非 y 一律拒绝。
fn prompt_on_terminal(message: &str) -> bool {
    eprint!("{message} Continue? [y/N] ");
    let _ = io::stderr().flush();
    let mut line = String::new();
    if io::stdin().lock().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{CheckOptions, run_check_with};
    use crate::testutil::temp_project;

    fn options(path: std::path::PathBuf, assume_yes: bool) -> CheckOptions {
        CheckOptions {
            path,
            requirements: None,
            base_url: "http://127.0.0.1:3000".to_owned(),
            provider: Default::default(),
            cache_dir: None,
            assume_yes,
            launch_timeout_secs: 1,
            browser_timeout_secs: 1,
        }
    }

    fn write_demo_project(root: &std::path::Path) {
        fs::write(
            root.join("package.json"),
            r#"{"scripts":{"dev":"node server.js"}}"#,
        )
        .expect("write package.json");
        fs::write(root.join("PRD.md"), "- 页面必须显示任务列表。").expect("write prd");
    }

    #[tokio::test]
    async fn declined_confirmation_falls_back_to_plan_only() {
        let root = temp_project("specprobe-check-decline");
        write_demo_project(&root);

        let report = run_check_with(options(root.clone(), false), |_| false)
            .await
            .expect("check succeeds");

        assert!(!report.executed);
        assert!(!report.review.config.execute);
        // 计划级仍产出启动命令识别(dry-run)与需求。
        assert!(report.review.launch_report.is_some());
        assert!(report.review.summary.requirements >= 1);
        assert!(
            report
                .profile
                .technologies
                .iter()
                .any(|t| t.contains("Node"))
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[tokio::test]
    async fn confirmation_prompt_receives_launch_command() {
        let root = temp_project("specprobe-check-prompt");
        write_demo_project(&root);
        let seen = std::sync::Mutex::new(String::new());

        let _ = run_check_with(options(root.clone(), false), |message| {
            *seen.lock().expect("lock") = message.to_owned();
            false
        })
        .await
        .expect("check succeeds");

        let message = seen.lock().expect("lock").clone();
        assert!(message.contains("npm run dev"), "got: {message}");
        assert!(message.contains("package.json"));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[tokio::test]
    async fn no_launch_command_skips_confirmation_and_allows_execution() {
        let root = temp_project("specprobe-check-nocmd");
        fs::write(root.join("PRD.md"), "- 页面必须显示任务列表。").expect("write prd");

        // 确认回调若被调用即 panic:无启动命令时不应询问,且默认允许执行。
        let confirm = |_: &str| -> bool { panic!("confirm should not be called") };
        let executed = super::should_execute(&options(root.clone(), false), &confirm).await;

        assert!(executed);
        fs::remove_dir_all(root).expect("cleanup");
    }
}
