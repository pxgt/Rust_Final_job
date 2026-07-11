use anyhow::Result;

use crate::ai::AiAnalysisReport;
use crate::browser::BrowserRunReport;
use crate::check::CheckReport;
use crate::doctor::DoctorReport;
use crate::remediation::RemediationReport;
use crate::requirements::RequirementReport;
use crate::review::ReviewReport;
use crate::runtime::LaunchReport;
use crate::scanner::ProjectProfile;
use crate::storage::{RunSummary, StoredIssue};

pub fn print_runs_list(runs: &[RunSummary], json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(runs)?);
        return Ok(());
    }
    if runs.is_empty() {
        println!("No archived runs. Run `specprobe check` or `review` first.");
        return Ok(());
    }
    println!("Archived runs");
    println!("-------------");
    for run in runs {
        println!(
            "{}  {}  engine={}  executed={}  reqs={}  issues={} (high={})",
            run.id,
            run.project_root,
            run.engine,
            yes_no(run.executed),
            run.requirements,
            run.issues,
            run.high
        );
    }
    Ok(())
}

pub fn print_run_show(
    id: &str,
    run: Option<&RunSummary>,
    issues: &[StoredIssue],
    json: bool,
) -> Result<()> {
    if json {
        let value = serde_json::json!({ "run": run, "issues": issues });
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    let Some(run) = run else {
        println!("Run '{id}' was not found. Use `specprobe runs list`.");
        return Ok(());
    };
    println!("Run {}", run.id);
    println!("----{}", "-".repeat(run.id.len()));
    println!("Project: {}", run.project_root);
    println!("Base URL: {}", run.base_url);
    println!("Engine: {}, executed: {}", run.engine, yes_no(run.executed));
    println!(
        "Requirements: {}, issues: {} (high: {})",
        run.requirements, run.issues, run.high
    );
    println!("Report: {}", run.report_path);
    if !issues.is_empty() {
        println!("\nIssues");
        for issue in issues {
            let requirement = issue
                .requirement
                .as_ref()
                .map(|value| format!(" ({value})"))
                .unwrap_or_default();
            println!(
                "- {} [{} / {} / {}]{} {}",
                issue.issue_id,
                issue.severity,
                issue.category,
                issue.approval,
                requirement,
                issue.title
            );
        }
    }
    Ok(())
}

pub fn print_check_report(report: &CheckReport, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(report)?);
        return Ok(());
    }

    println!("SpecProbe check");
    println!("===============");
    println!(
        "Project: {} ({})",
        report.profile.root,
        report.profile.technologies.join(", ")
    );
    if !report.executed {
        println!("Mode: plan-only (launch command was not confirmed; rerun with --yes to execute)");
    }
    println!();
    print_review_report(&report.review, false)
}

pub fn print_doctor_report(report: &DoctorReport, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(report)?);
        return Ok(());
    }

    println!("SpecProbe environment");
    println!("---------------------");
    for tool in &report.tools {
        let marker = if tool.available { "OK" } else { "MISSING" };
        let detail = tool
            .version
            .as_deref()
            .or(tool.note.as_deref())
            .unwrap_or("no details");
        println!("[{marker:7}] {:<8} {detail}", tool.name);
    }

    println!("\nAI providers");
    for provider in &report.ai_providers {
        let marker = if provider.configured {
            "CONFIGURED"
        } else {
            "NOT SET"
        };
        println!("[{marker:10}] {} ({})", provider.name, provider.source);
    }

    println!(
        "\nReadiness: core={}, web-testing={}, ai={}",
        yes_no(report.core_ready),
        yes_no(report.web_testing_ready),
        yes_no(report.ai_ready)
    );
    for note in &report.notes {
        println!("Note: {note}");
    }

    Ok(())
}

pub fn print_project_profile(profile: &ProjectProfile, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(profile)?);
        return Ok(());
    }

    println!("Project: {}", profile.root);
    println!(
        "Git repository: {}",
        if profile.git_repository { "yes" } else { "no" }
    );
    println!(
        "Source files: {} (tests: {})",
        profile.source_file_count, profile.test_file_count
    );
    println!("Manifests: {}", join_or_none(&profile.manifests));
    println!(
        "Requirement documents: {}",
        join_or_none(&profile.requirement_documents)
    );
    println!("Technologies: {}", join_or_none(&profile.technologies));

    if profile.languages.is_empty() {
        println!("Languages: none detected");
    } else {
        let languages = profile
            .languages
            .iter()
            .map(|language| format!("{} ({})", language.language, language.files))
            .collect::<Vec<_>>()
            .join(", ");
        println!("Languages: {languages}");
    }

    Ok(())
}

pub fn print_requirement_report(report: &RequirementReport, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(report)?);
        return Ok(());
    }

    println!("Requirement analysis: {}", report.source);
    println!(
        "Engine: {}, documents: {}, requirements: {}, test cases: {}",
        report.engine,
        report.documents.len(),
        report.requirements.len(),
        report.test_plan.cases.len()
    );

    if !report.documents.is_empty() {
        println!("\nDocuments");
        for document in &report.documents {
            println!(
                "- {}: {} requirement(s)",
                document.path, document.requirement_count
            );
        }
    }

    if !report.diagnostics.is_empty() {
        println!("\nDiagnostics");
        for diagnostic in &report.diagnostics {
            println!("- [{}] {}", diagnostic.severity, diagnostic.message);
        }
    }

    if report.requirements.is_empty() {
        println!("\nNo requirements were extracted.");
        return Ok(());
    }

    println!("\nRequirements");
    for requirement in &report.requirements {
        println!(
            "- {} [{} / {}] {}",
            requirement.id, requirement.priority, requirement.category, requirement.title
        );
        println!(
            "  source: {}:{}",
            requirement.source.path, requirement.source.line
        );
        for criterion in &requirement.acceptance_criteria {
            println!("  - {} {}", criterion.id, criterion.statement);
        }
        for flag in &requirement.quality_flags {
            println!("  warning: {} - {}", flag.kind, flag.message);
        }
    }

    Ok(())
}

pub fn print_ai_report(report: &AiAnalysisReport, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(report)?);
        return Ok(());
    }

    println!("AI-assisted analysis");
    println!("--------------------");
    println!(
        "Provider: {} ({}, model: {}, configured: {}, offline: {})",
        report.provider.name,
        report.provider.kind,
        report.provider.model,
        yes_no(report.provider.configured),
        yes_no(report.provider.offline)
    );
    println!(
        "Input: {} requirement(s), schema: {}",
        report.request.requirement_count, report.request.schema_name
    );
    let usage_text = report
        .transport
        .usage
        .as_ref()
        .map(|usage| {
            format!(
                ", tokens: {}+{}={}",
                usage.prompt_tokens, usage.completion_tokens, usage.total_tokens
            )
        })
        .unwrap_or_default();
    println!(
        "Transport: attempts={}, cache_hit={}{usage_text}",
        report.transport.attempts,
        yes_no(report.transport.cache_hit)
    );
    println!("Summary: {}", report.model_output.summary);
    println!("Confidence: {}", report.model_output.confidence);

    if report.model_output.suggestions.is_empty() {
        println!("\nNo AI suggestions were produced.");
    } else {
        println!("\nSuggestions");
        for suggestion in &report.model_output.suggestions {
            println!(
                "- {} [{} / {}] {}",
                suggestion.requirement_id,
                suggestion.severity,
                suggestion.suggestion_type,
                suggestion.message
            );
            println!("  rationale: {}", suggestion.rationale);
        }
    }

    if !report.model_output.follow_up_questions.is_empty() {
        println!("\nFollow-up questions");
        for question in &report.model_output.follow_up_questions {
            println!("- {question}");
        }
    }

    Ok(())
}

pub fn print_launch_report(report: &LaunchReport, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(report)?);
        return Ok(());
    }

    println!("Project launch");
    println!("--------------");
    println!("Project: {}", report.project_root);
    println!("Adapter: {}", report.adapter);
    println!(
        "Command: {} {}",
        report.command.program,
        report.command.args.join(" ")
    );
    println!("Working directory: {}", report.command.working_directory);
    println!(
        "Source: {} ({})",
        report.command.source, report.command.confidence
    );
    println!(
        "Execution: attempted={}, dry-run={}, success={}, timed-out={}, long-running={}, exit-code={}",
        yes_no(report.execution.attempted),
        yes_no(report.execution.dry_run),
        yes_no(report.execution.success),
        yes_no(report.execution.timed_out),
        yes_no(report.execution.long_running),
        report
            .execution
            .exit_code
            .map(|code| code.to_string())
            .unwrap_or_else(|| "none".to_owned())
    );
    println!(
        "Duration: {} ms (timeout: {} s)",
        report.execution.duration_ms, report.execution.timeout_secs
    );
    if let Some(readiness) = &report.readiness {
        println!(
            "Readiness: probed={}, ready={}, waited={} ms — {}",
            yes_no(readiness.probed),
            yes_no(readiness.ready),
            readiness.waited_ms,
            readiness.detail
        );
    }

    if !report.diagnostics.is_empty() {
        println!("\nDiagnostics");
        for diagnostic in &report.diagnostics {
            println!("- [{}] {}", diagnostic.severity, diagnostic.message);
        }
    }

    if !report.stdout_excerpt.is_empty() {
        println!("\nstdout");
        println!("{}", report.stdout_excerpt);
    }
    if !report.stderr_excerpt.is_empty() {
        println!("\nstderr");
        println!("{}", report.stderr_excerpt);
    }

    Ok(())
}

pub fn print_browser_report(report: &BrowserRunReport, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(report)?);
        return Ok(());
    }

    println!("Browser test execution");
    println!("----------------------");
    println!("Requirements: {}", report.requirement_source);
    println!("Base URL: {}", report.base_url);
    println!("Backend: {}", report.backend);
    println!(
        "Execution: attempted={}, dry-run={}, success={}, duration={} ms, timeout={} s",
        yes_no(report.execution.attempted),
        yes_no(report.execution.dry_run),
        yes_no(report.execution.success),
        report.execution.duration_ms,
        report.execution.timeout_secs
    );
    println!("Browser cases: {}", report.plan.cases.len());

    if !report.diagnostics.is_empty() {
        println!("\nDiagnostics");
        for diagnostic in &report.diagnostics {
            println!("- [{}] {}", diagnostic.severity, diagnostic.message);
        }
    }

    if let Some(page) = &report.page {
        println!("\nPage evidence");
        println!("URL: {}", page.url);
        println!(
            "Status: {}",
            page.status_code
                .map(|code| code.to_string())
                .unwrap_or_else(|| "unknown".to_owned())
        );
        println!("Status line: {}", page.status_text);
        if let Some(title) = &page.title {
            println!("Title: {title}");
        }
        println!("Response bytes: {}", page.response_bytes);
        if !page.body_excerpt.is_empty() {
            println!("Body excerpt: {}", page.body_excerpt);
        }
    }

    if let Some(evidence) = &report.playwright {
        let outcome = &evidence.outcome;
        println!("\nPlaywright evidence (archived in {})", evidence.run_dir);
        if let Some(snapshot) = &outcome.snapshot {
            if let Some(title) = &snapshot.title {
                println!("Page title: {title}");
            }
            println!("Interactive elements: {}", snapshot.interactive.len());
        }
        println!(
            "Actions: {} ({} failed)",
            outcome.actions.len(),
            outcome.actions.iter().filter(|action| !action.ok).count()
        );
        for action in &outcome.actions {
            if let Some(path) = &action.screenshot_path {
                println!("  screenshot: {path}");
            }
        }
        if !outcome.console.is_empty() {
            println!("Console messages: {}", outcome.console.len());
            for message in outcome.console.iter().take(10) {
                println!("  [{}] {}", message.level, message.text);
            }
        }
        if !outcome.network_failures.is_empty() {
            println!("Network failures: {}", outcome.network_failures.len());
            for failure in outcome.network_failures.iter().take(10) {
                let status = failure
                    .status
                    .map(|code| format!(" HTTP {code}"))
                    .unwrap_or_default();
                println!("  {}{status}", failure.url);
            }
        }
        for error in &outcome.page_errors {
            println!("Page error: {error}");
        }
    }

    if !report.scenarios.is_empty() {
        println!("\nInteraction scenarios");
        for scenario in &report.scenarios {
            println!(
                "- {} [{}] {}",
                scenario.requirement_id,
                if scenario.success { "PASS" } else { "FAIL" },
                scenario.title
            );
            for step in &scenario.steps {
                println!(
                    "  - {} {} -> {}",
                    step.action,
                    step.target,
                    if step.ok { "ok" } else { "FAIL" }
                );
            }
            if let Some(screenshot) = &scenario.screenshot {
                println!("  screenshot: {screenshot}");
            }
        }
    }

    if !report.plan.cases.is_empty() {
        println!("\nAction plan");
        for case in &report.plan.cases {
            println!(
                "- {} -> {} ({:?})",
                case.id, case.requirement_id, case.source_executor_hint
            );
            for action in &case.actions {
                println!("  - {} {}", action.action, action.target);
                if let Some(assertion) = &action.assertion {
                    println!("    assert: {assertion}");
                }
            }
        }
    }

    Ok(())
}

pub fn print_review_report(report: &ReviewReport, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(report)?);
        return Ok(());
    }

    println!("SpecProbe review");
    println!("----------------");
    println!("Requirements: {}", report.config.requirements_source);
    println!("Project: {}", report.config.project_root);
    println!("Base URL: {}", report.config.base_url);
    println!(
        "Mode: {}, launch={}, browser={}",
        if report.config.execute {
            "execute"
        } else {
            "plan-only"
        },
        yes_no(report.config.launch_enabled),
        yes_no(report.config.browser_enabled)
    );
    println!(
        "Summary: requirements={}, test-cases={}, evidence={}, issues={} (critical={}, high={}, medium={}, low={}, info={}), pending-decisions={}",
        report.summary.requirements,
        report.summary.test_cases,
        report.summary.evidence_items,
        report.summary.issues,
        report.summary.critical,
        report.summary.high,
        report.summary.medium,
        report.summary.low,
        report.summary.info,
        report.summary.pending_decisions
    );

    if report.issues.is_empty() {
        println!("\nNo issues were generated from the available evidence.");
    } else {
        println!("\nIssues");
        for issue in &report.issues {
            println!(
                "- {} [{} / {} / {}] {}",
                issue.id, issue.severity, issue.category, issue.approval, issue.title
            );
            if let Some(requirement_id) = &issue.related_requirement {
                println!("  requirement: {requirement_id}");
            }
            println!("  expected: {}", issue.expected);
            println!("  actual: {}", issue.actual);
            println!("  evidence: {}", issue.evidence_ids.join(", "));
            println!("  recommendation: {}", issue.recommendation);
        }
    }

    if !report.diagnoses.is_empty() {
        println!("\nAI diagnoses");
        for diagnosis in &report.diagnoses {
            println!(
                "- [{}] {} (issues: {})",
                diagnosis.confidence,
                diagnosis.title,
                if diagnosis.related_issue_ids.is_empty() {
                    "-".to_owned()
                } else {
                    diagnosis.related_issue_ids.join(", ")
                }
            );
            println!("  root cause: {}", diagnosis.root_cause);
            for location in &diagnosis.source_locations {
                let line = location
                    .line
                    .map(|line| format!(":{line}"))
                    .unwrap_or_default();
                println!("  location: {}{line}", location.file);
            }
            if !diagnosis.suggested_fix.is_empty() {
                println!("  suggested fix: {}", diagnosis.suggested_fix);
            }
        }
    }

    if !report.evidence.is_empty() {
        println!("\nEvidence");
        for evidence in &report.evidence {
            let related = evidence
                .related_requirement
                .as_ref()
                .map(|requirement_id| format!(" {requirement_id}"))
                .unwrap_or_default();
            println!(
                "- {} [{} / {}]{} {}",
                evidence.id, evidence.status, evidence.kind, related, evidence.summary
            );
        }
    }

    Ok(())
}

pub fn print_remediation_report(report: &RemediationReport, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(report)?);
        return Ok(());
    }

    println!("SpecProbe proposals");
    println!("-------------------");
    println!(
        "Review source: {}",
        report.review.config.requirements_source
    );
    println!("Project: {}", report.review.config.project_root);
    println!(
        "Summary: issues={}, proposals={}, patch-previews={}, regression-checks={}, auto-apply={}",
        report.summary.issues,
        report.summary.proposals,
        report.summary.patch_previews,
        report.summary.regression_checks,
        yes_no(report.summary.auto_apply_supported)
    );
    println!(
        "Approval required: {}",
        yes_no(report.summary.requires_user_approval)
    );

    if report.proposals.is_empty() {
        println!("\nNo patch proposals were generated from the current review issues.");
    } else {
        println!("\nPatch proposals");
        for proposal in &report.proposals {
            println!(
                "- {} -> {} [{} / {} / {}] {}",
                proposal.id,
                proposal.issue_id,
                proposal.strategy,
                proposal.safety,
                proposal.approval,
                proposal.title
            );
            println!("  target files: {}", join_or_none(&proposal.target_files));
            println!("  rationale: {}", proposal.rationale);
            println!("  steps:");
            for step in &proposal.steps {
                println!("    - {step}");
            }
            if let Some(preview) = &proposal.patch_preview {
                println!("  patch preview:");
                for line in preview.lines() {
                    println!("    {line}");
                }
            }
            if !proposal.risk_notes.is_empty() {
                println!("  risks:");
                for risk in &proposal.risk_notes {
                    println!("    - {risk}");
                }
            }
        }
    }

    if !report.regression_plan.checks.is_empty() {
        println!("\nRegression checks");
        for check in &report.regression_plan.checks {
            let proposal = check
                .proposal_id
                .as_ref()
                .map(|id| format!(" {id}"))
                .unwrap_or_default();
            println!(
                "- {}{} [{}] {}",
                check.id,
                proposal,
                if check.required {
                    "required"
                } else {
                    "optional"
                },
                check.command
            );
            println!("  reason: {}", check.reason);
        }
    }

    Ok(())
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn join_or_none(values: &[String]) -> String {
    if values.is_empty() {
        "none detected".to_owned()
    } else {
        values.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::{join_or_none, yes_no};

    #[test]
    fn formats_simple_values() {
        assert_eq!(yes_no(true), "yes");
        assert_eq!(yes_no(false), "no");
        assert_eq!(join_or_none(&[]), "none detected");
        assert_eq!(join_or_none(&["Rust".to_owned()]), "Rust");
    }
}
