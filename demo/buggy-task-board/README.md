# FocusBoard 演示项目

FocusBoard 是 SpecProbe 的故障注入基准测试项目（起源于课程 M8 阶段的演示项目）。它使用 Node.js 内置 HTTP 模块运行，不需要安装第三方依赖。

自 2026-07-06 起，本项目升级为 SpecProbe 的**基准测试集**：`KNOWN_ISSUES.md` 中的 5 个注入缺陷是判分答案，[ROADMAP](../../docs/ROADMAP.md) 各阶段验收门以其检出率量化（接手基线 1/5）。

## 启动

```powershell
node server.js --port 4173
```

浏览器访问 `http://127.0.0.1:4173`。

## 演示用途

- `REQUIREMENTS.md`：输入给 SpecProbe 的需求文档。
- `KNOWN_ISSUES.md`：预置缺陷基准答案。
- `/api/tasks`：固定返回 HTTP 500，用于生成确定性的失败证据。
- `public/app.js`：包含空任务校验、统计、筛选和持久化方面的故障注入。

从项目根目录执行 `.\scripts\run-demo.ps1` 可以生成完整的 SpecProbe JSON 演示报告。
