# SpecProbe 课堂演示指南

> 课程阶段（0.8.0）历史文档，操作步骤仍可复现。接手后的项目规划见 [ROADMAP.md](ROADMAP.md)。

## 1. 演示目标

用 FocusBoard 故障注入项目展示 SpecProbe 如何完成以下闭环：

1. 扫描项目并识别技术栈。
2. 解析需求并生成验收测试计划。
3. 使用离线 Mock Provider 给出 AI 增强建议。
4. 识别项目启动命令。
5. 探测正常页面和故障 API。
6. 根据运行证据生成 Issue。
7. 根据 Issue 生成修复提案和回归检查。

## 2. 一键演示

在项目根目录执行：

```powershell
.\scripts\run-demo.ps1
```

脚本会自动构建 SpecProbe、启动 FocusBoard、运行全流程并关闭演示服务器。JSON 报告保存在：

```text
.specprobe/demo-reports/
```

生成文件：

| 文件 | 内容 |
| --- | --- |
| `01-scan.json` | 项目技术栈和文件扫描结果 |
| `02-requirements.json` | 结构化需求、验收标准和测试计划 |
| `03-ai-mock.json` | 离线 AI 增强建议 |
| `04-launch-plan.json` | Node 项目启动命令识别结果 |
| `05-browser-home.json` | 首页 HTTP 状态、标题和正文证据 |
| `06-review-broken-api.json` | 故障 API 对应的证据和 Issue |
| `07-proposals-broken-api.json` | 修复提案、补丁预览和回归检查 |

## 3. 推荐讲解顺序

### 第一步：展示被测项目

```powershell
node .\demo\buggy-task-board\server.js --port 4173
```

打开 `http://127.0.0.1:4173`，说明这是一个由 AI 快速生成后仍带有缺陷的任务看板。

### 第二步：展示需求到测试计划

```powershell
.\scripts\cargo-msvc.ps1 run -- requirements .\demo\buggy-task-board\REQUIREMENTS.md
```

重点说明 SpecProbe 会生成需求编号、验收标准、证据类型、执行器建议和测试步骤。

### 第三步：展示确定性运行证据

```powershell
.\scripts\cargo-msvc.ps1 run -- browser `
  .\demo\buggy-task-board\REQUIREMENTS.md `
  --base-url http://127.0.0.1:4173/api/tasks
```

预期看到 HTTP 500 和页面探测失败诊断。

### 第四步：展示问题报告

```powershell
.\scripts\cargo-msvc.ps1 run -- review `
  .\demo\buggy-task-board\REQUIREMENTS.md `
  --project .\demo\buggy-task-board `
  --base-url http://127.0.0.1:4173/api/tasks `
  --execute `
  --skip-launch
```

重点展示 `browser-failure`、预期结果、实际结果、证据编号和 `pending` 审批状态。

### 第五步：展示修复提案

```powershell
.\scripts\cargo-msvc.ps1 run -- propose `
  .\demo\buggy-task-board\REQUIREMENTS.md `
  --project .\demo\buggy-task-board `
  --base-url http://127.0.0.1:4173/api/tasks `
  --execute `
  --skip-launch
```

重点展示 `PATCH-xxx`、目标文件、风险提示、补丁预览和回归命令。

## 4. 可手动复现的交互缺陷

1. 输入空格并点击“添加任务”，空任务仍进入列表。
2. 勾选任务后，“已完成”统计仍为 0。
3. 点击“已完成”筛选，列表仍显示全部任务。
4. 新增任务后刷新页面，任务消失。
5. 页面加载时 `/api/tasks` 返回 HTTP 500，并显示“部分数据未同步”横幅。

这些缺陷用于说明当前基础 HTTP 探测器和未来 Playwright 深度执行器之间的能力差异。

## 5. 答辩要点

- Rust 负责 CLI、需求解析、进程控制、HTTP 探测、证据建模和报告生成。
- AI Provider 被抽象为可替换组件，离线 Mock Provider 保证演示稳定。
- 所有 Issue 都关联证据，避免只靠模型猜测。
- 默认只读、计划级运行，不自动应用补丁。
- Web 只是课程版本的首个适配目标，架构可以继续扩展到 CLI、API 和桌面应用。
