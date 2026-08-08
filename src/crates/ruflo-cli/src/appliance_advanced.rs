//! Native V3 `appliance-advanced` command — RVFA sign/publish/update.
//!
//! Source: `v3/@claude-flow/cli/src/commands/appliance-advanced.ts`. The TS
//! implementation imports rvfa-signing (Ed25519), rvfa-distribution (IPFS),
//! and rvfa-format (RVFA container). The native build has no Ed25519 or IPFS
//! crate; these operations degrade with documented messages.

use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplianceAdvancedCommand {
    pub operation: String,
    pub file: Option<String>,
    pub section: Option<String>,
    pub patch: Option<String>,
    pub data: Option<String>,
    pub key: Option<String>,
    pub generate_keys: bool,
    pub key_dir: String,
    pub signer: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub version: String,
    pub no_backup: bool,
    pub public_key: Option<String>,
}

pub fn run(_root: &Path, command: ApplianceAdvancedCommand) -> u8 {
    match command.operation.as_str() {
        "sign" => sign(&command),
        "publish" => publish(&command),
        "update" => update(&command),
        _ => {
            eprintln!(
                "[ERROR] Unknown appliance-advanced operation: {}",
                command.operation
            );
            eprintln!("  Valid: sign, publish, update");
            1
        }
    }
}

fn sign(command: &ApplianceAdvancedCommand) -> u8 {
    let Some(file) = &command.file else {
        eprintln!("[ERROR] --file is required");
        return 1;
    };
    if !Path::new(file).exists() {
        eprintln!("[ERROR] File not found: {file}");
        return 1;
    }
    if command.generate_keys {
        println!("\nGenerating Ed25519 Key Pair");
        println!("{}", "\u{2500}".repeat(50));
        println!();
    }
    eprintln!("[ERROR] RVFA appliance signing not available in native build.");
    eprintln!("  Requires Ed25519 (no ed25519 crate in workspace deps).");
    eprintln!("  Use the TypeScript CLI: npx ruflo appliance-advanced sign -f {file}");
    1
}

fn publish(command: &ApplianceAdvancedCommand) -> u8 {
    let Some(file) = &command.file else {
        eprintln!("[ERROR] --file is required");
        return 1;
    };
    if !Path::new(file).exists() {
        eprintln!("[ERROR] File not found: {file}");
        return 1;
    }
    let size = fs::metadata(file).map(|m| m.len()).unwrap_or(0);
    println!("\nPublishing RVFA to IPFS");
    println!("File: {file} ({})", fmt_size(size));
    println!();
    eprintln!("[ERROR] IPFS publishing not available in native build.");
    eprintln!("  Requires the rvfa-distribution module (Pinata API client).");
    eprintln!("  Use the TypeScript CLI: npx ruflo appliance-advanced publish -f {file}");
    1
}

fn update(command: &ApplianceAdvancedCommand) -> u8 {
    let Some(file) = &command.file else {
        eprintln!("[ERROR] --file is required");
        return 1;
    };
    let Some(section) = &command.section else {
        eprintln!("[ERROR] --section is required");
        return 1;
    };
    if command.patch.is_none() && command.data.is_none() {
        eprintln!("[ERROR] Provide --patch (RVFP file) or --data (raw section data)");
        return 1;
    }
    if !Path::new(file).exists() {
        eprintln!("[ERROR] File not found: {file}");
        return 1;
    }
    println!("\nRVFA Hot-Patch Update");
    println!("Appliance: {file}");
    println!("Section:   {section}");
    println!();
    eprintln!("[ERROR] RVFA hot-patch not available in native build.");
    eprintln!("  Requires the rvfa-format + rvfa-distribution modules.");
    eprintln!("  Use the TypeScript CLI: npx ruflo appliance-advanced update");
    1
}

fn fmt_size(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    if bytes < 1024 * 1024 {
        return format!("{:.1} KB", bytes as f64 / 1024.0);
    }
    if bytes < 1024 * 1024 * 1024 {
        return format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0));
    }
    format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}
