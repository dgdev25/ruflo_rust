//! Native V3 `auth` command — Cognitum identity (ADR-306).
//!
//! Source: `v3/@claude-flow/cli/src/commands/auth.ts`. Subcommands:
//! status/login/logout. The OAuth PKCE flow, OS keychain, and browser launch
//! are deferred (no HTTP server or keychain crate in native build). The native
//! build manages profile state in .claude-flow/auth-profiles.json.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

fn auth_file(root: &Path) -> PathBuf {
    root.join(".claude-flow/auth-profiles.json")
}

fn load_profiles(root: &Path) -> Value {
    fs::read_to_string(auth_file(root))
        .ok()
        .and_then(|r| serde_json::from_str(&r).ok())
        .unwrap_or_else(|| json!({"profiles": {}}))
}

fn save_profiles(root: &Path, data: &Value) -> bool {
    let dir = root.join(".claude-flow");
    let _ = fs::create_dir_all(&dir);
    let path = auth_file(root);
    let tmp = path.with_extension("json.tmp");
    let Ok(bytes) = serde_json::to_vec_pretty(data) else {
        return false;
    };
    fs::write(&tmp, &bytes).is_ok() && fs::rename(&tmp, &path).is_ok()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthCommand {
    pub operation: String,
    pub profile: Option<String>,
    pub no_browser: bool,
    pub token_stdin: bool,
    pub all: bool,
    pub json: bool,
}

pub fn run(root: &Path, command: AuthCommand) -> u8 {
    match command.operation.as_str() {
        "status" => status(root, &command),
        "login" => login(root, &command),
        "logout" => logout(root, &command),
        _ => {
            eprintln!(
                "[ERROR] Unknown: {} (status|login|logout)",
                command.operation
            );
            1
        }
    }
}

fn status(root: &Path, command: &AuthCommand) -> u8 {
    let data = load_profiles(root);
    let profiles = &data["profiles"];
    if command.json {
        println!(
            "{}",
            serde_json::to_string_pretty(profiles).unwrap_or_default()
        );
        return 0;
    }
    println!("\nAuth Status");
    println!("{}", "\u{2500}".repeat(40));
    if let Some(ref name) = command.profile {
        if let Some(p) = profiles.get(name.as_str()) {
            let state = p["state"].as_str().unwrap_or("logged-out");
            let method = p["loginMethod"].as_str().unwrap_or("?");
            println!("  {name}: {state} (via {method})");
        } else {
            println!("  {name}: not logged in");
        }
    } else if profiles.as_object().is_some_and(|o| o.is_empty()) {
        println!("  No profiles logged in.");
    } else if let Some(obj) = profiles.as_object() {
        for (name, p) in obj {
            let state = p["state"].as_str().unwrap_or("logged-out");
            let method = p["loginMethod"].as_str().unwrap_or("?");
            println!("  {name}: {state} (via {method})");
        }
    }
    0
}

fn login(root: &Path, command: &AuthCommand) -> u8 {
    let profile = command.profile.clone().unwrap_or_else(|| "default".into());
    // The full OAuth PKCE flow requires an HTTP server + browser launch.
    // In the native build we document the degradation.
    if command.token_stdin {
        eprintln!("[ERROR] --token-stdin requires reading from stdin (not yet implemented in native build).");
        eprintln!("  The OAuth PKCE browser flow and OS keychain are also deferred.");
        return 1;
    }
    eprintln!("[ERROR] Interactive login (OAuth PKCE + browser) not available in native build.");
    eprintln!("  The native CLI does not include the HTTP server or browser launcher.");
    eprintln!("  Profile: {profile}");
    eprintln!();
    eprintln!("  To authenticate, run the TypeScript CLI (npx ruflo auth login) or");
    eprintln!("  use --token-stdin with a pre-obtained token (when implemented).");
    1
}

fn logout(root: &Path, command: &AuthCommand) -> u8 {
    let mut data = load_profiles(root);
    if command.all {
        if let Some(obj) = data.get_mut("profiles").and_then(Value::as_object_mut) {
            obj.clear();
        }
        if !save_profiles(root, &data) {
            eprintln!("[ERROR] Failed to save auth profiles");
            return 1;
        }
        println!("Logged out of all profiles.");
        return 0;
    }
    let profile = command.profile.clone().unwrap_or_else(|| "default".into());
    if let Some(obj) = data.get_mut("profiles").and_then(Value::as_object_mut) {
        if obj.remove(&profile).is_none() {
            eprintln!("[ERROR] Profile '{profile}' is not logged in.");
            return 1;
        }
    }
    if !save_profiles(root, &data) {
        eprintln!("[ERROR] Failed to save auth profiles");
        return 1;
    }
    println!("Logged out of profile: {profile}");
    0
}
