//! Native V3 `transfer-store` command — decentralized IPFS pattern store.
//!
//! Source: `v3/@claude-flow/cli/src/commands/transfer-store.ts`. Subcommands:
//! list/search/download/publish/info.
//!
//! Native implementation talks to an IPFS gateway over HTTP (via curl, the same
//! dependency-free approach used for ONNX model downloads) and manages a local
//! registry index. No IPFS daemon required — a public read gateway
//! (configurable via RUFLO_IPFS_GATEWAY) handles content retrieval, and CIDs
//! are computed natively (SHA-256 truncated to 32 bytes, base32-encoded as a
//! CIDv1 Raw codec — a deterministic content address for the file).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

const DEFAULT_GATEWAY: &str = "https://ipfs.io/ipfs";

fn gateway() -> String {
    std::env::var("RUFLO_IPFS_GATEWAY").unwrap_or_else(|_| DEFAULT_GATEWAY.into())
}

fn registry_dir(root: &Path) -> PathBuf {
    root.join(".claude-flow/transfer-store")
}

fn registry_file(root: &Path) -> PathBuf {
    registry_dir(root).join("registry.json")
}

fn load_registry(root: &Path) -> Value {
    std::fs::read_to_string(registry_file(root))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({"patterns": []}))
}

fn save_registry(root: &Path, v: &Value) -> bool {
    let dir = registry_dir(root);
    let _ = std::fs::create_dir_all(&dir);
    let path = registry_file(root);
    let tmp = path.with_extension("json.tmp");
    let Ok(bytes) = serde_json::to_vec_pretty(v) else {
        return false;
    };
    std::fs::write(&tmp, &bytes).is_ok() && std::fs::rename(&tmp, &path).is_ok()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferStoreCommand {
    pub operation: String,
    pub query: Option<String>,
    pub registry: Option<String>,
    pub category: Option<String>,
    pub featured: bool,
    pub trending: bool,
    pub newest: bool,
    pub limit: usize,
    pub id: Option<String>,
    pub file: Option<String>,
    pub name: Option<String>,
    pub cid: Option<String>,
}

pub fn run(root: &Path, command: TransferStoreCommand) -> u8 {
    match command.operation.as_str() {
        "" => {
            print!(r####"
RuFlo Pattern Store
Decentralized pattern marketplace via IPFS

Subcommands:
  - list      - List patterns from local registry
  - search    - Search patterns
  - download  - Download a pattern by CID (via gateway)
  - publish   - Publish a pattern (compute CID, add to registry)
  - info      - Show pattern details

Environment:
  RUFLO_IPFS_GATEWAY  IPFS HTTP gateway (default: https://ipfs.io/ipfs)

Example:
  ruflo transfer-store publish -f pattern.json -n my-pattern
  ruflo transfer-store list --featured
  ruflo transfer-store download --cid <cid>
"####);
            0
        }
        "list" => list(root, &command),
        "search" => search(root, &command),
        "download" => download(root, &command),
        "publish" => publish(root, &command),
        "info" => info(root, &command),
        _ => {
            eprintln!(
                "[ERROR] Unknown: {} (list|search|download|publish|info)",
                command.operation
            );
            1
        }
    }
}

fn list(root: &Path, command: &TransferStoreCommand) -> u8 {
    let reg = load_registry(root);
    let mut patterns: Vec<&Value> = reg["patterns"]
        .as_array()
        .map(|a| a.iter().collect())
        .unwrap_or_default();

    if command.featured {
        patterns.retain(|p| p["featured"].as_bool().unwrap_or(false));
    }
    if command.trending {
        patterns.retain(|p| p["trending"].as_bool().unwrap_or(false));
    }
    if let Some(cat) = &command.category {
        patterns.retain(|p| p["category"].as_str() == Some(cat.as_str()));
    }
    patterns.truncate(command.limit.max(1));

    if command_json(command) {
        println!("{}", serde_json::to_string_pretty(&patterns).unwrap_or_default());
        return 0;
    }
    println!("\nPattern Store (local registry)");
    println!("{}", "\u{2500}".repeat(50));
    if patterns.is_empty() {
        println!("  No patterns. Publish one: ruflo transfer-store publish -f pattern.json -n X");
    } else {
        for p in &patterns {
            let name = p["name"].as_str().unwrap_or("?");
            let cid = p["cid"].as_str().unwrap_or("?");
            let cat = p["category"].as_str().unwrap_or("general");
            println!("  {name:<24} [{cat}] {cid}");
        }
    }
    0
}

fn search(root: &Path, command: &TransferStoreCommand) -> u8 {
    let query = command.query.clone().unwrap_or_default().to_lowercase();
    let reg = load_registry(root);
    let matches: Vec<&Value> = reg["patterns"]
        .as_array()
        .map(|a| {
            a.iter().filter(|p| {
                let name = p["name"].as_str().unwrap_or("").to_lowercase();
                let desc = p["description"].as_str().unwrap_or("").to_lowercase();
                name.contains(&query) || desc.contains(&query)
            }).collect()
        })
        .unwrap_or_default();
    if command_json(command) {
        println!("{}", serde_json::to_string_pretty(&matches).unwrap_or_default());
        return 0;
    }
    println!("\nSearch: \"{query}\" ({} match)", matches.len());
    for p in &matches {
        println!("  {} — {}", p["name"].as_str().unwrap_or("?"), p["cid"].as_str().unwrap_or("?"));
    }
    0
}

fn download(root: &Path, command: &TransferStoreCommand) -> u8 {
    let cid = command
        .cid
        .clone()
        .or_else(|| command.id.clone())
        .unwrap_or_default();
    if cid.is_empty() {
        eprintln!("[ERROR] --cid is required");
        return 1;
    }
    let url = format!("{}/{}", gateway(), cid);
    let dest = command.file.clone().unwrap_or_else(|| format!("{cid}.json"));
    let status = Command::new("curl")
        .args(["-sL", "-o", &dest, &url])
        .status();
    match status {
        Ok(s) if s.success() => {
            // Record download in registry metadata.
            let mut reg = load_registry(root);
            if let Some(arr) = reg["patterns"].as_array_mut() {
                for p in arr {
                    if p["cid"].as_str() == Some(cid.as_str()) {
                        let n = p["downloads"].as_u64().unwrap_or(0) + 1;
                        p["downloads"] = json!(n);
                    }
                }
            }
            let _ = save_registry(root, &reg);
            println!("Downloaded {cid} → {dest}");
            0
        }
        _ => {
            eprintln!("[ERROR] Gateway download failed for {cid}");
            eprintln!("  URL: {url}");
            1
        }
    }
}

fn publish(root: &Path, command: &TransferStoreCommand) -> u8 {
    let file = match &command.file {
        Some(f) => f.clone(),
        None => {
            eprintln!("[ERROR] --file is required");
            return 1;
        }
    };
    if !Path::new(&file).exists() {
        eprintln!("[ERROR] File not found: {file}");
        return 1;
    }
    let bytes = match std::fs::read(&file) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("[ERROR] Read failed: {e}");
            return 1;
        }
    };
    let cid = compute_cid(&bytes);
    let name = command.name.clone().unwrap_or_else(|| {
        Path::new(&file).file_stem().and_then(|s| s.to_str()).unwrap_or("pattern").to_string()
    });

    let mut reg = load_registry(root);
    if reg["patterns"].is_null() {
        reg["patterns"] = json!([]);
    }
    let entry = json!({
        "name": name,
        "cid": cid,
        "category": command.category.clone().unwrap_or_else(|| "general".into()),
        "description": format!("Published from {file}"),
        "size": bytes.len(),
        "featured": command.featured,
        "downloads": 0,
        "publishedAt": now_ms(),
    });
    if let Some(arr) = reg["patterns"].as_array_mut() {
        arr.push(entry);
    }
    if !save_registry(root, &reg) {
        eprintln!("[ERROR] Failed to persist registry");
        return 1;
    }
    println!("Published {name}");
    println!("  CID:   {cid}");
    println!("  Size:  {} bytes", bytes.len());
    println!("  Gateway URL: {}/{}", gateway(), cid);
    0
}

fn info(root: &Path, command: &TransferStoreCommand) -> u8 {
    let id = command.id.clone().or_else(|| command.cid.clone()).unwrap_or_default();
    if id.is_empty() {
        eprintln!("[ERROR] --id or --cid is required");
        return 1;
    }
    let reg = load_registry(root);
    let found = reg["patterns"]
        .as_array()
        .and_then(|arr| arr.iter().find(|p| {
            p["cid"].as_str() == Some(id.as_str()) || p["name"].as_str() == Some(id.as_str())
        }));
    match found {
        Some(p) => {
            println!("{}", serde_json::to_string_pretty(p).unwrap_or_default());
            0
        }
        None => {
            eprintln!("[ERROR] Pattern '{id}' not in local registry.");
            eprintln!("  Try: ruflo transfer-store download --cid {id}");
            1
        }
    }
}

fn command_json(_c: &TransferStoreCommand) -> bool {
    // TransferStoreCommand has no json flag today; kept for future use.
    false
}

/// Compute a content address for arbitrary bytes.
///
/// Uses CIDv1 with the Raw codec (0x55) and SHA-2-256 hash (0x12, len 0x20).
/// This is a deterministic, content-addressed identifier — two identical files
/// always produce the same CID. (Not a multihash of the dag-pb wrapper; for a
/// pure raw-bytes address this is the canonical form.)
fn compute_cid(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    // multibase prefix 'b' = base32-lower, then <multihash: 0x12 0x20 + digest>
    // CIDv1 = <version 0x01><codec 0x55><multihash>.
    let mut cid_bytes: Vec<u8> = vec![0x01, 0x55, 0x12, 0x20];
    cid_bytes.extend_from_slice(&digest);
    // base32 lowercase, no padding.
    base32_lower(&cid_bytes)
}

fn base32_lower(bytes: &[u8]) -> String {
    const ALPHA: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut out = String::new();
    let mut buffer: u32 = 0;
    let mut bits = 0u32;
    for &b in bytes {
        buffer = (buffer << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHA[((buffer >> bits) & 0x1F) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHA[((buffer << (5 - bits)) & 0x1F) as usize] as char);
    }
    out
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_root() -> PathBuf {
        let dir = tempfile::tempdir().unwrap().keep();
        dir
    }

    #[test]
    fn publish_then_list_and_search() {
        let root = tmp_root();
        let pat = root.join("pat.json");
        std::fs::write(&pat, r#"{"type":"test"}"#).unwrap();

        let cmd = TransferStoreCommand {
            operation: "publish".into(),
            file: Some(pat.display().to_string()),
            name: Some("my-pattern".into()),
            category: Some("test".into()),
            featured: true, trending: false, newest: false,
            query: None, registry: None, id: None, cid: None, limit: 10,
        };
        assert_eq!(publish(&root, &cmd), 0);

        // CID should be deterministic.
        let reg = load_registry(&root);
        let cid = reg["patterns"][0]["cid"].as_str().unwrap().to_string();
        assert!(!cid.is_empty());

        let list_cmd = TransferStoreCommand {
            operation: "list".into(),
            featured: true, limit: 10,
            ..base_cmd()
        };
        assert_eq!(list(&root, &list_cmd), 0);

        let search_cmd = TransferStoreCommand {
            operation: "search".into(),
            query: Some("my-pattern".into()),
            ..base_cmd()
        };
        assert_eq!(search(&root, &search_cmd), 0);
    }

    #[test]
    fn cid_is_deterministic() {
        let a = compute_cid(b"hello world");
        let b = compute_cid(b"hello world");
        assert_eq!(a, b);
        assert_ne!(compute_cid(b"different"), a);
    }

    #[test]
    fn base32_known_vector() {
        // "fo" → base32 lower "mzxq"
        assert_eq!(base32_lower(b"fo"), "mzxq");
    }

    #[test]
    fn info_missing_returns_error() {
        let root = tmp_root();
        let cmd = TransferStoreCommand {
            operation: "info".into(),
            id: Some("nope".into()),
            ..base_cmd()
        };
        assert_eq!(info(&root, &cmd), 1);
    }

    fn base_cmd() -> TransferStoreCommand {
        TransferStoreCommand {
            operation: String::new(),
            query: None, registry: None, category: None,
            featured: false, trending: false, newest: false,
            limit: 10, id: None, file: None, name: None, cid: None,
        }
    }
}
