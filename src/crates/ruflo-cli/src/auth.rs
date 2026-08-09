//! Native V3 `auth` command — Cognitum identity (ADR-306).
//!
//! The Node CLI persists only non-secret profile metadata in `~/.ruflo/auth.json`;
//! access tokens are held in an OS keychain or process-lifetime session. Native
//! Rust currently supports the safe session-only path and never writes a token,
//! refresh token, authorization code, or credential-derived value to disk.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde_json::{json, Value};

fn auth_file() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("RUFLO_STATE_DIR") {
        return Some(PathBuf::from(path).join("auth.json"));
    }
    if let Some(path) = std::env::var_os("XDG_STATE_HOME") {
        return Some(PathBuf::from(path).join("ruflo/auth.json"));
    }
    std::env::var_os("HOME").map(|path| PathBuf::from(path).join(".ruflo/auth.json"))
}

fn empty_profiles() -> Value {
    json!({"schemaVersion": 1, "defaultProfile": "default", "profiles": {}})
}

fn load_profiles() -> Value {
    auth_file()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_else(empty_profiles)
}

fn save_profiles(data: &Value) -> bool {
    let Some(path) = auth_file() else {
        return false;
    };
    let Some(dir) = path.parent() else {
        return false;
    };
    if fs::create_dir_all(dir).is_err() {
        return false;
    }
    let tmp = path.with_extension("json.tmp");
    let Ok(bytes) = serde_json::to_vec_pretty(data) else {
        return false;
    };
    if fs::write(&tmp, bytes).is_err() || fs::rename(&tmp, &path).is_err() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).is_ok()
    }
    #[cfg(not(unix))]
    true
}

fn sessions() -> &'static Mutex<std::collections::BTreeMap<String, String>> {
    static SESSIONS: OnceLock<Mutex<std::collections::BTreeMap<String, String>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(std::collections::BTreeMap::new()))
}

fn session_active(profile: &str) -> bool {
    sessions()
        .lock()
        .is_ok_and(|entries| entries.contains_key(profile))
}

fn legacy_auth_file(root: &Path) -> PathBuf {
    root.join(".claude-flow/auth-profiles.json")
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
        "logout" => logout(&command),
        _ => {
            eprintln!(
                "[ERROR] Unknown: {} (status|login|logout)",
                command.operation
            );
            1
        }
    }
}

fn displayed_state<'a>(name: &str, profile: &'a Value) -> &'a str {
    if session_active(name) {
        "authenticated-session"
    } else {
        profile["state"].as_str().unwrap_or("logged-out")
    }
}

fn status(root: &Path, command: &AuthCommand) -> u8 {
    let data = load_profiles();
    let profiles = &data["profiles"];
    if command.json {
        let mut shown = profiles.clone();
        if let Some(entries) = shown.as_object_mut() {
            for (name, profile) in entries {
                if session_active(name) {
                    profile["state"] = json!("authenticated-session");
                }
            }
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&shown).unwrap_or_default()
        );
        return 0;
    }
    println!("\nAuth Status\n{}", "\u{2500}".repeat(40));
    if legacy_auth_file(root).exists() {
        eprintln!("[WARN] Legacy project-local auth-profiles.json is ignored; remove it because it may contain credentials.");
    }
    if let Some(name) = &command.profile {
        if let Some(profile) = profiles.get(name) {
            println!(
                "  {name}: {} (via {})",
                displayed_state(name, profile),
                profile["loginMethod"].as_str().unwrap_or("?")
            );
        } else {
            println!("  {name}: not logged in");
        }
    } else if profiles
        .as_object()
        .is_some_and(|entries| entries.is_empty())
    {
        println!("  No profiles logged in.");
    } else if let Some(entries) = profiles.as_object() {
        for (name, profile) in entries {
            println!(
                "  {name}: {} (via {})",
                displayed_state(name, profile),
                profile["loginMethod"].as_str().unwrap_or("?")
            );
        }
    }
    0
}

fn login(root: &Path, command: &AuthCommand) -> u8 {
    let profile = command.profile.clone().unwrap_or_else(|| "default".into());
    if !command.token_stdin {
        eprintln!("[ERROR] Interactive OAuth requires an OS-keychain adapter and is unavailable. Use --token-stdin for an explicit process-lifetime session.");
        return 1;
    }
    let mut token = String::new();
    if std::io::stdin().read_line(&mut token).is_err() {
        eprintln!("[ERROR] Failed to read token from stdin.");
        return 1;
    }
    let token = token.trim();
    if token.is_empty() {
        eprintln!("[ERROR] Empty token on stdin.");
        return 1;
    }
    store_token(root, &profile, token, "token-stdin")
}

/// Keep a credential in the current process only. Persisted data is metadata.
fn store_token(root: &Path, profile: &str, token: &str, method: &str) -> u8 {
    if profile.is_empty()
        || profile.chars().any(char::is_control)
        || token.chars().any(char::is_control)
    {
        eprintln!("[ERROR] Invalid profile or token input.");
        return 1;
    }
    let mut data = load_profiles();
    if !data["profiles"].is_object() {
        data["profiles"] = json!({});
    }
    if let Some(entries) = data["profiles"].as_object_mut() {
        entries.insert(
            profile.into(),
            json!({"state":"session-only", "loginMethod":method, "at":now_ms()}),
        );
    }
    if let Ok(mut entries) = sessions().lock() {
        entries.insert(profile.into(), token.into());
    } else {
        eprintln!("[ERROR] Failed to establish auth session.");
        return 1;
    }
    if !save_profiles(&data) {
        eprintln!("[WARN] Session is active, but non-secret profile metadata could not be saved.");
    }
    if legacy_auth_file(root).exists() {
        eprintln!("[WARN] Legacy project-local auth-profiles.json was ignored; remove it because it may contain credentials.");
    }
    println!("\n\u{2714} Session established for profile '{profile}' via {method}; credentials are never written to disk.");
    0
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn logout(command: &AuthCommand) -> u8 {
    let mut data = load_profiles();
    let Some(entries) = data["profiles"].as_object_mut() else {
        eprintln!("[ERROR] Invalid auth profile metadata.");
        return 1;
    };
    if command.all {
        entries.clear();
        if let Ok(mut sessions) = sessions().lock() {
            sessions.clear();
        }
        if !save_profiles(&data) {
            eprintln!("[ERROR] Failed to save auth profile metadata.");
            return 1;
        }
        println!("Logged out of all profiles.");
        return 0;
    }
    let profile = command.profile.clone().unwrap_or_else(|| "default".into());
    let removed_metadata = entries.remove(&profile).is_some();
    let removed_session = sessions()
        .lock()
        .is_ok_and(|mut sessions| sessions.remove(&profile).is_some());
    if !removed_metadata && !removed_session {
        eprintln!("[ERROR] Profile '{profile}' is not logged in.");
        return 1;
    }
    if !save_profiles(&data) {
        eprintln!("[ERROR] Failed to save auth profile metadata.");
        return 1;
    }
    println!("Logged out of profile: {profile}");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_session_only_and_never_written_to_metadata() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("RUFLO_STATE_DIR", dir.path());
        assert_eq!(store_token(dir.path(), "ci", "tok_123", "token-stdin"), 0);
        let saved = fs::read_to_string(auth_file().unwrap()).unwrap();
        assert!(!saved.contains("tok_123"));
        assert!(!saved.contains("token\""));
        assert!(saved.contains("session-only"));
        assert!(session_active("ci"));
        std::env::remove_var("RUFLO_STATE_DIR");
    }

    #[test]
    fn legacy_project_auth_is_never_loaded() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = legacy_auth_file(dir.path());
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        fs::write(&legacy, r#"{"profiles":{"old":{"token":"leaked"}}}"#).unwrap();
        let data = load_profiles();
        assert_ne!(data["profiles"]["old"]["token"].as_str(), Some("leaked"));
    }

    #[test]
    fn control_characters_are_rejected_before_session_storage() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(store_token(dir.path(), "ci\n", "secret", "token-stdin"), 1);
        assert_eq!(
            store_token(dir.path(), "ci", "secret\nvalue", "token-stdin"),
            1
        );
    }
}
