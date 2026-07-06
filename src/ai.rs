use std::env;
use std::fmt;
use std::path::Path;

use clap::ValueEnum;
use serde::Serialize;
use thiserror::Error;

use crate::requirements::{
    QualityFlagKind, Requirement, RequirementCategory, RequirementError, RequirementPriority,
    RequirementReport, analyze_requirements,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum AiProviderKind {
    Mock,
    OpenaiCompatible,
    Ollama,
}

#[derive(Debug, Error)]
pub enum AiError {
    #[error(transparent)]
    Requirements(#[from] RequirementError),
    #[error("{provider} provider is not configured: {message}")]
    MissingConfig {
        provider: &'static str,
        message: String,
    },
    #[error("{provider} provider transport is not implemented yet: {message}")]
    TransportNotImplemented {
        provider: &'static str,
        message: String,
    },
}

#[derive(Debug, Serialize)]
pub struct AiAnalysisReport {
    pub provider: AiProviderInfo,
    pub base_report: RequirementReport,
    pub request: AiRequestPreview,
    pub model_output: AiModelOutput,
}

#[derive(Debug, Clone, Serialize)]
pub struct AiProviderInfo {
    pub kind: AiProviderKind,
    pub name: String,
    pub model: String,
    pub configured: bool,
    pub offline: bool,
}

#[derive(Debug, Serialize)]
pub struct AiRequestPreview {
    pub task: String,
    pub requirement_count: usize,
    pub schema_name: String,
    pub prompt_excerpt: String,
}

#[derive(Debug, Serialize)]
pub struct AiModelOutput {
    pub summary: String,
    pub suggestions: Vec<AiSuggestion>,
    pub follow_up_questions: Vec<String>,
    pub confidence: Confidence,
}

#[derive(Debug, Serialize)]
pub struct AiSuggestion {
    pub requirement_id: String,
    pub suggestion_type: SuggestionType,
    pub severity: SuggestionSeverity,
    pub message: String,
    pub rationale: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionType {
    ClarifyRequirement,
    AddAcceptanceCriterion,
    AddNegativeCase,
    AddEvidence,
    ConfirmPriority,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionSeverity {
    Info,
    Warning,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

trait AiProvider {
    fn info(&self) -> AiProviderInfo;
    fn analyze(&self, report: &RequirementReport) -> Result<AiModelOutput, AiError>;
}

pub fn analyze_with_provider(
    path: &Path,
    provider_kind: AiProviderKind,
) -> Result<AiAnalysisReport, AiError> {
    let base_report = analyze_requirements(path)?;
    let provider = provider_for(provider_kind);
    let provider_info = provider.info();
    let request = build_request_preview(&base_report);
    let model_output = provider.analyze(&base_report)?;

    Ok(AiAnalysisReport {
        provider: provider_info,
        base_report,
        request,
        model_output,
    })
}

fn provider_for(kind: AiProviderKind) -> Box<dyn AiProvider> {
    match kind {
        AiProviderKind::Mock => Box::new(MockProvider),
        AiProviderKind::OpenaiCompatible => Box::new(OpenAiCompatibleProvider::from_env()),
        AiProviderKind::Ollama => Box::new(OllamaProvider::from_env()),
    }
}

fn build_request_preview(report: &RequirementReport) -> AiRequestPreview {
    let prompt = build_prompt(report);
    AiRequestPreview {
        task: "enhance_requirement_analysis".to_owned(),
        requirement_count: report.requirements.len(),
        schema_name: "AiModelOutput".to_owned(),
        prompt_excerpt: excerpt(&prompt, 360),
    }
}

fn build_prompt(report: &RequirementReport) -> String {
    let mut prompt = String::new();
    prompt.push_str("You are reviewing extracted software requirements.\n");
    prompt.push_str("Return structured suggestions only.\n\n");
    for requirement in &report.requirements {
        prompt.push_str(&format!(
            "{} [{} / {}]: {}\n",
            requirement.id, requirement.priority, requirement.category, requirement.description
        ));
    }
    prompt
}

struct MockProvider;

impl AiProvider for MockProvider {
    fn info(&self) -> AiProviderInfo {
        AiProviderInfo {
            kind: AiProviderKind::Mock,
            name: "Deterministic Mock Provider".to_owned(),
            model: "mock-v1".to_owned(),
            configured: true,
            offline: true,
        }
    }

    fn analyze(&self, report: &RequirementReport) -> Result<AiModelOutput, AiError> {
        let mut suggestions = Vec::new();

        for requirement in &report.requirements {
            suggestions.extend(suggestions_for_requirement(requirement));
        }

        let follow_up_questions = build_follow_up_questions(report, &suggestions);
        let confidence = if report.requirements.is_empty() {
            Confidence::Low
        } else if suggestions
            .iter()
            .any(|suggestion| suggestion.severity == SuggestionSeverity::High)
        {
            Confidence::Medium
        } else {
            Confidence::High
        };

        Ok(AiModelOutput {
            summary: format!(
                "Mock provider reviewed {} requirement(s) and produced {} suggestion(s).",
                report.requirements.len(),
                suggestions.len()
            ),
            suggestions,
            follow_up_questions,
            confidence,
        })
    }
}

struct OpenAiCompatibleProvider {
    api_key: Option<String>,
    base_url: String,
    model: Option<String>,
}

impl OpenAiCompatibleProvider {
    fn from_env() -> Self {
        Self {
            api_key: env::var("OPENAI_API_KEY").ok(),
            base_url: env::var("OPENAI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_owned()),
            model: env::var("OPENAI_MODEL").ok(),
        }
    }
}

impl AiProvider for OpenAiCompatibleProvider {
    fn info(&self) -> AiProviderInfo {
        AiProviderInfo {
            kind: AiProviderKind::OpenaiCompatible,
            name: "OpenAI-compatible Provider".to_owned(),
            model: self
                .model
                .clone()
                .unwrap_or_else(|| "not configured".to_owned()),
            configured: self.api_key.is_some() && self.model.is_some(),
            offline: false,
        }
    }

    fn analyze(&self, _report: &RequirementReport) -> Result<AiModelOutput, AiError> {
        if self.api_key.is_none() {
            return Err(AiError::MissingConfig {
                provider: "OpenAI-compatible",
                message: "set OPENAI_API_KEY before using this provider".to_owned(),
            });
        }
        if self.model.is_none() {
            return Err(AiError::MissingConfig {
                provider: "OpenAI-compatible",
                message: "set OPENAI_MODEL before using this provider".to_owned(),
            });
        }

        Err(AiError::TransportNotImplemented {
            provider: "OpenAI-compatible",
            message: format!(
                "configuration is present for {}, but HTTP transport will be implemented after API-key approval",
                self.base_url
            ),
        })
    }
}

struct OllamaProvider {
    model: Option<String>,
}

impl OllamaProvider {
    fn from_env() -> Self {
        Self {
            model: env::var("OLLAMA_MODEL").ok(),
        }
    }
}

impl AiProvider for OllamaProvider {
    fn info(&self) -> AiProviderInfo {
        AiProviderInfo {
            kind: AiProviderKind::Ollama,
            name: "Ollama Provider".to_owned(),
            model: self
                .model
                .clone()
                .unwrap_or_else(|| "not configured".to_owned()),
            configured: self.model.is_some(),
            offline: false,
        }
    }

    fn analyze(&self, _report: &RequirementReport) -> Result<AiModelOutput, AiError> {
        if self.model.is_none() {
            return Err(AiError::MissingConfig {
                provider: "Ollama",
                message: "set OLLAMA_MODEL before using this provider".to_owned(),
            });
        }

        Err(AiError::TransportNotImplemented {
            provider: "Ollama",
            message:
                "local model invocation will be enabled after the provider protocol is finalized"
                    .to_owned(),
        })
    }
}

fn suggestions_for_requirement(requirement: &Requirement) -> Vec<AiSuggestion> {
    let mut suggestions = Vec::new();

    if requirement.priority == RequirementPriority::Unknown {
        suggestions.push(AiSuggestion {
            requirement_id: requirement.id.clone(),
            suggestion_type: SuggestionType::ConfirmPriority,
            severity: SuggestionSeverity::Info,
            message: "补充该需求的优先级，例如必须、应该或可以。".to_owned(),
            rationale: "优先级会影响测试计划排序和缺陷严重程度判断。".to_owned(),
        });
    }

    for flag in &requirement.quality_flags {
        match flag.kind {
            QualityFlagKind::VagueLanguage => suggestions.push(AiSuggestion {
                requirement_id: requirement.id.clone(),
                suggestion_type: SuggestionType::ClarifyRequirement,
                severity: SuggestionSeverity::High,
                message: "将模糊描述改写成可量化的验收条件。".to_owned(),
                rationale: "模糊词会导致自动测试无法判断通过或失败。".to_owned(),
            }),
            QualityFlagKind::MissingObservableResult => suggestions.push(AiSuggestion {
                requirement_id: requirement.id.clone(),
                suggestion_type: SuggestionType::AddEvidence,
                severity: SuggestionSeverity::Warning,
                message: "补充可观察结果，例如页面提示、返回值、日志或状态变化。".to_owned(),
                rationale: "没有可观察结果时，测试执行器无法收集有效证据。".to_owned(),
            }),
            QualityFlagKind::TooBroad => suggestions.push(AiSuggestion {
                requirement_id: requirement.id.clone(),
                suggestion_type: SuggestionType::ClarifyRequirement,
                severity: SuggestionSeverity::High,
                message: "把该需求拆分成多个单一职责的需求。".to_owned(),
                rationale: "过宽需求会让失败定位不清晰，也难以生成精确补丁建议。".to_owned(),
            }),
        }
    }

    if matches!(
        requirement.category,
        RequirementCategory::Security | RequirementCategory::Api | RequirementCategory::Data
    ) {
        suggestions.push(AiSuggestion {
            requirement_id: requirement.id.clone(),
            suggestion_type: SuggestionType::AddNegativeCase,
            severity: SuggestionSeverity::Warning,
            message: "增加异常输入、权限不足或失败响应的负向测试。".to_owned(),
            rationale: "这类需求通常需要验证失败路径，避免只覆盖理想流程。".to_owned(),
        });
    }

    if requirement.acceptance_criteria.len() < 2 {
        suggestions.push(AiSuggestion {
            requirement_id: requirement.id.clone(),
            suggestion_type: SuggestionType::AddAcceptanceCriterion,
            severity: SuggestionSeverity::Info,
            message: "考虑补充边界条件或回归场景的验收标准。".to_owned(),
            rationale: "单条验收标准通常只能覆盖主路径。".to_owned(),
        });
    }

    suggestions
}

fn build_follow_up_questions(
    report: &RequirementReport,
    suggestions: &[AiSuggestion],
) -> Vec<String> {
    let mut questions = Vec::new();

    if report.requirements.is_empty() {
        questions.push("是否有独立的 PRD、需求说明或用户故事文档可以提供？".to_owned());
        return questions;
    }

    if suggestions
        .iter()
        .any(|suggestion| suggestion.suggestion_type == SuggestionType::ClarifyRequirement)
    {
        questions.push("哪些需求需要用户确认具体阈值、页面状态或错误提示文案？".to_owned());
    }

    if report
        .requirements
        .iter()
        .any(|requirement| requirement.category == RequirementCategory::Unknown)
    {
        questions.push("是否需要补充需求所属模块，以便后续定位功能入口？".to_owned());
    }

    if questions.is_empty() {
        questions.push("是否要把这些建议转换成下一阶段的可执行测试计划？".to_owned());
    }

    questions
}

fn excerpt(text: &str, limit: usize) -> String {
    let mut value = text.chars().take(limit).collect::<String>();
    if text.chars().count() > limit {
        value.push_str("...");
    }
    value
}

impl fmt::Display for AiProviderKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Mock => "mock",
            Self::OpenaiCompatible => "openai-compatible",
            Self::Ollama => "ollama",
        })
    }
}

impl fmt::Display for SuggestionType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ClarifyRequirement => "clarify-requirement",
            Self::AddAcceptanceCriterion => "add-acceptance-criterion",
            Self::AddNegativeCase => "add-negative-case",
            Self::AddEvidence => "add-evidence",
            Self::ConfirmPriority => "confirm-priority",
        })
    }
}

impl fmt::Display for SuggestionSeverity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::High => "high",
        })
    }
}

impl fmt::Display for Confidence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        AiProvider, AiProviderKind, OpenAiCompatibleProvider, SuggestionSeverity, SuggestionType,
        analyze_with_provider,
    };
    use crate::requirements::analyze_requirements;

    #[test]
    fn mock_provider_generates_suggestions_from_requirement_quality() {
        let root = temp_project("specprobe-ai-mock");
        let file = root.join("PRD.md");
        fs::write(&file, "- 页面应该简单友好。").expect("write requirement");

        let report =
            analyze_with_provider(&file, AiProviderKind::Mock).expect("mock analysis succeeds");

        assert_eq!(report.provider.kind, AiProviderKind::Mock);
        assert_eq!(report.base_report.requirements.len(), 1);
        assert!(report.model_output.suggestions.iter().any(|suggestion| {
            suggestion.suggestion_type == SuggestionType::ClarifyRequirement
                && suggestion.severity == SuggestionSeverity::High
        }));

        fs::remove_dir_all(root).expect("remove test project");
    }

    #[test]
    fn openai_compatible_provider_requires_api_key() {
        let root = temp_project("specprobe-ai-openai");
        let file = root.join("PRD.md");
        fs::write(&file, "- 系统必须显示解析结果。").expect("write requirement");
        let base_report = analyze_requirements(&file).expect("requirement analysis succeeds");
        let provider = OpenAiCompatibleProvider {
            api_key: None,
            base_url: "https://example.test/v1".to_owned(),
            model: Some("test-model".to_owned()),
        };

        let error = provider
            .analyze(&base_report)
            .expect_err("missing key should fail");

        assert!(error.to_string().contains("OPENAI_API_KEY"));
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
