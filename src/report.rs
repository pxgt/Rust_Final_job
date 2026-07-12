//! 自包含 HTML 报告渲染(ROADMAP 2.6)。
//!
//! 把 `ReviewReport` 渲染为单文件 HTML:内联 CSS、base64 内联截图,light/dark
//! 自适应。用于课堂演示与人工审阅,比 JSON/终端更直观。

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use minijinja::{Environment, context};

use crate::review::ReviewReport;

/// 渲染并写出 HTML 报告。截图按路径就地读取并 base64 内联,读不到则跳过。
pub fn write_review_html(report: &ReviewReport, out_path: &Path) -> Result<()> {
    let mut screenshots = BTreeMap::new();
    if let Some(browser) = &report.browser_report {
        for scenario in &browser.scenarios {
            if let Some(path) = &scenario.screenshot {
                add_screenshot(&mut screenshots, path);
            }
        }
        if let Some(playwright) = &browser.playwright {
            for action in &playwright.outcome.actions {
                if let Some(path) = &action.screenshot_path {
                    add_screenshot(&mut screenshots, path);
                }
            }
        }
    }

    let report_value = serde_json::to_value(report)?;
    let mut env = Environment::new();
    env.add_template("report.html", TEMPLATE)?;
    let template = env.get_template("report.html")?;
    let html = template.render(context! {
        report => report_value,
        screenshots => screenshots,
        version => env!("CARGO_PKG_VERSION"),
    })?;

    fs::write(out_path, html)?;
    Ok(())
}

fn add_screenshot(screenshots: &mut BTreeMap<String, String>, path: &str) {
    if screenshots.contains_key(path) {
        return;
    }
    if let Ok(bytes) = fs::read(path) {
        let encoded = STANDARD.encode(bytes);
        screenshots.insert(path.to_owned(), format!("data:image/png;base64,{encoded}"));
    }
}

const TEMPLATE: &str = r###"<!doctype html>
<html lang="zh">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>SpecProbe 审查报告</title>
<style>
:root{
  --bg:#f6f7f9; --card:#fff; --fg:#1c2024; --muted:#6b7280; --border:#e5e7eb;
  --crit:#b91c1c; --high:#dc2626; --med:#d97706; --low:#2563eb; --info:#6b7280;
  --pass:#16a34a; --fail:#dc2626;
}
@media (prefers-color-scheme:dark){
  :root{ --bg:#0f1115; --card:#181b20; --fg:#e6e8eb; --muted:#9aa4b2; --border:#2a2f37; }
}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--fg);font:14px/1.6 -apple-system,Segoe UI,Roboto,"Microsoft YaHei",sans-serif}
.wrap{max-width:960px;margin:0 auto;padding:24px 16px 64px}
h1{font-size:22px;margin:0 0 4px}
h2{font-size:16px;margin:32px 0 12px;padding-bottom:6px;border-bottom:1px solid var(--border)}
.sub{color:var(--muted);font-size:13px}
.cards{display:flex;flex-wrap:wrap;gap:12px;margin:16px 0}
.card{background:var(--card);border:1px solid var(--border);border-radius:10px;padding:12px 16px;min-width:120px;flex:1}
.card .n{font-size:24px;font-weight:600}
.card .l{color:var(--muted);font-size:12px}
.item{background:var(--card);border:1px solid var(--border);border-radius:10px;padding:14px 16px;margin:10px 0}
.item h3{margin:0 0 6px;font-size:15px}
.badge{display:inline-block;padding:1px 8px;border-radius:999px;font-size:11px;font-weight:600;color:#fff;vertical-align:middle}
.sev-critical{background:var(--crit)}.sev-high{background:var(--high)}.sev-medium{background:var(--med)}.sev-low{background:var(--low)}.sev-info{background:var(--info)}
.conf-high{background:var(--high)}.conf-medium{background:var(--med)}.conf-low{background:var(--info)}
.tag{display:inline-block;padding:1px 8px;border-radius:6px;font-size:11px;background:var(--border);color:var(--fg)}
.kv{color:var(--muted);font-size:12px;margin:2px 0}
.kv b{color:var(--fg);font-weight:600}
.pass{color:var(--pass);font-weight:600}.fail{color:var(--fail);font-weight:600}
.steps{margin:6px 0 0;padding-left:0;list-style:none;font-size:13px}
.steps li{padding:2px 0}
pre{background:var(--bg);border:1px solid var(--border);border-radius:6px;padding:8px;overflow-x:auto;font-size:12px;margin:6px 0}
img.shot{max-width:100%;border:1px solid var(--border);border-radius:6px;margin-top:8px}
.loc{font-family:ui-monospace,Consolas,monospace;font-size:12px}
footer{margin-top:40px;color:var(--muted);font-size:12px;text-align:center}
.empty{color:var(--muted);font-style:italic}
</style>
</head>
<body>
<div class="wrap">
  <h1>SpecProbe 审查报告</h1>
  <div class="sub">
    需求源:{{ report.config.requirements_source }} ·
    项目:{{ report.config.project_root }} ·
    引擎:{{ report.requirement_report.engine }} ·
    浏览器后端:{% if report.browser_report %}{{ report.browser_report.backend }}{% else %}—{% endif %}
  </div>

  <div class="cards">
    <div class="card"><div class="n">{{ report.summary.requirements }}</div><div class="l">需求</div></div>
    <div class="card"><div class="n">{{ report.summary.test_cases }}</div><div class="l">测试用例</div></div>
    <div class="card"><div class="n">{{ report.summary.issues }}</div><div class="l">问题</div></div>
    <div class="card"><div class="n" style="color:var(--high)">{{ report.summary.high }}</div><div class="l">高严重度</div></div>
    <div class="card"><div class="n" style="color:var(--med)">{{ report.summary.medium }}</div><div class="l">中严重度</div></div>
    <div class="card"><div class="n">{{ report.summary.evidence_items }}</div><div class="l">证据项</div></div>
  </div>

  {% if report.launch_report and report.launch_report.readiness %}
  <div class="kv">服务就绪:
    {% if report.launch_report.readiness.ready %}<span class="pass">是</span>{% else %}<span class="fail">否</span>{% endif %}
    · {{ report.launch_report.readiness.detail }}</div>
  {% endif %}

  <h2>问题 ({{ report.issues|length }})</h2>
  {% if report.issues %}
    {% for issue in report.issues %}
    <div class="item">
      <h3><span class="badge sev-{{ issue.severity }}">{{ issue.severity }}</span>
        {{ issue.title }} <span class="tag">{{ issue.category }}</span></h3>
      {% if issue.related_requirement %}<div class="kv"><b>需求</b> {{ issue.related_requirement }}</div>{% endif %}
      <div class="kv"><b>预期</b> {{ issue.expected }}</div>
      <div class="kv"><b>实际</b> {{ issue.actual }}</div>
      <div class="kv"><b>建议</b> {{ issue.recommendation }}</div>
      <div class="kv"><b>证据</b> {{ issue.evidence_ids|join(", ") }} · <b>审批</b> {{ issue.approval }}</div>
    </div>
    {% endfor %}
  {% else %}<p class="empty">未生成问题。</p>{% endif %}

  {% if report.diagnoses %}
  <h2>AI 诊断 ({{ report.diagnoses|length }})</h2>
    {% for d in report.diagnoses %}
    <div class="item">
      <h3><span class="badge conf-{{ d.confidence }}">{{ d.confidence }}</span> {{ d.title }}</h3>
      {% if d.related_issue_ids %}<div class="kv"><b>关联问题</b> {{ d.related_issue_ids|join(", ") }}</div>{% endif %}
      <div class="kv"><b>根因</b> {{ d.root_cause }}</div>
      {% for loc in d.source_locations %}
        <div class="kv loc">↳ {{ loc.file }}{% if loc.line %}:{{ loc.line }}{% endif %}</div>
        {% if loc.snippet %}<pre>{{ loc.snippet }}</pre>{% endif %}
      {% endfor %}
      {% if d.suggested_fix %}<div class="kv"><b>修复建议</b> {{ d.suggested_fix }}</div>{% endif %}
    </div>
    {% endfor %}
  {% endif %}

  {% if report.browser_report and report.browser_report.scenarios %}
  <h2>交互场景 ({{ report.browser_report.scenarios|length }})</h2>
    {% for s in report.browser_report.scenarios %}
    <div class="item">
      <h3>{% if s.success %}<span class="pass">PASS</span>{% else %}<span class="fail">FAIL</span>{% endif %}
        {{ s.requirement_id }} · {{ s.title }}</h3>
      {% if s.expected_observation %}<div class="kv"><b>预期</b> {{ s.expected_observation }}</div>{% endif %}
      <ul class="steps">
        {% for st in s.steps %}
        <li>{% if st.ok %}<span class="pass">✓</span>{% else %}<span class="fail">✗</span>{% endif %}
          <span class="loc">{{ st.action }} {{ st.target }}</span>
          {% if st.detail and not st.ok %} — {{ st.detail }}{% endif %}</li>
        {% endfor %}
      </ul>
      {% if s.screenshot and screenshots[s.screenshot] %}
      <img class="shot" src="{{ screenshots[s.screenshot] }}" alt="scenario screenshot">
      {% endif %}
    </div>
    {% endfor %}
  {% endif %}

  <h2>需求 ({{ report.requirement_report.requirements|length }})</h2>
  {% for r in report.requirement_report.requirements %}
  <div class="item">
    <h3>{{ r.id }} · {{ r.title }} <span class="tag">{{ r.category }}</span> <span class="tag">{{ r.priority }}</span></h3>
    <div class="kv">{{ r.description }}</div>
    <div class="kv"><b>来源</b> {{ r.source.path }}:{{ r.source.line }}</div>
    {% for ac in r.acceptance_criteria %}<div class="kv">· {{ ac.statement }}</div>{% endfor %}
  </div>
  {% endfor %}

  <footer>Generated by SpecProbe v{{ version }}</footer>
</div>
</body>
</html>
"###;

#[cfg(test)]
mod tests {
    use super::write_review_html;
    use crate::review::{ReviewOptions, generate_review_report};
    use crate::testutil::temp_project;

    use std::fs;

    #[tokio::test]
    async fn writes_self_contained_html_report() {
        let root = temp_project("specprobe-report-html");
        let requirements = root.join("PRD.md");
        fs::write(&requirements, "- 页面应该简单友好。").expect("write requirements");

        let report = generate_review_report(
            &requirements,
            ReviewOptions {
                project_path: root.clone(),
                base_url: "http://127.0.0.1:3000".to_owned(),
                provider: Default::default(),
                cache_dir: None,
                execute: false,
                skip_launch: true,
                skip_browser: true,
                launch_timeout_secs: 1,
                browser_timeout_secs: 1,
                samples: 0,
            },
        )
        .await
        .expect("review succeeds");

        let out = root.join("report.html");
        write_review_html(&report, &out).expect("html written");

        let html = fs::read_to_string(&out).expect("read html");
        assert!(html.contains("SpecProbe 审查报告"));
        assert!(html.contains("<!doctype html>"));
        // 需求质量问题应出现在报告里。
        assert!(html.contains("badge sev-"));
        assert!(html.contains("Generated by SpecProbe"));

        fs::remove_dir_all(root).expect("cleanup");
    }
}
