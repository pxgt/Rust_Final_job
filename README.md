# SpecProbe

SpecProbe 是一个使用 Rust 开发的、面向 AI 辅助开发项目的智能审查、自动化测试与缺陷诊断工具。

它把宽泛的产品要求转换为可验证的验收标准和测试计划,通过真实运行被测项目采集证据(进程日志、HTTP 响应等),生成带证据链的问题清单与可审批的修复提案。核心原则:AI 负责理解和推理,确定性程序负责执行与取证,用户保留最终修改权。

项目起源于 Rust 课程大作业(0.8.0 完成课程阶段交付),现已按产品化目标推进,完整规划见 [docs/ROADMAP.md](docs/ROADMAP.md)。

## 当前能力(0.8.0)

- `specprobe doctor`:检查本机 Rust、Git、Node、MSVC 和 AI 接入条件。
- `specprobe scan <PATH>`:识别项目技术栈、需求文档、源码语言及测试文件。
- `specprobe requirements <PATH>`:解析 Markdown/TXT 需求文档,生成需求、验收标准和初始测试计划(基于关键词规则)。
- `specprobe ai <PATH>`:通过大语言模型对需求解析结果生成结构化改进建议。支持 OpenAI 兼容端点(含 DeepSeek)与本地 Ollama 的真实调用,带 schema 校验重试、失败退避和 `.specprobe/cache` 响应缓存(`--no-cache` 关闭);默认仍为离线 Mock Provider,无需 API key。
- `specprobe launch <PATH>`:识别 Node/Rust/Python 项目启动命令,受控运行并采集 stdout、stderr、退出码和耗时。
- `specprobe browser <PATH>`:把测试计划转换为浏览器动作计划,并对 `http://` 或 `https://` 页面采集状态码、标题和正文摘要(支持重定向跟随)。
- `specprobe review <PATH>`:汇总需求质量、项目启动和页面探测证据,生成带审批状态的问题清单。
- `specprobe propose <PATH>`:把问题清单转换为修复提案、补丁预览和回归检查清单。
- 以上命令均支持 `--json`,供 AI 工作流和 CI 读取。

## 当前边界(如实声明)

以下能力**尚未实现**,是路线图 Phase 1 的核心工作,详见 [docs/ROADMAP.md](docs/ROADMAP.md):

- 真实浏览器自动化:当前"浏览器执行器"只做单页面 HTTP/HTTPS 探测,不执行点击、输入、DOM 断言、截图和 console/网络采集。
- 服务器生命周期编排:`launch` 以"进程退出"为终点,长驻服务器会在超时后被终止。
- 审批持久化与补丁应用:Issue 审批状态不落盘,修复提案只生成预览,不修改用户代码。

在 FocusBoard 基准(5 个注入缺陷)上,当前版本自动检出 1/5,详见 [docs/EXPERIMENT.md](docs/EXPERIMENT.md)。

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

## 文档索引

- [产品化路线图](docs/ROADMAP.md) — 当前有效的前瞻规划
- [项目台账与开发日志](PROJECT.md)
- [FocusBoard 基准测试项目](demo/buggy-task-board/README.md) — 含 5 个注入缺陷的判分基准
- [实验评估记录](docs/EXPERIMENT.md) — 课程阶段基线数据(1/5 检出)
- [课堂演示指南](docs/DEMO_GUIDE.md) — 课程阶段历史文档
- [课程报告初稿](docs/COURSE_REPORT.md) — 课程阶段历史文档
