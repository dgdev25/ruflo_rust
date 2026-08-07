use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use ruflo_storage::{MigrationPlan, PersistencePort};
use ruflo_types::RufloError;

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
    assert!(outcome.backup_path.starts_with(project.root()));
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
