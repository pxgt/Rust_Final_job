//! 运行归档与状态存储(ROADMAP 2.4)。
//!
//! 每次 `check`/`review` 生成一个 run:report.json 归档到
//! `.specprobe/runs/<run-id>/report.json`,索引与 Issue 状态写入 SQLite
//! (`.specprobe/specprobe.db`)。`runs list/show` 查询历史。
//! Issue 的 `approval` 列为 2.5 审批持久化预留。
//!
//! 存储是持久化层,与核心逻辑(review/check)解耦:核心只返回报告,
//! 由 CLI 层决定是否归档。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use serde::Serialize;
use thiserror::Error;

use crate::review::ReviewReport;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("failed to prepare storage at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("failed to serialize report: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// 打开的存储(SQLite 连接 + `.specprobe` 目录)。
pub struct Store {
    conn: Connection,
    base_dir: PathBuf,
}

/// 一次运行的索引摘要。
#[derive(Debug, Clone, Serialize)]
pub struct RunSummary {
    pub id: String,
    pub created_at_ms: i64,
    pub project_root: String,
    pub base_url: String,
    pub engine: String,
    pub executed: bool,
    pub requirements: usize,
    pub issues: usize,
    pub high: usize,
    pub report_path: String,
}

/// 打开(或初始化)`<base_dir>/specprobe.db` 并建表。`base_dir` 通常为 `.specprobe`。
pub fn open(base_dir: &Path) -> Result<Store, StoreError> {
    fs::create_dir_all(base_dir).map_err(|source| StoreError::Io {
        path: base_dir.to_path_buf(),
        source,
    })?;
    let conn = Connection::open(base_dir.join("specprobe.db"))?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS runs (
            id            TEXT PRIMARY KEY,
            created_at_ms INTEGER NOT NULL,
            project_root  TEXT NOT NULL,
            base_url      TEXT NOT NULL,
            engine        TEXT NOT NULL,
            executed      INTEGER NOT NULL,
            requirements  INTEGER NOT NULL,
            issues        INTEGER NOT NULL,
            high          INTEGER NOT NULL,
            report_path   TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS issues (
            run_id      TEXT NOT NULL,
            issue_id    TEXT NOT NULL,
            severity    TEXT NOT NULL,
            category    TEXT NOT NULL,
            title       TEXT NOT NULL,
            requirement TEXT,
            approval    TEXT NOT NULL,
            PRIMARY KEY (run_id, issue_id)
        );",
    )?;
    Ok(Store {
        conn,
        base_dir: base_dir.to_path_buf(),
    })
}

impl Store {
    /// 归档一次运行:写 report.json、插入 runs 与 issues。返回索引摘要。
    pub fn record_run(
        &mut self,
        project_root: &str,
        base_url: &str,
        executed: bool,
        report: &ReviewReport,
    ) -> Result<RunSummary, StoreError> {
        let created_at_ms = now_ms();
        let id = format!("run-{created_at_ms}");
        let run_dir = self.base_dir.join("runs").join(&id);
        fs::create_dir_all(&run_dir).map_err(|source| StoreError::Io {
            path: run_dir.clone(),
            source,
        })?;
        let report_path = run_dir.join("report.json");
        fs::write(&report_path, serde_json::to_string_pretty(report)?).map_err(|source| {
            StoreError::Io {
                path: report_path.clone(),
                source,
            }
        })?;
        let report_path_str = report_path.to_string_lossy().replace('\\', "/");

        let summary = RunSummary {
            id: id.clone(),
            created_at_ms,
            project_root: project_root.to_owned(),
            base_url: base_url.to_owned(),
            engine: report.requirement_report.engine.to_string(),
            executed,
            requirements: report.summary.requirements,
            issues: report.summary.issues,
            high: report.summary.high,
            report_path: report_path_str,
        };

        let transaction = self.conn.transaction()?;
        transaction.execute(
            "INSERT INTO runs
             (id, created_at_ms, project_root, base_url, engine, executed, requirements, issues, high, report_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                summary.id,
                summary.created_at_ms,
                summary.project_root,
                summary.base_url,
                summary.engine,
                summary.executed as i64,
                summary.requirements as i64,
                summary.issues as i64,
                summary.high as i64,
                summary.report_path,
            ],
        )?;
        for issue in &report.issues {
            transaction.execute(
                "INSERT INTO issues (run_id, issue_id, severity, category, title, requirement, approval)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    summary.id,
                    issue.id,
                    issue.severity.to_string(),
                    issue.category.to_string(),
                    issue.title,
                    issue.related_requirement,
                    issue.approval.to_string(),
                ],
            )?;
        }
        transaction.commit()?;
        Ok(summary)
    }

    /// 最近的运行(按时间倒序,最多 `limit` 条)。
    pub fn list_runs(&self, limit: usize) -> Result<Vec<RunSummary>, StoreError> {
        let mut statement = self.conn.prepare(
            "SELECT id, created_at_ms, project_root, base_url, engine, executed,
                    requirements, issues, high, report_path
             FROM runs ORDER BY created_at_ms DESC LIMIT ?1",
        )?;
        let rows = statement.query_map([limit as i64], row_to_summary)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// 按 id 查询一次运行;不存在返回 None。
    pub fn get_run(&self, id: &str) -> Result<Option<RunSummary>, StoreError> {
        let mut statement = self.conn.prepare(
            "SELECT id, created_at_ms, project_root, base_url, engine, executed,
                    requirements, issues, high, report_path
             FROM runs WHERE id = ?1",
        )?;
        let mut rows = statement.query_map([id], row_to_summary)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    /// 某次运行归档的 Issue(id、严重度、标题、审批状态)。
    pub fn run_issues(&self, run_id: &str) -> Result<Vec<StoredIssue>, StoreError> {
        let mut statement = self.conn.prepare(
            "SELECT issue_id, severity, category, title, requirement, approval
             FROM issues WHERE run_id = ?1 ORDER BY issue_id",
        )?;
        let rows = statement.query_map([run_id], |row| {
            Ok(StoredIssue {
                issue_id: row.get(0)?,
                severity: row.get(1)?,
                category: row.get(2)?,
                title: row.get(3)?,
                requirement: row.get(4)?,
                approval: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct StoredIssue {
    pub issue_id: String,
    pub severity: String,
    pub category: String,
    pub title: String,
    pub requirement: Option<String>,
    pub approval: String,
}

fn row_to_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<RunSummary> {
    Ok(RunSummary {
        id: row.get(0)?,
        created_at_ms: row.get(1)?,
        project_root: row.get(2)?,
        base_url: row.get(3)?,
        engine: row.get(4)?,
        executed: row.get::<_, i64>(5)? != 0,
        requirements: row.get::<_, i64>(6)? as usize,
        issues: row.get::<_, i64>(7)? as usize,
        high: row.get::<_, i64>(8)? as usize,
        report_path: row.get(9)?,
    })
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::open;
    use crate::review::{ReviewOptions, generate_review_report};
    use crate::testutil::temp_project;

    async fn sample_report(root: &std::path::Path) -> crate::review::ReviewReport {
        let requirements = root.join("PRD.md");
        fs::write(&requirements, "- 页面应该简单友好。").expect("write requirements");
        generate_review_report(
            &requirements,
            ReviewOptions {
                project_path: root.to_path_buf(),
                base_url: "http://127.0.0.1:3000".to_owned(),
                provider: Default::default(),
                cache_dir: None,
                execute: false,
                skip_launch: true,
                skip_browser: true,
                launch_timeout_secs: 1,
                browser_timeout_secs: 1,
            },
        )
        .await
        .expect("review succeeds")
    }

    #[tokio::test]
    async fn records_and_lists_runs_with_issues() {
        let root = temp_project("specprobe-store");
        let report = sample_report(&root).await;
        let mut store = open(&root.join(".specprobe")).expect("open store");

        let summary = store
            .record_run("proj", "http://127.0.0.1:3000", false, &report)
            .expect("record run");
        assert!(summary.issues >= 1);
        // report_path 用正斜杠;Windows 文件系统也接受,可直接检查存在性。
        assert!(PathBuf::from(&summary.report_path).exists());

        let runs = store.list_runs(10).expect("list runs");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].id, summary.id);

        let fetched = store.get_run(&summary.id).expect("get run");
        assert!(fetched.is_some());
        assert!(store.get_run("run-missing").expect("get missing").is_none());

        let issues = store.run_issues(&summary.id).expect("issues");
        assert_eq!(issues.len(), summary.issues);
        assert!(issues.iter().all(|issue| issue.approval == "pending"));

        drop(store); // 关闭 SQLite 连接,Windows 才能删除 db 文件
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[tokio::test]
    async fn list_orders_newest_first() {
        let root = temp_project("specprobe-store-order");
        let report = sample_report(&root).await;
        let mut store = open(&root.join(".specprobe")).expect("open store");

        let first = store
            .record_run("proj", "http://x", false, &report)
            .expect("first");
        // 保证时间戳单调递增,run id 不同。
        std::thread::sleep(std::time::Duration::from_millis(2));
        let second = store
            .record_run("proj", "http://x", false, &report)
            .expect("second");
        assert_ne!(first.id, second.id);

        let runs = store.list_runs(10).expect("list");
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].id, second.id, "newest first");

        drop(store); // 关闭 SQLite 连接,Windows 才能删除 db 文件
        fs::remove_dir_all(root).expect("cleanup");
    }
}
