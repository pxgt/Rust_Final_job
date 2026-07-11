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
#[cfg(test)]
pub(crate) mod testutil;
pub mod ui;

use anyhow::Result;
use clap::Parser;

use crate::cli::{Cli, Command};

fn cache_dir_unless(no_cache: bool) -> Option<std::path::PathBuf> {
    (!no_cache).then(|| std::path::PathBuf::from(".specprobe").join("cache"))
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
            output::print_check_report(&report, json)?;
        }
        Command::Init { path, force } => {
            let target = config::write_template(&path, force)?;
            println!("Wrote configuration template to {}", target.display());
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
