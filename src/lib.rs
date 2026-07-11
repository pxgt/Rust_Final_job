pub mod ai;
pub mod browser;
pub mod check;
pub mod cli;
pub mod config;
pub mod diagnosis;
pub mod doctor;
pub mod output;
pub mod playwright;
pub mod refine;
pub mod remediation;
pub mod report;
pub mod requirements;
pub mod review;
pub mod runtime;
pub mod scanner;
pub mod scenario;
pub mod storage;
#[cfg(test)]
pub(crate) mod testutil;
pub mod ui;

use anyhow::Result;
use clap::Parser;

use crate::cli::{Cli, Command, IssuesAction, RunsAction};

fn cache_dir_unless(no_cache: bool) -> Option<std::path::PathBuf> {
    (!no_cache).then(|| std::path::PathBuf::from(".specprobe").join("cache"))
}

/// 解析审批命令作用的 run:显式 `--run` 或最近一次运行。无运行则报错。
fn resolve_run(store: &storage::Store, run: Option<String>) -> Result<String> {
    match run {
        Some(id) => Ok(id),
        None => store
            .latest_run_id()?
            .ok_or_else(|| anyhow::anyhow!("no archived runs; run `specprobe check` first")),
    }
}

/// 设置某 issue 的审批状态(按指纹持久化,影响所有 run 的同指纹问题)。
fn set_issue_approval(
    store: &storage::Store,
    run: Option<String>,
    issue_id: &str,
    state: &str,
    note: Option<String>,
) -> Result<()> {
    let run_id = resolve_run(store, run)?;
    match store.issue_fingerprint_of(&run_id, issue_id)? {
        Some(fingerprint) => {
            store.set_approval(&fingerprint, state, note.as_deref())?;
            println!("Issue {issue_id} marked {state} (fingerprint {fingerprint}).");
        }
        None => println!("Issue {issue_id} was not found in run {run_id}."),
    }
    Ok(())
}

/// 归档一次运行到本地 store,失败仅告警不阻断(存储是尽力而为的附加能力)。
fn archive_run(project_root: &str, base_url: &str, executed: bool, review: &review::ReviewReport) {
    let base_dir = std::path::PathBuf::from(".specprobe");
    match storage::open(&base_dir)
        .and_then(|mut store| store.record_run(project_root, base_url, executed, review))
    {
        Ok(summary) => eprintln!("Run archived as {} ({})", summary.id, summary.report_path),
        Err(error) => eprintln!("Warning: failed to archive run: {error}"),
    }
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Check {
            path,
            requirements,
            base_url,
            provider,
            no_cache,
            yes,
            html,
            no_html,
            no_store,
            launch_timeout_secs,
            browser_timeout_secs,
            json,
        } => {
            let loaded = config::load_project_config(&path)?;
            if let Some(loaded) = &loaded {
                eprintln!("Using configuration from {}", loaded.source.display());
            }
            let settings = config::resolve_settings(
                &path,
                config::CliOverrides {
                    base_url,
                    provider,
                    requirements,
                    launch_timeout_secs,
                    browser_timeout_secs,
                    no_cache,
                },
                config::EnvOverrides::from_process_env(),
                loaded.as_ref(),
            )?;
            let progress = ui::Progress::spinner(!json);
            let stage = progress.stage_fn();
            let report = check::run_check(
                check::CheckOptions {
                    path,
                    requirements: settings.requirements,
                    base_url: settings.base_url,
                    provider: settings.provider,
                    cache_dir: cache_dir_unless(settings.no_cache),
                    assume_yes: yes,
                    launch_timeout_secs: settings.launch_timeout_secs,
                    browser_timeout_secs: settings.browser_timeout_secs,
                },
                &stage,
            )
            .await?;
            progress.finish();
            if !no_html {
                if let Some(parent) = html.parent()
                    && !parent.as_os_str().is_empty()
                {
                    std::fs::create_dir_all(parent)?;
                }
                report::write_review_html(&report.review, &html)?;
                eprintln!("HTML report written to {}", html.display());
            }
            if !no_store {
                archive_run(
                    &report.review.config.project_root,
                    &report.review.config.base_url,
                    report.executed,
                    &report.review,
                );
            }
            output::print_check_report(&report, json)?;
        }
        Command::Init { path, force } => {
            let target = config::write_template(&path, force)?;
            println!("Wrote configuration template to {}", target.display());
        }
        Command::Runs { action } => {
            let store = storage::open(&std::path::PathBuf::from(".specprobe"))?;
            match action {
                RunsAction::List { limit, json } => {
                    let runs = store.list_runs(limit)?;
                    output::print_runs_list(&runs, json)?;
                }
                RunsAction::Show { id, json } => {
                    let run = store.get_run(&id)?;
                    let issues = match &run {
                        Some(summary) => store.run_issues(&summary.id)?,
                        None => Vec::new(),
                    };
                    output::print_run_show(&id, run.as_ref(), &issues, json)?;
                }
            }
        }
        Command::Issues { action } => {
            let store = storage::open(&std::path::PathBuf::from(".specprobe"))?;
            match action {
                IssuesAction::List { run, all, json } => {
                    let run_id = resolve_run(&store, run)?;
                    let mut issues = store.run_issues(&run_id)?;
                    if !all {
                        issues.retain(|issue| issue.approval != "ignored");
                    }
                    output::print_issues_list(&run_id, &issues, json)?;
                }
                IssuesAction::Show {
                    issue_id,
                    run,
                    json,
                } => {
                    let run_id = resolve_run(&store, run)?;
                    let issues = store.run_issues(&run_id)?;
                    let issue = issues.iter().find(|issue| issue.issue_id == issue_id);
                    output::print_issue_show(&issue_id, &run_id, issue, json)?;
                }
                IssuesAction::Accept {
                    issue_id,
                    run,
                    note,
                } => set_issue_approval(&store, run, &issue_id, "accepted", note)?,
                IssuesAction::Reject {
                    issue_id,
                    run,
                    note,
                } => set_issue_approval(&store, run, &issue_id, "rejected", note)?,
                IssuesAction::Ignore {
                    issue_id,
                    run,
                    note,
                } => set_issue_approval(&store, run, &issue_id, "ignored", note)?,
            }
        }
        Command::Doctor { json } => {
            let report = doctor::inspect_environment();
            output::print_doctor_report(&report, json)?;
        }
        Command::SetupBrowser => {
            playwright::setup_runner().await?;
        }
        Command::Scan { path, json } => {
            let profile = scanner::scan_project(&path)?;
            output::print_project_profile(&profile, json)?;
        }
        Command::Requirements {
            path,
            provider,
            no_cache,
            json,
        } => {
            let report = refine::analyze_requirements_with_refinement(
                &path,
                refine::RefineOptions {
                    provider,
                    cache_dir: cache_dir_unless(no_cache),
                },
            )
            .await?;
            output::print_requirement_report(&report, json)?;
        }
        Command::Ai {
            path,
            provider,
            no_cache,
            json,
        } => {
            // ai 命令与 requirements 共用两级流水线:先精解析需求,再生成建议。
            let refined = refine::analyze_requirements_with_refinement(
                &path,
                refine::RefineOptions {
                    provider,
                    cache_dir: cache_dir_unless(no_cache),
                },
            )
            .await?;
            let report = ai::analyze_report_with_provider(
                refined,
                provider,
                ai::AiOptions {
                    cache_dir: cache_dir_unless(no_cache),
                },
            )
            .await?;
            output::print_ai_report(&report, json)?;
        }
        Command::Launch {
            path,
            timeout_secs,
            dry_run,
            json,
        } => {
            let report = runtime::launch_project(&path, timeout_secs, dry_run).await?;
            output::print_launch_report(&report, json)?;
        }
        Command::Browser {
            path,
            base_url,
            provider,
            no_cache,
            timeout_secs,
            dry_run,
            json,
        } => {
            let report = browser::run_browser_plan(
                &path,
                &base_url,
                timeout_secs,
                dry_run,
                browser::BrowserOptions {
                    provider,
                    cache_dir: cache_dir_unless(no_cache),
                },
            )
            .await?;
            output::print_browser_report(&report, json)?;
        }
        Command::Review {
            path,
            project,
            base_url,
            provider,
            no_cache,
            execute,
            skip_launch,
            skip_browser,
            launch_timeout_secs,
            browser_timeout_secs,
            html,
            no_store,
            json,
        } => {
            let progress = ui::Progress::spinner(!json);
            let stage = progress.stage_fn();
            let report = review::generate_review_report_with(
                &path,
                review::ReviewOptions {
                    project_path: project,
                    base_url,
                    provider,
                    cache_dir: cache_dir_unless(no_cache),
                    execute,
                    skip_launch,
                    skip_browser,
                    launch_timeout_secs,
                    browser_timeout_secs,
                },
                &stage,
            )
            .await?;
            progress.finish();
            if let Some(html_path) = html {
                report::write_review_html(&report, &html_path)?;
                eprintln!("HTML report written to {}", html_path.display());
            }
            if !no_store {
                archive_run(
                    &report.config.project_root,
                    &report.config.base_url,
                    report.config.execute,
                    &report,
                );
            }
            output::print_review_report(&report, json)?;
        }
        Command::Propose {
            path,
            project,
            base_url,
            provider,
            no_cache,
            execute,
            skip_launch,
            skip_browser,
            launch_timeout_secs,
            browser_timeout_secs,
            json,
        } => {
            let report = remediation::generate_remediation_report(
                &path,
                remediation::RemediationOptions {
                    project_path: project,
                    base_url,
                    provider,
                    cache_dir: cache_dir_unless(no_cache),
                    execute,
                    skip_launch,
                    skip_browser,
                    launch_timeout_secs,
                    browser_timeout_secs,
                },
            )
            .await?;
            output::print_remediation_report(&report, json)?;
        }
    }

    Ok(())
}
