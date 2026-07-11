use std::fmt;
use std::path::{Path, PathBuf};

use serde::Serialize;
use thiserror::Error;

use crate::ai::{AiError, AiProviderKind};
use crate::browser::{
    BrowserDiagnosticSeverity, BrowserOptions, BrowserRunReport, run_browser_plan,
};
use crate::diagnosis::{Diagnosis, FailedFinding, generate_diagnoses};
use crate::refine::{RefineOptions, analyze_requirements_with_refinement};
use crate::requirements::{
    DiagnosticSeverity, QualityFlagKind, Requirement, RequirementError, RequirementQualityFlag,
    RequirementReport,
};
use crate::runtime::{
    LaunchReport, RuntimeDiagnosticSeverity, RuntimeError, launch_project, start_app,
};

#[derive(Debug, Error)]
pub enum ReviewError {
    #[error(transparent)]
    Requirements(#[from] RequirementError),
    #[error(transparent)]
    Ai(#[from] AiError),
}

#[derive(Debug, Clone, Default)]
pub struct ReviewOptions {
    pub project_path: PathBuf,
    pub base_url: String,
    pub provider: AiProviderKind,
    pub cache_dir: Option<PathBuf>,
    pub execute: bool,
    pub skip_launch: bool,
    pub skip_browser: bool,
    pub launch_timeout_secs: u64,
    pub browser_timeout_secs: u64,
}

#[derive(Debug, Serialize)]
pub struct ReviewReport {
    pub config: ReviewRunConfig,
    pub summary: ReviewSummary,
    pub requirement_report: RequirementReport,
    pub launch_report: Option<LaunchReport>,
    pub browser_report: Option<BrowserRunReport>,
    pub evidence: Vec<EvidenceItem>,
    pub issues: Vec<Issue>,
    /// LLM 深度诊断(带源码定位),仅在启用 AI Provider 且有可诊断的失败时非空。
    pub diagnoses: Vec<Diagnosis>,
}

#[derive(Debug, Serialize)]
pub struct ReviewRunConfig {
    pub requirements_source: String,
    pub project_root: String,
    pub base_url: String,
    pub execute: bool,
    pub launch_enabled: bool,
    pub browser_enabled: bool,
    pub launch_timeout_secs: u64,
    pub browser_timeout_secs: u64,
}

#[derive(Debug, Serialize)]
pub struct ReviewSummary {
    pub requirements: usize,
    pub test_cases: usize,
    pub evidence_items: usize,
    pub issues: usize,
    pub critical: usize,
    pub high: usize,
    pub medium: usize,
    pub low: usize,
    pub info: usize,
    pub pending_decisions: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvidenceItem {
    pub id: String,
    pub kind: ReviewEvidenceKind,
    pub status: EvidenceStatus,
    pub source: String,
    pub related_requirement: Option<String>,
    pub summary: String,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewEvidenceKind {
    RequirementQuality,
    RequirementDiagnostic,
    LaunchCommand,
    ProcessOutput,
    BrowserPlan,
    PageProbe,
    BrowserDiagnostic,
    ReviewDiagnostic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStatus {
    Pass,
    Fail,
    Warning,
    Info,
    Skipped,
}

#[derive(Debug, Clone, Serialize)]
pub struct Issue {
    pub id: String,
    pub severity: IssueSeverity,
    pub category: IssueCategory,
    pub title: String,
    pub related_requirement: Option<String>,
    pub expected: String,
    pub actual: String,
    pub evidence_ids: Vec<String>,
    pub recommendation: String,
    pub approval: ApprovalState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueSeverity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueCategory {
    RequirementGap,
    MissingExecutionPath,
    RuntimeFailure,
    BrowserFailure,
    MissingEvidence,
    ReviewConfiguration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalState {
    Pending,
    Accepted,
    Rejected,
    Ignored,
}

struct ReviewBuilder {
    evidence: Vec<EvidenceItem>,
    issues: Vec<Issue>,
}

impl ReviewBuilder {
    fn new() -> Self {
        Self {
            evidence: Vec::new(),
            issues: Vec::new(),
        }
    }

    fn add_evidence(
        &mut self,
        kind: ReviewEvidenceKind,
        status: EvidenceStatus,
        source: impl Into<String>,
        related_requirement: Option<String>,
        summary: impl Into<String>,
        detail: impl Into<String>,
    ) -> String {
        let id = format!("EV-{:03}", self.evidence.len() + 1);
        self.evidence.push(EvidenceItem {
            id: id.clone(),
            kind,
            status,
            source: source.into(),
            related_requirement,
            summary: summary.into(),
            detail: detail.into(),
        });
        id
    }

    fn add_issue(&mut self, draft: IssueDraft) {
        let id = format!("ISSUE-{:03}", self.issues.len() + 1);
        self.issues.push(Issue {
            id,
            severity: draft.severity,
            category: draft.category,
            title: draft.title,
            related_requirement: draft.related_requirement,
            expected: draft.expected,
            actual: draft.actual,
            evidence_ids: draft.evidence_ids,
            recommendation: draft.recommendation,
            approval: ApprovalState::Pending,
        });
    }
}

struct IssueDraft {
    severity: IssueSeverity,
    category: IssueCategory,
    title: String,
    related_requirement: Option<String>,
    expected: String,
    actual: String,
    evidence_ids: Vec<String>,
    recommendation: String,
}

pub async fn generate_review_report(
    requirements_path: &Path,
    options: ReviewOptions,
) -> Result<ReviewReport, ReviewError> {
    generate_review_report_with(requirements_path, options, &|_| {}).await
}

/// 带阶段进度回调的版本。`progress` 在各长阶段边界收到简短文本(供 spinner 显示)。
pub async fn generate_review_report_with(
    requirements_path: &Path,
    options: ReviewOptions,
    progress: &(dyn Fn(&str) + Sync),
) -> Result<ReviewReport, ReviewError> {
    progress("Analyzing requirements…");
    let requirement_report = analyze_requirements_with_refinement(
        requirements_path,
        RefineOptions {
            provider: options.provider,
            cache_dir: options.cache_dir.clone(),
        },
    )
    .await?;
    let config = ReviewRunConfig {
        requirements_source: requirement_report.source.clone(),
        project_root: display_path(&options.project_path),
        base_url: options.base_url.clone(),
        execute: options.execute,
        launch_enabled: !options.skip_launch,
        browser_enabled: !options.skip_browser,
        launch_timeout_secs: options.launch_timeout_secs,
        browser_timeout_secs: options.browser_timeout_secs,
    };

    let mut builder = ReviewBuilder::new();
    collect_requirement_evidence(&mut builder, &requirement_report);

    // 完整执行(启动 + 浏览器)时用 ManagedApp 编排:起服务 → 等就绪 → 跑浏览器 → 关停。
    // 其余组合(计划级、单独跳过某一步)保留原独立路径。
    let orchestrate = options.execute && !options.skip_launch && !options.skip_browser;

    let (launch_report, browser_report) = if orchestrate {
        run_orchestrated(&mut builder, requirements_path, &options, progress).await
    } else {
        let launch_report = if options.skip_launch {
            builder.add_evidence(
                ReviewEvidenceKind::ReviewDiagnostic,
                EvidenceStatus::Skipped,
                "review",
                None,
                "Project launch evidence was skipped.",
                "Use review without --skip-launch to include launch command detection or execution.",
            );
            None
        } else {
            let launch = launch_project(
                &options.project_path,
                options.launch_timeout_secs,
                !options.execute,
            )
            .await;
            collect_launch_evidence(&mut builder, launch)
        };

        let browser_report = if options.skip_browser {
            builder.add_evidence(
                ReviewEvidenceKind::ReviewDiagnostic,
                EvidenceStatus::Skipped,
                "review",
                None,
                "Browser evidence was skipped.",
                "Use review without --skip-browser to include browser action planning or page probing.",
            );
            None
        } else {
            let browser = run_browser_plan(
                requirements_path,
                &options.base_url,
                options.browser_timeout_secs,
                !options.execute,
                BrowserOptions {
                    provider: options.provider,
                    cache_dir: options.cache_dir.clone(),
                },
            )
            .await;
            collect_browser_evidence(&mut builder, browser)
        };

        (launch_report, browser_report)
    };

    // LLM 深度诊断:对运行期失败(启动/浏览器)叠加带源码定位的根因分析。
    // findings 先 owned 收集,避免与后续 &mut builder 借用冲突。
    let findings: Vec<FailedFinding> = builder
        .issues
        .iter()
        .filter(|issue| is_diagnosable(issue))
        .map(|issue| FailedFinding {
            issue_id: issue.id.clone(),
            title: issue.title.clone(),
            expected: issue.expected.clone(),
            actual: issue.actual.clone(),
        })
        .collect();
    let diagnoses = if matches!(options.provider, AiProviderKind::Mock) || findings.is_empty() {
        Vec::new()
    } else {
        progress("Diagnosing failures…");
        match generate_diagnoses(
            &findings,
            &options.project_path,
            options.provider,
            options.cache_dir.clone(),
        )
        .await
        {
            Ok(diagnoses) => diagnoses,
            Err(error) => {
                builder.add_evidence(
                    ReviewEvidenceKind::ReviewDiagnostic,
                    EvidenceStatus::Warning,
                    "diagnosis",
                    None,
                    "AI defect diagnosis was skipped.",
                    error.to_string(),
                );
                Vec::new()
            }
        }
    };

    let summary = build_summary(
        &requirement_report,
        builder.evidence.len(),
        builder.issues.as_slice(),
    );

    Ok(ReviewReport {
        config,
        summary,
        requirement_report,
        launch_report,
        browser_report,
        evidence: builder.evidence,
        issues: builder.issues,
        diagnoses,
    })
}

/// 可做源码级诊断的失败:运行期(启动 / 浏览器)的高严重度问题。
/// 需求文档类问题(RequirementGap 等)不指向代码,不诊断。
fn is_diagnosable(issue: &Issue) -> bool {
    matches!(
        issue.category,
        IssueCategory::RuntimeFailure | IssueCategory::BrowserFailure
    ) && matches!(
        issue.severity,
        IssueSeverity::High | IssueSeverity::Critical
    )
}

/// 用 ManagedApp 编排完整执行:启动被测服务、探测就绪、运行浏览器、优雅关停。
/// 启动失败时记录 launch 错误并仍尝试浏览器(用户可能已自行启动服务)。
async fn run_orchestrated(
    builder: &mut ReviewBuilder,
    requirements_path: &Path,
    options: &ReviewOptions,
    progress: &(dyn Fn(&str) + Sync),
) -> (Option<LaunchReport>, Option<BrowserRunReport>) {
    let browser_options = BrowserOptions {
        provider: options.provider,
        cache_dir: options.cache_dir.clone(),
    };
    match start_app(&options.project_path).await {
        Ok(mut app) => {
            progress("Starting service, waiting until ready…");
            app.wait_until_ready(Some(&options.base_url), options.launch_timeout_secs)
                .await;
            progress("Running browser tests…");
            let browser = run_browser_plan(
                requirements_path,
                &options.base_url,
                options.browser_timeout_secs,
                false,
                browser_options,
            )
            .await;
            let launch_report = app.shutdown().await;
            let launch = collect_launch_evidence(builder, Ok(launch_report));
            let browser_report = collect_browser_evidence(builder, browser);
            (launch, browser_report)
        }
        Err(error) => {
            let launch = collect_launch_evidence(builder, Err(error));
            progress("Running browser tests…");
            let browser = run_browser_plan(
                requirements_path,
                &options.base_url,
                options.browser_timeout_secs,
                false,
                browser_options,
            )
            .await;
            let browser_report = collect_browser_evidence(builder, browser);
            (launch, browser_report)
        }
    }
}

fn collect_requirement_evidence(builder: &mut ReviewBuilder, report: &RequirementReport) {
    if report.requirements.is_empty() {
        let evidence_id = builder.add_evidence(
            ReviewEvidenceKind::RequirementDiagnostic,
            EvidenceStatus::Fail,
            &report.source,
            None,
            "No requirements were extracted.",
            "SpecProbe cannot build meaningful tests until at least one requirement is available.",
        );
        builder.add_issue(IssueDraft {
            severity: IssueSeverity::High,
            category: IssueCategory::RequirementGap,
            title: "未提取到可测试需求".to_owned(),
            related_requirement: None,
            expected: "需求文档应该包含可识别、可验证的软件需求。".to_owned(),
            actual: "当前输入没有产生任何结构化需求。".to_owned(),
            evidence_ids: vec![evidence_id],
            recommendation: "补充带有“必须、应该、支持、显示、返回”等可识别表达的需求条目。"
                .to_owned(),
        });
    }

    for diagnostic in &report.diagnostics {
        let status = match diagnostic.severity {
            DiagnosticSeverity::Info => EvidenceStatus::Info,
            DiagnosticSeverity::Warning => EvidenceStatus::Warning,
        };
        builder.add_evidence(
            ReviewEvidenceKind::RequirementDiagnostic,
            status,
            &report.source,
            None,
            diagnostic.message.clone(),
            "Requirement parser diagnostic.",
        );
    }

    for requirement in &report.requirements {
        for flag in &requirement.quality_flags {
            collect_quality_flag(builder, requirement, flag);
        }
    }
}

fn collect_quality_flag(
    builder: &mut ReviewBuilder,
    requirement: &Requirement,
    flag: &RequirementQualityFlag,
) {
    let source = format!("{}:{}", requirement.source.path, requirement.source.line);
    let evidence_id = builder.add_evidence(
        ReviewEvidenceKind::RequirementQuality,
        EvidenceStatus::Warning,
        source,
        Some(requirement.id.clone()),
        flag.message.clone(),
        requirement.description.clone(),
    );

    builder.add_issue(IssueDraft {
        severity: severity_for_quality_flag(flag.kind),
        category: IssueCategory::RequirementGap,
        title: format!("{} 的验收条件不够明确", requirement.id),
        related_requirement: Some(requirement.id.clone()),
        expected: "需求应该能被确定性测试步骤验证，并产生明确的运行证据。".to_owned(),
        actual: flag.message.clone(),
        evidence_ids: vec![evidence_id],
        recommendation: recommendation_for_quality_flag(flag.kind).to_owned(),
    });
}

fn collect_launch_evidence(
    builder: &mut ReviewBuilder,
    launch: Result<LaunchReport, RuntimeError>,
) -> Option<LaunchReport> {
    match launch {
        Ok(report) => {
            let command_line = format!(
                "{} {}",
                report.command.program,
                report.command.args.join(" ")
            )
            .trim()
            .to_owned();
            let command_evidence = builder.add_evidence(
                ReviewEvidenceKind::LaunchCommand,
                launch_status(&report),
                &report.project_root,
                None,
                format!("Launch command: {command_line}"),
                format!(
                    "adapter={}, source={}, dry_run={}, success={}, timed_out={}, long_running={}",
                    report.adapter,
                    report.command.source,
                    report.execution.dry_run,
                    report.execution.success,
                    report.execution.timed_out,
                    report.execution.long_running
                ),
            );

            let mut evidence_ids = vec![command_evidence];
            if !report.stdout_excerpt.is_empty() {
                evidence_ids.push(builder.add_evidence(
                    ReviewEvidenceKind::ProcessOutput,
                    EvidenceStatus::Info,
                    "stdout",
                    None,
                    "Process stdout was captured.",
                    report.stdout_excerpt.clone(),
                ));
            }
            if !report.stderr_excerpt.is_empty() {
                evidence_ids.push(builder.add_evidence(
                    ReviewEvidenceKind::ProcessOutput,
                    EvidenceStatus::Warning,
                    "stderr",
                    None,
                    "Process stderr was captured.",
                    report.stderr_excerpt.clone(),
                ));
            }

            for diagnostic in &report.diagnostics {
                let status = match diagnostic.severity {
                    RuntimeDiagnosticSeverity::Info => EvidenceStatus::Info,
                    RuntimeDiagnosticSeverity::Warning => EvidenceStatus::Warning,
                    RuntimeDiagnosticSeverity::Error => EvidenceStatus::Fail,
                };
                evidence_ids.push(builder.add_evidence(
                    ReviewEvidenceKind::ReviewDiagnostic,
                    status,
                    "launch",
                    None,
                    diagnostic.message.clone(),
                    "Launch diagnostic.",
                ));
            }

            if report.execution.attempted && !report.execution.success {
                builder.add_issue(IssueDraft {
                    severity: IssueSeverity::High,
                    category: IssueCategory::RuntimeFailure,
                    title: "项目启动或运行命令失败".to_owned(),
                    related_requirement: None,
                    expected: "被测项目应该能在受控超时时间内成功启动或完成命令。".to_owned(),
                    actual: launch_actual_result(&report),
                    evidence_ids,
                    recommendation:
                        "先修复启动脚本、依赖安装或运行时异常；后续浏览器测试依赖可运行的被测项目。"
                            .to_owned(),
                });
            }

            Some(report)
        }
        Err(error) => {
            let evidence_id = builder.add_evidence(
                ReviewEvidenceKind::ReviewDiagnostic,
                EvidenceStatus::Fail,
                "launch",
                None,
                "Project launch command could not be prepared.",
                error.to_string(),
            );
            let (severity, category, recommendation) = match &error {
                RuntimeError::NoLaunchCommand(_) => (
                    IssueSeverity::Medium,
                    IssueCategory::MissingExecutionPath,
                    "补充 package.json scripts、Cargo.toml、app.py/main.py，或后续提供显式启动命令配置。",
                ),
                RuntimeError::NotFound(_)
                | RuntimeError::NotDirectory(_)
                | RuntimeError::Io { .. } => (
                    IssueSeverity::High,
                    IssueCategory::ReviewConfiguration,
                    "检查 --project 指向的路径是否存在、是否为项目目录，以及当前用户是否有访问权限。",
                ),
            };
            builder.add_issue(IssueDraft {
                severity,
                category,
                title: "无法准备项目启动证据".to_owned(),
                related_requirement: None,
                expected: "审查流程应该能够识别或准备被测项目的启动方式。".to_owned(),
                actual: error.to_string(),
                evidence_ids: vec![evidence_id],
                recommendation: recommendation.to_owned(),
            });
            None
        }
    }
}

fn collect_browser_evidence(
    builder: &mut ReviewBuilder,
    browser: Result<BrowserRunReport, crate::browser::BrowserError>,
) -> Option<BrowserRunReport> {
    match browser {
        Ok(report) => {
            let plan_evidence = builder.add_evidence(
                ReviewEvidenceKind::BrowserPlan,
                if report.plan.cases.is_empty() {
                    EvidenceStatus::Warning
                } else {
                    EvidenceStatus::Info
                },
                &report.base_url,
                None,
                format!(
                    "Browser action plan contains {} case(s).",
                    report.plan.cases.len()
                ),
                "Generated from the requirement test plan.",
            );

            if report.plan.cases.is_empty() {
                builder.add_issue(IssueDraft {
                    severity: IssueSeverity::Medium,
                    category: IssueCategory::MissingEvidence,
                    title: "没有生成浏览器测试用例".to_owned(),
                    related_requirement: None,
                    expected: "至少一部分需求应该能映射为可执行或可审查的浏览器测试步骤。"
                        .to_owned(),
                    actual: "当前需求没有生成浏览器动作计划。".to_owned(),
                    evidence_ids: vec![plan_evidence.clone()],
                    recommendation: "检查需求是否描述了可观察的页面、交互、接口响应或用户流程。"
                        .to_owned(),
                });
            }

            let mut evidence_ids = vec![plan_evidence];
            if let Some(page) = &report.page {
                evidence_ids.push(builder.add_evidence(
                    ReviewEvidenceKind::PageProbe,
                    if report.execution.success {
                        EvidenceStatus::Pass
                    } else {
                        EvidenceStatus::Fail
                    },
                    &page.url,
                    None,
                    format!("Page probe status: {}", page.status_text),
                    format!(
                        "title={}, bytes={}, excerpt={}",
                        page.title.as_deref().unwrap_or("none"),
                        page.response_bytes,
                        page.body_excerpt
                    ),
                ));
            }

            for diagnostic in &report.diagnostics {
                let status = match diagnostic.severity {
                    BrowserDiagnosticSeverity::Info => EvidenceStatus::Info,
                    BrowserDiagnosticSeverity::Warning => EvidenceStatus::Warning,
                    BrowserDiagnosticSeverity::Error => EvidenceStatus::Fail,
                };
                evidence_ids.push(builder.add_evidence(
                    ReviewEvidenceKind::BrowserDiagnostic,
                    status,
                    "browser",
                    None,
                    diagnostic.message.clone(),
                    "Browser executor diagnostic.",
                ));
            }

            // 仅当没有具体场景结果时,才生成这条笼统的浏览器失败问题;
            // 有场景时由更精确的场景失败问题(collect_scenario_evidence)覆盖,避免冗余与超长拼接。
            if report.execution.attempted
                && !report.execution.success
                && report.scenarios.is_empty()
            {
                builder.add_issue(IssueDraft {
                    severity: IssueSeverity::High,
                    category: IssueCategory::BrowserFailure,
                    title: "浏览器页面探测失败".to_owned(),
                    related_requirement: None,
                    expected: "页面应该能通过基础 URL 返回 2xx 或 3xx HTTP 状态，供后续交互测试执行。".to_owned(),
                    actual: browser_actual_result(&report),
                    evidence_ids,
                    recommendation: "确认被测项目已经启动、base URL 和端口正确，并优先修复 HTTP 错误或连接失败。"
                        .to_owned(),
                });
            }

            collect_playwright_evidence(builder, &report);
            collect_scenario_evidence(builder, &report);
            Some(report)
        }
        Err(error) => {
            let evidence_id = builder.add_evidence(
                ReviewEvidenceKind::ReviewDiagnostic,
                EvidenceStatus::Fail,
                "browser",
                None,
                "Browser report could not be generated.",
                error.to_string(),
            );
            builder.add_issue(IssueDraft {
                severity: IssueSeverity::High,
                category: IssueCategory::BrowserFailure,
                title: "无法生成浏览器证据".to_owned(),
                related_requirement: None,
                expected: "审查流程应该能生成浏览器动作计划或页面探测证据。".to_owned(),
                actual: error.to_string(),
                evidence_ids: vec![evidence_id],
                recommendation: "检查需求输入路径和 base URL；若使用 HTTPS 或复杂浏览器能力，后续需要 Playwright 后端。"
                    .to_owned(),
            });
            None
        }
    }
}

/// 把 Playwright 深度执行证据(DOM 摘要、网络失败、页面脚本错误、console 错误)
/// 转化为证据项与问题。网络失败与页面错误各聚合为一个高严重度 Issue。
fn collect_playwright_evidence(builder: &mut ReviewBuilder, report: &BrowserRunReport) {
    let Some(evidence) = &report.playwright else {
        return;
    };
    let outcome = &evidence.outcome;

    if let Some(snapshot) = &outcome.snapshot {
        builder.add_evidence(
            ReviewEvidenceKind::PageProbe,
            EvidenceStatus::Info,
            &report.base_url,
            None,
            format!(
                "Playwright captured page '{}' with {} interactive element(s).",
                snapshot.title.as_deref().unwrap_or("(no title)"),
                snapshot.interactive.len()
            ),
            format!("Evidence archived in {}.", evidence.run_dir),
        );
    }

    if !outcome.network_failures.is_empty() {
        // 去重:同一 (url, status) 只列一次,附出现次数,避免重复请求刷屏。
        let mut unique: Vec<(String, usize)> = Vec::new();
        for failure in &outcome.network_failures {
            let status = failure
                .status
                .map(|code| format!(" (HTTP {code})"))
                .unwrap_or_default();
            let label = format!("{}{status}", failure.url);
            if let Some(entry) = unique.iter_mut().find(|(existing, _)| *existing == label) {
                entry.1 += 1;
            } else {
                unique.push((label, 1));
            }
        }
        let detail = unique
            .iter()
            .take(10)
            .map(|(label, count)| {
                if *count > 1 {
                    format!("{label} x{count}")
                } else {
                    label.clone()
                }
            })
            .collect::<Vec<_>>()
            .join("; ");
        let evidence_id = builder.add_evidence(
            ReviewEvidenceKind::BrowserDiagnostic,
            EvidenceStatus::Fail,
            &report.base_url,
            None,
            format!(
                "Browser observed {} distinct network failure(s) ({} total requests).",
                unique.len(),
                outcome.network_failures.len()
            ),
            detail.clone(),
        );
        builder.add_issue(IssueDraft {
            severity: IssueSeverity::High,
            category: IssueCategory::BrowserFailure,
            title: "页面运行时出现网络请求失败".to_owned(),
            related_requirement: None,
            expected: "页面加载与交互过程中的关键请求应返回成功状态。".to_owned(),
            actual: detail,
            evidence_ids: vec![evidence_id],
            recommendation: "检查失败请求对应的后端接口或资源路径，优先修复 4xx/5xx 响应。"
                .to_owned(),
        });
    }

    if !outcome.page_errors.is_empty() {
        let detail = outcome.page_errors.join("; ");
        let evidence_id = builder.add_evidence(
            ReviewEvidenceKind::BrowserDiagnostic,
            EvidenceStatus::Fail,
            "browser",
            None,
            format!(
                "Browser reported {} page script error(s).",
                outcome.page_errors.len()
            ),
            detail.clone(),
        );
        builder.add_issue(IssueDraft {
            severity: IssueSeverity::High,
            category: IssueCategory::BrowserFailure,
            title: "页面运行时抛出脚本错误".to_owned(),
            related_requirement: None,
            expected: "页面在加载和交互过程中不应抛出未处理的 JavaScript 错误。".to_owned(),
            actual: detail,
            evidence_ids: vec![evidence_id],
            recommendation: "根据错误堆栈定位前端脚本缺陷并修复。".to_owned(),
        });
    }

    let console_errors: Vec<&str> = outcome
        .console
        .iter()
        .filter(|message| message.level == "error")
        .map(|message| message.text.as_str())
        .collect();
    if !console_errors.is_empty() {
        builder.add_evidence(
            ReviewEvidenceKind::BrowserDiagnostic,
            EvidenceStatus::Warning,
            "console",
            None,
            format!(
                "Browser console reported {} error message(s).",
                console_errors.len()
            ),
            console_errors
                .iter()
                .take(10)
                .copied()
                .collect::<Vec<_>>()
                .join("; "),
        );
    }
}

/// 把 LLM 生成并执行的浏览器场景结果转化为证据与问题:每个失败场景(有步骤未通过)
/// 关联对应需求生成一个高严重度 Issue,成功场景记为通过证据。
fn collect_scenario_evidence(builder: &mut ReviewBuilder, report: &BrowserRunReport) {
    for scenario in &report.scenarios {
        let failed: Vec<&crate::browser::ScenarioStepReport> =
            scenario.steps.iter().filter(|step| !step.ok).collect();
        let status = if scenario.success {
            EvidenceStatus::Pass
        } else {
            EvidenceStatus::Fail
        };
        let detail = scenario
            .steps
            .iter()
            .map(|step| {
                format!(
                    "{} {} -> {}",
                    step.action,
                    step.target,
                    if step.ok { "ok" } else { "FAIL" }
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        let evidence_id = builder.add_evidence(
            ReviewEvidenceKind::PageProbe,
            status,
            &report.base_url,
            Some(scenario.requirement_id.clone()),
            format!(
                "Browser scenario '{}' {}.",
                scenario.title,
                if scenario.success { "passed" } else { "failed" }
            ),
            detail,
        );

        if !scenario.success {
            let failed_detail = failed
                .iter()
                .map(|step| {
                    format!(
                        "{} {}: {}",
                        step.action,
                        step.target,
                        step.detail.as_deref().unwrap_or("assertion failed")
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            builder.add_issue(IssueDraft {
                severity: IssueSeverity::High,
                category: IssueCategory::BrowserFailure,
                title: format!("交互场景未通过：{}", scenario.title),
                related_requirement: Some(scenario.requirement_id.clone()),
                expected: if scenario.expected_observation.is_empty() {
                    "页面交互后应产生与需求一致的可观察结果。".to_owned()
                } else {
                    scenario.expected_observation.clone()
                },
                actual: failed_detail,
                evidence_ids: vec![evidence_id],
                recommendation: "根据失败的操作或断言定位前端逻辑缺陷并修复。".to_owned(),
            });
        }
    }
}

fn build_summary(
    report: &RequirementReport,
    evidence_items: usize,
    issues: &[Issue],
) -> ReviewSummary {
    ReviewSummary {
        requirements: report.requirements.len(),
        test_cases: report.test_plan.cases.len(),
        evidence_items,
        issues: issues.len(),
        critical: count_severity(issues, IssueSeverity::Critical),
        high: count_severity(issues, IssueSeverity::High),
        medium: count_severity(issues, IssueSeverity::Medium),
        low: count_severity(issues, IssueSeverity::Low),
        info: count_severity(issues, IssueSeverity::Info),
        pending_decisions: issues
            .iter()
            .filter(|issue| issue.approval == ApprovalState::Pending)
            .count(),
    }
}

fn count_severity(issues: &[Issue], severity: IssueSeverity) -> usize {
    issues
        .iter()
        .filter(|issue| issue.severity == severity)
        .count()
}

fn severity_for_quality_flag(kind: QualityFlagKind) -> IssueSeverity {
    match kind {
        QualityFlagKind::VagueLanguage | QualityFlagKind::TooBroad => IssueSeverity::High,
        QualityFlagKind::MissingObservableResult => IssueSeverity::Medium,
    }
}

fn recommendation_for_quality_flag(kind: QualityFlagKind) -> &'static str {
    match kind {
        QualityFlagKind::VagueLanguage => {
            "把模糊词替换为可观察标准，例如具体页面元素、响应字段、时间阈值、错误提示或状态变化。"
        }
        QualityFlagKind::MissingObservableResult => {
            "补充用户完成操作后应看到的输出、状态变化、日志记录或接口响应，方便自动测试断言。"
        }
        QualityFlagKind::TooBroad => {
            "将该需求拆成多个独立需求，每个需求只描述一个动作、一个结果和一组验收证据。"
        }
    }
}

fn launch_status(report: &LaunchReport) -> EvidenceStatus {
    if report.execution.dry_run {
        EvidenceStatus::Info
    } else if report.execution.success {
        EvidenceStatus::Pass
    } else if report.execution.timed_out {
        EvidenceStatus::Warning
    } else {
        EvidenceStatus::Fail
    }
}

fn launch_actual_result(report: &LaunchReport) -> String {
    format!(
        "success={}, timed_out={}, exit_code={}, duration={}ms",
        report.execution.success,
        report.execution.timed_out,
        report
            .execution
            .exit_code
            .map(|code| code.to_string())
            .unwrap_or_else(|| "none".to_owned()),
        report.execution.duration_ms
    )
}

fn browser_actual_result(report: &BrowserRunReport) -> String {
    report
        .page
        .as_ref()
        .map(|page| {
            format!(
                "status={}, status_line={}",
                page.status_code
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "unknown".to_owned()),
                page.status_text
            )
        })
        .unwrap_or_else(|| {
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        })
}

fn display_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    value.strip_prefix(r"\\?\").unwrap_or(&value).to_owned()
}

impl fmt::Display for ReviewEvidenceKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RequirementQuality => "requirement-quality",
            Self::RequirementDiagnostic => "requirement-diagnostic",
            Self::LaunchCommand => "launch-command",
            Self::ProcessOutput => "process-output",
            Self::BrowserPlan => "browser-plan",
            Self::PageProbe => "page-probe",
            Self::BrowserDiagnostic => "browser-diagnostic",
            Self::ReviewDiagnostic => "review-diagnostic",
        })
    }
}

impl fmt::Display for EvidenceStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Warning => "warning",
            Self::Info => "info",
            Self::Skipped => "skipped",
        })
    }
}

impl fmt::Display for IssueSeverity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Critical => "critical",
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
            Self::Info => "info",
        })
    }
}

impl fmt::Display for IssueCategory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RequirementGap => "requirement-gap",
            Self::MissingExecutionPath => "missing-execution-path",
            Self::RuntimeFailure => "runtime-failure",
            Self::BrowserFailure => "browser-failure",
            Self::MissingEvidence => "missing-evidence",
            Self::ReviewConfiguration => "review-configuration",
        })
    }
}

impl fmt::Display for ApprovalState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Pending => "pending",
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::Ignored => "ignored",
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        ApprovalState, IssueCategory, IssueSeverity, ReviewOptions, generate_review_report,
    };

    #[tokio::test]
    async fn review_report_includes_requirement_quality_issue() {
        let root = temp_project("specprobe-review-quality");
        let requirements = root.join("PRD.md");
        fs::write(&requirements, "- 页面应该简单友好。").expect("write requirements");

        let report = generate_review_report(
            &requirements,
            ReviewOptions {
                project_path: root.clone(),
                base_url: "http://127.0.0.1:3000".to_owned(),
                provider: Default::default(),
                cache_dir: None,
                execute: false,
                skip_launch: true,
                skip_browser: true,
                launch_timeout_secs: 1,
                browser_timeout_secs: 1,
            },
        )
        .await
        .expect("review succeeds");

        assert_eq!(report.summary.requirements, 1);
        assert!(report.issues.iter().any(|issue| {
            issue.severity == IssueSeverity::High
                && issue.category == IssueCategory::RequirementGap
                && issue.approval == ApprovalState::Pending
        }));
        fs::remove_dir_all(root).expect("remove test project");
    }

    #[tokio::test]
    async fn browser_probe_failure_becomes_issue_when_executed() {
        let root = temp_project("specprobe-review-browser");
        let requirements = root.join("PRD.md");
        fs::write(&requirements, "- 页面必须显示首页标题。").expect("write requirements");

        let report = generate_review_report(
            &requirements,
            ReviewOptions {
                project_path: root.clone(),
                base_url: "http://127.0.0.1:9".to_owned(),
                provider: Default::default(),
                cache_dir: None,
                execute: true,
                skip_launch: true,
                skip_browser: false,
                launch_timeout_secs: 1,
                browser_timeout_secs: 1,
            },
        )
        .await
        .expect("review succeeds");

        assert!(report.browser_report.is_some());
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.category == IssueCategory::BrowserFailure)
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
