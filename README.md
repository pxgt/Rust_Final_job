# SpecProbe

SpecProbe 是一个使用 Rust 开发的、面向 AI 辅助开发项目的智能审查、自动化测试与缺陷诊断工具。

它把宽泛的产品要求转换为可验证的验收标准和测试计划,通过真实运行被测项目采集证据(进程日志、HTTP 响应等),生成带证据链的问题清单与可审批的修复提案。核心原则:AI 负责理解和推理,确定性程序负责执行与取证,用户保留最终修改权。

项目起源于 Rust 课程大作业(0.8.0 完成课程阶段交付),现已按产品化目标推进,完整规划见 [docs/ROADMAP.md](docs/ROADMAP.md)。

## 快速开始

```powershell
specprobe check .\你的Web项目 --base-url http://127.0.0.1:3000
```

一条命令完成:扫描技术栈 → 解析需求 → (确认后)自动启动被测服务并探测就绪 → 真实浏览器执行 → 生成问题清单与 `.specprobe/report.html` 可视化报告。执行项目启动命令前会交互确认(`--yes` 跳过);加 `--provider openai-compatible|ollama` 启用 LLM 精解析、具体交互场景与源码级诊断。

## 当前能力(0.9.0)

- `specprobe check [PATH]`:一键检查(上文快速开始),是面向用户的主入口;以下子命令用于分步调试。
- `specprobe init [PATH]`:在项目根生成 `specprobe.toml` 配置模板,把常用参数(base_url、provider、需求源、超时等)写进去,之后 `check` 就不必每次带一串 flag。优先级:CLI 参数 > 环境变量 > 配置文件 > 默认。
- `specprobe doctor`:检查本机 Rust、Git、Node、MSVC 和 AI 接入条件。
- `specprobe scan <PATH>`:识别项目技术栈、需求文档、源码语言及测试文件。
- `specprobe requirements <PATH>`:两级流水线解析需求文档——规则引擎粗筛兜底,`--provider openai-compatible|ollama` 启用 LLM 精解析(带行号溯源、具体到页面/接口的验收标准,校验失败自动回退规则结果);测试计划始终由确定性代码从需求生成。
- `specprobe ai <PATH>`:通过大语言模型对需求解析结果生成结构化改进建议。支持 OpenAI 兼容端点(含 DeepSeek)与本地 Ollama 的真实调用,带 schema 校验重试、失败退避和 `.specprobe/cache` 响应缓存(`--no-cache` 关闭);默认仍为离线 Mock Provider,无需 API key。
- `specprobe launch <PATH>`:识别 Node/Rust/Python 项目启动命令,受控运行并采集 stdout、stderr、退出码和耗时。
- `specprobe browser <PATH>`:把测试计划转换为浏览器动作计划并执行。装了 Playwright runner 时用真实浏览器打开页面,采集截图、console 错误、网络失败和可交互元素摘要,证据归档到 `.specprobe/runs/`;未装时自动降级为 `http`/`https` 页面探测(状态码、标题、正文摘要,支持重定向)。
- `specprobe setup-browser`:一键安装 Playwright runner(`npm install` + `npx playwright install chromium`,需要 Node.js)。
- `specprobe review <PATH>`:汇总需求质量、项目启动和浏览器证据,生成带审批状态的问题清单。`--execute` 时用托管生命周期编排:自动启动被测服务→探测就绪→运行浏览器→优雅关停(进程树 kill);`--provider` 非 Mock 时对运行期失败叠加带源码定位与置信度的 LLM 深度诊断;`--html <PATH>` 额外输出可视化 HTML 报告(内联截图、light/dark 自适应)。
- `specprobe propose <PATH>`:把问题清单转换为修复提案、补丁预览和回归检查清单。
- `specprobe runs list` / `runs show <id>`:浏览归档的历史运行(`.specprobe/specprobe.db`)。
- `specprobe issues list` / `show <ID>` / `accept|reject|ignore <ID> [--note]`:审批问题。审批按 Issue 指纹跨运行持久——重跑时同一问题继承之前的决定,被 `ignore` 的默认不再出现(`--all` 显示)。
- 安全:发给 LLM 的所有内容在出站前做**密钥脱敏**(不外泄 API key/token);`check` 确认过的启动命令按项目**记忆**,同项目同命令下次免再确认。威胁模型见 [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md)。
- `specprobe fix <ISSUE-ID> --provider <p> [--apply]`:对已诊断的问题生成修复补丁。从归档运行读取问题与诊断的源码定位,交给 LLM 产出 unified diff,并强制只改被诊断的文件、且必须通过 `git apply --check`(不过则带反馈重问)。默认只生成并展示;加 `--apply` 时,先终端确认,再把补丁提交到**隔离分支** `specprobe/fix-<issue>`(前置项目 git 工作区干净,否则需 `--allow-dirty`),提交后切回你的原分支——绝不改动当前分支,失败自动回滚。再加 `--verify` 时,应用后用 `git worktree` 把修复分支物化重跑评审,按 Issue 指纹对比:目标缺陷消失且无新增问题 → 报"验证通过"保留分支;否则自动回滚分支并列出仍在/新增的问题。之后用 `git diff <原分支>..specprobe/fix-<issue>` 审阅再决定合并。
- 以上命令均支持 `--json`,供 AI 工作流和 CI 读取。

## 当前状态与边界(如实声明)

Phase 1(真实化)已完成并通过端到端真机验收(DeepSeek + Chromium):FocusBoard 5 个注入缺陷检出 3~4/5(基线 1/5,LLM 场景生成有单次波动),API 500 缺陷经 LLM 诊断精确定位到 `server.js:47`,详见 [docs/ACCEPTANCE.md](docs/ACCEPTANCE.md)。Phase 2(易用性)、Phase 3(修复闭环:补丁生成 → 安全应用隔离分支 → 回归验证 → 安全强化 → 稳定性)均已完成。浏览器执行支持失败重试(抗 flaky)、Playwright trace 归档与 sidecar 崩溃识别。

Phase 2(易用性)已完成:一键 `check`、`specprobe.toml` 配置、进度条、运行归档 + SQLite、审批持久化。**Phase 3(修复闭环)已完成**:3.1 真补丁生成 + 3.2 安全应用到隔离分支 + 3.3 自动回归验证(`fix --apply --verify`),端到端"诊断 → 生成补丁 → 应用到隔离分支 → 回归验证"闭环打通。检出率的稳定提升需要场景执行级修复回路,见 [docs/ROADMAP.md](docs/ROADMAP.md) 1.8 遗留。CI 在 Linux / Windows / macOS 三平台验证。

## 本地运行

Linux / macOS 直接用 Cargo:

```bash
cargo build
cargo run -- check ./你的Web项目 --base-url http://127.0.0.1:3000
cargo test
```

Windows 上,若 Visual Studio 未被 vswhere 正确注册(本开发机情况),用包装脚本加载 MSVC 环境后再执行 Cargo:

```powershell
.\scripts\cargo-msvc.ps1 run -- check .\你的Web项目
.\scripts\cargo-msvc.ps1 test
.\scripts\cargo-msvc.ps1 --% clippy --all-targets -- -D warnings
```

(MSVC 环境正常的 Windows 机器可直接用 `cargo`,不需要脚本。)

`review` 和 `propose` 默认进行计划级审查,不主动执行被测项目;需要真实启动和页面探测时添加 `--execute`(`check` 会先交互确认启动命令)。

## AI Provider 配置

`specprobe ai` 默认使用离线 Mock Provider。接入真实模型通过环境变量配置:

```powershell
# OpenAI 兼容端点(以 DeepSeek 为例)
$env:OPENAI_BASE_URL = "https://api.deepseek.com"
$env:OPENAI_API_KEY  = "sk-..."
$env:OPENAI_MODEL    = "deepseek-chat"
.\scripts\cargo-msvc.ps1 run -- ai .\docs\specprobe-requirements.md --provider openai-compatible

# 本地 Ollama
$env:OLLAMA_MODEL = "qwen2.5:7b"          # OLLAMA_BASE_URL 默认 http://127.0.0.1:11434
.\scripts\cargo-msvc.ps1 run -- ai . --provider ollama
```

说明:模型输出受 JSON schema 约束(json_object 模式 + 校验失败自动带反馈重问 ≤2 轮);网络错误、429 和 5xx 指数退避重试 ≤3 次;响应按请求指纹缓存于 `.specprobe/cache/`,重复分析零 token 消耗,`--no-cache` 可关闭。若本机需经代理访问云端 API,请设置 `HTTPS_PROXY`(reqwest 自动读取)。

## 浏览器测试

真实浏览器执行由 Playwright Node sidecar([executors/playwright-runner](executors/playwright-runner))完成,SpecProbe 通过 stdin 发送 JSON 动作计划、读取 sidecar 回传的 NDJSON 事件。首次使用先安装:

```powershell
.\scripts\cargo-msvc.ps1 run -- setup-browser   # 或手动 cd executors/playwright-runner && npm install && npx playwright install chromium
.\scripts\cargo-msvc.ps1 run -- browser .\demo\buggy-task-board\REQUIREMENTS.md --base-url http://127.0.0.1:4173
```

装好后 `browser`/`review --execute`/`propose --execute` 自动走 Playwright:打开页面、截图、采集 console 错误与网络失败、抓取可交互元素摘要,证据归档到 `.specprobe/runs/`。**未安装时自动降级为 HTTP 探测**,CI 与无 Node 环境不受影响;报告的 `backend` 字段标注实际使用的后端。设 `SPECPROBE_NO_PLAYWRIGHT=1` 可强制走 HTTP 探测(禁用浏览器执行)。

加 `--provider openai-compatible`(或 `ollama`)时启用**具体交互场景**:SpecProbe 先探针采集页面可交互元素,连同需求交给 LLM 生成带真实 selector 的动作步骤(如"在 `#task-input` 输入空串、点击 `#add-task-btn`、断言列表数量不变"),校验 selector 后逐场景执行;失败的场景在 `review` 中生成关联需求的高严重度 Issue。默认 `mock` 只做通用采集,不调 LLM。

## 文档索引

- [产品化路线图](docs/ROADMAP.md) — 当前有效的前瞻规划
- [项目台账与开发日志](PROJECT.md)
- [FocusBoard 基准测试项目](demo/buggy-task-board/README.md) — 含 5 个注入缺陷的判分基准
- [实验评估记录](docs/EXPERIMENT.md) — 课程阶段基线数据(1/5 检出)
- [课堂演示指南](docs/DEMO_GUIDE.md) — 课程阶段历史文档
- [课程报告初稿](docs/COURSE_REPORT.md) — 课程阶段历史文档
