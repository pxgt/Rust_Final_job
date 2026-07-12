use std::collections::HashSet;
use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::requirements::{
    QualityFlagKind, Requirement, RequirementCategory, RequirementError, RequirementPriority,
    RequirementReport, analyze_requirements,
};

const AI_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
/// 传输层重试:网络错误、429 和 5xx 最多尝试 3 次(指数退避)。
const TRANSPORT_ATTEMPTS: u32 = 3;
/// 校验层重试:模型输出不符合 schema 时,带反馈重问,最多 2 轮。
const VALIDATION_ROUNDS: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum AiProviderKind {
    #[default]
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
    #[error("{provider} request failed with HTTP {status}: {detail}")]
    Http {
        provider: &'static str,
        status: u16,
        detail: String,
    },
    #[error("{provider} request failed: {source}")]
    Network {
        provider: &'static str,
        #[source]
        source: reqwest::Error,
    },
    #[error("{provider} returned an unusable response: {message}")]
    InvalidResponse {
        provider: &'static str,
        message: String,
    },
}

#[derive(Debug, Serialize)]
pub struct AiAnalysisReport {
    pub provider: AiProviderInfo,
    pub transport: AiTransportInfo,
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

#[derive(Debug, Clone, Serialize)]
pub struct AiTransportInfo {
    /// 实际发出的 HTTP 请求数;命中缓存或使用 Mock 时为 0。
    pub attempts: u32,
    pub cache_hit: bool,
    pub usage: Option<AiUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Serialize)]
pub struct AiRequestPreview {
    pub task: String,
    pub requirement_count: usize,
    pub schema_name: String,
    pub prompt_excerpt: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AiModelOutput {
    pub summary: String,
    #[serde(default)]
    pub suggestions: Vec<AiSuggestion>,
    #[serde(default)]
    pub follow_up_questions: Vec<String>,
    pub confidence: Confidence,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AiSuggestion {
    pub requirement_id: String,
    pub suggestion_type: SuggestionType,
    pub severity: SuggestionSeverity,
    pub message: String,
    pub rationale: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionType {
    ClarifyRequirement,
    AddAcceptanceCriterion,
    AddNegativeCase,
    AddEvidence,
    ConfirmPriority,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionSeverity {
    Info,
    Warning,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

/// AI 命令的运行时选项。`cache_dir` 为 None 时禁用响应缓存。
#[derive(Debug, Clone, Default)]
pub struct AiOptions {
    pub cache_dir: Option<PathBuf>,
}

pub async fn analyze_with_provider(
    path: &Path,
    provider_kind: AiProviderKind,
    options: AiOptions,
) -> Result<AiAnalysisReport, AiError> {
    let base_report = analyze_requirements(path)?;
    analyze_report_with_provider(base_report, provider_kind, options).await
}

/// 对已有需求报告(规则或 LLM 精解析产物)运行建议分析。
pub async fn analyze_report_with_provider(
    base_report: RequirementReport,
    provider_kind: AiProviderKind,
    options: AiOptions,
) -> Result<AiAnalysisReport, AiError> {
    let request = build_request_preview(&base_report);
    let cache = options.cache_dir.map(|dir| AiCache { dir });

    // Provider 通过枚举分发而不是 dyn trait:analyze 是 async fn,
    // 且各 Provider 集合封闭、构造方式各异,枚举分发无需额外依赖。
    let (provider_info, model_output, transport) = match provider_kind {
        AiProviderKind::Mock => {
            let provider = MockProvider;
            let output = provider.analyze(&base_report).await?;
            let transport = AiTransportInfo {
                attempts: 0,
                cache_hit: false,
                usage: None,
            };
            (provider.info(), output, transport)
        }
        AiProviderKind::OpenaiCompatible => {
            let provider = OpenAiCompatibleProvider::from_env();
            let (output, transport) = provider.analyze(&base_report, cache.as_ref()).await?;
            (provider.info(), output, transport)
        }
        AiProviderKind::Ollama => {
            let provider = OllamaProvider::from_env();
            let (output, transport) = provider.analyze(&base_report, cache.as_ref()).await?;
            (provider.info(), output, transport)
        }
    };

    Ok(AiAnalysisReport {
        provider: provider_info,
        transport,
        base_report,
        request,
        model_output,
    })
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

fn build_system_prompt() -> String {
    r#"You are a software requirements reviewer inside an evidence-driven testing tool.
Review the extracted requirements and produce improvement suggestions.

Respond with a single JSON object and nothing else, matching this schema exactly:
{
  "summary": string,
  "suggestions": [
    {
      "requirement_id": string,  // must be an id that appears in the input list
      "suggestion_type": "clarify_requirement" | "add_acceptance_criterion" | "add_negative_case" | "add_evidence" | "confirm_priority",
      "severity": "info" | "warning" | "high",
      "message": string,
      "rationale": string
    }
  ],
  "follow_up_questions": [string],
  "confidence": "low" | "medium" | "high"
}

Rules:
- Write summary, message, rationale and follow_up_questions in the same language as the requirements (Chinese requirements get Chinese text).
- Focus on testability: vague wording, missing observable results, missing negative cases, missing evidence.
- Do not invent requirement ids. Do not wrap the JSON in markdown fences."#
        .to_owned()
}

fn build_prompt(report: &RequirementReport) -> String {
    let mut prompt = String::new();
    prompt.push_str("Extracted requirements to review:\n\n");
    for requirement in &report.requirements {
        prompt.push_str(&format!(
            "{} [priority={} / category={}] {}\n",
            requirement.id, requirement.priority, requirement.category, requirement.description
        ));
        for criterion in &requirement.acceptance_criteria {
            prompt.push_str(&format!(
                "  acceptance {}: {}\n",
                criterion.id, criterion.statement
            ));
        }
        for flag in &requirement.quality_flags {
            prompt.push_str(&format!("  quality_flag {}: {}\n", flag.kind, flag.message));
        }
    }
    if report.requirements.is_empty() {
        prompt.push_str("(no requirements were extracted)\n");
    }
    prompt
}

fn build_messages(report: &RequirementReport) -> Vec<Value> {
    vec![
        json!({"role": "system", "content": build_system_prompt()}),
        json!({"role": "user", "content": build_prompt(report)}),
    ]
}

// ---------------------------------------------------------------------------
// 响应缓存:请求指纹 -> 已验证的模型输出原文,尽力而为,失败不阻断分析。
// ---------------------------------------------------------------------------

pub(crate) struct AiCache {
    pub(crate) dir: PathBuf,
}

impl AiCache {
    fn read(&self, key: &str) -> Option<String> {
        fs::read_to_string(self.dir.join(format!("{key}.json"))).ok()
    }

    fn write(&self, key: &str, content: &str) {
        if fs::create_dir_all(&self.dir).is_ok() {
            let _ = fs::write(self.dir.join(format!("{key}.json")), content);
        }
    }
}

fn cache_key(provider: &str, endpoint: &str, model: &str, messages: &[Value]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(provider.as_bytes());
    hasher.update(endpoint.as_bytes());
    hasher.update(model.as_bytes());
    hasher.update(Value::Array(messages.to_vec()).to_string().as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

// ---------------------------------------------------------------------------
// 聊天协议抽象:OpenAI 兼容与 Ollama 共用一套重试 / 校验 / 缓存循环,
// 仅请求体构造与响应字段提取不同。
// ---------------------------------------------------------------------------

pub(crate) struct ChatProtocol {
    pub(crate) provider: &'static str,
    pub(crate) endpoint: String,
    pub(crate) api_key: Option<String>,
    pub(crate) model: String,
    /// 是否在 400 且提示 response_format 不支持时降级重试(OpenAI 兼容端点差异)。
    pub(crate) supports_format_fallback: bool,
    pub(crate) build_body: fn(model: &str, messages: &[Value], include_format: bool) -> Value,
    pub(crate) extract_content: fn(&Value) -> Option<String>,
    pub(crate) extract_usage: fn(&Value) -> Option<AiUsage>,
}

/// 测试辅助:构造指向指定端点的 OpenAI 风格协议,避免测试修改进程级环境变量。
#[cfg(test)]
pub(crate) fn test_openai_protocol(base_url: String) -> ChatProtocol {
    OpenAiCompatibleProvider {
        api_key: Some("test-key".to_owned()),
        base_url,
        model: Some("test-model".to_owned()),
    }
    .chat_protocol()
    .expect("test protocol is configured")
}

/// 按 Provider 类型构造聊天协议;Mock 返回 None(离线路径,无传输)。
pub(crate) fn chat_protocol_for(kind: AiProviderKind) -> Result<Option<ChatProtocol>, AiError> {
    match kind {
        AiProviderKind::Mock => Ok(None),
        AiProviderKind::OpenaiCompatible => OpenAiCompatibleProvider::from_env()
            .chat_protocol()
            .map(Some),
        AiProviderKind::Ollama => OllamaProvider::from_env().chat_protocol().map(Some),
    }
}

/// 通用的"聊天 → JSON 输出"循环:缓存、传输重试、schema 校验反馈重问。
/// `parse(content, lenient)` 负责解析与校验;lenient 在最后一轮为 true。
/// 对所有消息的 content 字段做出站脱敏(密钥不外发给第三方 LLM)。
fn redact_messages(mut messages: Vec<Value>) -> Vec<Value> {
    for message in &mut messages {
        if let Some(content) = message.get("content").and_then(Value::as_str) {
            let redacted = crate::redact::redact_secrets(content);
            message["content"] = Value::String(redacted);
        }
    }
    messages
}

pub(crate) async fn run_chat_json<T>(
    protocol: &ChatProtocol,
    base_messages: Vec<Value>,
    cache: Option<&AiCache>,
    parse: impl Fn(&str, bool) -> Result<T, String>,
) -> Result<(T, AiTransportInfo), AiError> {
    // 出站脱敏:发送给外部 LLM 的所有内容先过一遍密钥脱敏(ROADMAP 3.4)。
    let base_messages = redact_messages(base_messages);
    let key = cache_key(
        protocol.provider,
        &protocol.endpoint,
        &protocol.model,
        &base_messages,
    );

    if let Some(store) = cache
        && let Some(cached) = store.read(&key)
        && let Ok(output) = parse(&cached, true)
    {
        return Ok((
            output,
            AiTransportInfo {
                attempts: 0,
                cache_hit: true,
                usage: None,
            },
        ));
    }

    let client = reqwest::Client::builder()
        .timeout(AI_REQUEST_TIMEOUT)
        .build()
        .map_err(|source| AiError::Network {
            provider: protocol.provider,
            source,
        })?;

    let mut messages = base_messages;
    let mut include_format = true;
    let mut attempts = 0_u32;
    let mut usage = None;
    let mut round = 0_u32;

    loop {
        let body = (protocol.build_body)(&protocol.model, &messages, include_format);
        let response = match post_with_retry(
            &client,
            protocol.provider,
            &protocol.endpoint,
            protocol.api_key.as_deref(),
            &body,
            &mut attempts,
        )
        .await
        {
            Err(AiError::Http {
                status: 400,
                ref detail,
                ..
            }) if include_format
                && protocol.supports_format_fallback
                && detail.to_ascii_lowercase().contains("response_format") =>
            {
                include_format = false;
                continue;
            }
            other => other?,
        };

        usage = (protocol.extract_usage)(&response).or(usage);
        let content =
            (protocol.extract_content)(&response).ok_or_else(|| AiError::InvalidResponse {
                provider: protocol.provider,
                message: "response is missing the assistant message content".to_owned(),
            })?;

        round += 1;
        let lenient = round >= VALIDATION_ROUNDS;
        match parse(&content, lenient) {
            Ok(output) => {
                if let Some(store) = cache {
                    store.write(&key, &content);
                }
                return Ok((
                    output,
                    AiTransportInfo {
                        attempts,
                        cache_hit: false,
                        usage,
                    },
                ));
            }
            Err(feedback) => {
                if round >= VALIDATION_ROUNDS {
                    return Err(AiError::InvalidResponse {
                        provider: protocol.provider,
                        message: feedback,
                    });
                }
                messages.push(json!({"role": "assistant", "content": content}));
                messages.push(json!({
                    "role": "user",
                    "content": format!(
                        "Your previous reply was rejected: {feedback}. Reply again with only the JSON object matching the schema, no other text."
                    ),
                }));
            }
        }
    }
}

async fn post_with_retry(
    client: &reqwest::Client,
    provider: &'static str,
    endpoint: &str,
    api_key: Option<&str>,
    body: &Value,
    attempts: &mut u32,
) -> Result<Value, AiError> {
    let mut delay = Duration::from_millis(250);
    let mut last_error = None;

    for try_index in 0..TRANSPORT_ATTEMPTS {
        if try_index > 0 {
            tokio::time::sleep(delay).await;
            delay *= 3;
        }
        *attempts += 1;

        let mut request = client.post(endpoint).json(body);
        if let Some(key) = api_key {
            request = request.bearer_auth(key);
        }

        match request.send().await {
            Err(source) => {
                last_error = Some(AiError::Network { provider, source });
            }
            Ok(response) => {
                let status = response.status().as_u16();
                let text = response.text().await.unwrap_or_default();
                if status == 429 || status >= 500 {
                    last_error = Some(AiError::Http {
                        provider,
                        status,
                        detail: excerpt(&text, 400),
                    });
                    continue;
                }
                if !(200..300).contains(&status) {
                    // 认证或请求错误重试也不会恢复,直接返回。
                    return Err(AiError::Http {
                        provider,
                        status,
                        detail: excerpt(&text, 400),
                    });
                }
                return serde_json::from_str(&text).map_err(|error| AiError::InvalidResponse {
                    provider,
                    message: format!("response body is not JSON: {error}"),
                });
            }
        }
    }

    Err(last_error.unwrap_or(AiError::InvalidResponse {
        provider,
        message: "transport retries exhausted without a response".to_owned(),
    }))
}

/// 解析并校验模型输出。`lenient` 为 true 时,引用未知需求 ID 的建议被
/// 静默过滤(最后一轮);否则返回反馈文本用于重问。
fn parse_model_output(
    content: &str,
    report: &RequirementReport,
    lenient: bool,
) -> Result<AiModelOutput, String> {
    let cleaned = strip_code_fence(content);
    let mut output = serde_json::from_str::<AiModelOutput>(cleaned)
        .map_err(|error| format!("content is not valid JSON for the expected schema: {error}"))?;

    let known: HashSet<&str> = report
        .requirements
        .iter()
        .map(|requirement| requirement.id.as_str())
        .collect();
    let unknown: Vec<String> = output
        .suggestions
        .iter()
        .filter(|suggestion| !known.contains(suggestion.requirement_id.as_str()))
        .map(|suggestion| suggestion.requirement_id.clone())
        .collect();

    if unknown.is_empty() {
        return Ok(output);
    }
    if lenient {
        output
            .suggestions
            .retain(|suggestion| known.contains(suggestion.requirement_id.as_str()));
        return Ok(output);
    }
    Err(format!(
        "suggestions reference unknown requirement ids: {}",
        unknown.join(", ")
    ))
}

pub(crate) fn strip_code_fence(content: &str) -> &str {
    let trimmed = content.trim();
    let Some(rest) = trimmed.strip_prefix("```") else {
        return trimmed;
    };
    let rest = rest.strip_prefix("json").unwrap_or(rest);
    let rest = rest.trim_start_matches(['\r', '\n']);
    rest.strip_suffix("```").map(str::trim_end).unwrap_or(rest)
}

// ---------------------------------------------------------------------------
// Providers
// ---------------------------------------------------------------------------

struct MockProvider;

impl MockProvider {
    fn info(&self) -> AiProviderInfo {
        AiProviderInfo {
            kind: AiProviderKind::Mock,
            name: "Deterministic Mock Provider".to_owned(),
            model: "mock-v1".to_owned(),
            configured: true,
            offline: true,
        }
    }

    async fn analyze(&self, report: &RequirementReport) -> Result<AiModelOutput, AiError> {
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

    fn chat_protocol(&self) -> Result<ChatProtocol, AiError> {
        const PROVIDER: &str = "OpenAI-compatible";
        let Some(api_key) = self.api_key.clone() else {
            return Err(AiError::MissingConfig {
                provider: PROVIDER,
                message: "set OPENAI_API_KEY before using this provider".to_owned(),
            });
        };
        let Some(model) = self.model.clone() else {
            return Err(AiError::MissingConfig {
                provider: PROVIDER,
                message: "set OPENAI_MODEL before using this provider".to_owned(),
            });
        };

        Ok(ChatProtocol {
            provider: PROVIDER,
            endpoint: format!("{}/chat/completions", self.base_url.trim_end_matches('/')),
            api_key: Some(api_key),
            model,
            supports_format_fallback: true,
            build_body: |model, messages, include_format| {
                let mut body = json!({
                    "model": model,
                    "messages": messages,
                    "temperature": 0,
                });
                if include_format {
                    body["response_format"] = json!({"type": "json_object"});
                }
                body
            },
            extract_content: |response| {
                response
                    .pointer("/choices/0/message/content")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            },
            extract_usage: |response| {
                let usage = response.get("usage")?;
                Some(AiUsage {
                    prompt_tokens: usage.get("prompt_tokens").and_then(Value::as_u64)?,
                    completion_tokens: usage
                        .get("completion_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                    total_tokens: usage
                        .get("total_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                })
            },
        })
    }

    async fn analyze(
        &self,
        report: &RequirementReport,
        cache: Option<&AiCache>,
    ) -> Result<(AiModelOutput, AiTransportInfo), AiError> {
        let protocol = self.chat_protocol()?;
        run_chat_json(
            &protocol,
            build_messages(report),
            cache,
            |content, lenient| parse_model_output(content, report, lenient),
        )
        .await
    }
}

struct OllamaProvider {
    base_url: String,
    model: Option<String>,
}

impl OllamaProvider {
    fn from_env() -> Self {
        Self {
            base_url: env::var("OLLAMA_BASE_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:11434".to_owned()),
            model: env::var("OLLAMA_MODEL").ok(),
        }
    }

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

    fn chat_protocol(&self) -> Result<ChatProtocol, AiError> {
        const PROVIDER: &str = "Ollama";
        let Some(model) = self.model.clone() else {
            return Err(AiError::MissingConfig {
                provider: PROVIDER,
                message: "set OLLAMA_MODEL before using this provider".to_owned(),
            });
        };

        Ok(ChatProtocol {
            provider: PROVIDER,
            endpoint: format!("{}/api/chat", self.base_url.trim_end_matches('/')),
            api_key: None,
            model,
            supports_format_fallback: false,
            build_body: |model, messages, _include_format| {
                json!({
                    "model": model,
                    "messages": messages,
                    "stream": false,
                    "format": "json",
                    "options": {"temperature": 0},
                })
            },
            extract_content: |response| {
                response
                    .pointer("/message/content")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            },
            extract_usage: |response| {
                let prompt_tokens = response.get("prompt_eval_count").and_then(Value::as_u64)?;
                let completion_tokens = response
                    .get("eval_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                Some(AiUsage {
                    prompt_tokens,
                    completion_tokens,
                    total_tokens: prompt_tokens + completion_tokens,
                })
            },
        })
    }

    async fn analyze(
        &self,
        report: &RequirementReport,
        cache: Option<&AiCache>,
    ) -> Result<(AiModelOutput, AiTransportInfo), AiError> {
        let protocol = self.chat_protocol()?;
        run_chat_json(
            &protocol,
            build_messages(report),
            cache,
            |content, lenient| parse_model_output(content, report, lenient),
        )
        .await
    }
}

fn suggestions_for_requirement(requirement: &Requirement) -> Vec<AiSuggestion> {
    let mut suggestions = Vec::new();

    if requirement.priority == RequirementPriority::Unknown {
        suggestions.push(AiSuggestion {
            requirement_id: requirement.id.clone(),
            suggestion_type: SuggestionType::ConfirmPriority,
            severity: SuggestionSeverity::Info,
            message: "补充该需求的优先级,例如必须、应该或可以。".to_owned(),
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
                message: "补充可观察结果,例如页面提示、返回值、日志或状态变化。".to_owned(),
                rationale: "没有可观察结果时,测试执行器无法收集有效证据。".to_owned(),
            }),
            QualityFlagKind::TooBroad => suggestions.push(AiSuggestion {
                requirement_id: requirement.id.clone(),
                suggestion_type: SuggestionType::ClarifyRequirement,
                severity: SuggestionSeverity::High,
                message: "把该需求拆分成多个单一职责的需求。".to_owned(),
                rationale: "过宽需求会让失败定位不清晰,也难以生成精确补丁建议。".to_owned(),
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
            rationale: "这类需求通常需要验证失败路径,避免只覆盖理想流程。".to_owned(),
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
        questions.push("是否有独立的 PRD、需求说明或用户故事文档可以提供?".to_owned());
        return questions;
    }

    if suggestions
        .iter()
        .any(|suggestion| suggestion.suggestion_type == SuggestionType::ClarifyRequirement)
    {
        questions.push("哪些需求需要用户确认具体阈值、页面状态或错误提示文案?".to_owned());
    }

    if report
        .requirements
        .iter()
        .any(|requirement| requirement.category == RequirementCategory::Unknown)
    {
        questions.push("是否需要补充需求所属模块,以便后续定位功能入口?".to_owned());
    }

    if questions.is_empty() {
        questions.push("是否要把这些建议转换成下一阶段的可执行测试计划?".to_owned());
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

    use super::{
        AiCache, AiOptions, AiProviderKind, Confidence, OllamaProvider, OpenAiCompatibleProvider,
        SuggestionSeverity, SuggestionType, analyze_with_provider, parse_model_output,
    };
    use crate::testutil::{chat_response, requirement_fixture, spawn_chat_server, temp_project};

    const VALID_OUTPUT: &str = r#"{"summary":"分析完成","suggestions":[{"requirement_id":"REQ-001","suggestion_type":"clarify_requirement","severity":"high","message":"补充可观察的登录成功提示","rationale":"便于自动断言"}],"follow_up_questions":["登录成功后跳转到哪个页面?"],"confidence":"medium"}"#;

    #[tokio::test]
    async fn mock_provider_generates_suggestions_from_requirement_quality() {
        let root = temp_project("specprobe-ai-mock");
        let file = root.join("PRD.md");
        fs::write(&file, "- 页面应该简单友好。").expect("write requirement");

        let report = analyze_with_provider(&file, AiProviderKind::Mock, AiOptions::default())
            .await
            .expect("mock analysis succeeds");

        assert_eq!(report.provider.kind, AiProviderKind::Mock);
        assert_eq!(report.base_report.requirements.len(), 1);
        assert_eq!(report.transport.attempts, 0);
        assert!(!report.transport.cache_hit);
        assert!(report.model_output.suggestions.iter().any(|suggestion| {
            suggestion.suggestion_type == SuggestionType::ClarifyRequirement
                && suggestion.severity == SuggestionSeverity::High
        }));

        fs::remove_dir_all(root).expect("remove test project");
    }

    #[tokio::test]
    async fn openai_compatible_provider_requires_api_key() {
        let report = requirement_fixture("specprobe-ai-openai-nokey");
        let provider = OpenAiCompatibleProvider {
            api_key: None,
            base_url: "https://example.test/v1".to_owned(),
            model: Some("test-model".to_owned()),
        };

        let error = provider
            .analyze(&report, None)
            .await
            .expect_err("missing key should fail");

        assert!(error.to_string().contains("OPENAI_API_KEY"));
    }

    #[tokio::test]
    async fn openai_transport_parses_structured_output() {
        let report = requirement_fixture("specprobe-ai-openai-ok");
        let (base_url, handle) = spawn_chat_server(vec![(200, chat_response(VALID_OUTPUT))]);
        let provider = OpenAiCompatibleProvider {
            api_key: Some("test-key".to_owned()),
            base_url,
            model: Some("test-model".to_owned()),
        };

        let (output, transport) = provider
            .analyze(&report, None)
            .await
            .expect("analysis succeeds");

        let requests = handle.join().expect("server thread joins");
        assert_eq!(requests.len(), 1);
        let request = requests[0].to_ascii_lowercase();
        assert!(request.contains("authorization: bearer test-key"));
        assert!(request.contains("response_format"));
        assert_eq!(output.suggestions.len(), 1);
        assert_eq!(
            output.suggestions[0].suggestion_type,
            SuggestionType::ClarifyRequirement
        );
        assert_eq!(output.suggestions[0].severity, SuggestionSeverity::High);
        assert_eq!(transport.attempts, 1);
        assert!(!transport.cache_hit);
        assert_eq!(
            transport.usage.as_ref().map(|usage| usage.total_tokens),
            Some(18)
        );
    }

    #[tokio::test]
    async fn openai_retries_when_content_fails_validation() {
        let report = requirement_fixture("specprobe-ai-openai-retry");
        let (base_url, handle) = spawn_chat_server(vec![
            (200, chat_response("这不是一段 JSON")),
            (200, chat_response(VALID_OUTPUT)),
        ]);
        let provider = OpenAiCompatibleProvider {
            api_key: Some("test-key".to_owned()),
            base_url,
            model: Some("test-model".to_owned()),
        };

        let (output, transport) = provider
            .analyze(&report, None)
            .await
            .expect("second round succeeds");

        let requests = handle.join().expect("server thread joins");
        assert_eq!(requests.len(), 2);
        assert!(requests[1].contains("rejected"));
        assert_eq!(transport.attempts, 2);
        assert_eq!(output.suggestions.len(), 1);
    }

    #[tokio::test]
    async fn openai_retries_transport_errors() {
        let report = requirement_fixture("specprobe-ai-openai-5xx");
        let (base_url, handle) = spawn_chat_server(vec![
            (500, r#"{"error":"boom"}"#.to_owned()),
            (200, chat_response(VALID_OUTPUT)),
        ]);
        let provider = OpenAiCompatibleProvider {
            api_key: Some("test-key".to_owned()),
            base_url,
            model: Some("test-model".to_owned()),
        };

        let (_, transport) = provider
            .analyze(&report, None)
            .await
            .expect("retry succeeds");

        let requests = handle.join().expect("server thread joins");
        assert_eq!(requests.len(), 2);
        assert_eq!(transport.attempts, 2);
    }

    #[tokio::test]
    async fn openai_auth_error_fails_without_retry() {
        let report = requirement_fixture("specprobe-ai-openai-401");
        let (base_url, handle) =
            spawn_chat_server(vec![(401, r#"{"error":"invalid key"}"#.to_owned())]);
        let provider = OpenAiCompatibleProvider {
            api_key: Some("bad-key".to_owned()),
            base_url,
            model: Some("test-model".to_owned()),
        };

        let error = provider
            .analyze(&report, None)
            .await
            .expect_err("auth error should fail fast");

        let requests = handle.join().expect("server thread joins");
        assert_eq!(requests.len(), 1);
        assert!(error.to_string().contains("401"));
    }

    #[tokio::test]
    async fn openai_cache_serves_second_call_without_network() {
        let report = requirement_fixture("specprobe-ai-openai-cache");
        let cache_dir = temp_project("specprobe-ai-cache-store");
        let cache = AiCache {
            dir: cache_dir.clone(),
        };
        let (base_url, handle) = spawn_chat_server(vec![(200, chat_response(VALID_OUTPUT))]);
        let provider = OpenAiCompatibleProvider {
            api_key: Some("test-key".to_owned()),
            base_url,
            model: Some("test-model".to_owned()),
        };

        let (_, first) = provider
            .analyze(&report, Some(&cache))
            .await
            .expect("first call succeeds");
        let (output, second) = provider
            .analyze(&report, Some(&cache))
            .await
            .expect("second call succeeds");

        handle.join().expect("server thread joins");
        assert!(!first.cache_hit);
        assert_eq!(first.attempts, 1);
        assert!(second.cache_hit);
        assert_eq!(second.attempts, 0);
        assert_eq!(output.suggestions.len(), 1);
        fs::remove_dir_all(cache_dir).expect("remove cache dir");
    }

    #[tokio::test]
    async fn ollama_transport_parses_output() {
        let report = requirement_fixture("specprobe-ai-ollama");
        let response = serde_json::json!({
            "message": {"role": "assistant", "content": VALID_OUTPUT},
            "prompt_eval_count": 3,
            "eval_count": 4,
        })
        .to_string();
        let (base_url, handle) = spawn_chat_server(vec![(200, response)]);
        let provider = OllamaProvider {
            base_url,
            model: Some("qwen-test".to_owned()),
        };

        let (output, transport) = provider
            .analyze(&report, None)
            .await
            .expect("ollama analysis succeeds");

        let requests = handle.join().expect("server thread joins");
        assert!(requests[0].contains("/api/chat"));
        assert_eq!(output.confidence, Confidence::Medium);
        assert_eq!(
            transport.usage.as_ref().map(|usage| usage.total_tokens),
            Some(7)
        );
    }

    #[test]
    fn parse_output_strips_markdown_fences() {
        let report = requirement_fixture("specprobe-ai-fence");
        let fenced = format!("```json\n{VALID_OUTPUT}\n```");

        let output = parse_model_output(&fenced, &report, false).expect("fenced json parses");

        assert_eq!(output.suggestions.len(), 1);
    }

    #[test]
    fn parse_output_validates_requirement_ids() {
        let report = requirement_fixture("specprobe-ai-unknown-id");
        let content = VALID_OUTPUT.replace("REQ-001", "REQ-999");

        let feedback =
            parse_model_output(&content, &report, false).expect_err("strict mode rejects");
        assert!(feedback.contains("REQ-999"));

        let output = parse_model_output(&content, &report, true).expect("lenient mode filters");
        assert!(output.suggestions.is_empty());
    }
}
