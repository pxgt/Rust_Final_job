# SpecProbe 产品化路线图

> 本文件自 2026-07-06 项目接手起生效,取代 PROJECT.md 原第 10 节"下一步任务"和课程里程碑规划,成为项目唯一的前瞻规划文档。PROJECT.md 继续作为状态台账和开发日志维护。
>
> 目标定位从"课程演示闭环"调整为:**真实可用、交互友好、完整成熟的工具**。

## 1. 接手评估结论

### 1.1 总体判断

骨架优秀,肌肉缺失。架构方向正确,数据模型正确,工程纪律好;但三个核心部件均为占位实现,当前无法对真实项目发现真实的功能缺陷。

### 1.2 资产(接手时真实存在的价值)

- 数据模型"价值链"设计正确且完整:需求 → 验收标准 → 测试计划 → 证据 → Issue → 修复提案 → 回归检查,全链 ID 关联。后续真实化工作可以直接装进这套模型,不需要推翻。
- 模块边界干净,依赖单向,`AiProvider` trait 抽象到位。
- 工程细节扎实:UTF-8 BOM、Windows `.cmd` 包装、verbatim 路径、chunked 解码、日志脱敏;29 个单测全过,严格 Clippy 无警告。
- 安全边界意识正确:默认 dry-run、`--execute` 显式开启、补丁只预览。
- FocusBoard 故障注入项目 + KNOWN_ISSUES 基准答案,可直接升级为基准测试集。

### 1.3 核心差距(按严重程度)

| # | 差距 | 现状 | 位置 |
| --- | --- | --- | --- |
| 1 | AI 层没有真实模型调用 | OpenAI/Ollama Provider 的 `analyze()` 硬编码返回 `TransportNotImplemented`;无 HTTP 客户端依赖;Mock 是规则模板 | `src/ai.rs` |
| 2 | 浏览器执行器只是一次裸 TCP GET | 仅 `http://`、单 URL、无重定向;无 DOM/交互/截图/console;动作计划是不可执行的统一模板 | `src/browser.rs` |
| 3 | 需求解析是关键词匹配 | 精确率/召回率在真实 PRD 上不可用;验收标准与测试步骤是话术模板 | `src/requirements.rs` |
| 4 | launch 进程模型对 Web 服务器语义错误 | "运行→等退出或杀掉":健康服务器 15 秒后被 kill 并报告为失败;`child.kill()` 不杀进程树(Windows 下 npm 的 node 子进程泄漏);无"启动→就绪→保持→收尾"编排 | `src/runtime.rs` |
| 5 | Issue 是"元问题"而非应用缺陷 | 全部来自需求措辞质量和配置失败;补丁预览是需求文档措辞建议,不是代码修复 | `src/review.rs`, `src/remediation.rs` |
| 6 | 无状态持久化 | `ApprovalState` 恒为 Pending,无审批命令;每次全量重算;`propose --execute` 会把被测项目启动两次 | 全局 |

**北极星指标**:FocusBoard 的 5 个注入缺陷,当前自动检出 1/5(20%,见 docs/EXPERIMENT.md)。本路线图各阶段都以此基准量化验收。

### 1.4 接手时已知小缺陷清单(Phase 0 输入)

- ~~`strip_inline_markdown` 把 `[文字](url)` 处理成 `文字(url)`~~(已修复,2026-07-06)。
- 页面探测不支持 https、不跟随重定向、只探测单页面(`src/browser.rs`)——归入 1.1 reqwest 改造。
- ~~只有 lib 单元测试,没有 CLI 集成测试和 JSON 快照测试~~(已补齐,2026-07-06)。
- 本机 vswhere 不可用,依赖 `scripts/cargo-msvc.ps1`(仅本机问题,CI 标准 runner 不受影响)。
- 脚本链 Windows-only。
- ~~换行符未通过 `.gitattributes` 归一化~~(已修复,2026-07-06)。
- launch 超时 kill 只终止直接子进程,进程树中的孙进程泄漏(runtime 超时测试中可直接观察到)——彻底修复在 1.6 Job Object / 进程组。

## 2. 战略原则

1. **价值公式是乘法**:AI 理解 × 真实执行取证 = 可信缺陷诊断。任何一项为零,整体为零。所以先补"真",再做"好用"。
2. **顺序**:真实化(Phase 1)→ 易用性(Phase 2)→ 修复闭环(Phase 3)→ 广度(Phase 4)。不先做 TUI、多语言适配器等锦上添花项。
3. **FocusBoard 是基准测试集**,不再只是演示道具。每阶段的验收门(Definition of Done)用检出率/误报数量化,不达标不进入下一阶段。
4. **安全边界继承并强化**:默认只读、执行需显式确认、补丁不自动应用、AI 出站内容脱敏。
5. **降级路径永远保留**:规则引擎作为 LLM 不可用时的兜底;Mock Provider 作为离线/测试路径。

## 3. Phase 0 — 地基修复(2~3 天)

| 编号 | 任务 | 具体落实 | 状态 |
| --- | --- | --- | --- |
| 0.1 | 建立提交历史 | 首次 commit 固化接手基线;分支 `main`;之后按 Conventional Commits(`feat:`/`fix:`/`docs:`/`chore:`)提交;bin 项目提交 Cargo.lock;推送 GitHub 远程仓库做异地备份 | 已完成(2026-07-06,远程 `github.com/pxgt/Rust_Final_job`) |
| 0.2 | CI | GitHub Actions:`windows-latest` + `ubuntu-latest` 矩阵,跑 `cargo test`、`cargo clippy --all-targets -- -D warnings`、`cargo fmt --check`。标准 runner 自带 MSVC,不需要 cargo-msvc.ps1 | 已完成(2026-07-06,`.github/workflows/ci.yml`) |
| 0.3 | 测试基建 | 引入 `assert_cmd` 为每个子命令写端到端用例;引入 `insta` 快照测试锁定 `--json` 输出结构,防止重构悄悄破坏机器可读接口 | 已完成(2026-07-06,7 个 CLI 集成测试 + requirements JSON 快照) |
| 0.4 | 小缺陷修复 | 修 `strip_inline_markdown` 链接处理;launch 临时语义修补:超时但已产生输出的进程标记 `long_running` 而非失败(彻底修复在 1.6);加 `.gitattributes` 归一化换行 | 已完成(2026-07-06;https 探测支持归入 1.1 reqwest 改造) |

**验收门**:CI 双平台绿灯;`cargo test` 含集成测试全过。——已达成(2026-07-06):ubuntu + windows 双平台绿灯,32 单测 + 7 集成测试。CI 排查中额外修复一个生产代码并发缺陷(launch 日志目录唯一性)。**Phase 0 完成。**

## 4. Phase 1 — 核心闭环真实化(3~4 周,决定成败)

### 1.1 异步化改造(2~3 天)——已完成(2026-07-06)

- 引入 `tokio`(macros/process/rt-multi-thread/time)+ `reqwest`(json + rustls-tls,default-features 关闭)。
- `main` 改 `#[tokio::main]`;`std::process::Command` → `tokio::process`;页面探测改用 reqwest(顺带解决 https、重定向、超时)。
- 模块边界不动,工作量主要是 async 签名传染。必须最先做:后续服务器进程、浏览器 sidecar、AI 调用需要并发管理。
- 实施记录:launch 超时从 50ms 轮询改为 `tokio::time::timeout` + `start_kill`,并加 `kill_on_drop` 兜底;AI Provider 从 `Box<dyn Trait>` 改枚举分发(async fn 与 dyn 不兼容),`analyze` 已是 async 签名,1.2 可直接填充传输;手写 TCP/HTTP/chunked 解析约 150 行删除,由 reqwest 取代;`doctor` 的短命 `--version` 探测有意保留同步 std::process,待 1.2 接入端点检查时一并转换。

### 1.2 真 AI Provider(约 1 周)——已完成(2026-07-06)

- OpenAI 兼容 chat completions 传输:优先 `response_format: json_schema`;对不支持的端点降级为"prompt 内嵌 schema + 响应校验 + 温度 0 + 重试 ≤2"。
- Ollama `/api/chat`(本地免费,保证开发迭代和离线演示)。
- 统一基础设施:指数退避重试、超时、token 用量统计(写入报告)、错误分类(认证/限流/网络/格式)。
- **响应缓存**:请求体 hash → 响应,落盘 `.specprobe/cache/`。同时解决省钱、离线重放、演示确定性三件事。
- Mock Provider 保留为 `--provider mock` 与单测路径。
- 实施记录:OpenAI 兼容与 Ollama 共用 `ChatProtocol` 抽象(同一套重试/校验/缓存循环,仅请求体构造与字段提取不同);结构化输出采用 json_object 模式 + prompt 内嵌 schema + 校验反馈重问 ≤2 轮(DeepSeek 等端点不支持严格 json_schema,故不依赖);对不支持 `response_format` 的端点自动降级重试;缓存 key 为 SHA-256(provider+endpoint+model+messages),仅缓存已通过校验的输出;`ai` 命令新增 `--no-cache`;传输信息(attempts/cache_hit/token 用量)进入报告与终端输出;8 个基于本地假端点的离线单测覆盖成功解析、校验重试、5xx 退避、401 快速失败、缓存命中与 Ollama 协议。配置文件化(取代环境变量)属 2.2。
- 配置从纯环境变量升级为配置文件 + 环境变量覆盖(配置文件全量设计在 2.2)。

### 1.3 需求理解升级为两级流水线(3~5 天)——已完成(2026-07-06)

- 规则引擎(现有代码)降级为:候选段落粗筛 + LLM 不可用时的兜底。
- LLM 精解析:输入文档全文 + `scan` 技术栈信息,输出 schema 约束的 `Requirement[]`(复用现有数据模型)。
- 验收标准要求落到具体页面/接口,不再输出通用话术。
- 实施记录:新增 `refine` 模块;`requirements`/`ai` 命令共用流水线(`--provider` 选择引擎,默认 mock=纯规则,行为与快照不变);LLM 按文档逐个精解析(带行号的全文 + 规则候选行提示 + scan 技术栈提示),线格式经 serde 严格校验(行号越界/空描述/缺验收标准会带反馈重问,最后一轮宽容过滤);报告新增 `engine` 字段(rule_based/llm_refined),测试计划与 REQ/AC 编号始终由确定性代码生成;传输/校验失败自动回退规则结果并附告警诊断,MissingConfig 则直接报错(用户显式选择的 Provider 未配置);1.2 的传输层泛型化为 `run_chat_json`,建议分析与需求精解析复用同一套重试/校验/缓存循环。

### 1.4 真浏览器执行器(1.5~2 周,最大单项)——已完成(2026-07-07)

- **技术决策:Playwright Node sidecar,不用纯 Rust CDP(chromiumoxide)。**理由:Playwright 自带 auto-wait、选择器引擎、截图、console/network 捕获、trace,稳定性远超自写 CDP;被测目标本就是 Web 项目,Node 已在环境假设内;符合"Rust 编排、执行器执行"的既定架构。
- 实施记录:新增 `executors/playwright-runner/`(runner.mjs + package.json,stdin 收 JSON 计划、stdout 回 NDJSON 事件,协议版本 1,含 BOM 剥离);新增 `src/playwright.rs`(协议类型、`detect_runner`、`run_actions` 事件流聚合、`setup_runner`)。9 个动作原语全部在 sidecar 实现;1.4 的计划生成用通用采集序列(goto → wait_for_selector body → screenshot + 自动附带 console/网络/DOM 元素摘要),把需求映射为具体 selector 步骤留给 1.5(1.4 采集的 DOM 摘要正是其输入)。证据归档 `.specprobe/runs/browser-<id>/`(截图 PNG + console/network/snapshot JSON)。**关键降级设计**:探测不到 sidecar(如 CI 无 Node/Playwright,或未 `npm install`)时自动回退 HTTP 探测,报告 `backend` 字段标注 playwright/http_probe/none。`review` 消费 Playwright 证据:网络失败与页面脚本错误各聚合为高严重度 Issue,console error 记为证据——FocusBoard 的 `/api/tasks` 500 现在通过首页加载的 network failure 被发现,不再需要把 base_url 指到 API。`doctor` 增加 playwright 检查;新增 `specprobe setup-browser`。协议层纯 Rust 单测覆盖(序列化/事件聚合/探测),真实浏览器执行放最后真机验收。
- 实现:新建 `executors/playwright-runner/`(独立 npm 包:`runner.mjs` + playwright 依赖)。
- 协议:Rust 经 stdin 发 JSON 动作计划;runner 逐动作执行,stdout 每行一个 JSON 事件(动作结果 / console 消息 / 网络失败 / 截图路径);协议带版本号。
- Rust 端实现 `TestExecutor` trait(spawn + 双向流)。
- 动作集(首版 9 个):`goto / click / fill / press / wait_for_selector / expect_text / expect_visible / screenshot / eval`。
- 证据落盘 `.specprobe/runs/<run-id>/`:截图 PNG、console.json、network.json、trace.zip。
- `doctor` 增加 Playwright 检查;新增 `specprobe setup-browser` 一键执行 `npx playwright install chromium`。
- 未来增强(未做):Playwright trace.zip 归档、多页面导航、`TestExecutor` trait 抽象(当前直接函数,待第二个执行器出现时再抽象)。

### 1.5 测试计划生成真实化(3~4 天)——已完成(2026-07-08)

- 先让执行器打开首页,抓取可交互元素摘要(accessibility tree:可见按钮、输入框、链接)。
- 元素摘要 + 需求一起喂给 LLM,生成带真实 selector 的具体步骤。
- 生成后 dry-run 校验 selector 存在;不存在的回喂 LLM 修正(≤2 轮)。
- 产出形如"在 `#task-input` 输入空字符串并点击 `#add-task-btn`,断言列表数量不变",而不是"执行核心操作"。
- 实施记录:新增 `src/scenario.rs`,复用 1.2 的 `run_chat_json` 传输层。流程:browser 探针采集 DOM 摘要 → `generate_scenarios`(需求 + 元素摘要 → 每需求一个动作序列)→ **静态 selector 校验即"dry-run 校验"**:操作类动作(click/fill/press)的 selector 必须来自页面元素列表,不通过则通过 `run_chat_json` 的反馈重问回路自动修正(≤2 轮),断言/等待类允许任意 CSS selector。执行:每场景前 goto+等待隔离状态、结尾截图,一次 sidecar run 执行所有场景,按 index 区间切回各场景结果。report 新增 `scenarios` 字段;review 消费:失败场景关联需求生成高严重度 Issue。启用条件:有 sidecar + `--provider` 非 Mock;默认 Mock 走 1.4 通用采集。`browser`/`review`/`propose` 均加 `--provider`/`--no-cache`,review 的需求也改走 1.3 精解析。真实浏览器端到端(含 selector 修正回路的真机表现)放最后真机验收。

### 1.6 服务器生命周期编排(3~4 天)——已完成(2026-07-08)

- launch 改造为 `ManagedApp`:启动 → 就绪探测(轮询 base_url / 日志正则,超时可配)→ 就绪后保持运行并持续采集 stdout/stderr → 测试结束后优雅关停。
- **修进程树 kill**:Windows 用 Job Object(`windows` crate)或 `taskkill /T /F`;Unix 用进程组 kill。
- `review --execute` 变为编排器管理的完整流程:起服务 → 等就绪 → 跑浏览器计划 → 收尾。消除"健康服务器被报告为失败"的语义错误。
- 实施记录:runtime.rs 新增 `ManagedApp`(`start_app` 启动持有进程、`wait_until_ready` 轮询 base_url HTTP 直到响应/进程退出/超时、`shutdown` 杀进程树+采集日志返回 LaunchReport)。进程树 kill 采用 **taskkill /T /F(Windows)+ 进程组负 PID kill(Unix,`build_process` 设 `process_group(0)`)**,未引入 windows crate。`LaunchReport` 新增 `readiness` 字段。`review --execute`(且未 skip launch/browser)走 `run_orchestrated`:起服务→等就绪→跑浏览器→关停,取代原先各自独立的一次性 launch;server 起不来则记录 launch 错误并仍尝试浏览器(用户可能自行启动了服务)。就绪即视为 launch 成功(API 500 等属浏览器层证据),消除健康服务器被判失败的语义错误。日志正则就绪探测未做(仅 HTTP 轮询),够用。真机端到端(自动起 FocusBoard + 浏览器)放最后真机验收。

### 1.7 缺陷诊断真实化(3~4 天)——已完成(2026-07-08)

- 把(需求 + 验收标准 + 动作执行结果 + 截图 + console 错误 + 网络 5xx + 服务端日志 + 相关源码片段)打包给 LLM,输出 schema 约束的 `Issue`(含源码定位猜想 + 置信度)。
- 源码片段检索先用启发式:按路由路径、文件名、关键词 grep 找相关文件,截取片段喂给模型。
- 实施记录:新增 `src/diagnosis.rs`,在确定性规则 Issue 之上**叠加**一层 LLM 深度诊断(规则 Issue 始终保留,离线可用)。流程:从失败 Issue(运行期 RuntimeFailure/BrowserFailure 高严重度)提取关键词(CSS selector、URL 路径段、kebab/snake 标识符)→ 遍历项目源文件 grep 命中行截取带行号片段(跳过 node_modules/target 等,限量 12 片段)→ 失败发现 + 源码片段交 LLM → 输出带 `source_locations`(文件+行+片段)、`confidence`、`suggested_fix` 的诊断。校验:引用的源文件必须来自检索到的片段集合(严格拒绝臆造、宽容丢弃),related_issue_ids 过滤未知。复用 `run_chat_json`。`ReviewReport` 新增 `diagnoses` 字段;仅 `--provider` 非 Mock 且有可诊断失败时触发,失败不阻断 review。5 个单测(标识符提取、源码检索跳过依赖目录、未知文件拒绝+宽容过滤、关键词去重限量、Mock 空)。**Phase 1 全部完成。**

**Phase 1 验收门(用 FocusBoard)**:`review --execute` 自动检出 5 个注入缺陷中至少 4 个,且无高严重度误报。

**验收结果(2026-07-08,DeepSeek + Chromium 真机,详见 docs/ACCEPTANCE.md)**:全链路端到端跑通(精解析→起服务→就绪→真浏览器→场景→规则 Issue→LLM 源码诊断);注入缺陷**检出 4/5**(API 500、空输入、统计、持久化;筛选漏检),从基线 1/5 大幅提升,**达到数量门槛**。同时暴露 LLM 输出波动(1 个断言 selector 猜测误报、诊断在多失败时过度合并、筛选缺 negative 断言原语)。数量目标达成;质量调优列为 Phase 2 前的收尾项(见下)与后续迭代。

### 1.8 真机验收后的质量调优——机制已完成(2026-07-08)

三项机制改进已落地并验证(详见 docs/ACCEPTANCE.md 复测节):

- ✅ 否定断言原语:sidecar 增加 `expect_hidden`(`waitForSelector state:hidden`),筛选/隐藏类需求可用。
- ✅ 断言 selector 收敛:prompt 引导用文本断言而非猜测 class/id,消除了 `.error-banner` 类误报。
- ✅ 诊断防过度合并:prompt 要求每根因一诊断,恢复为多个独立诊断,定位不再漂移。

**遗留(边际收益递减,需更大工程)**:纯 prompt 无法根除 LLM 场景生成的内在波动(漏步骤、编造断言目标、断言强度不一),检出率单次在 3~4/5 波动。稳定提升需要:

- **场景执行级修复回路**:生成 → dry 执行 → 失败步骤带执行证据回喂 LLM 修正(当前仅静态 selector 校验)。这是 1.5 原设想但未做的部分,列为独立硬骨头。
- **多次采样取一致**:同一需求多次生成/执行,取稳定判定,降低单次波动影响。

投入产出上,上述两项可在 Phase 2 之后按需推进;不建议继续堆叠 prompt 规则(过拟合风险)。

**2026-07-12 决定启动尝试**:Phase 0-3 完成后以 `v0.9.0` 打里程碑 tag 作回档基线;1.8 在独立分支 `phase-1.8-repair-loop` 上推进,验收达标才合入 main——尝试失败则弃分支,main 永远停在完整可用形态。

**进展(2026-07-12,分支)——场景执行级修复回路已实现**:
- 分类规则(防目标冲突的核心):场景第一个失败步骤是**操作类**(goto/wait/click/fill/press)→ 场景本身坏了,可修复;是**断言类**(expect_*)→ 缺陷证据,**绝不修复**;步骤未执行(前置失败)→ 不修复。
- 修复回路:坏场景带执行证据(失败步骤+错误详情+**失败当刻 DOM 快照**,runner 新增 failure_snapshot 事件)回喂 LLM 修正 → 只重执行修正的场景 → 按需求 ID 替换结果(一轮上限)。
- **断言强度护栏**:修复输出的 expect_* 类型多重集与 expect_text 断言文本必须与原场景一致(只许修操作步骤与断言 selector),违反则反馈重问。
- 修复成功的场景:第一次执行的失败动作降为 Info 诊断,整体成功以修复后结果为准(否则白修);修复失败则保留原始失败证据。
- 真机 e2e(真 sidecar + 假 LLM)双向验证:操作失败→修复→通过(LLM 恰 2 次调用);断言失败→不修复(LLM 恰 1 次调用),缺陷证据保留。
- 待做:多次采样取一致;真实 LLM(DeepSeek)对 FocusBoard 的检出率稳定性验收(达标才合 main)。

## 5. Phase 2 — 让它好用(2~3 周)

| 编号 | 任务 | 具体落实 |
| --- | --- | --- |
| 2.1 | 一键主命令 `specprobe check [PATH]` ✅ 已完成(2026-07-10) | 新增 `src/check.rs`:scan → 需求精解析 → 编排执行 → 浏览器 → 诊断,默认写 `.specprobe/report.html`(`--no-html` 关闭)。安全边界:探测到启动命令时交互确认(stderr 提问,非 TTY/EOF 视为拒绝并安全降级为计划级),`--yes` 跳过;未检测到命令则无需确认。确认回调可注入(单测覆盖拒绝降级、提示词含命令、无命令跳过确认);真机验证一条命令从零到 HTML 报告 |
| 2.2 | 配置文件 `specprobe.toml` ✅ 已完成(2026-07-10) | 项目根 `specprobe.toml`:base_url、provider、requirements、超时、no_cache。`specprobe init` 生成模板(拒绝覆盖,`--force` 强制)。优先级 **CLI 显式参数 > 环境变量(`SPECPROBE_BASE_URL`/`SPECPROBE_PROVIDER`)> 项目配置 > 默认**,逐字段合并;未知字段/非法 provider 显式报错。`check` 已接入。**遗留**:用户级 `~/.config/specprobe/config.toml`、启动命令覆盖与就绪探测配置(属 runtime 管道) |
| 2.3 | 终端体验 ✅ 已完成(2026-07-10) | `indicatif` 阶段 spinner(scan / 精解析 / 起服务就绪 / 浏览器 / 诊断),消除长静默。进度走 stderr、非 TTY(管道/CI/重定向)自动静默、`--json` 显式禁用,stdout 保持纯 JSON。核心逻辑经 `&(dyn Fn(&str)+Sync)` 阶段回调解耦(review/check 加 `_with_progress` 变体,旧 API 向后兼容)。**遗留**:彩色分级输出与 `miette` 错误建议 |
| 2.4 | 运行归档与状态存储 ✅ 已完成(2026-07-10) | 每次 check/review 归档 report.json 到 `.specprobe/runs/<run-id>/`;SQLite(`rusqlite` bundled)`.specprobe/specprobe.db` 存 runs 索引与 issues(含 approval 列,为 2.5 预留)。新增 `runs list` / `runs show <id>`(`--json` 支持),`--no-store` 关闭归档。存储与核心逻辑解耦(CLI 层归档,失败仅告警)。**遗留**:`runs diff`(修复前后对比) |
| 2.5 | 审批工作流落地 ✅ 已完成(2026-07-10) | `issues list / show <ID> / accept\|reject\|ignore <ID> [--note]`(`--run` 指定运行、默认最近,`--json` 支持)。**Issue 指纹**(类别+需求+标题 SHA-256)使审批按指纹跨 run 持久:重跑时同指纹问题**继承**审批状态(不回到 pending),`issues list` 默认隐藏 ignored(`--all` 显示)。真机验证 accept/ignore → 重跑继承。这是"平台"与"一次性脚本"的分水岭 |
| 2.6 | HTML 报告 ✅ 已完成(2026-07-08,插队优先) | `review --html <PATH>` 输出单文件自包含 HTML(`minijinja` 模板 + base64 内联截图,light/dark 自适应),含摘要卡片、问题、AI 诊断(源码定位)、交互场景(带截图)、需求、证据。真机验证 3.9MB 报告 12 张截图内联可显示。配套报告质量优化:网络失败去重计数、有场景时跳过冗余笼统问题 |
| 2.7 | 跨平台 ✅ 已完成(2026-07-10) | CI 拆为 `lint`(fmt/clippy,单平台) + `test`(ubuntu/windows/**macos** 三平台矩阵冒烟),验证进程树 kill(taskkill / 进程组)、路径、bundled SQLite 的平台差异。核心早已用 `cfg(windows/unix)` 处理差异(macOS 走 unix 分支)。README 补 Linux/macOS 直接 `cargo` 说明。`cargo-msvc.ps1` 是本机 MSVC workaround(非产品功能),正常环境直接 cargo,不需跨平台等价物 |

**验收门**:新用户在一个陌生 Web 项目上,从 `specprobe init` 到拿到 HTML 报告 ≤ 3 条命令、≤ 5 分钟,全程无需查文档。

## 6. Phase 3 — 修复闭环与信任(2~3 周)

| 编号 | 任务 | 具体落实 |
| --- | --- | --- |
| 3.1 | 真补丁生成 ✅ 已完成(2026-07-11) | `specprobe fix <ISSUE-ID> [--run] --provider <p>`:从归档 report.json 取 issue + 诊断源码定位 → LLM 读源文件全文 → 输出 unified diff(`src/patch.rs`)。硬约束落进 `run_chat_json` 的 parse 回路:只允许修改提供的文件、diff 必须过 `git apply --check`(经 stdin,`--recount`,无需 git 仓库),不过则带反馈自动重问。真机(fake OpenAI 端点)验证读报告→生成→校验→打印全链路;git apply "corrupt patch"(补丁无末尾换行)已修。**仅生成不应用**(应用属 3.2)|
| 3.2 | 安全应用 ✅ 已完成(2026-07-12) | `fix <ISSUE-ID> --apply [--allow-dirty]`(`src/apply.rs`):前置校验项目为 git 仓库 + 工作区干净(否则 `--allow-dirty` 豁免);应用前展示 diff 并终端确认(非 TTY/EOF/非 y 一律拒绝)。创建隔离分支 `specprobe/fix-<issue>` → `git apply --index` → 在该分支提交 → **切回用户原分支**,绝不碰当前分支工作区;任何一步失败自动回滚(切回原分支 + 删除新分支)。分支已存在则拒绝。真机验证 apply/abort/branch-exists/dirty/non-git 全路径 |
| 3.3 | 自动回归闭环 ✅ 已完成(2026-07-12) | `fix --apply --verify`(`src/regression.rs`):用 `git worktree` 把修复分支物化到临时目录(不碰用户工作区)→ 按 baseline 配置重跑评审 → 按 Issue 指纹对比修复前后。目标缺陷消失且无新增问题 → `verified`;否则自动回滚(删除分支)并列出仍在/新增指纹。纯裁决 `evaluate` + 编排 `verify_on_branch` 分离,均独立测试(需求质量缺陷作确定性载体,无需运行时)。CLI 真机验证 verified(留分支)/ failed(回滚)双路径。**遗留**:代码级缺陷的验证需在 worktree 内真实启动应用(依赖项目运行时/依赖),这部分沿用 FocusBoard 手工验收 |
| 3.4 | 安全强化 ✅ 已完成(2026-07-12) | ①启动命令确认记忆:`check` 确认过的启动命令按"项目+命令"存入 SQLite(`approved_commands`),下次同项目同命令免再确认(变更命令重新询问);②AI 出站脱敏:所有发给 LLM 的消息在单一出站入口 `run_chat_json` 过密钥脱敏(`src/redact.rs`,敏感键的值 + sk-/ghp_ 等已知令牌前缀);③威胁模型文档 [docs/THREAT_MODEL.md](THREAT_MODEL.md)。真机验证命令记忆(首次提示→记住→二次免提示) |
| 3.5 | 稳定性 ✅ 已完成(2026-07-12) | ①动作失败重试一次(仅时序敏感动作 click/fill/expect_* 等,goto/screenshot 不重试),`action_result` 带 `attempts`,二次仍失败抓失败截图作证据;②**Playwright trace 归档**(runner tracing.start/stop → trace.zip 落到 run 证据目录,`trace` 事件回传路径 → `outcome.trace_path`);③**sidecar 崩溃恢复**:事件流无 finished/fatal 时 Rust 合成 fatal("possible sidecar crash"),崩溃变显式高severity失败而非静默未完成。协议保持 v1(新增向后兼容)。真机 sidecar 验证 retry/失败截图/trace.zip 落盘 |

**验收门**:对 FocusBoard 的空输入校验缺陷,端到端完成"诊断 → 提案 → 用户 accept → 应用到分支 → 回归验证通过"且统计/筛选用例无回归。

## 7. Phase 4 — 广度与成熟(已决定不做,2026-07-12)

> 用户决定:产品化目标(真实可用/好用/成熟)已由 Phase 0-3 达成,广度扩展不再投入;下表保留作历史规划参考。优先级转向 1.8 场景执行级修复回路(深水区尝试)。

| 编号 | 任务 | 说明 |
| --- | --- | --- |
| 4.1 | API 执行器 | 纯 Rust(reqwest):从需求生成接口用例(方法/路径/断言)。很多缺陷不需要浏览器,性价比高,**可提前到 Phase 2 之后** |
| 4.2 | CLI 应用执行器 | spawn + stdin/stdout 断言;交互式程序用 `portable-pty` |
| 4.3 | 适配器库 | Vite/Next/Flask/FastAPI/Spring Boot 的启动命令与就绪特征;monorepo 支持 |
| 4.4 | CI 集成 | GitHub Action(`specprobe check --json` + PR 评论);退出码约定:0=无高危,1=有高危,2=运行错误 |
| 4.5 | 发布工程 | `cargo-dist` 出三平台二进制;scoop/homebrew 分发;JSON 报告加 `report_version` 字段做 schema 版本化;CHANGELOG |
| 4.6 | 可选 UI | ratatui TUI 或本地 Web UI。HTML 报告 + CLI 审批已够用,此项仅加分 |

## 8. 技术选型汇总

| 依赖/工具 | 用途 | 引入阶段 |
| --- | --- | --- |
| `assert_cmd` + `predicates` + `insta` | CLI 集成测试与 JSON 快照 | 0 |
| `tokio` | 异步运行时、进程编排 | 1.1 |
| `reqwest`(rustls) | AI 传输、页面探测、就绪轮询 | 1.1 |
| Playwright(Node sidecar) | 浏览器执行器 | 1.4 |
| `windows` crate(Job Object) | Windows 进程树管理 | 1.6 |
| `indicatif` + `miette` | 终端进度与错误体验 | 2.3 |
| `rusqlite` | 运行索引、审批状态 | 2.4 |
| `minijinja` | HTML 报告模板 | 2.6 |
| `portable-pty` | 交互式 CLI 执行器 | 4.2 |
| `cargo-dist` | 多平台发布 | 4.5 |

## 9. 风险与对策

| 风险 | 对策 |
| --- | --- |
| LLM 输出不稳定/格式错误 | schema 校验 + 温度 0 + 重试 ≤2 + 响应缓存;规则引擎永远保留为降级路径 |
| Playwright sidecar 协议膨胀 | 首版只做 9 个动作;协议带版本号;先满足 FocusBoard 基准再扩 |
| Token 成本失控 | 响应缓存;喂 DOM 摘要而非全量 HTML;分级模型(便宜模型做解析,强模型做诊断) |
| 执行被测项目的安全风险 | 命令确认/白名单、超时、进程树管控、目录限制;后续评估容器隔离 |
| 单人开发时间不够 | Phase 0+1 即可产生质变;2.6 HTML 报告可随时插队;Phase 3/4 明确标注为后续工作 |

## 10. 与旧文档的关系

- **PROJECT.md**:继续作为状态台账 + 开发日志;其课程里程碑(M0-M8)与旧"下一步任务"标记为历史,前瞻规划以本文件为准。
- **docs/DEMO_GUIDE.md / docs/EXPERIMENT.md / docs/COURSE_REPORT.md**:课程阶段历史记录,保留不动;EXPERIMENT.md 的 20% 检出率是本路线图的基线数据。
- **demo/buggy-task-board**:从演示道具升级为基准测试集,KNOWN_ISSUES.md 是判分答案。
- **README.md**:已按接手后的定位重写,能力描述与实现保持一致,不超前宣传。
