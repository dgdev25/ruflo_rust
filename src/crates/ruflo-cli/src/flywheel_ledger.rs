//! Flywheel governed promotion — immutable signed receipts + append-only
//! hash-chained ledger + compare-and-swap promotion (ADR-322).
//!
//! ADR-322 §3: evaluation produces an immutable receipt; the promotion ledger
//! is append-only and hash-chained (each entry commits to the previous hash);
//! promotion is an atomic compare-and-swap on the current champion.
//!
//! Signatures: HMAC-SHA256 (sha2, already a dep) keyed by RUFLO_FLYWHEEL_KEY
//! (or a derived default for local-only ledgers). Ed25519 would need a new dep;
//! HMAC gives tamper-evidence under a shared secret, which is the ADR's intent
//! for the local ledger. The receipt also carries a SHA-256 content digest.

use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::{json, Value};

static LEDGER_LOCK: Mutex<()> = Mutex::new(());

fn ledger_path() -> PathBuf {
    let dir = std::env::var("RUFLO_FLYWHEEL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(".claude-flywheel")
        });
    let _ = fs::create_dir_all(&dir);
    dir.join("ledger.jsonl")
}

fn receipts_path() -> PathBuf {
    ledger_path().with_file_name("receipts.jsonl")
}

fn champion_path() -> PathBuf {
    ledger_path().with_file_name("champion.json")
}

/// Resolve the HMAC key. RUFLO_FLYWHEEL_KEY if set; else a deterministic
/// per-host default (NOT secure across machines — local-only ledgers only).
fn hmac_key() -> Vec<u8> {
    if let Ok(k) = std::env::var("RUFLO_FLYWHEEL_KEY") {
        return k.into_bytes();
    }
    let host = std::env::var("USER").unwrap_or_else(|_| "anon".into());
    format!("ruflo-flywheel-default-{host}").into_bytes()
}

fn hmac_sha256(key: &[u8], msg: &[u8]) -> Vec<u8> {
    const BLOCK: usize = 64;
    let mut k = if key.len() > BLOCK {
        Sha256::digest(key).to_vec()
    } else {
        key.to_vec()
    };
    k.resize(BLOCK, 0);
    let mut ipad = vec![0x36u8; BLOCK];
    let mut opad = vec![0x5cu8; BLOCK];
    for i in 0..BLOCK {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }
    let mut inner = Sha256::new();
    inner.update(&ipad);
    inner.update(msg);
    let inner_digest = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(&opad);
    outer.update(&inner_digest);
    outer.finalize().to_vec()
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Read the last entry's hash (the chain head). Empty ledger → genesis zero-hash.
fn chain_head() -> String {
    let path = ledger_path();
    if !path.exists() {
        return "0".repeat(64);
    }
    let raw = fs::read_to_string(&path).unwrap_or_default();
    raw.lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .last()
        .and_then(|v| v["hash"].as_str().map(String::from))
        .unwrap_or_else(|| "0".repeat(64))
}

/// Append an immutable receipt to the ledger with hash-chaining + HMAC.
/// Returns the receipt (id, prevHash, bodyHash, signature).
pub fn append_receipt(event: &str, payload: &Value) -> Value {
    let _g = LEDGER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let prev = chain_head();
    let id = format!("rcpt-{}-{}", now_ms(), nonce());
    let body = json!({"id": id, "event": event, "payload": payload, "prevHash": prev, "at": now_ms()});
    let body_str = serde_json::to_string(&body).unwrap_or_default();
    let body_hash = hex(&Sha256::digest(body_str.as_bytes()));
    let chain_msg = format!("{}{}", prev, body_hash);
    let sig = hex(&hmac_sha256(&hmac_key(), chain_msg.as_bytes()));
    let entry = json!({
        "id": id, "event": event, "payload": payload,
        "prevHash": prev, "bodyHash": body_hash, "signature": sig, "at": now_ms(),
        "hash": body_hash,
    });
    // Append to ledger (jsonl).
    use std::io::Write;
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(ledger_path()) {
        let _ = writeln!(f, "{}", serde_json::to_string(&entry).unwrap_or_default());
    }
    // Also record in receipts file.
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(receipts_path()) {
        let _ = writeln!(f, "{}", serde_json::to_string(&entry).unwrap_or_default());
    }
    entry
}

/// Verify the full chain: every entry's prevHash == prior entry's hash, and
/// every signature recomputes under the current key. Returns (entries, ok).
pub fn verify_ledger() -> (Vec<Value>, bool) {
    let raw = fs::read_to_string(ledger_path()).unwrap_or_default();
    let entries: Vec<Value> = raw.lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let mut prev = "0".repeat(64);
    let key = hmac_key();
    let mut ok = true;
    for e in &entries {
        if e["prevHash"].as_str() != Some(prev.as_str()) {
            ok = false;
            break;
        }
        let body_hash = e["bodyHash"].as_str().unwrap_or("");
        let chain_msg = format!("{}{}", prev, body_hash);
        let expected = hex(&hmac_sha256(&key, chain_msg.as_bytes()));
        if e["signature"].as_str() != Some(expected.as_str()) {
            ok = false;
            break;
        }
        prev = e["hash"].as_str().unwrap_or(&prev).to_string();
    }
    (entries, ok)
}

/// Compare-and-swap promotion: promote a candidate to champion only if the
/// current champion hash matches `expected_champion_hash`. Atomic under the
/// ledger lock. Returns Ok(new_champion) or Err(reason).
pub fn promote(candidate: &str, expected_champion_hash: &str) -> Result<Value, String> {
    let _g = LEDGER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let current = read_champion();
    let current_hash = current["hash"].as_str().unwrap_or("");
    if current_hash != expected_champion_hash {
        return Err(format!(
            "CAS mismatch: expected champion hash {expected_champion_hash} but current is {current_hash}"
        ));
    }
    let body = json!({"candidate": candidate, "promotedFrom": current_hash, "at": now_ms()});
    let body_str = serde_json::to_string(&body).unwrap_or_default();
    let hash = hex(&Sha256::digest(body_str.as_bytes()));
    let new_champ = json!({
        "candidate": candidate, "hash": hash,
        "promotedFrom": current_hash, "at": now_ms(),
    });
    let tmp = champion_path().with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(&new_champ).unwrap_or_default())
        .map_err(|e| e.to_string())?;
    fs::rename(&tmp, champion_path()).map_err(|e| e.to_string())?;
    // Record the promotion as an immutable receipt.
    drop(_g);
    append_receipt("promote", &new_champ);
    // Record the evolve-proof accept gate (V2 behavioral).
    let _ = crate::services::evolve_proof_v2::accept(candidate, 1.0, 0.5);
    // Record the flywheel transaction commit (V2 behavioral).
    let _ = crate::services::flywheel_tx_v2::commit_atomic("promote", new_champ.clone());
    Ok(new_champ)
}

pub fn read_champion() -> Value {
    fs::read_to_string(champion_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({"candidate": null, "hash": ""}))
}

pub fn list_receipts(limit: usize) -> Vec<Value> {
    let raw = fs::read_to_string(receipts_path()).unwrap_or_default();
    let mut all: Vec<Value> = raw.lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    all.reverse();
    all.truncate(limit.max(1));
    all
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn nonce() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::Mutex;
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn isolated() -> (tempfile::TempDir, std::sync::MutexGuard<'static, ()>) {
        let g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let d = tempfile::tempdir().unwrap();
        // SAFETY: ENV_LOCK serializes all flywheel tests; no other thread
        // reads/writes RUFLO_FLYWHEEL_DIR concurrently.
        env::set_var("RUFLO_FLYWHEEL_DIR", d.path());
        let _ = fs::remove_file(ledger_path());
        let _ = fs::remove_file(receipts_path());
        let _ = fs::remove_file(champion_path());
        (d, g)
    }

    #[test]
    fn chain_is_contiguous_and_verifies() {
        let (_d, _g) = isolated();
        append_receipt("eval", &json!({"score": 0.8}));
        append_receipt("eval", &json!({"score": 0.9}));
        let (entries, ok) = verify_ledger();
        assert!(ok, "ledger should verify");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["prevHash"].as_str(), Some("0".repeat(64).as_str()));
        assert_eq!(entries[1]["prevHash"], entries[0]["hash"]);
    }

    #[test]
    fn tamper_breaks_verification() {
        let (_d, _g) = isolated();
        append_receipt("eval", &json!({"score": 0.8}));
        let path = ledger_path();
        let raw = fs::read_to_string(&path).unwrap();
        let forged = raw.replace("\"signature\":\"", "\"signature\":\"00");
        fs::write(&path, forged).unwrap();
        let (_, ok) = verify_ledger();
        assert!(!ok, "forged signature should fail verification");
    }

    #[test]
    fn cas_promotion_atomic() {
        let (_d, _g) = isolated();
        let r = promote("cand-A", "");
        assert!(r.is_ok(), "first promote should succeed on empty champion");
        let champ = read_champion();
        let hash = champ["hash"].as_str().unwrap().to_string();
        assert!(promote("cand-B", &hash).is_ok());
        let r = promote("cand-C", &hash);
        assert!(r.is_err(), "stale CAS should fail");
    }
}
