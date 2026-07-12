pub mod ai;
pub mod apply;
pub mod browser;
pub mod check;
pub mod cli;
pub mod config;
pub mod diagnosis;
pub mod doctor;
pub mod output;
pub mod patch;
pub mod playwright;
pub mod redact;
pub mod refine;
pub mod regression;
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

/// 真实终端确认:stderr 提问,stdin 读一行;非 TTY / EOF / 非 y 一律拒绝。
fn confirm_on_terminal(message: &str) -> bool {
    use std::io::{BufRead, Write, stderr, stdin};
    eprint!("{message} [y/N] ");
    let _ = stderr().flush();
    let mut line = String::new();
    if stdin().lock().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
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

/// 从归档的 report.json(以 Value 读取,避免为 ReviewReport 引入 Deserialize)
/// 构造补丁生成输入:取 issue 的描述,并汇总关联诊断给出的源码定位文件。
fn build_patch_input(report: &serde_json::Value, issue_id: &str) -> Result<patch::PatchInput> {
    let issue = report
        .get("issues")
        .and_then(serde_json::Value::as_array)
        .and_then(|issues| {
            issues
                .iter()
                .find(|entry| entry.get("id").and_then(serde_json::Value::as_str) == Some(issue_id))
        })
        .ok_or_else(|| anyhow::anyhow!("issue {issue_id} was not found in the run report"))?;

    let field = |name: &str| {
        issue
            .get(name)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };

    // 汇总引用了该 issue 的诊断给出的源码文件(去重)。
    let mut source_files: Vec<String> = Vec::new();
    if let Some(diagnoses) = report
        .get("diagnoses")
        .and_then(serde_json::Value::as_array)
    {
        for diagnosis in diagnoses {
            let refers = diagnosis
                .get("related_issue_ids")
                .and_then(serde_json::Value::as_array)
                .map(|ids| ids.iter().any(|id| id.as_str() == Some(issue_id)))
                .unwrap_or(false);
            if !refers {
                continue;
            }
            if let Some(locations) = diagnosis
                .get("source_locations")
                .and_then(serde_json::Value::as_array)
            {
                for location in locations {
                    if let Some(file) = location.get("file").and_then(serde_json::Value::as_str)
                        && !source_files.iter().any(|existing| existing == file)
                    {
                        source_files.push(file.to_owned());
                    }
                }
            }
        }
    }
    if source_files.is_empty() {
        return Err(anyhow::anyhow!(
            "issue {issue_id} has no diagnosed source locations; re-run `check`/`review` with a real --provider so diagnosis can locate the source"
        ));
    }

    Ok(patch::PatchInput {
        issue_id: issue_id.to_owned(),
        title: field("title"),
        expected: field("expected"),
        actual: field("actual"),
        source_files,
    })
}

/// 从归档 report.json 的 config 重建验证重跑参数(与 baseline run 一致的执行设置)。
fn verify_options_from_report(
    report: &serde_json::Value,
    provider: ai::AiProviderKind,
    no_cache: bool,
) -> regression::VerifyOptions {
    let string_at = |ptr: &str| {
        report
            .pointer(ptr)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let bool_at = |ptr: &str| report.pointer(ptr).and_then(serde_json::Value::as_bool);
    let u64_at = |ptr: &str, default: u64| {
        report
            .pointer(ptr)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(default)
    };
    regression::VerifyOptions {
        requirements_source: std::path::PathBuf::from(string_at("/config/requirements_source")),
        base_url: string_at("/config/base_url"),
        provider,
        cache_dir: cache_dir_unless(no_cache),
        execute: bool_at("/config/execute").unwrap_or(false),
        skip_launch: !bool_at("/config/launch_enabled").unwrap_or(true),
        skip_browser: !bool_at("/config/browser_enabled").unwrap_or(true),
        launch_timeout_secs: u64_at("/config/launch_timeout_secs", 15),
        browser_timeout_secs: u64_at("/config/browser_timeout_secs", 10),
    }
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
            samples,
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
            // 启动命令确认记忆(ROADMAP 3.4):同项目同命令确认过一次后免再确认。
            let project_key = std::fs::canonicalize(&path)
                .map(|resolved| resolved.display().to_string())
                .unwrap_or_else(|_| path.display().to_string());
            let command_store = storage::open(&std::path::PathBuf::from(".specprobe")).ok();
            let confirm = |message: &str| -> bool {
                if let Some(store) = &command_store
                    && store
                        .is_command_approved(&project_key, message)
                        .unwrap_or(false)
                {
                    eprintln!(
                        "Launch command previously approved for this project; executing without prompt."
                    );
                    return true;
                }
                let approved = confirm_on_terminal(message);
                if approved && let Some(store) = &command_store {
                    let _ = store.approve_command(&project_key, message);
                }
                approved
            };
            let report = check::run_check_with_progress(
                check::CheckOptions {
                    path,
                    requirements: settings.requirements,
                    base_url: settings.base_url,
                    provider: settings.provider,
                    cache_dir: cache_dir_unless(settings.no_cache),
                    assume_yes: yes,
                    launch_timeout_secs: settings.launch_timeout_secs,
                    browser_timeout_secs: settings.browser_timeout_secs,
                    samples,
                },
                confirm,
                &stage,
            )
            .await?;
            drop(command_store);
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
        Command::Fix {
            issue_id,
            run,
            provider,
            no_cache,
            apply,
            allow_dirty,
            verify,
            json,
        } => {
            if verify && !apply {
                anyhow::bail!("--verify requires --apply");
            }
            let store = storage::open(&std::path::PathBuf::from(".specprobe"))?;
            let run_id = resolve_run(&store, run)?;
            let summary = store
                .get_run(&run_id)?
                .ok_or_else(|| anyhow::anyhow!("run {run_id} was not found"))?;
            let report: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&summary.report_path)?)?;
            let input = build_patch_input(&report, &issue_id)?;
            let project_root = report
                .pointer("/config/project_root")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("run report is missing config.project_root"))?
                .to_owned();
            let generated = patch::generate_patch(
                &input,
                std::path::Path::new(&project_root),
                provider,
                cache_dir_unless(no_cache),
            )
            .await?;
            output::print_patch(&generated, json)?;

            if apply {
                let branch = format!("specprobe/fix-{}", generated.issue_id.to_ascii_lowercase());
                let message = format!(
                    "About to apply this patch to a new branch {branch} in {project_root} (your current branch is left untouched)."
                );
                if confirm_on_terminal(&message) {
                    let project = std::path::Path::new(&project_root);
                    let outcome = apply::apply_patch(
                        project,
                        &generated,
                        &apply::ApplyOptions { allow_dirty },
                    )?;
                    output::print_apply_outcome(&outcome, json)?;

                    if verify {
                        let baseline: Vec<String> = store
                            .run_issues(&run_id)?
                            .into_iter()
                            .map(|issue| issue.fingerprint)
                            .collect();
                        let target = store
                            .issue_fingerprint_of(&run_id, &issue_id)?
                            .unwrap_or_default();
                        let options = verify_options_from_report(&report, provider, no_cache);
                        eprintln!("Verifying the fix by re-running review on the branch…");
                        let verdict = regression::verify_on_branch(
                            project,
                            &outcome.branch,
                            &baseline,
                            &target,
                            &options,
                        )
                        .await?;
                        output::print_verdict(&verdict, json)?;
                        if !verdict.verified {
                            apply::delete_branch(project, &outcome.branch)?;
                            eprintln!(
                                "Verification failed; rolled back branch {}.",
                                outcome.branch
                            );
                        }
                    }
                } else {
                    eprintln!("Aborted; patch was not applied.");
                }
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
            samples,
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
                    samples,
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
            samples,
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
                    samples,
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
