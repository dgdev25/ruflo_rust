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
    // Direct store contract — the consumer depends on this file, not JSON sidecars.
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
    assert_eq!(store.list_agents().unwrap().len(), 1);
    assert!(store.claim_job().unwrap().is_some());
    assert!(project.path().join(".swarm/memory.db").is_file());
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
