# SpecProbe

SpecProbe 是一个使用 Rust 开发的、面向 AI 辅助开发项目的可扩展智能审查、自动化测试与缺陷诊断平台。

项目目标是把宽泛的产品要求转换为可验证的验收标准和可执行测试，通过真实运行证据发现功能缺失、运行错误和交互问题，再由 AI 生成可追踪的缺陷解释与修改建议。所有修改都需要用户审批。

平台在设计上不限定项目语言、框架或交互形态。课程版本优先实现 Web 项目适配器和浏览器测试执行器，后续可以扩展到命令行程序、后端服务、桌面应用和移动应用。

## 当前能力

- `specprobe doctor`：检查本机 Rust、Git、Node、MSVC 和 AI 接入条件。
- `specprobe scan <PATH>`：识别项目技术栈、需求文档、源码语言及测试文件。
- `specprobe requirements <PATH>`：解析 Markdown/TXT 需求文档，生成需求、验收标准和初始测试计划。
- `specprobe ai <PATH>`：通过 AI Provider 对需求解析结果生成改进建议。默认 Mock Provider 不需要 API key。
- `specprobe launch <PATH>`：识别项目启动命令，受控运行并采集 stdout、stderr、退出码和耗时。
- `specprobe browser <PATH>`：把测试计划转换为浏览器动作计划，并可对本地 `http://` 页面采集状态码、标题和正文摘要。
- `specprobe review <PATH>`：汇总需求质量、项目启动和浏览器证据，生成带审批状态的问题清单。
- `specprobe propose <PATH>`：把问题清单转换为可审批的修复提案、补丁预览和回归检查清单。
- `scripts/run-demo.ps1`：在 FocusBoard 故障注入项目上运行完整课程演示并归档 JSON 报告。
- 以上命令均支持 `--json`，供后续 AI 工作流读取。

## 本地运行

这台开发机需要先加载 Visual Studio C++ 环境：

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

`review` 和 `propose` 默认进行计划级审查，不主动执行被测项目；需要真实启动和页面探测时添加 `--execute`。当前修复提案只生成预览和回归检查，不自动修改用户代码。真实云端模型调用和 Playwright 深度浏览器自动化会在后续接入。当前已预留 OpenAI 兼容 Provider 和 Ollama Provider 的配置入口；Mock Provider 可用于离线演示和单元测试。

## 课程演示资料

- [课堂演示指南](docs/DEMO_GUIDE.md)
- [实验评估记录](docs/EXPERIMENT.md)
- [课程报告初稿](docs/COURSE_REPORT.md)
- [FocusBoard 故障注入项目](demo/buggy-task-board/README.md)

项目的详细状态、设计决策和开发记录见 [PROJECT.md](PROJECT.md)。
