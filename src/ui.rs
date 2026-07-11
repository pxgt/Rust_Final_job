//! 终端进度反馈(ROADMAP 2.3)。
//!
//! 长阶段(需求精解析、起服务、浏览器执行、诊断)用 spinner 消除黑屏等待。
//! 进度走 stderr,不污染 `--json` 的 stdout;非 TTY(管道 / CI / 重定向)时
//! indicatif 自动隐藏动画,不产生乱码。核心逻辑通过 `&(dyn Fn(&str) + Sync)`
//! 阶段回调解耦,不依赖本模块。

use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};

/// 进度句柄。`enabled=false`(如 `--json`)或非 TTY 时为静默 no-op。
pub struct Progress {
    bar: Option<ProgressBar>,
}

impl Progress {
    /// 创建 spinner。`enabled=false` 返回静默句柄;终端非交互时 indicatif 亦自动静默。
    pub fn spinner(enabled: bool) -> Self {
        if !enabled {
            return Self { bar: None };
        }
        let bar = ProgressBar::new_spinner();
        bar.set_style(
            ProgressStyle::with_template("{spinner:.cyan} {msg}")
                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
        );
        bar.enable_steady_tick(Duration::from_millis(120));
        Self { bar: Some(bar) }
    }

    /// 更新当前阶段文本。
    pub fn stage(&self, message: &str) {
        if let Some(bar) = &self.bar {
            bar.set_message(message.to_owned());
        }
    }

    /// 收尾并清除 spinner 行(最终结果由调用方另行打印)。
    pub fn finish(&self) {
        if let Some(bar) = &self.bar {
            bar.finish_and_clear();
        }
    }

    /// 借出阶段回调,传给核心逻辑(review/check)。
    pub fn stage_fn(&self) -> impl Fn(&str) + Sync + '_ {
        move |message: &str| self.stage(message)
    }
}

/// 无操作阶段回调:核心逻辑的默认参数与测试使用。
pub fn noop_progress(_: &str) {}
