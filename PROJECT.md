# SpecProbe 项目文档

> 本文件是项目的持续维护台账。每次开发完成后都要更新“当前状态”“里程碑”“开发日志”和“下一步任务”。

## 1. 项目概述

- 项目名称：SpecProbe
- 项目起源：Rust 语言课程大作业（课程阶段 M0-M8 已于 2026-07-03 交付）
- 项目类型：可扩展的 AI 代码审查与自动化测试平台
- 当前版本：1.0.0（正式版 `v1.0.0`：ROADMAP 全部产品化阶段完成——Phase 1 真实化 / 1.8 检出稳定性 / 2 易用性 / 3 修复闭环；`v0.9.0` 为 1.8 合入前的回档基线）
- 当前阶段：项目已接手，按产品化目标推进，前瞻规划以 [docs/ROADMAP.md](docs/ROADMAP.md) 为唯一依据
- 最近更新：2026-07-06

SpecProbe 面向通过 AI Coding 工具生成或快速迭代的软件项目。目标形态（完整实现后）：读取项目代码和需求文档，生成结构化验收测试，运行被测项目并模拟真实使用过程，收集运行结果、日志、截图、网络请求和进程输出，最终给出带证据的问题清单与修改建议；用户决定是否应用修改，系统在修改后执行回归测试。当前版本与该目标的差距、以及逐步补齐的计划，见 [docs/ROADMAP.md](docs/ROADMAP.md) 第 1 节接手评估。

SpecProbe 的总体定位不限定语言、框架或应用形态。系统通过项目适配器、测试执行器和 AI Provider 等可替换组件支持不同类型的软件。Web 项目是第一个完整适配目标，这属于实现顺序，而不是平台能力边界。

## 2. 问题背景

宽泛需求会让 AI 生成的项目停留在初稿状态，常见问题包括：

- 核心功能只实现了理想路径，边界条件和错误处理缺失。
- 文档描述与实际功能不一致。
- 页面可以打开，但交互流程、状态反馈或导航逻辑不符合预期。
- 代码量较大，非专业用户无法快速定位问题。
- 单纯让 AI 阅读源码缺少运行证据，容易产生误判。

SpecProbe 的核心原则是：AI 负责理解和推理，确定性程序负责执行与取证，用户保留最终修改权。

## 3. 课程阶段范围(历史记录)

> 本节描述课程阶段(2026-06-15 至 2026-07-03)的交付范围,保留作历史参照。接手后的规划见 [docs/ROADMAP.md](docs/ROADMAP.md)。

### 3.1 课程阶段交付目标

- Markdown/TXT 需求文档。
- 通用项目结构、编程语言、构建清单和测试文件识别。
- Web 项目的启动方式识别，作为首个项目适配器。
- 从需求中提取功能点与验收标准。
- 生成与执行器无关的结构化测试计划。
- 将通用测试计划转换为可重复执行的浏览器测试步骤。
- 执行打开页面、点击、输入、等待和断言。
- 捕获截图、控制台错误、HTTP 错误和项目日志。
- 结合需求、运行证据和源码生成问题清单。
- 用户接受、拒绝或忽略修改建议。
- 以 Git patch 形式生成修改，并进行回归测试。

### 3.2 课程阶段未实现项

- 命令行、桌面和移动应用的完整测试执行器。
- 所有语言与框架的项目启动适配器。
- 无需审批的自主代码修改。
- 无证据的纯主观 UI 审美评分。
- 执行不受限制的 AI 生成 Shell 命令。

以上项目不属于架构上的禁止能力。后续可通过新增适配器和执行器逐步支持，无需改变需求、证据、问题与审批等核心数据模型。

## 4. 总体流程

```text
需求文档 + 项目代码
        |
        v
项目扫描与技术栈识别
        |
        v
需求提取与验收标准生成
        |
        v
结构化测试计划
        |
        v
项目适配器启动被测程序
        |
        v
对应测试执行器模拟使用行为
        |
        v
截图 / 日志 / 网络请求 / 断言结果
        |
        v
AI 缺陷诊断与源码定位
        |
        v
问题列表与修改建议
        |
        v
用户审批 -> Git patch -> 回归测试
```

## 5. 技术方案

### 5.1 Rust 核心

- `clap`：命令行界面。
- `serde` / `serde_json`：测试计划、证据和报告序列化。
- `thiserror` / `anyhow`：分层错误处理。
- `tokio`：异步运行时，负责子进程、超时与后续并发编排（1.1 已引入）。
- `reqwest`（rustls）：页面探测与后续 AI 传输、就绪轮询（1.1 已引入）。
- SQLite：后续保存项目、测试执行和问题历史。

### 5.2 AI 层

AI Provider 通过 trait 抽象，当前已实现离线 Mock Provider，并预留 OpenAI 兼容 Provider 与 Ollama Provider 的配置入口。AI 层接收 M2 输出的结构化需求报告，返回受 Schema 约束的结构化建议，不直接获得无限制命令执行权限。

当前 Mock Provider 用确定性规则模拟 AI 分析，用于课程演示、离线开发和回归测试。真实云端模型调用需要 API key；本地 Ollama 调用需要确认本地模型名称。

### 5.3 项目适配层

项目适配器负责识别、构建、启动和停止特定类型的被测项目。首个完整适配目标是 Web 项目，后续可以增加 CLI、后端服务、桌面应用和移动应用适配器。

### 5.4 测试执行层

测试执行器通过统一接口接收测试计划并返回结构化证据。浏览器执行器采用 Playwright Node sidecar 方案（决策依据见 ROADMAP §4 的 1.4 节）；后续可扩展命令行执行器、API 执行器、桌面 UI 执行器等。Rust 负责计划、调度、进程管理和结果归档。

### 5.5 扩展机制

- `ProjectAdapter`：识别技术栈并管理被测程序生命周期。
- `TestExecutor`：将通用测试步骤映射为具体操作。
- `EvidenceCollector`：采集不同运行环境下的日志、截图和响应。
- `AiProvider`：接入云端或本地大语言模型。
- `ReportRenderer`：输出终端、JSON、HTML 或后续图形界面报告。

核心流程只依赖这些抽象接口，不直接依赖 Playwright 或某一种项目框架。

### 5.6 安全边界

- 默认只读分析被测项目。
- 启动命令采用白名单或显式审批。
- 修改以补丁形式展示，不直接覆盖用户代码。
- 每条问题必须关联需求、复现步骤和运行证据。
- 日志进入 AI 前进行密钥和敏感字段脱敏。

## 6. 当前代码结构

```text
.
|-- .github/
|   `-- workflows/
|       `-- ci.yml
|-- Cargo.toml
|-- PROJECT.md
|-- README.md
|-- tests/
|   |-- cli.rs
|   |-- fixtures/
|   |   `-- demo-prd.md
|   `-- snapshots/
|-- demo/
|   `-- buggy-task-board/
|       |-- public/
|       |   |-- app.js
|       |   |-- index.html
|       |   `-- styles.css
|       |-- KNOWN_ISSUES.md
|       |-- README.md
|       |-- REQUIREMENTS.md
|       |-- package.json
|       `-- server.js
|-- docs/
|   |-- COURSE_REPORT.md
|   |-- DEMO_GUIDE.md
|   |-- EXPERIMENT.md
|   `-- specprobe-requirements.md
|-- executors/
|   `-- playwright-runner/     # 浏览器执行器 Node sidecar
|       |-- package.json
|       |-- runner.mjs
|       `-- README.md
|-- scripts/
|   |-- cargo-msvc.ps1
|   `-- run-demo.ps1
`-- src/
    |-- ai.rs
    |-- main.rs
    |-- lib.rs
    |-- cli.rs
    |-- check.rs
    |-- config.rs
    |-- storage.rs
    |-- ui.rs
    |-- doctor.rs
    |-- browser.rs
    |-- playwright.rs
    |-- scenario.rs
    |-- refine.rs
    |-- diagnosis.rs
    |-- report.rs
    |-- output.rs
    |-- remediation.rs
    |-- requirements.rs
    |-- review.rs
    |-- runtime.rs
    |-- scanner.rs
    `-- testutil.rs
```

- `cli.rs`：CLI 参数与子命令定义。
- `check.rs`：一键检查主入口(scan→精解析→编排执行→浏览器→诊断,含启动命令交互确认)。
- `config.rs`：`specprobe.toml` 项目配置(逐字段优先级合并)与 `init` 模板生成。
- `ui.rs`：终端进度 spinner(indicatif 封装,非 TTY / `--json` 自动静默;阶段回调解耦)。
- `storage.rs`：运行归档与 SQLite 索引(rusqlite bundled),`runs list/show`、`issues list/show/accept/reject/ignore`,审批按 Issue 指纹跨 run 持久继承。
- `patch.rs`：LLM 真补丁生成(`fix` 命令)。读诊断定位的源文件全文 → 生成 unified diff,parse 回路强制"只改提供文件"+ `git apply --check` 通过(不过则反馈重问)。仅生成不应用。
- `apply.rs`：补丁安全应用(`fix --apply`)。前置校验 git 仓库 + 工作区干净 → 隔离分支 `specprobe/fix-<issue>` 应用并提交 → 切回原分支;失败自动回滚。绝不碰用户当前分支工作区。
- `regression.rs`：修复回归验证(`fix --apply --verify`)。`git worktree` 物化修复分支重跑评审,按 Issue 指纹对比修复前后(纯裁决 `evaluate` + 编排 `verify_on_branch`)。目标缺陷消失且无新增 → 通过;否则回滚分支。
- `redact.rs`：出站脱敏。发给 LLM 的所有消息在 `run_chat_json` 单一入口过密钥脱敏(敏感键的值 + 已知令牌前缀)。
- `ai.rs`：AI Provider 抽象、Mock Provider、OpenAI/Ollama 配置入口和 AI 增强报告。
- `doctor.rs`：核心开发环境、首版 Web 测试环境和 AI Provider 配置诊断。
- `browser.rs`：浏览器动作计划生成、Playwright 深度执行编排、HTTP 探测降级和证据归档。
- `playwright.rs`：Playwright sidecar 协议(动作/事件)、runner 探测、事件流聚合与 `setup-browser` 安装。动作失败重试一次(attempts)、trace 归档(trace_path)、事件流无 finished 判为崩溃(3.5)。
- `scenario.rs`：基于 DOM 摘要 + 需求用 LLM 生成带真实 selector 的浏览器交互场景(含 selector 静态校验与反馈重问)。
- `diagnosis.rs`：对运行期失败叠加 LLM 深度诊断——启发式源码检索 + 带源码定位与置信度的结构化诊断。
- `report.rs`：把审查报告渲染为自包含 HTML(minijinja 模板 + base64 内联截图,light/dark 自适应)。
- `refine.rs`：需求理解两级流水线(规则粗筛 + LLM 精解析)。
- `testutil.rs`：测试共享工具(临时项目、需求夹具、假聊天端点)。
- `executors/playwright-runner`：浏览器执行器 Node sidecar(stdin 收计划、stdout 回 NDJSON 事件)。
- `remediation.rs`：把问题清单转换为修复提案、补丁预览和回归检查清单。
- `requirements.rs`：Markdown/TXT 需求解析、验收标准生成和测试计划生成。
- `review.rs`：综合需求、启动与浏览器证据，生成结构化问题清单和审批状态。
- `scanner.rs`：项目文件、技术栈、需求文档和测试文件识别。
- `output.rs`：人类可读及 JSON 输出。
- `scripts/cargo-msvc.ps1`：为当前开发机加载 MSVC 环境后执行 Cargo。
- `scripts/run-demo.ps1`：构建工具、启动 FocusBoard 并生成完整演示报告。
- `runtime.rs`：项目启动命令识别、一次性受控运行,以及 `ManagedApp` 托管生命周期(启动→就绪探测→保持运行→进程树 kill 关停)。
- `demo/buggy-task-board`：包含五类预置缺陷的零依赖 Node.js 演示项目。
- `docs/DEMO_GUIDE.md`：课堂演示步骤和推荐讲解顺序。
- `docs/EXPERIMENT.md`：自动化指标、已知缺陷覆盖和浏览器复核结果。
- `docs/COURSE_REPORT.md`：课程报告 Markdown 初稿。

## 7. 数据模型草案

后续核心对象：

- `Requirement`：需求描述、来源位置、类别、优先级、验收标准和质量提示。
- `AcceptanceCriterion`：可验证的预期行为和所需证据类型。
- `TestPlan`：由需求生成的初始测试用例集合。
- `TestStep`：动作、目标和输入。
- `AiProvider`：AI 调用抽象，统一 Mock、OpenAI 兼容接口和 Ollama。
- `AiModelOutput`：AI 增强分析结果，包括摘要、建议、追问和置信度。
- `AiSuggestion`：针对单条需求的改进建议、严重程度和理由。
- `LaunchReport`：被测项目启动命令、执行状态、日志摘要和诊断信息。
- `LaunchExecution`：是否执行、是否超时、退出码、耗时和超时阈值。
- `BrowserRunReport`：浏览器执行器报告，包含动作计划、页面探测证据和诊断信息。
- `BrowserActionPlan`：由通用测试计划转换而来的浏览器动作集合。
- `PageProbeEvidence`：页面状态码、标题、正文摘要、响应大小和耗时。
- `ReviewReport`：综合审查报告，包含配置、摘要、原始证据和问题清单。
- `EvidenceItem`：需求质量、启动命令、进程输出、浏览器计划或页面探测证据。
- `Issue`：严重程度、问题类别、预期结果、实际结果、证据编号和建议。
- `ApprovalState`：问题审批状态，目前默认 `pending`，后续支持用户接受、拒绝或忽略。
- `RemediationReport`：修复提案报告，包含审查报告、提案摘要、补丁提案和回归计划。
- `PatchProposal`：关联 `Issue` 的候选修复方案，包含目标文件、步骤、风险和补丁预览。
- `RegressionCheck`：修复后的验证命令或人工检查步骤。
- `Suggestion`：AI 层修改说明，后续可与候选补丁和审批状态关联。
- `TestRun`：一次执行的环境、时间、结果和回归关系。

## 8. 里程碑

课程阶段里程碑(已全部完成,历史记录);接手后的阶段规划(Phase 0-4)见 [docs/ROADMAP.md](docs/ROADMAP.md)。

| 里程碑 | 内容 | 状态 |
| --- | --- | --- |
| M0 | Rust/MSVC 开发环境准备 | 已完成 |
| M1 | CLI、环境诊断、项目扫描 | 已完成 |
| M2 | 需求文档解析与结构化验收标准 | 已完成 |
| M3 | AI Provider 抽象与模型调用 | 已完成 |
| M4 | 被测项目启动与进程日志采集 | 已完成 |
| M5 | 浏览器测试执行器基础版 | 已完成 |
| M6 | 缺陷报告、证据链和审批界面 | 基础版已完成 |
| M7 | 补丁生成与回归测试 | 基础版已完成 |
| M8 | 演示项目、实验评估和课程报告 | 基础版已完成 |

## 9. 当前状态（截至课程阶段交付，2026-07-03）

> 接手评估对下列已完成项的真实深度做了逐项核对（哪些是完整实现、哪些是占位），结论见 [docs/ROADMAP.md](docs/ROADMAP.md) §1。

已完成：

- 安装 Rust 1.96.0 MSVC 工具链、Cargo、Clippy、rustfmt 和 rust-analyzer。
- 验证 Visual Studio C++ 编译器与 Windows SDK 可用。
- 初始化 Git 仓库和 Rust 2024 Edition 二进制项目。
- 确定首版产品范围、系统流程和安全边界。
- 实现 `doctor` 环境诊断。
- 实现 `scan` 项目扫描。
- 实现 `requirements` 需求解析。
- 定义 `Requirement`、`AcceptanceCriterion`、`TestPlan` 和 `TestCase` 等结构化模型。
- 支持 Markdown/TXT 文件和项目目录两种输入模式。
- 从需求中推断类别、优先级、证据类型和执行器建议。
- 识别模糊描述、过宽需求和缺少可观察结果等质量问题。
- 实现 `ai` 增强分析命令。
- 定义 `AiProvider` trait、`AiModelOutput`、`AiSuggestion` 等 AI 层模型。
- 实现离线 Mock Provider，不需要 API key 即可生成稳定建议。
- 预留 OpenAI 兼容 Provider 与 Ollama Provider 的配置和错误路径。
- 实现 `launch` 项目启动与日志采集命令。
- 支持 Node、Rust、Python 项目的初步启动命令识别。
- 支持 dry-run 模式、受控超时、进程 kill、stdout/stderr 摘要、退出码和耗时记录。
- 支持 Windows `.cmd/.bat` 命令执行与 UTF-8 BOM `package.json` 解析。
- 对日志中的 token、password、secret、api key 等敏感行进行脱敏。
- 实现 `browser` 浏览器动作计划与页面探测命令。
- 将 M2 的 `TestPlan` 转换为打开页面、等待、交互、断言和采集证据等浏览器动作。
- 支持浏览器执行器 dry-run 模式。
- 支持本地 `http://` 页面探测，采集 HTTP 状态码、页面标题、正文摘要、响应大小和耗时。
- 支持 chunked HTTP 响应正文解码，避免摘要中出现传输编码噪声。
- 实现 `review` 综合审查命令。
- 定义 `ReviewReport`、`EvidenceItem`、`Issue`、`ApprovalState` 等缺陷报告模型。
- 将需求质量提示、启动命令结果、进程输出、浏览器动作计划和页面探测结果统一为证据项。
- 可根据缺少可观察结果、无法准备启动命令、进程失败、浏览器探测失败等证据生成问题清单。
- 每个问题包含严重程度、类别、预期结果、实际结果、证据编号、修改建议和默认待审批状态。
- `review` 默认计划级审查，只有 `--execute` 才执行启动和页面探测，符合默认安全边界。
- 实现 `propose` 修复提案命令。
- 定义 `RemediationReport`、`PatchProposal`、`PatchStrategy`、`PatchSafety`、`RegressionPlan` 和 `RegressionCheck`。
- 将 M6 的 `Issue` 问题清单转换为可审批的修复提案。
- 对需求澄清和启动配置类问题生成 Git patch 风格的补丁预览。
- 对每个修复提案生成回归检查命令，并提供全局综合审查回归命令。
- `propose` 默认不执行被测项目，也不自动修改用户代码，只输出预览和检查清单。
- 创建 FocusBoard 故障注入 Web 项目，包含 API 500、空输入校验、统计更新、筛选和持久化五类缺陷。
- 演示项目只使用 Node.js 内置模块，不需要安装第三方 npm 依赖。
- 实现 `scripts/run-demo.ps1`，自动运行 scan、requirements、ai、launch、browser、review 和 propose。
- 演示流程生成 7 份 JSON 报告并保存到 `.specprobe/demo-reports`。
- 实验提取 12 条需求和 12 个测试用例，生成 15 条 Mock AI 建议。
- 首页探测返回 HTTP 200，故障 API 返回 HTTP 500 并生成高严重度浏览器失败 Issue。
- 综合生成 4 个 Issue、4 个修复提案、3 个补丁预览和 9 个回归检查。
- 浏览器人工复现五个预置缺陷，并验证 375px 页面无横向溢出。
- 完成课堂演示指南、实验评估记录和课程报告初稿。
- 支持人类可读与 JSON 两种输出。
- 建立 29 个单元测试并验证真实编译。
- 通过 `cargo fmt` 与严格模式 Clippy 检查。

当前 M0-M8 基础交付均已完成，已经形成可运行、可测试、可演示的课程项目闭环。Playwright 深度执行、真实模型传输、审批持久化和自动应用补丁仍属于后续增强能力。

已知环境事项：

- 当前 Visual Studio 安装未被 `vswhere` 正确注册。
- 普通 PowerShell 不会自动包含 `link.exe`。
- 使用 `scripts/cargo-msvc.ps1` 加载 `vcvars64.bat` 后执行 Cargo。
- PowerShell 下运行严格 Clippy 时使用 `.\scripts\cargo-msvc.ps1 --% clippy --all-targets -- -D warnings`，避免 `--` 被脚本参数解析吞掉。
- 本机直连 GitHub / crates.io 不通，需走本地代理（127.0.0.1:7897）。git 已配置仓库级 `http.proxy` 与 `http.sslBackend openssl`（系统级 gitconfig 强制的 schannel 后端经代理握手失败）。
- cargo 更新依赖时 schannel 证书吊销检查经代理会失败，报 `SSL connect error`。临时方案：`$env:CARGO_HTTP_CHECK_REVOKE = "false"` 后再执行 cargo；若要持久化，可自行在 `~/.cargo/config.toml` 写入 `[http]` `check-revoke = false`（有轻微安全权衡，故未代为写入）。

## 10. 下一步任务

> 2026-07-06 起，本节不再维护独立任务清单。全部前瞻规划（Phase 0-4、验收门、技术选型、风险对策）以 [docs/ROADMAP.md](docs/ROADMAP.md) 为唯一依据。

Phase 0 地基修复已于 2026-07-06 完成，验收门达成（CI ubuntu + windows 双平台绿灯）：

- 0.1 提交历史与远程备份：完成，远程仓库 `https://github.com/pxgt/Rust_Final_job`。
- 0.2 CI：完成，`.github/workflows/ci.yml`（ubuntu + windows 矩阵，fmt/clippy/test）。
- 0.3 测试基建：完成，7 个 CLI 集成测试 + requirements JSON 快照（32 单测 + 7 集成全过）。
- 0.4 小缺陷修复：完成（markdown 链接文本保留、launch `long_running` 语义、`.gitattributes`）。

Phase 1（真实化)已全部完成并通过端到端真机验收：1.1-1.7 子阶段（2026-07-06 至 07-08）。**真机验收（2026-07-08,DeepSeek + Chromium）：FocusBoard 注入缺陷检出 4/5（基线 1/5),达到验收门数量目标,详见 [docs/ACCEPTANCE.md](docs/ACCEPTANCE.md)。** Phase 1.8 三项质量调优机制已完成(否定断言原语、断言 selector 收敛、诊断防过度合并);剩余稳定性问题(LLM 场景生成波动)需执行级修复回路,列为 ROADMAP 1.8 遗留。

用户选择走 Phase 2（易用性)。**已完成 2.6 HTML 报告**（插队优先,`review --html`)与 **2.1 一键 `check` 命令**（主入口,scan→精解析→编排执行→浏览器→诊断→HTML,启动命令交互确认安全边界)。**2.2 配置文件 `specprobe.toml`**（`init` 生成模板,优先级 CLI > 环境 > 配置 > 默认,check 已接入)。**2.3 终端体验**(indicatif 阶段 spinner,进度走 stderr、非 TTY/`--json` 自动静默)。**2.4 运行归档与 SQLite 存储**(每次运行归档 report.json + `.specprobe/specprobe.db` 索引,`runs list/show`)。**2.5 审批工作流**(`issues accept/reject/ignore`,审批按 Issue 指纹跨 run 持久继承)、**2.7 跨平台**(CI 三平台矩阵 + lint/test 拆分)。**Phase 2 易用性全部完成。** 下一步 Phase 3(修复闭环:真补丁生成 + 安全应用 + 自动回归)或 Phase 1.8 遗留(场景执行级修复回路)。

AI Provider 环境变量约定：OpenAI 兼容端点需要 `OPENAI_API_KEY` + `OPENAI_MODEL`（`OPENAI_BASE_URL` 默认 api.openai.com/v1，DeepSeek 设为 `https://api.deepseek.com` + 模型 `deepseek-chat`）；Ollama 需要 `OLLAMA_MODEL`（`OLLAMA_BASE_URL` 默认 127.0.0.1:11434）；云端调用经代理时设置 `HTTPS_PROXY`。

环境变量约定不变：Mock Provider 无需 API key；OpenAI 兼容接口需要 `OPENAI_API_KEY` 和 `OPENAI_MODEL`；本地 Ollama 需要 `OLLAMA_MODEL`。

## 11. 风险记录

> 接手后的风险与对策以 [docs/ROADMAP.md](docs/ROADMAP.md) §9 为准；下表为课程阶段记录，保留作历史参照。

| 风险 | 影响 | 缓解方案 |
| --- | --- | --- |
| 项目范围过大 | 无法在课程周期内形成完整演示 | 保留通用扩展架构，课程版本只交付 Web 适配器和受限测试动作 |
| AI 输出不稳定 | 测试无法重复或报告误判 | JSON Schema、确定性执行器、证据校验 |
| 执行被测项目存在安全风险 | 执行恶意脚本或泄露数据 | 命令审批、超时、目录限制，后续考虑容器 |
| UI 判断过于主观 | 结果缺乏可信度 | 优先检测可访问性、布局溢出和流程状态 |
| 模型 API 不可用 | 演示中断 | Mock Provider、本地 Ollama、结果缓存 |

## 12. 开发日志

### 2026-07-12（v1.0.0 正式版发布)

- 最终复盘:代码零 TODO/FIXME,fmt/clippy/103 测试(95 单测 + 8 集成)全绿,.gitignore 与入库文件干净,三平台 CI 绿;ACCEPTANCE/ROADMAP/THREAT_MODEL 均为最新。
- 发布整理:版本 0.9.0→1.0.0;README 清理 1.8 之前的过时检出率表述("3~4/5 波动"与"≥4/5"并存的矛盾),"当前状态"重写为四阶段完成 + 真机验收轨迹 + 已知边界;`--samples` 补进命令清单;删除已合并的 `phase-1.8-repair-loop` 分支(本地 + 远程)。
- 遗留(如实记录于 ROADMAP,不阻塞 1.0):`runs diff`、彩色输出/miette、Phase 4 广度(决定不做)。

### 2026-07-12（Phase 1.8 场景执行级修复回路,分支 phase-1.8-repair-loop)

- **分类规则(防目标冲突的核心设计)**:场景第一个失败步骤是操作类(goto/wait/click/fill/press)→ 场景本身坏了(selector 错/漏步骤),可修复;是断言类(expect_*)→ 这是缺陷证据,绝不修复;步骤未执行(detail=None,前置失败/执行器中断)→ 不修复。`PlaywrightAction::is_assertion` + `collect_broken_scenarios`。
- **失败证据采集**:runner.mjs 动作最终失败时采集当刻 DOM 快照,发 `failure_snapshot` 事件(带动作下标);Rust `PlaywrightOutcome.failure_snapshots`。协议 v1 向后兼容。
- **修复回路**(`scenario::repair_scenarios` + `browser::repair_round`):坏场景带失败步骤/错误详情/失败时页面状态回喂 LLM;selector 合法集 = 初始快照 ∪ 失败快照(动态元素);复用 run_chat_json 反馈重问;只重执行修正的场景,按需求 ID 替换结果;一轮上限。
- **断言强度护栏**(`enforce_assertion_strength`):修复输出的 expect_* 动作类型多重集与 expect_text 断言文本必须与原场景一致,只许修操作步骤与断言 selector;违反则反馈重问。防止修复回路把真缺陷检测"修"掉。
- **修复成功的"原谅"语义**:第一次执行中已修复场景的失败动作降为 Info 诊断(带 repaired 标注),整体 success 以修复后场景结果为准(fatal 除外);ScenarioResult 新增 `repaired` 字段。修复失败保留原始失败证据。
- 真机 e2e(真 sidecar + demo 服务 + 顺序应答假 LLM)双向验证:①操作失败→修复→重执行通过,repaired=true、execution.success=true、LLM 恰 2 次调用;②断言失败→不触发修复(LLM 恰 1 次调用)、缺陷证据原样保留。
- 94 单测(+5:3 分类 + 2 护栏)+ 8 集成全过,严格 Clippy 无警告。

**同日:多次采样取一致(1.8 第二块)**
- `--samples N`(1~3,check/review/browser):`generate_scenarios` 增 `variant: Option<(round,total)>`,prompt 注入轮次标记(区分缓存键——温度 0 下同 prompt 会命中缓存返回相同样本——并引导本轮独立设计);每轮独立生成/执行/修复(`run_scenario_sample`)。
- 合并规则 `merge_sample_results`:**检出并集**——任一轮失败即判失败并保留该轮证据(标题/步骤/截图),已失败不被后续轮通过"洗白";新需求结果直接加入。首轮证据为主证据,后续轮 run_dir 记入诊断。
- 依据:真机验收观察到的波动全是漏检方向(漏步骤/编造 expect_hidden 目标致假通过/断言强度回退),故取检出上界;误报侧由 2.5 审批指纹持久兜底。
- 真机 e2e(--samples 2,顺序假 LLM):弱断言轮通过+强断言轮失败 → 合并判失败附强断言轮证据,"union of detections" 诊断,恰 2 次生成调用。
- 95 单测(+1 合并)+ 8 集成全过,严格 Clippy 无警告。

**同日:真实 LLM 检出率稳定性验收通过,1.8 收官合入 main**
- DeepSeek + Chromium,`check --no-cache --yes` 独立 6 轮:单采样 5/5、5/5、4/5;`--samples 2` 4/5、5/5(1 误报)、5/5(地标快照后,0 误报)。历史 3~4/5(最差 3/5)→ 全轮 ≥4/5;DEMO-005(localStorage)历史从未检出,本轮 6/6 稳定检出。
- 并集救援真实可见:B1 首轮筛选场景通过(单独仅 3/5),第二轮抓回 → 4/5;B2 并集救回 DEMO-003。
- 断言强度护栏真实触发:B2 中 LLM 修复两度试图弱化断言被拒,原始失败证据保留。
- 验收暴露误报根因并修复:断言目标 `#api-banner` 为非交互元素、不在 DOM 摘要 → LLM 猜 `.error-banner`;runner `collectSnapshot` 补充**地标元素**(带 id 的非交互节点,上限 80),B3 复测误报消除、两轮生成独立全中。
- 验收门(稳定 ≥4/5)达成。分支合入 main。

### 2026-07-12（里程碑 v0.9.0 与 Phase 1.8 启动决策)

- 用户决策:Phase 4(广度)不做;尝试 Phase 1.8 场景执行级修复回路(深水区,风险高)。
- 立里程碑 `v0.9.0`(版本 0.8.0→0.9.0):Phase 0-3 产品化目标全部完成的回档基线。**回档方式:`git checkout v0.9.0`(或 main 上 `git reset --hard v0.9.0`)。**
- 工作方式变更:1.8 在独立分支 `phase-1.8-repair-loop` 推进(CI 触发已加 `phase-*` 分支),验收达标才合入 main;尝试失败弃分支即可,main 永远停在完整可用形态。

### 2026-07-12（Phase 3.5 稳定性,Phase 3 全部完成)

- 抗 flaky:runner.mjs 对时序敏感动作(click/fill/press/wait_for_selector/expect_*)失败后等 500ms **重试一次**;goto/screenshot/eval 不重试。`action_result` 新增 `attempts`;二次仍失败抓 `failure-<index>.png` 作证据。Rust 端 `ActionOutcome.attempts`(缺省 1)。
- Trace 归档:runner `context.tracing.start/stop` 把 `trace.zip` 写入 run 证据目录(与截图同目录),回传 `trace` 事件;Rust `PlaywrightOutcome.trace_path`。
- sidecar 崩溃恢复:`build_outcome` 收尾时若既无 finished 也无 fatal,合成 fatal("possible sidecar crash"),让崩溃/被杀变成显式高severity失败而非静默"未完成"。
- 协议保持 v1:新增字段/事件向后兼容(旧 Rust 忽略 attempts、trace 归为 Unknown;新 Rust 对旧 runner attempts 缺省 1、无 trace)。
- 真机 sidecar 验证(demo 服务 + 缺失 selector 触发重试):attempts 正确(goto/screenshot=1、失败断言=2)、`failure-2.png` + `trace.zip`(94KB)落盘、trace 事件回传。Rust 侧 attempts/trace/崩溃三项单测。
- 89 单测(+2 playwright)+ 8 集成全过,严格 Clippy 无警告。**Phase 3 修复闭环 3.1-3.5 全部完成。**

### 2026-07-12（Phase 3.4 安全强化)

- ①启动命令确认记忆:`storage` 加 `approved_commands` 表(project_root + command)+ `approve_command`/`is_command_approved`;check 处理器把确认回调换成白名单感知闭包——确认过的命令按"规范化项目路径 + 确认消息"记住,下次同项目同命令直接执行并提示"previously approved"。真机验证首次提示→记住→二次免提示。
- ②AI 出站脱敏:新增 `src/redact.rs`,在 `run_chat_json` 单一出站入口对所有消息 content 脱敏(敏感键 api_key/token/password 等的值 + sk-/ghp_/xoxb- 等已知令牌前缀;保守起见宁多脱一行)。脱敏在缓存键计算前,缓存一致。与运行期日志整行脱敏(runtime)互补。
- ③威胁模型文档 `docs/THREAT_MODEL.md`:信任边界(用户指令可信、被测内容/LLM 输出不可信)、攻击面(任意代码执行/数据外泄/不可信补丁/提示注入)、逐项缓解与残留风险。
- 复用点:`confirm_on_terminal` 泛化(去掉硬编码 "Apply?" 动词)供 fix 与 launch 两处确认共用。
- 88 单测(+5:4 redact + 1 命令记忆)+ 8 集成全过,严格 Clippy 无警告。

### 2026-07-12（Phase 3.3 自动回归闭环,Phase 3 收官)

- 新增 `src/regression.rs` 与 `fix --apply --verify`:应用补丁后自动验证修复是否成立。
- 两部分解耦:①纯裁决 `evaluate(baseline, post_fix, target)`——按 Issue 指纹集合对比,目标指纹消失且无新增指纹 → verified,并给出 resolved/remaining/new_issues;②编排 `verify_on_branch`——`git worktree add --detach` 把修复分支物化到临时目录(绝不碰用户工作区)→ 按 baseline 配置重跑 `generate_review_report_with` → 计算修复后指纹 → 裁决 → 无论成败都清理 worktree。
- CLI 流程:`--verify` 隐含需 `--apply`;应用成功后重跑验证,verified 则保留分支,否则 `apply::delete_branch` 回滚并列出证据。验证重跑参数从归档 report.json 的 config 重建(base_url/requirements_source/execute/skip 标志/超时)。
- 测试载体:用**需求质量缺陷**(模糊需求→质量问题)作确定性验证载体——它由文件解析产生、无需运行时,却真实响应 worktree 内的文件改动,故能在 CI 无 Node 环境下忠实验证整个回归闭环。3 个 evaluate 单测 + 2 个 worktree 集成测试(正/负)。
- 关键修复:归档的 `project_root`(正斜杠)与 `requirements_source`(反斜杠)分隔符不一致导致 `strip_prefix` 失败、验证误读原始需求文档 → 新增 `relative_within` 按正斜杠归一比较。
- 真机(fake OpenAI 端点 + 独立临时 git 仓库)验证 CLI `fix --apply --verify` 双路径:修复到位 → "Regression check PASSED" 留分支;修复不到位 → "FAILED, target still present" 自动回滚分支(仅剩 master),worktree 均清理。
- 83 单测(+6 regression)+ 8 集成全过,严格 Clippy 无警告。**Phase 3 修复闭环全部完成**:诊断 → 生成补丁(git apply --check 校验)→ 应用到隔离分支(安全守卫)→ 回归验证(worktree 重跑对比)。

### 2026-07-12（Phase 3.2 安全应用)

- 新增 `src/apply.rs` 与 `fix --apply [--allow-dirty]`:把已校验补丁应用到隔离分支 `specprobe/fix-<issue>`。
- 安全设计:①前置校验被测项目是 git 仓库(`rev-parse --is-inside-work-tree`)且工作区干净(`status --porcelain`,否则 `--allow-dirty` 豁免);②应用前展示 diff + 终端确认(`confirm_on_terminal`,非 TTY/EOF/非 y 拒绝,提示走 stderr 不污染 `--json` stdout);③记录当前分支(分离头指针记 sha)→ `checkout -b` 隔离分支 → `git apply --recount --index` → 在该分支提交 → **切回原分支**,绝不改动用户当前分支工作区;④任何一步失败自动回滚(强制切回原分支 + 删除新分支);⑤分支已存在则拒绝,避免覆盖上次修复。
- 真机验证(fake OpenAI 端点 + 独立临时 git 仓库,避免污染本仓库):apply 成功(master 不变仍 500、fix 分支 200、已切回 master、工作区干净)、abort(答 n 不创建分支)、branch-exists 拒绝三条路径正确;单测覆盖 apply/dirty/non-git。
- 关键:被测项目 project_root 必须是 Rust/Windows 可解析路径(真机用 `cygpath -m` 转 `C:/...`);git apply/branch 操作都以 project_root 为 cwd,故 project_root 应为被测项目自己的仓库根。
- 78 单测(+3 apply)+ 8 集成全过,严格 Clippy 无警告。

### 2026-07-11（Phase 3.1 真补丁生成)

- 新增 `src/patch.rs` 与 `fix <ISSUE-ID>` 命令:从归档 report.json(以 `serde_json::Value` 读取,避免为 `ReviewReport` 引入 Deserialize)取 issue 描述 + 关联诊断的源码定位文件 → 读文件全文 → LLM 生成 unified diff。
- 两项硬约束落进 `run_chat_json` 的 parse 回路,失败即带反馈自动重问:①只允许修改提供的文件(diff `+++` 头解析后比对);②`git apply --check --recount`(经 stdin,无需 git 仓库)必须通过。复用既有传输层的重试/缓存/反馈机制,与 refine/scenario 同构(拆 `generate_patch` 解析 provider + `generate_patch_with_protocol` 核心,测试用 `test_openai_protocol` + 假端点驱动)。
- 补丁生成需真实 provider(Mock 直接报 `NoProvider`);无诊断源码定位的 issue 报清晰错误引导用户以 `--provider` 重跑。仅生成并展示,不应用(应用属 3.2)。
- 关键修复:`.trim()` 会吃掉 diff 末尾换行,导致 `git apply` 报 "corrupt patch at line N"。改为保留恰好一个末尾换行(`trim_end_matches(['\n','\r'])` 后补 `\n`)。
- 验证:4 个 patch 单测(含假 OpenAI 端点全链路 + git apply 真校验);真机用本地 fake OpenAI 端点跑通 CLI `fix` 读报告→生成→`git apply --check` 真 server.js→打印;三条 CLI 守卫路径(Mock 拒绝 / issue 不存在 / 无诊断)均正确。75 单测 + 8 集成全过,严格 Clippy 无警告。

### 2026-07-10（Phase 2.7 跨平台,Phase 2 收官）

- CI 重构:拆为 `lint`(fmt + clippy,单平台 ubuntu,平台无关只跑一次)与 `test`(ubuntu / windows / **macos** 三平台矩阵冒烟)。新增 macOS 验证进程树 kill、路径处理、bundled SQLite 编译的平台差异。
- 核心跨平台正确性早已由 `cfg(windows/unix)` 保证:runtime 进程组(unix)/ 一次性命令、playwright kill_tree(taskkill / kill -TERM 进程组)、doctor MSVC 探测(windows-only,unix 返回 not-found 不 panic)。macOS 走 unix 分支,无需新代码。
- README:本地运行补 Linux/macOS 直接 `cargo` 路径,澄清 `cargo-msvc.ps1` 仅为本机 MSVC 未注册 vswhere 的 workaround;更新"当前状态"反映 Phase 2 完成。
- **Phase 2(易用性)全部完成**:一键 check / specprobe.toml / 进度条 / 运行归档+SQLite / 审批持久化 / 跨平台。

### 2026-07-10（Phase 2.5 审批工作流）

- `issues` 子命令:`list`(默认最近 run,隐藏 ignored,`--all` 显示)/`show <ID>`/`accept|reject|ignore <ID> [--run] [--note]`,均支持 `--json`。
- **Issue 指纹**:`issue_fingerprint`(类别+需求+标题 SHA-256 前 16 hex)跨 run 稳定识别同一问题。storage 加 `approvals` 表(fingerprint→state/note),issues 表加 fingerprint 列。
- 审批持久化:`set_approval` upsert approvals 并同步更新所有 run 中该指纹 issue 的 approval;`record_run` 时新 run 的 issue 按指纹**继承**已有审批(不回 pending)。
- 审批与核心解耦:review/check 报告的 approval 仍为核心默认;继承与持久发生在 storage/CLI 层。`resolve_run`(--run 或最近)、`set_issue_approval`(ISSUE-ID→指纹→审批)在 lib 层。
- 真机验证:accept ISSUE-001 + ignore ISSUE-002 → list 隐藏 ignored → 重跑新 run 继承(ISSUE-001 仍 accepted、ISSUE-002 仍隐藏,ISSUE-003 pending)。
- 新增 1 个 storage 审批单测(跨 run 继承)。71 单测 + 8 集成全过,严格 Clippy 无警告。

### 2026-07-10（Phase 2.4 运行归档与 SQLite 存储）

- 新增 `src/storage.rs`(rusqlite bundled)与 `runs` 子命令。每次 check/review 归档 report.json 到 `.specprobe/runs/<run-id>/`,索引写入 `.specprobe/specprobe.db`:`runs` 表(id/时间/项目/引擎/executed/计数/report 路径)与 `issues` 表(id/严重度/类别/标题/需求/approval)。
- `runs list`(时间倒序,`--limit`)/`runs show <id>`(详情 + Issue 列表),均支持 `--json`。`issues.approval` 列为 2.5 审批持久化预留(当前恒 pending)。
- 存储与核心逻辑解耦:review/check 只返回报告,CLI 层 `archive_run` 归档,失败仅 stderr 告警不阻断。`--no-store` 关闭归档;check/review CLI 集成测试加 `--no-store` 避免写 repo db 与并行冲突。
- Windows 坑:storage 单测 `remove_dir_all` 前需显式 `drop(store)` 关闭 SQLite 连接(否则文件占用删不掉)。真机验证 check 归档 → `runs list`/`show` 端到端。
- 依赖新增 `rusqlite`(bundled,自带 SQLite,跨平台不依赖系统库)。70 单测 + 8 集成,严格 Clippy 无警告。

### 2026-07-10（Phase 2.3 终端进度 spinner）

- 新增 `src/ui.rs`(indicatif spinner 封装)与阶段进度:scan / 需求精解析 / 起服务就绪 / 浏览器执行 / 诊断。消除 check/review 执行时的长静默。
- 解耦:核心逻辑经 `&(dyn Fn(&str)+Sync)` 阶段回调报告进度,不依赖 UI 层。`review::generate_review_report_with`、`check::run_check_with_progress` 为带回调的变体,旧 API(`generate_review_report`/`run_check_with`)向后兼容(内部 no-op),现有测试无需改。
- 进度走 stderr;`Progress::spinner(!json)` 在 `--json` 时禁用;indicatif 在非 TTY(管道/CI/重定向)自动隐藏动画。真机验证 `check --json` 的 stdout 为干净合法 JSON,进度/确认在 stderr。
- 依赖新增 `indicatif`。68 单测 + 8 集成全过,严格 Clippy 无警告。

### 2026-07-10（Phase 2.2 配置文件 specprobe.toml）

- 新增 `src/config.rs` 与 `init` 命令:项目根 `specprobe.toml` 配置(base_url、provider、requirements、超时、no_cache)。
- 优先级逐字段合并:CLI 显式参数 > 环境变量(`SPECPROBE_BASE_URL`/`SPECPROBE_PROVIDER`)> 项目配置 > 内置默认。为区分"用户显式传参",`check` 的公共参数(base_url/provider/超时)CLI 定义改为 `Option`。环境层做成显式结构注入,避免测试依赖进程级环境变量。
- 健壮性:`deny_unknown_fields`(拼写错误显式报错)、非法 provider 报错、`toml::de::Error` 装箱(clippy result_large_err)。`init` 拒绝覆盖已存在文件(`--force` 强制),模板本身是合法可解析配置(单测保证)。
- `check` 已接入配置(`load_project_config` + `resolve_settings`);读到配置时 stderr 提示来源。review 作为分步调试命令保持显式参数。
- 依赖新增 `toml`。4 个 config 单测(优先级、相对路径需求、非法字段/provider、模板生成拒覆盖)。真机验证 `init` 生成 + `check` 读取配置。当前 68 单测 + 8 集成,严格 Clippy 无警告。

### 2026-07-10（Phase 2.1 一键 check 命令）

- 新增 `src/check.rs` 与 `check` 子命令(主入口):scan → 需求精解析 → 编排执行 → 浏览器 → 诊断,默认写 `.specprobe/report.html`(`--no-html` 关闭,`--html <PATH>` 改路径),`--requirements` 可覆盖需求源(默认同项目目录)。
- 安全边界:探测到启动命令(dry-run)时先交互确认(stderr 提问 + stdin 读答复,非 TTY/EOF 一律拒绝→安全降级为计划级);`--yes` 跳过;未检测到命令时无需确认(浏览器仍可探测已运行服务)。确认回调可注入,便于测试。
- 输出:`print_check_report`(项目/技术栈头 + 复用 review 打印);`--json` 输出 {profile, executed, review}。
- 真机验证:`check .\demo\buggy-task-board --yes --provider openai-compatible` 一条命令从零到 HTML 报告;上一轮网络失败去重在真机生效(33 行 → "x17; x16" 两条),笼统探测失败问题不再出现。
- README 增加"快速开始"(check 为主入口),清理"当前边界"节残留的过时表述(旧 launch 边界、已过时的 1/5 检出)。
- 新增 3 个 check 单测 + 1 个 CLI 集成测试(stdin EOF → 拒绝 → 计划级)。当前 64 单测 + 8 集成,严格 Clippy 无警告。

### 2026-07-08（Phase 2.6 HTML 报告,插队优先）

- 用户选择走 Phase 2 易用性;按约定 HTML 报告优先(演示性价比最高)。
- 新增 `src/report.rs`:minijinja 模板把 `ReviewReport` 渲染为单文件自包含 HTML,base64 内联截图,light/dark 自适应。`review` 命令加 `--html <PATH>`。分区:摘要卡片、问题、AI 诊断(源码定位)、交互场景(带截图)、需求、证据。依赖新增 minijinja + base64。
- 配套报告质量优化:网络失败按 (url,status) 有序去重计数(FocusBoard 33 条重复 /api/tasks 500 → 2 条);有具体场景结果时跳过冗余的笼统"浏览器探测失败"问题(消除超长拼接)。
- 真机验证:`review --html` 生成 3.9MB 报告,12 张场景截图 base64 内联可显示(注:minijinja autoescape 把 base64 里的 `/` 转成 `&#x2f;`,浏览器正常解码,不影响显示);诊断精确定位 server.js:47。本轮 expect_hidden 兑现价值检出筛选缺陷,单次 4/5。
- 61 单测 + 7 集成全过,严格 Clippy 无警告。

### 2026-07-08（Phase 1.8 质量调优,机制完成）

- 三项机制改进落地:①新增 `expect_hidden` 否定断言原语(playwright.rs + sidecar `waitForSelector state:hidden` + scenario 支持);②scenario prompt 收敛断言 selector(优先文本断言,消除 `.error-banner` 类误报);③diagnosis prompt 防过度合并(每根因一诊断)。
- 真机 `--no-cache` 复测:REQ-008 误报消除✅、诊断恢复 3 个独立✅、expect_hidden 可用✅;但暴露 LLM 场景生成内在波动(漏 click 步骤、否定断言编造目标、断言强度回退),检出率单次 3~4/5 波动。
- **判断**:纯 prompt 调优触及天花板;稳定提升需场景执行级修复回路(生成→dry 执行→失败回喂修正)+ 多次采样,属独立硬骨头(见 ROADMAP 1.8 遗留)。1.8 机制为正确基础设施,保留。
- 60 单测 + 7 集成全过,严格 Clippy 无警告。

### 2026-07-08（Phase 1 端到端真机验收）

- 用 DeepSeek(deepseek-chat,OpenAI 兼容)+ Playwright/Chromium 对 FocusBoard 跑 `review --execute --provider openai-compatible`,完成 Phase 1 真机验收。详见 [docs/ACCEPTANCE.md](docs/ACCEPTANCE.md)。
- 全链路端到端跑通:精解析(engine=llm-refined,12 需求带行号)→ ManagedApp 起服务并就绪(Server responded at 4173)→ 真 Chromium 执行 12 场景 → 采集 35 条 /api/tasks 500 → LLM 诊断精确定位 API 500 到 **server.js:47**(经核对准确)。
- 注入缺陷**检出 4/5**(DEMO-001 API 500、002 空白、003 统计、005 持久化;004 筛选漏检),从基线 1/5 大幅提升,达到验收门数量目标。
- 验收中修复/新增:强化 scenario 断言 prompt(要求断言可观察结果的具体值而非元素存在,使统计缺陷可检出);**修复场景执行超时缺陷**(整体超时原为固定 20s,多个失败断言累加即误杀整场执行;改为按动作数分摊 + 上限 180s,单动作超时独立为 6s);新增 `SPECPROBE_NO_PLAYWRIGHT` 逃生开关(强制 HTTP 探测,便于本地测试对齐 CI)。
- 暴露 3 项 LLM 质量调优点(见 ROADMAP 1.8):断言 selector 猜测误报(REQ-008 猜 .error-banner 实为 #api-banner)、诊断多失败时过度合并、筛选类需 negative 断言原语。
- 59 单测 + 7 集成(设 SPECPROBE_NO_PLAYWRIGHT 对齐 CI 的 HTTP 路径)全过,严格 Clippy 无警告。

### 2026-07-08（深夜,Phase 1.7 完成,Phase 1 收尾）

- 新增 `src/diagnosis.rs`:在确定性规则 Issue 之上叠加 LLM 深度诊断(规则 Issue 始终保留,离线可用)。
- 流程:从失败 Issue(运行期 RuntimeFailure/BrowserFailure 高严重度)提取关键词(CSS selector、URL 路径段、kebab/snake 标识符)→ 遍历项目源文件 grep 命中行截取带行号片段(跳过 node_modules/target 等,限量 12)→ 失败发现 + 源码片段交 LLM → 输出带 source_locations(文件+行+片段)、confidence、suggested_fix 的诊断。
- 校验:引用的源文件必须来自检索到的片段集合(严格拒绝臆造、宽容丢弃),related_issue_ids 过滤未知。复用 `run_chat_json`。
- `ReviewReport` 新增 `diagnoses` 字段;仅 `--provider` 非 Mock 且有可诊断失败时触发,诊断失败不阻断 review(记警告证据)。output 展示。
- 新增 5 个 diagnosis 单测。当前 59 单测 + 7 集成,严格 Clippy 无警告。
- **Phase 1 全部子阶段完成**。下一步端到端真机验收。

### 2026-07-08（晚间,Phase 1.6 完成）

- runtime.rs 新增 `ManagedApp` 托管生命周期:`start_app` 启动并持有进程(不等退出)、`wait_until_ready` 轮询 base_url HTTP 直到响应/进程退出/超时、`shutdown` 杀进程树 + 采集脱敏日志返回 LaunchReport。取代"运行到退出或超时杀掉"对 Web 服务器的错误语义。
- 进程树 kill:Windows `taskkill /PID <pid> /T /F`;Unix `build_process` 设 `process_group(0)`,关停时对负 PID 发 SIGTERM 杀整组(解决 npm→node 孤儿泄漏)。未引入 windows crate。
- `LaunchReport` 新增 `readiness` 字段;output 显示就绪探测结果。
- `review.rs`:`--execute` 且未 skip launch/browser 时走 `run_orchestrated`(起服务→等就绪→跑浏览器→关停);server 起不来则记录 launch 错误并仍尝试浏览器。就绪即视为 launch 成功,消除健康服务器被判失败。
- 新增 2 个 ManagedApp 单测(真实起长驻进程 + 假 HTTP 端点探测就绪并关停、不可达端口超时未就绪)。当前 54 单测 + 7 集成,严格 Clippy 无警告。
- 真机端到端(review --execute 自动起 FocusBoard + 浏览器场景)与其余能力一并放最后真机验收。

### 2026-07-08（Phase 1.5 完成）

- 新增 `src/scenario.rs`:基于 1.4 采集的 DOM 元素摘要 + 需求,用 LLM 生成带真实 selector 的浏览器交互场景。复用 1.2 的 `run_chat_json` 传输层。
- selector 静态校验即"dry-run 校验":操作类动作(click/fill/press)的 selector 必须来自页面可交互元素列表,不通过则经 `run_chat_json` 反馈重问回路自动修正(≤2 轮);断言/等待类允许任意 CSS selector。
- `browser.rs` 编排:探针采集 DOM 摘要 → 生成场景 → 每场景 goto 隔离后一次 sidecar run 执行所有场景 → 按 index 区间切回各场景结果。report 新增 `scenarios` 字段与 `ScenarioResult`/`ScenarioStepReport`。
- `review.rs` 消费场景:失败场景关联需求生成高严重度 Issue;需求来源改走 1.3 精解析(`analyze_requirements_with_refinement`)。
- `browser`/`review`/`propose` 命令均加 `--provider`/`--no-cache`;默认 Mock 走 1.4 通用采集不调 LLM,`--provider openai-compatible|ollama` 启用场景生成。
- 4 个 scenario 单测(有效解析、未知 selector 拒绝+宽容过滤、未知需求 id 拒绝、Mock 空计划)。当前 52 单测 + 7 集成,严格 Clippy 无警告。
- 真实浏览器端到端(含 selector 修正回路的真机表现)与 AI/Playwright 一并放最后真机验收。

### 2026-07-07（Phase 1.4 完成）

- 新增浏览器执行器 Node sidecar `executors/playwright-runner`(runner.mjs + package.json):stdin 收单个 JSON 计划,stdout 回 NDJSON 事件(started/action_result/console/page_error/network_failed/snapshot/finished/fatal),协议版本 1,含 UTF-8 BOM 剥离。
- 新增 `src/playwright.rs`:协议类型(9 个动作原语、事件反序列化含未知类型向前兼容)、`detect_runner`(要求 runner.mjs + node_modules/playwright 存在)、`run_actions`(tokio 子进程 + 事件流聚合 + 超时)、`runner_dir`/`setup_runner`。
- `browser.rs` 集成:优先 Playwright 深度执行(goto → wait_for_selector body → screenshot + 自动采集 console/网络/DOM 元素摘要),证据归档 `.specprobe/runs/browser-<id>/`;探测不到 runner 或执行失败自动降级 HTTP 探测。报告新增 `backend`(playwright/http_probe/none)与 `playwright` 证据字段。
- `review.rs` 消费 Playwright 证据:网络失败、页面脚本错误各聚合为高严重度 Issue,console error 记为证据。FocusBoard 的 `/api/tasks` 500 现可通过首页加载的 network failure 被自动发现。
- `doctor` 增加 playwright 检查与安装提示;新增 `specprobe setup-browser` 命令(npm install + npx playwright install chromium)。
- 关键降级设计确保 CI(无 Node/Playwright)与无 sidecar 环境正常:detect 返回 None 即走 HTTP 探测。协议层纯 Rust 单测覆盖(动作序列化、事件流聚合、探测),真实浏览器执行放最后真机验收。
- 用 node v24 零依赖冒烟验证 sidecar:BOM 剥离、协议版本检查、playwright 未装的 fatal 路径均正常。
- 新增 4 个 playwright 单测,当前 48 单测 + 7 集成,严格 Clippy 无警告。

### 2026-07-06（深夜二，Phase 1.3 完成）

- 新增 `refine` 模块：需求理解两级流水线。规则引擎降级为候选粗筛与兜底；LLM 按文档逐个精解析（带行号全文 + 规则候选行提示 + scan 技术栈提示）。
- LLM 线格式经 serde 严格校验：行号越界、空描述、缺验收标准会带反馈重问（≤2 轮），最后一轮宽容过滤无效条目；REQ/AC 编号与测试计划始终由确定性代码生成，AI 不直接产出计划。
- `RequirementReport` 新增 `engine` 字段（rule_based / llm_refined）；`requirements` 与 `ai` 命令共用流水线（`--provider`，默认 mock=纯规则，原行为与 JSON 快照仅新增字段）。
- 降级策略：传输/校验失败回退规则结果并附 Warning 诊断；MissingConfig（用户显式选择的 Provider 未配置）直接报错。
- 1.2 传输层泛型化为 `run_chat_json`，建议分析与需求精解析共用重试/校验/缓存循环；测试辅助（假聊天端点等）提取到 `testutil` 模块。
- 新增 5 个 refine 单测（mock 保持规则、LLM 替换需求并重建计划、传输失败回退、行号校验、验收标准必填）。当前 44 单测 + 7 集成，严格 Clippy 无警告。

### 2026-07-06（深夜，Phase 1.2 完成）

- 实现真实 AI 传输：OpenAI 兼容 chat completions 与 Ollama `/api/chat`，共用 `ChatProtocol` 抽象（重试/校验/缓存一套循环，端点差异只在请求体构造与字段提取）。
- 结构化输出：json_object 模式 + prompt 内嵌 schema；模型输出经 serde 严格校验（含 requirement_id 引用校验），失败带反馈重问 ≤2 轮，最后一轮宽容过滤未知 ID；自动剥离 markdown 代码围栏。
- 可靠性：网络错误/429/5xx 指数退避 ≤3 次；4xx 快速失败；对不支持 `response_format` 的端点自动降级。
- 响应缓存：SHA-256 请求指纹 → `.specprobe/cache/<hash>.json`，只缓存已验证输出，命中时零网络请求；`ai` 命令新增 `--no-cache`。
- 报告新增 `transport` 字段（attempts / cache_hit / token 用量），终端与 JSON 同步输出。
- 依赖新增 `sha2`；`scripts/cargo-msvc.ps1` 内置 `CARGO_HTTP_CHECK_REVOKE=false`（项目级方案 B，用户知情选择，替代被拒的全局 config 写入）。
- 新增 8 个基于本地假 HTTP 端点的离线单测（结构化解析、Authorization 头、校验重试、5xx 退避、401 快速失败、缓存命中、Ollama 协议、围栏剥离）。当前 39 单测 + 7 集成，严格 Clippy 无警告。
- 真机验收待用户提供 DeepSeek key 后进行（`OPENAI_BASE_URL=https://api.deepseek.com`）。

### 2026-07-06（晚间，Phase 1.1 完成）

- 引入 `tokio`（macros/process/rt-multi-thread/time）与 `reqwest`（rustls-tls + json，关闭默认特性），全链路异步化：`main`/`run`/`launch`/`browser`/`ai`/`review`/`propose` 均为 async。
- `runtime.rs`：`std::process` → `tokio::process`；超时从 50ms 轮询改为 `tokio::time::timeout` + `start_kill`（含与自然退出的竞争处理），并加 `kill_on_drop` 兜底。
- `browser.rs`：删除手写 TCP/HTTP 解析与 chunked 解码（约 150 行），改用 reqwest；探测能力提升——支持 `https://`、重定向跟随；错误类型细分 `Client`/`Probe`。
- `ai.rs`：`Box<dyn AiProvider>` 改为枚举分发（async fn 与 dyn trait 不兼容），`analyze` 已是 async 签名，1.2 填充真实传输即可。
- `doctor.rs` 的短命 `--version` 探测有意保留同步实现，待 1.2 一并转换。
- 测试全部迁移 `#[tokio::test]`；chunked 解码测试随实现删除，URL scheme 校验测试重写。当前 31 单测 + 7 集成，严格 Clippy 无警告。
- 用户级 `~/.cargo/config.toml` 持久化方案被权限系统拒绝（TLS 弱化类配置），已向用户说明三个替代选项，本次沿用会话级 `CARGO_HTTP_CHECK_REVOKE=false`。

### 2026-07-06（下午，Phase 0 完成）

- 配置远程仓库 `https://github.com/pxgt/Rust_Final_job` 并推送。解决本机网络问题：git 仓库级配置 `http.proxy=127.0.0.1:7897` + `http.sslBackend=openssl`；cargo 依赖下载需要 `CARGO_HTTP_CHECK_REVOKE=false`（会话级）。
- 修复 `strip_inline_markdown`：`[文字](url)` 与 `![alt](url)` 现在保留文字、丢弃 URL（新增 `strip_markdown_links`）。
- 修复 launch 对长驻服务器的语义：新增 `LaunchExecution.long_running`（超时被杀前仍在运行且有输出），此类进程视为健康而非失败；诊断信息、人类可读输出与 review 证据详情同步。新增两个超时行为单测，测试中确认了进程树 kill 泄漏孤儿进程的已知缺陷（ROADMAP 1.6 修复）。
- 新增 GitHub Actions CI（ubuntu + windows 矩阵：fmt --check、clippy -D warnings、test）。
- 新增测试基建：`assert_cmd` + `insta` dev 依赖；7 个 CLI 集成测试覆盖全部 8 个子命令的 JSON 出口；`tests/fixtures/demo-prd.md` 固定夹具；requirements JSON 快照锁定机器可读接口。
- 当前测试规模：32 个单元测试 + 7 个集成测试，严格 Clippy 无警告。
- 首次 CI：windows 全绿；ubuntu 两轮失败定位出真正根因——`unique_suffix()` 只用毫秒+进程 ID，同进程并行测试在同一毫秒共享日志目录，互相截断/删除对方的 stdout 采集文件（Linux runner 线程启动快必现，Windows 侥幸错开）。这是生产代码缺陷（未来并发 launch 同样触发），已用进程内原子计数器修复。第一轮误判为 dash 内建 echo 块缓冲，`/bin/echo` 的改动作为 SIGKILL 缓冲防护保留。
- ROADMAP Phase 0 各项标记完成，下一步进入 Phase 1.1 异步化改造。

### 2026-07-06

- 项目接手，目标从课程演示调整为真实可用、交互友好、完整成熟的工具。
- 完成全量源码评估：确认架构与数据模型可继续沿用；确认三个核心占位（AI 无真实传输、浏览器执行器仅 HTTP 探测、需求解析为关键词规则）与 launch 对长驻服务器的语义错误、进程树 kill 缺陷。
- 建立 [docs/ROADMAP.md](docs/ROADMAP.md)：接手评估、Phase 0-4 详细规划、FocusBoard 基准验收门（当前 1/5 检出为基线）、技术选型与风险对策。
- 完成 Phase 0.1 首次 git 提交：基线 commit 固化课程阶段原始状态，分支定为 `main`，此后按 Conventional Commits 提交。
- 新增 `.gitattributes` 归一化换行符。
- 重写 README.md：能力描述与实现对齐，新增"当前边界"如实声明；PROJECT.md 概述、范围、里程碑、下一步任务各节改为历史记录 + 指向 ROADMAP。

### 2026-07-03

- 开启并完成 M8 课程演示与实验评估基础阶段。
- 创建零依赖 Node.js 演示项目 FocusBoard，并注入五类可复现缺陷。
- 新增 `scripts/run-demo.ps1`，一键运行 M1-M7 核心能力并归档 7 份 JSON 报告。
- 实验识别 Node.js 技术栈，提取 12 条需求和 12 个测试用例，生成 15 条 Mock AI 建议。
- 首页探测返回 HTTP 200，故障 API 返回 HTTP 500。
- 综合生成 4 个 Issue、4 个修复提案、3 个补丁预览和 9 个回归检查。
- 使用浏览器复现 API、空输入、完成统计、筛选和持久化缺陷，并验证 375px 无横向溢出。
- 新增课堂演示指南、实验评估记录和课程报告初稿。
- 项目版本更新到 0.8.0，M0-M8 基础交付全部完成。

### 2026-06-17

- 开启并完成 M7 修复提案与回归检查基础阶段。
- 新增 `propose` 子命令，基于 `review` 问题清单生成候选修复方案。
- 定义 `RemediationReport`、`PatchProposal`、`PatchStrategy`、`PatchSafety`、`RegressionPlan` 和 `RegressionCheck`。
- 为需求澄清和启动配置类问题生成 Git patch 风格的补丁预览。
- 为每个修复提案生成回归检查命令，并增加全局综合审查回归命令。
- 保持默认只读和不自动应用补丁的安全边界。
- 单元测试增加到 29 个，严格模式 Clippy 无警告。
- 开启并完成 M6 缺陷报告与证据链基础阶段。
- 新增 `review` 子命令，默认进行计划级审查，使用 `--execute` 后执行启动和页面探测。
- 定义 `ReviewReport`、`EvidenceItem`、`Issue`、`ReviewSummary` 和 `ApprovalState`。
- 将需求质量、启动命令、进程输出、浏览器动作计划、页面探测和执行诊断统一为证据项。
- 根据证据生成带严重程度、类别、预期结果、实际结果、证据编号和建议的问题清单。
- 问题默认进入 `pending` 审批状态，为后续用户接受、拒绝或忽略修改建议预留接口。
- 单元测试增加到 27 个，严格模式 Clippy 无警告。

### 2026-06-16

- 明确平台总体定位不局限于 Web 项目。
- 将 Web 调整为课程版本的首个适配目标，而不是系统能力边界。
- 补充项目适配器、测试执行器、证据采集器和报告渲染器等扩展机制。
- 开启并完成 M2 需求解析阶段。
- 新增 `requirements` 子命令，支持文件或目录输入。
- 实现需求模型、验收标准模型、测试计划模型、质量提示和执行器建议。
- 增加演示用需求文档 `docs/specprobe-requirements.md`。
- 开启并完成 M3 AI Provider 阶段。
- 新增 `ai` 子命令，默认使用离线 Mock Provider。
- 定义 `AiProvider`、`AiModelOutput`、`AiSuggestion` 和 `AiAnalysisReport`。
- Mock Provider 可以基于需求质量提示生成澄清、补充验收标准、补充负向测试和补充证据等建议。
- 预留 OpenAI 兼容接口和 Ollama 的配置检查路径。
- 开启并完成 M4 项目启动与日志采集阶段。
- 新增 `launch` 子命令，支持 dry-run 和受控执行。
- 实现 Node/Rust/Python 启动命令识别、超时控制、进程终止、stdout/stderr 采集和敏感日志脱敏。
- 修复 Windows PowerShell 写入 UTF-8 BOM `package.json` 导致解析失败的问题。
- 开启并完成 M5 浏览器执行器基础阶段。
- 新增 `browser` 子命令，支持 dry-run 和本地 HTTP 页面探测。
- 将 `TestPlan` 转换为浏览器动作计划，包含打开页面、等待、交互、断言和采集证据。
- 页面探测可采集 HTTP 状态码、页面标题、正文摘要和响应大小。
- 增加 chunked HTTP 响应正文解码，避免输出传输编码噪声。
- 单元测试增加到 25 个，严格模式 Clippy 无警告。

### 2026-06-15

- 确定项目主题为“基于 Rust 与大语言模型的智能代码审查及自动化测试平台”。
- 完成 Rust 与 MSVC 环境检查和安装。
- 创建 SpecProbe Git/Cargo 项目。
- 建立本项目文档，明确范围、架构、安全原则和里程碑。
- 完成 `doctor` 环境诊断，正确识别 Rust、MSVC、Node.js、npm、Docker 和 AI Provider。
- 完成 `scan` 项目扫描，支持技术栈、需求文档、源码语言和测试文件识别。
- 处理 Windows `.cmd` 命令执行、MSVC 环境加载和规范化路径前缀问题。
- 6 个单元测试全部通过，严格模式 Clippy 无警告。
