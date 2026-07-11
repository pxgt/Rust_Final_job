use std::path::PathBuf;
use std::process::Command;

use assert_cmd::prelude::*;
use serde_json::Value;

fn project_path(relative: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(relative)
        .display()
        .to_string()
}

fn run_json(args: &[&str]) -> Value {
    let output = Command::cargo_bin("specprobe")
        .expect("specprobe binary should build")
        .args(args)
        .output()
        .expect("specprobe should run");
    assert!(
        output.status.success(),
        "specprobe {:?} failed:\n{}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON")
}

#[test]
fn doctor_json_has_report_structure() {
    let report = run_json(&["doctor", "--json"]);

    assert!(
        report["tools"]
            .as_array()
            .is_some_and(|tools| !tools.is_empty())
    );
    assert!(report["ai_providers"].is_array());
    assert!(report["core_ready"].is_boolean());
}

#[test]
fn scan_identifies_node_demo_project() {
    let report = run_json(&["scan", &project_path("demo/buggy-task-board"), "--json"]);

    let technologies = report["technologies"]
        .as_array()
        .expect("technologies should be an array");
    assert!(
        technologies.iter().any(|technology| {
            technology
                .as_str()
                .unwrap_or_default()
                .to_lowercase()
                .contains("node")
        }),
        "expected a Node technology in {technologies:?}"
    );
}

#[test]
fn requirements_json_matches_snapshot() {
    let report = run_json(&[
        "requirements",
        &project_path("tests/fixtures/demo-prd.md"),
        "--json",
    ]);

    insta::assert_json_snapshot!("requirements-demo-prd", report, {
        ".source" => "[fixture-path]",
    });
}

#[test]
fn launch_dry_run_detects_npm_command() {
    let report = run_json(&[
        "launch",
        &project_path("demo/buggy-task-board"),
        "--dry-run",
        "--json",
    ]);

    assert_eq!(report["adapter"], "node");
    assert_eq!(report["command"]["program"], "npm");
    assert_eq!(report["command"]["args"], serde_json::json!(["run", "dev"]));
    assert_eq!(report["execution"]["dry_run"], true);
    assert_eq!(report["execution"]["attempted"], false);
    assert_eq!(report["execution"]["long_running"], false);
}

#[test]
fn browser_dry_run_builds_action_plan() {
    let report = run_json(&[
        "browser",
        &project_path("tests/fixtures/demo-prd.md"),
        "--dry-run",
        "--json",
    ]);

    assert_eq!(report["execution"]["attempted"], false);
    assert_eq!(report["execution"]["dry_run"], true);
    // 夹具的 4 条需求中,UI、功能、模糊需求映射为浏览器用例,API 需求被排除。
    let cases = report["plan"]["cases"]
        .as_array()
        .expect("cases should be an array");
    assert_eq!(cases.len(), 3);
}

#[test]
fn review_plan_only_reports_pending_issues() {
    let report = run_json(&[
        "review",
        &project_path("tests/fixtures/demo-prd.md"),
        "--project",
        &project_path("demo/buggy-task-board"),
        "--no-store",
        "--json",
    ]);

    assert_eq!(report["config"]["execute"], false);
    assert_eq!(report["launch_report"]["execution"]["dry_run"], true);

    let issues = report["issues"].as_array().expect("issues array");
    assert!(!issues.is_empty());
    assert!(issues.iter().all(|issue| issue["approval"] == "pending"));
}

#[test]
fn check_declines_execution_without_confirmation() {
    // stdin 关闭(EOF)→ 确认被拒绝 → 安全降级为计划级。
    let report = run_json(&[
        "check",
        &project_path("demo/buggy-task-board"),
        "--no-html",
        "--no-store",
        "--json",
    ]);

    assert_eq!(report["executed"], false);
    assert_eq!(report["review"]["config"]["execute"], false);
    let technologies = report["profile"]["technologies"]
        .as_array()
        .expect("technologies array");
    assert!(!technologies.is_empty());
    assert!(
        report["review"]["summary"]["requirements"]
            .as_u64()
            .is_some_and(|count| count > 0)
    );
}

#[test]
fn propose_generates_proposal_per_issue_without_auto_apply() {
    let report = run_json(&[
        "propose",
        &project_path("tests/fixtures/demo-prd.md"),
        "--project",
        &project_path("demo/buggy-task-board"),
        "--json",
    ]);

    let issues = report["review"]["issues"]
        .as_array()
        .expect("issues array")
        .len();
    let proposals = report["proposals"]
        .as_array()
        .expect("proposals array")
        .len();
    assert_eq!(proposals, issues);
    assert!(proposals > 0);
    assert_eq!(report["summary"]["auto_apply_supported"], false);
    assert_eq!(report["summary"]["requires_user_approval"], true);
}
