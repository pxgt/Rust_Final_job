use std::fmt;
use std::path::{Path, PathBuf};

use serde::Serialize;
use thiserror::Error;

use crate::review::{
    ApprovalState, Issue, IssueCategory, ReviewError, ReviewOptions, ReviewReport,
    generate_review_report,
};

#[derive(Debug, Error)]
pub enum RemediationError {
    #[error(transparent)]
    Review(#[from] ReviewError),
}

#[derive(Debug, Clone, Default)]
pub struct RemediationOptions {
    pub project_path: PathBuf,
    pub base_url: String,
    pub provider: crate::ai::AiProviderKind,
    pub cache_dir: Option<PathBuf>,
    pub execute: bool,
    pub skip_launch: bool,
    pub skip_browser: bool,
    pub launch_timeout_secs: u64,
    pub browser_timeout_secs: u64,
}

#[derive(Debug, Serialize)]
pub struct RemediationReport {
    pub review: ReviewReport,
    pub summary: RemediationSummary,
    pub proposals: Vec<PatchProposal>,
    pub regression_plan: RegressionPlan,
}

#[derive(Debug, Serialize)]
pub struct RemediationSummary {
    pub issues: usize,
    pub proposals: usize,
    pub patch_previews: usize,
    pub regression_checks: usize,
    pub auto_apply_supported: bool,
    pub requires_user_approval: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PatchProposal {
    pub id: String,
    pub issue_id: String,
    pub approval: ApprovalState,
    pub strategy: PatchStrategy,
    pub safety: PatchSafety,
    pub title: String,
    pub target_files: Vec<String>,
    pub rationale: String,
    pub steps: Vec<String>,
    pub patch_preview: Option<String>,
    pub risk_notes: Vec<String>,
    pub regression_checks: Vec<RegressionCheck>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PatchStrategy {
    RequirementClarification,
    LaunchConfiguration,
    RuntimeFix,
    BrowserConfiguration,
    EvidenceExpansion,
    ReviewConfiguration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PatchSafety {
    PreviewOnly,
    ManualPatch,
    NeedsDeveloperInput,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegressionPlan {
    pub checks: Vec<RegressionCheck>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegressionCheck {
    pub id: String,
    pub proposal_id: Option<String>,
    pub command: String,
    pub reason: String,
    pub required: bool,
}

struct ProposalBuilder<'a> {
    review: &'a ReviewReport,
    proposals: Vec<PatchProposal>,
    regression_checks: Vec<RegressionCheck>,
}

impl<'a> ProposalBuilder<'a> {
    fn new(review: &'a ReviewReport) -> Self {
        Self {
            review,
            proposals: Vec::new(),
            regression_checks: Vec::new(),
        }
    }

    fn build(mut self) -> (Vec<PatchProposal>, RegressionPlan) {
        for issue in &self.review.issues {
            let proposal_id = format!("PATCH-{:03}", self.proposals.len() + 1);
            let mut proposal = build_patch_proposal(self.review, issue, proposal_id.clone());
            let checks = regression_checks_for(self.review, issue, &proposal_id);
            self.regression_checks.extend(checks.clone());
            proposal.regression_checks = checks;
            self.proposals.push(proposal);
        }

        self.regression_checks
            .extend(global_regression_checks(self.review));

        (
            self.proposals,
            RegressionPlan {
                checks: self.regression_checks,
            },
        )
    }
}

pub async fn generate_remediation_report(
    requirements_path: &Path,
    options: RemediationOptions,
) -> Result<RemediationReport, RemediationError> {
    let review = generate_review_report(
        requirements_path,
        ReviewOptions {
            project_path: options.project_path,
            base_url: options.base_url,
            provider: options.provider,
            cache_dir: options.cache_dir,
            execute: options.execute,
            skip_launch: options.skip_launch,
            skip_browser: options.skip_browser,
            launch_timeout_secs: options.launch_timeout_secs,
            browser_timeout_secs: options.browser_timeout_secs,
        },
    )
    .await?;

    let (proposals, regression_plan) = ProposalBuilder::new(&review).build();
    let summary = RemediationSummary {
        issues: review.issues.len(),
        proposals: proposals.len(),
        patch_previews: proposals
            .iter()
            .filter(|proposal| proposal.patch_preview.is_some())
            .count(),
        regression_checks: regression_plan.checks.len(),
        auto_apply_supported: false,
        requires_user_approval: proposals
            .iter()
            .any(|proposal| proposal.approval == ApprovalState::Pending),
    };

    Ok(RemediationReport {
        review,
        summary,
        proposals,
        regression_plan,
    })
}

fn build_patch_proposal(
    review: &ReviewReport,
    issue: &Issue,
    proposal_id: String,
) -> PatchProposal {
    let strategy = strategy_for_issue(issue.category);
    let target_files = target_files_for(review, issue, strategy);
    let patch_preview = patch_preview_for(review, issue, strategy);
    let safety = safety_for(strategy, patch_preview.as_ref());

    PatchProposal {
        id: proposal_id,
        issue_id: issue.id.clone(),
        approval: ApprovalState::Pending,
        strategy,
        safety,
        title: format!("修复 {}", issue.title),
        target_files,
        rationale: issue.recommendation.clone(),
        steps: steps_for_issue(issue, strategy),
        patch_preview,
        risk_notes: risk_notes_for(strategy),
        regression_checks: Vec::new(),
    }
}

fn strategy_for_issue(category: IssueCategory) -> PatchStrategy {
    match category {
        IssueCategory::RequirementGap => PatchStrategy::RequirementClarification,
        IssueCategory::MissingExecutionPath => PatchStrategy::LaunchConfiguration,
        IssueCategory::RuntimeFailure => PatchStrategy::RuntimeFix,
        IssueCategory::BrowserFailure => PatchStrategy::BrowserConfiguration,
        IssueCategory::MissingEvidence => PatchStrategy::EvidenceExpansion,
        IssueCategory::ReviewConfiguration => PatchStrategy::ReviewConfiguration,
    }
}

fn target_files_for(review: &ReviewReport, issue: &Issue, strategy: PatchStrategy) -> Vec<String> {
    if let Some(requirement) = issue.related_requirement.as_ref().and_then(|id| {
        review
            .requirement_report
            .requirements
            .iter()
            .find(|requirement| &requirement.id == id)
    }) {
        return vec![requirement.source.path.clone()];
    }

    match strategy {
        PatchStrategy::RequirementClarification | PatchStrategy::EvidenceExpansion => {
            vec![review.config.requirements_source.clone()]
        }
        PatchStrategy::LaunchConfiguration => vec![
            "package.json".to_owned(),
            "Cargo.toml".to_owned(),
            "app.py or main.py".to_owned(),
        ],
        PatchStrategy::RuntimeFix => {
            vec!["project startup scripts and runtime entrypoint".to_owned()]
        }
        PatchStrategy::BrowserConfiguration => {
            vec!["frontend route, dev server config or base URL setting".to_owned()]
        }
        PatchStrategy::ReviewConfiguration => vec!["SpecProbe command arguments".to_owned()],
    }
}

fn patch_preview_for(
    review: &ReviewReport,
    issue: &Issue,
    strategy: PatchStrategy,
) -> Option<String> {
    match strategy {
        PatchStrategy::RequirementClarification | PatchStrategy::EvidenceExpansion => {
            requirement_patch_preview(review, issue)
        }
        PatchStrategy::LaunchConfiguration => Some(launch_config_patch_preview()),
        _ => None,
    }
}

fn requirement_patch_preview(review: &ReviewReport, issue: &Issue) -> Option<String> {
    let requirement = issue.related_requirement.as_ref().and_then(|id| {
        review
            .requirement_report
            .requirements
            .iter()
            .find(|requirement| &requirement.id == id)
    })?;
    let old_line = requirement.description.trim();
    let new_line = suggested_requirement_line(old_line, issue);

    Some(format!(
        "--- a/{path}\n+++ b/{path}\n@@ line {line} @@\n- {old_line}\n+ {new_line}\n",
        path = requirement.source.path,
        line = requirement.source.line
    ))
}

fn suggested_requirement_line(old_line: &str, issue: &Issue) -> String {
    let trimmed = old_line.trim_end_matches(['。', '.', ';', '；']);
    if issue.actual.contains("缺少明确") {
        format!("{trimmed}；验收时应明确显示、返回或记录可观察结果，并采集对应运行证据。")
    } else if issue.actual.contains("模糊") || issue.title.contains("不够明确") {
        format!("{trimmed}；验收标准应补充量化条件、目标元素或明确状态变化。")
    } else {
        format!("{trimmed}；补充可验证的验收标准和证据要求。")
    }
}

fn launch_config_patch_preview() -> String {
    [
        "--- a/package.json",
        "+++ b/package.json",
        "@@",
        "+  \"scripts\": {",
        "+    \"dev\": \"<command that starts the app under test>\"",
        "+  }",
        "",
    ]
    .join("\n")
}

fn safety_for(strategy: PatchStrategy, patch_preview: Option<&String>) -> PatchSafety {
    match (strategy, patch_preview.is_some()) {
        (PatchStrategy::RequirementClarification | PatchStrategy::LaunchConfiguration, true) => {
            PatchSafety::PreviewOnly
        }
        (PatchStrategy::ReviewConfiguration, _) => PatchSafety::ManualPatch,
        _ => PatchSafety::NeedsDeveloperInput,
    }
}

fn steps_for_issue(issue: &Issue, strategy: PatchStrategy) -> Vec<String> {
    match strategy {
        PatchStrategy::RequirementClarification => vec![
            "打开目标需求文档，定位相关需求行。".to_owned(),
            "把当前描述补充为可观察、可断言的验收标准。".to_owned(),
            "确认新增描述不会改变原需求意图，只增强可测试性。".to_owned(),
        ],
        PatchStrategy::LaunchConfiguration => vec![
            "确认项目的真实启动方式和工作目录。".to_owned(),
            "在项目清单或入口文件中补充可重复执行的启动命令。".to_owned(),
            "用 SpecProbe 重新执行 launch dry-run，确认命令可以被识别。".to_owned(),
        ],
        PatchStrategy::RuntimeFix => vec![
            "根据进程退出码、stdout 和 stderr 定位启动失败位置。".to_owned(),
            "修复依赖缺失、脚本错误或运行时异常。".to_owned(),
            "重新运行启动命令并确认超时、退出码和日志均正常。".to_owned(),
        ],
        PatchStrategy::BrowserConfiguration => vec![
            "确认被测项目已启动并监听预期端口。".to_owned(),
            "检查 base URL、路由、HTTP 状态和开发服务器配置。".to_owned(),
            "重新执行浏览器探测，确认页面返回 2xx 或 3xx 状态。".to_owned(),
        ],
        PatchStrategy::EvidenceExpansion => vec![
            "为相关需求补充需要采集的证据类型。".to_owned(),
            "增加可以证明功能成功或失败的断言条件。".to_owned(),
            "重新生成 review 报告，确认问题清单减少。".to_owned(),
        ],
        PatchStrategy::ReviewConfiguration => vec![
            "检查 SpecProbe 命令参数、项目路径和需求路径。".to_owned(),
            "修正 --project、--base-url 或执行模式后重新生成报告。".to_owned(),
        ],
    }
    .into_iter()
    .chain([format!(
        "保留用户审批记录，确认是否接受 {} 的修复。",
        issue.id
    )])
    .collect()
}

fn risk_notes_for(strategy: PatchStrategy) -> Vec<String> {
    match strategy {
        PatchStrategy::RequirementClarification | PatchStrategy::EvidenceExpansion => {
            vec!["修改需求文档可能改变验收口径，提交前需要用户确认。".to_owned()]
        }
        PatchStrategy::LaunchConfiguration => vec![
            "启动命令可能执行项目脚本，运行前应确认脚本来源可信。".to_owned(),
            "不同包管理器或框架的启动命令可能不同，预览补丁需要人工替换占位符。".to_owned(),
        ],
        PatchStrategy::RuntimeFix => {
            vec!["运行时修复可能影响业务逻辑，需要结合源码和日志进一步确认。".to_owned()]
        }
        PatchStrategy::BrowserConfiguration => {
            vec!["仅修正 URL 或端口不一定能解决页面内部交互失败。".to_owned()]
        }
        PatchStrategy::ReviewConfiguration => {
            vec!["如果输入路径错误，后续报告可能针对错误项目生成。".to_owned()]
        }
    }
}

fn regression_checks_for(
    review: &ReviewReport,
    issue: &Issue,
    proposal_id: &str,
) -> Vec<RegressionCheck> {
    let mut checks = Vec::new();
    checks.push(RegressionCheck {
        id: format!("{proposal_id}-REG-01"),
        proposal_id: Some(proposal_id.to_owned()),
        command: format!(
            "specprobe requirements {}",
            review.config.requirements_source
        ),
        reason: "确认需求仍能被解析并生成测试计划。".to_owned(),
        required: true,
    });

    match issue.category {
        IssueCategory::RequirementGap | IssueCategory::MissingEvidence => {
            checks.push(RegressionCheck {
                id: format!("{proposal_id}-REG-02"),
                proposal_id: Some(proposal_id.to_owned()),
                command: format!(
                    "specprobe review {} --project {} --skip-launch --skip-browser",
                    review.config.requirements_source, review.config.project_root
                ),
                reason: "确认需求质量问题已经减少，且不会触发真实项目执行。".to_owned(),
                required: true,
            });
        }
        IssueCategory::MissingExecutionPath | IssueCategory::RuntimeFailure => {
            checks.push(RegressionCheck {
                id: format!("{proposal_id}-REG-02"),
                proposal_id: Some(proposal_id.to_owned()),
                command: format!("specprobe launch {} --dry-run", review.config.project_root),
                reason: "确认项目启动命令可以被识别。".to_owned(),
                required: true,
            });
        }
        IssueCategory::BrowserFailure => {
            checks.push(RegressionCheck {
                id: format!("{proposal_id}-REG-02"),
                proposal_id: Some(proposal_id.to_owned()),
                command: format!(
                    "specprobe browser {} --base-url {} --dry-run",
                    review.config.requirements_source, review.config.base_url
                ),
                reason: "确认浏览器动作计划仍可生成。".to_owned(),
                required: true,
            });
        }
        IssueCategory::ReviewConfiguration => {
            checks.push(RegressionCheck {
                id: format!("{proposal_id}-REG-02"),
                proposal_id: Some(proposal_id.to_owned()),
                command: format!(
                    "specprobe review {} --project {}",
                    review.config.requirements_source, review.config.project_root
                ),
                reason: "确认修正后的审查参数可以生成报告。".to_owned(),
                required: true,
            });
        }
    }

    checks
}

fn global_regression_checks(review: &ReviewReport) -> Vec<RegressionCheck> {
    vec![RegressionCheck {
        id: "GLOBAL-REG-01".to_owned(),
        proposal_id: None,
        command: format!(
            "specprobe review {} --project {}",
            review.config.requirements_source, review.config.project_root
        ),
        reason: "完成修复后重新生成综合审查报告，比较 Issue 数量和证据变化。".to_owned(),
        required: true,
    }]
}

impl fmt::Display for PatchStrategy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RequirementClarification => "requirement-clarification",
            Self::LaunchConfiguration => "launch-configuration",
            Self::RuntimeFix => "runtime-fix",
            Self::BrowserConfiguration => "browser-configuration",
            Self::EvidenceExpansion => "evidence-expansion",
            Self::ReviewConfiguration => "review-configuration",
        })
    }
}

impl fmt::Display for PatchSafety {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::PreviewOnly => "preview-only",
            Self::ManualPatch => "manual-patch",
            Self::NeedsDeveloperInput => "needs-developer-input",
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{PatchStrategy, RemediationOptions, generate_remediation_report};

    #[tokio::test]
    async fn generates_requirement_patch_preview() {
        let root = temp_project("specprobe-remediation-requirement");
        let requirements = root.join("PRD.md");
        fs::write(&requirements, "- 页面应该简单友好。").expect("write requirements");

        let report = generate_remediation_report(
            &requirements,
            RemediationOptions {
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
        .expect("remediation succeeds");

        let proposal = report
            .proposals
            .iter()
            .find(|proposal| proposal.strategy == PatchStrategy::RequirementClarification)
            .expect("requirement proposal exists");
        assert!(
            proposal.patch_preview.as_deref().is_some_and(|preview| {
                preview.contains("--- a/") && preview.contains("验收")
            })
        );
        assert!(!report.regression_plan.checks.is_empty());
        fs::remove_dir_all(root).expect("remove test project");
    }

    #[tokio::test]
    async fn clean_review_still_has_global_regression_check() {
        let root = temp_project("specprobe-remediation-clean");
        let requirements = root.join("PRD.md");
        fs::write(&requirements, "- 系统必须显示登录成功提示。").expect("write requirements");

        let report = generate_remediation_report(
            &requirements,
            RemediationOptions {
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
        .expect("remediation succeeds");

        assert!(report.proposals.is_empty());
        assert_eq!(report.regression_plan.checks.len(), 1);
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
