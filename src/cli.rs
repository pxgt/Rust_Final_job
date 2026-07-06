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
pub enum Command {
    /// Check whether the local machine is ready for analysis and web testing.
    Doctor {
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
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
        /// Maximum page probe time.
        #[arg(long, default_value_t = 10)]
        timeout_secs: u64,
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
