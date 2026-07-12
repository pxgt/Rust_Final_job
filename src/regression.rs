//! 修复回归验证(ROADMAP 3.3)。
//!
//! 补丁应用到隔离分支后,用 `git worktree` 把该分支物化到临时目录,在其上重跑
//! 评审,按 Issue 指纹对比修复前后:目标缺陷消失且无新增问题 → 验证通过。
//! worktree 用完即删,绝不改动用户工作区。裁决为"未通过"时由调用方回滚分支。
//!
//! 注意:验证代码级缺陷需要在 worktree 里真实启动被测应用(execute=true),这依赖
//! 项目自身的运行环境(如 Node + 依赖)。纯逻辑裁决(`evaluate`)与编排(worktree
//! 物化 + 重跑 + 指纹对比 + 清理)与运行环境无关,可独立测试。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use thiserror::Error;

use crate::ai::AiProviderKind;
use crate::review::{ReviewError, ReviewOptions, generate_review_report_with};
use crate::storage::issue_fingerprint;
use crate::ui::noop_progress;

/// 修复前后按指纹对比的裁决。
#[derive(Debug, Clone, Serialize)]
pub struct RegressionVerdict {
    pub verified: bool,
    pub target_resolved: bool,
    /// baseline 中已消失的指纹(含目标)。
    pub resolved: Vec<String>,
    /// baseline 中修复后仍在的指纹。
    pub remaining: Vec<String>,
    /// 修复后新出现、baseline 没有的指纹(视为回归)。
    pub new_issues: Vec<String>,
}

/// 纯裁决:目标指纹从 post_fix 消失,且没有新增指纹 → verified。
pub fn evaluate(baseline: &[String], post_fix: &[String], target: &str) -> RegressionVerdict {
    let post: HashSet<&str> = post_fix.iter().map(String::as_str).collect();
    let base: HashSet<&str> = baseline.iter().map(String::as_str).collect();

    let resolved = baseline
        .iter()
        .filter(|fp| !post.contains(fp.as_str()))
        .cloned()
        .collect();
    let remaining = baseline
        .iter()
        .filter(|fp| post.contains(fp.as_str()))
        .cloned()
        .collect();
    let new_issues: Vec<String> = post_fix
        .iter()
        .filter(|fp| !base.contains(fp.as_str()))
        .cloned()
        .collect();

    let target_resolved = !post.contains(target);
    let verified = target_resolved && new_issues.is_empty();
    RegressionVerdict {
        verified,
        target_resolved,
        resolved,
        remaining,
        new_issues,
    }
}

/// 在 worktree 上重跑评审所需的参数(取自 baseline run 的配置)。
pub struct VerifyOptions {
    pub requirements_source: PathBuf,
    pub base_url: String,
    pub provider: AiProviderKind,
    pub cache_dir: Option<PathBuf>,
    pub execute: bool,
    pub skip_launch: bool,
    pub skip_browser: bool,
    pub launch_timeout_secs: u64,
    pub browser_timeout_secs: u64,
}

#[derive(Debug, Error)]
pub enum RegressionError {
    #[error("git worktree {step} failed: {detail}")]
    Worktree { step: &'static str, detail: String },
    #[error(transparent)]
    Review(#[from] ReviewError),
}

/// 在隔离分支上验证修复:worktree 物化 → 重跑评审 → 指纹裁决。无论成败都清理 worktree。
pub async fn verify_on_branch(
    project: &Path,
    branch: &str,
    baseline: &[String],
    target: &str,
    options: &VerifyOptions,
) -> Result<RegressionVerdict, RegressionError> {
    let worktree = worktree_dir(branch);
    let worktree_str = worktree.to_string_lossy().into_owned();
    // 用 --detach 物化分支的提交(分支不能同时被两个 worktree 检出)。
    run_git(
        project,
        &["worktree", "add", "--detach", &worktree_str, branch],
    )
    .map_err(|detail| RegressionError::Worktree {
        step: "add",
        detail,
    })?;

    let outcome = run_review_and_evaluate(project, &worktree, baseline, target, options).await;

    // 清理:先注销 worktree,再兜底删目录。
    let _ = run_git(project, &["worktree", "remove", "--force", &worktree_str]);
    let _ = std::fs::remove_dir_all(&worktree);

    outcome
}

async fn run_review_and_evaluate(
    project: &Path,
    worktree: &Path,
    baseline: &[String],
    target: &str,
    options: &VerifyOptions,
) -> Result<RegressionVerdict, RegressionError> {
    // 需求文档若在项目内,则重定位到 worktree 的对应副本(修复可能改了需求文档);
    // 若在项目外则原样使用(外部需求不受修复影响)。
    let requirements = match relative_within(project, &options.requirements_source) {
        Some(rel) => worktree.join(rel),
        None => options.requirements_source.clone(),
    };
    let report = generate_review_report_with(
        &requirements,
        ReviewOptions {
            project_path: worktree.to_path_buf(),
            base_url: options.base_url.clone(),
            provider: options.provider,
            cache_dir: options.cache_dir.clone(),
            execute: options.execute,
            skip_launch: options.skip_launch,
            skip_browser: options.skip_browser,
            launch_timeout_secs: options.launch_timeout_secs,
            browser_timeout_secs: options.browser_timeout_secs,
            // 回归验证保持单次采样:验证求确定性,不求检出并集。
            samples: 0,
        },
        &noop_progress,
    )
    .await?;

    let post: Vec<String> = report
        .issues
        .iter()
        .map(|issue| {
            issue_fingerprint(
                &issue.category.to_string(),
                issue.related_requirement.as_deref(),
                &issue.title,
            )
        })
        .collect();
    Ok(evaluate(baseline, &post, target))
}

/// 若 `child` 在 `base` 之内,返回其相对部分(按正斜杠归一,容忍混用分隔符);
/// 否则 None。归档的 project_root 与 requirements_source 可能一个用 `/` 一个用 `\`。
fn relative_within(base: &Path, child: &Path) -> Option<String> {
    let base = base.to_string_lossy().replace('\\', "/");
    let child = child.to_string_lossy().replace('\\', "/");
    let base = base.trim_end_matches('/');
    let rest = child.strip_prefix(base)?;
    Some(rest.trim_start_matches('/').to_owned())
}

fn worktree_dir(branch: &str) -> PathBuf {
    let sanitized = branch.replace(['/', '\\'], "-");
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("specprobe-verify-{sanitized}-{nanos}"))
}

fn run_git(project: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .current_dir(project)
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    use super::{VerifyOptions, evaluate, verify_on_branch};
    use crate::ai::AiProviderKind;
    use crate::apply::{ApplyOptions, apply_patch};
    use crate::patch::GeneratedPatch;
    use crate::storage::issue_fingerprint;
    use crate::testutil::temp_project;

    fn git_available() -> bool {
        Command::new("git").arg("--version").output().is_ok()
    }

    fn git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // 目标缺陷的指纹:REQ-001 的需求质量问题。
    fn target_fingerprint() -> String {
        issue_fingerprint(
            "requirement-gap",
            Some("REQ-001"),
            "REQ-001 的验收条件不够明确",
        )
    }

    fn init_repo_with_vague_requirement(root: &Path) {
        git(root, &["init", "-q"]);
        git(root, &["config", "user.email", "test@example.com"]);
        git(root, &["config", "user.name", "Test"]);
        git(root, &["config", "commit.gpgsign", "false"]);
        fs::write(root.join("PRD.md"), "- 页面应该简单友好。\n").expect("write requirements");
        fs::write(root.join("README.md"), "# proj\n").expect("write readme");
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", "init"]);
    }

    fn verify_options(root: &Path) -> VerifyOptions {
        VerifyOptions {
            requirements_source: root.join("PRD.md"),
            base_url: "http://127.0.0.1:9".to_owned(),
            provider: AiProviderKind::Mock,
            cache_dir: None,
            execute: false,
            skip_launch: true,
            skip_browser: true,
            launch_timeout_secs: 1,
            browser_timeout_secs: 1,
        }
    }

    #[test]
    fn evaluate_verifies_when_target_gone_and_no_new_issues() {
        let base = vec!["aaa".to_owned(), "bbb".to_owned()];
        let verdict = evaluate(&base, &["bbb".to_owned()], "aaa");
        assert!(verdict.verified);
        assert!(verdict.target_resolved);
        assert_eq!(verdict.resolved, vec!["aaa".to_owned()]);
        assert_eq!(verdict.remaining, vec!["bbb".to_owned()]);
        assert!(verdict.new_issues.is_empty());
    }

    #[test]
    fn evaluate_fails_on_new_regression() {
        let base = vec!["aaa".to_owned()];
        let verdict = evaluate(&base, &["ccc".to_owned()], "aaa");
        assert!(verdict.target_resolved); // 目标没了
        assert!(!verdict.verified); // 但引入了新问题 ccc
        assert_eq!(verdict.new_issues, vec!["ccc".to_owned()]);
    }

    #[test]
    fn evaluate_fails_when_target_remains() {
        let base = vec!["aaa".to_owned()];
        let verdict = evaluate(&base, &["aaa".to_owned()], "aaa");
        assert!(!verdict.target_resolved);
        assert!(!verdict.verified);
        assert_eq!(verdict.remaining, vec!["aaa".to_owned()]);
    }

    #[tokio::test]
    async fn verifies_a_requirement_fix_on_isolated_branch() {
        if !git_available() {
            return;
        }
        let root = temp_project("specprobe-regress-ok");
        init_repo_with_vague_requirement(&root);
        let target = target_fingerprint();

        // 补丁把模糊需求改写为可验证需求 → 重跑后质量问题消失。
        let patch = GeneratedPatch {
            issue_id: "ISSUE-001".to_owned(),
            target_files: vec!["PRD.md".to_owned()],
            diff: "--- a/PRD.md\n+++ b/PRD.md\n@@ -1 +1 @@\n-- 页面应该简单友好。\n+- 用户提交表单后必须显示保存成功提示。\n".to_owned(),
        };
        let outcome = apply_patch(&root, &patch, &ApplyOptions { allow_dirty: false })
            .expect("apply succeeds");

        let verdict = verify_on_branch(
            &root,
            &outcome.branch,
            std::slice::from_ref(&target),
            &target,
            &verify_options(&root),
        )
        .await
        .expect("verification runs");

        assert!(verdict.verified, "expected verified, got {verdict:?}");
        assert!(verdict.target_resolved);
        // worktree 已清理:仓库只应有主 worktree。
        let list = Command::new("git")
            .current_dir(&root)
            .args(["worktree", "list"])
            .output()
            .expect("worktree list");
        assert_eq!(String::from_utf8_lossy(&list.stdout).lines().count(), 1);

        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn does_not_verify_when_defect_untouched() {
        if !git_available() {
            return;
        }
        let root = temp_project("specprobe-regress-fail");
        init_repo_with_vague_requirement(&root);
        let target = target_fingerprint();

        // 补丁只改了 README,模糊需求原封不动 → 目标缺陷仍在。
        let patch = GeneratedPatch {
            issue_id: "ISSUE-001".to_owned(),
            target_files: vec!["README.md".to_owned()],
            diff: "--- a/README.md\n+++ b/README.md\n@@ -1 +1 @@\n-# proj\n+# proj updated\n"
                .to_owned(),
        };
        let outcome = apply_patch(&root, &patch, &ApplyOptions { allow_dirty: false })
            .expect("apply succeeds");

        let verdict = verify_on_branch(
            &root,
            &outcome.branch,
            std::slice::from_ref(&target),
            &target,
            &verify_options(&root),
        )
        .await
        .expect("verification runs");

        assert!(!verdict.verified);
        assert!(!verdict.target_resolved);
        assert_eq!(verdict.remaining, vec![target]);

        let _ = fs::remove_dir_all(&root);
    }
}
