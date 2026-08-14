//! Auto-split from services.rs
use super::*;

    use super::*;

    /// WAL-safe backup: copies .db + .db-wal + .db-shm together so the backup
    /// captures committed-but-uncheckpointed transactions. Naive fs::copy of
    /// just the .db file produces a corrupt, inconsistent snapshot.
    pub fn create(src: &Path) -> Result<PathBuf, String> {
        let backup_dir = root().join(".claude-flow/backups");
        fs::create_dir_all(&backup_dir).map_err(|e| e.to_string())?;
        let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("memory");
        let backup_path = backup_dir.join(format!("{stem}-{}.db", now_ms()));
        fs::copy(src, &backup_path).map_err(|e| e.to_string())?;
        // Copy the WAL + shared-memory sidecars if they exist.
        for suffix in &["-wal", "-shm"] {
            let sidecar = src.with_extension(format!("db{suffix}"));
            // Try with the common .db-wal naming.
            let wal_path = src.with_file_name(format!("{}.db{suffix}", stem));
            let src_side = if sidecar.exists() { sidecar } else { wal_path };
            if src_side.exists() {
                let dest_side = backup_path.with_file_name(format!(
                    "{}.db{suffix}", backup_path.file_stem().and_then(|s| s.to_str()).unwrap_or("backup")
                ));
                let _ = fs::copy(&src_side, &dest_side);
            }
        }
        let mut state = read_state("memory-backups");
        ensure_arr(&mut state, "backups").push(json!({
            "path": backup_path.display().to_string(),
            "source": src.display().to_string(),
            "walSafe": true,
            "createdAt": now_ms(),
        }));
        write_state("memory-backups", &state);
        Ok(backup_path)
    }

    pub fn list() -> Vec<Value> {
        read_state("memory-backups")["backups"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }
