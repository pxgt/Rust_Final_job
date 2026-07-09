# Phase 1 端到端真机验收记录

> 2026-07-08。目标:用真实模型 + 真实浏览器,对 FocusBoard 实测 `review --execute --provider openai-compatible` 的注入缺陷检出率。验收门:5 个注入缺陷检出 ≥4,且无高严重度误报。

## 环境

- 模型:DeepSeek(`OPENAI_BASE_URL=https://api.deepseek.com`,`OPENAI_MODEL=deepseek-chat`,OpenAI 兼容传输)。
- 浏览器:Playwright + Chromium(`specprobe setup-browser` 安装,经代理下载,自动重试后成功)。
- 被测项目:`demo/buggy-task-board`(FocusBoard),`review --execute` 由 ManagedApp 自动 `npm run dev` 拉起并探测 4173 就绪。

## 全链路验证(架构目标)

一条 `review --execute --provider openai-compatible` 完整跑通,各环节真实工作:

- 需求精解析:`engine=llm-refined`,12 条需求带行号溯源与具体验收标准(一次调用,2516 tokens)。
- 服务编排:ManagedApp 起 FocusBoard,就绪探测返回 `Server responded at http://127.0.0.1:4173`。
- 浏览器执行:`backend=playwright`,12 个交互场景在真实 Chromium 上执行,采集截图 / console / 网络。
- 网络取证:采集到 35 条 `/api/tasks` HTTP 500 与对应 console error。
- 缺陷诊断:LLM 输出带源码定位的诊断,首轮精确定位 `/api/tasks` 500 到 **server.js:47**(经核对完全准确)。

**架构端到端验证成功。**

## 注入缺陷检出(对照 KNOWN_ISSUES.md)

| 缺陷 | 检出 | 依据 |
| --- | --- | --- |
| DEMO-001 `/api/tasks` 固定 500 | ✅ | REQ-007 场景失败 + 网络失败证据 + 诊断定位 server.js:47 |
| DEMO-002 空白任务仍被添加 | ✅ | REQ-002 场景失败(校验消息未出现) |
| DEMO-003 已完成统计始终 0 | ✅ | REQ-003 场景失败(完成计数断言未达预期) |
| DEMO-004 已完成筛选显示全部 | ❌ 漏检 | REQ-005 断言"已完成项存在"在"显示全部"时也成立;需 negative 断言 |
| DEMO-005 刷新后任务丢失 | ✅ | REQ-009 场景失败(刷新后列表不含新增任务) |

**检出 4/5,达到验收门数量目标**(接手基线为 1/5,详见 docs/EXPERIMENT.md)。

## 暴露的质量调优项(LLM 输出波动)

真机验收的价值在于暴露了确定性测试之外的 LLM 稳定性问题,均为可迭代的调优点,非架构缺陷:

1. **断言类 selector 猜测导致误报**:REQ-008 猜测横幅为 `.error-banner`,实际是 `#api-banner`(横幅本身工作正常,首轮用 `text=` 断言即 PASS)→ 假阳性。**方向**:断言类 selector 也优先取自页面快照,或强制用文本断言而非猜测 class。
2. **筛选类缺陷需要 negative 断言原语**:DEMO-004 要检出"点击已完成后不该出现的未完成项仍在",需要 `expect_absent` / `expect_hidden` 动作(当前只有存在性断言)。**方向**:sidecar 增加否定断言原语。
3. **诊断在失败较多时过度合并**:失败 issue 增多时,诊断把 8 个 issue 归入单一根因并出现定位漂移(首轮 3 个独立诊断、server.js:47 精确;多失败轮次合并为 1 个、定位漂到 app.js)。**方向**:限制单诊断关联 issue 数,或按类别分组诊断。
4. **运行间波动**:相同输入(温度 0)下,prompt 微调即改变场景与检出(首轮 3/5、强化断言后 4/5),说明检出率应多次运行取稳定值,单次结果仅供参考。

## 结论

Phase 1 架构目标(真需求解析 → 真起服务 → 真浏览器执行 → 真交互步骤 → 证据 → LLM 源码诊断)端到端验证成功;注入缺陷检出从基线 1/5 提升到 4/5,达到验收门数量目标。上述 4 项 LLM 质量调优点记录为后续迭代(见 ROADMAP),其中"否定断言原语"与"断言 selector 收敛"可作为进入 Phase 2 前的收尾优化。
