//! End-to-end claims command tests through both native binaries.
//!
//! Exercises the V3 contract surface: config precedence, wildcard evaluation,
//! atomic mutation, grant/revoke round trips, roles/policies CRUD, and the
//! required-value error paths. See `v3/@claude-flow/cli/src/commands/claims.ts`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

#[test]
fn both_binaries_run_claims_overview_and_help() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        let overview = run(binary, project.path(), &["claims"]);
        assert_success(&overview);
        assert!(stdout(&overview).contains("RuFlo Claims System"));
        assert!(stdout(&overview).contains("policies"));

        let help = run(binary, project.path(), &["claims", "--help"]);
        assert_success(&help);
        assert!(stdout(&help).contains("SUBCOMMANDS"));
        assert!(stdout(&help).contains("grant"));

        let grant_help = run(binary, project.path(), &["claims", "grant", "-h"]);
        assert_success(&grant_help);
        assert!(stdout(&grant_help).contains("--claim"));
        assert!(stdout(&grant_help).contains("--expires"));
    }
}

#[test]
fn both_binaries_check_default_policy_and_deny_admin_without_grant() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();

        let granted = run(
            binary,
            project.path(),
            &["claims", "check", "-c", "swarm:create"],
        );
        assert_eq!(granted.status.code(), Some(0));
        assert!(stdout(&granted).contains("Claim granted"));
        assert!(stdout(&granted).contains("swarm:create"));

        let denied = run(
            binary,
            project.path(),
            &["claims", "check", "-c", "admin:delete"],
        );
        assert_eq!(denied.status.code(), Some(1));
        assert!(stdout(&denied).contains("Claim denied"));
    }
}

#[test]
fn both_binaries_grant_requires_claim_and_target() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();

        let missing_target = run(
            binary,
            project.path(),
            &["claims", "grant", "-c", "swarm:create"],
        );
        assert_eq!(missing_target.status.code(), Some(1));
        assert_eq!(
            stderr(&missing_target),
            "[ERROR] Either user or role is required\n"
        );

        let missing_claim = run(binary, project.path(), &["claims", "grant", "-u", "user1"]);
        assert_eq!(missing_claim.status.code(), Some(1));
        assert_eq!(stderr(&missing_claim), "[ERROR] Claim is required\n");
    }
}

#[test]
fn both_binaries_grant_and_revoke_user_and_role_claims_atomically() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();

        let grant_user = run(
            binary,
            project.path(),
            &["claims", "grant", "-c", "admin:delete", "-u", "root"],
        );
        assert_success(&grant_user);
        assert!(stdout(&grant_user).contains("Granted \"admin:delete\" to user \"root\""));
        assert!(stdout(&grant_user).contains("Saved to"));
        assert_atomic_state(project.path());

        let state = read_state(project.path());
        let user_claims = state["users"]["root"]["claims"].as_array().unwrap();
        assert!(user_claims
            .iter()
            .any(|c| c.as_str() == Some("admin:delete")));

        let grant_role = run(
            binary,
            project.path(),
            &["claims", "grant", "-c", "agent:spawn", "-r", "bot"],
        );
        assert_success(&grant_role);
        let state = read_state(project.path());
        let role_claims = state["roles"]["bot"].as_array().unwrap();
        assert!(role_claims
            .iter()
            .any(|c| c.as_str() == Some("agent:spawn")));

        let check_root = run(
            binary,
            project.path(),
            &["claims", "check", "-c", "admin:delete", "-u", "root"],
        );
        assert_eq!(check_root.status.code(), Some(0));
        assert!(stdout(&check_root).contains("GRANTED"));

        let revoke_user = run(
            binary,
            project.path(),
            &["claims", "revoke", "-c", "admin:delete", "-u", "root"],
        );
        assert_success(&revoke_user);
        assert!(stdout(&revoke_user).contains("Revoked \"admin:delete\" from user \"root\""));
        let state = read_state(project.path());
        let remaining = state["users"]["root"]["claims"].as_array();
        assert!(
            remaining.is_none()
                || remaining
                    .unwrap()
                    .iter()
                    .all(|c| c.as_str() != Some("admin:delete"))
        );

        let revoke_missing = run(
            binary,
            project.path(),
            &["claims", "revoke", "-c", "nope:nothing", "-u", "root"],
        );
        assert_eq!(revoke_missing.status.code(), Some(1));
        assert!(stderr(&revoke_missing).contains("not found"));
    }
}

#[test]
fn both_binaries_manage_roles_and_policies_crud() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();

        let create = run(
            binary,
            project.path(),
            &["claims", "roles", "-a", "create", "-n", "auditor"],
        );
        assert_success(&create);
        assert!(stdout(&create).contains("Created role \"auditor\""));
        assert!(read_state(project.path())["roles"].get("auditor").is_some());

        let duplicate = run(
            binary,
            project.path(),
            &["claims", "roles", "-a", "create", "-n", "auditor"],
        );
        assert_eq!(duplicate.status.code(), Some(1));
        assert!(stderr(&duplicate).contains("already exists"));

        let list = run(binary, project.path(), &["claims", "roles"]);
        assert_success(&list);
        assert!(stdout(&list).contains("auditor"));

        let show = run(
            binary,
            project.path(),
            &["claims", "roles", "-a", "show", "-n", "auditor"],
        );
        assert_success(&show);
        assert!(stdout(&show).contains("Role: auditor"));

        let show_missing = run(
            binary,
            project.path(),
            &["claims", "roles", "-a", "show", "-n", "ghost"],
        );
        assert_eq!(show_missing.status.code(), Some(1));
        assert!(stderr(&show_missing).contains("not found"));

        let unknown = run(
            binary,
            project.path(),
            &["claims", "roles", "-a", "frobnicate", "-n", "x"],
        );
        assert_eq!(unknown.status.code(), Some(1));
        assert!(stderr(&unknown).contains("Unknown action"));

        let policy_create = run(
            binary,
            project.path(),
            &["claims", "policies", "-a", "create", "-n", "rate-limit"],
        );
        assert_success(&policy_create);
        assert!(stdout(&policy_create).contains("Created policy \"rate-limit\""));

        let policy_list = run(binary, project.path(), &["claims", "policies"]);
        assert_success(&policy_list);
        assert!(stdout(&policy_list).contains("rate-limit"));

        let policy_delete = run(
            binary,
            project.path(),
            &["claims", "policies", "-a", "delete", "-n", "rate-limit"],
        );
        assert_success(&policy_delete);
        assert!(read_state(project.path())["roles"]
            .get("rate-limit")
            .is_none());

        let role_delete = run(
            binary,
            project.path(),
            &["claims", "roles", "-a", "delete", "-n", "auditor"],
        );
        assert_success(&role_delete);
        assert!(read_state(project.path())["roles"].get("auditor").is_none());
    }
}

#[test]
fn both_binaries_list_reports_loaded_config_path() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        run(
            binary,
            project.path(),
            &["claims", "grant", "-c", "memory:write", "-u", "alice"],
        );
        let list = run(binary, project.path(), &["claims", "list"]);
        assert_success(&list);
        assert!(stdout(&list).contains("alice"));
        assert!(stdout(&list).contains("memory:write"));
    }
}

#[test]
fn project_claims_precedence_over_home_config() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join(".claude-flow")).unwrap();
        std::fs::write(
            project.path().join(".claude-flow/claims.json"),
            serde_json::json!({"defaultClaims": ["project:only"]}).to_string(),
        )
        .unwrap();

        // project:only is granted, admin:delete is not (defaults overridden).
        let granted = run(
            binary,
            project.path(),
            &["claims", "check", "-c", "project:only"],
        );
        assert_eq!(granted.status.code(), Some(0));

        let denied = run(
            binary,
            project.path(),
            &["claims", "check", "-c", "swarm:create"],
        );
        assert_eq!(denied.status.code(), Some(1));
    }
}

#[test]
fn malformed_config_exits_one_not_zero_or_panic() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join(".claude-flow")).unwrap();
        std::fs::write(project.path().join(".claude-flow/claims.json"), "{broken").unwrap();

        let list = run(binary, project.path(), &["claims", "list"]);
        assert_eq!(list.status.code(), Some(1));
        assert!(stderr(&list).starts_with("[ERROR] Failed to list claims"));

        let grant = run(
            binary,
            project.path(),
            &["claims", "grant", "-c", "swarm:create", "-u", "x"],
        );
        assert_eq!(grant.status.code(), Some(1));
    }
}

#[test]
fn empty_target_values_are_rejected_like_typescript_falsy() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        let empty_user = run(
            binary,
            project.path(),
            &["claims", "grant", "--claim=agent:spawn", "--user="],
        );
        assert_eq!(empty_user.status.code(), Some(1));
        assert_eq!(
            stderr(&empty_user),
            "[ERROR] Either user or role is required\n"
        );
        // No state file created for a rejected mutation.
        assert!(!project.path().join(".claude-flow/claims.json").exists());

        let empty_role = run(
            binary,
            project.path(),
            &["claims", "roles", "-a", "create", "-n", ""],
        );
        assert_eq!(empty_role.status.code(), Some(1));
    }
}

#[test]
fn wildcard_evaluation_requires_colon_boundary_and_check_merges_defaults() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join(".claude-flow")).unwrap();
        std::fs::write(
            project.path().join(".claude-flow/claims.json"),
            serde_json::json!({
                "users": {
                    "dev": { "claims": ["swarm:*"] },
                    "lister": { "claims": ["*:list"] }
                }
            })
            .to_string(),
        )
        .unwrap();

        // swarm:* matches swarm:create but NOT bare swarm.
        let granted = run(
            binary,
            project.path(),
            &["claims", "check", "-c", "swarm:create", "-u", "dev"],
        );
        assert_eq!(granted.status.code(), Some(0));

        let bare = run(
            binary,
            project.path(),
            &["claims", "check", "-c", "swarm", "-u", "dev"],
        );
        assert_eq!(bare.status.code(), Some(1));

        // *:list matches agent:list but NOT bare list.
        let list_granted = run(
            binary,
            project.path(),
            &["claims", "check", "-c", "agent:list", "-u", "lister"],
        );
        assert_eq!(list_granted.status.code(), Some(0));

        let bare_list = run(
            binary,
            project.path(),
            &["claims", "check", "-c", "list", "-u", "lister"],
        );
        assert_eq!(bare_list.status.code(), Some(1));

        // check merges defaults: a users-only config still grants defaultClaims.
        let default_granted = run(
            binary,
            project.path(),
            &["claims", "check", "-c", "swarm:create"],
        );
        assert_eq!(default_granted.status.code(), Some(0));
    }
}

#[test]
fn help_is_binary_aware() {
    let project = tempfile::tempdir().unwrap();
    let ruflo_help = run("ruflo", project.path(), &["claims", "grant", "--help"]);
    assert_success(&ruflo_help);
    assert!(stdout(&ruflo_help).contains("ruflo claims grant"));
    assert!(!stdout(&ruflo_help).contains("claude-flow claims grant"));

    let cf_help = run(
        "claude-flow",
        project.path(),
        &["claims", "grant", "--help"],
    );
    assert_success(&cf_help);
    assert!(stdout(&cf_help).contains("claude-flow claims grant"));
    assert!(!stdout(&cf_help).contains("ruflo claims grant"));
}

#[test]
fn empty_action_falls_back_to_list() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        run(
            binary,
            project.path(),
            &["claims", "roles", "-a", "create", "-n", "auditor"],
        );
        // `--action=` (empty) is falsy → defaults to list, exits 0, shows the role.
        let list = run(binary, project.path(), &["claims", "roles", "--action="]);
        assert_success(&list);
        assert!(stdout(&list).contains("auditor"));

        let policies_list = run(binary, project.path(), &["claims", "policies", "--action="]);
        assert_success(&policies_list);
    }
}

#[test]
fn wrong_shaped_existing_role_does_not_panic() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join(".claude-flow")).unwrap();
        std::fs::write(
            project.path().join(".claude-flow/claims.json"),
            serde_json::json!({"roles": {"dev": {}}}).to_string(),
        )
        .unwrap();
        // Previously panicked (exit 101); must now normalize and grant.
        let grant = run(
            binary,
            project.path(),
            &["claims", "grant", "-c", "x:y", "-r", "dev"],
        );
        assert_eq!(grant.status.code(), Some(0));
        let state: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(project.path().join(".claude-flow/claims.json")).unwrap(),
        )
        .unwrap();
        assert!(state["roles"]["dev"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c.as_str() == Some("x:y")));
    }
}

fn assert_success(output: &Output) {
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(output));
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

fn read_state(root: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(root.join(".claude-flow/claims.json")).unwrap())
        .unwrap()
}

fn assert_atomic_state(root: &Path) {
    let state = root.join(".claude-flow/claims.json");
    assert!(state.is_file());
    assert!(
        !PathBuf::from(format!("{}.tmp", state.display())).exists(),
        "atomic rename left a .tmp sibling"
    );
}

fn run(binary: &str, root: &Path, args: &[&str]) -> Output {
    Command::new(executable(binary))
        .current_dir(root)
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .unwrap()
}

fn executable(binary: &str) -> PathBuf {
    static BUILT: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    let mut built = BUILT.get_or_init(|| Mutex::new(Vec::new())).lock().unwrap();
    if !built.iter().any(|name| name == binary) {
        let status = Command::new(env!("CARGO"))
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args(["build", "--quiet", "--package", binary, "--bin", binary])
            .status()
            .unwrap();
        assert!(status.success(), "failed to build {binary}");
        built.push(binary.to_string());
    }
    std::env::var_os(format!("CARGO_BIN_EXE_{}", binary.replace('-', "_")))
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/debug")
                .join(binary)
        })
}
