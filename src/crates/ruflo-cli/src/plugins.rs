//! Native V3 `plugins` command — plugin management (IPFS registry).
//!
//! Source: `v3/@claude-flow/cli/src/commands/plugins.ts`. Subcommands:
//! list/search/install/uninstall/upgrade/toggle/info/create/rate.
//! IPFS registry not in native build; local list + degrade.

use std::fs;
use std::path::{Path, PathBuf};

fn plugins_file(root: &Path) -> PathBuf {
    root.join(".claude-flow/plugins.json")
}

fn load_installed(root: &Path) -> Vec<serde_json::Value> {
    fs::read_to_string(plugins_file(root))
        .ok()
        .and_then(|r| serde_json::from_str(&r).ok())
        .unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginsCommand {
    pub operation: String,
    pub name: Option<String>,
    pub installed: bool,
    pub available: bool,
    pub category: Option<String>,
    pub plugin_type: Option<String>,
    pub official: bool,
    pub featured: bool,
    pub json: bool,
    pub enabled: Option<bool>,
}

pub fn run(root: &Path, command: PluginsCommand) -> u8 {
    match command.operation.as_str() {
        "" | "list" => list(root, &command),
        "search" => search(&command),
        "install" => install(root, &command),
        "uninstall" => uninstall(root, &command),
        "upgrade" => upgrade(&command),
        "toggle" => toggle(root, &command),
        "info" => info(&command),
        "create" => create(&command),
        "rate" => rate(&command),
        _ => {
            eprintln!("[ERROR] Unknown: {} (list|search|install|uninstall|upgrade|toggle|info|create|rate)", command.operation);
            1
        }
    }
}

fn list(root: &Path, command: &PluginsCommand) -> u8 {
    let installed = load_installed(root);
    if command.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!(installed)).unwrap_or_default()
        );
    } else {
        println!("\nPlugins");
        println!("{}", "\u{2500}".repeat(50));
        if installed.is_empty() {
            println!("  No plugins installed.");
            println!("  Browse: ruflo plugins search <query>");
            println!("  Install: ruflo plugins install <name>");
        } else {
            for p in &installed {
                let name = p["name"].as_str().unwrap_or("?");
                let ver = p["version"].as_str().unwrap_or("?");
                let enabled = p["enabled"].as_bool().unwrap_or(true);
                let icon = if enabled { "\u{2714}" } else { "\u{2718}" };
                println!("  {icon} {name} ({ver})");
            }
        }
        if command.available {
            println!("\n  Available plugins (transfer-store registry):");
        }
    }
    0
}

fn search(command: &PluginsCommand) -> u8 {
    let query = command.name.as_deref().unwrap_or("");
    eprintln!("[ERROR] Plugin registry search via transfer-store (native).");
    eprintln!("  Query: \"{query}\"");
    eprintln!("  Use: ruflo plugins search \"{query}\"");
    1
}

fn install(root: &Path, command: &PluginsCommand) -> u8 {
    let Some(name) = &command.name else {
        eprintln!("[ERROR] --name is required");
        return 1;
    };
    eprintln!("[ERROR] Plugin installation requires the IPFS registry client.");
    eprintln!("  Plugin: {name}");
    eprintln!("  Use: ruflo plugins install {name}");
    // Could record intent locally, but actual install is deferred.
    let _ = root;
    1
}

fn uninstall(root: &Path, command: &PluginsCommand) -> u8 {
    let Some(name) = &command.name else {
        eprintln!("[ERROR] --name is required");
        return 1;
    };
    let mut installed = load_installed(root);
    let before = installed.len();
    installed.retain(|p| p["name"].as_str() != Some(name.as_str()));
    if installed.len() == before {
        eprintln!("[ERROR] Plugin '{name}' is not installed.");
        return 1;
    }
    let dir = root.join(".claude-flow");
    let _ = fs::create_dir_all(&dir);
    let path = plugins_file(root);
    let tmp = path.with_extension("json.tmp");
    let _ = fs::write(
        &tmp,
        serde_json::to_vec_pretty(&installed).unwrap_or_default(),
    );
    let _ = fs::rename(&tmp, &path);
    println!("Uninstalled plugin: {name}");
    0
}

fn upgrade(command: &PluginsCommand) -> u8 {
    let name = command.name.as_deref().unwrap_or("all");
    eprintln!("[ERROR] Plugin upgrade requires the IPFS registry client.");
    eprintln!("  Use: ruflo plugins upgrade {name}");
    1
}

fn toggle(root: &Path, command: &PluginsCommand) -> u8 {
    let Some(name) = &command.name else {
        eprintln!("[ERROR] --name is required");
        return 1;
    };
    let enabled = command.enabled.unwrap_or(true);
    let mut installed = load_installed(root);
    let mut found = false;
    for p in &mut installed {
        if p["name"].as_str() == Some(name.as_str()) {
            p["enabled"] = serde_json::json!(enabled);
            found = true;
            break;
        }
    }
    if !found {
        eprintln!("[ERROR] Plugin '{name}' is not installed.");
        return 1;
    }
    let dir = root.join(".claude-flow");
    let _ = fs::create_dir_all(&dir);
    let path = plugins_file(root);
    let tmp = path.with_extension("json.tmp");
    let _ = fs::write(
        &tmp,
        serde_json::to_vec_pretty(&installed).unwrap_or_default(),
    );
    let _ = fs::rename(&tmp, &path);
    println!(
        "Plugin '{name}' {}",
        if enabled { "enabled" } else { "disabled" }
    );
    0
}

fn info(command: &PluginsCommand) -> u8 {
    let name = command.name.as_deref().unwrap_or("");
    eprintln!("[ERROR] Plugin info requires the IPFS registry client.");
    eprintln!("  Plugin: {name}");
    eprintln!("  Use: ruflo plugins info {name}");
    1
}

fn create(command: &PluginsCommand) -> u8 {
    let name = command.name.as_deref().unwrap_or("");
    eprintln!("[ERROR] Plugin scaffolding requires the plugin-manager module.");
    eprintln!("  Use: ruflo plugins create {name}");
    1
}

fn rate(command: &PluginsCommand) -> u8 {
    let name = command.name.as_deref().unwrap_or("");
    eprintln!("[ERROR] Plugin rating requires the IPFS registry client.");
    eprintln!("  Use: ruflo plugins rate {name}");
    1
}
