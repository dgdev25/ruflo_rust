use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use ruflo_storage::{MemoryStoreInput, MigrationPlan, PersistencePort, SqliteMemoryStore};
use ruflo_types::RufloError;
use rusqlite::{params, Connection};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);

#[test]
fn successful_migration_commits_atomically_and_cleans_marker() {
    let project = TestProject::new("success");
    let port = PersistencePort::open(project.root(), project.database_path()).unwrap();
    let outcome = port
        .begin_migration()
        .unwrap()
        .commit(&MigrationPlan::append(b"\nmigrated=true\n"))
        .unwrap();

    let contents = project.read_database();
    assert!(contents.ends_with(b"\nmigrated=true\n"));
    assert_eq!(
        outcome.database_path,
        project.database_path().canonicalize().unwrap()
    );
    assert!(outcome
        .backup_path
        .starts_with(project.root().canonicalize().unwrap()));
    assert_eq!(
        project.read_fixture(),
        project.read_backup(&outcome.backup_path)
    );
    assert!(!project.marker_path().exists());
    assert!(!project.lock_path().exists());
}

#[test]
fn migration_lock_is_project_scoped() {
    let project = TestProject::new("lock-conflict");
    let port = PersistencePort::open(project.root(), project.database_path()).unwrap();
    let session = port.begin_migration().unwrap();

    let error = port.begin_migration().unwrap_err();
    assert!(matches!(error, RufloError::LockConflict));
    assert!(project.lock_path().exists());

    drop(session);
    assert!(!project.lock_path().exists());
}

#[test]
fn failed_migration_preserves_original_and_creates_owner_only_backup() {
    let project = TestProject::new("failing");
    let port = PersistencePort::open(project.root(), project.database_path()).unwrap();

    let error = port
        .begin_migration()
        .unwrap()
        .commit(&MigrationPlan::failing())
        .unwrap_err();
    assert!(matches!(error, RufloError::MigrationFailed { .. }));

    assert_eq!(project.read_database(), project.read_fixture());
    let backup_path = project.only_backup_path();
    assert_eq!(project.read_backup(&backup_path), project.read_fixture());
    assert_backup_has_owner_only_permissions(&backup_path);
    let marker = fs::read_to_string(project.marker_path()).unwrap();
    assert!(marker.contains("status=backup_created") || marker.contains("status=rolled_back"));
    assert!(marker.contains("reason=fixture migration failed"));
    assert!(!project.lock_path().exists());
}

#[test]
fn explicit_rollback_restores_original_and_records_recovery_metadata() {
    let project = TestProject::new("rollback");
    let port = PersistencePort::open(project.root(), project.database_path()).unwrap();
    let mut session = port.begin_migration().unwrap();
    let backup_path = session.backup().unwrap();

    fs::write(project.database_path(), b"corrupted\n").unwrap();
    let recovery = session.rollback("operator requested rollback").unwrap();

    assert_eq!(project.read_database(), project.read_fixture());
    assert_eq!(recovery.backup_path, backup_path);
    assert!(recovery.marker_path.exists());
    let marker = fs::read_to_string(recovery.marker_path).unwrap();
    assert!(marker.contains("status=rolled_back"));
    assert!(marker.contains("reason=operator requested rollback"));
    assert!(!project.lock_path().exists());
}

#[test]
fn open_rejects_paths_outside_the_project_root() {
    let project = TestProject::new("outside-root");
    let outside_path = project.outside_database_path();
    let error = PersistencePort::open(project.root(), &outside_path).unwrap_err();

    assert!(matches!(
        error,
        RufloError::InvalidInput { code, .. } if code == "storage.project_root.escape"
    ));
}

#[test]
fn populated_memory_entries_survive_atomic_migration_and_reopen() {
    let project = TestProject::new("populated-node-compatible");
    let database = project.root().join(".swarm/memory.db");
    let store = SqliteMemoryStore::open(project.root(), &database).unwrap();
    store.store(&MemoryStoreInput {
        key: "migration-contract".into(),
        namespace: "patterns".into(),
        content: "preserve populated Node-compatible memory entries".into(),
        memory_type: "semantic".into(),
        tags_json: Some(r#"["parity","migration"]"#.into()),
        provenance_type: "user_claim".into(),
        upsert: true,
    }).unwrap();
    drop(store);

    let port = PersistencePort::open(project.root(), &database).unwrap();
    port.begin_migration().unwrap().commit(&MigrationPlan::new(
        |bytes| Ok(bytes.to_vec()),
        |bytes| if bytes.starts_with(b"SQLite format 3\0") { Ok(()) } else { Err(RufloError::MigrationFailed { message: "not a SQLite database".into() }) },
    )).unwrap();

    let reopened = SqliteMemoryStore::open(project.root(), &database).unwrap();
    let entry = reopened.retrieve("patterns", "migration-contract").unwrap().unwrap();
    assert_eq!(entry.content, "preserve populated Node-compatible memory entries");
    assert_eq!(entry.memory_type, "semantic");
    assert_eq!(entry.provenance_type, "user_claim");
}

#[test]
fn populated_node_v3_memory_fields_survive_atomic_migration() {
    // Source contract: v3/@claude-flow/cli/src/memory/memory-initializer.ts,
    // MEMORY_SCHEMA_V3. sql.js serializes this standard SQLite schema, so this
    // fixture exercises fields not written by the native store itself.
    let project = TestProject::new("populated-node-v3");
    let database = project.root().join(".swarm/memory.db");
    fs::create_dir_all(database.parent().unwrap()).unwrap();
    let connection = Connection::open(&database).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE memory_entries (
                id TEXT PRIMARY KEY,
                key TEXT NOT NULL,
                namespace TEXT DEFAULT 'default',
                content TEXT NOT NULL,
                type TEXT DEFAULT 'semantic',
                embedding TEXT,
                embedding_model TEXT DEFAULT 'local',
                embedding_dimensions INTEGER,
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
            );",
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO memory_entries (
                id, key, namespace, content, type, embedding, embedding_model,
                embedding_dimensions, tags, metadata, owner_id, provenance_type,
                created_at, updated_at, expires_at, last_accessed_at, access_count, status
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            params![
                "node-v3-memory-1",
                "release-parity",
                "patterns",
                "preserve a populated sql.js memory record",
                "semantic",
                "[0.25,-0.5,0.75]",
                "Xenova/bge-base-en-v1.5",
                768_i64,
                r#"[\"release\",\"parity\"]"#,
                r#"{\"source\":\"node-v3\",\"confidence\":0.9}"#,
                "agent-42",
                "agent_output",
                1_700_000_000_000_i64,
                1_700_000_001_000_i64,
                1_800_000_000_000_i64,
                1_700_000_002_000_i64,
                7_i64,
                "active",
            ],
        )
        .unwrap();
    drop(connection);

    let port = PersistencePort::open(project.root(), &database).unwrap();
    port.begin_migration()
        .unwrap()
        .commit(&MigrationPlan::new(
            |bytes| Ok(bytes.to_vec()),
            |bytes| {
                if bytes.starts_with(b"SQLite format 3\0") {
                    Ok(())
                } else {
                    Err(RufloError::MigrationFailed {
                        message: "not a SQLite database".into(),
                    })
                }
            },
        ))
        .unwrap();

    let reopened = SqliteMemoryStore::open(project.root(), &database).unwrap();
    let entry = reopened.retrieve("patterns", "release-parity").unwrap().unwrap();
    assert_eq!(entry.id, "node-v3-memory-1");
    assert_eq!(entry.provenance_type, "agent_output");
    drop(reopened);

    let connection = Connection::open(&database).unwrap();
    let row = connection
        .query_row(
            "SELECT embedding, embedding_model, embedding_dimensions, tags, metadata, owner_id,
                    expires_at, last_accessed_at, access_count, status
             FROM memory_entries WHERE id = 'node-v3-memory-1'",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?, row.get::<_, i64>(7)?, row.get::<_, i64>(8)?, row.get::<_, String>(9)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(row.0, "[0.25,-0.5,0.75]");
    assert_eq!(row.1, "Xenova/bge-base-en-v1.5");
    assert_eq!(row.2, 768);
    assert_eq!(row.3, r#"[\"release\",\"parity\"]"#);
    assert_eq!(row.4, r#"{\"source\":\"node-v3\",\"confidence\":0.9}"#);
    assert_eq!(row.5, "agent-42");
    assert_eq!(row.6, 1_800_000_000_000);
    assert_eq!(row.7, 1_700_000_002_000);
    assert_eq!(row.8, 7);
    assert_eq!(row.9, "active");
}

struct TestProject {
    root: PathBuf,
    database_path: PathBuf,
}

impl TestProject {
    fn new(label: &str) -> Self {
        let root = unique_temp_dir(label);
        fs::create_dir_all(&root).unwrap();
        let database_path = root.join("legacy-empty.db");
        fs::copy("tests/fixtures/persistence/legacy-empty.db", &database_path).unwrap();

        let outside_root = root.with_file_name(format!(
            "{}-outside",
            root.file_name().unwrap().to_string_lossy()
        ));
        fs::create_dir_all(&outside_root).unwrap();
        fs::copy(
            "tests/fixtures/persistence/legacy-empty.db",
            outside_root.join("legacy-empty.db"),
        )
        .unwrap();

        Self {
            root,
            database_path,
        }
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn database_path(&self) -> &Path {
        &self.database_path
    }

    fn marker_path(&self) -> PathBuf {
        self.root.join(".ruflo-storage.migration")
    }

    fn lock_path(&self) -> PathBuf {
        self.root.join(".ruflo-storage.lock")
    }

    fn outside_database_path(&self) -> PathBuf {
        self.root
            .with_file_name(format!(
                "{}-outside",
                self.root.file_name().unwrap().to_string_lossy()
            ))
            .join("legacy-empty.db")
    }

    fn only_backup_path(&self) -> PathBuf {
        let mut backups = fs::read_dir(&self.root)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name.starts_with("legacy-empty.db.backup."))
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();
        backups.sort();
        assert_eq!(backups.len(), 1);
        backups.remove(0)
    }

    fn read_fixture(&self) -> Vec<u8> {
        fs::read("tests/fixtures/persistence/legacy-empty.db").unwrap()
    }

    fn read_database(&self) -> Vec<u8> {
        fs::read(&self.database_path).unwrap()
    }

    fn read_backup(&self, path: &Path) -> Vec<u8> {
        fs::read(path).unwrap()
    }
}

impl Drop for TestProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
        let _ = fs::remove_dir_all(self.root.with_file_name(format!(
            "{}-outside",
            self.root.file_name().unwrap().to_string_lossy()
        )));
    }
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("ruflo-storage-{label}-{stamp}-{sequence}"))
}

#[cfg(unix)]
fn assert_backup_has_owner_only_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let mode = fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

#[cfg(not(unix))]
fn assert_backup_has_owner_only_permissions(_path: &Path) {}
