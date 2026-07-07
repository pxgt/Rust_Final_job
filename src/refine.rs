//! 需求理解两级流水线(ROADMAP 1.3)。
//!
//! 第一级:`requirements` 规则引擎做候选粗筛,并在 LLM 不可用时兜底。
//! 第二级:LLM 按文档全文精解析,输出 schema 约束的需求与验收标准;
//! 测试计划仍由确定性代码从需求生成,LLM 不直接产出计划。

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::ai::{
    AiCache, AiError, AiProviderKind, ChatProtocol, chat_protocol_for, run_chat_json,
    strip_code_fence,
};
use crate::requirements::{
    AcceptanceCriterion, AnalysisEngine, DiagnosticSeverity, EvidenceKind, QualityFlagKind,
    Requirement, RequirementCategory, RequirementDiagnostic, RequirementError, RequirementPriority,
    RequirementQualityFlag, RequirementReport, SourceLocation, analyze_requirements,
    build_test_plan, evidence_for_category,
};
use crate::scanner::scan_project;

/// requirements 命令的两级流水线选项。
#[derive(Debug, Clone, Default)]
pub struct RefineOptions {
    pub provider: AiProviderKind,
    pub cache_dir: Option<PathBuf>,
}

/// 两级流水线入口:规则引擎粗筛,非 Mock Provider 时用 LLM 精解析。
/// 配置缺失(MissingConfig)属用户显式选择的 Provider 不可用,直接报错;
/// 传输或校验失败则回退到规则结果并附告警诊断。
pub async fn analyze_requirements_with_refinement(
    path: &Path,
    options: RefineOptions,
) -> Result<RequirementReport, AiError> {
    let rule_report = analyze_requirements(path)?;
    let Some(protocol) = chat_protocol_for(options.provider)? else {
        return Ok(rule_report);
    };
    let cache = options.cache_dir.map(|dir| AiCache { dir });

    refine_with_protocol(path, rule_report, &protocol, cache.as_ref()).await
}

/// 用给定协议做精解析;失败(除配置错误外)回退规则报告。
/// 独立出来便于测试注入假端点协议。
async fn refine_with_protocol(
    path: &Path,
    mut rule_report: RequirementReport,
    protocol: &ChatProtocol,
    cache: Option<&AiCache>,
) -> Result<RequirementReport, AiError> {
    match refine_report(protocol, path, &rule_report, cache).await {
        Ok(refined) => Ok(refined),
        Err(error) => {
            rule_report.diagnostics.push(RequirementDiagnostic {
                severity: DiagnosticSeverity::Warning,
                message: format!(
                    "LLM refinement failed, falling back to rule-based extraction: {error}"
                ),
            });
            Ok(rule_report)
        }
    }
}

async fn refine_report(
    protocol: &ChatProtocol,
    input_path: &Path,
    rule_report: &RequirementReport,
    cache: Option<&AiCache>,
) -> Result<RequirementReport, AiError> {
    let root = fs::canonicalize(input_path).map_err(|source| {
        AiError::Requirements(RequirementError::Io {
            path: input_path.to_path_buf(),
            source,
        })
    })?;
    let tech_hint = if root.is_dir() {
        scan_project(&root)
            .ok()
            .filter(|profile| !profile.technologies.is_empty())
            .map(|profile| profile.technologies.join(", "))
    } else {
        None
    };

    let mut requirements: Vec<Requirement> = Vec::new();
    let mut notes = Vec::new();
    let mut requirement_counts = Vec::new();
    let mut attempts_total = 0_u32;
    let mut cache_hits = 0_u32;
    let mut total_tokens = 0_u64;

    for document in &rule_report.documents {
        let absolute = if root.is_file() {
            root.clone()
        } else {
            root.join(&document.path)
        };
        let text = fs::read_to_string(&absolute).map_err(|source| {
            AiError::Requirements(RequirementError::Io {
                path: absolute.clone(),
                source,
            })
        })?;
        let line_count = text.lines().count();
        let candidate_lines: Vec<usize> = rule_report
            .requirements
            .iter()
            .filter(|requirement| requirement.source.path == document.path)
            .map(|requirement| requirement.source.line)
            .collect();

        let messages = build_refine_messages(
            &document.path,
            &text,
            tech_hint.as_deref(),
            &candidate_lines,
        );
        let parse =
            |content: &str, lenient: bool| parse_refined_document(content, line_count, lenient);
        let (wire, transport) = run_chat_json(protocol, messages, cache, parse).await?;

        attempts_total += transport.attempts;
        if transport.cache_hit {
            cache_hits += 1;
        }
        if let Some(usage) = &transport.usage {
            total_tokens += usage.total_tokens;
        }

        let before = requirements.len();
        for item in wire.requirements {
            let requirement = build_refined_requirement(item, &document.path, requirements.len());
            requirements.push(requirement);
        }
        requirement_counts.push((document.path.clone(), requirements.len() - before));
        notes.extend(wire.notes);
    }

    Ok(assemble_report(
        rule_report,
        requirements,
        requirement_counts,
        notes,
        RefineRunStats {
            model: protocol.model.clone(),
            attempts: attempts_total,
            cache_hits,
            total_tokens,
        },
    ))
}

struct RefineRunStats {
    model: String,
    attempts: u32,
    cache_hits: u32,
    total_tokens: u64,
}

fn assemble_report(
    rule_report: &RequirementReport,
    requirements: Vec<Requirement>,
    requirement_counts: Vec<(String, usize)>,
    notes: Vec<String>,
    stats: RefineRunStats,
) -> RequirementReport {
    let documents = rule_report
        .documents
        .iter()
        .map(|document| {
            let count = requirement_counts
                .iter()
                .find(|(path, _)| path == &document.path)
                .map(|(_, count)| *count)
                .unwrap_or(0);
            crate::requirements::RequirementDocument {
                path: document.path.clone(),
                title: document.title.clone(),
                requirement_count: count,
            }
        })
        .collect();

    let mut diagnostics = vec![RequirementDiagnostic {
        severity: DiagnosticSeverity::Info,
        message: format!(
            "Requirements refined by LLM ({}): requests={}, cache_hits={}, total_tokens={}.",
            stats.model, stats.attempts, stats.cache_hits, stats.total_tokens
        ),
    }];
    diagnostics.extend(notes.into_iter().map(|note| RequirementDiagnostic {
        severity: DiagnosticSeverity::Info,
        message: format!("LLM note: {note}"),
    }));
    for requirement in &requirements {
        diagnostics.extend(
            requirement
                .quality_flags
                .iter()
                .map(|flag| RequirementDiagnostic {
                    severity: DiagnosticSeverity::Info,
                    message: format!(
                        "{} at {}:{} - {}",
                        flag.kind, requirement.source.path, requirement.source.line, flag.message
                    ),
                }),
        );
    }
    if requirements.is_empty() {
        diagnostics.push(RequirementDiagnostic {
            severity: DiagnosticSeverity::Warning,
            message: "LLM refinement returned no requirements.".to_owned(),
        });
    }

    let test_plan = build_test_plan(&requirements);

    RequirementReport {
        source: rule_report.source.clone(),
        engine: AnalysisEngine::LlmRefined,
        documents,
        requirements,
        test_plan,
        diagnostics,
    }
}

fn build_refined_requirement(
    item: RefinedRequirementWire,
    document_path: &str,
    existing: usize,
) -> Requirement {
    let id = format!("REQ-{:03}", existing + 1);
    let acceptance_criteria = item
        .acceptance_criteria
        .into_iter()
        .enumerate()
        .map(|(index, criterion)| AcceptanceCriterion {
            id: format!("{id}-AC-{:02}", index + 1),
            evidence: if criterion.evidence.is_empty() {
                evidence_for_category(item.category)
            } else {
                criterion.evidence
            },
            statement: criterion.statement,
        })
        .collect();

    Requirement {
        id,
        title: item.title,
        description: item.description,
        category: item.category,
        priority: item.priority,
        source: SourceLocation {
            path: document_path.to_owned(),
            line: item.source_line,
        },
        acceptance_criteria,
        quality_flags: item
            .quality_flags
            .into_iter()
            .map(|flag| RequirementQualityFlag {
                kind: flag.kind,
                message: flag.message,
            })
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// LLM 线格式与校验
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RefinedDocumentWire {
    #[serde(default)]
    requirements: Vec<RefinedRequirementWire>,
    #[serde(default)]
    notes: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RefinedRequirementWire {
    title: String,
    description: String,
    category: RequirementCategory,
    priority: RequirementPriority,
    source_line: usize,
    #[serde(default)]
    acceptance_criteria: Vec<RefinedCriterionWire>,
    #[serde(default)]
    quality_flags: Vec<RefinedFlagWire>,
}

#[derive(Debug, Deserialize)]
struct RefinedCriterionWire {
    statement: String,
    #[serde(default)]
    evidence: Vec<EvidenceKind>,
}

#[derive(Debug, Deserialize)]
struct RefinedFlagWire {
    kind: QualityFlagKind,
    message: String,
}

/// 解析并校验单个文档的精解析输出。严格模式下,行号越界、描述为空或
/// 缺少验收标准会返回反馈文本用于重问;宽容模式(最后一轮)丢弃无效条目。
fn parse_refined_document(
    content: &str,
    line_count: usize,
    lenient: bool,
) -> Result<RefinedDocumentWire, String> {
    let cleaned = strip_code_fence(content);
    let mut wire = serde_json::from_str::<RefinedDocumentWire>(cleaned)
        .map_err(|error| format!("content is not valid JSON for the expected schema: {error}"))?;

    let mut problems = Vec::new();
    for (index, item) in wire.requirements.iter().enumerate() {
        if item.description.trim().is_empty() {
            problems.push(format!("requirements[{index}] has an empty description"));
        }
        if item.source_line == 0 || item.source_line > line_count {
            problems.push(format!(
                "requirements[{index}] cites source_line {} outside the document (1..={line_count})",
                item.source_line
            ));
        }
        if item.acceptance_criteria.is_empty() {
            problems.push(format!("requirements[{index}] has no acceptance criteria"));
        } else if item
            .acceptance_criteria
            .iter()
            .any(|criterion| criterion.statement.trim().is_empty())
        {
            problems.push(format!(
                "requirements[{index}] contains an empty acceptance criterion statement"
            ));
        }
    }

    if problems.is_empty() {
        return Ok(wire);
    }
    if lenient {
        wire.requirements.retain(|item| {
            !item.description.trim().is_empty()
                && item.source_line >= 1
                && item.source_line <= line_count
                && !item.acceptance_criteria.is_empty()
                && item
                    .acceptance_criteria
                    .iter()
                    .all(|criterion| !criterion.statement.trim().is_empty())
        });
        return Ok(wire);
    }
    Err(problems.join("; "))
}

// ---------------------------------------------------------------------------
// Prompt
// ---------------------------------------------------------------------------

fn build_refine_messages(
    document_path: &str,
    text: &str,
    tech_hint: Option<&str>,
    candidate_lines: &[usize],
) -> Vec<Value> {
    vec![
        json!({"role": "system", "content": refine_system_prompt()}),
        json!({
            "role": "user",
            "content": refine_user_prompt(document_path, text, tech_hint, candidate_lines),
        }),
    ]
}

fn refine_system_prompt() -> String {
    r#"You are the requirement extraction engine of an evidence-driven software testing tool.
Read the product/requirements document and extract individually testable software requirements.

Respond with a single JSON object and nothing else, matching this schema exactly:
{
  "requirements": [
    {
      "title": string,
      "description": string,
      "category": "functional" | "ui" | "api" | "data" | "security" | "performance" | "compatibility" | "non_functional" | "unknown",
      "priority": "must" | "should" | "could" | "unknown",
      "source_line": number,
      "acceptance_criteria": [
        {
          "statement": string,
          "evidence": ["runtime_result" | "log" | "screenshot" | "network_trace" | "source_location" | "manual_observation"]
        }
      ],
      "quality_flags": [
        { "kind": "vague_language" | "missing_observable_result" | "too_broad", "message": string }
      ]
    }
  ],
  "notes": [string]
}

Rules:
- Extract real product requirements only. Skip installation guides, run instructions, changelogs, roadmaps, milestones and development journals.
- Each requirement is exactly one verifiable behavior. Split compound sentences into separate requirements.
- title is a short summary (max 60 characters). description must be self-contained and testable.
- acceptance_criteria: 1 to 3 per requirement. Each statement must name a concrete observable outcome (a specific page, element, API response, state change or error message), never a generic phrase like "the system behaves correctly".
- source_line is the line number shown in the numbered document where the requirement is stated.
- Add quality_flags when the original wording is vague, lacks an observable result, or bundles too many behaviors.
- Use notes for parser observations (e.g. sections skipped), not for requirements.
- Keep title, description, statements, flag messages and notes in the document's language (Chinese document gets Chinese text).
- Do not wrap the JSON in markdown fences."#
        .to_owned()
}

fn refine_user_prompt(
    document_path: &str,
    text: &str,
    tech_hint: Option<&str>,
    candidate_lines: &[usize],
) -> String {
    let mut prompt = format!("Document: {document_path}\n");
    if let Some(hint) = tech_hint {
        prompt.push_str(&format!("Project technologies detected: {hint}\n"));
    }
    if !candidate_lines.is_empty() {
        let lines = candidate_lines
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        prompt.push_str(&format!(
            "Rule-engine candidate lines (hints only, may be incomplete or wrong): {lines}\n"
        ));
    }
    prompt.push_str("\nNumbered document content:\n");
    for (index, line) in text.lines().enumerate() {
        prompt.push_str(&format!("{:>4}| {line}\n", index + 1));
    }
    prompt
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;

    use super::{
        RefineOptions, analyze_requirements_with_refinement, parse_refined_document,
        refine_with_protocol,
    };
    use crate::ai::{AiProviderKind, ChatProtocol};
    use crate::requirements::{AnalysisEngine, DiagnosticSeverity, analyze_requirements};
    use crate::testutil::{chat_response, spawn_chat_server, temp_project};

    fn refined_document_json() -> String {
        json!({
            "requirements": [
                {
                    "title": "登录成功提示",
                    "description": "用户使用有效凭据登录后,页面顶部必须显示“登录成功”提示条。",
                    "category": "ui",
                    "priority": "must",
                    "source_line": 1,
                    "acceptance_criteria": [
                        {
                            "statement": "输入有效账号密码并点击登录后,页面顶部出现文字为“登录成功”的提示条。",
                            "evidence": ["runtime_result", "screenshot"]
                        }
                    ],
                    "quality_flags": []
                },
                {
                    "title": "登录失败反馈",
                    "description": "使用错误密码登录时,系统必须在表单下方显示明确的错误信息。",
                    "category": "security",
                    "priority": "must",
                    "source_line": 1,
                    "acceptance_criteria": [
                        {
                            "statement": "输入错误密码提交后,表单下方显示“账号或密码错误”。",
                            "evidence": []
                        }
                    ],
                    "quality_flags": [
                        {"kind": "missing_observable_result", "message": "原文未指明错误文案。"}
                    ]
                }
            ],
            "notes": ["跳过了文档中的安装说明段落。"]
        })
        .to_string()
    }

    fn openai_protocol(base_url: String) -> ChatProtocol {
        // 直接构造协议注入假端点,避免测试修改进程级环境变量。
        crate::ai::test_openai_protocol(base_url)
    }

    #[tokio::test]
    async fn mock_provider_keeps_rule_engine_result() {
        let root = temp_project("specprobe-refine-mock");
        let file = root.join("PRD.md");
        fs::write(&file, "- 系统必须显示登录成功提示。").expect("write requirement");

        let report = analyze_requirements_with_refinement(
            &file,
            RefineOptions {
                provider: AiProviderKind::Mock,
                cache_dir: None,
            },
        )
        .await
        .expect("mock path succeeds");

        assert_eq!(report.engine, AnalysisEngine::RuleBased);
        assert_eq!(report.requirements.len(), 1);
        fs::remove_dir_all(root).expect("remove test project");
    }

    #[tokio::test]
    async fn llm_refinement_replaces_requirements_and_rebuilds_plan() {
        let root = temp_project("specprobe-refine-llm");
        let file = root.join("PRD.md");
        fs::write(&file, "- 系统必须显示登录成功提示。").expect("write requirement");
        let rule_report = analyze_requirements(&file).expect("rule analysis succeeds");
        let (base_url, handle) =
            spawn_chat_server(vec![(200, chat_response(&refined_document_json()))]);

        let report = refine_with_protocol(&file, rule_report, &openai_protocol(base_url), None)
            .await
            .expect("refinement succeeds");

        handle.join().expect("server thread joins");
        assert_eq!(report.engine, AnalysisEngine::LlmRefined);
        assert_eq!(report.requirements.len(), 2);
        assert_eq!(report.requirements[0].id, "REQ-001");
        assert_eq!(report.requirements[1].id, "REQ-002");
        assert_eq!(
            report.requirements[0].acceptance_criteria[0].id,
            "REQ-001-AC-01"
        );
        // 空 evidence 由类别默认证据补齐。
        assert!(
            !report.requirements[1].acceptance_criteria[0]
                .evidence
                .is_empty()
        );
        assert_eq!(report.test_plan.cases.len(), 2);
        assert_eq!(report.documents[0].requirement_count, 2);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.message.contains("Requirements refined by LLM") })
        );
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.message.contains("LLM note") })
        );
        fs::remove_dir_all(root).expect("remove test project");
    }

    #[tokio::test]
    async fn transport_failure_falls_back_to_rule_engine() {
        let root = temp_project("specprobe-refine-fallback");
        let file = root.join("PRD.md");
        fs::write(&file, "- 系统必须显示登录成功提示。").expect("write requirement");
        let rule_report = analyze_requirements(&file).expect("rule analysis succeeds");
        let (base_url, handle) = spawn_chat_server(vec![
            (500, r#"{"error":"boom"}"#.to_owned()),
            (500, r#"{"error":"boom"}"#.to_owned()),
            (500, r#"{"error":"boom"}"#.to_owned()),
        ]);

        let report = refine_with_protocol(&file, rule_report, &openai_protocol(base_url), None)
            .await
            .expect("fallback should not error");

        handle.join().expect("server thread joins");
        assert_eq!(report.engine, AnalysisEngine::RuleBased);
        assert_eq!(report.requirements.len(), 1);
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.severity == DiagnosticSeverity::Warning
                && diagnostic.message.contains("falling back to rule-based")
        }));
        fs::remove_dir_all(root).expect("remove test project");
    }

    #[test]
    fn parse_rejects_out_of_range_lines_then_filters_leniently() {
        // json!().to_string() 产出紧凑 JSON,键值间没有空格。
        let content = refined_document_json().replace(r#""source_line":1"#, r#""source_line":99"#);

        let feedback = parse_refined_document(&content, 3, false).expect_err("strict rejects");
        assert!(feedback.contains("source_line"));

        let wire = parse_refined_document(&content, 3, true).expect("lenient filters");
        assert!(wire.requirements.is_empty());
    }

    #[test]
    fn parse_requires_acceptance_criteria() {
        let content = json!({
            "requirements": [{
                "title": "无验收标准",
                "description": "系统必须显示结果。",
                "category": "functional",
                "priority": "must",
                "source_line": 1,
                "acceptance_criteria": [],
                "quality_flags": []
            }],
            "notes": []
        })
        .to_string();

        let feedback = parse_refined_document(&content, 5, false).expect_err("strict rejects");
        assert!(feedback.contains("no acceptance criteria"));
    }
}
