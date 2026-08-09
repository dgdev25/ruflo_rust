//! Byte-parity fixture existence for the native capabilities added in the
//! zero-node-dependency plan (sona, bandit route, ipfs, embeddings ingest, auth pkce).
//!
//! These files are the reference shapes the CLI must emit; if a refactor
//! changes the output shape, update the fixture intentionally.

use std::path::Path;

#[test]
fn route_task_fixture_present() {
    let p = Path::new("tests/fixtures/cli/route/task.json");
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    assert_eq!(v["agent"].as_str(), Some("coder"));
    assert!(v["samples"].is_array());
}

#[test]
fn route_feedback_fixture_present() {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(
        "tests/fixtures/cli/route/feedback.json").unwrap()).unwrap();
    assert_eq!(v["feedback"].as_bool(), Some(true));
    assert_eq!(v["updated"].as_bool(), Some(true));
}

#[test]
fn transfer_store_publish_fixture_present() {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(
        "tests/fixtures/cli/transfer-store/publish.json").unwrap()).unwrap();
    assert!(v["cid"].as_str().unwrap().starts_with("bafy"));
    assert!(v["size"].as_u64().is_some());
}

#[test]
fn neural_train_fixture_present() {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(
        "tests/fixtures/cli/neural/train.json").unwrap()).unwrap();
    assert_eq!(v["backend"].as_str(), Some("native-sona"));
}

#[test]
fn embeddings_ingest_fixture_present() {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(
        "tests/fixtures/cli/embeddings/ingest.json").unwrap()).unwrap();
    assert_eq!(v["backend"].as_str(), Some("ruvector-rvf-hnsw"));
}

#[test]
fn auth_login_pkce_fixture_present() {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(
        "tests/fixtures/cli/auth/login.json").unwrap()).unwrap();
    assert_eq!(v["codeChallengeMethod"].as_str(), Some("S256"));
    let kv = &v["knownVector"];
    assert!(!kv["verifier"].as_str().unwrap().is_empty());
    assert!(!kv["challenge"].as_str().unwrap().is_empty());
}
