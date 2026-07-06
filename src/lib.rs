pub mod ai;
pub mod browser;
pub mod cli;
pub mod doctor;
pub mod output;
pub mod remediation;
pub mod requirements;
pub mod review;
pub mod runtime;
pub mod scanner;

use anyhow::Result;
use clap::Parser;

use crate::cli::{Cli, Command};

pub async fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Doctor { json } => {
            let report = doctor::inspect_environment();
            output::print_doctor_report(&report, json)?;
        }
        Command::Scan { path, json } => {
            let profile = scanner::scan_project(&path)?;
            output::print_project_profile(&profile, json)?;
        }
        Command::Requirements { path, json } => {
            let report = requirements::analyze_requirements(&path)?;
            output::print_requirement_report(&report, json)?;
        }
        Command::Ai {
            path,
            provider,
            no_cache,
            json,
        } => {
            let options = ai::AiOptions {
                cache_dir: (!no_cache)
                    .then(|| std::path::PathBuf::from(".specprobe").join("cache")),
            };
            let report = ai::analyze_with_provider(&path, provider, options).await?;
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
            timeout_secs,
            dry_run,
            json,
        } => {
            let report = browser::run_browser_plan(&path, &base_url, timeout_secs, dry_run).await?;
            output::print_browser_report(&report, json)?;
        }
        Command::Review {
            path,
            project,
            base_url,
            execute,
            skip_launch,
            skip_browser,
            launch_timeout_secs,
            browser_timeout_secs,
            json,
        } => {
            let report = review::generate_review_report(
                &path,
                review::ReviewOptions {
                    project_path: project,
                    base_url,
                    execute,
                    skip_launch,
                    skip_browser,
                    launch_timeout_secs,
                    browser_timeout_secs,
                },
            )
            .await?;
            output::print_review_report(&report, json)?;
        }
        Command::Propose {
            path,
            project,
            base_url,
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
