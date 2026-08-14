//! Consumer fixture for the native appliance host (REQ-013 / ADR-0012).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};

#[test]
fn appliance_build_records_host_checksum() {
    let project = tempfile::tempdir().unwrap();
    fs::create_dir_all(project.path().join(".claude-flow")).unwrap();
    fs::write(project.path().join(".claude-flow/config.yaml"), "version: 3\n").unwrap();
    let out = run(project.path(), &["appliance", "build", "-o", "box.rvfa", "--profile", "cloud"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let rvfa: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(project.path().join("box.rvfa")).unwrap()).unwrap();
    assert_eq!(rvfa["manifest"]["format"], "rvfa");
    assert_eq!(rvfa["manifest"]["profile"], "cloud");
    assert!(rvfa["manifest"]["host"]["sha256"].as_str().unwrap().len() == 64);
    assert!(!rvfa["checksum"].as_str().unwrap().is_empty());
}

#[test]
fn agents_and_jobs_use_sqlite_store() {
    let project = tempfile::tempdir().unwrap();
    let store = ruflo_storage::ApplianceStore::open(project.path()).unwrap();
    store
        .upsert_agent(&ruflo_storage::AgentRow {
            id: "resident-map".into(),
            agent_type: "map".into(),
            status: "resident-idle".into(),
            role: "map".into(),
            heartbeat_ms: 1,
        })
        .unwrap();
    store.enqueue_job("audit", "fixture").unwrap();
    drop(store);
    let reopened = ruflo_storage::ApplianceStore::open(project.path()).unwrap();
    assert_eq!(reopened.list_agents().unwrap().len(), 1);
    assert!(reopened.claim_job().unwrap().is_some());
    assert!(project.path().join(".swarm/memory.db").is_file());
    assert!(!project.path().join(".swarm/agents").exists()
        || project.path().join(".swarm/agents").read_dir().map(|d| d.count()).unwrap_or(0) == 0);
}

#[test]
fn appliance_verify_fails_closed_on_tampered_checksum() {
    let project = tempfile::tempdir().unwrap();
    fs::create_dir_all(project.path().join(".claude-flow")).unwrap();
    fs::write(project.path().join(".claude-flow/config.yaml"), "version: 3\n").unwrap();
    let build = run(project.path(), &["appliance", "build", "-o", "box.rvfa", "--profile", "cloud"]);
    assert_eq!(build.status.code(), Some(0), "{}", stderr(&build));
    let ok = run(project.path(), &["appliance", "verify", "--file", "box.rvfa", "--quick"]);
    assert_eq!(ok.status.code(), Some(0), "{}", stderr(&ok));
    let path = project.path().join("box.rvfa");
    let mut rvfa: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    rvfa["checksum"] = serde_json::json!("0".repeat(64));
    fs::write(&path, serde_json::to_vec_pretty(&rvfa).unwrap()).unwrap();
    let bad = run(project.path(), &["appliance", "verify", "--file", "box.rvfa", "--quick"]);
    assert_ne!(bad.status.code(), Some(0), "tampered checksum must fail");
    let run_bad = run(project.path(), &["appliance", "run", "--file", "box.rvfa"]);
    assert_ne!(run_bad.status.code(), Some(0), "run must refuse tampered host/checksum");
}

#[test]
fn spend_pause_blocks_swarm_spawn() {
    let project = tempfile::tempdir().unwrap();
    let budget = tempfile::tempdir().unwrap();
    let exe = executable();
    for args in [vec!["init"], vec!["swarm", "init"]] {
        let out = Command::new(&exe)
            .current_dir(project.path())
            .args(&args)
            .env("NO_COLOR", "1")
            .env("RUFLO_AI_BUDGET_DIR", budget.path())
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{args:?} {}", String::from_utf8_lossy(&out.stderr));
    }
    let pause = Command::new(&exe)
        .current_dir(project.path())
        .args(["daemon", "budget", "pause", "--reason", "fixture"])
        .env("NO_COLOR", "1")
        .env("RUFLO_AI_BUDGET_DIR", budget.path())
        .output()
        .unwrap();
    assert_eq!(pause.status.code(), Some(0), "{}", String::from_utf8_lossy(&pause.stderr));
    let swarm = Command::new(&exe)
        .current_dir(project.path())
        .args(["swarm", "start", "--objective", "fixture", "--workers", "1"])
        .env("NO_COLOR", "1")
        .env("RUFLO_AI_BUDGET_DIR", budget.path())
        .output()
        .unwrap();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&swarm.stdout),
        String::from_utf8_lossy(&swarm.stderr)
    );
    assert!(
        swarm.status.code() != Some(0) || combined.contains("budget") || combined.contains("paused"),
        "paused spend must block swarm: {combined}"
    );
}

#[test]
fn spend_ledger_is_shared_and_fail_closed() {
    let budget = tempfile::tempdir().unwrap();
    std::env::set_var("RUFLO_AI_BUDGET_DIR", budget.path());
    let spend = ruflo_storage::SpendLedger::open(budget.path()).unwrap();
    spend.reserve("swarm", "claude", "/tmp").unwrap();
    assert!(spend.reserve("headless", "claude", "/tmp").is_err());
    spend.pause("fixture").unwrap();
    assert!(spend.check().is_err());
    std::env::remove_var("RUFLO_AI_BUDGET_DIR");
}

#[test]
fn cloud_profile_is_packed_in_tree() {
    let profile = Path::new(env!("CARGO_MANIFEST_DIR")).join("config/appliance/cloud.yaml");
    let raw = fs::read_to_string(profile).unwrap();
    assert!(raw.contains("profile: cloud"));
    assert!(raw.contains("store: sqlite"));
}

fn run(cwd: &Path, args: &[&str]) -> Output {
    let _g = LOCK.lock().unwrap();
    Command::new(executable())
        .current_dir(cwd)
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .unwrap()
}

static LOCK: Mutex<()> = Mutex::new(());

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn executable() -> PathBuf {
    static BUILT: OnceLock<Mutex<bool>> = OnceLock::new();
    let mut built = BUILT.get_or_init(|| Mutex::new(false)).lock().unwrap();
    if !*built {
        let s = Command::new(env!("CARGO"))
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args(["build", "--quiet", "--package", "ruflo", "--bin", "ruflo"])
            .status()
            .unwrap();
        assert!(s.success());
        *built = true;
    }
    std::env::var_os("CARGO_BIN_EXE_ruflo")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/debug")
                .join("ruflo")
        })
}
