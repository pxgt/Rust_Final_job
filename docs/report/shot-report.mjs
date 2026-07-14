// 报告配图辅助脚本:用本仓库 executors/playwright-runner 的 Playwright 截取 HTML 页面首屏。
// 用法: node docs/report/shot-report.mjs <path-with-forward-slashes> <out.png>
import { createRequire } from "node:module";

const require = createRequire(
  new URL("../../executors/playwright-runner/", import.meta.url),
);
const { chromium } = require("playwright");

const [, , target, out] = process.argv;
const url = target.startsWith("file://") ? target : `file:///${target}`;
const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1280, height: 1400 } });
await page.goto(url);
await page.waitForTimeout(500);
await page.screenshot({ path: out, fullPage: false });
await browser.close();
console.log("saved", out);
