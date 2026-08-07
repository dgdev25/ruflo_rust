use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use ruflo_actions::{
    ActionInvocation, ActionManifestEnvelope, ActionRequest, NativeAction, NativeActionExecutor,
    NativeActionExecutorBuilder,
};
use ruflo_types::{CapabilityStatus, RufloError};
use tempfile::TempDir;

fn load_manifest() -> ActionManifestEnvelope {
    let fixture = fs::read_to_string("tests/fixtures/plugins/declarative-plugin.json").unwrap();
    serde_json::from_str(&fixture).unwrap()
}

fn project_root() -> TempDir {
    tempfile::tempdir().unwrap()
}

fn ensure_dir(path: &Path) {
    fs::create_dir_all(path).unwrap();
}

#[test]
fn shell_metacharacters_never_reach_an_executable() {
    let manifest = load_manifest().validate().unwrap();
    let project = project_root();
    let working_directory = project.path().join("workspace");
    ensure_dir(&working_directory);

    let result = NativeActionExecutor::default()
        .execute(ActionRequest {
            manifest,
            project_root: project.path().to_path_buf(),
            working_directory,
            environment: Default::default(),
            action_id: "echo-args".into(),
            invocation: ActionInvocation::Native(NativeAction::Echo {
                arguments: vec!["$(whoami); touch should-not-exist".into()],
            }),
        })
        .unwrap();

    assert_eq!(result.stdout.trim(), "$(whoami); touch should-not-exist");
    assert!(!project.path().join("should-not-exist").exists());
}

#[test]
fn working_directory_escape_is_rejected() {
    let manifest = load_manifest().validate().unwrap();
    let project = project_root();
    let escape = tempfile::tempdir().unwrap();

    let error = NativeActionExecutor::default()
        .execute(ActionRequest {
            manifest,
            project_root: project.path().to_path_buf(),
            working_directory: escape.path().to_path_buf(),
            environment: Default::default(),
            action_id: "print-working-directory".into(),
            invocation: ActionInvocation::Native(NativeAction::PrintWorkingDirectory),
        })
        .unwrap_err();

    assert!(matches!(
        error,
        RufloError::InvalidInput { code, .. } if code == "actions.working_directory.escape"
    ));
}

#[test]
fn action_allowlist_is_enforced() {
    let manifest = load_manifest().validate().unwrap();
    let project = project_root();
    let working_directory = project.path().join("workspace");
    ensure_dir(&working_directory);

    let executor = NativeActionExecutorBuilder::new()
        .with_no_actions()
        .allow_action(NativeAction::PrintWorkingDirectory)
        .build();

    let error = executor
        .execute(ActionRequest {
            manifest,
            project_root: project.path().to_path_buf(),
            working_directory,
            environment: Default::default(),
            action_id: "echo-args".into(),
            invocation: ActionInvocation::Native(NativeAction::Echo {
                arguments: vec!["hello".into()],
            }),
        })
        .unwrap_err();

    assert!(matches!(
        error,
        RufloError::InvalidInput { code, .. } if code == "actions.action.disallowed"
    ));
}

#[test]
fn action_timeout_is_enforced() {
    let manifest = load_manifest().validate().unwrap();
    let project = project_root();
    let working_directory = project.path().join("workspace");
    ensure_dir(&working_directory);

    let executor = NativeActionExecutorBuilder::new()
        .default_timeout(Duration::from_millis(25))
        .build();

    let error = executor
        .execute(ActionRequest {
            manifest,
            project_root: project.path().to_path_buf(),
            working_directory,
            environment: Default::default(),
            action_id: "sleep-briefly".into(),
            invocation: ActionInvocation::Native(NativeAction::Sleep { duration_ms: 250 }),
        })
        .unwrap_err();

    assert!(matches!(error, RufloError::Timeout));
}

#[test]
fn concurrency_limit_is_enforced() {
    let manifest = load_manifest().validate().unwrap();
    let project = Arc::new(project_root());
    let workspace = project.path().join("workspace");
    ensure_dir(&workspace);

    let executor = Arc::new(
        NativeActionExecutorBuilder::new()
            .max_concurrent_executions(1)
            .default_timeout(Duration::from_millis(500))
            .build(),
    );

    let first_executor = Arc::clone(&executor);
    let first_project = Arc::clone(&project);
    let first_manifest = manifest.clone();
    let first_workspace = workspace.clone();
    let handle = thread::spawn(move || {
        first_executor.execute(ActionRequest {
            manifest: first_manifest,
            project_root: first_project.path().to_path_buf(),
            working_directory: first_workspace,
            environment: Default::default(),
            action_id: "sleep-briefly".into(),
            invocation: ActionInvocation::Native(NativeAction::Sleep { duration_ms: 150 }),
        })
    });

    thread::sleep(Duration::from_millis(25));

    let error = executor
        .execute(ActionRequest {
            manifest,
            project_root: project.path().to_path_buf(),
            working_directory: workspace,
            environment: Default::default(),
            action_id: "sleep-briefly".into(),
            invocation: ActionInvocation::Native(NativeAction::Sleep { duration_ms: 10 }),
        })
        .unwrap_err();

    assert!(matches!(
        error,
        RufloError::RateLimited { retry_after_ms: 0 }
    ));
    handle.join().unwrap().unwrap();
}

#[test]
fn canonicalized_working_directory_stays_beneath_project_root() {
    let manifest = load_manifest().validate().unwrap();
    let project = project_root();
    let nested = project.path().join("workspace").join("nested");
    ensure_dir(&nested);
    let alias = project.path().join("workspace").join("nested").join("..");

    let result = NativeActionExecutor::default()
        .execute(ActionRequest {
            manifest,
            project_root: project.path().to_path_buf(),
            working_directory: alias,
            environment: Default::default(),
            action_id: "print-working-directory".into(),
            invocation: ActionInvocation::Native(NativeAction::PrintWorkingDirectory),
        })
        .unwrap();

    assert_eq!(
        result.stdout.trim(),
        nested.parent().unwrap().display().to_string()
    );
}

#[test]
fn javascript_executable_plugins_return_stable_unsupported_in_wave() {
    let manifest: ActionManifestEnvelope = serde_json::from_str(
        r#"{
            "plugin_type": "javascript_executable",
            "version": 1,
            "name": "legacy-plugin",
            "entrypoint": "index.js"
        }"#,
    )
    .unwrap();

    let error = manifest.validate().unwrap_err();

    match error {
        RufloError::UnsupportedInWave { capability } => {
            assert_eq!(capability.name, "plugins.javascript_executable");
            assert_eq!(capability.wave, 2);
            assert_eq!(capability.status, CapabilityStatus::Unsupported);
        }
        other => panic!("unexpected error: {other:?}"),
    }
}
