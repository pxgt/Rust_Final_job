//! 缺陷诊断真实化(ROADMAP 1.7)。
//!
//! 在确定性规则 Issue 之上叠加一层 LLM 深度诊断:把失败的运行发现 + 启发式检索到
//! 的相关源码片段交给模型,输出带源码定位与置信度的结构化诊断。规则 Issue 始终保留
//! (离线可用),诊断仅在启用 AI Provider 时产出。复用 1.2 的 `run_chat_json` 传输层。

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::ai::{
    AiCache, AiError, AiProviderKind, Confidence, chat_protocol_for, run_chat_json,
    strip_code_fence,
};

/// 送入诊断的失败发现(由 review 从失败 Issue 映射而来,避免反向依赖 review 类型)。
pub struct FailedFinding {
    pub issue_id: String,
    pub title: String,
    pub expected: String,
    pub actual: String,
}

/// LLM 对一组失败发现的诊断结论。
#[derive(Debug, Clone, Serialize)]
pub struct Diagnosis {
    pub title: String,
    pub related_issue_ids: Vec<String>,
    pub root_cause: String,
    pub source_locations: Vec<DiagnosedLocation>,
    pub confidence: Confidence,
    pub suggested_fix: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosedLocation {
    pub file: String,
    pub line: Option<usize>,
    pub snippet: String,
}

const MAX_SNIPPETS: usize = 12;
const SNIPPET_CONTEXT_LINES: usize = 3;
const MAX_SOURCE_FILE_BYTES: u64 = 256 * 1024;
const SOURCE_EXTENSIONS: &[&str] = &[
    "js", "mjs", "cjs", "ts", "tsx", "jsx", "vue", "svelte", "html", "css", "py", "rs", "go",
    "java", "rb", "php",
];
const SKIPPED_DIRECTORIES: &[&str] = &[
    ".git",
    ".next",
    ".venv",
    ".specprobe",
    "build",
    "coverage",
    "dist",
    "node_modules",
    "target",
    "vendor",
];

/// 对失败发现做 LLM 诊断。Mock Provider 或无失败发现时返回空(不调用模型)。
pub async fn generate_diagnoses(
    findings: &[FailedFinding],
    project_path: &Path,
    provider: AiProviderKind,
    cache_dir: Option<PathBuf>,
) -> Result<Vec<Diagnosis>, AiError> {
    if findings.is_empty() {
        return Ok(Vec::new());
    }
    let Some(protocol) = chat_protocol_for(provider)? else {
        return Ok(Vec::new());
    };
    let cache = cache_dir.map(|dir| AiCache { dir });

    let keywords = keywords_from_findings(findings);
    let snippets = fs::canonicalize(project_path)
        .ok()
        .filter(|root| root.is_dir())
        .map(|root| collect_source_context(&root, &keywords, MAX_SNIPPETS))
        .unwrap_or_default();

    let known_issue_ids: HashSet<String> = findings.iter().map(|f| f.issue_id.clone()).collect();
    let known_files: HashSet<String> = snippets.iter().map(|s| s.file.clone()).collect();
    let messages = build_messages(findings, &snippets);

    let (diagnoses, _transport) =
        run_chat_json(&protocol, messages, cache.as_ref(), |content, lenient| {
            parse_diagnoses(content, &known_issue_ids, &known_files, lenient)
        })
        .await?;
    Ok(diagnoses)
}

// ---------------------------------------------------------------------------
// 关键词提取与源码检索
// ---------------------------------------------------------------------------

fn keywords_from_findings(findings: &[FailedFinding]) -> Vec<String> {
    let mut set = BTreeSet::new();
    for finding in findings {
        extract_identifiers(&finding.actual, &mut set);
        extract_identifiers(&finding.title, &mut set);
        extract_identifiers(&finding.expected, &mut set);
    }
    set.into_iter().take(16).collect()
}

/// 从文本提取可能出现在源码中的标识符:CSS selector(#id/.class)、URL 路径段、
/// kebab/snake 标识符。用于 grep 相关源文件。
fn extract_identifiers(text: &str, out: &mut BTreeSet<String>) {
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    let is_ident = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';

    while index < chars.len() {
        let ch = chars[index];
        if ch == '#' || ch == '.' {
            // CSS selector 或路径分隔:读取后续标识符。
            let start = index + 1;
            let mut end = start;
            while end < chars.len() && is_ident(chars[end]) {
                end += 1;
            }
            if end > start {
                let token: String = chars[start..end].iter().collect();
                add_keyword(&token, out);
            }
            index = end;
        } else if ch == '/' {
            let start = index + 1;
            let mut end = start;
            while end < chars.len() && is_ident(chars[end]) {
                end += 1;
            }
            if end > start {
                let token: String = chars[start..end].iter().collect();
                add_keyword(&token, out);
            }
            index = end;
        } else {
            index += 1;
        }
    }
}

fn add_keyword(token: &str, out: &mut BTreeSet<String>) {
    let token = token.trim_matches(|c: char| c == '-' || c == '_');
    // 至少 3 字符、含字母、非纯数字,过滤 http/https/www 等噪声。
    if token.len() >= 3
        && token.chars().any(|c| c.is_ascii_alphabetic())
        && !matches!(
            token.to_ascii_lowercase().as_str(),
            "http" | "https" | "www" | "com"
        )
    {
        out.insert(token.to_owned());
    }
}

#[derive(Debug, Clone)]
struct SourceSnippet {
    file: String,
    start_line: usize,
    text: String,
}

/// 遍历项目源文件,对每个关键词截取首个命中行附近的片段(去重、限量)。
fn collect_source_context(
    root: &Path,
    keywords: &[String],
    max_snippets: usize,
) -> Vec<SourceSnippet> {
    if keywords.is_empty() {
        return Vec::new();
    }
    let mut files = Vec::new();
    collect_source_files(root, root, &mut files);

    let mut snippets = Vec::new();
    let mut seen = HashSet::new();
    for (display, path) in files {
        if snippets.len() >= max_snippets {
            break;
        }
        let Ok(contents) = fs::read_to_string(&path) else {
            continue;
        };
        let lines: Vec<&str> = contents.lines().collect();
        for keyword in keywords {
            if snippets.len() >= max_snippets {
                break;
            }
            if let Some(hit) = lines.iter().position(|line| {
                line.to_ascii_lowercase()
                    .contains(&keyword.to_ascii_lowercase())
            }) {
                let start = hit.saturating_sub(SNIPPET_CONTEXT_LINES);
                let end = (hit + SNIPPET_CONTEXT_LINES + 1).min(lines.len());
                if !seen.insert((display.clone(), start)) {
                    continue;
                }
                let text = lines[start..end]
                    .iter()
                    .enumerate()
                    .map(|(offset, line)| format!("{:>4}| {line}", start + offset + 1))
                    .collect::<Vec<_>>()
                    .join("\n");
                snippets.push(SourceSnippet {
                    file: display.clone(),
                    start_line: start + 1,
                    text,
                });
            }
        }
    }
    snippets
}

fn collect_source_files(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            let name = entry.file_name();
            if !SKIPPED_DIRECTORIES.iter().any(|skip| name == *skip) {
                collect_source_files(root, &path, out);
            }
        } else if file_type.is_file() && is_source_file(&path) {
            if entry.metadata().map(|m| m.len()).unwrap_or(u64::MAX) > MAX_SOURCE_FILE_BYTES {
                continue;
            }
            let display = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.push((display, path));
        }
    }
}

fn is_source_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            SOURCE_EXTENSIONS
                .iter()
                .any(|s| ext.eq_ignore_ascii_case(s))
        })
}

// ---------------------------------------------------------------------------
// LLM 线格式与校验
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct DiagnosesWire {
    #[serde(default)]
    diagnoses: Vec<DiagnosisWire>,
}

#[derive(Debug, Deserialize)]
struct DiagnosisWire {
    title: String,
    #[serde(default)]
    related_issue_ids: Vec<String>,
    root_cause: String,
    #[serde(default)]
    source_locations: Vec<LocationWire>,
    confidence: Confidence,
    #[serde(default)]
    suggested_fix: String,
}

#[derive(Debug, Deserialize)]
struct LocationWire {
    file: String,
    #[serde(default)]
    line: Option<usize>,
    #[serde(default)]
    snippet: String,
}

fn parse_diagnoses(
    content: &str,
    known_issue_ids: &HashSet<String>,
    known_files: &HashSet<String>,
    lenient: bool,
) -> Result<Vec<Diagnosis>, String> {
    let cleaned = strip_code_fence(content);
    let wire = serde_json::from_str::<DiagnosesWire>(cleaned)
        .map_err(|error| format!("content is not valid JSON for the expected schema: {error}"))?;

    let mut problems = Vec::new();
    let mut diagnoses = Vec::new();

    for (index, diagnosis) in wire.diagnoses.into_iter().enumerate() {
        // 引用未知源文件属臆造,严格模式拒绝、宽容模式丢弃该定位。
        let mut locations = Vec::new();
        for location in diagnosis.source_locations {
            if known_files.is_empty() || known_files.contains(&location.file) {
                locations.push(DiagnosedLocation {
                    file: location.file,
                    line: location.line,
                    snippet: location.snippet,
                });
            } else {
                problems.push(format!(
                    "diagnosis {index} cites file '{}' outside the provided source context",
                    location.file
                ));
            }
        }
        let related: Vec<String> = diagnosis
            .related_issue_ids
            .into_iter()
            .filter(|id| known_issue_ids.contains(id))
            .collect();

        diagnoses.push(Diagnosis {
            title: diagnosis.title,
            related_issue_ids: related,
            root_cause: diagnosis.root_cause,
            source_locations: locations,
            confidence: diagnosis.confidence,
            suggested_fix: diagnosis.suggested_fix,
        });
    }

    if !problems.is_empty() && !lenient {
        return Err(problems.join("; "));
    }
    Ok(diagnoses)
}

fn build_messages(findings: &[FailedFinding], snippets: &[SourceSnippet]) -> Vec<Value> {
    vec![
        json!({"role": "system", "content": system_prompt()}),
        json!({"role": "user", "content": user_prompt(findings, snippets)}),
    ]
}

fn system_prompt() -> String {
    r#"You are a defect diagnosis expert inside an evidence-driven testing tool.
Given failed test findings and relevant source code snippets, locate the root cause of each defect.

Respond with a single JSON object and nothing else, matching this schema exactly:
{
  "diagnoses": [
    {
      "title": string,
      "related_issue_ids": [string],           // issue ids from the findings this explains
      "root_cause": string,
      "source_locations": [
        { "file": string, "line": number, "snippet": string }
      ],
      "confidence": "low" | "medium" | "high",
      "suggested_fix": string
    }
  ]
}

Rules:
- Ground every source_locations.file in the provided snippets; never invent file paths. If the snippets do not contain the cause, return an empty source_locations and lower confidence.
- Produce ONE diagnosis per distinct root cause. Only group findings that genuinely share the SAME underlying defect. Do NOT merge unrelated failures (e.g. a backend 500, a missing validation message, and a localStorage bug are three separate root causes → three diagnoses). related_issue_ids must list only the issues explained by that one cause.
- root_cause and suggested_fix must be concrete and reference the observed evidence and the cited source location.
- Write title, root_cause and suggested_fix in the findings' language.
- Do not wrap the JSON in markdown fences."#
        .to_owned()
}

fn user_prompt(findings: &[FailedFinding], snippets: &[SourceSnippet]) -> String {
    let mut prompt = String::from("Failed findings:\n");
    for finding in findings {
        prompt.push_str(&format!(
            "- {} | {}\n  expected: {}\n  actual: {}\n",
            finding.issue_id, finding.title, finding.expected, finding.actual
        ));
    }
    prompt.push_str("\nRelevant source snippets:\n");
    if snippets.is_empty() {
        prompt.push_str("(no matching source files were found)\n");
    }
    for snippet in snippets {
        prompt.push_str(&format!(
            "--- {} (from line {}) ---\n{}\n",
            snippet.file, snippet.start_line, snippet.text
        ));
    }
    prompt
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashSet};
    use std::fs;

    use super::{
        FailedFinding, collect_source_context, extract_identifiers, generate_diagnoses,
        keywords_from_findings, parse_diagnoses,
    };
    use crate::ai::AiProviderKind;
    use crate::testutil::temp_project;

    #[test]
    fn extracts_selectors_and_path_segments() {
        let mut set = BTreeSet::new();
        extract_identifiers(
            "click #add-task-btn then GET /api/tasks returned 500",
            &mut set,
        );
        assert!(set.contains("add-task-btn"));
        assert!(set.contains("api"));
        assert!(set.contains("tasks"));
        // 纯数字与噪声被过滤。
        assert!(!set.contains("500"));
    }

    #[test]
    fn collects_source_snippets_by_keyword() {
        let root = temp_project("specprobe-diag-src");
        fs::create_dir_all(root.join("public")).expect("mkdir");
        fs::write(
            root.join("public").join("app.js"),
            "function addTask() {\n  const name = input.value;\n  list.push(name); // add-task-btn handler\n}\n",
        )
        .expect("write app.js");
        fs::create_dir_all(root.join("node_modules")).expect("mkdir nm");
        fs::write(root.join("node_modules").join("junk.js"), "add-task-btn").expect("write junk");

        let keywords = vec!["add-task-btn".to_owned()];
        let snippets = collect_source_context(&root, &keywords, 12);

        assert_eq!(snippets.len(), 1);
        assert_eq!(snippets[0].file, "public/app.js");
        assert!(snippets[0].text.contains("add-task-btn"));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn parse_rejects_unknown_file_then_filters_leniently() {
        let known_issue_ids: HashSet<String> = ["ISSUE-001".to_owned()].into_iter().collect();
        let known_files: HashSet<String> = ["public/app.js".to_owned()].into_iter().collect();
        let content = serde_json::json!({
            "diagnoses": [{
                "title": "空任务未校验",
                "related_issue_ids": ["ISSUE-001", "ISSUE-999"],
                "root_cause": "addTask 未过滤空白输入",
                "source_locations": [{"file": "ghost.js", "line": 3, "snippet": "..."}],
                "confidence": "high",
                "suggested_fix": "添加 trim 校验"
            }]
        })
        .to_string();

        let feedback = parse_diagnoses(&content, &known_issue_ids, &known_files, false)
            .expect_err("strict rejects unknown file");
        assert!(feedback.contains("ghost.js"));

        let diagnoses = parse_diagnoses(&content, &known_issue_ids, &known_files, true)
            .expect("lenient filters");
        assert_eq!(diagnoses.len(), 1);
        assert!(diagnoses[0].source_locations.is_empty());
        // 未知 issue id 被过滤。
        assert_eq!(diagnoses[0].related_issue_ids, vec!["ISSUE-001".to_owned()]);
    }

    #[test]
    fn keywords_are_deduplicated_and_capped() {
        let findings = vec![FailedFinding {
            issue_id: "ISSUE-001".to_owned(),
            title: "交互场景未通过".to_owned(),
            expected: "点击 #add-task-btn 后列表更新".to_owned(),
            actual: "click #add-task-btn: 元素不可见; /api/tasks 500".to_owned(),
        }];
        let keywords = keywords_from_findings(&findings);
        assert!(keywords.contains(&"add-task-btn".to_owned()));
        assert!(keywords.len() <= 16);
    }

    #[tokio::test]
    async fn mock_provider_yields_no_diagnoses() {
        let root = temp_project("specprobe-diag-mock");
        let findings = vec![FailedFinding {
            issue_id: "ISSUE-001".to_owned(),
            title: "t".to_owned(),
            expected: "e".to_owned(),
            actual: "a".to_owned(),
        }];
        let diagnoses = generate_diagnoses(&findings, &root, AiProviderKind::Mock, None)
            .await
            .expect("mock yields empty");
        assert!(diagnoses.is_empty());
        fs::remove_dir_all(root).expect("cleanup");
    }
}
