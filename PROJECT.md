# SpecProbe 项目文档

> 本文件是项目的持续维护台账。每次开发完成后都要更新“当前状态”“里程碑”“开发日志”和“下一步任务”。

## 1. 项目概述

- 项目名称：SpecProbe
- 项目起源：Rust 语言课程大作业（课程阶段 M0-M8 已于 2026-07-03 交付）
- 项目类型：可扩展的 AI 代码审查与自动化测试平台
- 当前版本：0.8.0
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
|-- scripts/
|   |-- cargo-msvc.ps1
|   `-- run-demo.ps1
`-- src/
    |-- ai.rs
    |-- main.rs
    |-- lib.rs
    |-- cli.rs
    |-- doctor.rs
    |-- browser.rs
    |-- output.rs
    |-- remediation.rs
    |-- requirements.rs
    |-- review.rs
    |-- runtime.rs
    `-- scanner.rs
```

- `cli.rs`：CLI 参数与子命令定义。
- `ai.rs`：AI Provider 抽象、Mock Provider、OpenAI/Ollama 配置入口和 AI 增强报告。
- `doctor.rs`：核心开发环境、首版 Web 测试环境和 AI Provider 配置诊断。
- `browser.rs`：浏览器动作计划生成、dry-run 和本地 HTTP 页面探测。
- `remediation.rs`：把问题清单转换为修复提案、补丁预览和回归检查清单。
- `requirements.rs`：Markdown/TXT 需求解析、验收标准生成和测试计划生成。
- `review.rs`：综合需求、启动与浏览器证据，生成结构化问题清单和审批状态。
- `scanner.rs`：项目文件、技术栈、需求文档和测试文件识别。
- `output.rs`：人类可读及 JSON 输出。
- `scripts/cargo-msvc.ps1`：为当前开发机加载 MSVC 环境后执行 Cargo。
- `scripts/run-demo.ps1`：构建工具、启动 FocusBoard 并生成完整演示报告。
- `runtime.rs`：项目启动命令识别、受控运行、超时控制和 stdout/stderr 采集。
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

Phase 1.1 异步化改造、1.2 真 AI Provider 已于 2026-07-06 完成。下一步：Phase 1.3 需求理解升级为两级流水线（规则粗筛 + LLM 精解析，见 ROADMAP §4）。

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
