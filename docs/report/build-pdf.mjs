// 实验报告 PDF 导出:用本仓库 executors/playwright-runner 的 Chromium 打印 final-report.html。
// 用法: node docs/report/build-pdf.mjs   (输出 docs/report/SpecProbe-实验报告.pdf)
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import path from "node:path";

const require = createRequire(
  new URL("../../executors/playwright-runner/", import.meta.url),
);
const { chromium } = require("playwright");

const here = path.dirname(fileURLToPath(import.meta.url));
const htmlPath = path.join(here, "final-report.html").replace(/\\/g, "/");
const outPath = path.join(here, "SpecProbe-实验报告.pdf");

const browser = await chromium.launch();
const page = await browser.newPage();
await page.goto(`file:///${htmlPath}`);
await page.waitForTimeout(600);
await page.pdf({
  path: outPath,
  format: "A4",
  printBackground: true,
  margin: { top: "18mm", bottom: "16mm", left: "16mm", right: "16mm" },
  displayHeaderFooter: true,
  headerTemplate: `<div style="width:100%;font-size:8px;color:#888;text-align:center;font-family:'Microsoft YaHei',sans-serif;">SpecProbe 实验报告 · Rust 课程大作业</div>`,
  footerTemplate: `<div style="width:100%;font-size:8px;color:#888;text-align:center;font-family:'Microsoft YaHei',sans-serif;">第 <span class="pageNumber"></span> 页 / 共 <span class="totalPages"></span> 页</div>`,
});
await browser.close();
console.log("PDF saved:", outPath);
