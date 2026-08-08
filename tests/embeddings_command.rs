//! End-to-end `embeddings` command tests through both native binaries (ADR-016).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};

#[test]
fn overview_lists_subcommands() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        let out = run(binary, project.path(), &["embeddings"]);
        assert_eq!(out.status.code(), Some(0));
        let s = stdout(&out);
        for sub in ["generate", "compare", "providers", "hyperbolic"] {
            assert!(s.contains(sub), "{binary}: overview missing '{sub}'");
        }
    }
}

#[test]
fn generate_preview_and_json() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        let out = run(binary, project.path(), &["embeddings", "generate", "-t", "hello world"]);
        assert_eq!(out.status.code(), Some(0));
        let s = stdout(&out);
        assert!(s.contains("Dimensions: 384"));
        assert!(s.contains("Vector preview"));
    }
    // JSON shape.
    let project = tempfile::tempdir().unwrap();
    let out = run("ruflo", project.path(), &["embeddings", "generate", "-t", "test", "-o", "json"]);
    let s = stdout(&out);
    let start = s.find('{').unwrap();
    let v: serde_json::Value = serde_json::from_str(&s[start..]).unwrap();
    assert_eq!(v["dimensions"], 384);
    assert!(v["embedding"].as_array().unwrap().len() == 384);
}

#[test]
fn generate_requires_text() {
    let project = tempfile::tempdir().unwrap();
    let out = run("ruflo", project.path(), &["embeddings", "generate"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("Text is required"));
}

#[test]
fn compare_identical_is_one() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        let out = run(
            binary,
            project.path(),
            &["embeddings", "compare", "--text1", "the cat sat", "--text2", "the cat sat"],
        );
        assert_eq!(out.status.code(), Some(0));
        assert!(stdout(&out).contains("1.000000"));
    }
}

#[test]
fn compare_metric_validation() {
    let project = tempfile::tempdir().unwrap();
    let out = run(
        "ruflo",
        project.path(),
        &["embeddings", "compare", "--text1", "a", "--text2", "b", "-m", "bogus"],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("Unknown metric"));
}

#[test]
fn init_writes_config() {
    let project = tempfile::tempdir().unwrap();
    let out = run("ruflo", project.path(), &["embeddings", "init"]);
    assert_eq!(out.status.code(), Some(0));
    let cfg = project.path().join(".claude-flow/embeddings-config.json");
    assert!(cfg.is_file());
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(cfg).unwrap()).unwrap();
    assert_eq!(v["dimensions"], 384);
}

#[test]
fn benchmark_reports_throughput() {
    let project = tempfile::tempdir().unwrap();
    let out = run("ruflo", project.path(), &["embeddings", "benchmark", "--limit", "20"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).contains("embeddings/sec"));
}

#[test]
fn search_degrades_without_store() {
    let project = tempfile::tempdir().unwrap();
    let out = run("ruflo", project.path(), &["embeddings", "search", "-q", "x"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("Memory store not found") || stderr(&out).contains("Node"));
}

#[test]
fn providers_lists_local() {
    let project = tempfile::tempdir().unwrap();
    let out = run("ruflo", project.path(), &["embeddings", "providers"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).contains("local"));
    assert!(stdout(&out).contains("openai"));
}

#[test]
fn hyperbolic_convert_into_ball() {
    let project = tempfile::tempdir().unwrap();
    let out = run(
        "ruflo",
        project.path(),
        &["embeddings", "hyperbolic", "-a", "convert", "-t", "some text"],
    );
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).contains("inside ball"));
}

#[test]
fn binary_parity_overview() {
    let project = tempfile::tempdir().unwrap();
    let a = run("ruflo", project.path(), &["embeddings"]);
    let b = run("claude-flow", project.path(), &["embeddings"]);
    assert_eq!(stdout(&a), stdout(&b));
}

// ---- helpers ----------------------------------------------------------------

fn run(binary: &str, cwd: &Path, args: &[&str]) -> Output {
    let _g = LOCK.lock().unwrap();
    Command::new(executable(binary))
        .current_dir(cwd)
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .unwrap()
}
static LOCK: Mutex<()> = Mutex::new(());
fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}
fn stderr(o: &Output) -> String {
    String::from_utf8(o.stderr.clone()).unwrap()
}
fn executable(binary: &str) -> PathBuf {
    static BUILT: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    let mut built = BUILT.get_or_init(|| Mutex::new(Vec::new())).lock().unwrap();
    if !built.iter().any(|n| n == binary) {
        let s = Command::new(env!("CARGO"))
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args(["build", "--quiet", "--package", binary, "--bin", binary])
            .status()
            .unwrap();
        assert!(s.success());
        built.push(binary.into());
    }
    std::env::var_os(format!("CARGO_BIN_EXE_{}", binary.replace('-', "_")))
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/debug")
                .join(binary)
        })
}
