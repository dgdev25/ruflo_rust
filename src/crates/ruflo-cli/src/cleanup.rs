use std::fs;
use std::io::{self, Write};
use std::path::Path;

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub path: &'static str,
    pub description: &'static str,
    pub size: u64,
    pub kind: ArtifactKind,
    pub skipped: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactKind {
    Directory,
    File,
}

#[derive(Debug, Clone)]
pub struct CleanupResult {
    pub artifacts: Vec<Artifact>,
    pub removed_count: usize,
    pub removed_size: u64,
    pub total_size: u64,
    pub failures: Vec<(Artifact, String)>,
}

const CANDIDATES: &[(&str, &str, ArtifactKind)] = &[
    (
        ".claude/helpers",
        "Ruflo hook scripts",
        ArtifactKind::Directory,
    ),
    (
        ".claude/settings.json",
        "Remove ruflo hooks/claudeFlow blocks (preserves rest)",
        ArtifactKind::File,
    ),
    (
        ".claude-flow",
        "Capabilities and configuration",
        ArtifactKind::Directory,
    ),
    ("data", "Memory databases", ArtifactKind::Directory),
    (".swarm", "Swarm state", ArtifactKind::Directory),
    (".hive-mind", "Consensus state", ArtifactKind::Directory),
    ("coordination", "Coordination data", ArtifactKind::Directory),
    ("memory", "Memory storage", ArtifactKind::Directory),
    (
        "claude-flow.config.json",
        "Claude Flow configuration",
        ArtifactKind::File,
    ),
];

pub fn run(root: &Path, force: bool, keep_config: bool) -> CleanupResult {
    let mut artifacts = discover(root, keep_config);
    let total_size = artifacts.iter().map(|item| item.size).sum();
    let mut removed_count = 0;
    let mut removed_size = 0;
    let mut failures = Vec::new();

    if force {
        for artifact in artifacts.iter().filter(|item| !item.skipped) {
            match remove(root, artifact) {
                Ok(()) => {
                    removed_count += 1;
                    removed_size += artifact.size;
                }
                Err(error) => failures.push((artifact.clone(), error.to_string())),
            }
        }
    }

    CleanupResult {
        artifacts: std::mem::take(&mut artifacts),
        removed_count,
        removed_size,
        total_size,
        failures,
    }
}

pub fn discover(root: &Path, keep_config: bool) -> Vec<Artifact> {
    CANDIDATES
        .iter()
        .filter_map(|(relative, description, kind)| {
            let path = root.join(relative);
            if !path.exists() {
                return None;
            }
            // Handoff item 40: never traverse outside the project through
            // symlinks. Skip any candidate whose real path escapes the repo root
            // (e.g. `.claude` symlinked to an external dir → `.claude/helpers`
            // would otherwise delete/size foreign files).
            if !within_root(root, &path) {
                return None;
            }
            Some(Artifact {
                path: relative,
                description,
                size: if *relative == ".claude/settings.json" {
                    0
                } else {
                    size(&path)
                },
                kind: *kind,
                skipped: keep_config
                    && matches!(
                        *relative,
                        ".claude/settings.json" | "claude-flow.config.json"
                    ),
            })
        })
        .collect()
}

/// True when `candidate`'s real (canonical) path stays inside `root`. Absent
/// paths are treated as safe (they would be created under root). This is the
/// symlink-escape guard mandated by handoff item 40.
fn within_root(root: &Path, candidate: &Path) -> bool {
    let Ok(root_real) = fs::canonicalize(root) else {
        return true;
    };
    match fs::canonicalize(candidate) {
        Ok(real) => real.starts_with(&root_real),
        // Absent or broken symlink → not traversable; safe to consider in-root.
        Err(_) => true,
    }
}

fn size(path: &Path) -> u64 {
    // Use symlink_metadata so a symlinked child is never followed out of the
    // project (handoff item 40). A symlink counts as its link size, not its
    // target tree.
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return 0;
    };
    if metadata.is_symlink() {
        return metadata.len();
    }
    if metadata.is_file() {
        return metadata.len();
    }
    if !metadata.is_dir() {
        return 0;
    }
    fs::read_dir(path)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| size(&entry.path()))
        .sum()
}

fn remove(root: &Path, artifact: &Artifact) -> io::Result<()> {
    let path = root.join(artifact.path);
    if artifact.path == ".claude/settings.json" {
        let Ok(contents) = fs::read_to_string(&path) else {
            return Ok(());
        };
        let Ok(mut settings) = serde_json::from_str::<Value>(&contents) else {
            return Ok(());
        };
        let Some(object) = settings.as_object_mut() else {
            return Ok(());
        };
        // hooks/claudeFlow are ruflo-owned blocks (written by `ruflo init`);
        // unrelated top-level keys (agents/, theme, …) are preserved.
        object.remove("hooks");
        object.remove("claudeFlow");
        normalize_js_numbers(&mut settings);
        let mut contents = serde_json::to_string_pretty(&settings)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        contents.push('\n');
        // Atomic + no-symlink-redirect write (handoff item 40). create_new
        // (O_CREAT|O_EXCL) refuses an existing final component — including a
        // symlink — and the pid-suffixed name is not predictable enough to
        // pre-plant; failure is surfaced rather than silently claiming removal.
        write_atomic_nofollow(&path, contents.as_bytes())?;
        return Ok(());
    }
    if artifact.kind == ArtifactKind::Directory {
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            fs::remove_file(path)
        } else {
            fs::remove_dir_all(path)
        }
    } else {
        fs::remove_file(path)
    }
}

/// Atomic same-directory write. `create_new` (O_CREAT|O_EXCL) refuses to open a
/// path that already exists — including a symlink — so a pre-existing
/// `settings.json.<pid>.tmp` symlink cannot redirect the write to a foreign
/// file (EEXIST is surfaced as a failure). The unique pid-suffixed name avoids
/// collisions with a stale regular tmp from a prior crash.
#[cfg(unix)]
fn write_atomic_nofollow(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = tmp_path(path);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    let res = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)
    })();
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    res
}

#[cfg(not(unix))]
fn write_atomic_nofollow(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = tmp_path(path);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)?;
    let res = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)
    })();
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    res
}

fn tmp_path(path: &Path) -> std::path::PathBuf {
    // settings.json → settings.json.<pid>.tmp (sibling, unique per process).
    let mut name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    name.push_str(&format!(".{}.tmp", std::process::id()));
    path.with_file_name(name)
}

fn normalize_js_numbers(value: &mut Value) {
    match value {
        Value::Array(values) => values.iter_mut().for_each(normalize_js_numbers),
        Value::Object(values) => values.values_mut().for_each(normalize_js_numbers),
        Value::Number(number) if number.is_f64() => {
            if let Some(value) = number.as_f64() {
                if value.fract() == 0.0 && value >= i64::MIN as f64 && value <= i64::MAX as f64 {
                    *number = serde_json::Number::from(value as i64);
                }
            }
        }
        _ => {}
    }
}

pub fn format_size(bytes: u64) -> String {
    if bytes == 0 {
        return "0 B".into();
    }
    const UNITS: &[&str] = &["B", "KB", "MB", "GB"];
    let index = ((bytes as f64).ln() / 1024_f64.ln()).floor().max(0.0) as usize;
    let index = index.min(UNITS.len() - 1);
    let value = bytes as f64 / 1024_f64.powi(index as i32);
    if index == 0 {
        format!("{value:.0} {}", UNITS[index])
    } else {
        format!("{value:.1} {}", UNITS[index])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dry_run_discovers_in_source_order_and_changes_nothing() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".claude/helpers")).unwrap();
        fs::write(temp.path().join(".claude/helpers/hook.sh"), "hook").unwrap();
        fs::create_dir_all(temp.path().join(".swarm")).unwrap();
        fs::write(temp.path().join(".swarm/state"), "state").unwrap();
        fs::write(temp.path().join("claude-flow.config.json"), "{}").unwrap();
        let result = run(temp.path(), false, false);
        assert_eq!(
            result
                .artifacts
                .iter()
                .map(|item| item.path)
                .collect::<Vec<_>>(),
            [".claude/helpers", ".swarm", "claude-flow.config.json"]
        );
        assert!(temp.path().join(".swarm/state").is_file());
        assert_eq!(result.removed_count, 0);
    }

    #[test]
    fn force_is_surgical_inside_claude_and_keep_config_is_byte_preserving() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".claude/helpers")).unwrap();
        fs::create_dir_all(temp.path().join(".claude/agents")).unwrap();
        fs::write(temp.path().join(".claude/agents/user.md"), "keep").unwrap();
        let settings =
            "{\n  \"hooks\": {\"x\": 1},\n  \"claudeFlow\": true,\n  \"theme\": \"dark\"\n}\n";
        fs::write(temp.path().join(".claude/settings.json"), settings).unwrap();
        fs::write(
            temp.path().join("claude-flow.config.json"),
            "{\"keep\":true}\n",
        )
        .unwrap();
        let result = run(temp.path(), true, true);
        assert_eq!(result.removed_count, 1);
        assert!(!temp.path().join(".claude/helpers").exists());
        assert!(temp.path().join(".claude/agents/user.md").is_file());
        assert_eq!(
            fs::read_to_string(temp.path().join(".claude/settings.json")).unwrap(),
            settings
        );
        assert!(temp.path().join("claude-flow.config.json").is_file());
    }

    #[test]
    fn force_removes_only_ruflo_blocks_from_settings() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".claude")).unwrap();
        fs::write(
            temp.path().join(".claude/settings.json"),
            r#"{"hooks":{"a":1},"claudeFlow":{},"other":{"nested":true}}"#,
        )
        .unwrap();
        let result = run(temp.path(), true, false);
        assert_eq!(result.removed_count, 1);
        let settings: Value = serde_json::from_str(
            &fs::read_to_string(temp.path().join(".claude/settings.json")).unwrap(),
        )
        .unwrap();
        assert!(settings.get("hooks").is_none());
        assert!(settings.get("claudeFlow").is_none());
        assert_eq!(settings["other"]["nested"], true);
    }

    #[test]
    fn malformed_settings_match_the_source_noop_success_quirk() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".claude")).unwrap();
        let malformed = b"{ this is not json }\n";
        fs::write(temp.path().join(".claude/settings.json"), malformed).unwrap();

        let result = run(temp.path(), true, false);

        assert_eq!(result.removed_count, 1);
        assert_eq!(result.removed_size, 0);
        assert!(result.failures.is_empty());
        assert_eq!(
            fs::read(temp.path().join(".claude/settings.json")).unwrap(),
            malformed
        );
    }

    #[cfg(unix)]
    #[test]
    fn directory_symlink_to_outside_is_skipped_not_traversed() {
        use std::os::unix::fs::symlink;

        let project = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("keep.txt"), "outside").unwrap();
        symlink(outside.path(), project.path().join(".swarm")).unwrap();

        let result = run(project.path(), true, false);

        // Handoff item 40: a symlink resolving outside the project is never
        // traversed/removed — both the link and its foreign target are preserved.
        assert_eq!(result.removed_count, 0);
        assert!(result.artifacts.is_empty());
        assert!(fs::symlink_metadata(project.path().join(".swarm")).is_ok());
        assert_eq!(
            fs::read_to_string(outside.path().join("keep.txt")).unwrap(),
            "outside"
        );
    }

    #[cfg(unix)]
    #[test]
    fn intermediate_claude_symlink_cannot_escape_to_foreign_helpers() {
        use std::os::unix::fs::symlink;

        let project = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        // Foreign `.claude/helpers` living OUTSIDE the project, surfaced via a
        // `.claude` symlink. Without the escape guard, --force would delete the
        // foreign helpers tree.
        fs::create_dir_all(outside.path().join(".claude/helpers")).unwrap();
        fs::write(outside.path().join(".claude/helpers/foreign.sh"), "foreign").unwrap();
        symlink(
            outside.path().join(".claude"),
            project.path().join(".claude"),
        )
        .unwrap();

        let result = run(project.path(), true, false);

        assert_eq!(result.removed_count, 0);
        assert!(result.artifacts.is_empty());
        // Foreign helper survives.
        assert!(outside.path().join(".claude/helpers/foreign.sh").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn broken_directory_symlink_is_ignored_like_node_exists_sync() {
        use std::os::unix::fs::symlink;

        let project = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(
            outside.path().join("missing"),
            project.path().join(".swarm"),
        )
        .unwrap();

        let result = run(project.path(), true, false);

        assert!(result.artifacts.is_empty());
        assert!(fs::symlink_metadata(project.path().join(".swarm")).is_ok());
    }

    #[test]
    fn directory_candidate_that_is_a_file_is_removed() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join(".swarm"), "state").unwrap();

        let result = run(temp.path(), true, false);

        assert_eq!(result.removed_count, 1);
        assert!(!temp.path().join(".swarm").exists());
    }

    #[test]
    fn non_object_settings_are_left_byte_identical_but_reported_removed() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".claude")).unwrap();
        fs::write(temp.path().join(".claude/settings.json"), "null").unwrap();

        let result = run(temp.path(), true, false);

        assert_eq!(result.removed_count, 1);
        assert_eq!(
            fs::read_to_string(temp.path().join(".claude/settings.json")).unwrap(),
            "null"
        );
    }

    #[test]
    fn settings_rewrite_uses_javascript_integer_number_spelling() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".claude")).unwrap();
        fs::write(
            temp.path().join(".claude/settings.json"),
            r#"{"hooks":{},"other":1.0}"#,
        )
        .unwrap();

        run(temp.path(), true, false);

        assert_eq!(
            fs::read_to_string(temp.path().join(".claude/settings.json")).unwrap(),
            "{\n  \"other\": 1\n}\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn settings_tmp_symlink_is_not_followed_and_foreign_file_is_preserved() {
        use std::os::unix::fs::symlink;

        let project = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir_all(project.path().join(".claude")).unwrap();
        // Adversary plants the EXACT pid-suffixed temp path as a symlink to a
        // foreign file. create_new (O_CREAT|O_EXCL) refuses to open an existing
        // symlink (EEXIST) — the write is not redirected.
        fs::write(outside.path().join("sentinel"), "foreign-secret").unwrap();
        let tmp = format!("settings.json.{}.tmp", std::process::id());
        symlink(
            outside.path().join("sentinel"),
            project.path().join(".claude").join(&tmp),
        )
        .unwrap();
        fs::write(
            project.path().join(".claude/settings.json"),
            r#"{"hooks":{"x":1},"other":true}"#,
        )
        .unwrap();

        let result = run(project.path(), true, false);

        // The symlinked temp must NOT be followed: the mutation fails (create_new
        // on a path that exists errors), the foreign sentinel is untouched, and
        // the original settings.json is unchanged.
        assert!(result
            .failures
            .iter()
            .any(|(a, _)| a.path == ".claude/settings.json"));
        assert_eq!(
            fs::read_to_string(outside.path().join("sentinel")).unwrap(),
            "foreign-secret"
        );
    }
}
