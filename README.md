# SpecProbe

SpecProbe 是一个使用 Rust 开发的、面向 AI 辅助开发项目的智能审查、自动化测试与缺陷诊断工具。

它把宽泛的产品要求转换为可验证的验收标准和测试计划,通过真实运行被测项目采集证据(进程日志、HTTP 响应等),生成带证据链的问题清单与可审批的修复提案。核心原则:AI 负责理解和推理,确定性程序负责执行与取证,用户保留最终修改权。

项目起源于 Rust 课程大作业(0.8.0 完成课程阶段交付),现已按产品化目标推进,完整规划见 [docs/ROADMAP.md](docs/ROADMAP.md)。

## 快速开始

```powershell
specprobe check .\你的Web项目 --base-url http://127.0.0.1:3000
```

一条命令完成:扫描技术栈 → 解析需求 → (确认后)自动启动被测服务并探测就绪 → 真实浏览器执行 → 生成问题清单与 `.specprobe/report.html` 可视化报告。执行项目启动命令前会交互确认(`--yes` 跳过);加 `--provider openai-compatible|ollama` 启用 LLM 精解析、具体交互场景与源码级诊断。

## 当前能力(0.8.0)

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
- 以上命令均支持 `--json`,供 AI 工作流和 CI 读取。

## 当前状态与边界(如实声明)

Phase 1(真实化)已完成并通过端到端真机验收(DeepSeek + Chromium):FocusBoard 5 个注入缺陷检出 3~4/5(基线 1/5,LLM 场景生成有单次波动),API 500 缺陷经 LLM 诊断精确定位到 `server.js:47`,详见 [docs/ACCEPTANCE.md](docs/ACCEPTANCE.md)。Phase 2(易用性)进行中:一键 `check` 与 HTML 报告已完成。

尚未实现:配置文件(`specprobe.toml`)、进度条、运行归档索引与审批状态持久化(Issue 审批恒为 pending)、补丁自动应用(提案只生成预览,不修改用户代码)。检出率的稳定提升需要场景执行级修复回路,见 [docs/ROADMAP.md](docs/ROADMAP.md) 1.8 遗留。

## 本地运行

标准环境直接使用 Cargo;本开发机的 Visual Studio 未注册 vswhere,需要通过包装脚本加载 MSVC 环境:

```powershell
.\scripts\cargo-msvc.ps1 run -- doctor
.\scripts\cargo-msvc.ps1 run -- scan .
.\scripts\cargo-msvc.ps1 run -- requirements .\docs\specprobe-requirements.md
.\scripts\cargo-msvc.ps1 run -- ai .\docs\specprobe-requirements.md
.\scripts\cargo-msvc.ps1 run -- launch . --dry-run
.\scripts\cargo-msvc.ps1 run -- browser .\docs\specprobe-requirements.md --dry-run
.\scripts\cargo-msvc.ps1 run -- review .\docs\specprobe-requirements.md
.\scripts\cargo-msvc.ps1 run -- propose .\docs\specprobe-requirements.md
.\scripts\run-demo.ps1
.\scripts\cargo-msvc.ps1 test
.\scripts\cargo-msvc.ps1 --% clippy --all-targets -- -D warnings
```

`review` 和 `propose` 默认进行计划级审查,不主动执行被测项目;需要真实启动和页面探测时添加 `--execute`。

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
