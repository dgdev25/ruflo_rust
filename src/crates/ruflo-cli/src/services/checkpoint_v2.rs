//! Auto-split from services.rs
use super::*;

    use super::*;

    /// Create a checkpoint by copying the current RVF store to a snapshot.
    pub fn checkpoint(store_path: &str, label: &str) -> Result<Value, String> {
        let ckpt_dir = root().join(".claude-flow/checkpoints");
        let _ = fs::create_dir_all(&ckpt_dir);
        // #2: sanitize label — reject path separators + ..
        let safe_label: String = label.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
        let ckpt_path = ckpt_dir.join(format!("{safe_label}-{}.rvf", now_ms()));
        let src = std::path::Path::new(store_path);
        if src.exists() {
            fs::copy(src, &ckpt_path).map_err(|e| e.to_string())?;
        }
        let entry = json!({
            "label": label, "store": store_path,
            "checkpoint": ckpt_path.display().to_string(),
            "createdAt": now_ms(),
        });
        let mut state = read_state("checkpoints");
        ensure_arr(&mut state, "checkpoints").push(entry.clone());
        write_state("checkpoints", &state);
        Ok(entry)
    }

    /// Rollback to a checkpoint by restoring the snapshot.
    pub fn rollback(label: &str, store_path: &str) -> Result<Value, String> {
        let state = read_state("checkpoints");
        let ckpt = state["checkpoints"].as_array()
            .and_then(|arr| arr.iter().rev().find(|c| c["label"].as_str() == Some(label)))
            .cloned()
            .ok_or_else(|| format!("checkpoint '{label}' not found"))?;
        let ckpt_path = ckpt["checkpoint"].as_str().ok_or("missing path")?;
        let src = std::path::Path::new(ckpt_path);
        if src.exists() {
            fs::copy(src, store_path).map_err(|e| e.to_string())?;
        }
        Ok(json!({"rolled": label, "restored": store_path, "at": now_ms()}))
    }

    /// Conditional rollback: only restore if a quality check fails.
    pub fn rollback_on_fail(label: &str, store_path: &str, quality: f64, threshold: f64) -> Result<Value, String> {
        if quality >= threshold {
            return Ok(json!({"action": "keep", "quality": quality, "threshold": threshold}));
        }
        rollback(label, store_path)
    }
