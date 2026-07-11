//! LLM 补丁生成(ROADMAP 3.1)。
//!
//! 对一个已定位的缺陷:读取诊断指向的源码文件全文,交给 LLM 生成修复的
//! unified diff。硬约束:只允许修改提供的文件,且 diff 必须能 `git apply --check`
//! 通过——两项校验都放进 `run_chat_json` 的 parse 回路,不通过则带反馈自动重问。
//! 本模块只生成并校验补丁,不应用(应用到分支属 3.2)。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;
use serde_json::{Value, json};
use thiserror::Error;

use crate::ai::{
    AiCache, AiError, AiProviderKind, ChatProtocol, chat_protocol_for, run_chat_json,
    strip_code_fence,
};

/// 补丁生成的输入:缺陷描述 + 候选源文件(相对项目根的路径)。
pub struct PatchInput {
    pub issue_id: String,
    pub title: String,
    pub expected: String,
    pub actual: String,
    pub source_files: Vec<String>,
}

/// 生成并经校验的补丁。`diff` 已通过 `git apply --check`。
#[derive(Debug, Clone, Serialize)]
pub struct GeneratedPatch {
    pub issue_id: String,
    pub target_files: Vec<String>,
    pub diff: String,
}

#[derive(Debug, Error)]
pub enum PatchError {
    #[error(transparent)]
    Ai(#[from] AiError),
    #[error("patch generation requires a real AI provider (not mock)")]
    NoProvider,
    #[error("no readable source files were available for the fix")]
    NoSourceFiles,
}

/// 生成补丁。Mock Provider 直接报错(补丁生成需要真实模型)。
pub async fn generate_patch(
    input: &PatchInput,
    project_path: &Path,
    provider: AiProviderKind,
    cache_dir: Option<PathBuf>,
) -> Result<GeneratedPatch, PatchError> {
    let Some(protocol) = chat_protocol_for(provider)? else {
        return Err(PatchError::NoProvider);
    };
    let cache = cache_dir.map(|dir| AiCache { dir });
    generate_patch_with_protocol(input, project_path, &protocol, cache.as_ref()).await
}

/// 核心流程:读源文件 → 组织消息 → `run_chat_json`(parse 里做范围与 git 校验)。
async fn generate_patch_with_protocol(
    input: &PatchInput,
    project_path: &Path,
    protocol: &ChatProtocol,
    cache: Option<&AiCache>,
) -> Result<GeneratedPatch, PatchError> {
    // 读取候选源文件全文(读不到的跳过)。
    let mut files = Vec::new();
    for relative in &input.source_files {
        if let Ok(contents) = std::fs::read_to_string(project_path.join(relative)) {
            files.push((relative.clone(), contents));
        }
    }
    if files.is_empty() {
        return Err(PatchError::NoSourceFiles);
    }
    let known: Vec<String> = files.iter().map(|(path, _)| path.clone()).collect();

    let messages = build_messages(input, &files);
    let project = project_path.to_path_buf();
    let (patch, _transport) = run_chat_json(protocol, messages, cache, move |content, _lenient| {
        // 补丁必须精确,不设宽容模式。
        parse_patch(content, &known, &project)
    })
    .await?;

    Ok(GeneratedPatch {
        issue_id: input.issue_id.clone(),
        target_files: patch.target_files,
        diff: patch.diff,
    })
}

struct ParsedPatch {
    diff: String,
    target_files: Vec<String>,
}

/// 校验 LLM 输出:是 unified diff、只改提供的文件、能 `git apply --check`。
fn parse_patch(content: &str, known: &[String], project: &Path) -> Result<ParsedPatch, String> {
    // 保证补丁以恰好一个换行结尾:git apply 会把无末尾换行的补丁判为 "corrupt patch"。
    let cleaned = strip_code_fence(content)
        .trim_start()
        .trim_end_matches(['\n', '\r']);
    let diff = format!("{cleaned}\n");
    let target_files = validate_diff_shape(&diff, known)?;
    git_apply_check(project, &diff)
        .map_err(|error| format!("the diff does not apply cleanly (git apply --check: {error})"))?;
    Ok(ParsedPatch { diff, target_files })
}

/// 校验 diff 格式并返回被修改的文件;不合法或越界返回反馈文本。
fn validate_diff_shape(diff: &str, known: &[String]) -> Result<Vec<String>, String> {
    if !diff.contains("--- ") || !diff.contains("+++ ") {
        return Err("output is not a unified diff (missing '---' / '+++' headers)".to_owned());
    }
    let mut files = Vec::new();
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("+++ ") {
            let head = rest.split('\t').next().unwrap_or(rest).trim();
            let path = head.strip_prefix("b/").unwrap_or(head).to_owned();
            if path != "/dev/null" && !files.contains(&path) {
                files.push(path);
            }
        }
    }
    if files.is_empty() {
        return Err("the diff does not modify any file".to_owned());
    }
    for file in &files {
        if !known.iter().any(|candidate| candidate == file) {
            return Err(format!(
                "the diff modifies '{file}', which is not one of the provided source files"
            ));
        }
    }
    Ok(files)
}

/// 在项目目录运行 `git apply --check`(经 stdin 传入 diff)。不需要 git 仓库。
fn git_apply_check(project: &Path, diff: &str) -> Result<(), String> {
    let mut child = Command::new("git")
        .current_dir(project)
        .args(["apply", "--check", "--recount", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to run git: {error}"))?;
    child
        .stdin
        .take()
        .ok_or("git stdin unavailable")?
        .write_all(diff.as_bytes())
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

fn build_messages(input: &PatchInput, files: &[(String, String)]) -> Vec<Value> {
    vec![
        json!({"role": "system", "content": system_prompt()}),
        json!({"role": "user", "content": user_prompt(input, files)}),
    ]
}

fn system_prompt() -> String {
    r#"You are a code-fixing engine. Given a defect and the current source files, output a single
unified diff that fixes the defect.

Rules:
- Output ONLY the unified diff, applicable with `git apply`. Use `a/<path>` and `b/<path>` headers
  where <path> is exactly one of the provided relative file paths.
- Modify only the provided files. Do not create or rename files.
- Keep the change minimal — fix the described defect and nothing else.
- Include enough surrounding context lines for the hunks to apply.
- Do not add explanations or markdown fences; emit the diff text only."#
        .to_owned()
}

fn user_prompt(input: &PatchInput, files: &[(String, String)]) -> String {
    let mut prompt = format!(
        "Defect {}: {}\nExpected: {}\nActual: {}\n\nSource files:\n",
        input.issue_id, input.title, input.expected, input.actual
    );
    for (path, contents) in files {
        prompt.push_str(&format!("\n--- FILE: {path} ---\n{contents}\n"));
    }
    prompt
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;

    use super::{
        AiProviderKind, PatchError, PatchInput, generate_patch, generate_patch_with_protocol,
        validate_diff_shape,
    };
    use crate::ai::test_openai_protocol;
    use crate::testutil::{chat_response, spawn_chat_server, temp_project};

    fn git_available() -> bool {
        Command::new("git").arg("--version").output().is_ok()
    }

    #[test]
    fn validate_diff_shape_checks_headers_and_scope() {
        let known = vec!["app.js".to_owned()];
        assert!(validate_diff_shape("not a diff", &known).is_err());

        let ok = "--- a/app.js\n+++ b/app.js\n@@ -1 +1 @@\n-old\n+new\n";
        assert_eq!(validate_diff_shape(ok, &known).unwrap(), vec!["app.js"]);

        let outside = "--- a/other.js\n+++ b/other.js\n@@ -1 +1 @@\n-old\n+new\n";
        assert!(
            validate_diff_shape(outside, &known)
                .unwrap_err()
                .contains("other.js")
        );
    }

    #[tokio::test]
    async fn mock_provider_is_rejected() {
        let root = temp_project("specprobe-patch-mock");
        let input = PatchInput {
            issue_id: "ISSUE-001".to_owned(),
            title: "t".to_owned(),
            expected: "e".to_owned(),
            actual: "a".to_owned(),
            source_files: vec!["app.js".to_owned()],
        };
        let result = generate_patch(&input, &root, AiProviderKind::Mock, None).await;
        assert!(matches!(result, Err(PatchError::NoProvider)));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[tokio::test]
    async fn missing_source_files_are_reported() {
        let root = temp_project("specprobe-patch-nofiles");
        let (base_url, handle) = spawn_chat_server(vec![]);
        let input = PatchInput {
            issue_id: "ISSUE-001".to_owned(),
            title: "t".to_owned(),
            expected: "e".to_owned(),
            actual: "a".to_owned(),
            source_files: vec!["does-not-exist.js".to_owned()],
        };
        let result =
            generate_patch_with_protocol(&input, &root, &test_openai_protocol(base_url), None)
                .await;
        assert!(matches!(result, Err(PatchError::NoSourceFiles)));
        handle.join().expect("server thread joins");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[tokio::test]
    async fn generates_patch_that_applies_cleanly() {
        if !git_available() {
            return;
        }
        let root = temp_project("specprobe-patch");
        fs::write(root.join("app.js"), "const status = 500;\n").expect("write source");

        let diff =
            "--- a/app.js\n+++ b/app.js\n@@ -1 +1 @@\n-const status = 500;\n+const status = 200;\n";
        let (base_url, handle) = spawn_chat_server(vec![(200, chat_response(diff))]);
        let input = PatchInput {
            issue_id: "ISSUE-001".to_owned(),
            title: "API returns 500".to_owned(),
            expected: "200".to_owned(),
            actual: "500".to_owned(),
            source_files: vec!["app.js".to_owned()],
        };

        let patch =
            generate_patch_with_protocol(&input, &root, &test_openai_protocol(base_url), None)
                .await
                .expect("patch generated");

        let requests = handle.join().expect("server thread joins");
        assert_eq!(requests.len(), 1);
        assert_eq!(patch.target_files, vec!["app.js".to_owned()]);
        assert!(patch.diff.contains("+const status = 200;"));
        fs::remove_dir_all(root).expect("cleanup");
    }
}
