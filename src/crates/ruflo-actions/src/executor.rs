use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use ruflo_types::RufloError;

use crate::manifest::{ActionInvocation, ActionManifest, NativeAction};

#[derive(Debug, Clone)]
pub struct ActionRequest {
    pub manifest: ActionManifest,
    pub project_root: PathBuf,
    pub working_directory: PathBuf,
    pub environment: BTreeMap<String, String>,
    pub action_id: String,
    pub invocation: ActionInvocation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub working_directory: PathBuf,
}

#[derive(Debug)]
pub struct NativeActionExecutor {
    allowlist: BTreeSet<NativeActionKey>,
    default_timeout: Duration,
    max_concurrent_executions: usize,
    inherited_environment: BTreeSet<String>,
    active_executions: AtomicUsize,
}

impl Default for NativeActionExecutor {
    fn default() -> Self {
        NativeActionExecutorBuilder::new().build()
    }
}

impl NativeActionExecutor {
    pub fn builder() -> NativeActionExecutorBuilder {
        NativeActionExecutorBuilder::new()
    }

    pub fn execute(&self, request: ActionRequest) -> Result<ActionOutput, RufloError> {
        let declared_action = request
            .manifest
            .declared_action(&request.action_id)
            .ok_or_else(|| {
                RufloError::invalid_input(
                    "actions.action.unknown",
                    format!("manifest does not declare action `{}`", request.action_id),
                )
            })?;

        match &request.invocation {
            ActionInvocation::Native(invocation) => {
                if NativeActionKey::from_action(&declared_action.action)
                    != NativeActionKey::from_action(invocation)
                {
                    return Err(RufloError::invalid_input(
                        "actions.action.mismatch",
                        format!(
                            "action `{}` must invoke `{}`",
                            request.action_id,
                            declared_action.action.name()
                        ),
                    ));
                }
            }
        }

        let project_root = canonicalize_directory(
            &request.project_root,
            "actions.project_root.invalid",
            "project root must be an existing directory",
        )?;
        let working_directory = canonicalize_directory(
            &request.working_directory,
            "actions.working_directory.invalid",
            "working directory must be an existing directory",
        )?;

        if !working_directory.starts_with(&project_root) {
            return Err(RufloError::invalid_input(
                "actions.working_directory.escape",
                "working directory must stay inside the configured project root",
            ));
        }

        let invocation = match request.invocation {
            ActionInvocation::Native(invocation) => invocation,
        };
        let key = NativeActionKey::from_action(&invocation);
        if !self.allowlist.contains(&key) {
            return Err(RufloError::invalid_input(
                "actions.action.disallowed",
                format!(
                    "native action `{}` is not in the executor allowlist",
                    key.as_str()
                ),
            ));
        }

        for key in request.environment.keys() {
            validate_environment_key(key)?;
        }

        let prior = self.active_executions.fetch_add(1, Ordering::SeqCst);
        if prior >= self.max_concurrent_executions {
            self.active_executions.fetch_sub(1, Ordering::SeqCst);
            return Err(RufloError::RateLimited { retry_after_ms: 0 });
        }

        let result = self.execute_inner(
            invocation,
            working_directory,
            request.environment,
            self.default_timeout,
        );
        self.active_executions.fetch_sub(1, Ordering::SeqCst);
        result
    }

    fn execute_inner(
        &self,
        invocation: NativeAction,
        working_directory: PathBuf,
        environment: BTreeMap<String, String>,
        timeout: Duration,
    ) -> Result<ActionOutput, RufloError> {
        for key in &self.inherited_environment {
            let _ = std::env::var_os(key);
        }
        drop(environment);

        match invocation {
            NativeAction::Sleep { duration_ms } => {
                let sleep_for = Duration::from_millis(duration_ms);
                if sleep_for > timeout {
                    thread::sleep(timeout);
                    return Err(RufloError::Timeout);
                }
                thread::sleep(sleep_for);
                Ok(ActionOutput {
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: 0,
                    working_directory,
                })
            }
            NativeAction::Echo { arguments } => Ok(ActionOutput {
                stdout: format!("{}\n", arguments.join(" ")),
                stderr: String::new(),
                exit_code: 0,
                working_directory,
            }),
            NativeAction::PrintWorkingDirectory => Ok(ActionOutput {
                stdout: format!("{}\n", working_directory.display()),
                stderr: String::new(),
                exit_code: 0,
                working_directory,
            }),
        }
    }
}

#[derive(Debug, Clone)]
pub struct NativeActionExecutorBuilder {
    allowlist: BTreeSet<NativeActionKey>,
    default_timeout: Duration,
    max_concurrent_executions: usize,
    inherited_environment: BTreeSet<String>,
}

impl NativeActionExecutorBuilder {
    pub fn new() -> Self {
        let mut inherited_environment = BTreeSet::new();
        inherited_environment.insert("PATH".to_string());
        inherited_environment.insert("LANG".to_string());
        inherited_environment.insert("LC_ALL".to_string());

        let mut allowlist = BTreeSet::new();
        allowlist.insert(NativeActionKey::Echo);
        allowlist.insert(NativeActionKey::PrintWorkingDirectory);
        allowlist.insert(NativeActionKey::Sleep);

        Self {
            allowlist,
            default_timeout: Duration::from_secs(1),
            max_concurrent_executions: 4,
            inherited_environment,
        }
    }

    pub fn with_no_actions(mut self) -> Self {
        self.allowlist.clear();
        self
    }

    pub fn allow_action(mut self, action: NativeAction) -> Self {
        self.allowlist.insert(NativeActionKey::from_action(&action));
        self
    }

    pub fn default_timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = timeout;
        self
    }

    pub fn max_concurrent_executions(mut self, limit: usize) -> Self {
        self.max_concurrent_executions = limit.max(1);
        self
    }

    pub fn build(self) -> NativeActionExecutor {
        NativeActionExecutor {
            allowlist: self.allowlist,
            default_timeout: self.default_timeout,
            max_concurrent_executions: self.max_concurrent_executions,
            inherited_environment: self.inherited_environment,
            active_executions: AtomicUsize::new(0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum NativeActionKey {
    Echo,
    PrintWorkingDirectory,
    Sleep,
}

impl NativeActionKey {
    fn from_action(action: &NativeAction) -> Self {
        match action {
            NativeAction::Echo { .. } => Self::Echo,
            NativeAction::PrintWorkingDirectory => Self::PrintWorkingDirectory,
            NativeAction::Sleep { .. } => Self::Sleep,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::Echo => "echo",
            Self::PrintWorkingDirectory => "print_working_directory",
            Self::Sleep => "sleep",
        }
    }
}

fn canonicalize_directory(
    path: &Path,
    code: &'static str,
    message: &'static str,
) -> Result<PathBuf, RufloError> {
    let canonical = path
        .canonicalize()
        .map_err(|error| RufloError::invalid_input(code, format!("{message}: {error}")))?;
    let metadata = fs::metadata(&canonical)
        .map_err(|error| RufloError::invalid_input(code, format!("{message}: {error}")))?;
    if !metadata.is_dir() {
        return Err(RufloError::invalid_input(code, message));
    }
    Ok(canonical)
}

fn validate_environment_key(key: &str) -> Result<(), RufloError> {
    if key.is_empty() || key.contains('=') {
        return Err(RufloError::invalid_input(
            "actions.environment.key",
            format!("invalid environment key `{key}`"),
        ));
    }
    Ok(())
}
