//! Native V3 `claims` command — claims-based authorization, permissions, access control.
//!
//! Source of truth: `v3/@claude-flow/cli/src/commands/claims.ts`. Preserves the V3
//! config-path precedence, default policy, wildcard claim matching, atomic write
//! behavior, and per-subcommand output/exit semantics.

use std::env;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimsCommand {
    Overview,
    Help {
        subcommand: Option<String>,
    },
    List {
        user: Option<String>,
        role: Option<String>,
        resource: Option<String>,
    },
    Check {
        claim: Option<String>,
        user: Option<String>,
        resource: Option<String>,
    },
    Grant {
        claim: Option<String>,
        user: Option<String>,
        role: Option<String>,
        scope: String,
        expires: Option<String>,
    },
    Revoke {
        claim: Option<String>,
        user: Option<String>,
        role: Option<String>,
    },
    Roles {
        action: String,
        name: Option<String>,
    },
    Policies {
        action: String,
        name: Option<String>,
    },
}

pub fn run(root: &Path, command: ClaimsCommand) -> u8 {
    match run_inner(root, command) {
        Ok(code) => code,
        Err(message) => {
            // V3 output.printError → stderr `[ERROR] <message>` (single line).
            eprintln!("[ERROR] {message}");
            1
        }
    }
}

fn run_inner(root: &Path, command: ClaimsCommand) -> Result<u8, String> {
    match command {
        ClaimsCommand::Overview => {
            print!("{OVERVIEW}");
            Ok(0)
        }
        ClaimsCommand::Help { subcommand } => {
            print!("{}", help(&binary_name(), subcommand.as_deref()));
            Ok(0)
        }
        ClaimsCommand::List {
            user,
            role,
            resource,
        } => list(root, user, role, resource),
        ClaimsCommand::Check {
            claim,
            user,
            resource,
        } => check(root, claim, user, resource),
        ClaimsCommand::Grant {
            claim,
            user,
            role,
            scope: _,
            expires: _,
        } => grant(root, claim, user, role),
        ClaimsCommand::Revoke { claim, user, role } => revoke(root, claim, user, role),
        ClaimsCommand::Roles { action, name } => roles(root, action, name),
        ClaimsCommand::Policies { action, name } => policies(root, action, name),
    }
}

/// argv[0] basename so `claude-flow claims grant --help` does not print `ruflo`.
fn binary_name() -> String {
    std::env::args()
        .next()
        .and_then(|arg| {
            Path::new(&arg)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "ruflo".into())
}

fn list(
    root: &Path,
    user: Option<String>,
    role: Option<String>,
    resource: Option<String>,
) -> Result<u8, String> {
    let _ = (user, role, resource); // V3 ignores these filters for the table render today.
    let (config, path) = load(root).map_err(|e| format!("Failed to list claims: {e}"))?;

    println!();
    println!("\x1b[1mClaims Configuration\x1b[0m");
    println!("\x1b[2m{}\x1b[0m", "─".repeat(50));

    let roles = config
        .get("roles")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if !roles.is_empty() {
        println!();
        println!("\x1b[1mRoles\x1b[0m");
        let rows = roles
            .iter()
            .map(|(name, claims)| {
                let claims = claim_list(claims);
                let count = claims.len();
                let preview = preview(&claims, 4);
                vec![name.clone(), count.to_string(), preview]
            })
            .collect::<Vec<_>>();
        println!(
            "{}",
            table(&["Role", "Claims", "Preview"], &rows, &[0, 0, 50])
        );
    }

    let users = config
        .get("users")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if !users.is_empty() {
        println!();
        println!("\x1b[1mUsers\x1b[0m");
        let rows = users
            .iter()
            .map(|(name, info)| {
                let role = info
                    .get("role")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| "\x1b[2m(none)\x1b[0m".into());
                let extra = info
                    .get("claims")
                    .map(|claims| claim_list(claims).join(", "))
                    .unwrap_or_else(|| "\x1b[2m(none)\x1b[0m".into());
                vec![name.clone(), role, extra]
            })
            .collect::<Vec<_>>();
        println!(
            "{}",
            table(&["User", "Role", "Extra Claims"], &rows, &[0, 0, 0])
        );
    }

    let defaults = config
        .get("defaultClaims")
        .map(claim_list)
        .unwrap_or_default();
    if !defaults.is_empty() {
        println!();
        println!("\x1b[1mDefault Claims\x1b[0m");
        for claim in &defaults {
            println!("  {claim}");
        }
    }

    println!();
    println!("\x1b[2mConfig: {}\x1b[0m", path.display());
    Ok(0)
}

#[allow(clippy::too_many_lines)]
fn check(
    root: &Path,
    claim: Option<String>,
    user: Option<String>,
    resource: Option<String>,
) -> Result<u8, String> {
    // claims.ts:168 — empty claim is falsy and rejected.
    let Some(claim) = non_empty(claim) else {
        return Err("Claim is required".into());
    };
    // claims.ts:165 — `ctx.flags.user || 'current'` (empty string is falsy).
    let user = non_empty(user).unwrap_or_else(|| "current".into());

    println!();
    println!("\x1b[1mClaim Check\x1b[0m");
    println!("\x1b[2m{}\x1b[0m", "─".repeat(40));

    // claims.ts:188-271 — resolve the merged policy; on any error fall back to the
    // permissive default (admin:* still requires an explicit grant).
    let (granted, reason, policy_source) = match load_merged_for_check(root) {
        Ok((config, source)) => {
            let user_config = config.get("users").and_then(|u| u.get(&user));
            let mut user_claims: Vec<String> = config
                .get("defaultClaims")
                .map(claim_list)
                .unwrap_or_default();
            if let Some(info) = user_config {
                if let Some(extra) = info.get("claims") {
                    user_claims.extend(claim_list(extra));
                }
                if let Some(Value::String(role_name)) = info.get("role") {
                    if let Some(role_claims) = config.get("roles").and_then(|r| r.get(role_name)) {
                        user_claims.extend(claim_list(role_claims));
                    }
                }
            }

            let granted = check_claim(&claim, &user_claims);
            let reason = if granted {
                if user_config.and_then(|i| i.get("role")).is_some() {
                    let role = user_config
                        .and_then(|i| i.get("role"))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    format!("Granted via role: {role}")
                } else {
                    "Granted via default policy".into()
                }
            } else {
                "Not in user claims or role permissions".into()
            };
            (granted, reason, source)
        }
        Err(_) => {
            let granted = !claim.starts_with("admin:");
            let reason = if granted {
                "Granted (default permissive policy)".into()
            } else {
                "Admin claims require explicit grant".into()
            };
            (granted, reason, "fallback".into())
        }
    };

    if granted {
        println!("\x1b[32m✓ Claim granted\x1b[0m");
    } else {
        println!("\x1b[31m✗ Claim denied\x1b[0m");
    }
    println!();
    let resource = non_empty(resource).unwrap_or_else(|| "global".into());
    let result_label = if granted {
        "\x1b[32mGRANTED\x1b[0m".to_string()
    } else {
        "\x1b[31mDENIED\x1b[0m".to_string()
    };
    print!(
        "{}",
        boxed(
            "Result",
            &[
                format!("Claim: {claim}"),
                format!("User: {user}"),
                format!("Resource: {resource}"),
                format!("Result: {result_label}"),
                String::new(),
                format!("Reason: {reason}"),
                format!("Policy: {policy_source}"),
            ]
        )
    );
    Ok(if granted { 0 } else { 1 })
}

/// Treat empty strings (and flag-like values consumed by the shared option parser)
/// as absent, mirroring JavaScript falsy / yargs value handling.
fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.is_empty() && !v.starts_with('-'))
}

fn grant(
    root: &Path,
    claim: Option<String>,
    user: Option<String>,
    role: Option<String>,
) -> Result<u8, String> {
    let Some(claim) = non_empty(claim) else {
        return Err("Claim is required".into());
    };
    let user = non_empty(user);
    let role = non_empty(role);
    // claims.ts:319 — empty user/role are falsy; both absent is rejected.
    if user.is_none() && role.is_none() {
        return Err("Either user or role is required".into());
    }
    let (mut config, path) = load(root).map_err(|e| format!("Failed to grant claim: {e}"))?;
    ensure_object(&mut config, "users");
    ensure_object(&mut config, "roles");

    if let Some(user) = &user {
        let users = config
            .get_mut("users")
            .and_then(Value::as_object_mut)
            .expect("users object");
        if !users.contains_key(user) {
            users.insert(user.clone(), json!({}));
        }
        let info = users.get_mut(user).expect("user entry");
        ensure_array(info, "claims");
        let claims = info
            .get_mut("claims")
            .and_then(Value::as_array_mut)
            .expect("claims array");
        if !claims.iter().any(|c| c.as_str() == Some(&claim)) {
            claims.push(Value::String(claim.clone()));
        }
    }

    if let Some(role) = &role {
        let roles = config
            .get_mut("roles")
            .and_then(Value::as_object_mut)
            .expect("roles object");
        // Normalize a wrong-shaped existing role value (e.g. `{}`) to an array
        // rather than panicking on the subsequent `as_array_mut`.
        match roles.get_mut(role) {
            None => {
                roles.insert(role.clone(), json!([]));
            }
            Some(value) if !value.is_array() => {
                *value = json!([]);
            }
            _ => {}
        }
        let claims = roles
            .get_mut(role)
            .and_then(Value::as_array_mut)
            .expect("role claims");
        if !claims.iter().any(|c| c.as_str() == Some(&claim)) {
            claims.push(Value::String(claim.clone()));
        }
    }

    save(root, &config, &path).map_err(|e| format!("Failed to grant claim: {e}"))?;

    println!();
    let target = if user.is_some() {
        format!("user \"{}\"", user.unwrap())
    } else {
        format!("role \"{}\"", role.unwrap())
    };
    println!("\x1b[32mGranted \"{claim}\" to {target}\x1b[0m");
    println!("\x1b[2mSaved to: {}\x1b[0m", path.display());
    Ok(0)
}

fn revoke(
    root: &Path,
    claim: Option<String>,
    user: Option<String>,
    role: Option<String>,
) -> Result<u8, String> {
    let Some(claim) = non_empty(claim) else {
        return Err("Claim is required".into());
    };
    let user = non_empty(user);
    let role = non_empty(role);
    if user.is_none() && role.is_none() {
        return Err("Either user or role is required".into());
    }
    let (mut config, path) = load(root).map_err(|e| format!("Failed to revoke claim: {e}"))?;
    let mut removed = false;

    if let Some(user) = &user {
        if let Some(info) = config
            .get_mut("users")
            .and_then(Value::as_object_mut)
            .and_then(|u| u.get_mut(user))
        {
            if let Some(claims) = info.get_mut("claims").and_then(Value::as_array_mut) {
                if let Some(idx) = claims.iter().position(|c| c.as_str() == Some(&claim)) {
                    claims.remove(idx);
                    removed = true;
                }
            }
        }
    }

    if let Some(role) = &role {
        if let Some(claims) = config
            .get_mut("roles")
            .and_then(Value::as_object_mut)
            .and_then(|r| r.get_mut(role))
            .and_then(Value::as_array_mut)
        {
            if let Some(idx) = claims.iter().position(|c| c.as_str() == Some(&claim)) {
                claims.remove(idx);
                removed = true;
            }
        }
    }

    if !removed {
        let target = if user.is_some() {
            format!("user \"{}\"", user.unwrap())
        } else {
            format!("role \"{}\"", role.unwrap())
        };
        return Err(format!("Claim \"{claim}\" not found on {target}"));
    }

    save(root, &config, &path).map_err(|e| format!("Failed to revoke claim: {e}"))?;

    println!();
    let target = if user.is_some() {
        format!("user \"{}\"", user.unwrap())
    } else {
        format!("role \"{}\"", role.unwrap())
    };
    println!("\x1b[32mRevoked \"{claim}\" from {target}\x1b[0m");
    println!("\x1b[2mSaved to: {}\x1b[0m", path.display());
    Ok(0)
}

fn roles(root: &Path, action: String, name: Option<String>) -> Result<u8, String> {
    // claims.ts:440 — `action || 'list'` (empty action is falsy).
    let action = if action.is_empty() {
        "list".into()
    } else {
        action
    };
    let name = non_empty(name);
    let (mut config, path) = load(root).map_err(|e| format!("Failed to manage roles: {e}"))?;
    match action.as_str() {
        "list" => {
            let roles = config
                .get("roles")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            if roles.is_empty() {
                println!();
                println!("\x1b[2mNo roles defined.\x1b[0m");
                return Ok(0);
            }
            println!();
            println!("\x1b[1mRoles\x1b[0m");
            let rows = roles
                .iter()
                .map(|(role_name, claims)| {
                    let claims = claim_list(claims);
                    vec![
                        role_name.clone(),
                        claims.len().to_string(),
                        claims.join(", "),
                    ]
                })
                .collect::<Vec<_>>();
            println!(
                "{}",
                table(&["Role", "Claims", "Claims List"], &rows, &[0, 0, 60])
            );
            println!("\x1b[2mConfig: {}\x1b[0m", path.display());
            Ok(0)
        }
        "show" => {
            let Some(name) = name else {
                return Err("Role name is required (use -n <name>)".into());
            };
            let Some(claims) = config.get("roles").and_then(|r| r.get(&name)) else {
                return Err(format!("Role \"{name}\" not found"));
            };
            let claims = claim_list(claims);
            println!();
            println!("\x1b[1mRole: {name}\x1b[0m");
            println!("\x1b[2m{}\x1b[0m", "─".repeat(40));
            println!("Claims ({}):", claims.len());
            for claim in &claims {
                println!("  {claim}");
            }
            Ok(0)
        }
        "create" => {
            let Some(name) = name else {
                return Err("Role name is required (use -n <name>)".into());
            };
            ensure_object(&mut config, "roles");
            let roles = config
                .get_mut("roles")
                .and_then(Value::as_object_mut)
                .expect("roles object");
            if roles.contains_key(&name) {
                return Err(format!("Role \"{name}\" already exists"));
            }
            roles.insert(name.clone(), json!([]));
            save(root, &config, &path).map_err(|e| format!("Failed to manage roles: {e}"))?;
            println!();
            println!("\x1b[32mCreated role \"{name}\"\x1b[0m");
            println!("\x1b[2mUse \"claims grant -c <claim> -r {name}\" to add claims.\x1b[0m");
            Ok(0)
        }
        "delete" => {
            let Some(name) = name else {
                return Err("Role name is required (use -n <name>)".into());
            };
            let Some(roles) = config.get_mut("roles").and_then(Value::as_object_mut) else {
                return Err(format!("Role \"{name}\" not found"));
            };
            if roles.remove(&name).is_none() {
                return Err(format!("Role \"{name}\" not found"));
            }
            save(root, &config, &path).map_err(|e| format!("Failed to manage roles: {e}"))?;
            println!();
            println!("\x1b[32mDeleted role \"{name}\"\x1b[0m");
            Ok(0)
        }
        other => Err(format!(
            "Unknown action \"{other}\". Use: list, create, delete, show"
        )),
    }
}

fn policies(root: &Path, action: String, name: Option<String>) -> Result<u8, String> {
    // claims.ts:548 — `action || 'list'` (empty action is falsy).
    let action = if action.is_empty() {
        "list".into()
    } else {
        action
    };
    let name = non_empty(name);
    let (mut config, path) = load(root).map_err(|e| format!("Failed to manage policies: {e}"))?;
    match action.as_str() {
        "list" => {
            println!();
            println!("\x1b[1mPolicies\x1b[0m");
            println!("\x1b[2m{}\x1b[0m", "─".repeat(50));

            let defaults = config
                .get("defaultClaims")
                .map(claim_list)
                .unwrap_or_default();
            println!();
            println!("\x1b[1mDefault Policy\x1b[0m");
            if defaults.is_empty() {
                println!("\x1b[2m  (no default claims)\x1b[0m");
            } else {
                for claim in &defaults {
                    println!("  {claim}");
                }
            }

            let roles = config
                .get("roles")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            if !roles.is_empty() {
                println!();
                println!("\x1b[1mRole-Based Policies\x1b[0m");
                let rows = roles
                    .iter()
                    .map(|(role_name, claims)| {
                        let claims = claim_list(claims);
                        vec![
                            role_name.clone(),
                            claims.len().to_string(),
                            preview(&claims, 4),
                        ]
                    })
                    .collect::<Vec<_>>();
                println!(
                    "{}",
                    table(&["Policy (Role)", "Claims", "Preview"], &rows, &[0, 0, 50])
                );
            }

            println!();
            println!("\x1b[2mConfig: {}\x1b[0m", path.display());
            Ok(0)
        }
        "create" => {
            let Some(name) = name else {
                return Err("Policy name is required (use -n <name>)".into());
            };
            ensure_object(&mut config, "roles");
            let roles = config
                .get_mut("roles")
                .and_then(Value::as_object_mut)
                .expect("roles object");
            if roles.contains_key(&name) {
                return Err(format!("Policy \"{name}\" already exists"));
            }
            roles.insert(name.clone(), json!([]));
            save(root, &config, &path).map_err(|e| format!("Failed to manage policies: {e}"))?;
            println!();
            println!("\x1b[32mCreated policy \"{name}\"\x1b[0m");
            println!("\x1b[2mUse \"claims grant -c <claim> -r {name}\" to add claims.\x1b[0m");
            Ok(0)
        }
        "delete" => {
            let Some(name) = name else {
                return Err("Policy name is required (use -n <name>)".into());
            };
            let Some(roles) = config.get_mut("roles").and_then(Value::as_object_mut) else {
                return Err(format!("Policy \"{name}\" not found"));
            };
            if roles.remove(&name).is_none() {
                return Err(format!("Policy \"{name}\" not found"));
            }
            save(root, &config, &path).map_err(|e| format!("Failed to manage policies: {e}"))?;
            println!();
            println!("\x1b[32mDeleted policy \"{name}\"\x1b[0m");
            Ok(0)
        }
        other => Err(format!(
            "Unknown action \"{other}\". Use: list, create, delete"
        )),
    }
}

/// V3 wildcard claim matcher (claims.ts:236-253): exact, `*`, `prefix:*`
/// (matches `prefix:anything`), `*:suffix` (matches `anything:suffix`). The colon
/// boundary is required in both wildcard forms — `swarm:*` must NOT match bare
/// `swarm`, and `*:list` must NOT match bare `list`.
fn check_claim(claim: &str, granted: &[String]) -> bool {
    for g in granted {
        if g == claim {
            return true;
        }
        if g == "*" {
            return true;
        }
        // claims.ts:242 — `granted.endsWith(':*')` → prefix keeps the colon.
        if let Some(prefix) = g.strip_suffix("*") {
            if prefix.ends_with(':') && claim.starts_with(prefix) {
                return true;
            }
        }
        // claims.ts:247 — `granted.startsWith('*:')` → suffix keeps the colon.
        if let Some(suffix) = g.strip_prefix("*") {
            if suffix.starts_with(':') && claim.ends_with(suffix) {
                return true;
            }
        }
    }
    false
}

fn claim_list(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|array| {
            array
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn preview(claims: &[String], limit: usize) -> String {
    let head: Vec<&str> = claims.iter().take(limit).map(String::as_str).collect();
    let mut preview = head.join(", ");
    if claims.len() > limit {
        preview.push_str(", ...");
    }
    preview
}

fn ensure_object(config: &mut Value, key: &str) {
    if !config.is_object() {
        *config = json!({});
    }
    if let Some(obj) = config.as_object_mut() {
        if !obj.get(key).is_some_and(Value::is_object) {
            obj.insert(key.to_string(), json!({}));
        }
    }
}

fn ensure_array(info: &mut Value, key: &str) {
    if !info.is_object() {
        *info = json!({});
    }
    if let Some(obj) = info.as_object_mut() {
        if !obj.get(key).is_some_and(Value::is_array) {
            obj.insert(key.to_string(), json!([]));
        }
    }
}

fn config_paths(root: &Path) -> Vec<PathBuf> {
    let mut paths = vec![
        root.join(".claude-flow/claims.json"),
        root.join("claude-flow.claims.json"),
    ];
    if let Some(home) = env::var_os("HOME") {
        paths.push(PathBuf::from(home).join(".config/claude-flow/claims.json"));
    }
    paths
}

fn default_config() -> Value {
    json!({
        "roles": {
            "admin": ["*"],
            "developer": ["swarm:*", "agent:*", "memory:*", "task:*", "session:*"],
            "operator": ["swarm:status", "agent:list", "memory:read", "task:list"],
            "viewer": ["*:list", "*:status", "*:read"]
        },
        "defaultClaims": [
            "swarm:create", "swarm:status", "agent:spawn",
            "agent:list", "memory:read", "memory:write", "task:create"
        ]
    })
}

/// V3 `loadClaimsConfig` semantics (claims.ts:32-53): first existing config path
/// wins; its JSON is parsed strictly (malformed JSON throws, surfacing as exit 1
/// via the caller's `Failed to ...` handler). Absent config yields the default
/// policy written to the first (project-local) path.
fn load(root: &Path) -> Result<(Value, PathBuf), String> {
    for path in config_paths(root) {
        if path.exists() {
            let contents = fs::read_to_string(&path)
                .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
            let parsed: Value = serde_json::from_str(&contents)
                .map_err(|e| format!("Unexpected token in {}: {}", path.display(), e))?;
            return Ok((parsed, path));
        }
    }
    let write_path = config_paths(root)
        .into_iter()
        .next()
        .unwrap_or_else(|| root.join(".claude-flow/claims.json"));
    Ok((default_config(), write_path))
}

/// V3 `check` inline policy resolution (claims.ts:196-218): start from the full
/// default policy, then shallow-merge the first existing config file over it. The
/// returned source string is the resolved policy path, or `"default"` when no file
/// exists. Errors propagate so `check` can fall back to its permissive policy.
fn load_merged_for_check(root: &Path) -> Result<(Value, String), String> {
    let mut merged = default_config();
    for path in config_paths(root) {
        if path.exists() {
            let contents = fs::read_to_string(&path)
                .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
            let parsed: Value = serde_json::from_str(&contents)
                .map_err(|e| format!("Unexpected token in {}: {}", path.display(), e))?;
            if let (Some(dst), Some(src)) = (merged.as_object_mut(), parsed.as_object()) {
                for (key, value) in src {
                    dst.insert(key.clone(), value.clone());
                }
            } else {
                merged = parsed;
            }
            return Ok((merged, path.display().to_string()));
        }
    }
    Ok((merged, "default".into()))
}

fn save(root: &Path, config: &Value, path: &Path) -> io::Result<()> {
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    if let Some(parent) = resolved.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = PathBuf::from(format!("{}.tmp", resolved.display()));
    let bytes = serde_json::to_vec_pretty(config).expect("claims config serializes");
    {
        let mut file = File::create(&tmp)?;
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
    }
    fs::rename(&tmp, &resolved)?;
    if let Ok(dir_file) = File::open(resolved.parent().unwrap_or(Path::new("."))) {
        let _ = dir_file.sync_all();
    }
    Ok(())
}

fn table(headers: &[&str], rows: &[Vec<String>], widths: &[usize]) -> String {
    let computed = headers
        .iter()
        .enumerate()
        .map(|(index, header)| {
            let natural = rows
                .iter()
                .filter_map(|row| row.get(index))
                .map(|cell| visible_len(cell))
                .max()
                .unwrap_or(0)
                .max(header.len());
            let cap = widths.get(index).copied().filter(|w| *w > 0).unwrap_or(0);
            if cap > 0 {
                natural.min(cap)
            } else {
                natural
            }
        })
        .collect::<Vec<_>>();
    let border = format!(
        "+{}+",
        computed
            .iter()
            .map(|width| "-".repeat(width + 2))
            .collect::<Vec<_>>()
            .join("+")
    );
    let row = |values: Vec<String>| {
        format!(
            "|{}|",
            values
                .into_iter()
                .enumerate()
                .map(|(index, value)| {
                    let width = computed[index];
                    let len = visible_len(&value);
                    let pad = width.saturating_sub(len);
                    format!(" {}{} ", value, " ".repeat(pad))
                })
                .collect::<Vec<_>>()
                .join("|")
        )
    };
    let mut lines = vec![
        border.clone(),
        row(headers.iter().map(|h| (*h).to_string()).collect()),
        border.clone(),
    ];
    lines.extend(rows.iter().cloned().map(row));
    lines.push(border);
    lines.join("\n")
}

fn boxed(title: &str, lines: &[String]) -> String {
    let inner = lines.len().max(1);
    let width = lines
        .iter()
        .map(|line| visible_len(line))
        .max()
        .unwrap_or(0)
        .max(title.len())
        .max(20);
    let top = format!("┌{}┐", "─".repeat(width + 2));
    let bottom = format!("└{}┘", "─".repeat(width + 2));
    let title_bar = format!("│ {:<width$} │", title, width = width);
    let sep = format!("├{}┤", "─".repeat(width + 2));
    let mut out = vec![top, title_bar, sep];
    for _ in 0..(inner.saturating_sub(lines.len())) {
        out.push(format!("│ {:<width$} │", "", width = width));
    }
    for line in lines {
        let pad = width.saturating_sub(visible_len(line));
        out.push(format!("│ {}{} │", line, " ".repeat(pad)));
    }
    out.push(bottom);
    out.join("\n") + "\n"
}

/// Visible length ignoring ANSI escape codes.
fn visible_len(value: &str) -> usize {
    let mut len = 0;
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            // Skip escape sequence: ESC [ ... letter
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            len += 1;
        }
    }
    len
}

const OVERVIEW: &str = "\n\x1b[1mRuFlo Claims System\x1b[0m\n\x1b[2mFine-grained authorization and access control\x1b[0m\n\nSubcommands:\n  list     - List claims and permissions\n  check    - Check if a claim is granted\n  grant    - Grant a claim to user or role\n  revoke   - Revoke a claim\n  roles    - Manage roles and their claims\n  policies - Manage claim policies\n\nClaim Types:\n  swarm:*   - Swarm operations (create, delete, scale)\n  agent:*   - Agent operations (spawn, terminate)\n  memory:*  - Memory operations (read, write, delete)\n  admin:*   - Administrative operations\n\n\x1b[2mCreated with ❤️ by ruv.io\x1b[0m\n";

fn help(bin: &str, subcommand: Option<&str>) -> String {
    match subcommand {
        Some("list") => format!("\n{bin} claims list\nList claims and permissions\n\nOPTIONS:\n  -u, --user <value>      Filter by user ID\n  -r, --role <value>      Filter by role\n      --resource <value>  Filter by resource\n\nEXAMPLES:\n  {bin} claims list                List all claims\n  {bin} claims list -u user123     List user claims\n"),
        Some("check") => format!("\n{bin} claims check\nCheck if a specific claim is granted\n\nOPTIONS:\n  -c, --claim <value>     Claim to check (required)\n  -u, --user <value>      User ID to check\n  -r, --resource <value>  Resource context\n\nEXAMPLES:\n  {bin} claims check -c swarm:create            Check swarm creation permission\n  {bin} claims check -c admin:delete -u user123 Check user permission\n"),
        Some("grant") => format!("\n{bin} claims grant\nGrant a claim to user or role\n\nOPTIONS:\n  -c, --claim <value>   Claim to grant (required)\n  -u, --user <value>    User ID\n  -r, --role <value>    Role name\n  -s, --scope <value>   Scope: global, namespace, resource [default: global]\n  -e, --expires <value> Expiration time (e.g., 24h, 7d)\n\nEXAMPLES:\n  {bin} claims grant -c swarm:create -u user123   Grant to user\n  {bin} claims grant -c agent:spawn -r developer Grant to role\n"),
        Some("revoke") => format!("\n{bin} claims revoke\nRevoke a claim from user or role\n\nOPTIONS:\n  -c, --claim <value>  Claim to revoke (required)\n  -u, --user <value>   User ID\n  -r, --role <value>   Role name\n\nEXAMPLES:\n  {bin} claims revoke -c swarm:delete -u user123 Revoke from user\n  {bin} claims revoke -c admin:* -r guest        Revoke from role\n"),
        Some("roles") => format!("\n{bin} claims roles\nManage roles and their claims\n\nOPTIONS:\n  -a, --action <value>  Action: list, create, delete, show [default: list]\n  -n, --name <value>    Role name\n\nEXAMPLES:\n  {bin} claims roles                  List all roles\n  {bin} claims roles -a show -n admin Show role details\n"),
        Some("policies") => format!("\n{bin} claims policies\nManage claim policies\n\nOPTIONS:\n  -a, --action <value>  Action: list, create, delete [default: list]\n  -n, --name <value>    Policy name\n\nEXAMPLES:\n  {bin} claims policies                       List policies\n  {bin} claims policies -a create -n rate-limit Create policy\n"),
        _ => format!("\n{bin} claims\nClaims-based authorization, permissions, and access control\n\nSUBCOMMANDS:\n  list      List claims and permissions\n  check     Check if a claim is granted\n  grant     Grant a claim to user or role\n  revoke    Revoke a claim\n  roles     Manage roles and their claims\n  policies  Manage claim policies\n\nEXAMPLES:\n  {bin} claims list                          List all claims\n  {bin} claims check -c swarm:create         Check permission\n  {bin} claims grant -c agent:spawn -r dev   Grant claim\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn wildcard_exact_match() {
        let _g = lock();
        assert!(check_claim("swarm:create", &["swarm:create".into()]));
    }

    #[test]
    fn wildcard_star_matches_all() {
        let _g = lock();
        assert!(check_claim("admin:delete", &["*".into()]));
    }

    #[test]
    fn wildcard_prefix_matches() {
        let _g = lock();
        assert!(check_claim("swarm:create", &["swarm:*".into()]));
        assert!(check_claim("agent:spawn", &["agent:*".into()]));
    }

    #[test]
    fn wildcard_suffix_matches() {
        let _g = lock();
        assert!(check_claim("swarm:list", &["*:list".into()]));
        assert!(check_claim("memory:list", &["*:list".into()]));
    }

    #[test]
    fn wildcard_no_match() {
        let _g = lock();
        assert!(!check_claim("admin:delete", &["swarm:*".into()]));
        assert!(!check_claim("swarm:create", &["*:list".into()]));
    }

    #[test]
    fn wildcard_requires_colon_boundary_no_overgrant() {
        let _g = lock();
        // claims.ts:242,247 — the colon boundary is mandatory.
        assert!(!check_claim("swarm", &["swarm:*".into()]));
        assert!(!check_claim("list", &["*:list".into()]));
        // `swarm.*` is not a V3 wildcard form and must not match.
        assert!(!check_claim("swarm.create", &["swarm:*".into()]));
        assert!(check_claim("swarm:create", &["swarm:*".into()]));
        assert!(check_claim("agent:list", &["*:list".into()]));
    }

    #[test]
    fn malformed_config_returns_error_not_default() {
        let _g = lock();
        let project = tempfile::tempdir().unwrap();
        let path = project.path().join(".claude-flow/claims.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{broken").unwrap();
        assert!(load(project.path()).is_err());
        assert!(load_merged_for_check(project.path()).is_err());
    }

    #[test]
    fn check_merges_defaults_so_users_only_config_still_grants_defaults() {
        let _g = lock();
        let project = tempfile::tempdir().unwrap();
        let path = project.path().join(".claude-flow/claims.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            json!({"users": {"alice": {"claims": ["custom:only"]}}}).to_string(),
        )
        .unwrap();
        let (merged, source) = load_merged_for_check(project.path()).unwrap();
        // defaultClaims retained after shallow merge.
        assert!(merged
            .get("defaultClaims")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c.as_str() == Some("swarm:create")));
        assert_eq!(source, path.display().to_string());
    }

    #[test]
    fn non_empty_treats_empty_and_flag_like_as_absent() {
        let _g = lock();
        assert_eq!(non_empty(Some("".into())), None);
        assert_eq!(non_empty(Some("-r".into())), None);
        assert_eq!(non_empty(Some("--role".into())), None);
        assert_eq!(non_empty(Some("alice".into())).as_deref(), Some("alice"));
        assert_eq!(non_empty(None), None);
    }

    #[test]
    fn default_config_has_expected_roles() {
        let _g = lock();
        let config = default_config();
        let roles = config.get("roles").and_then(Value::as_object).unwrap();
        assert!(roles.contains_key("admin"));
        assert!(roles.contains_key("developer"));
        let defaults = config.get("defaultClaims").unwrap().as_array().unwrap();
        assert!(defaults.len() >= 7);
    }

    #[test]
    fn grant_then_revoke_user_claim_round_trip() {
        let _g = lock();
        let project = tempfile::tempdir().unwrap();
        let root = project.path();

        grant(
            root,
            Some("swarm:create".into()),
            Some("user1".into()),
            None,
        )
        .unwrap();
        let (config, _) = load(root).unwrap();
        let claims = config
            .get("users")
            .and_then(|u| u.get("user1"))
            .and_then(|i| i.get("claims"))
            .and_then(Value::as_array)
            .unwrap();
        assert!(claims.iter().any(|c| c.as_str() == Some("swarm:create")));

        revoke(
            root,
            Some("swarm:create".into()),
            Some("user1".into()),
            None,
        )
        .unwrap();
        let (config, _) = load(root).unwrap();
        let claims = config
            .get("users")
            .and_then(|u| u.get("user1"))
            .and_then(|i| i.get("claims"))
            .and_then(Value::as_array);
        assert!(claims.is_none() || claims.unwrap().is_empty());

        // tmp file cleaned up by atomic rename
        assert!(!root.join(".claude-flow/claims.json.tmp").exists());
    }

    #[test]
    fn grant_requires_user_or_role() {
        let _g = lock();
        let project = tempfile::tempdir().unwrap();
        assert!(grant(project.path(), Some("swarm:create".into()), None, None).is_err());
        assert_eq!(
            run(
                project.path(),
                ClaimsCommand::Grant {
                    claim: Some("swarm:create".into()),
                    user: None,
                    role: None,
                    scope: "global".into(),
                    expires: None,
                }
            ),
            1
        );
    }

    #[test]
    fn grant_rejects_empty_target_values() {
        let _g = lock();
        let project = tempfile::tempdir().unwrap();
        // --user= (empty) is falsy, so both targets are absent.
        assert!(grant(
            project.path(),
            Some("agent:spawn".into()),
            Some(String::new()),
            None,
        )
        .is_err());
        assert!(!project.path().join(".claude-flow/claims.json").exists());
    }

    #[test]
    fn check_granted_via_default_policy() {
        let _g = lock();
        let project = tempfile::tempdir().unwrap();
        let code = check(project.path(), Some("swarm:create".into()), None, None).unwrap();
        assert_eq!(code, 0);
    }

    #[test]
    fn check_denied_admin_claim() {
        let _g = lock();
        let project = tempfile::tempdir().unwrap();
        let code = check(project.path(), Some("admin:delete".into()), None, None).unwrap();
        assert_eq!(code, 1);
    }

    #[test]
    fn roles_create_show_delete() {
        let _g = lock();
        let project = tempfile::tempdir().unwrap();
        let root = project.path();
        roles(root, "create".into(), Some("auditor".into())).unwrap();
        let (config, _) = load(root).unwrap();
        assert!(config.get("roles").and_then(|r| r.get("auditor")).is_some());
        roles(root, "delete".into(), Some("auditor".into())).unwrap();
        let (config, _) = load(root).unwrap();
        assert!(config.get("roles").and_then(|r| r.get("auditor")).is_none());
    }

    #[test]
    fn project_config_precedence_over_home() {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let home_claims = home.path().join(".config/claude-flow/claims.json");
        fs::create_dir_all(home_claims.parent().unwrap()).unwrap();
        fs::write(
            &home_claims,
            json!({"defaultClaims": ["home:claim"]}).to_string(),
        )
        .unwrap();
        let project_claims = project.path().join(".claude-flow/claims.json");
        fs::create_dir_all(project_claims.parent().unwrap()).unwrap();
        fs::write(
            &project_claims,
            json!({"defaultClaims": ["project:claim"]}).to_string(),
        )
        .unwrap();

        // SAFETY: tests run single-threaded by default for env-mutating tests; this
        // sets HOME only for the duration of this test via a guard.
        struct HomeGuard {
            previous: Option<std::ffi::OsString>,
        }
        impl Drop for HomeGuard {
            fn drop(&mut self) {
                let _g = lock();
                match &self.previous {
                    Some(v) => env::set_var("HOME", v),
                    None => env::remove_var("HOME"),
                }
            }
        }
        let _guard = HomeGuard {
            previous: env::var_os("HOME"),
        };
        env::set_var("HOME", home.path());

        let (config, path) = load(project.path()).unwrap();
        assert_eq!(path, project.path().join(".claude-flow/claims.json"));
        let defaults = config.get("defaultClaims").unwrap().as_array().unwrap();
        assert!(defaults.iter().any(|c| c.as_str() == Some("project:claim")));
    }
}
