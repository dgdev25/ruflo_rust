use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ruflo_types::RufloError;
use rusqlite::{params, Connection, OptionalExtension};

/// Minimal, SQLite-compatible projection of Ruflo's public `memory_entries`
/// contract. Semantic embedding data remains owned by the RVF AgentDB
/// adapter; this store provides durable exact reads, listing, and the legacy
/// keyword fallback used when semantic search is unavailable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryEntry {
    pub id: String,
    pub key: String,
    pub namespace: String,
    pub content: String,
    pub memory_type: String,
    pub provenance_type: String,
    pub semantic_id: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryStoreInput {
    pub key: String,
    pub namespace: String,
    pub content: String,
    pub memory_type: String,
    pub tags_json: Option<String>,
    pub provenance_type: String,
    pub upsert: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryStats {
    pub total_entries: u64,
    pub entries_with_vectors: u64,
    pub total_content_bytes: u64,
}

pub struct SqliteMemoryStore {
    connection: Connection,
    database_path: PathBuf,
}

impl SqliteMemoryStore {
    pub fn open_from_current_dir() -> Result<Self, RufloError> {
        let root = env::current_dir().map_err(|error| {
            RufloError::invalid_input(
                "memory.project_root",
                format!("failed to resolve cwd: {error}"),
            )
        })?;
        let database_path = env::var_os("CLAUDE_FLOW_DB_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join(".swarm").join("memory.db"));
        Self::open(root, database_path)
    }

    pub fn open(
        project_root: impl AsRef<Path>,
        database_path: impl AsRef<Path>,
    ) -> Result<Self, RufloError> {
        let project_root = project_root.as_ref().canonicalize().map_err(|error| {
            RufloError::invalid_input(
                "memory.project_root",
                format!("project root is unavailable: {error}"),
            )
        })?;
        let requested = database_path.as_ref();
        let database_path = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            project_root.join(requested)
        };
        let parent = database_path.parent().ok_or_else(|| {
            RufloError::invalid_input(
                "memory.database_path",
                "memory database must have a parent directory",
            )
        })?;
        fs::create_dir_all(parent).map_err(|error| {
            RufloError::invalid_input(
                "memory.database_path",
                format!("failed to create memory directory: {error}"),
            )
        })?;
        let canonical_parent = parent.canonicalize().map_err(|error| {
            RufloError::invalid_input(
                "memory.database_path",
                format!("memory directory is unavailable: {error}"),
            )
        })?;
        if !canonical_parent.starts_with(&project_root) {
            return Err(RufloError::invalid_input(
                "memory.database_path.escape",
                "memory database path must stay inside the project root",
            ));
        }
        let database_path = canonical_parent.join(database_path.file_name().ok_or_else(|| {
            RufloError::invalid_input("memory.database_path", "memory database needs a filename")
        })?);
        let connection = Connection::open(&database_path).map_err(map_sqlite("memory.open"))?;
        connection
            .execute_batch(MEMORY_SCHEMA)
            .map_err(map_sqlite("memory.schema"))?;
        ensure_semantic_id_column(&connection)?;
        Ok(Self {
            connection,
            database_path,
        })
    }

    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    pub fn store(&self, input: &MemoryStoreInput) -> Result<MemoryEntry, RufloError> {
        validate_input(input)?;
        let existing = self.find(&input.namespace, &input.key)?;
        if existing.is_some() && !input.upsert {
            return Err(RufloError::invalid_input(
                "memory.key.exists",
                format!(
                    "memory key `{}` already exists in namespace `{}`",
                    input.key, input.namespace
                ),
            ));
        }
        let id = existing
            .map(|entry| entry.id)
            .unwrap_or_else(|| memory_id(&input.namespace, &input.key));
        let now = now_ms();
        self.connection
            .execute(
                "INSERT INTO memory_entries (id, key, namespace, content, type, tags, provenance_type, created_at, updated_at, status) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, 'active') \
                 ON CONFLICT(namespace, key) DO UPDATE SET \
                   content=excluded.content, type=excluded.type, tags=excluded.tags, provenance_type=excluded.provenance_type, \
                   updated_at=excluded.updated_at, status='active'",
                params![
                    id,
                    input.key,
                    input.namespace,
                    input.content,
                    input.memory_type,
                    input.tags_json,
                    input.provenance_type,
                    now,
                ],
            )
            .map_err(map_sqlite("memory.store"))?;
        self.find(&input.namespace, &input.key)?
            .ok_or_else(|| RufloError::UpstreamAdapter {
                message: "memory store completed without a readable entry".to_string(),
            })
    }

    pub fn retrieve(&self, namespace: &str, key: &str) -> Result<Option<MemoryEntry>, RufloError> {
        self.find(namespace, key)
    }

    pub fn search_keyword(
        &self,
        namespace: Option<&str>,
        query: &str,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>, RufloError> {
        if query.trim().is_empty() {
            return Err(RufloError::invalid_input(
                "memory.query",
                "query must not be empty",
            ));
        }
        let pattern = format!("%{}%", query.to_lowercase());
        let limit = i64::try_from(limit.max(1)).unwrap_or(i64::MAX);
        let mut entries = Vec::new();
        if let Some(namespace) = namespace {
            let mut statement = self.connection.prepare(
                "SELECT id, key, namespace, content, type, provenance_type, semantic_id FROM memory_entries \
                 WHERE status = 'active' AND namespace = ?1 AND lower(content) LIKE ?2 \
                 ORDER BY updated_at DESC, id ASC LIMIT ?3",
            ).map_err(map_sqlite("memory.search"))?;
            let rows = statement
                .query_map(params![namespace, pattern, limit], row_to_entry)
                .map_err(map_sqlite("memory.search"))?;
            for row in rows {
                entries.push(row.map_err(map_sqlite("memory.search"))?);
            }
        } else {
            let mut statement = self.connection.prepare(
                "SELECT id, key, namespace, content, type, provenance_type, semantic_id FROM memory_entries \
                 WHERE status = 'active' AND lower(content) LIKE ?1 \
                 ORDER BY updated_at DESC, id ASC LIMIT ?2",
            ).map_err(map_sqlite("memory.search"))?;
            let rows = statement
                .query_map(params![pattern, limit], row_to_entry)
                .map_err(map_sqlite("memory.search"))?;
            for row in rows {
                entries.push(row.map_err(map_sqlite("memory.search"))?);
            }
        }
        Ok(entries)
    }

    /// Bind an entry to its existing AgentDB/RVF numeric vector identifier.
    /// The metadata stays in SQLite; vector bytes and HNSW layout stay owned
    /// by the upstream RVF adapter.
    pub fn set_semantic_id(
        &self,
        namespace: &str,
        key: &str,
        semantic_id: u64,
    ) -> Result<(), RufloError> {
        let semantic_id = i64::try_from(semantic_id).map_err(|_| {
            RufloError::invalid_input("memory.semantic_id", "semantic ID exceeds SQLite range")
        })?;
        let updated = self.connection.execute(
            "UPDATE memory_entries SET semantic_id = ?1 WHERE namespace = ?2 AND key = ?3 AND status = 'active'",
            params![semantic_id, namespace, key],
        ).map_err(map_sqlite("memory.semantic_id"))?;
        if updated == 0 {
            return Err(RufloError::invalid_input(
                "memory.not_found",
                "cannot bind a vector to a missing memory entry",
            ));
        }
        Ok(())
    }

    pub fn retrieve_semantic_id(
        &self,
        semantic_id: u64,
    ) -> Result<Option<MemoryEntry>, RufloError> {
        let semantic_id = i64::try_from(semantic_id).map_err(|_| {
            RufloError::invalid_input("memory.semantic_id", "semantic ID exceeds SQLite range")
        })?;
        self.connection.query_row(
            "SELECT id, key, namespace, content, type, provenance_type, semantic_id FROM memory_entries \
             WHERE semantic_id = ?1 AND status = 'active'",
            params![semantic_id],
            row_to_entry,
        ).optional().map_err(map_sqlite("memory.retrieve_semantic"))
    }

    /// Semantic k-NN search via the RVF HNSW store. The query vector is
    /// produced by the caller (CLI/MCP embeds the query string); this method
    /// opens the RVF store sibling to the SQLite db, runs k-NN, and joins the
    /// returned RVF ids back to memory_entries via semantic_id.
    ///
    /// Returns `(entry, similarity)` pairs ordered by descending similarity.
    /// Falls back to an empty result (not an error) when no RVF store exists
    /// yet — the caller can retry with `search_keyword`.
    pub fn search_semantic(
        &self,
        query_vec: &[f32],
        limit: usize,
        dimension: u16,
    ) -> Result<Vec<(MemoryEntry, f32)>, RufloError> {
        let rvf_path = self.database_path
            .with_file_name("memory.rvf");
        if !rvf_path.exists() {
            return Ok(Vec::new());
        }
        let config = crate::rvf_adapter::AgentDbFixtureConfig::new(dimension);
        let store = crate::rvf_adapter::RvfPersistencePort::open_agentdb(&rvf_path, config)?;
        let matches = store.search_agentdb(query_vec, limit)?;
        drop(store);
        if matches.is_empty() {
            return Ok(Vec::new());
        }
        // Join semantic_id → SQLite row, preserving similarity order.
        let mut out = Vec::with_capacity(matches.len());
        for m in matches {
            if let Some(entry) = self.retrieve_semantic_id(m.id)? {
                let similarity = (1.0 - m.distance).clamp(-1.0, 1.0);
                out.push((entry, similarity));
            }
        }
        Ok(out)
    }

    /// Ingest a vector into the RVF store and bind it to the given entry via
    /// semantic_id. Called by the CLI/MCP layer after computing an embedding.
    pub fn ingest_semantic(
        &self,
        namespace: &str,
        key: &str,
        vector: &[f32],
        dimension: u16,
    ) -> Result<u64, RufloError> {
        let rvf_path = self.database_path.with_file_name("memory.rvf");
        let config = crate::rvf_adapter::AgentDbFixtureConfig::new(dimension);
        // Open or create the RVF store.
        let mut store = if rvf_path.exists() {
            crate::rvf_adapter::RvfPersistencePort::open_agentdb(&rvf_path, config)?
        } else {
            if let Some(parent) = rvf_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            crate::rvf_adapter::RvfPersistencePort::create_agentdb(&rvf_path, config)?
        };
        // Next id = current vector count + 1.
        let status = store.status();
        let next_id = status.total_vectors + 1;
        let record = crate::rvf_adapter::AgentDbVectorRecord {
            id: next_id,
            vector: vector.to_vec(),
        };
        store.ingest_agentdb(&[record])?;
        let _ = store.close();
        // Bind to the SQLite entry.
        self.set_semantic_id(namespace, key, next_id)?;
        Ok(next_id)
    }

    /// Enumerate active memory entries for compatibility views such as
    /// `memory list` and Codex dual-mode status. This deliberately remains a
    /// SQLite projection; semantic ordering belongs to the RVF adapter wave.
    pub fn list(
        &self,
        namespace: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>, RufloError> {
        let limit = i64::try_from(limit.max(1)).unwrap_or(i64::MAX);
        let mut entries = Vec::new();
        if let Some(namespace) = namespace {
            let mut statement = self
                .connection
                .prepare(
                    "SELECT id, key, namespace, content, type, provenance_type, semantic_id FROM memory_entries \
                     WHERE status = 'active' AND namespace = ?1 \
                     ORDER BY updated_at DESC, id ASC LIMIT ?2",
                )
                .map_err(map_sqlite("memory.list"))?;
            let rows = statement
                .query_map(params![namespace, limit], row_to_entry)
                .map_err(map_sqlite("memory.list"))?;
            for row in rows {
                entries.push(row.map_err(map_sqlite("memory.list"))?);
            }
        } else {
            let mut statement = self
                .connection
                .prepare(
                    "SELECT id, key, namespace, content, type, provenance_type, semantic_id FROM memory_entries \
                     WHERE status = 'active' ORDER BY updated_at DESC, id ASC LIMIT ?1",
                )
                .map_err(map_sqlite("memory.list"))?;
            let rows = statement
                .query_map(params![limit], row_to_entry)
                .map_err(map_sqlite("memory.list"))?;
            for row in rows {
                entries.push(row.map_err(map_sqlite("memory.list"))?);
            }
        }
        Ok(entries)
    }

    /// Soft-delete a memory row. The record remains auditable, while all
    /// public reads and listings exclude it. Its vector may remain in the
    /// upstream RVF index until that index's owner compacts it; callers must
    /// not hand-edit RVF segments to remove it.
    pub fn delete(&self, namespace: &str, key: &str) -> Result<Option<MemoryEntry>, RufloError> {
        let existing = self.find(namespace, key)?;
        if existing.is_none() {
            return Ok(None);
        }
        self.connection
            .execute(
                "UPDATE memory_entries SET status = 'deleted', updated_at = ?1 WHERE namespace = ?2 AND key = ?3 AND status = 'active'",
                params![now_ms(), namespace, key],
            )
            .map_err(map_sqlite("memory.delete"))?;
        Ok(existing)
    }

    pub fn stats(&self) -> Result<MemoryStats, RufloError> {
        self.connection
            .query_row(
                "SELECT COUNT(*), COUNT(semantic_id), COALESCE(SUM(length(content)), 0) \
                 FROM memory_entries WHERE status = 'active'",
                [],
                |row| {
                    Ok(MemoryStats {
                        total_entries: row.get::<_, i64>(0)? as u64,
                        entries_with_vectors: row.get::<_, i64>(1)? as u64,
                        total_content_bytes: row.get::<_, i64>(2)? as u64,
                    })
                },
            )
            .map_err(map_sqlite("memory.stats"))
    }

    pub fn count_namespace(&self, namespace: &str) -> Result<u64, RufloError> {
        self.connection
            .query_row(
                "SELECT COUNT(*) FROM memory_entries WHERE namespace = ?1 AND status = 'active'",
                params![namespace],
                |row| row.get::<_, i64>(0).map(|count| count as u64),
            )
            .map_err(map_sqlite("memory.count_namespace"))
    }

    /// Permanently removes the metadata projection for one namespace. Vector
    /// segments remain under the upstream RVF adapter's lifecycle; results
    /// referencing purged metadata are ignored by the semantic facade.
    pub fn purge_namespace(&self, namespace: &str) -> Result<u64, RufloError> {
        if namespace.trim().is_empty() {
            return Err(RufloError::invalid_input(
                "memory.namespace",
                "namespace must not be empty",
            ));
        }
        let deleted = self
            .connection
            .execute(
                "DELETE FROM memory_entries WHERE namespace = ?1",
                params![namespace],
            )
            .map_err(map_sqlite("memory.purge"))?;
        Ok(deleted as u64)
    }

    fn find(&self, namespace: &str, key: &str) -> Result<Option<MemoryEntry>, RufloError> {
        self.connection
            .query_row(
                "SELECT id, key, namespace, content, type, provenance_type, semantic_id FROM memory_entries \
                 WHERE namespace = ?1 AND key = ?2 AND status = 'active'",
                params![namespace, key],
                row_to_entry,
            )
            .optional()
            .map_err(map_sqlite("memory.retrieve"))
    }
}

const MEMORY_SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
CREATE TABLE IF NOT EXISTS memory_entries (
  id TEXT PRIMARY KEY,
  key TEXT NOT NULL,
  namespace TEXT DEFAULT 'default',
  content TEXT NOT NULL,
  type TEXT DEFAULT 'semantic',
  embedding TEXT,
  embedding_model TEXT DEFAULT 'local',
  embedding_dimensions INTEGER,
  semantic_id INTEGER UNIQUE,
  tags TEXT,
  metadata TEXT,
  owner_id TEXT,
  provenance_type TEXT DEFAULT 'unknown',
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  expires_at INTEGER,
  last_accessed_at INTEGER,
  access_count INTEGER DEFAULT 0,
  status TEXT DEFAULT 'active',
  UNIQUE(namespace, key)
);
CREATE INDEX IF NOT EXISTS idx_memory_namespace ON memory_entries(namespace);
CREATE INDEX IF NOT EXISTS idx_memory_key ON memory_entries(key);
CREATE INDEX IF NOT EXISTS idx_memory_status ON memory_entries(status);
"#;

fn ensure_semantic_id_column(connection: &Connection) -> Result<(), RufloError> {
    let mut statement = connection
        .prepare("PRAGMA table_info(memory_entries)")
        .map_err(map_sqlite("memory.schema"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(map_sqlite("memory.schema"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_sqlite("memory.schema"))?;
    if !columns.iter().any(|column| column == "semantic_id") {
        connection
            .execute(
                "ALTER TABLE memory_entries ADD COLUMN semantic_id INTEGER",
                [],
            )
            .map_err(map_sqlite("memory.schema"))?;
    }
    connection
        .execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_memory_semantic_id \
             ON memory_entries(semantic_id) WHERE semantic_id IS NOT NULL",
            [],
        )
        .map_err(map_sqlite("memory.schema"))?;
    Ok(())
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryEntry> {
    Ok(MemoryEntry {
        id: row.get(0)?,
        key: row.get(1)?,
        namespace: row.get(2)?,
        content: row.get(3)?,
        memory_type: row.get(4)?,
        provenance_type: row.get(5)?,
        semantic_id: row.get::<_, Option<i64>>(6)?.map(|value| value as u64),
    })
}

fn validate_input(input: &MemoryStoreInput) -> Result<(), RufloError> {
    for (name, value) in [
        ("key", &input.key),
        ("namespace", &input.namespace),
        ("content", &input.content),
    ] {
        if value.trim().is_empty() {
            return Err(RufloError::invalid_input(
                "memory.input",
                format!("{name} must not be empty"),
            ));
        }
    }
    Ok(())
}

fn memory_id(namespace: &str, key: &str) -> String {
    let mut encoded = String::with_capacity((namespace.len() + key.len()) * 2 + 5);
    encoded.push_str("mem-");
    for byte in namespace
        .bytes()
        .chain(std::iter::once(0))
        .chain(key.bytes())
    {
        use std::fmt::Write;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

fn now_ms() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(i64::MAX)
}

fn map_sqlite(operation: &'static str) -> impl FnOnce(rusqlite::Error) -> RufloError {
    move |error| RufloError::UpstreamAdapter {
        message: format!("{operation}: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(key: &str, content: &str) -> MemoryStoreInput {
        MemoryStoreInput {
            key: key.into(),
            namespace: "test".into(),
            content: content.into(),
            memory_type: "semantic".into(),
            tags_json: None,
            provenance_type: "unknown".into(),
            upsert: true,
        }
    }

    #[test]
    fn delete_hides_an_entry_and_stats_only_count_active_rows() {
        let project = tempfile::tempdir().unwrap();
        let store = SqliteMemoryStore::open(project.path(), ".swarm/memory.db").unwrap();
        store.store(&input("a", "alpha")).unwrap();
        store.store(&input("b", "bravo")).unwrap();
        assert_eq!(store.stats().unwrap().total_entries, 2);
        assert_eq!(store.delete("test", "a").unwrap().unwrap().key, "a");
        assert!(store.retrieve("test", "a").unwrap().is_none());
        assert!(store.delete("test", "a").unwrap().is_none());
        let stats = store.stats().unwrap();
        assert_eq!(stats.total_entries, 1);
        assert_eq!(stats.total_content_bytes, 5);
    }

    #[test]
    fn purge_is_explicitly_namespace_scoped() {
        let project = tempfile::tempdir().unwrap();
        let store = SqliteMemoryStore::open(project.path(), ".swarm/memory.db").unwrap();
        store.store(&input("a", "alpha")).unwrap();
        store
            .store(&MemoryStoreInput {
                namespace: "other".into(),
                ..input("b", "bravo")
            })
            .unwrap();
        assert_eq!(store.count_namespace("test").unwrap(), 1);
        assert_eq!(store.purge_namespace("test").unwrap(), 1);
        assert!(store.list(Some("test"), 10).unwrap().is_empty());
        assert_eq!(store.count_namespace("other").unwrap(), 1);
        assert!(store.purge_namespace(" ").is_err());
    }
}

#[cfg(test)]
mod semantic_tests {
    use super::*;

    fn tmp_store() -> SqliteMemoryStore {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let db = root.join("memory.db");
        // Leak the tempdir so the db + sibling .rvf survive the test.
        std::mem::forget(dir);
        SqliteMemoryStore::open(&root, &db).unwrap()
    }

    #[test]
    fn ingest_then_semantic_search_returns_entry() {
        let store = tmp_store();
        let input = MemoryStoreInput {
            key: "k1".into(),
            namespace: "default".into(),
            content: "the quick brown fox".into(),
            memory_type: "semantic".into(),
            tags_json: None,
            provenance_type: "test".into(),
            upsert: true,
        };
        let entry = store.store(&input).unwrap();
        // Embed the content (simple deterministic vector).
        let vec: Vec<f32> = (0..8).map(|i| i as f32 / 8.0).collect();
        let sid = store.ingest_semantic(&entry.namespace, &entry.key, &vec, 8).unwrap();
        assert!(sid >= 1);

        // Query with the same vector → should find the entry.
        let results = store.search_semantic(&vec, 5, 8).unwrap();
        assert!(!results.is_empty(), "semantic search should return the ingested entry");
        let (found, _sim) = &results[0];
        assert_eq!(found.key, "k1");
    }

    #[test]
    fn semantic_search_empty_without_rvf_store() {
        let store = tmp_store();
        let q = vec![0.1f32; 8];
        // No ingest → no RVF store → empty (not error).
        let results = store.search_semantic(&q, 5, 8).unwrap();
        assert!(results.is_empty());
    }
}
