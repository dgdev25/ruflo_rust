use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use ruflo_types::RufloError;

use crate::migration::MigrationSession;

#[derive(Debug, Clone)]
pub struct PersistencePort {
    project_root: PathBuf,
    database_path: PathBuf,
}

impl PersistencePort {
    pub fn open(
        project_root: impl AsRef<Path>,
        database_path: impl AsRef<Path>,
    ) -> Result<Self, RufloError> {
        let project_root = canonicalize_directory(project_root.as_ref())?;
        let database_path = canonicalize_file(database_path.as_ref())?;

        if !database_path.starts_with(&project_root) {
            return Err(RufloError::invalid_input(
                "storage.project_root.escape",
                "database path must stay inside the configured project root",
            ));
        }

        Ok(Self {
            project_root,
            database_path,
        })
    }

    pub fn begin_migration(&self) -> Result<MigrationSession, RufloError> {
        let lock_path = self.project_root.join(".ruflo-storage.lock");
        acquire_project_lock(&lock_path)?;
        Ok(MigrationSession::new(
            self.project_root.clone(),
            self.database_path.clone(),
            lock_path,
        ))
    }
}

fn canonicalize_directory(path: &Path) -> Result<PathBuf, RufloError> {
    let canonical = path.canonicalize().map_err(|error| {
        RufloError::invalid_input(
            "storage.project_root.invalid",
            format!("project root is unavailable: {error}"),
        )
    })?;
    let metadata = fs::metadata(&canonical).map_err(|error| {
        RufloError::invalid_input(
            "storage.project_root.invalid",
            format!("project root metadata is unavailable: {error}"),
        )
    })?;
    if !metadata.is_dir() {
        return Err(RufloError::invalid_input(
            "storage.project_root.invalid",
            "project root must be a directory",
        ));
    }
    Ok(canonical)
}

fn canonicalize_file(path: &Path) -> Result<PathBuf, RufloError> {
    let canonical = path.canonicalize().map_err(|error| {
        RufloError::invalid_input(
            "storage.database.invalid",
            format!("database path is unavailable: {error}"),
        )
    })?;
    let metadata = fs::metadata(&canonical).map_err(|error| {
        RufloError::invalid_input(
            "storage.database.invalid",
            format!("database metadata is unavailable: {error}"),
        )
    })?;
    if !metadata.is_file() {
        return Err(RufloError::invalid_input(
            "storage.database.invalid",
            "database path must be a regular file",
        ));
    }
    Ok(canonical)
}

fn acquire_project_lock(lock_path: &Path) -> Result<(), RufloError> {
    // Stale-lock recovery: if the lock file is older than 30s, take it over.
    match open_lock_file(lock_path) {
        Ok(file) => {
            drop(file);
            set_owner_only_permissions(lock_path)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            // Check if stale — older than 30s belongs to a crashed process.
            if let Ok(meta) = std::fs::metadata(lock_path) {
                if let Ok(mtime) = meta.modified() {
                    let age = std::time::SystemTime::now()
                        .duration_since(mtime)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    if age > 30 {
                        let _ = std::fs::remove_file(lock_path);
                        return acquire_project_lock(lock_path);
                    }
                }
            }
            Err(RufloError::LockConflict)
        }
        Err(error) => Err(RufloError::MigrationFailed {
            message: format!("storage.lock.acquire: {error}"),
        }),
    }
}

#[cfg(unix)]
fn open_lock_file(lock_path: &Path) -> Result<File, std::io::Error> {
    use std::os::unix::fs::OpenOptionsExt;

    OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(lock_path)
}

#[cfg(not(unix))]
fn open_lock_file(lock_path: &Path) -> Result<File, std::io::Error> {
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(lock_path)
}

pub(crate) fn set_owner_only_permissions(path: &Path) -> Result<(), RufloError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let permissions = fs::Permissions::from_mode(0o600);
        fs::set_permissions(path, permissions).map_err(|error| RufloError::MigrationFailed {
            message: format!("storage.permissions: {error}"),
        })?;
    }

    #[cfg(not(unix))]
    {
        let metadata = fs::metadata(path).map_err(|error| RufloError::MigrationFailed {
            message: format!("storage.permissions.metadata: {error}"),
        })?;
        let mut permissions = metadata.permissions();
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions).map_err(|error| RufloError::MigrationFailed {
            message: format!("storage.permissions: {error}"),
        })?;
    }

    Ok(())
}
