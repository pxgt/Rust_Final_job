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

### 1.1 异步化改造(2~3 天)

- 引入 `tokio`(full)+ `reqwest`(json + rustls-tls)。
- `main` 改 `#[tokio::main]`;`std::process::Command` → `tokio::process`;页面探测改用 reqwest(顺带解决 https、重定向、超时)。
- 模块边界不动,工作量主要是 async 签名传染。必须最先做:后续服务器进程、浏览器 sidecar、AI 调用需要并发管理。

### 1.2 真 AI Provider(约 1 周)

- OpenAI 兼容 chat completions 传输:优先 `response_format: json_schema`;对不支持的端点降级为"prompt 内嵌 schema + 响应校验 + 温度 0 + 重试 ≤2"。
- Ollama `/api/chat`(本地免费,保证开发迭代和离线演示)。
- 统一基础设施:指数退避重试、超时、token 用量统计(写入报告)、错误分类(认证/限流/网络/格式)。
- **响应缓存**:请求体 hash → 响应,落盘 `.specprobe/cache/`。同时解决省钱、离线重放、演示确定性三件事。
- Mock Provider 保留为 `--provider mock` 与单测路径。
- 配置从纯环境变量升级为配置文件 + 环境变量覆盖(配置文件全量设计在 2.2)。

### 1.3 需求理解升级为两级流水线(3~5 天)

- 规则引擎(现有代码)降级为:候选段落粗筛 + LLM 不可用时的兜底。
- LLM 精解析:输入文档全文 + `scan` 技术栈信息,输出 schema 约束的 `Requirement[]`(复用现有数据模型)。
- 验收标准要求落到具体页面/接口,不再输出通用话术。

### 1.4 真浏览器执行器(1.5~2 周,最大单项)

- **技术决策:Playwright Node sidecar,不用纯 Rust CDP(chromiumoxide)。**理由:Playwright 自带 auto-wait、选择器引擎、截图、console/network 捕获、trace,稳定性远超自写 CDP;被测目标本就是 Web 项目,Node 已在环境假设内;符合"Rust 编排、执行器执行"的既定架构。
- 实现:新建 `executors/playwright-runner/`(独立 npm 包:`runner.mjs` + playwright 依赖)。
- 协议:Rust 经 stdin 发 JSON 动作计划;runner 逐动作执行,stdout 每行一个 JSON 事件(动作结果 / console 消息 / 网络失败 / 截图路径);协议带版本号。
- Rust 端实现 `TestExecutor` trait(spawn + 双向流)。
- 动作集(首版 9 个):`goto / click / fill / press / wait_for_selector / expect_text / expect_visible / screenshot / eval`。
- 证据落盘 `.specprobe/runs/<run-id>/`:截图 PNG、console.json、network.json、trace.zip。
- `doctor` 增加 Playwright 检查;新增 `specprobe setup-browser` 一键执行 `npx playwright install chromium`。

### 1.5 测试计划生成真实化(3~4 天)

- 先让执行器打开首页,抓取可交互元素摘要(accessibility tree:可见按钮、输入框、链接)。
- 元素摘要 + 需求一起喂给 LLM,生成带真实 selector 的具体步骤。
- 生成后 dry-run 校验 selector 存在;不存在的回喂 LLM 修正(≤2 轮)。
- 产出形如"在 `#task-input` 输入空字符串并点击 `#add-task-btn`,断言列表数量不变",而不是"执行核心操作"。

### 1.6 服务器生命周期编排(3~4 天)

- launch 改造为 `ManagedApp`:启动 → 就绪探测(轮询 base_url / 日志正则,超时可配)→ 就绪后保持运行并持续采集 stdout/stderr → 测试结束后优雅关停。
- **修进程树 kill**:Windows 用 Job Object(`windows` crate)或 `taskkill /T /F`;Unix 用进程组 kill。
- `review --execute` 变为编排器管理的完整流程:起服务 → 等就绪 → 跑浏览器计划 → 收尾。消除"健康服务器被报告为失败"的语义错误。

### 1.7 缺陷诊断真实化(3~4 天)

- 把(需求 + 验收标准 + 动作执行结果 + 截图 + console 错误 + 网络 5xx + 服务端日志 + 相关源码片段)打包给 LLM,输出 schema 约束的 `Issue`(含源码定位猜想 + 置信度)。
- 源码片段检索先用启发式:按路由路径、文件名、关键词 grep 找相关文件,截取片段喂给模型。

**Phase 1 验收门(用 FocusBoard)**:`review --execute` 自动检出 5 个注入缺陷中至少 4 个(API 500、空输入校验、统计不更新、筛选失效;持久化缺陷需要 reload 场景,列为挑战项),且无高严重度误报。**不达标不进 Phase 2。**

## 5. Phase 2 — 让它好用(2~3 周)

| 编号 | 任务 | 具体落实 |
| --- | --- | --- |
| 2.1 | 一键主命令 `specprobe check [PATH]` | 串起 scan → requirements → 计划 → 启动 → 浏览器 → 诊断 → 报告,成为默认入口;现有 8 个子命令降级为分步调试工具。首次执行前交互确认启动命令(安全边界),`--yes` 跳过 |
| 2.2 | 配置文件 `specprobe.toml` | 项目根:base_url、启动命令覆盖、就绪探测、超时、provider/model、需求文档 glob、忽略路径。`specprobe init` 生成模板。优先级:CLI 参数 > 环境变量 > 项目配置 > 用户级 `~/.config/specprobe/config.toml`。解决"8 个命令各带 7~9 个 flag"的问题 |
| 2.3 | 终端体验 | `indicatif` 每阶段进度条/spinner(消除 launch 的长静默);彩色分级输出;错误信息带"下一步怎么办"建议(可用 `miette`)。`--json` 模式进度走 stderr,stdout 保持纯 JSON |
| 2.4 | 运行归档与状态存储 | `.specprobe/runs/<timestamp-id>/` 存 report.json + 证据;SQLite(`rusqlite`)存运行索引、Issue 状态、审批记录。新增 `specprobe runs list / show / diff` |
| 2.5 | 审批工作流落地 | `specprobe issues list / show <ID> / accept\|reject\|ignore <ID> [--note]`,状态持久化。引入 Issue 指纹(类别 + 需求 ID + 关键证据 hash):重跑不重复报同指纹问题,被 ignore 的不再出现。这是"平台"与"一次性脚本"的分水岭 |
| 2.6 | HTML 报告 | `specprobe report --open`:单文件自包含 HTML(`minijinja` 模板 + base64 内联截图),含摘要卡片、按严重度排列的 Issue 列表、可展开证据链与截图、执行时间线。演示性价比最高,可随时提前插队 |
| 2.7 | 跨平台 | CI 已保证 Linux 编译;补 macOS/Linux 冒烟;PowerShell 脚本的功能收编进主程序或提供 bash 等价物 |

**验收门**:新用户在一个陌生 Web 项目上,从 `specprobe init` 到拿到 HTML 报告 ≤ 3 条命令、≤ 5 分钟,全程无需查文档。

## 6. Phase 3 — 修复闭环与信任(2~3 周)

| 编号 | 任务 | 具体落实 |
| --- | --- | --- |
| 3.1 | 真补丁生成 | 对 accepted 的 Issue:LLM 读相关源码 → 输出 unified diff。硬约束:只允许修改它实际读过的文件;diff 必须过 `git apply --check`,失败回喂重试 ≤2 |
| 3.2 | 安全应用 | 前置:被测项目 git 工作区干净(否则需 `--allow-dirty` 显式豁免);补丁应用到新分支 `specprobe/fix-issue-xxx`,绝不碰用户当前分支;应用前展示 diff 并确认 |
| 3.3 | 自动回归闭环 | 应用后自动重跑关联用例 + 全量浏览器计划,对比前后:目标缺陷消失且无新增失败 → "验证通过";否则自动回滚分支并附失败证据 |
| 3.4 | 安全强化 | 启动命令白名单 + 首次确认后记忆(SQLite);AI 出站 payload 脱敏审计(现有日志脱敏扩展到所有出站内容);文档化威胁模型 |
| 3.5 | 稳定性 | 浏览器动作失败自动重试一次 + 二次截图对比(抗 flaky);Playwright trace 归档;sidecar 崩溃恢复 |

**验收门**:对 FocusBoard 的空输入校验缺陷,端到端完成"诊断 → 提案 → 用户 accept → 应用到分支 → 回归验证通过"且统计/筛选用例无回归。

## 7. Phase 4 — 广度与成熟(持续)

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
