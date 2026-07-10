pub mod ai;
pub mod browser;
pub mod cli;
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
#[cfg(test)]
pub(crate) mod testutil;

use anyhow::Result;
use clap::Parser;

use crate::cli::{Cli, Command};

fn cache_dir_unless(no_cache: bool) -> Option<std::path::PathBuf> {
    (!no_cache).then(|| std::path::PathBuf::from(".specprobe").join("cache"))
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
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
            json,
        } => {
            let report = review::generate_review_report(
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
            )
            .await?;
            if let Some(html_path) = html {
                report::write_review_html(&report, &html_path)?;
                eprintln!("HTML report written to {}", html_path.display());
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
