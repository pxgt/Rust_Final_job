use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::ai::AiProviderKind;

#[derive(Debug, Parser)]
#[command(
    name = "specprobe",
    version,
    about = "Evidence-driven project inspection and AI-assisted testing"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum IssuesAction {
    /// List a run's issues with their approval state (hides ignored unless --all).
    List {
        /// Run id (defaults to the latest run).
        #[arg(long)]
        run: Option<String>,
        /// Include ignored issues.
        #[arg(long)]
        all: bool,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Show one issue's details from an archived run.
    Show {
        /// Issue id (e.g. ISSUE-001).
        issue_id: String,
        /// Run id (defaults to the latest run).
        #[arg(long)]
        run: Option<String>,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Accept an issue (persisted by fingerprint across runs).
    Accept {
        issue_id: String,
        #[arg(long)]
        run: Option<String>,
        #[arg(long)]
        note: Option<String>,
    },
    /// Reject an issue.
    Reject {
        issue_id: String,
        #[arg(long)]
        run: Option<String>,
        #[arg(long)]
        note: Option<String>,
    },
    /// Ignore an issue (future runs hide it unless --all).
    Ignore {
        issue_id: String,
        #[arg(long)]
        run: Option<String>,
        #[arg(long)]
        note: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum RunsAction {
    /// List recent archived runs.
    List {
        /// Maximum number of runs to show.
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Show one archived run and its issues.
    Show {
        /// Run id (from `runs list`).
        id: String,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// One-shot inspection: scan, refine requirements, run the app and browser, report.
    Check {
        /// Project directory (also searched for requirement documents).
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Requirement document or directory override. Defaults to PATH (or specprobe.toml).
        #[arg(long)]
        requirements: Option<PathBuf>,
        /// Base URL for the web application under test (default http://127.0.0.1:3000; configurable via specprobe.toml).
        #[arg(long)]
        base_url: Option<String>,
        /// AI provider for refinement, browser scenarios and diagnosis (default mock; configurable via specprobe.toml).
        #[arg(long, value_enum)]
        provider: Option<AiProviderKind>,
        /// Disable the on-disk AI response cache (.specprobe/cache).
        #[arg(long)]
        no_cache: bool,
        /// Execute the detected launch command without asking for confirmation.
        #[arg(long)]
        yes: bool,
        /// Where to write the HTML report.
        #[arg(long, value_name = "PATH", default_value = ".specprobe/report.html")]
        html: PathBuf,
        /// Skip writing the HTML report.
        #[arg(long)]
        no_html: bool,
        /// Do not archive this run to the local run store (.specprobe/specprobe.db).
        #[arg(long)]
        no_store: bool,
        /// Maximum launch/readiness time in seconds (default 15).
        #[arg(long)]
        launch_timeout_secs: Option<u64>,
        /// Maximum browser page probe time in seconds (default 10).
        #[arg(long)]
        browser_timeout_secs: Option<u64>,
        /// Scenario sampling rounds (1-3). >1 unions detections across rounds for stability.
        #[arg(long, default_value_t = 1)]
        samples: u32,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Write a specprobe.toml configuration template into a project.
    Init {
        /// Project directory to initialize.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Overwrite an existing specprobe.toml.
        #[arg(long)]
        force: bool,
    },
    /// Browse archived runs stored in .specprobe/specprobe.db.
    Runs {
        #[command(subcommand)]
        action: RunsAction,
    },
    /// Review and set approval state for issues from archived runs.
    Issues {
        #[command(subcommand)]
        action: IssuesAction,
    },
    /// Generate a validated fix patch for an archived issue (LLM-backed, git apply --check verified).
    Fix {
        /// Issue id to fix (e.g. ISSUE-001).
        issue_id: String,
        /// Run id (defaults to the latest run).
        #[arg(long)]
        run: Option<String>,
        /// AI provider used to generate the patch. Mock is rejected (needs a real model).
        #[arg(long, value_enum, default_value_t = AiProviderKind::Mock)]
        provider: AiProviderKind,
        /// Disable the on-disk AI response cache (.specprobe/cache).
        #[arg(long)]
        no_cache: bool,
        /// After showing the patch, apply it to an isolated branch specprobe/fix-<issue> (asks for confirmation).
        #[arg(long)]
        apply: bool,
        /// Allow applying even if the project's git working tree is not clean.
        #[arg(long)]
        allow_dirty: bool,
        /// After applying, re-run review on the branch and verify the defect is resolved; roll back if not (implies --apply).
        #[arg(long)]
        verify: bool,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Check whether the local machine is ready for analysis and web testing.
    Doctor {
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Install the Playwright browser runner (requires Node.js).
    SetupBrowser,
    /// Inspect a project and summarize its technology and test surface.
    Scan {
        /// Project directory to inspect.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Parse requirement documents and generate acceptance criteria.
    Requirements {
        /// Requirement document or project directory to inspect.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// AI provider used to refine extraction. Mock keeps the offline rule engine.
        #[arg(long, value_enum, default_value_t = AiProviderKind::Mock)]
        provider: AiProviderKind,
        /// Disable the on-disk AI response cache (.specprobe/cache).
        #[arg(long)]
        no_cache: bool,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Run AI-assisted analysis over extracted requirements.
    Ai {
        /// Requirement document or project directory to inspect.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// AI provider to use. Mock works without API keys.
        #[arg(long, value_enum, default_value_t = AiProviderKind::Mock)]
        provider: AiProviderKind,
        /// Disable the on-disk AI response cache (.specprobe/cache).
        #[arg(long)]
        no_cache: bool,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Launch a detected project command and collect process evidence.
    Launch {
        /// Project directory to launch.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Maximum execution time before the process is killed.
        #[arg(long, default_value_t = 15)]
        timeout_secs: u64,
        /// Detect the command without executing it.
        #[arg(long)]
        dry_run: bool,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Build and execute an initial browser-oriented test plan.
    Browser {
        /// Requirement document or project directory to inspect.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Base URL for the web application under test.
        #[arg(long, default_value = "http://127.0.0.1:3000")]
        base_url: String,
        /// AI provider for generating concrete interaction steps (needs Playwright). Mock probes only.
        #[arg(long, value_enum, default_value_t = AiProviderKind::Mock)]
        provider: AiProviderKind,
        /// Disable the on-disk AI response cache (.specprobe/cache).
        #[arg(long)]
        no_cache: bool,
        /// Maximum page probe time.
        #[arg(long, default_value_t = 10)]
        timeout_secs: u64,
        /// Scenario sampling rounds (1-3). >1 unions detections across rounds for stability.
        #[arg(long, default_value_t = 1)]
        samples: u32,
        /// Generate the browser plan without probing the page.
        #[arg(long)]
        dry_run: bool,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Generate an evidence-backed issue list for user review.
    Review {
        /// Requirement document or project directory to inspect.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Project directory used for launch evidence.
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// Base URL for browser evidence.
        #[arg(long, default_value = "http://127.0.0.1:3000")]
        base_url: String,
        /// AI provider for requirement refinement and browser scenarios. Mock stays offline.
        #[arg(long, value_enum, default_value_t = AiProviderKind::Mock)]
        provider: AiProviderKind,
        /// Disable the on-disk AI response cache (.specprobe/cache).
        #[arg(long)]
        no_cache: bool,
        /// Execute launch and browser probes. Without this, review stays plan-only.
        #[arg(long)]
        execute: bool,
        /// Skip project launch evidence.
        #[arg(long)]
        skip_launch: bool,
        /// Skip browser evidence.
        #[arg(long)]
        skip_browser: bool,
        /// Maximum launch execution time.
        #[arg(long, default_value_t = 15)]
        launch_timeout_secs: u64,
        /// Maximum browser page probe time.
        #[arg(long, default_value_t = 10)]
        browser_timeout_secs: u64,
        /// Scenario sampling rounds (1-3). >1 unions detections across rounds for stability.
        #[arg(long, default_value_t = 1)]
        samples: u32,
        /// Also write a self-contained HTML report to this path.
        #[arg(long, value_name = "PATH")]
        html: Option<PathBuf>,
        /// Do not archive this run to the local run store (.specprobe/specprobe.db).
        #[arg(long)]
        no_store: bool,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Generate safe patch proposals and regression checks from review issues.
    Propose {
        /// Requirement document or project directory to inspect.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Project directory used for launch evidence.
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// Base URL for browser evidence.
        #[arg(long, default_value = "http://127.0.0.1:3000")]
        base_url: String,
        /// AI provider for requirement refinement and browser scenarios. Mock stays offline.
        #[arg(long, value_enum, default_value_t = AiProviderKind::Mock)]
        provider: AiProviderKind,
        /// Disable the on-disk AI response cache (.specprobe/cache).
        #[arg(long)]
        no_cache: bool,
        /// Execute launch and browser probes before generating proposals.
        #[arg(long)]
        execute: bool,
        /// Skip project launch evidence.
        #[arg(long)]
        skip_launch: bool,
        /// Skip browser evidence.
        #[arg(long)]
        skip_browser: bool,
        /// Maximum launch execution time.
        #[arg(long, default_value_t = 15)]
        launch_timeout_secs: u64,
        /// Maximum browser page probe time.
        #[arg(long, default_value_t = 10)]
        browser_timeout_secs: u64,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
}
