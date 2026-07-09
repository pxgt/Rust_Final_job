//! 浏览器测试场景生成(ROADMAP 1.5)。
//!
//! 输入需求(1.3 精解析结果)与首页可交互元素摘要(1.4 采集的 snapshot),
//! 由 LLM 生成带真实 selector 的具体动作步骤。生成后对操作类动作的 selector
//! 做静态校验(必须来自页面元素列表),不通过则复用 `run_chat_json` 的反馈重问
//! 回路自动修正。步骤的实际执行在 `browser` 模块完成。

use std::collections::HashSet;
use std::path::PathBuf;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::ai::{
    AiCache, AiError, AiProviderKind, AiTransportInfo, chat_protocol_for, run_chat_json,
    strip_code_fence,
};
use crate::playwright::{PageSnapshot, PlaywrightAction};
use crate::requirements::RequirementReport;

/// 一个需求对应的可执行浏览器场景。
#[derive(Debug, Clone)]
pub struct Scenario {
    pub requirement_id: String,
    pub title: String,
    pub expected_observation: String,
    pub steps: Vec<PlaywrightAction>,
}

pub struct ScenarioPlan {
    pub scenarios: Vec<Scenario>,
    pub notes: Vec<String>,
    pub transport: AiTransportInfo,
}

/// 用 LLM 生成场景。Mock Provider 返回空计划(调用方据此退回通用采集)。
pub async fn generate_scenarios(
    report: &RequirementReport,
    snapshot: &PageSnapshot,
    base_url: &str,
    provider: AiProviderKind,
    cache_dir: Option<PathBuf>,
) -> Result<ScenarioPlan, AiError> {
    let Some(protocol) = chat_protocol_for(provider)? else {
        return Ok(ScenarioPlan {
            scenarios: Vec::new(),
            notes: Vec::new(),
            transport: AiTransportInfo {
                attempts: 0,
                cache_hit: false,
                usage: None,
            },
        });
    };
    let cache = cache_dir.map(|dir| AiCache { dir });
    let messages = build_messages(report, snapshot);
    let base_url = base_url.to_owned();

    let (parsed, transport) =
        run_chat_json(&protocol, messages, cache.as_ref(), |content, lenient| {
            parse_scenarios(content, report, snapshot, &base_url, lenient)
        })
        .await?;

    Ok(ScenarioPlan {
        scenarios: parsed.scenarios,
        notes: parsed.notes,
        transport,
    })
}

#[derive(Debug)]
struct ParsedScenarios {
    scenarios: Vec<Scenario>,
    notes: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ScenarioPlanWire {
    #[serde(default)]
    scenarios: Vec<ScenarioWire>,
    #[serde(default)]
    notes: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ScenarioWire {
    requirement_id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    expected_observation: String,
    #[serde(default)]
    steps: Vec<ScenarioStepWire>,
}

/// LLM 步骤的扁平中间格式(对模型比 tagged enum 更友好),再转 `PlaywrightAction`。
#[derive(Debug, Deserialize)]
struct ScenarioStepWire {
    action: String,
    #[serde(default)]
    selector: Option<String>,
    #[serde(default)]
    value: Option<String>,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

fn parse_scenarios(
    content: &str,
    report: &RequirementReport,
    snapshot: &PageSnapshot,
    base_url: &str,
    lenient: bool,
) -> Result<ParsedScenarios, String> {
    let cleaned = strip_code_fence(content);
    let wire = serde_json::from_str::<ScenarioPlanWire>(cleaned)
        .map_err(|error| format!("content is not valid JSON for the expected schema: {error}"))?;

    let known_ids: HashSet<&str> = report
        .requirements
        .iter()
        .map(|requirement| requirement.id.as_str())
        .collect();
    let known_selectors: HashSet<&str> = snapshot
        .interactive
        .iter()
        .map(|element| element.selector.as_str())
        .collect();

    let mut problems = Vec::new();
    let mut scenarios = Vec::new();

    for (scenario_index, scenario) in wire.scenarios.iter().enumerate() {
        let id_ok = known_ids.contains(scenario.requirement_id.as_str());
        if !id_ok {
            problems.push(format!(
                "scenario {scenario_index} references unknown requirement_id {}",
                scenario.requirement_id
            ));
        }

        let mut steps = Vec::new();
        let mut all_steps_ok = true;
        for (step_index, step) in scenario.steps.iter().enumerate() {
            match to_action(step, &known_selectors, base_url, step_index) {
                Ok(action) => steps.push(action),
                Err(problem) => {
                    problems.push(format!("scenario {scenario_index}: {problem}"));
                    all_steps_ok = false;
                }
            }
        }
        if steps.is_empty() {
            problems.push(format!(
                "scenario {scenario_index} ({}) has no usable steps",
                scenario.requirement_id
            ));
            continue;
        }

        // 严格模式只保留完全有效的场景;宽容模式(最后一轮)保留可执行的部分。
        if id_ok && (all_steps_ok || lenient) {
            scenarios.push(Scenario {
                requirement_id: scenario.requirement_id.clone(),
                title: if scenario.title.trim().is_empty() {
                    format!("验证 {}", scenario.requirement_id)
                } else {
                    scenario.title.clone()
                },
                expected_observation: scenario.expected_observation.clone(),
                steps,
            });
        }
    }

    if !problems.is_empty() && !lenient {
        return Err(problems.join("; "));
    }
    if scenarios.is_empty() {
        return Err("no executable scenarios were produced".to_owned());
    }
    Ok(ParsedScenarios {
        scenarios,
        notes: wire.notes,
    })
}

/// 转换并校验单个步骤。操作类动作(click/fill/press)的 selector 必须来自页面
/// 元素列表;断言/等待类允许任意 CSS selector(可能指向非交互元素)。
fn to_action(
    step: &ScenarioStepWire,
    known_selectors: &HashSet<&str>,
    base_url: &str,
    index: usize,
) -> Result<PlaywrightAction, String> {
    let selector = || {
        step.selector
            .clone()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| format!("step {index} ({}) requires a selector", step.action))
    };
    let known_selector = || {
        let value = selector()?;
        if known_selectors.contains(value.as_str()) {
            Ok(value)
        } else {
            Err(format!(
                "step {index} uses selector '{value}' which is not in the page's interactive elements"
            ))
        }
    };

    match step.action.as_str() {
        "goto" => Ok(PlaywrightAction::Goto {
            url: step.url.clone().unwrap_or_else(|| base_url.to_owned()),
        }),
        "screenshot" => Ok(PlaywrightAction::Screenshot {
            name: step.name.clone().unwrap_or_else(|| format!("step-{index}")),
        }),
        "click" => Ok(PlaywrightAction::Click {
            selector: known_selector()?,
        }),
        "fill" => Ok(PlaywrightAction::Fill {
            selector: known_selector()?,
            value: step.value.clone().unwrap_or_default(),
        }),
        "press" => Ok(PlaywrightAction::Press {
            selector: known_selector()?,
            key: step
                .key
                .clone()
                .ok_or_else(|| format!("step {index} (press) requires a key"))?,
        }),
        "wait_for_selector" => Ok(PlaywrightAction::WaitForSelector {
            selector: selector()?,
        }),
        "expect_visible" => Ok(PlaywrightAction::ExpectVisible {
            selector: selector()?,
        }),
        "expect_hidden" => Ok(PlaywrightAction::ExpectHidden {
            selector: selector()?,
        }),
        "expect_text" => Ok(PlaywrightAction::ExpectText {
            selector: selector()?,
            text: step
                .text
                .clone()
                .ok_or_else(|| format!("step {index} (expect_text) requires text"))?,
        }),
        other => Err(format!("step {index} uses unsupported action '{other}'")),
    }
}

fn build_messages(report: &RequirementReport, snapshot: &PageSnapshot) -> Vec<Value> {
    vec![
        json!({"role": "system", "content": system_prompt()}),
        json!({"role": "user", "content": user_prompt(report, snapshot)}),
    ]
}

fn system_prompt() -> String {
    r#"You generate concrete browser test steps for an evidence-driven testing tool.
Given requirements and the interactive elements found on the page, produce one scenario per
requirement that can be executed with Playwright.

Respond with a single JSON object and nothing else, matching this schema exactly:
{
  "scenarios": [
    {
      "requirement_id": string,        // must be an id from the input list
      "title": string,
      "expected_observation": string,  // what a correct implementation should show
      "steps": [
        { "action": "fill", "selector": string, "value": string },
        { "action": "click", "selector": string },
        { "action": "expect_text", "selector": string, "text": string }
      ]
    }
  ],
  "notes": [string]
}

Allowed actions: goto, wait_for_selector, click, fill, press, expect_visible, expect_hidden, expect_text, screenshot.
Rules:
- For click, fill and press you MUST use a selector taken verbatim from the provided interactive elements.
- For assertions (expect_text / expect_visible / expect_hidden / wait_for_selector) you may target any CSS selector, but PREFER text-based checks: use expect_text with a container selector, or a `text=` selector, rather than guessing a class or id that is not in the provided elements. Guessed selectors like `.error-banner` cause false results — assert the visible TEXT the requirement promises instead.
- Each scenario must perform the requirement's action and then assert the requirement's concrete observable OUTCOME — the changed value, count, or list content — NOT merely that a label or button exists.
- When the requirement involves a count or statistic, assert the exact expected value with expect_text (e.g. the completed counter should read "1" after one item is completed). A bug that leaves the counter at 0 must make this assertion fail.
- When the requirement involves filtering or hiding, use expect_hidden to assert that items which should be filtered OUT are no longer visible (e.g. after clicking the "completed" filter, an active/unfinished task must become hidden). A bug that still shows everything must make this assertion fail.
- Prefer negative/edge cases when the requirement implies validation (e.g. submit an empty title and assert the specific validation message text appears).
- Do not use the eval action. Do not invent requirement ids. Do not wrap the JSON in markdown fences.
- Write title, expected_observation and notes in the requirements' language."#
        .to_owned()
}

fn user_prompt(report: &RequirementReport, snapshot: &PageSnapshot) -> String {
    let mut prompt = String::new();
    if let Some(title) = &snapshot.title {
        prompt.push_str(&format!("Page title: {title}\n"));
    }
    prompt.push_str("\nInteractive elements (selector — role — text):\n");
    if snapshot.interactive.is_empty() {
        prompt.push_str("(none detected)\n");
    }
    for element in &snapshot.interactive {
        prompt.push_str(&format!(
            "- {} — {} — {}\n",
            element.selector, element.role, element.text
        ));
    }
    prompt.push_str("\nRequirements to cover:\n");
    for requirement in &report.requirements {
        prompt.push_str(&format!(
            "{} [{}]: {}\n",
            requirement.id, requirement.category, requirement.description
        ));
    }
    prompt
}

#[cfg(test)]
mod tests {
    use super::{Scenario, generate_scenarios, parse_scenarios};
    use crate::ai::AiProviderKind;
    use crate::playwright::{InteractiveElement, PageSnapshot};
    use crate::requirements::analyze_requirements;
    use crate::testutil::temp_project;

    use std::fs;

    fn snapshot() -> PageSnapshot {
        PageSnapshot {
            title: Some("FocusBoard".to_owned()),
            interactive: vec![
                InteractiveElement {
                    tag: "input".to_owned(),
                    role: "input".to_owned(),
                    text: "新任务".to_owned(),
                    selector: "#task-input".to_owned(),
                },
                InteractiveElement {
                    tag: "button".to_owned(),
                    role: "button".to_owned(),
                    text: "添加任务".to_owned(),
                    selector: "#add-task-btn".to_owned(),
                },
            ],
        }
    }

    fn report() -> crate::requirements::RequirementReport {
        let root = temp_project("specprobe-scenario-report");
        let file = root.join("PRD.md");
        fs::write(&file, "- 用户必须能够添加任务并在列表中看到它。").expect("write requirement");
        let report = analyze_requirements(&file).expect("analysis succeeds");
        fs::remove_dir_all(root).expect("cleanup");
        report
    }

    fn valid_scenarios_json() -> String {
        serde_json::json!({
            "scenarios": [{
                "requirement_id": "REQ-001",
                "title": "添加任务",
                "expected_observation": "任务出现在列表中",
                "steps": [
                    {"action": "fill", "selector": "#task-input", "value": "写周报"},
                    {"action": "click", "selector": "#add-task-btn"},
                    {"action": "expect_text", "selector": "#task-list", "text": "写周报"}
                ]
            }],
            "notes": ["覆盖了添加任务的主路径。"]
        })
        .to_string()
    }

    #[test]
    fn parses_valid_scenarios_into_actions() {
        let report = report();
        let snapshot = snapshot();

        let parsed = parse_scenarios(
            &valid_scenarios_json(),
            &report,
            &snapshot,
            "http://127.0.0.1:4173",
            false,
        )
        .expect("valid scenarios parse");

        assert_eq!(parsed.scenarios.len(), 1);
        let scenario: &Scenario = &parsed.scenarios[0];
        assert_eq!(scenario.requirement_id, "REQ-001");
        assert_eq!(scenario.steps.len(), 3);
        assert_eq!(scenario.steps[1].label(), "click");
        assert_eq!(scenario.steps[1].target(), "#add-task-btn");
    }

    #[test]
    fn parses_expect_hidden_for_filtering() {
        let report = report();
        let snapshot = snapshot();
        let content = serde_json::json!({
            "scenarios": [{
                "requirement_id": "REQ-001",
                "title": "筛选隐藏未完成任务",
                "expected_observation": "点击已完成筛选后未完成任务被隐藏",
                "steps": [
                    {"action": "click", "selector": "#add-task-btn"},
                    {"action": "expect_hidden", "selector": "ul li:not(.completed)"}
                ]
            }],
            "notes": []
        })
        .to_string();

        let parsed = parse_scenarios(&content, &report, &snapshot, "http://x", false)
            .expect("expect_hidden parses");
        let steps = &parsed.scenarios[0].steps;
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[1].label(), "expect_hidden");
        assert_eq!(steps[1].target(), "ul li:not(.completed)");
    }

    #[test]
    fn rejects_operation_selector_outside_page_elements() {
        let report = report();
        let snapshot = snapshot();
        let content = valid_scenarios_json().replace("#add-task-btn", "#ghost-button");

        let feedback = parse_scenarios(&content, &report, &snapshot, "http://x", false)
            .expect_err("strict mode rejects unknown selector");
        assert!(feedback.contains("#ghost-button"));

        // 宽容模式丢弃无效步骤后,该场景剩余步骤仍可执行。
        let parsed =
            parse_scenarios(&content, &report, &snapshot, "http://x", true).expect("lenient");
        assert_eq!(parsed.scenarios[0].steps.len(), 2);
    }

    #[test]
    fn rejects_unknown_requirement_id() {
        let report = report();
        let snapshot = snapshot();
        let content = valid_scenarios_json().replace("REQ-001", "REQ-404");

        let feedback = parse_scenarios(&content, &report, &snapshot, "http://x", false)
            .expect_err("unknown id rejected");
        assert!(feedback.contains("REQ-404"));
    }

    #[tokio::test]
    async fn mock_provider_yields_empty_plan() {
        let report = report();
        let snapshot = snapshot();

        let plan = generate_scenarios(
            &report,
            &snapshot,
            "http://127.0.0.1:4173",
            AiProviderKind::Mock,
            None,
        )
        .await
        .expect("mock returns empty plan");

        assert!(plan.scenarios.is_empty());
        assert_eq!(plan.transport.attempts, 0);
    }
}
