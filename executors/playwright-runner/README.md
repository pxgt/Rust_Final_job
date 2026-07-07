# SpecProbe Playwright 执行器

浏览器测试执行器的 Node sidecar。SpecProbe(Rust)通过 stdin 发送一个 JSON 动作计划,本进程用 Playwright 逐动作执行,并通过 stdout 以 NDJSON(每行一个 JSON)回传事件。

## 安装

```powershell
cd executors/playwright-runner
npm install
npx playwright install chromium
```

或在项目根目录执行 `specprobe setup-browser` 一键完成。

## 协议(版本 1)

输入(stdin,单个 JSON,读到 EOF):

```json
{
  "protocol_version": 1,
  "base_url": "http://127.0.0.1:4173",
  "timeout_ms": 10000,
  "screenshot_dir": "/abs/path/.specprobe/runs/<id>",
  "actions": [{ "type": "goto", "url": "http://127.0.0.1:4173" }]
}
```

动作类型:`goto` `wait_for_selector` `click` `fill` `press` `expect_visible` `expect_text` `screenshot` `eval`。

输出(stdout,NDJSON)事件:`started` `action_result` `console` `page_error` `network_failed` `snapshot` `finished` `fatal`。

`snapshot` 事件包含页面标题与可交互元素摘要(建议 selector + 文本),供后续阶段生成具体交互步骤。

## 安全边界

`eval` 动作在被测页面上下文求值,与 `specprobe launch` 处于同一信任边界(用户显式运行自己的项目)。默认计划生成不产生 `eval` 动作。
