//! Native V3 `auth` command — Cognitum identity (ADR-306).
//!
//! Source: `v3/@claude-flow/cli/src/commands/auth.ts`. Subcommands:
//! status/login/logout. The OAuth PKCE flow, OS keychain, and browser launch
//! are deferred (no HTTP server or keychain crate in native build). The native
//! build manages profile state in .claude-flow/auth-profiles.json.

use std::fs;
use std::io::Read;
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

    // --token-stdin: read a pre-obtained token and store it. This is the
    // non-interactive path — useful in CI and headless boxes.
    if command.token_stdin {
        let mut buffer = String::new();
        if std::io::stdin().read_line(&mut buffer).is_err() {
            eprintln!("[ERROR] Failed to read token from stdin.");
            return 1;
        }
        let token = buffer.trim().to_string();
        if token.is_empty() {
            eprintln!("[ERROR] Empty token on stdin.");
            return 1;
        }
        return store_token(root, &profile, &token, "token-stdin");
    }

    // PKCE flow: generate verifier + challenge (S256), build the auth URL,
    // print it for the user to open, then accept the returned code/token via
    // a prompt. No HTTP callback server (keeps the native build dependency-free).
    let verifier = pkce_code_verifier();
    let challenge = pkce_code_challenge(&verifier);
    let state = random_state(16);

    let client_id = std::env::var("RUFLO_OAUTH_CLIENT_ID")
        .unwrap_or_else(|_| "ruflo-cli".into());
    let auth_url = std::env::var("RUFLO_OAUTH_AUTH_URL")
        .unwrap_or_else(|_| "https://auth.anthropic.com/authorize".into());
    let redirect = "http://localhost:8765/callback";

    let url = format!(
        "{auth_url}?response_type=code&client_id={client_id}\
         &redirect_uri={redirect}&scope=openid%20profile\
         &code_challenge={challenge}&code_challenge_method=S256&state={state}"
    );

    println!("\nOAuth PKCE Login (profile: {profile})");
    println!("{}", "\u{2500}".repeat(55));
    println!("Open this URL in your browser to authorize:\n");
    println!("  {url}\n");
    println!("Code challenge (S256): {challenge}");
    println!("Verifier (send with token exchange): {verifier}\n");

    // Best-effort browser launch.
    if let Some(opener) = browser_opener() {
        let _ = std::process::Command::new(&opener).arg(&url).spawn();
    }

    println!("After authorizing, paste the full redirect URL or the code here:");
    print!("> ");
    use std::io::Write;
    let _ = std::io::stdout().flush();

    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        eprintln!("[ERROR] Failed to read response.");
        return 1;
    }
    let line = line.trim();
    // Accept either a bare code or a full callback URL containing ?code=.
    let code = if let Some(idx) = line.find("code=") {
        line[idx + 5..].split('&').next().unwrap_or("").to_string()
    } else {
        line.to_string()
    };
    if code.is_empty() {
        eprintln!("[ERROR] No authorization code provided.");
        return 1;
    }

    // Store the code as the credential (token exchange would hit the token
    // endpoint with verifier — left to the operator/server side in this
    // serverless flow). Persist what we have so the profile is marked in.
    store_token(root, &profile, &code, "oauth-pkce")
}

/// Store a credential under a profile, persisting via atomic write.
fn store_token(root: &Path, profile: &str, token: &str, method: &str) -> u8 {
    let mut data = load_profiles(root);
    if data.get("profiles").is_none() {
        data["profiles"] = serde_json::json!({});
    }
    if let Some(obj) = data.get_mut("profiles").and_then(Value::as_object_mut) {
        obj.insert(
            profile.to_string(),
            serde_json::json!({
                "token": token,
                "loginMethod": method,
                "at": now_ms(),
            }),
        );
    }
    if !save_profiles(root, &data) {
        eprintln!("[ERROR] Failed to persist auth profile.");
        return 1;
    }
    println!("\n\u{2714} Logged in as profile '{profile}' via {method}.");
    0
}

/// PKCE code_verifier: 43-128 chars from the unreserved set (RFC 7636 §4.1).
fn pkce_code_verifier() -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    let len = 64;
    (0..len).map(|_| {
        let r = pseudo_rand_u32() as usize % CHARSET.len();
        CHARSET[r] as char
    }).collect()
}

/// PKCE code_challenge: base64url(sha256(verifier)) without padding (S256).
fn pkce_code_challenge(verifier: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let digest = hasher.finalize();
    base64url_no_pad(&digest)
}

fn random_state(bytes: usize) -> String {
    (0..bytes)
        .map(|_| format!("{:02x}", pseudo_rand_u32() & 0xFF))
        .collect()
}

fn base64url_no_pad(bytes: &[u8]) -> String {
    const ALPHA: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8) | (bytes[i + 2] as u32);
        out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 6) & 0x3F) as usize] as char);
        out.push(ALPHA[(n & 0x3F) as usize] as char);
        i += 3;
    }
    let rem = bytes.len() - i;
    if rem == 1 {
        let n = (bytes[i] as u32) << 16;
        out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
    } else if rem == 2 {
        let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8);
        out.push(ALPHA[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 12) & 0x3F) as usize] as char);
        out.push(ALPHA[((n >> 6) & 0x3F) as usize] as char);
    }
    out
}

fn browser_opener() -> Option<&'static str> {
    for cmd in &["xdg-open", "open", "wslview", "start"] {
        if std::process::Command::new(cmd).arg("--version").output().is_ok()
            || which_on_path(cmd)
        {
            return Some(cmd);
        }
    }
    None
}

fn which_on_path(cmd: &str) -> bool {
    if let Ok(path) = std::env::var("PATH") {
        for dir in path.split(':') {
            if std::path::Path::new(dir).join(cmd).is_file() {
                return true;
            }
        }
    }
    false
}

fn pseudo_rand_u32() -> u32 {
    use std::cell::Cell;
    thread_local! {
        static STATE: Cell<u64> = Cell::new(0x2545F4914F6CDD1D);
    }
    STATE.with(|s| {
        let mut x = s.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        (x & 0xFFFFFFFF) as u32
    })
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_verifier_length_and_charset() {
        let v = pkce_code_verifier();
        // RFC 7636: 43-128 chars, unreserved set only.
        assert!(v.len() >= 43 && v.len() <= 128, "len {}", v.len());
        for c in v.chars() {
            assert!(c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_' || c == '~');
        }
    }

    #[test]
    fn pkce_challenge_is_base64url_s256() {
        // Known vector: verifier "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"
        // → challenge "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM".
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = pkce_code_challenge(verifier);
        assert_eq!(challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn base64url_no_padding_correct() {
        // "abc" → b64url "YWJj" (no padding).
        assert_eq!(base64url_no_pad(b"abc"), "YWJj");
        // 1 byte: "f" → "Zg"
        assert_eq!(base64url_no_pad(b"f"), "Zg");
        // 2 bytes: "fo" → "Zm8"
        assert_eq!(base64url_no_pad(b"fo"), "Zm8");
    }

    #[test]
    fn token_stdin_stores_profile() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // Simulate stdin token via store_token directly.
        let rc = store_token(root, "ci", "tok_123", "token-stdin");
        assert_eq!(rc, 0);
        let data = load_profiles(root);
        let prof = &data["profiles"]["ci"];
        assert_eq!(prof["token"].as_str(), Some("tok_123"));
        assert_eq!(prof["loginMethod"].as_str(), Some("token-stdin"));
    }
}
