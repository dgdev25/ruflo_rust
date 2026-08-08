//! Native V3 `appliance` command — RVFA appliance management.
//!
//! Source: `v3/@claude-flow/cli/src/commands/appliance.ts`. Subcommands:
//! build/inspect/verify/extract/run. The RVFA format requires
//! `../appliance/rvfa-format.js` + `../appliance/rvfa-builder.js` (not
//! available as Rust crates). The native build degrades gracefully with
//! file-existence checks.

use std::fs;
use std::path::Path;

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
    println!("\nBuilding RVFA Appliance");
    println!("  Profile: {profile}");
    println!("  Output:  {output}");
    println!("  Arch:    {arch}");
    println!();
    eprintln!("[ERROR] RVFA builder not available in native build.");
    eprintln!("  The rvfa-builder module is a TypeScript-only component.");
    eprintln!("  Use: npx ruflo appliance build --profile {profile} -o {output}");
    1
}

fn inspect(command: &ApplianceCommand) -> u8 {
    let Some(file) = &command.file else {
        eprintln!("[ERROR] --file is required");
        return 1;
    };
    if !Path::new(file).exists() {
        eprintln!("[ERROR] File not found: {file}");
        return 1;
    }
    let size = fs::metadata(file).map(|m| m.len()).unwrap_or(0);
    if command.json {
        println!("{{\"file\":\"{file}\",\"size\":{size},\"available\":false}}",);
    } else {
        println!("\nRVFA Inspection");
        println!("  File: {file}");
        println!("  Size: {} bytes", size);
        println!();
        eprintln!("[ERROR] RVFA format reader not available in native build.");
        eprintln!("  Use: npx ruflo appliance inspect -f {file}");
    }
    1
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
    eprintln!("[ERROR] RVFA verifier not available in native build.");
    eprintln!(
        "  Use: npx ruflo appliance verify -f {file}{}",
        if quick { " --quick" } else { "" }
    );
    1
}

fn extract(command: &ApplianceCommand) -> u8 {
    let Some(file) = &command.file else {
        eprintln!("[ERROR] --file is required");
        return 1;
    };
    if !Path::new(file).exists() {
        eprintln!("[ERROR] File not found: {file}");
        return 1;
    }
    let target = command.target_dir.as_deref().unwrap_or("./extracted");
    println!("\nExtracting RVFA Appliance");
    println!("  File:   {file}");
    println!("  Target: {target}");
    println!();
    eprintln!("[ERROR] RVFA extractor not available in native build.");
    eprintln!("  Use: npx ruflo appliance extract -f {file} --target {target}");
    1
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
    eprintln!("[ERROR] RVFA runner not available in native build.");
    eprintln!("  Use: npx ruflo appliance run -f {file}");
    1
}
