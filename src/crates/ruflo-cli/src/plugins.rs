//! Native `plugins` lifecycle.
//!
//! This module deliberately manages *metadata*, not executable code.  A plugin
//! must be a versioned native declarative manifest in the project-local
//! registry.  JavaScript/npm/IPFS plugin execution is rejected deterministically
//! at this boundary; execution itself remains owned by `ruflo-actions` (ADR-0005).

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const UNSUPPORTED_EXIT: u8 = 2;
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const UNSUPPORTED_MESSAGE: &str = "[UNSUPPORTED] legacy JavaScript/npm plugin execution is unavailable in the native build (ADR-0005). Use a versioned native declarative manifest and an allowlisted ruflo-actions action.";

fn state_file(root: &Path) -> PathBuf {
    root.join(".claude-flow/plugins.json")
}

fn registry_dir(root: &Path) -> PathBuf {
    root.join(".claude-flow/plugins/registry")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct NativePluginManifest {
    schema_version: u8,
    name: String,
    version: String,
    runtime: String,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct InstalledPlugin {
    schema_version: u8,
    name: String,
    version: String,
    runtime: String,
    source: String,
    enabled: bool,
    #[serde(default)]
    capabilities: Vec<String>,
}

impl TryFrom<NativePluginManifest> for InstalledPlugin {
    type Error = String;

    fn try_from(manifest: NativePluginManifest) -> Result<Self, Self::Error> {
        validate_name(&manifest.name)?;
        if manifest.schema_version != 1 {
            return Err(format!(
                "unsupported native plugin schemaVersion {}; expected 1",
                manifest.schema_version
            ));
        }
        if manifest.version.trim().is_empty() {
            return Err("plugin version must not be empty".into());
        }
        if manifest.runtime != "native-declarative" {
            return Err(UNSUPPORTED_MESSAGE.into());
        }
        if manifest
            .actions
            .iter()
            .any(|action| action.trim().is_empty())
        {
            return Err("plugin actions must be non-empty native action identifiers".into());
        }
        Ok(Self {
            schema_version: manifest.schema_version,
            name: manifest.name,
            version: manifest.version,
            runtime: manifest.runtime,
            source: "local-native-registry".into(),
            enabled: true,
            capabilities: manifest.capabilities,
        })
    }
}

fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 128 || name == "." || name == ".." {
        return Err("plugin name must be 1-128 safe characters".into());
    }
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err("plugin name may contain only ASCII letters, digits, '.', '_' and '-'".into());
    }
    Ok(())
}

fn load_installed(root: &Path) -> Result<Vec<InstalledPlugin>, String> {
    let path = state_file(root);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let installed: Vec<InstalledPlugin> = serde_json::from_str(&raw)
        .map_err(|error| format!("invalid plugin state {}: {error}", path.display()))?;
    for plugin in &installed {
        validate_name(&plugin.name)?;
        if plugin.runtime != "native-declarative" {
            return Err(format!("invalid plugin state: {}", UNSUPPORTED_MESSAGE));
        }
    }
    Ok(installed)
}

fn save_installed(root: &Path, installed: &[InstalledPlugin]) -> Result<(), String> {
    let path = state_file(root);
    let parent = path.parent().expect("state file has a parent");
    fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    let temp = path.with_extension("json.tmp");
    fs::write(
        &temp,
        serde_json::to_vec_pretty(installed).expect("serializable plugin state"),
    )
    .map_err(|error| format!("failed to write {}: {error}", temp.display()))?;
    fs::rename(&temp, &path)
        .map_err(|error| format!("failed to replace {}: {error}", path.display()))
}

fn load_registry_plugin(root: &Path, name: &str) -> Result<InstalledPlugin, String> {
    validate_name(name)?;
    let path = registry_dir(root).join(format!("{name}.json"));
    let metadata = fs::metadata(&path).map_err(|_| {
        format!(
            "native plugin '{name}' is not present in the local registry ({})",
            registry_dir(root).display()
        )
    })?;
    if metadata.len() > MAX_MANIFEST_BYTES {
        return Err(format!(
            "native plugin manifest '{name}' exceeds {MAX_MANIFEST_BYTES} bytes"
        ));
    }
    let raw = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let manifest: NativePluginManifest = serde_json::from_str(&raw)
        .map_err(|error| format!("invalid native plugin manifest {}: {error}", path.display()))?;
    if manifest.name != name {
        return Err(format!(
            "registry filename '{name}' does not match manifest name '{}'",
            manifest.name
        ));
    }
    InstalledPlugin::try_from(manifest)
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
        "search" => search(root, &command),
        "install" => install(root, &command),
        "uninstall" => uninstall(root, &command),
        "upgrade" => upgrade(root, &command),
        "toggle" => toggle(root, &command),
        "info" => info(root, &command),
        "create" => create(root, &command),
        "rate" => unsupported("ratings require the legacy network registry"),
        _ => {
            eprintln!("[ERROR] Unknown: {} (list|search|install|uninstall|upgrade|toggle|info|create|rate)", command.operation);
            1
        }
    }
}

fn list(root: &Path, command: &PluginsCommand) -> u8 {
    let installed = match load_installed(root) {
        Ok(value) => value,
        Err(message) => return error(&message),
    };
    if command.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&installed).expect("serializable plugin state")
        );
        return 0;
    }
    println!("\nNative declarative plugins");
    println!("{}", "─".repeat(50));
    if installed.is_empty() {
        println!("  No native declarative plugins installed.");
        println!("  Add a manifest to .claude-flow/plugins/registry/ then run: ruflo plugins install -n <name>");
    } else {
        for plugin in installed {
            println!(
                "  {} {} ({})",
                if plugin.enabled { "✔" } else { "✘" },
                plugin.name,
                plugin.version
            );
        }
    }
    if command.available {
        println!("  Local registry: {}", registry_dir(root).display());
    }
    0
}

fn search(root: &Path, command: &PluginsCommand) -> u8 {
    let query = command.name.as_deref().unwrap_or("").to_ascii_lowercase();
    let entries = match fs::read_dir(registry_dir(root)) {
        Ok(entries) => entries,
        Err(_) => {
            println!(
                "No local native plugin registry at {}",
                registry_dir(root).display()
            );
            return 0;
        }
    };
    let mut names = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry
                .path()
                .file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_owned)
        })
        .filter(|name| name.to_ascii_lowercase().contains(&query))
        .collect::<Vec<_>>();
    names.sort();
    if command.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&names).expect("serializable names")
        );
    } else {
        for name in names {
            println!("{name}");
        }
    }
    0
}

fn install(root: &Path, command: &PluginsCommand) -> u8 {
    let Some(name) = command.name.as_deref() else {
        eprintln!("[ERROR] --name is required");
        return 1;
    };
    if looks_like_legacy_source(name) {
        return unsupported(UNSUPPORTED_MESSAGE);
    }
    let plugin = match load_registry_plugin(root, name) {
        Ok(plugin) => plugin,
        Err(message) if message == UNSUPPORTED_MESSAGE => return unsupported(&message),
        Err(message) => return error(&message),
    };
    let mut installed = match load_installed(root) {
        Ok(value) => value,
        Err(message) => return error(&message),
    };
    if installed
        .iter()
        .any(|candidate| candidate.name == plugin.name)
    {
        return error(&format!(
            "Plugin '{}' is already installed. Use upgrade.",
            plugin.name
        ));
    }
    installed.push(plugin);
    installed.sort_by(|left, right| left.name.cmp(&right.name));
    match save_installed(root, &installed) {
        Ok(()) => {
            println!("Installed native declarative plugin: {name}");
            0
        }
        Err(message) => error(&message),
    }
}

fn uninstall(root: &Path, command: &PluginsCommand) -> u8 {
    let Some(name) = command.name.as_deref() else {
        eprintln!("[ERROR] --name is required");
        return 1;
    };
    let mut installed = match load_installed(root) {
        Ok(value) => value,
        Err(message) => return error(&message),
    };
    let before = installed.len();
    installed.retain(|plugin| plugin.name != name);
    if before == installed.len() {
        return error(&format!("Plugin '{name}' is not installed."));
    }
    match save_installed(root, &installed) {
        Ok(()) => {
            println!("Uninstalled plugin: {name}");
            0
        }
        Err(message) => error(&message),
    }
}

fn upgrade(root: &Path, command: &PluginsCommand) -> u8 {
    let Some(name) = command.name.as_deref() else {
        eprintln!("[ERROR] --name is required");
        return 1;
    };
    let replacement = match load_registry_plugin(root, name) {
        Ok(plugin) => plugin,
        Err(message) if message == UNSUPPORTED_MESSAGE => return unsupported(&message),
        Err(message) => return error(&message),
    };
    let mut installed = match load_installed(root) {
        Ok(value) => value,
        Err(message) => return error(&message),
    };
    let Some(index) = installed.iter().position(|plugin| plugin.name == name) else {
        return error(&format!("Plugin '{name}' is not installed."));
    };
    replacement_with_existing_state(&mut installed[index], replacement);
    match save_installed(root, &installed) {
        Ok(()) => {
            println!("Upgraded native declarative plugin: {name}");
            0
        }
        Err(message) => error(&message),
    }
}

fn replacement_with_existing_state(
    existing: &mut InstalledPlugin,
    mut replacement: InstalledPlugin,
) {
    replacement.enabled = existing.enabled;
    *existing = replacement;
}

fn toggle(root: &Path, command: &PluginsCommand) -> u8 {
    let Some(name) = command.name.as_deref() else {
        eprintln!("[ERROR] --name is required");
        return 1;
    };
    let enabled = command.enabled.unwrap_or(true);
    let mut installed = match load_installed(root) {
        Ok(value) => value,
        Err(message) => return error(&message),
    };
    let Some(plugin) = installed.iter_mut().find(|plugin| plugin.name == name) else {
        return error(&format!("Plugin '{name}' is not installed."));
    };
    plugin.enabled = enabled;
    match save_installed(root, &installed) {
        Ok(()) => {
            println!(
                "Plugin '{name}' {}",
                if enabled { "enabled" } else { "disabled" }
            );
            0
        }
        Err(message) => error(&message),
    }
}

fn info(root: &Path, command: &PluginsCommand) -> u8 {
    let Some(name) = command.name.as_deref() else {
        eprintln!("[ERROR] --name is required");
        return 1;
    };
    let installed = match load_installed(root) {
        Ok(value) => value,
        Err(message) => return error(&message),
    };
    let Some(plugin) = installed.iter().find(|plugin| plugin.name == name) else {
        return error(&format!("Plugin '{name}' is not installed."));
    };
    if command.json {
        println!(
            "{}",
            serde_json::to_string_pretty(plugin).expect("serializable plugin")
        );
    } else {
        println!(
            "{} {}\n  runtime: {}\n  source: {}\n  enabled: {}",
            plugin.name, plugin.version, plugin.runtime, plugin.source, plugin.enabled
        );
    }
    0
}

fn create(root: &Path, command: &PluginsCommand) -> u8 {
    let Some(name) = command.name.as_deref() else {
        eprintln!("[ERROR] --name is required");
        return 1;
    };
    if let Err(message) = validate_name(name) {
        return error(&message);
    }
    let path = registry_dir(root).join(format!("{name}.json"));
    if path.exists() {
        return error(&format!(
            "Native plugin manifest already exists: {}",
            path.display()
        ));
    }
    if let Err(io) = fs::create_dir_all(registry_dir(root)) {
        return error(&format!("failed to create local registry: {io}"));
    }
    let template = NativePluginManifest {
        schema_version: 1,
        name: name.to_owned(),
        version: "0.1.0".into(),
        runtime: "native-declarative".into(),
        capabilities: Vec::new(),
        actions: Vec::new(),
    };
    match fs::write(
        &path,
        serde_json::to_vec_pretty(&template).expect("serializable template"),
    ) {
        Ok(()) => {
            println!(
                "Created native declarative plugin manifest: {}",
                path.display()
            );
            0
        }
        Err(io) => error(&format!("failed to create {}: {io}", path.display())),
    }
}

fn looks_like_legacy_source(name: &str) -> bool {
    name.contains('/')
        || name.starts_with('@')
        || name.ends_with(".js")
        || name.ends_with(".mjs")
        || name.ends_with(".cjs")
}
fn unsupported(message: &str) -> u8 {
    eprintln!("{message}");
    UNSUPPORTED_EXIT
}
fn error(message: &str) -> u8 {
    eprintln!("[ERROR] {message}");
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn command(operation: &str, name: Option<&str>) -> PluginsCommand {
        PluginsCommand {
            operation: operation.into(),
            name: name.map(str::to_owned),
            installed: false,
            available: false,
            category: None,
            plugin_type: None,
            official: false,
            featured: false,
            json: false,
            enabled: None,
        }
    }
    fn manifest(root: &Path, name: &str, runtime: &str, version: &str) {
        fs::create_dir_all(registry_dir(root)).unwrap();
        let value = serde_json::json!({"schemaVersion": 1, "name": name, "version": version, "runtime": runtime, "actions": ["echo-args"]});
        fs::write(
            registry_dir(root).join(format!("{name}.json")),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn local_native_lifecycle_preserves_enabled_state_on_upgrade() {
        let temp = TempDir::new().unwrap();
        manifest(temp.path(), "example", "native-declarative", "1.0.0");
        assert_eq!(run(temp.path(), command("install", Some("example"))), 0);
        let mut disable = command("toggle", Some("example"));
        disable.enabled = Some(false);
        assert_eq!(run(temp.path(), disable), 0);
        manifest(temp.path(), "example", "native-declarative", "2.0.0");
        assert_eq!(run(temp.path(), command("upgrade", Some("example"))), 0);
        let state = load_installed(temp.path()).unwrap();
        assert_eq!(state[0].version, "2.0.0");
        assert!(!state[0].enabled);
        assert_eq!(run(temp.path(), command("uninstall", Some("example"))), 0);
        assert!(load_installed(temp.path()).unwrap().is_empty());
    }

    #[test]
    fn javascript_and_npm_plugin_requests_have_stable_unsupported_exit() {
        let temp = TempDir::new().unwrap();
        manifest(temp.path(), "legacy", "javascript", "1.0.0");
        assert_eq!(
            run(temp.path(), command("install", Some("legacy"))),
            UNSUPPORTED_EXIT
        );
        assert_eq!(
            run(temp.path(), command("install", Some("@scope/plugin"))),
            UNSUPPORTED_EXIT
        );
    }

    #[test]
    fn traversal_and_mismatched_manifest_names_are_rejected() {
        let temp = TempDir::new().unwrap();
        assert_eq!(run(temp.path(), command("create", Some("../escape"))), 1);
        manifest(temp.path(), "different", "native-declarative", "1.0.0");
        assert_eq!(run(temp.path(), command("install", Some("example"))), 1);
    }
}
