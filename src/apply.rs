//! 补丁安全应用(ROADMAP 3.2)。
//!
//! 把已校验的补丁应用到被测项目的隔离分支 `specprobe/fix-<issue>`,在该分支
//! 提交后切回用户原分支——绝不改动用户当前分支的工作区。前置:被测项目是
//! 一个 git 仓库,且工作区干净(否则需 `--allow-dirty` 显式豁免)。
//!
//! 任何一步失败都会回滚:切回原分支并删除刚创建的分支,不给用户留下半成品状态。

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde::Serialize;
use thiserror::Error;

use crate::patch::GeneratedPatch;

pub struct ApplyOptions {
    /// 允许在工作区不干净时应用(默认拒绝)。
    pub allow_dirty: bool,
}

/// 应用结果:补丁提交在 `branch`,应用后已切回 `restored_branch`。
#[derive(Debug, Clone, Serialize)]
pub struct ApplyOutcome {
    pub branch: String,
    pub restored_branch: String,
    pub commit: String,
    pub files: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ApplyError {
    #[error("the project at {0} is not a git repository; patch application needs one")]
    NotGitRepo(String),
    #[error("the git working tree is not clean:\n{0}\n(apply with --allow-dirty to override)")]
    DirtyWorktree(String),
    #[error("branch {0} already exists; delete it or resolve the previous fix first")]
    BranchExists(String),
    #[error("git {step} failed: {detail}")]
    Git { step: &'static str, detail: String },
}

/// 把补丁应用到隔离分支并提交,随后切回原分支。
pub fn apply_patch(
    project: &Path,
    patch: &GeneratedPatch,
    options: &ApplyOptions,
) -> Result<ApplyOutcome, ApplyError> {
    // 1. 必须是 git 仓库。
    run_git(project, &["rev-parse", "--is-inside-work-tree"])
        .map_err(|_| ApplyError::NotGitRepo(project.display().to_string()))?;

    // 2. 记录当前分支(分离头指针时记录 sha 以便还原)。
    let head = git_step(
        project,
        "rev-parse HEAD name",
        &["rev-parse", "--abbrev-ref", "HEAD"],
    )?;
    let head = head.trim().to_owned();
    let restore_ref = if head == "HEAD" {
        git_step(project, "rev-parse HEAD", &["rev-parse", "HEAD"])?
            .trim()
            .to_owned()
    } else {
        head.clone()
    };

    // 3. 工作区干净检查(除非显式豁免)。
    if !options.allow_dirty {
        let status = git_step(project, "status", &["status", "--porcelain"])?;
        if !status.trim().is_empty() {
            return Err(ApplyError::DirtyWorktree(status.trim().to_owned()));
        }
    }

    // 4. 分支名;已存在则拒绝(避免覆盖之前的修复)。
    let branch = format!("specprobe/fix-{}", patch.issue_id.to_ascii_lowercase());
    if run_git(
        project,
        &["rev-parse", "--verify", &format!("refs/heads/{branch}")],
    )
    .is_ok()
    {
        return Err(ApplyError::BranchExists(branch));
    }

    // 5. 创建并切换到隔离分支。
    git_step(project, "checkout -b", &["checkout", "-b", &branch])?;

    // 6. 应用 + 提交;失败则回滚(切回原分支并删除新分支)。
    match apply_and_commit(project, patch) {
        Ok(commit) => {
            git_step(project, "checkout restore", &["checkout", &restore_ref])?;
            Ok(ApplyOutcome {
                branch,
                restored_branch: head,
                commit,
                files: patch.target_files.clone(),
            })
        }
        Err(error) => {
            let _ = run_git(project, &["checkout", "--force", &restore_ref]);
            let _ = run_git(project, &["branch", "-D", &branch]);
            Err(error)
        }
    }
}

fn apply_and_commit(project: &Path, patch: &GeneratedPatch) -> Result<String, ApplyError> {
    run_git_stdin(
        project,
        &["apply", "--recount", "--index", "-"],
        &patch.diff,
    )
    .map_err(|detail| ApplyError::Git {
        step: "apply",
        detail,
    })?;
    let message = format!("fix({}): apply specprobe-generated patch", patch.issue_id);
    git_step(project, "commit", &["commit", "-m", &message])?;
    let commit = git_step(
        project,
        "rev-parse short HEAD",
        &["rev-parse", "--short", "HEAD"],
    )?;
    Ok(commit.trim().to_owned())
}

fn git_step(project: &Path, step: &'static str, args: &[&str]) -> Result<String, ApplyError> {
    run_git(project, args).map_err(|detail| ApplyError::Git { step, detail })
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

fn run_git_stdin(project: &Path, args: &[&str], input: &str) -> Result<(), String> {
    let mut child = Command::new("git")
        .current_dir(project)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    child
        .stdin
        .take()
        .ok_or("git stdin unavailable")?
        .write_all(input.as_bytes())
        .map_err(|error| error.to_string())?;
    let output = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    use super::{ApplyError, ApplyOptions, apply_patch};
    use crate::patch::GeneratedPatch;
    use crate::testutil::temp_project;

    fn git_available() -> bool {
        Command::new("git").arg("--version").output().is_ok()
    }

    fn git(root: &Path, args: &[&str]) -> String {
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
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn init_repo(root: &Path) {
        git(root, &["init", "-q"]);
        git(root, &["config", "user.email", "test@example.com"]);
        git(root, &["config", "user.name", "Test"]);
        git(root, &["config", "commit.gpgsign", "false"]);
        fs::write(root.join("app.js"), "const status = 500;\n").expect("write source");
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", "init"]);
    }

    fn current_branch(root: &Path) -> String {
        git(root, &["rev-parse", "--abbrev-ref", "HEAD"])
            .trim()
            .to_owned()
    }

    fn fix_patch() -> GeneratedPatch {
        GeneratedPatch {
            issue_id: "ISSUE-001".to_owned(),
            target_files: vec!["app.js".to_owned()],
            diff: "--- a/app.js\n+++ b/app.js\n@@ -1 +1 @@\n-const status = 500;\n+const status = 200;\n"
                .to_owned(),
        }
    }

    #[test]
    fn applies_patch_to_isolated_branch_and_restores_original() {
        if !git_available() {
            return;
        }
        let root = temp_project("specprobe-apply-ok");
        init_repo(&root);
        let original = current_branch(&root);

        let outcome = apply_patch(&root, &fix_patch(), &ApplyOptions { allow_dirty: false })
            .expect("apply succeeds");

        assert_eq!(outcome.branch, "specprobe/fix-issue-001");
        assert_eq!(outcome.restored_branch, original);
        // 已切回原分支,原分支文件与工作区都不受影响。
        assert_eq!(current_branch(&root), original);
        assert!(
            fs::read_to_string(root.join("app.js"))
                .unwrap()
                .contains("500")
        );
        assert!(git(&root, &["status", "--porcelain"]).trim().is_empty());
        // 修复提交在隔离分支上。
        let on_branch = git(&root, &["show", "specprobe/fix-issue-001:app.js"]);
        assert!(on_branch.contains("200"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn refuses_dirty_worktree_without_override() {
        if !git_available() {
            return;
        }
        let root = temp_project("specprobe-apply-dirty");
        init_repo(&root);
        fs::write(root.join("app.js"), "const status = 500; // edited\n").expect("dirty edit");

        let error = apply_patch(&root, &fix_patch(), &ApplyOptions { allow_dirty: false })
            .expect_err("dirty worktree is refused");
        assert!(matches!(error, ApplyError::DirtyWorktree(_)));
        // 分支未创建。
        assert!(
            Command::new("git")
                .current_dir(&root)
                .args([
                    "rev-parse",
                    "--verify",
                    "refs/heads/specprobe/fix-issue-001"
                ])
                .output()
                .map(|o| !o.status.success())
                .unwrap_or(true)
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_non_git_project() {
        let root = temp_project("specprobe-apply-nogit");
        let error = apply_patch(&root, &fix_patch(), &ApplyOptions { allow_dirty: false })
            .expect_err("non-git project is rejected");
        assert!(matches!(error, ApplyError::NotGitRepo(_)));
        let _ = fs::remove_dir_all(&root);
    }
}
