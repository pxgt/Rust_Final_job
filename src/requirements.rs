use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use thiserror::Error;

const SKIPPED_DIRECTORIES: &[&str] = &[
    ".git",
    ".idea",
    ".next",
    ".venv",
    ".vscode",
    "build",
    "coverage",
    "dist",
    "node_modules",
    "target",
    "vendor",
];

const REQUIREMENT_KEYWORDS: &[&str] = &[
    "必须", "应当", "应该", "需要", "支持", "能够", "可以", "允许", "禁止", "不得", "shall",
    "must", "should", "need", "support", "allow", "can",
];

const VAGUE_WORDS: &[&str] = &[
    "简单",
    "友好",
    "快速",
    "高效",
    "完善",
    "合理",
    "适当",
    "美观",
    "良好",
    "尽量",
    "等等",
    "相关",
    "若干",
    "simple",
    "friendly",
    "fast",
    "reasonable",
    "proper",
    "etc",
];

#[derive(Debug, Error)]
pub enum RequirementError {
    #[error("requirement path does not exist: {0}")]
    NotFound(PathBuf),
    #[error("requirement path must be a file or directory: {0}")]
    UnsupportedPath(PathBuf),
    #[error("failed to inspect {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct RequirementReport {
    pub source: String,
    pub documents: Vec<RequirementDocument>,
    pub requirements: Vec<Requirement>,
    pub test_plan: TestPlan,
    pub diagnostics: Vec<RequirementDiagnostic>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RequirementDocument {
    pub path: String,
    pub title: Option<String>,
    pub requirement_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Requirement {
    pub id: String,
    pub title: String,
    pub description: String,
    pub category: RequirementCategory,
    pub priority: RequirementPriority,
    pub source: SourceLocation,
    pub acceptance_criteria: Vec<AcceptanceCriterion>,
    pub quality_flags: Vec<RequirementQualityFlag>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceLocation {
    pub path: String,
    pub line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RequirementCategory {
    Functional,
    Ui,
    Api,
    Data,
    Security,
    Performance,
    Compatibility,
    NonFunctional,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RequirementPriority {
    Must,
    Should,
    Could,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct AcceptanceCriterion {
    pub id: String,
    pub statement: String,
    pub evidence: Vec<EvidenceKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    RuntimeResult,
    Log,
    Screenshot,
    NetworkTrace,
    SourceLocation,
    ManualObservation,
}

#[derive(Debug, Clone, Serialize)]
pub struct RequirementQualityFlag {
    pub kind: QualityFlagKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityFlagKind {
    VagueLanguage,
    MissingObservableResult,
    TooBroad,
}

#[derive(Debug, Clone, Serialize)]
pub struct TestPlan {
    pub name: String,
    pub cases: Vec<TestCase>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TestCase {
    pub id: String,
    pub requirement_id: String,
    pub title: String,
    pub executor_hint: ExecutorHint,
    pub steps: Vec<TestStep>,
    pub expected_result: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TestStep {
    pub action: TestAction,
    pub target: String,
    pub input: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TestAction {
    LocateEntryPoint,
    PerformRequirementAction,
    ObserveResult,
    CollectEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutorHint {
    Browser,
    Api,
    Cli,
    Generic,
    ManualReview,
}

#[derive(Debug, Clone, Serialize)]
pub struct RequirementDiagnostic {
    pub severity: DiagnosticSeverity,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Info,
    Warning,
}

struct RequirementCandidate {
    description: String,
    headings: Vec<String>,
    source: SourceLocation,
}

struct ParsedDocument {
    path: String,
    title: Option<String>,
    candidates: Vec<RequirementCandidate>,
}

pub fn analyze_requirements(path: &Path) -> Result<RequirementReport, RequirementError> {
    if !path.exists() {
        return Err(RequirementError::NotFound(path.to_path_buf()));
    }

    let source = canonicalize(path)?;
    let document_paths = if source.is_file() {
        vec![source.clone()]
    } else if source.is_dir() {
        collect_requirement_documents(&source)?
    } else {
        return Err(RequirementError::UnsupportedPath(path.to_path_buf()));
    };

    let mut documents = Vec::new();
    let mut requirements = Vec::new();
    let mut diagnostics = Vec::new();

    if document_paths.is_empty() {
        diagnostics.push(RequirementDiagnostic {
            severity: DiagnosticSeverity::Warning,
            message: "No requirement documents were found.".to_owned(),
        });
    }

    for document_path in document_paths {
        let document = parse_document(&source, &document_path)?;
        let requirement_count = document.candidates.len();
        documents.push(RequirementDocument {
            path: document.path.clone(),
            title: document.title,
            requirement_count,
        });

        for candidate in document.candidates {
            let requirement = build_requirement(candidate, requirements.len() + 1);
            diagnostics.extend(requirement.quality_flags.iter().map(|flag| {
                RequirementDiagnostic {
                    severity: DiagnosticSeverity::Info,
                    message: format!(
                        "{} at {}:{} - {}",
                        flag.kind, requirement.source.path, requirement.source.line, flag.message
                    ),
                }
            }));
            requirements.push(requirement);
        }
    }

    if documents.is_empty() {
        diagnostics.push(RequirementDiagnostic {
            severity: DiagnosticSeverity::Info,
            message: "Pass a Markdown/TXT file directly, or place requirement documents in README, PRD, SPEC, REQUIREMENTS, or docs/.".to_owned(),
        });
    } else if requirements.is_empty() {
        diagnostics.push(RequirementDiagnostic {
            severity: DiagnosticSeverity::Warning,
            message: "Requirement documents were found, but no testable requirement sentence was extracted.".to_owned(),
        });
    }

    let test_plan = build_test_plan(&requirements);

    Ok(RequirementReport {
        source: display_path(&source),
        documents,
        requirements,
        test_plan,
        diagnostics,
    })
}

fn collect_requirement_documents(root: &Path) -> Result<Vec<PathBuf>, RequirementError> {
    let mut documents = Vec::new();
    collect_requirement_documents_from(root, root, &mut documents)?;
    documents.sort();
    Ok(documents)
}

fn collect_requirement_documents_from(
    root: &Path,
    directory: &Path,
    documents: &mut Vec<PathBuf>,
) -> Result<(), RequirementError> {
    let entries = fs::read_dir(directory).map_err(|source| RequirementError::Io {
        path: directory.to_path_buf(),
        source,
    })?;

    for entry in entries {
        let entry = entry.map_err(|source| RequirementError::Io {
            path: directory.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|source| RequirementError::Io {
            path: path.clone(),
            source,
        })?;

        if file_type.is_symlink() {
            continue;
        }

        if file_type.is_dir() {
            let name = entry.file_name();
            if !SKIPPED_DIRECTORIES.iter().any(|skipped| name == *skipped) {
                collect_requirement_documents_from(root, &path, documents)?;
            }
            continue;
        }

        if file_type.is_file() && is_requirement_document(root, &path) {
            documents.push(path);
        }
    }

    Ok(())
}

fn parse_document(root: &Path, path: &Path) -> Result<ParsedDocument, RequirementError> {
    let contents = fs::read_to_string(path).map_err(|source| RequirementError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let display = relative_display(root, path);
    let mut title = None;
    let mut headings = Vec::<String>::new();
    let mut candidates = Vec::new();
    let mut in_code_fence = false;

    for (index, line) in contents.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim();

        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_code_fence = !in_code_fence;
            continue;
        }
        if in_code_fence || trimmed.is_empty() || is_markdown_table_line(trimmed) {
            continue;
        }

        if let Some((level, heading)) = parse_heading(trimmed) {
            if title.is_none() && level == 1 {
                title = Some(heading.clone());
            }
            update_headings(&mut headings, level, heading);
            continue;
        }

        let (text, list_item) = strip_list_marker(trimmed)
            .map(|text| (text, true))
            .unwrap_or_else(|| (trimmed.to_owned(), false));
        let text = strip_inline_markdown(&text);

        if is_requirement_candidate(&text, &headings, list_item) {
            candidates.push(RequirementCandidate {
                description: text,
                headings: headings.clone(),
                source: SourceLocation {
                    path: display.clone(),
                    line: line_number,
                },
            });
        }
    }

    Ok(ParsedDocument {
        path: display,
        title,
        candidates,
    })
}

fn build_requirement(candidate: RequirementCandidate, index: usize) -> Requirement {
    let id = format!("REQ-{index:03}");
    let category = classify_category(&candidate.description, &candidate.headings);
    let priority = classify_priority(&candidate.description);
    let quality_flags = quality_flags_for(&candidate.description);
    let title = summarize_title(&candidate.description);
    let acceptance_criteria =
        build_acceptance_criteria(&id, &candidate.description, category, &quality_flags);

    Requirement {
        id,
        title,
        description: candidate.description,
        category,
        priority,
        source: candidate.source,
        acceptance_criteria,
        quality_flags,
    }
}

fn build_acceptance_criteria(
    requirement_id: &str,
    description: &str,
    category: RequirementCategory,
    quality_flags: &[RequirementQualityFlag],
) -> Vec<AcceptanceCriterion> {
    let mut criteria = Vec::new();
    criteria.push(AcceptanceCriterion {
        id: format!("{requirement_id}-AC-01"),
        statement: format!(
            "执行与需求“{}”对应的操作后，系统应产生可观察且与需求一致的结果。",
            trim_sentence(description)
        ),
        evidence: evidence_for_category(category),
    });

    if matches!(
        category,
        RequirementCategory::Security | RequirementCategory::Api | RequirementCategory::Data
    ) {
        criteria.push(AcceptanceCriterion {
            id: format!("{requirement_id}-AC-02"),
            statement: "异常输入、权限不足或失败响应应被明确处理，并留下可检查的错误信息。"
                .to_owned(),
            evidence: vec![
                EvidenceKind::RuntimeResult,
                EvidenceKind::Log,
                EvidenceKind::NetworkTrace,
            ],
        });
    }

    if quality_flags
        .iter()
        .any(|flag| flag.kind == QualityFlagKind::VagueLanguage)
    {
        criteria.push(AcceptanceCriterion {
            id: format!("{requirement_id}-AC-{:02}", criteria.len() + 1),
            statement: "需求中的模糊描述需要被量化或补充为可判断的验收条件。".to_owned(),
            evidence: vec![EvidenceKind::ManualObservation],
        });
    }

    criteria
}

fn build_test_plan(requirements: &[Requirement]) -> TestPlan {
    let cases = requirements
        .iter()
        .enumerate()
        .map(|(index, requirement)| TestCase {
            id: format!("TC-{:03}", index + 1),
            requirement_id: requirement.id.clone(),
            title: format!("验证 {}", requirement.title),
            executor_hint: executor_hint_for(requirement),
            steps: vec![
                TestStep {
                    action: TestAction::LocateEntryPoint,
                    target: "找到与该需求相关的功能入口、接口或命令。".to_owned(),
                    input: None,
                },
                TestStep {
                    action: TestAction::PerformRequirementAction,
                    target: "执行需求描述的核心操作。".to_owned(),
                    input: Some(requirement.description.clone()),
                },
                TestStep {
                    action: TestAction::ObserveResult,
                    target: "检查运行结果是否满足验收标准。".to_owned(),
                    input: None,
                },
                TestStep {
                    action: TestAction::CollectEvidence,
                    target: "采集输出、日志、截图、网络记录或源码位置作为证据。".to_owned(),
                    input: None,
                },
            ],
            expected_result: requirement
                .acceptance_criteria
                .first()
                .map(|criterion| criterion.statement.clone())
                .unwrap_or_else(|| "需求应被满足。".to_owned()),
        })
        .collect();

    TestPlan {
        name: "Generated acceptance test plan".to_owned(),
        cases,
    }
}

fn parse_heading(line: &str) -> Option<(usize, String)> {
    let hashes = line
        .chars()
        .take_while(|character| *character == '#')
        .count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = line.get(hashes..)?.trim();
    if rest.is_empty() {
        return None;
    }
    Some((hashes, strip_inline_markdown(rest)))
}

fn update_headings(headings: &mut Vec<String>, level: usize, heading: String) {
    headings.truncate(level.saturating_sub(1));
    headings.push(heading);
}

fn strip_list_marker(line: &str) -> Option<String> {
    let line = line.trim_start();

    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(marker) {
            return Some(strip_checkbox(rest).to_owned());
        }
    }

    let mut digits = 0;
    for character in line.chars() {
        if character.is_ascii_digit() {
            digits += 1;
        } else {
            break;
        }
    }

    if digits == 0 {
        return None;
    }

    let rest = &line[digits..];
    rest.strip_prefix(". ")
        .or_else(|| rest.strip_prefix(") "))
        .map(|value| value.to_owned())
}

fn strip_checkbox(text: &str) -> &str {
    let trimmed = text.trim_start();
    for prefix in ["[ ] ", "[x] ", "[X] "] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return rest;
        }
    }
    trimmed
}

fn strip_inline_markdown(text: &str) -> String {
    strip_markdown_links(text.trim())
        .trim_matches('`')
        .replace("**", "")
        .replace("__", "")
        .replace(['[', ']'], "")
        .trim()
        .to_owned()
}

/// 把 `[文字](url)` 和 `![alt](url)` 替换为纯文字,丢弃 URL,避免链接目标混入需求描述。
fn strip_markdown_links(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(open) = rest.find('[') {
        let Some(separator) = rest[open..].find("](").map(|offset| open + offset) else {
            break;
        };
        let Some(close) = rest[separator + 2..]
            .find(')')
            .map(|offset| separator + 2 + offset)
        else {
            break;
        };

        let before = rest[..open].strip_suffix('!').unwrap_or(&rest[..open]);
        result.push_str(before);
        result.push_str(&rest[open + 1..separator]);
        rest = &rest[close + 1..];
    }

    result.push_str(rest);
    result
}

fn is_requirement_candidate(text: &str, headings: &[String], list_item: bool) -> bool {
    if text.chars().count() < 6 || text.starts_with('|') {
        return false;
    }
    if headings
        .iter()
        .any(|heading| is_non_requirement_section(heading))
    {
        return false;
    }

    let lower = text.to_lowercase();
    let keyword_hit = REQUIREMENT_KEYWORDS
        .iter()
        .any(|keyword| lower.contains(&keyword.to_lowercase()));
    let section_hit = headings
        .iter()
        .any(|heading| is_requirement_section(heading));

    if list_item {
        keyword_hit || section_hit
    } else {
        keyword_hit && section_hit
    }
}

fn is_requirement_section(heading: &str) -> bool {
    let lower = heading.to_lowercase();
    [
        "需求",
        "功能",
        "验收",
        "目标",
        "能力",
        "用户故事",
        "requirement",
        "feature",
        "story",
        "acceptance",
        "prd",
        "spec",
    ]
    .iter()
    .any(|keyword| lower.contains(keyword))
}

fn is_non_requirement_section(heading: &str) -> bool {
    let lower = heading.to_lowercase();
    [
        "本地运行",
        "运行方式",
        "安装",
        "开发日志",
        "当前状态",
        "已知环境",
        "风险",
        "里程碑",
        "代码结构",
        "技术方案",
        "数据模型",
        "下一步",
        "local run",
        "installation",
        "changelog",
        "milestone",
        "risk",
    ]
    .iter()
    .any(|keyword| lower.contains(keyword))
}

fn classify_category(text: &str, headings: &[String]) -> RequirementCategory {
    let combined = format!("{} {}", headings.join(" "), text).to_lowercase();

    if contains_any(
        &combined,
        &["安全", "权限", "认证", "登录", "密码", "token", "security"],
    ) {
        RequirementCategory::Security
    } else if contains_any(&combined, &["api", "接口", "http", "请求", "响应"]) {
        RequirementCategory::Api
    } else if contains_any(
        &combined,
        &["数据", "数据库", "保存", "导入", "导出", "持久", "data"],
    ) {
        RequirementCategory::Data
    } else if contains_any(
        &combined,
        &["性能", "响应时间", "耗时", "并发", "performance"],
    ) {
        RequirementCategory::Performance
    } else if contains_any(
        &combined,
        &["页面", "界面", "按钮", "输入框", "导航", "布局", "ui"],
    ) {
        RequirementCategory::Ui
    } else if contains_any(&combined, &["兼容", "浏览器", "windows", "linux", "mobile"]) {
        RequirementCategory::Compatibility
    } else if contains_any(&combined, &["非功能", "可用性", "稳定性", "可靠性"]) {
        RequirementCategory::NonFunctional
    } else if combined.trim().is_empty() {
        RequirementCategory::Unknown
    } else {
        RequirementCategory::Functional
    }
}

fn classify_priority(text: &str) -> RequirementPriority {
    let lower = text.to_lowercase();
    if contains_any(
        &lower,
        &["必须", "不得", "禁止", "must", "shall", "required"],
    ) {
        RequirementPriority::Must
    } else if contains_any(&lower, &["应该", "应当", "需要", "should", "need"]) {
        RequirementPriority::Should
    } else if contains_any(&lower, &["可以", "允许", "建议", "may", "could", "allow"]) {
        RequirementPriority::Could
    } else {
        RequirementPriority::Unknown
    }
}

fn quality_flags_for(text: &str) -> Vec<RequirementQualityFlag> {
    let lower = text.to_lowercase();
    let mut flags = Vec::new();
    if VAGUE_WORDS
        .iter()
        .any(|word| lower.contains(&word.to_lowercase()))
    {
        flags.push(RequirementQualityFlag {
            kind: QualityFlagKind::VagueLanguage,
            message: "包含较难直接验证的模糊词，需要补充量化标准。".to_owned(),
        });
    }
    if text.chars().count() > 140 {
        flags.push(RequirementQualityFlag {
            kind: QualityFlagKind::TooBroad,
            message: "需求句子较长，建议拆分为多个独立验收点。".to_owned(),
        });
    }
    if !has_observable_result(text) {
        flags.push(RequirementQualityFlag {
            kind: QualityFlagKind::MissingObservableResult,
            message: "缺少明确的输出、状态变化或错误反馈。".to_owned(),
        });
    }
    flags
}

fn has_observable_result(text: &str) -> bool {
    contains_any(
        &text.to_lowercase(),
        &[
            "显示", "返回", "跳转", "保存", "生成", "导出", "提示", "记录", "创建", "更新", "删除",
            "拒绝", "display", "return", "redirect", "save", "show", "create", "update", "delete",
        ],
    )
}

fn evidence_for_category(category: RequirementCategory) -> Vec<EvidenceKind> {
    match category {
        RequirementCategory::Ui => vec![
            EvidenceKind::RuntimeResult,
            EvidenceKind::Screenshot,
            EvidenceKind::Log,
        ],
        RequirementCategory::Api => vec![
            EvidenceKind::RuntimeResult,
            EvidenceKind::NetworkTrace,
            EvidenceKind::Log,
        ],
        RequirementCategory::Data => vec![
            EvidenceKind::RuntimeResult,
            EvidenceKind::Log,
            EvidenceKind::SourceLocation,
        ],
        RequirementCategory::Security => vec![
            EvidenceKind::RuntimeResult,
            EvidenceKind::NetworkTrace,
            EvidenceKind::Log,
        ],
        _ => vec![EvidenceKind::RuntimeResult, EvidenceKind::ManualObservation],
    }
}

fn executor_hint_for(requirement: &Requirement) -> ExecutorHint {
    if requirement
        .quality_flags
        .iter()
        .any(|flag| flag.kind == QualityFlagKind::VagueLanguage)
    {
        return ExecutorHint::ManualReview;
    }

    match requirement.category {
        RequirementCategory::Ui | RequirementCategory::Compatibility => ExecutorHint::Browser,
        RequirementCategory::Api => ExecutorHint::Api,
        _ if requirement.description.contains("命令")
            || requirement.description.contains("CLI") =>
        {
            ExecutorHint::Cli
        }
        _ => ExecutorHint::Generic,
    }
}

fn contains_any(text: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|keyword| text.contains(keyword))
}

fn summarize_title(text: &str) -> String {
    let sentence = trim_sentence(text);
    let mut title = sentence.chars().take(48).collect::<String>();
    if sentence.chars().count() > 48 {
        title.push_str("...");
    }
    title
}

fn trim_sentence(text: &str) -> String {
    text.trim()
        .trim_end_matches(['。', '.', ';', '；'])
        .to_owned()
}

fn is_markdown_table_line(line: &str) -> bool {
    line.starts_with('|') || (line.contains('|') && line.contains("---"))
}

fn is_requirement_document(root: &Path, path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return false;
    };
    if !extension.eq_ignore_ascii_case("md") && !extension.eq_ignore_ascii_case("txt") {
        return false;
    }

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_uppercase();
    let named_document = file_name.starts_with("README")
        || file_name.starts_with("REQUIREMENT")
        || file_name.starts_with("SPEC")
        || file_name.starts_with("PRD");
    let in_docs = path
        .strip_prefix(root)
        .ok()
        .and_then(|relative| relative.components().next())
        .is_some_and(|component| component.as_os_str().eq_ignore_ascii_case("docs"));

    named_document || in_docs
}

fn canonicalize(path: &Path) -> Result<PathBuf, RequirementError> {
    fs::canonicalize(path).map_err(|source| RequirementError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn relative_display(root: &Path, path: &Path) -> String {
    let value = if root.is_dir() {
        path.strip_prefix(root).unwrap_or(path)
    } else {
        path.file_name().map(Path::new).unwrap_or(path)
    };
    normalize_path(value)
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn display_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    value.strip_prefix(r"\\?\").unwrap_or(&value).to_owned()
}

impl fmt::Display for RequirementCategory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Functional => "functional",
            Self::Ui => "ui",
            Self::Api => "api",
            Self::Data => "data",
            Self::Security => "security",
            Self::Performance => "performance",
            Self::Compatibility => "compatibility",
            Self::NonFunctional => "non-functional",
            Self::Unknown => "unknown",
        })
    }
}

impl fmt::Display for RequirementPriority {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Must => "must",
            Self::Should => "should",
            Self::Could => "could",
            Self::Unknown => "unknown",
        })
    }
}

impl fmt::Display for QualityFlagKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::VagueLanguage => "vague-language",
            Self::MissingObservableResult => "missing-observable-result",
            Self::TooBroad => "too-broad",
        })
    }
}

impl fmt::Display for DiagnosticSeverity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Info => "info",
            Self::Warning => "warning",
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        ExecutorHint, RequirementCategory, RequirementPriority, analyze_requirements,
        classify_category, classify_priority, parse_heading, strip_inline_markdown,
    };

    #[test]
    fn parses_markdown_requirements_into_criteria_and_test_cases() {
        let root = temp_project("specprobe-req");
        fs::write(
            root.join("README.md"),
            r#"# Demo Product

## 功能需求

- 用户必须可以登录系统，并在成功后跳转到首页。
- 系统应该支持导出 CSV 报表。

## 非功能需求

- 页面应该快速加载。
"#,
        )
        .expect("write requirements");

        let report = analyze_requirements(&root).expect("analysis should succeed");

        assert_eq!(report.documents.len(), 1);
        assert_eq!(report.requirements.len(), 3);
        assert_eq!(report.test_plan.cases.len(), 3);
        assert_eq!(report.requirements[0].id, "REQ-001");
        assert_eq!(report.requirements[0].priority, RequirementPriority::Must);
        assert_eq!(
            report.requirements[0].category,
            RequirementCategory::Security
        );
        assert_eq!(report.test_plan.cases[0].requirement_id, "REQ-001");
        assert!(
            report.requirements[2]
                .quality_flags
                .iter()
                .any(|flag| flag.message.contains("模糊词"))
        );

        fs::remove_dir_all(root).expect("remove test project");
    }

    #[test]
    fn supports_direct_file_analysis() {
        let root = temp_project("specprobe-req-file");
        let file = root.join("PRD.md");
        fs::write(&file, "- API 必须返回用户列表。").expect("write requirement");

        let report = analyze_requirements(&file).expect("analysis should succeed");

        assert_eq!(report.documents[0].path, "PRD.md");
        assert_eq!(report.requirements.len(), 1);
        assert_eq!(report.requirements[0].category, RequirementCategory::Api);

        fs::remove_dir_all(root).expect("remove test project");
    }

    #[test]
    fn ignores_code_fences() {
        let root = temp_project("specprobe-req-code");
        fs::write(
            root.join("README.md"),
            r#"# Requirements

```text
- 用户必须看到这行，但它只是示例代码。
```

- 用户必须看到真实结果。
"#,
        )
        .expect("write requirements");

        let report = analyze_requirements(&root).expect("analysis should succeed");

        assert_eq!(report.requirements.len(), 1);
        assert!(report.requirements[0].description.contains("真实结果"));

        fs::remove_dir_all(root).expect("remove test project");
    }

    #[test]
    fn directory_mode_ignores_project_journal_documents() {
        let root = temp_project("specprobe-req-project-doc");
        fs::write(
            root.join("PROJECT.md"),
            r#"# Project journal

## 下一步任务

- 实现 Markdown 需求文档的规则解析器。
"#,
        )
        .expect("write project journal");
        fs::write(
            root.join("README.md"),
            r#"# Product

## 当前能力

- 用户必须可以导入需求文档，并显示解析结果。
"#,
        )
        .expect("write readme");

        let report = analyze_requirements(&root).expect("analysis should succeed");

        assert_eq!(report.documents.len(), 1);
        assert_eq!(report.documents[0].path, "README.md");
        assert_eq!(report.requirements.len(), 1);

        fs::remove_dir_all(root).expect("remove test project");
    }

    #[test]
    fn classifies_priority_and_category() {
        assert_eq!(
            classify_priority("系统必须保存用户数据"),
            RequirementPriority::Must
        );
        assert_eq!(
            classify_priority("系统可以导出数据"),
            RequirementPriority::Could
        );
        assert_eq!(
            classify_category("接口必须返回 JSON 响应", &[]),
            RequirementCategory::Api
        );
        assert_eq!(
            classify_category("页面应该显示错误提示", &[]),
            RequirementCategory::Ui
        );
    }

    #[test]
    fn keeps_link_text_and_drops_urls() {
        assert_eq!(
            strip_inline_markdown("详见 [接口规范](https://example.com/spec) 第 3 节"),
            "详见 接口规范 第 3 节"
        );
        assert_eq!(
            strip_inline_markdown("![登录页](img/login.png) 页面必须显示错误提示"),
            "登录页 页面必须显示错误提示"
        );
        assert_eq!(
            strip_inline_markdown("**加粗** 与 `代码` 不受影响"),
            "加粗 与 `代码` 不受影响"
        );
    }

    #[test]
    fn parses_heading_levels() {
        assert_eq!(
            parse_heading("## 功能需求"),
            Some((2, "功能需求".to_owned()))
        );
        assert_eq!(parse_heading("not a heading"), None);
    }

    #[test]
    fn vague_requirements_use_manual_review_hint() {
        let root = temp_project("specprobe-req-vague");
        fs::write(root.join("README.md"), "- 页面应该简单友好。").expect("write requirements");

        let report = analyze_requirements(&root).expect("analysis should succeed");

        assert_eq!(
            report.test_plan.cases[0].executor_hint,
            ExecutorHint::ManualReview
        );
        fs::remove_dir_all(root).expect("remove test project");
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
