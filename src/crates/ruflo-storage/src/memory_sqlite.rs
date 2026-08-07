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
