//! Single SQLite store for appliance agents, jobs, daemon, and spend.
//!
//! Project data lives beside memory in `.swarm/memory.db`. Global spend lives
//! in `$RUFLO_AI_BUDGET_DIR/ai-budget.db` so swarm, hive, and daemon share
//! one fail-closed ledger.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ruflo_types::RufloError;
use rusqlite::{params, Connection, OptionalExtension};

const PROJECT_SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS appliance_agents (
  id TEXT PRIMARY KEY,
  agent_type TEXT NOT NULL,
  status TEXT NOT NULL,
  role TEXT NOT NULL DEFAULT '',
  heartbeat_ms INTEGER NOT NULL DEFAULT 0,
  last_job TEXT,
  created_ms INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS appliance_jobs (
  id TEXT PRIMARY KEY,
  worker_type TEXT NOT NULL,
  payload TEXT NOT NULL,
  status TEXT NOT NULL,
  created_ms INTEGER NOT NULL,
  claimed_ms INTEGER,
  done_ms INTEGER
);
CREATE TABLE IF NOT EXISTS appliance_kv (
  k TEXT PRIMARY KEY,
  v TEXT NOT NULL,
  updated_ms INTEGER NOT NULL
);
";

const SPEND_SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS appliance_spend (
  permit_id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  at_ms INTEGER NOT NULL,
  released_ms INTEGER,
  pid INTEGER,
  worker_type TEXT,
  workspace TEXT
);
CREATE TABLE IF NOT EXISTS appliance_spend_meta (
  k TEXT PRIMARY KEY,
  v TEXT NOT NULL
);
";

const LIMIT_CONCURRENT: usize = 1;
const LIMIT_HOURLY: usize = 2;
const LIMIT_DAILY: usize = 12;
const HOUR_MS: u64 = 3_600_000;
const DAY_MS: u64 = 86_400_000;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn map_sql(code: &'static str) -> impl Fn(rusqlite::Error) -> RufloError {
    move |error| {
        RufloError::invalid_input(code, format!("{error}"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRow {
    pub id: String,
    pub agent_type: String,
    pub status: String,
    pub role: String,
    pub heartbeat_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRow {
    pub id: String,
    pub worker_type: String,
    pub payload: String,
    pub status: String,
}

pub struct ApplianceStore {
    conn: Connection,
    path: PathBuf,
}

impl ApplianceStore {
    pub fn open(project_root: &Path) -> Result<Self, RufloError> {
        let dir = project_root.join(".swarm");
        std::fs::create_dir_all(&dir).map_err(|e| {
            RufloError::invalid_input("appliance.store.dir", e.to_string())
        })?;
        let path = dir.join("memory.db");
        let conn = Connection::open(&path).map_err(map_sql("appliance.store.open"))?;
        conn.execute_batch(PROJECT_SCHEMA)
            .map_err(map_sql("appliance.store.schema"))?;
        Ok(Self { conn, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn upsert_agent(&self, row: &AgentRow) -> Result<(), RufloError> {
        self.conn
            .execute(
                "INSERT INTO appliance_agents (id, agent_type, status, role, heartbeat_ms, last_job, created_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6)
                 ON CONFLICT(id) DO UPDATE SET
                   agent_type=excluded.agent_type,
                   status=excluded.status,
                   role=excluded.role,
                   heartbeat_ms=excluded.heartbeat_ms",
                params![
                    row.id,
                    row.agent_type,
                    row.status,
                    row.role,
                    row.heartbeat_ms as i64,
                    now_ms() as i64
                ],
            )
            .map_err(map_sql("appliance.agent.upsert"))?;
        Ok(())
    }

    pub fn get_agent(&self, id: &str) -> Result<Option<AgentRow>, RufloError> {
        self.conn
            .query_row(
                "SELECT id, agent_type, status, role, heartbeat_ms FROM appliance_agents WHERE id = ?1",
                params![id],
                |r| {
                    Ok(AgentRow {
                        id: r.get(0)?,
                        agent_type: r.get(1)?,
                        status: r.get(2)?,
                        role: r.get(3)?,
                        heartbeat_ms: r.get::<_, i64>(4)? as u64,
                    })
                },
            )
            .optional()
            .map_err(map_sql("appliance.agent.get"))
    }

    pub fn clear_agents(&self) -> Result<(), RufloError> {
        self.conn
            .execute("DELETE FROM appliance_agents", [])
            .map_err(map_sql("appliance.agent.clear"))?;
        Ok(())
    }

    pub fn list_agents(&self) -> Result<Vec<AgentRow>, RufloError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, agent_type, status, role, heartbeat_ms FROM appliance_agents ORDER BY id",
            )
            .map_err(map_sql("appliance.agent.list"))?;
        let rows = stmt
            .query_map([], |r| {
                Ok(AgentRow {
                    id: r.get(0)?,
                    agent_type: r.get(1)?,
                    status: r.get(2)?,
                    role: r.get(3)?,
                    heartbeat_ms: r.get::<_, i64>(4)? as u64,
                })
            })
            .map_err(map_sql("appliance.agent.list"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(map_sql("appliance.agent.list"))
    }

    pub fn enqueue_job(&self, worker_type: &str, payload: &str) -> Result<String, RufloError> {
        let id = format!("job-{}", now_ms());
        self.conn
            .execute(
                "INSERT INTO appliance_jobs (id, worker_type, payload, status, created_ms)
                 VALUES (?1, ?2, ?3, 'queued', ?4)",
                params![id, worker_type, payload, now_ms() as i64],
            )
            .map_err(map_sql("appliance.job.enqueue"))?;
        Ok(id)
    }

    pub fn claim_job(&self) -> Result<Option<JobRow>, RufloError> {
        let job = self
            .conn
            .query_row(
                "SELECT id, worker_type, payload, status FROM appliance_jobs
                 WHERE status = 'queued' ORDER BY created_ms ASC LIMIT 1",
                [],
                |r| {
                    Ok(JobRow {
                        id: r.get(0)?,
                        worker_type: r.get(1)?,
                        payload: r.get(2)?,
                        status: r.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(map_sql("appliance.job.claim"))?;
        if let Some(job) = &job {
            self.conn
                .execute(
                    "UPDATE appliance_jobs SET status='running', claimed_ms=?1 WHERE id=?2",
                    params![now_ms() as i64, job.id],
                )
                .map_err(map_sql("appliance.job.claim"))?;
        }
        Ok(job)
    }

    pub fn finish_job(&self, id: &str, ok: bool) -> Result<(), RufloError> {
        let status = if ok { "done" } else { "failed" };
        self.conn
            .execute(
                "UPDATE appliance_jobs SET status=?1, done_ms=?2 WHERE id=?3",
                params![status, now_ms() as i64, id],
            )
            .map_err(map_sql("appliance.job.finish"))?;
        Ok(())
    }

    pub fn put_kv(&self, key: &str, value: &str) -> Result<(), RufloError> {
        self.conn
            .execute(
                "INSERT INTO appliance_kv (k, v, updated_ms) VALUES (?1, ?2, ?3)
                 ON CONFLICT(k) DO UPDATE SET v=excluded.v, updated_ms=excluded.updated_ms",
                params![key, value, now_ms() as i64],
            )
            .map_err(map_sql("appliance.kv.put"))?;
        Ok(())
    }

    pub fn get_kv(&self, key: &str) -> Result<Option<String>, RufloError> {
        self.conn
            .query_row(
                "SELECT v FROM appliance_kv WHERE k = ?1",
                params![key],
                |r| r.get(0),
            )
            .optional()
            .map_err(map_sql("appliance.kv.get"))
    }
}

pub struct SpendLedger {
    conn: Connection,
}

impl SpendLedger {
    pub fn open_default() -> Result<Self, RufloError> {
        let dir = std::env::var("RUFLO_AI_BUDGET_DIR")
            .map(PathBuf::from)
            .or_else(|_| std::env::var("RUFLO_STATE_DIR").map(PathBuf::from))
            .unwrap_or_else(|_| {
                std::env::var("HOME")
                    .map(|h| PathBuf::from(h).join(".claude-flow"))
                    .unwrap_or_else(|_| PathBuf::from(".claude-flow"))
            });
        Self::open(&dir)
    }

    pub fn open(dir: &Path) -> Result<Self, RufloError> {
        std::fs::create_dir_all(dir).map_err(|e| {
            RufloError::invalid_input("appliance.spend.dir", e.to_string())
        })?;
        let path = dir.join("ai-budget.db");
        let conn = Connection::open(&path).map_err(map_sql("appliance.spend.open"))?;
        conn.execute_batch(SPEND_SCHEMA)
            .map_err(map_sql("appliance.spend.schema"))?;
        Ok(Self { conn })
    }

    pub fn check(&self) -> Result<(), String> {
        if self.meta("paused") == Some("1".into()) {
            return Err(self
                .meta("pause_reason")
                .unwrap_or_else(|| "paused".into()));
        }
        let now = now_ms();
        let active = self.count_active().map_err(|e| e.to_string())?;
        if active >= LIMIT_CONCURRENT {
            return Err(format!("concurrent limit reached ({active}/{LIMIT_CONCURRENT})"));
        }
        let hourly = self
            .count_since(now.saturating_sub(HOUR_MS))
            .map_err(|e| e.to_string())?;
        if hourly >= LIMIT_HOURLY {
            return Err(format!("hourly launch limit reached ({hourly}/{LIMIT_HOURLY})"));
        }
        let daily = self
            .count_since(now.saturating_sub(DAY_MS))
            .map_err(|e| e.to_string())?;
        if daily >= LIMIT_DAILY {
            return Err(format!("daily launch limit reached ({daily}/{LIMIT_DAILY})"));
        }
        Ok(())
    }

    pub fn reserve(&self, kind: &str, worker_type: &str, workspace: &str) -> Result<String, String> {
        self.check()?;
        let permit = format!("p-{}-{}", now_ms(), std::process::id());
        self.conn
            .execute(
                "INSERT INTO appliance_spend (permit_id, kind, at_ms, pid, worker_type, workspace)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    permit,
                    kind,
                    now_ms() as i64,
                    std::process::id() as i64,
                    worker_type,
                    workspace
                ],
            )
            .map_err(|e| e.to_string())?;
        Ok(permit)
    }

    pub fn release(&self, permit: &str) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE appliance_spend SET released_ms=?1 WHERE permit_id=?2",
                params![now_ms() as i64, permit],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn pause(&self, reason: &str) -> Result<(), String> {
        self.set_meta("paused", "1")?;
        self.set_meta("pause_reason", reason)
    }

    pub fn resume(&self) -> Result<(), String> {
        self.set_meta("paused", "0")?;
        self.set_meta("pause_reason", "")
    }

    pub fn is_paused(&self) -> bool {
        self.meta("paused") == Some("1".into())
    }

    fn count_active(&self) -> Result<usize, RufloError> {
        let n: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM appliance_spend WHERE released_ms IS NULL",
                [],
                |r| r.get(0),
            )
            .map_err(map_sql("appliance.spend.active"))?;
        Ok(n as usize)
    }

    fn count_since(&self, since_ms: u64) -> Result<usize, RufloError> {
        let n: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM appliance_spend WHERE at_ms >= ?1",
                params![since_ms as i64],
                |r| r.get(0),
            )
            .map_err(map_sql("appliance.spend.window"))?;
        Ok(n as usize)
    }

    fn meta(&self, k: &str) -> Option<String> {
        self.conn
            .query_row(
                "SELECT v FROM appliance_spend_meta WHERE k = ?1",
                params![k],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten()
    }

    fn set_meta(&self, k: &str, v: &str) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO appliance_spend_meta (k, v) VALUES (?1, ?2)
                 ON CONFLICT(k) DO UPDATE SET v=excluded.v",
                params![k, v],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_and_job_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = ApplianceStore::open(dir.path()).unwrap();
        store
            .upsert_agent(&AgentRow {
                id: "coder-1".into(),
                agent_type: "coder".into(),
                status: "resident-idle".into(),
                role: "coder".into(),
                heartbeat_ms: 1,
            })
            .unwrap();
        let listed = store.list_agents().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].status, "resident-idle");
        store.enqueue_job("audit", "scan").unwrap();
        let job = store.claim_job().unwrap().unwrap();
        assert_eq!(job.worker_type, "audit");
        store.finish_job(&job.id, true).unwrap();
        assert!(store.claim_job().unwrap().is_none());
    }

    #[test]
    fn spend_fail_closed_on_pause_and_concurrent() {
        let dir = tempfile::tempdir().unwrap();
        let spend = SpendLedger::open(dir.path()).unwrap();
        spend.reserve("swarm", "claude", "/tmp").unwrap();
        assert!(spend.reserve("hive", "claude", "/tmp").is_err());
        spend.pause("test").unwrap();
        assert!(spend.check().is_err());
        spend.resume().unwrap();
    }
}
