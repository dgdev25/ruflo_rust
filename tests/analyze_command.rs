//! End-to-end `analyze` command tests through both native binaries (ADR-016).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};

fn write_fixture(dir: &Path) {
    // Two files with a circular import + a function and a TODO.
    fs::write(
        dir.join("a.ts"),
        "import { b } from './b';\n// TODO: fix this\nexport function alpha() { return b; }\n",
    )
    .unwrap();
    fs::write(
        dir.join("b.ts"),
        "import { a } from './a';\nexport function beta() { return a; }\n",
    )
    .unwrap();
}

#[test]
fn overview_lists_subcommands() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        let out = run(binary, project.path(), &["analyze"]);
        assert_eq!(out.status.code(), Some(0));
        let s = stdout(&out);
        for sub in [
            "diff", "code", "deps", "ast", "complexity", "symbols", "imports",
            "boundaries", "modules", "dependencies", "circular",
        ] {
            assert!(s.contains(sub), "{binary}: overview missing '{sub}'");
        }
    }
}

#[test]
fn code_quality_counts_files_and_functions() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        write_fixture(project.path());
        let out = run(binary, project.path(), &["analyze", "code", "-p", "."]);
        assert_eq!(out.status.code(), Some(0));
        let s = stdout(&out);
        assert!(s.contains("Files: 2"));
        assert!(s.contains("Functions: 2"));
        assert!(s.contains("TODO/FIXME: 1"));
    }
}

#[test]
fn code_json_shape() {
    let project = tempfile::tempdir().unwrap();
    write_fixture(project.path());
    let out = run("ruflo", project.path(), &["analyze", "code", "-p", ".", "-f", "json"]);
    assert_eq!(out.status.code(), Some(0));
    let s = stdout(&out);
    let json_start = s.find('{').expect("code json missing object");
    let v: serde_json::Value = serde_json::from_str(&s[json_start..]).unwrap();
    assert_eq!(v["files"], 2);
    assert_eq!(v["totalFunctions"], 2);
}

#[test]
fn circular_detects_cycle() {
    for binary in ["ruflo", "claude-flow"] {
        let project = tempfile::tempdir().unwrap();
        write_fixture(project.path());
        let out = run(binary, project.path(), &["analyze", "circular", "."]);
        assert_eq!(out.status.code(), Some(0));
        let s = stdout(&out);
        assert!(s.contains("Cycles found: 1"), "{binary}: {s}");
        assert!(s.contains("a.ts -> b.ts -> a.ts"));
    }
}

#[test]
fn symbols_extracts_functions() {
    let project = tempfile::tempdir().unwrap();
    write_fixture(project.path());
    let out = run("ruflo", project.path(), &["analyze", "symbols", "."]);
    assert_eq!(out.status.code(), Some(0));
    let s = stdout(&out);
    assert!(s.contains("alpha"));
    assert!(s.contains("beta"));
}

#[test]
fn symbols_filter_function_only() {
    let project = tempfile::tempdir().unwrap();
    write_fixture(project.path());
    let out = run("ruflo", project.path(), &["analyze", "symbols", ".", "--type", "function"]);
    assert_eq!(out.status.code(), Some(0));
    let s = stdout(&out);
    assert!(s.contains("alpha") && s.contains("beta"));
}

#[test]
fn imports_lists_local() {
    let project = tempfile::tempdir().unwrap();
    write_fixture(project.path());
    let out = run("ruflo", project.path(), &["analyze", "imports", "."]);
    assert_eq!(out.status.code(), Some(0));
    let s = stdout(&out);
    assert!(s.contains("Local (relative): 2"));
}

#[test]
fn dependencies_dot_format() {
    let project = tempfile::tempdir().unwrap();
    write_fixture(project.path());
    let out = run("ruflo", project.path(), &["analyze", "dependencies", ".", "-f", "dot"]);
    assert_eq!(out.status.code(), Some(0));
    let s = stdout(&out);
    assert!(s.contains("digraph dependencies"));
    assert!(s.contains("a.ts\" -> \"b.ts"));
}

#[test]
fn complexity_flags_over_threshold() {
    let project = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join("c.ts"),
        "function big() {\n  if (a) { if (b) { if (c) { if (d) { if (e) { if (f) {} } } } } }\n}\n",
    )
    .unwrap();
    let out = run("ruflo", project.path(), &["analyze", "complexity", ".", "--threshold", "3"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).contains("c.ts"));
}

#[test]
fn deps_requires_package_json() {
    let project = tempfile::tempdir().unwrap();
    for binary in ["ruflo", "claude-flow"] {
        let out = run(binary, project.path(), &["analyze", "deps"]);
        assert_eq!(out.status.code(), Some(1));
        assert!(stderr(&out).contains("No package.json"));
    }
}

#[test]
fn deps_summary_from_package_json() {
    let project = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join("package.json"),
        r#"{"name":"x","version":"1.0.0","dependencies":{"react":"^18.0.0"},"devDependencies":{"vitest":"^1.0.0"}}"#,
    )
    .unwrap();
    let out = run("ruflo", project.path(), &["analyze", "deps"]);
    assert_eq!(out.status.code(), Some(0));
    let s = stdout(&out);
    assert!(s.contains("Dependencies: 1"));
    assert!(s.contains("Dev Dependencies: 1"));
}

#[test]
fn binary_parity_overview() {
    let project = tempfile::tempdir().unwrap();
    let a = run("ruflo", project.path(), &["analyze"]);
    let b = run("claude-flow", project.path(), &["analyze"]);
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
