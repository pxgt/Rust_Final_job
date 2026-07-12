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

/// 一个执行时坏掉的场景:第一个失败步骤是操作类,未走到断言(ROADMAP 1.8)。
/// 断言失败的场景不属于此类——那是缺陷证据,不修复。
pub struct BrokenScenario {
    pub scenario: Scenario,
    /// `scenario.steps` 内第一个失败步骤的下标。
    pub failed_step: usize,
    /// 执行器报告的失败详情。
    pub failed_detail: String,
    /// 失败当刻的页面快照(执行器采集;可能缺失)。
    pub page_at_failure: Option<PageSnapshot>,
}

/// 执行级修复回路(ROADMAP 1.8):把坏场景的失败步骤 + 执行证据回喂 LLM 修正。
/// 护栏:修复只许改操作步骤与断言 selector,断言强度(expect_* 类型集合与
/// expect_text 文本)必须与原场景一致,防止修复回路把真缺陷检测"修"掉。
pub async fn repair_scenarios(
    broken: &[BrokenScenario],
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

    // selector 合法集:初始快照 ∪ 各失败快照(动态出现的元素只在失败快照里)。
    let mut merged = snapshot.clone();
    for item in broken {
        if let Some(page) = &item.page_at_failure {
            for element in &page.interactive {
                if !merged
                    .interactive
                    .iter()
                    .any(|existing| existing.selector == element.selector)
                {
                    merged.interactive.push(element.clone());
                }
            }
        }
    }

    let messages = build_repair_messages(broken, &merged);
    let base_url = base_url.to_owned();
    let (parsed, transport) =
        run_chat_json(&protocol, messages, cache.as_ref(), |content, lenient| {
            let parsed = parse_scenarios(content, report, &merged, &base_url, lenient)?;
            enforce_assertion_strength(&parsed, broken)?;
            Ok(parsed)
        })
        .await?;

    Ok(ScenarioPlan {
        scenarios: parsed.scenarios,
        notes: parsed.notes,
        transport,
    })
}

/// 断言强度护栏:每个修复场景的 expect_* 动作类型多重集与 expect_text 断言文本
/// 必须与原场景一致(断言 selector 允许调整)。不一致返回反馈文本触发重问。
fn enforce_assertion_strength(
    parsed: &ParsedScenarios,
    broken: &[BrokenScenario],
) -> Result<(), String> {
    for repaired in &parsed.scenarios {
        let Some(original) = broken
            .iter()
            .find(|item| item.scenario.requirement_id == repaired.requirement_id)
        else {
            return Err(format!(
                "scenario {} was not among the broken scenarios to repair",
                repaired.requirement_id
            ));
        };
        let signature = |scenario: &Scenario| {
            let mut labels: Vec<&str> = scenario
                .steps
                .iter()
                .filter(|step| step.is_assertion())
                .map(|step| step.label())
                .collect();
            labels.sort_unstable();
            let mut texts: Vec<String> = scenario
                .steps
                .iter()
                .filter_map(|step| match step {
                    PlaywrightAction::ExpectText { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect();
            texts.sort_unstable();
            (labels, texts)
        };
        if signature(repaired) != signature(&original.scenario) {
            return Err(format!(
                "scenario {}: the repaired steps weaken or change the assertions; keep the same expect_* actions and the same expect_text texts, only fix the failing operational step (and assertion selectors if needed)",
                repaired.requirement_id
            ));
        }
    }
    Ok(())
}

fn build_repair_messages(broken: &[BrokenScenario], snapshot: &PageSnapshot) -> Vec<Value> {
    vec![
        json!({"role": "system", "content": repair_system_prompt()}),
        json!({"role": "user", "content": repair_user_prompt(broken, snapshot)}),
    ]
}

fn repair_system_prompt() -> String {
    r#"You repair broken browser test scenarios for an evidence-driven testing tool.
Each scenario below failed at an OPERATIONAL step (navigation / clicking / filling) before its
assertions could run — the test itself is broken, not the application.

Fix each scenario using the execution evidence (failing step, error, page state at failure):
correct wrong selectors, add missing interaction steps, or reorder steps.

Hard rules:
- Do NOT weaken assertions. Keep exactly the same expect_visible / expect_hidden / expect_text
  actions and the same expect_text texts as the original scenario. You may fix an assertion's
  selector, never its intent.
- For click, fill and press use a selector taken verbatim from the provided interactive elements.
- Respond with the same JSON schema as scenario generation:
  {"scenarios": [{"requirement_id": ..., "title": ..., "expected_observation": ..., "steps": [...]}], "notes": [...]}
  Return one repaired scenario per broken scenario, same requirement_id. No markdown fences."#
        .to_owned()
}

fn repair_user_prompt(broken: &[BrokenScenario], snapshot: &PageSnapshot) -> String {
    let mut prompt = String::new();
    prompt.push_str("Interactive elements (selector — role — text):\n");
    for element in &snapshot.interactive {
        prompt.push_str(&format!(
            "- {} — {} — {}\n",
            element.selector, element.role, element.text
        ));
    }
    for item in broken {
        let scenario = &item.scenario;
        prompt.push_str(&format!(
            "\nBroken scenario for {} — {}\nExpected observation: {}\nSteps:\n",
            scenario.requirement_id, scenario.title, scenario.expected_observation
        ));
        for (index, step) in scenario.steps.iter().enumerate() {
            let marker = if index == item.failed_step {
                "  <-- FAILED HERE"
            } else {
                ""
            };
            prompt.push_str(&format!(
                "{index}. {} {}{marker}\n",
                step.label(),
                step.target()
            ));
        }
        prompt.push_str(&format!("Failure detail: {}\n", item.failed_detail));
        if let Some(page) = &item.page_at_failure {
            prompt.push_str("Page state at failure (interactive elements):\n");
            for element in &page.interactive {
                prompt.push_str(&format!(
                    "- {} — {} — {}\n",
                    element.selector, element.role, element.text
                ));
            }
        }
    }
    prompt
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

    fn scenarios_json_with_assert(assert_step: serde_json::Value) -> String {
        serde_json::json!({
            "scenarios": [{
                "requirement_id": "REQ-001",
                "title": "添加任务",
                "expected_observation": "任务出现在列表中",
                "steps": [
                    {"action": "fill", "selector": "#task-input", "value": "写周报"},
                    {"action": "click", "selector": "#add-task-btn"},
                    assert_step
                ]
            }],
            "notes": []
        })
        .to_string()
    }

    fn broken_original() -> Vec<super::BrokenScenario> {
        let report = report();
        let snapshot = snapshot();
        let original = parse_scenarios(
            &valid_scenarios_json(),
            &report,
            &snapshot,
            "http://x",
            false,
        )
        .expect("original parses")
        .scenarios
        .remove(0);
        vec![super::BrokenScenario {
            scenario: original,
            failed_step: 1,
            failed_detail: "click timed out".to_owned(),
            page_at_failure: None,
        }]
    }

    #[test]
    fn repair_guard_rejects_weakened_assertions() {
        let report = report();
        let snapshot = snapshot();
        let broken = broken_original();

        // 把 expect_text 弱化为 expect_visible → 拒绝并给出反馈。
        let weakened = scenarios_json_with_assert(
            serde_json::json!({"action": "expect_visible", "selector": "#task-list"}),
        );
        let parsed =
            parse_scenarios(&weakened, &report, &snapshot, "http://x", false).expect("parses");
        let feedback = super::enforce_assertion_strength(&parsed, &broken)
            .expect_err("weakened assertion rejected");
        assert!(feedback.contains("weaken"));

        // 改掉断言文本 → 同样拒绝。
        let changed_text = scenarios_json_with_assert(
            serde_json::json!({"action": "expect_text", "selector": "#task-list", "text": "别的内容"}),
        );
        let parsed =
            parse_scenarios(&changed_text, &report, &snapshot, "http://x", false).expect("parses");
        assert!(super::enforce_assertion_strength(&parsed, &broken).is_err());
    }

    #[test]
    fn repair_guard_allows_selector_fix_with_same_assertions() {
        let report = report();
        let snapshot = snapshot();
        let broken = broken_original();

        // 断言类型与文本不变,仅调整断言 selector(和操作步骤)→ 允许。
        let fixed = scenarios_json_with_assert(
            serde_json::json!({"action": "expect_text", "selector": "#todo-list", "text": "写周报"}),
        );
        let parsed =
            parse_scenarios(&fixed, &report, &snapshot, "http://x", false).expect("parses");
        assert!(super::enforce_assertion_strength(&parsed, &broken).is_ok());
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
