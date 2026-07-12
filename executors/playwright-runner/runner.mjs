#!/usr/bin/env node
// SpecProbe Playwright 执行器 sidecar。
//
// 协议(带版本号,与 Rust 端 src/playwright.rs 的 PROTOCOL_VERSION 对齐):
//   输入:stdin 读取到 EOF 的单个 JSON 计划对象
//     { protocol_version, base_url, timeout_ms, screenshot_dir, actions: [{ type, ... }] }
//   输出:stdout 每行一个 JSON 事件对象(NDJSON)
//     started / action_result / console / page_error / network_failed / snapshot / finished / fatal
//
// Rust 端负责创建 screenshot_dir 与超时;本 sidecar 只执行动作并如实上报事件。

import process from "node:process";

const PROTOCOL_VERSION = 1;

function emit(event) {
  process.stdout.write(`${JSON.stringify(event)}\n`);
}

async function readStdin() {
  const chunks = [];
  for await (const chunk of process.stdin) {
    chunks.push(chunk);
  }
  return Buffer.concat(chunks).toString("utf8");
}

// 收集页面内所有可交互元素的建议 selector 与文本,供后续阶段(1.5)生成具体步骤。
async function collectSnapshot(page) {
  const title = await page.title().catch(() => null);
  const interactive = await page
    .evaluate(() => {
      const items = [];
      const selectorFor = (el) => {
        if (el.id) return `#${el.id}`;
        const testId = el.getAttribute("data-testid");
        if (testId) return `[data-testid="${testId}"]`;
        if (el.name) return `${el.tagName.toLowerCase()}[name="${el.name}"]`;
        return el.tagName.toLowerCase();
      };
      const nodes = document.querySelectorAll(
        "button, a[href], input, select, textarea, [role=button]",
      );
      for (const el of nodes) {
        const text = (
          el.innerText ||
          el.value ||
          el.getAttribute("aria-label") ||
          el.getAttribute("placeholder") ||
          ""
        )
          .trim()
          .slice(0, 80);
        items.push({
          tag: el.tagName.toLowerCase(),
          role: el.getAttribute("role") || el.tagName.toLowerCase(),
          text,
          selector: selectorFor(el),
        });
        if (items.length >= 60) break;
      }
      return items;
    })
    .catch(() => []);
  return { title, interactive };
}

async function runAction(page, action, index, timeout, screenshotDir) {
  const kind = action.type;
  switch (kind) {
    case "goto":
      await page.goto(action.url, { waitUntil: "domcontentloaded", timeout });
      return { detail: `navigated to ${action.url}` };
    case "wait_for_selector":
      await page.waitForSelector(action.selector, { timeout });
      return { detail: `found ${action.selector}` };
    case "click":
      await page.click(action.selector, { timeout });
      return { detail: `clicked ${action.selector}` };
    case "fill":
      await page.fill(action.selector, action.value ?? "", { timeout });
      return { detail: `filled ${action.selector}` };
    case "press":
      await page.press(action.selector, action.key, { timeout });
      return { detail: `pressed ${action.key} on ${action.selector}` };
    case "expect_visible": {
      const visible = await page.isVisible(action.selector);
      if (!visible) throw new Error(`${action.selector} is not visible`);
      return { detail: `${action.selector} is visible` };
    }
    case "expect_hidden": {
      // 等待元素变为不可见或从 DOM 移除;超时说明它仍可见 → 断言失败。
      try {
        await page.waitForSelector(action.selector, { state: "hidden", timeout });
      } catch {
        throw new Error(`${action.selector} is still visible`);
      }
      return { detail: `${action.selector} is hidden or absent` };
    }
    case "expect_text": {
      const content = (await page.textContent(action.selector, { timeout })) ?? "";
      if (!content.includes(action.text)) {
        throw new Error(
          `${action.selector} text does not contain "${action.text}"`,
        );
      }
      return { detail: `${action.selector} contains expected text` };
    }
    case "screenshot": {
      const path = `${screenshotDir}/${action.name}.png`;
      await page.screenshot({ path, fullPage: true });
      return { detail: `screenshot saved`, path };
    }
    case "eval": {
      const value = await page.evaluate((expr) => {
        // 受控求值:被测项目本就在用户机器上运行,与 launch 同一信任边界。
        // eslint-disable-next-line no-eval
        return String(eval(expr));
      }, action.expression);
      return { detail: `eval result: ${String(value).slice(0, 200)}` };
    }
    default:
      throw new Error(`unknown action type: ${kind}`);
  }
}

// 时序敏感、可能 flaky 的动作:失败后重试一次(抗 flaky,ROADMAP 3.5)。
// goto/screenshot/eval 不重试(导航与快照不因抖动而暂时失败)。
const RETRIABLE_ACTIONS = new Set([
  "wait_for_selector",
  "click",
  "fill",
  "press",
  "expect_visible",
  "expect_hidden",
  "expect_text",
]);

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

// 执行动作;失败且可重试则等待后再试一次。返回 { ok, detail, path, attempts }。
async function runActionWithRetry(page, action, index, timeout, screenshotDir) {
  try {
    const result = await runAction(page, action, index, timeout, screenshotDir);
    return { ok: true, detail: result.detail ?? null, path: result.path ?? null, attempts: 1 };
  } catch (first) {
    if (!RETRIABLE_ACTIONS.has(action.type)) {
      return { ok: false, detail: first.message, path: null, attempts: 1 };
    }
    await sleep(500);
    try {
      const result = await runAction(page, action, index, timeout, screenshotDir);
      return {
        ok: true,
        detail: `${result.detail ?? "ok"} (passed on retry)`,
        path: result.path ?? null,
        attempts: 2,
      };
    } catch (second) {
      // 二次仍失败:抓一张失败截图作证据(截图本身失败不影响主流程)。
      let path = null;
      try {
        path = `${screenshotDir}/failure-${index}.png`;
        await page.screenshot({ path, fullPage: true });
      } catch {
        path = null;
      }
      return { ok: false, detail: second.message, path, attempts: 2 };
    }
  }
}

async function main() {
  const raw = await readStdin();
  let plan;
  try {
    // 剥离可能的 UTF-8 BOM(某些 shell 管道会注入)。
    plan = JSON.parse(raw.replace(/^﻿/, ""));
  } catch (error) {
    emit({ type: "fatal", message: `invalid plan JSON: ${error.message}` });
    process.exitCode = 1;
    return;
  }

  if (plan.protocol_version !== PROTOCOL_VERSION) {
    emit({
      type: "fatal",
      message: `unsupported protocol_version ${plan.protocol_version}, expected ${PROTOCOL_VERSION}`,
    });
    process.exitCode = 1;
    return;
  }

  let chromium;
  try {
    ({ chromium } = await import("playwright"));
  } catch (error) {
    emit({
      type: "fatal",
      message: `playwright is not installed: ${error.message}`,
    });
    process.exitCode = 1;
    return;
  }

  emit({ type: "started", protocol_version: PROTOCOL_VERSION });

  const timeout = Number(plan.timeout_ms) || 10000;
  let browser;
  let context;
  let allOk = true;
  try {
    browser = await chromium.launch({ headless: true });
    context = await browser.newContext();
    // 录制 trace 供事后排查(ROADMAP 3.5);tracing 失败不影响主流程。
    await context.tracing
      .start({ screenshots: true, snapshots: true })
      .catch(() => {});
    const page = await context.newPage();

    page.on("console", (message) => {
      emit({ type: "console", level: message.type(), text: message.text() });
    });
    page.on("pageerror", (error) => {
      emit({ type: "page_error", message: String(error) });
    });
    page.on("requestfailed", (request) => {
      emit({
        type: "network_failed",
        url: request.url(),
        failure: request.failure()?.errorText ?? null,
      });
    });
    page.on("response", (response) => {
      const status = response.status();
      if (status >= 400) {
        emit({ type: "network_failed", url: response.url(), status });
      }
    });

    const actions = Array.isArray(plan.actions) ? plan.actions : [];
    for (let index = 0; index < actions.length; index += 1) {
      const action = actions[index];
      const result = await runActionWithRetry(
        page,
        action,
        index,
        timeout,
        plan.screenshot_dir,
      );
      if (!result.ok) allOk = false;
      emit({
        type: "action_result",
        index,
        action: action.type,
        ok: result.ok,
        detail: result.detail,
        path: result.path,
        attempts: result.attempts,
      });
      if (!result.ok) {
        // 失败时 DOM 快照:供执行级修复回路把"失败当刻的页面状态"回喂 LLM(ROADMAP 1.8)。
        const failureSnapshot = await collectSnapshot(page);
        emit({
          type: "failure_snapshot",
          index,
          title: failureSnapshot.title,
          interactive: failureSnapshot.interactive,
        });
      }
    }

    const snapshot = await collectSnapshot(page);
    emit({ type: "snapshot", title: snapshot.title, interactive: snapshot.interactive });
  } catch (error) {
    allOk = false;
    emit({ type: "fatal", message: String(error?.message ?? error) });
  } finally {
    // 归档 trace(ROADMAP 3.5),再关浏览器;两步失败都不影响已产出的证据。
    if (context) {
      try {
        const tracePath = `${plan.screenshot_dir}/trace.zip`;
        await context.tracing.stop({ path: tracePath });
        emit({ type: "trace", path: tracePath });
      } catch {
        // tracing 不可用或写入失败:忽略
      }
    }
    if (browser) await browser.close().catch(() => {});
  }

  emit({ type: "finished", ok: allOk });
}

main().catch((error) => {
  emit({ type: "fatal", message: String(error?.message ?? error) });
  process.exitCode = 1;
});
