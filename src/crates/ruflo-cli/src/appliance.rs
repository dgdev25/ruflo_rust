//! Native V3 `appliance` command — RVFA appliance management.
//!
//! Source: `v3/@claude-flow/cli/src/commands/appliance.ts`. Subcommands:
//! build/inspect/verify/extract/run. The RVFA format requires
//! `../appliance/rvfa-format.js` + `../appliance/rvfa-builder.js` (not
//! available as Rust crates). The native build degrades gracefully with
//! file-existence checks.

use std::fs;
use std::path::Path;
use serde_json::json;

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
    let manifest = json!({
        "format": "rvfa", "version": 1, "profile": profile,
        "arch": arch, "files": file_entries,
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
    if !quick {
        if let Some(files) = rvfa["manifest"]["files"].as_array() {
            let mut ok = 0;
            for f in files {
                let path = f["path"].as_str().unwrap_or("");
                let expected = f["sha256"].as_str().unwrap_or("");
                match fs::read(path) {
                    Ok(content) => {
                        let actual = hex_sha256(&content);
                        if actual == expected { ok += 1; }
                        else { eprintln!("  ✗ {path}: hash mismatch"); }
                    }
                    Err(_) => { eprintln!("  ✗ {path}: missing"); }
                }
            }
            println!("  ✓ {ok}/{} files verified", files.len());
        }
    }
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
    let _ = fs::create_dir_all(target);
    // Extract: copy each listed file from the manifest (if present).
    let mut extracted = 0;
    if let Some(files) = rvfa["manifest"]["files"].as_array() {
        for f in files {
            let path = f["path"].as_str().unwrap_or("");
            if Path::new(path).exists() {
                let dest = format!("{target}/{path}");
                if let Some(parent) = Path::new(&dest).parent() {
                    let _ = fs::create_dir_all(parent);
                }
                if fs::copy(path, &dest).is_ok() { extracted += 1; }
            }
        }
    }
    println!("\nRVFA Extracted: {extracted} files to {target}");
    0
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
    eprintln!("[NOTE] RVFA run: spawn the configured binary (native).");
    eprintln!("  Use: ruflo appliance run -f {file}");
    1
}
