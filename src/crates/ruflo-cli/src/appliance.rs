//! Native V3 `appliance` command — RVFA appliance management.
//!
//! Source: `v3/@claude-flow/cli/src/commands/appliance.ts`. Subcommands:
//! build/inspect/verify/extract/run. The RVFA format requires
//! `../appliance/rvfa-format.js` + `../appliance/rvfa-builder.js` (not
//! available as Rust crates). The native build degrades gracefully with
//! file-existence checks.

use std::fs;
use std::path::{Component, Path, PathBuf};
use serde_json::json;

/// Cloud appliance profile packed into the standalone binary.
pub const CLOUD_PROFILE: &str = include_str!("../../../../config/appliance/cloud.yaml");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplianceCommand {
    pub operation: String,
    pub file: Option<String>,
    pub output: Option<String>,
    pub profile: Option<String>,
    pub arch: Option<String>,
    pub json: bool,
    pub quick: bool,
    pub target_dir: Option<String>,
}

pub fn run(_root: &Path, command: ApplianceCommand) -> u8 {
    match command.operation.as_str() {
        "" => {
            print!(r####"
Ruflo Appliance (RVFA)
Self-contained deployment format for the full Ruflo platform.

Subcommands:
  - build     - Build a self-contained ruflo.rvf appliance
  - inspect   - Show appliance header and section manifest
  - verify    - Verify appliance integrity and run capability tests
  - extract   - Extract all sections from an appliance
  - run       - Boot and run an RVFA appliance
  - sign      - Sign an appliance with Ed25519 for tamper detection
  - publish   - Publish an appliance to IPFS via Pinata
  - update    - Hot-patch a section in an appliance

Profiles:
  - cloud    - API-only, smallest footprint (~15 MB)
  - hybrid   - API + local fallback models (~500 MB)
  - offline  - Fully air-gapped with bundled models (~4 GB)

Use "ruflo appliance <subcommand> --help" for details.
"####);
            0
        }
        "build" => build(&command),
        "inspect" => inspect(&command),
        "verify" => verify(&command),
        "extract" => extract(&command),
        "run" => run_cmd(&command),
        _ => {
            eprintln!("[ERROR] Unknown appliance operation: {}", command.operation);
            eprintln!("  Valid: build, inspect, verify, extract, run");
            1
        }
    }
}

fn build(command: &ApplianceCommand) -> u8 {
    let profile = command.profile.as_deref().unwrap_or("cloud");
    let output = command.output.as_deref().unwrap_or("ruflo.rvf");
    let arch = command.arch.as_deref().unwrap_or("x86_64");
    // Native RVFA builder: create a JSON manifest with SHA-256 checksums of
    // project config files. No Node rvfa-builder module needed.
    use sha2::{Digest, Sha256};
    let manifest_files = [".claude-flow/config.yaml", ".claude/settings.json",
                          ".mcp.json", "CLAUDE.md", ".claude/CLAUDE.md"];
    let mut file_entries = Vec::new();
    for f in &manifest_files {
        if let Ok(content) = fs::read(f) {
            let hash = hex_sha256(&content);
            file_entries.push(json!({"path": f, "sha256": hash, "size": content.len()}));
        }
    }
    let mut host = json!(null);
    if let Ok(exe) = std::env::current_exe() {
        if let Ok(bytes) = fs::read(&exe) {
            host = json!({
                "path": exe.display().to_string(),
                "sha256": hex_sha256(&bytes),
                "size": bytes.len(),
            });
        }
    }
    let manifest = json!({
        "format": "rvfa", "version": 1, "profile": profile,
        "arch": arch, "files": file_entries,
        "host": host,
        "cloudProfileSha256": hex_sha256(CLOUD_PROFILE.as_bytes()),
        "standalone": true,
        "createdAt": now_ms(),
    });
    let manifest_str = serde_json::to_string_pretty(&manifest).unwrap_or_default();
    let checksum = hex_sha256(manifest_str.as_bytes());
    let rvfa = json!({"manifest": manifest, "checksum": checksum});
    let rvfa_bytes = serde_json::to_vec_pretty(&rvfa).unwrap_or_default();
    if fs::write(&output, &rvfa_bytes).is_err() {
        eprintln!("[ERROR] Failed to write RVFA: {output}");
        return 1;
    }
    println!("\nRVFA Appliance Built");
    println!("  Profile:  {profile}");
    println!("  Output:   {output} ({} bytes)", rvfa_bytes.len());
    println!("  Arch:     {arch}");
    println!("  Files:    {}", file_entries.len());
    println!("  Checksum: {checksum}");
    0
}

fn hex_sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let d = Sha256::digest(bytes);
    d.iter().map(|b| format!("{b:02x}")).collect()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn inspect(command: &ApplianceCommand) -> u8 {
    let Some(file) = &command.file else {
        eprintln!("[ERROR] --file is required");
        return 1;
    };
    let content = match fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => { eprintln!("[ERROR] File not found: {file}"); return 1; }
    };
    let rvfa: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => { eprintln!("[ERROR] Invalid RVFA format"); return 1; }
    };
    if command.json {
        println!("{}", serde_json::to_string_pretty(&rvfa).unwrap_or_default());
    } else {
        println!("\nRVFA Inspection");
        println!("  Format:   {}", rvfa["manifest"]["format"].as_str().unwrap_or("?"));
        println!("  Profile:  {}", rvfa["manifest"]["profile"].as_str().unwrap_or("?"));
        println!("  Checksum: {}", rvfa["checksum"].as_str().unwrap_or("?"));
        println!("  Files:    {}", rvfa["manifest"]["files"].as_array().map(|a| a.len()).unwrap_or(0));
    }
    0
}

fn verify(command: &ApplianceCommand) -> u8 {
    let Some(file) = &command.file else {
        eprintln!("[ERROR] --file is required");
        return 1;
    };
    if !Path::new(file).exists() {
        eprintln!("[ERROR] File not found: {file}");
        return 1;
    }
    let quick = command.quick;
    println!("\nVerifying RVFA Appliance");
    println!("  File: {file}");
    println!(
        "  Mode: {}",
        if quick {
            "quick (integrity only)"
        } else {
            "full"
        }
    );
    println!();
    // Native verify: recompute manifest checksum + verify each file hash.
    let content = match fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => { eprintln!("[ERROR] Cannot read: {file}"); return 1; }
    };
    let rvfa: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => { eprintln!("[ERROR] Invalid RVFA format"); return 1; }
    };
    let stored_checksum = rvfa["checksum"].as_str().unwrap_or("");
    let manifest_str = serde_json::to_string_pretty(&rvfa["manifest"]).unwrap_or_default();
    let computed = hex_sha256(manifest_str.as_bytes());
    if stored_checksum != computed {
        eprintln!("  ✗ Checksum mismatch: stored={stored_checksum} computed={computed}");
        return 1;
    }
    println!("  ✓ Checksum verified: {computed}");
    if let Some(expected) = rvfa["manifest"]["host"]["sha256"].as_str() {
        if let Some(host_path) = rvfa["manifest"]["host"]["path"].as_str() {
            match fs::read(host_path) {
                Ok(bytes) => {
                    let actual = hex_sha256(&bytes);
                    if actual != expected {
                        eprintln!("  ✗ Host hash mismatch: stored={expected} computed={actual}");
                        return 1;
                    }
                    println!("  ✓ Host hash verified");
                }
                Err(e) => {
                    eprintln!("  ✗ Host binary missing ({host_path}): {e}");
                    return 1;
                }
            }
        }
    }
    if quick {
        return 0;
    }
    let Some(files) = rvfa["manifest"]["files"].as_array() else {
        return 0;
    };
    let mut ok = 0usize;
    let mut failed = false;
    for f in files {
        let path = f["path"].as_str().unwrap_or("");
        if confined_rel(path).is_err() {
            eprintln!("  ✗ {path}: path is not confined");
            failed = true;
            continue;
        }
        let expected = f["sha256"].as_str().unwrap_or("");
        match fs::read(path) {
            Ok(content) => {
                let actual = hex_sha256(&content);
                if actual == expected {
                    ok += 1;
                } else {
                    eprintln!("  ✗ {path}: hash mismatch");
                    failed = true;
                }
            }
            Err(_) => {
                eprintln!("  ✗ {path}: missing");
                failed = true;
            }
        }
    }
    if failed {
        eprintln!("  ✗ {ok}/{} files verified", files.len());
        return 1;
    }
    println!("  ✓ {ok}/{} files verified", files.len());
    0
}

fn extract(command: &ApplianceCommand) -> u8 {
    let Some(file) = &command.file else {
        eprintln!("[ERROR] --file is required");
        return 1;
    };
    let content = match fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => { eprintln!("[ERROR] File not found: {file}"); return 1; }
    };
    let rvfa: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => { eprintln!("[ERROR] Invalid RVFA format"); return 1; }
    };
    let target = command.target_dir.as_deref().unwrap_or("./extracted");
    if fs::create_dir_all(target).is_err() {
        eprintln!("[ERROR] Cannot create target directory: {target}");
        return 1;
    }
    let mut extracted = 0;
    let mut rejected = 0;
    if let Some(files) = rvfa["manifest"]["files"].as_array() {
        for f in files {
            let path = f["path"].as_str().unwrap_or("");
            if let Err(reason) = confined_rel(path) {
                eprintln!("[ERROR] Rejected path '{path}': {reason}");
                rejected += 1;
                continue;
            }
            if !Path::new(path).exists() {
                continue;
            }
            let dest = PathBuf::from(target).join(path);
            if let Some(parent) = dest.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if fs::copy(path, &dest).is_ok() {
                extracted += 1;
            }
        }
    }
    println!("\nRVFA Extracted: {extracted} files to {target}");
    if rejected > 0 {
        eprintln!("[ERROR] {rejected} path(s) rejected");
        return 1;
    }
    0
}

/// Reject absolute paths and any `..` segment so extract/verify cannot leave
/// the intended tree.
pub(crate) fn confined_rel(path: &str) -> Result<&str, String> {
    if path.is_empty() {
        return Err("empty path".into());
    }
    let p = Path::new(path);
    if p.is_absolute() {
        return Err("absolute paths are not allowed".into());
    }
    for c in p.components() {
        match c {
            Component::Normal(_) | Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => {
                return Err("parent or root segments are not allowed".into());
            }
        }
    }
    Ok(path)
}

fn run_cmd(command: &ApplianceCommand) -> u8 {
    let Some(file) = &command.file else {
        eprintln!("[ERROR] --file is required");
        return 1;
    };
    if !Path::new(file).exists() {
        eprintln!("[ERROR] File not found: {file}");
        return 1;
    }
    println!("\nRunning RVFA Appliance");
    println!("  File: {file}");
    println!();
    let content = match fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("[ERROR] Cannot read: {file}");
            return 1;
        }
    };
    let rvfa: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => {
            eprintln!("[ERROR] Invalid RVFA format");
            return 1;
        }
    };
    let stored_checksum = rvfa["checksum"].as_str().unwrap_or("");
    let manifest_str = serde_json::to_string_pretty(&rvfa["manifest"]).unwrap_or_default();
    let computed = hex_sha256(manifest_str.as_bytes());
    if stored_checksum != computed {
        eprintln!("[ERROR] RVFA checksum mismatch. Refusing to run.");
        return 1;
    }
    let Some(host_path) = rvfa["manifest"]["host"]["path"].as_str() else {
        eprintln!("[ERROR] RVFA has no host binary path. Rebuild with `ruflo appliance build`.");
        return 1;
    };
    let expected = rvfa["manifest"]["host"]["sha256"].as_str().unwrap_or("");
    let bytes = match fs::read(host_path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("[ERROR] Cannot read host binary {host_path}: {e}");
            return 1;
        }
    };
    let actual = hex_sha256(&bytes);
    if !expected.is_empty() && actual != expected {
        eprintln!("[ERROR] Host binary hash mismatch. Refusing to run.");
        return 1;
    }
    let status = std::process::Command::new(host_path)
        .args(["daemon", "start", "--foreground", "--ttl", "0"])
        .status();
    match status {
        Ok(s) => s.code().unwrap_or(1) as u8,
        Err(e) => {
            eprintln!("[ERROR] Failed to exec host: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confined_rel_rejects_parent_and_absolute() {
        assert!(confined_rel("ok/file.json").is_ok());
        assert!(confined_rel("../secret").is_err());
        assert!(confined_rel("/etc/passwd").is_err());
        assert!(confined_rel("").is_err());
        assert!(confined_rel("a/../../b").is_err());
    }

    static CWD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn verify_fails_when_listed_file_hash_mismatches() {
        let _g = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("payload.txt"), b"hello").unwrap();
        let manifest = json!({
            "format": "rvfa",
            "files": [{"path": "payload.txt", "sha256": "00", "size": 5}],
        });
        let checksum = hex_sha256(serde_json::to_string_pretty(&manifest).unwrap().as_bytes());
        fs::write(
            dir.path().join("box.rvfa"),
            serde_json::to_vec_pretty(&json!({"manifest": manifest, "checksum": checksum})).unwrap(),
        )
        .unwrap();
        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let code = verify(&ApplianceCommand {
            operation: "verify".into(),
            file: Some("box.rvfa".into()),
            output: None,
            profile: None,
            arch: None,
            json: false,
            quick: false,
            target_dir: None,
        });
        let _ = std::env::set_current_dir(cwd);
        assert_eq!(code, 1);
    }

    #[test]
    fn extract_rejects_parent_path() {
        let dir = tempfile::tempdir().unwrap();
        let rvfa = json!({
            "manifest": {"files": [{"path": "../etc/passwd", "sha256": "00"}]},
            "checksum": "x",
        });
        let file = dir.path().join("box.rvfa");
        fs::write(&file, serde_json::to_vec_pretty(&rvfa).unwrap()).unwrap();
        let target = dir.path().join("out");
        let code = extract(&ApplianceCommand {
            operation: "extract".into(),
            file: Some(file.to_string_lossy().into_owned()),
            output: None,
            profile: None,
            arch: None,
            json: false,
            quick: false,
            target_dir: Some(target.to_string_lossy().into_owned()),
        });
        assert_eq!(code, 1);
        assert!(!target.join("etc/passwd").exists());
    }

    #[test]
    fn embedded_cloud_profile_is_standalone() {
        assert!(CLOUD_PROFILE.contains("profile: cloud"));
        assert!(CLOUD_PROFILE.contains("store: sqlite"));
        assert!(CLOUD_PROFILE.contains("\"daemon\""));
        assert!(CLOUD_PROFILE.contains("\"start\""));
    }
}
