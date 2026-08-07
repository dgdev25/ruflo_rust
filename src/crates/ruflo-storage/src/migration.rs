use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use ruflo_types::RufloError;

use crate::port::set_owner_only_permissions;

static UNIQUE_COUNTER: AtomicU64 = AtomicU64::new(1);

type Transform = dyn Fn(&[u8]) -> Result<Vec<u8>, RufloError> + Send + Sync;
type Validator = dyn Fn(&[u8]) -> Result<(), RufloError> + Send + Sync;

pub struct MigrationPlan {
    transform: Box<Transform>,
    validate: Box<Validator>,
}

impl MigrationPlan {
    pub fn new(
        transform: impl Fn(&[u8]) -> Result<Vec<u8>, RufloError> + Send + Sync + 'static,
        validate: impl Fn(&[u8]) -> Result<(), RufloError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            transform: Box::new(transform),
            validate: Box::new(validate),
        }
    }

    pub fn append(bytes: &'static [u8]) -> Self {
        Self::new(
            move |input| {
                let mut output = input.to_vec();
                output.extend_from_slice(bytes);
                Ok(output)
            },
            move |input| {
                if input.ends_with(bytes) {
                    Ok(())
                } else {
                    Err(RufloError::MigrationFailed {
                        message: "postcondition validation failed".to_string(),
                    })
                }
            },
        )
    }

    pub fn failing() -> Self {
        Self::new(
            |_input| {
                Err(RufloError::MigrationFailed {
                    message: "fixture migration failed".to_string(),
                })
            },
            |_input| Ok(()),
        )
    }

    pub(crate) fn transform(&self, input: &[u8]) -> Result<Vec<u8>, RufloError> {
        (self.transform)(input)
    }

    pub(crate) fn validate(&self, input: &[u8]) -> Result<(), RufloError> {
        (self.validate)(input)
    }
}

#[derive(Debug, Clone)]
pub struct MigrationMetadata {
    pub database_path: PathBuf,
    pub lock_path: PathBuf,
    pub marker_path: PathBuf,
    pub backup_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct RecoveryMetadata {
    pub database_path: PathBuf,
    pub backup_path: PathBuf,
    pub marker_path: PathBuf,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct MigrationOutcome {
    pub database_path: PathBuf,
    pub backup_path: PathBuf,
}

#[derive(Debug)]
pub struct MigrationSession {
    project_root: PathBuf,
    database_path: PathBuf,
    lock_path: PathBuf,
    marker_path: PathBuf,
    backup_path: Option<PathBuf>,
    finished: bool,
}

impl MigrationSession {
    pub(crate) fn new(project_root: PathBuf, database_path: PathBuf, lock_path: PathBuf) -> Self {
        let marker_path = project_root.join(".ruflo-storage.migration");
        Self {
            project_root,
            database_path,
            lock_path,
            marker_path,
            backup_path: None,
            finished: false,
        }
    }

    pub fn metadata(&self) -> MigrationMetadata {
        MigrationMetadata {
            database_path: self.database_path.clone(),
            lock_path: self.lock_path.clone(),
            marker_path: self.marker_path.clone(),
            backup_path: self.backup_path.clone(),
        }
    }

    pub fn backup(&mut self) -> Result<PathBuf, RufloError> {
        if let Some(path) = &self.backup_path {
            return Ok(path.clone());
        }

        let backup_path = sibling_path(&self.database_path, "backup");
        fs::copy(&self.database_path, &backup_path).map_err(map_io("storage.backup.copy"))?;
        set_owner_only_permissions(&backup_path)?;
        self.write_marker("backup_created")?;
        self.backup_path = Some(backup_path.clone());
        Ok(backup_path)
    }

    pub fn commit(mut self, plan: &MigrationPlan) -> Result<MigrationOutcome, RufloError> {
        let backup_path = self.backup()?;
        self.write_marker("migrating")?;

        let original = fs::read(&self.database_path).map_err(map_io("storage.migration.read"))?;
        let staged = match plan.transform(&original) {
            Ok(staged) => staged,
            Err(error) => return Err(self.rollback_from_error(error)),
        };
        if let Err(error) = plan.validate(&staged) {
            return Err(self.rollback_from_error(error));
        }

        let staged_path = sibling_path(&self.database_path, "staged");
        write_owner_only(&staged_path, &staged)?;

        match fs::rename(&staged_path, &self.database_path) {
            Ok(()) => {}
            Err(error) => {
                let _ = fs::remove_file(&staged_path);
                return Err(self.rollback_from_message(format!(
                    "commit failed while replacing persisted state: {error}"
                )));
            }
        }

        let committed = match fs::read(&self.database_path) {
            Ok(bytes) => bytes,
            Err(error) => {
                return Err(self.rollback_from_message(format!(
                    "postcondition validation could not read migrated state: {error}"
                )));
            }
        };

        if let Err(error) = plan.validate(&committed) {
            return Err(self.rollback_from_message(format!(
                "postcondition validation failed after commit: {error}"
            )));
        }

        let _ = fs::remove_file(&self.marker_path);
        let _ = fs::remove_file(&self.lock_path);
        self.finished = true;

        Ok(MigrationOutcome {
            database_path: self.database_path.clone(),
            backup_path,
        })
    }

    pub fn rollback(self, reason: impl Into<String>) -> Result<RecoveryMetadata, RufloError> {
        self.rollback_with_reason(reason.into())
    }

    fn rollback_with_reason(mut self, reason: String) -> Result<RecoveryMetadata, RufloError> {
        let backup_path = self
            .backup_path
            .clone()
            .ok_or_else(|| RufloError::MigrationFailed {
                message: "rollback requested before backup existed".to_string(),
            })?;

        let restore_path = sibling_path(&self.database_path, "restore");
        fs::copy(&backup_path, &restore_path).map_err(map_io("storage.rollback.restore_copy"))?;
        set_owner_only_permissions(&restore_path)?;
        fs::rename(&restore_path, &self.database_path)
            .map_err(map_io("storage.rollback.restore"))?;
        self.write_marker("rolled_back")?;
        append_recovery_reason(&self.marker_path, &reason)?;
        let _ = fs::remove_file(&self.lock_path);
        self.finished = true;

        Ok(RecoveryMetadata {
            database_path: self.database_path.clone(),
            backup_path,
            marker_path: self.marker_path.clone(),
            reason,
        })
    }

    fn write_marker(&self, status: &str) -> Result<(), RufloError> {
        let backup = self
            .backup_path
            .as_ref()
            .map(|path| safe_project_relative(&self.project_root, path))
            .transpose()?
            .unwrap_or_else(|| "-".to_string());
        let database = safe_project_relative(&self.project_root, &self.database_path)?;
        let lock = safe_project_relative(&self.project_root, &self.lock_path)?;
        let contents =
            format!("status={status}\ndatabase={database}\nlock={lock}\nbackup={backup}\n");
        write_owner_only(&self.marker_path, contents.as_bytes())
    }

    fn rollback_from_error(&self, error: RufloError) -> RufloError {
        let reason = match &error {
            RufloError::MigrationFailed { message } => message.clone(),
            _ => error.to_string(),
        };
        self.rollback_from_message(reason)
    }

    fn rollback_from_message(&self, reason: String) -> RufloError {
        let session = self.clone_for_recovery();
        match session.rollback_with_reason(reason.clone()) {
            Ok(_) => RufloError::MigrationFailed { message: reason },
            Err(rollback_error) => rollback_error,
        }
    }

    fn clone_for_recovery(&self) -> Self {
        Self {
            project_root: self.project_root.clone(),
            database_path: self.database_path.clone(),
            lock_path: self.lock_path.clone(),
            marker_path: self.marker_path.clone(),
            backup_path: self.backup_path.clone(),
            finished: self.finished,
        }
    }
}

impl Drop for MigrationSession {
    fn drop(&mut self) {
        if self.finished {
            return;
        }

        let _ = fs::remove_file(&self.lock_path);
    }
}

fn write_owner_only(path: &Path, bytes: &[u8]) -> Result<(), RufloError> {
    fs::write(path, bytes).map_err(map_io("storage.write"))?;
    set_owner_only_permissions(path)
}

fn append_recovery_reason(path: &Path, reason: &str) -> Result<(), RufloError> {
    let mut file = OpenOptions::new()
        .append(true)
        .open(path)
        .map_err(map_io("storage.recovery.open"))?;
    writeln!(file, "reason={reason}").map_err(map_io("storage.recovery.write"))?;
    Ok(())
}

fn safe_project_relative(project_root: &Path, path: &Path) -> Result<String, RufloError> {
    path.strip_prefix(project_root)
        .map(|relative| relative.display().to_string())
        .map_err(|_| RufloError::MigrationFailed {
            message: "migration marker escaped the configured project root".to_string(),
        })
}

fn sibling_path(path: &Path, suffix: &str) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("state");
    let unique = unique_suffix();
    path.with_file_name(format!("{file_name}.{suffix}.{unique}"))
}

fn unique_suffix() -> String {
    let sequence = UNIQUE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{timestamp}-{sequence}")
}

fn map_io(code: &'static str) -> impl Fn(std::io::Error) -> RufloError {
    move |error| RufloError::MigrationFailed {
        message: format!("{code}: {error}"),
    }
}
