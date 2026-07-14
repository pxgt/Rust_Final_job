# SpecProbe 3 分钟介绍视频 —— 录制脚本

> 目标档次:A 档(清晰完整、条理清楚、展示核心功能与亮点、说明与开源项目的区别)。
> 总时长控制在 2:40–2:55,给片头片尾留余量。

## 录前准备(一次性,约 10 分钟)

1. 终端窗口最大化,字号调大(建议 16pt+,评委在压缩视频里也要能看清)。
2. 预热演示环境,把"等待时间"排除在录制之外:
   ```powershell
   # 提前构建 + 预跑一次,让 npm/Playwright 都是热的
   .\scripts\cargo-msvc.ps1 build
   $env:OPENAI_BASE_URL="https://api.deepseek.com"; $env:OPENAI_API_KEY="sk-..."; $env:OPENAI_MODEL="deepseek-chat"
   ```
3. 提前开一个浏览器标签页停在 `.specprobe/report.html`(上一次 check 的产物),避免现场等待。
4. 录屏工具任选:**OBS Studio**(推荐,免费)或 Windows 自带 **Xbox Game Bar(Win+G)**;麦克风口播,或后期配音。

## 分镜脚本

### 0:00–0:20|开场:问题与定位(口播 + 展示 README 首屏)
> "AI 让写代码变快了,但'验证'成了瓶颈——需求是自然语言,代码是 AI 生成的,谁来证明它真的符合需求?
> SpecProbe 是我用 Rust 开发的证据驱动测试工具:一条命令,把需求文档变成真实浏览器里执行的测试,
> 产出带证据链的问题清单,还能生成补丁、隔离应用、自动回归验证。"

画面:README 标题 + 架构一句话;可叠加一行字幕"需求 → 执行 → 检出 → 诊断 → 修复 → 回归"。

### 0:20–1:10|核心演示:一键 check(提前录好命令执行过程,可 2 倍速)
```powershell
.\target\debug\specprobe.exe check .\demo\buggy-task-board --base-url http://127.0.0.1:4173 --provider openai-compatible --samples 2
```
口播要点(边跑边说):
> "被测项目是内置的 FocusBoard,一个注入了 5 个已知缺陷的任务看板。
> SpecProbe 自动识别启动命令——注意这里会先请求确认,这是安全边界;
> 然后托管启动服务、用 LLM 把 9 条需求变成带真实 selector 的交互场景,在真实 Chromium 里逐个执行。"

跑完后切到 `.specprobe/report.html`:
> "这是产出的报告:每个问题都关联需求、附截图和网络证据——比如 API 500 这个缺陷,LLM 诊断直接定位到了 server.js 第 46 行。"

### 1:10–1:40|亮点一:检出稳定性(口播 + 报告/ACCEPTANCE 表格特写)
> "LLM 生成测试有内在波动。SpecProbe 用两个机制解决:
> 一是执行级修复回路——测试自身坏了就带着失败证据让模型修,但断言失败是缺陷证据,绝不允许修改,
> 还有断言强度护栏,模型想弱化断言会被直接拒绝;
> 二是多轮采样取检出并集。效果:检出率从基线 1/5,到真机 6 轮复测全部 4/5 以上、多数 5/5。"

画面:ACCEPTANCE.md 的 6 轮验收表格特写(A1–B3)。

### 1:40–2:25|亮点二:修复闭环(提前录好 fix 演示)
```powershell
.\target\debug\specprobe.exe issues list
.\target\debug\specprobe.exe fix ISSUE-00X --provider openai-compatible --apply --verify
```
口播:
> "确认的问题可以一键修复:LLM 生成补丁,必须通过 git apply --check 校验;
> 应用只落到隔离分支,绝不碰你当前的工作区;然后用 git worktree 重跑评审做回归验证——
> 目标缺陷消失且无新增问题才算通过,否则自动回滚。最终看到 Regression check PASSED。"

画面:终端里 diff 展示 → 确认 → "Applied patch on branch specprobe/fix-..." → "Regression check PASSED"。

### 2:25–2:50|与开源项目的区别 + 收尾(口播 + 一页对比字幕)
> "和 Playwright Codegen 比,它的输入是需求文档而不是人工录制;
> 和 AI 断言生成工具比,它多了防目标冲突的修复回路和到源码行号的诊断;
> 和 Aider 这类 AI 修码工具比,它坚持先有运行证据、修复必须隔离且可回滚。
> 项目约 1.4 万行 Rust、103 个测试、三平台 CI 全绿,全部源码原创并开源。谢谢观看。"

画面:报告 5.3 节对比表 → GitHub 仓库页收尾。

## 录制与压缩建议

- 命令执行过程**先录素材再剪辑**,等待段落加速(2–4 倍)或跳剪,保证总时长 <3:00;
- 分辨率 1080p 录制;导出后若文件过大,用 HandBrake(预设 Fast 1080p30)或
  `ffmpeg -i in.mp4 -vcodec libx264 -crf 28 -preset slow out.mp4` 压缩,一般可压到 20–40MB;
- 口播如果不想露声,可用剪映/Clipchamp 的文字转语音,同步加字幕(评委静音观看也不丢信息)。
