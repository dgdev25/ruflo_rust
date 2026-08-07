use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use ruflo_types::{Capability, RufloError};

const SUPPORTED_MANIFEST_VERSION: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "plugin_type", rename_all = "snake_case")]
pub enum ActionManifestEnvelope {
    Declarative(ActionManifest),
    JavascriptExecutable(JavascriptExecutablePlugin),
}

impl ActionManifestEnvelope {
    pub fn validate(self) -> Result<ActionManifest, RufloError> {
        match self {
            Self::Declarative(manifest) => manifest.validate(),
            Self::JavascriptExecutable(_) => Err(RufloError::unsupported(Capability::unsupported(
                "plugins.javascript_executable",
                2,
                "migrate plugin hooks to a declarative native manifest",
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionManifest {
    pub version: u8,
    pub name: String,
    pub actions: Vec<DeclaredAction>,
}

impl ActionManifest {
    pub fn validate(self) -> Result<Self, RufloError> {
        if self.version != SUPPORTED_MANIFEST_VERSION {
            return Err(RufloError::invalid_input(
                "actions.manifest.version",
                format!(
                    "unsupported manifest version {}; expected {}",
                    self.version, SUPPORTED_MANIFEST_VERSION
                ),
            ));
        }

        if self.name.trim().is_empty() {
            return Err(RufloError::invalid_input(
                "actions.manifest.name",
                "manifest name must not be empty",
            ));
        }

        if self.actions.is_empty() {
            return Err(RufloError::invalid_input(
                "actions.manifest.actions",
                "manifest must declare at least one action",
            ));
        }

        let mut ids = BTreeSet::new();
        for action in &self.actions {
            if action.id.trim().is_empty() {
                return Err(RufloError::invalid_input(
                    "actions.manifest.action_id",
                    "action identifier must not be empty",
                ));
            }
            if !ids.insert(action.id.clone()) {
                return Err(RufloError::invalid_input(
                    "actions.manifest.action_id",
                    format!("duplicate action identifier `{}`", action.id),
                ));
            }
        }

        Ok(self)
    }

    pub fn declared_action(&self, action_id: &str) -> Option<&DeclaredAction> {
        self.actions.iter().find(|action| action.id == action_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclaredAction {
    pub id: String,
    pub action: NativeAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JavascriptExecutablePlugin {
    pub version: u8,
    pub name: String,
    pub entrypoint: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActionInvocation {
    Native(NativeAction),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NativeAction {
    Echo { arguments: Vec<String> },
    PrintWorkingDirectory,
    Sleep { duration_ms: u64 },
}

impl NativeAction {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Echo { .. } => "echo",
            Self::PrintWorkingDirectory => "print_working_directory",
            Self::Sleep { .. } => "sleep",
        }
    }
}
