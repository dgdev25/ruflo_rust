mod capability_manifest;
mod policy;

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

pub use capability_manifest::{CapabilityManifest, RegisteredCapability};
pub use policy::{Caller, DispatchRequest, Limits, PolicyDecision, ToolPolicy};
use ruflo_types::RufloError;

const DEFAULT_CONFIG_FILE: &str = "ruflo.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveConfig {
    pub policy: PolicyConfig,
    pub limits: Limits,
}

impl EffectiveConfig {
    pub fn load() -> Result<Self, RufloError> {
        let cwd = env::current_dir().map_err(|error| {
            RufloError::invalid_input("config.cwd", format!("failed to resolve cwd: {error}"))
        })?;
        Self::load_with(&CliOverrides::default(), env::vars(), cwd)
    }

    pub fn load_with<I, K, V>(
        cli: &CliOverrides,
        env_vars: I,
        project_root: impl AsRef<Path>,
    ) -> Result<Self, RufloError>
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        let project_root = project_root.as_ref();
        let config_path = cli
            .config_path
            .clone()
            .unwrap_or_else(|| project_root.join(DEFAULT_CONFIG_FILE));

        let mut raw = RawConfig::default();
        if config_path.is_file() {
            let contents = fs::read_to_string(&config_path).map_err(|error| {
                RufloError::invalid_input(
                    "config.read",
                    format!("failed to read {}: {error}", config_path.display()),
                )
            })?;
            raw.merge_project(ProjectConfigFile::parse(&contents)?);
        }

        let env_overlay = EnvOverlay::from_pairs(env_vars)?;
        raw.merge_env(env_overlay);
        raw.merge_cli(CliOverlay::from(cli));

        Ok(Self {
            policy: PolicyConfig {
                allow: normalize_tokens(raw.policy.allow),
                deny: normalize_tokens(raw.policy.deny),
            },
            limits: Limits {
                max_request_bytes: raw.limits.max_request_bytes,
                max_concurrent_executions: raw.limits.max_concurrent_executions,
                max_duration_ms: raw.limits.max_duration_ms,
            },
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CliOverrides {
    pub config_path: Option<PathBuf>,
    pub allow: Option<Vec<String>>,
    pub deny: Option<Vec<String>>,
    pub max_request_bytes: Option<usize>,
    pub max_concurrent_executions: Option<usize>,
    pub max_duration_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyConfig {
    pub allow: Vec<String>,
    pub deny: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawConfig {
    #[serde(default)]
    policy: RawPolicyConfig,
    #[serde(default)]
    limits: RawLimitsConfig,
}

impl Default for RawConfig {
    fn default() -> Self {
        Self {
            policy: RawPolicyConfig::default(),
            limits: RawLimitsConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RawPolicyConfig {
    #[serde(default)]
    allow: Vec<String>,
    #[serde(default)]
    deny: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawLimitsConfig {
    #[serde(default = "default_max_request_bytes")]
    max_request_bytes: usize,
    #[serde(default = "default_max_concurrent_executions")]
    max_concurrent_executions: usize,
    #[serde(default = "default_max_duration_ms")]
    max_duration_ms: u64,
}

impl Default for RawLimitsConfig {
    fn default() -> Self {
        Self {
            max_request_bytes: default_max_request_bytes(),
            max_concurrent_executions: default_max_concurrent_executions(),
            max_duration_ms: default_max_duration_ms(),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct EnvOverlay {
    policy: Option<RawPolicyConfig>,
    limits: Option<RawLimitsConfig>,
}

impl EnvOverlay {
    fn from_pairs<I, K, V>(pairs: I) -> Result<Self, RufloError>
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        let mut allow = None;
        let mut deny = None;
        let mut max_request_bytes = None;
        let mut max_concurrent_executions = None;
        let mut max_duration_ms = None;

        for (key, value) in pairs {
            match key.as_ref() {
                "RUFLO_MCP_ALLOW" => allow = Some(parse_csv(value.as_ref())),
                "RUFLO_MCP_DENY" => deny = Some(parse_csv(value.as_ref())),
                "RUFLO_MCP_MAX_REQUEST_BYTES" => {
                    max_request_bytes =
                        Some(parse_number("RUFLO_MCP_MAX_REQUEST_BYTES", value.as_ref())?)
                }
                "RUFLO_MCP_MAX_CONCURRENT_EXECUTIONS" => {
                    max_concurrent_executions = Some(parse_number(
                        "RUFLO_MCP_MAX_CONCURRENT_EXECUTIONS",
                        value.as_ref(),
                    )?)
                }
                "RUFLO_MCP_MAX_DURATION_MS" => {
                    max_duration_ms =
                        Some(parse_number("RUFLO_MCP_MAX_DURATION_MS", value.as_ref())?)
                }
                _ => {}
            }
        }

        let policy = if allow.is_some() || deny.is_some() {
            Some(RawPolicyConfig {
                allow: allow.unwrap_or_default(),
                deny: deny.unwrap_or_default(),
            })
        } else {
            None
        };

        Ok(Self {
            policy,
            limits: if max_request_bytes.is_some()
                || max_concurrent_executions.is_some()
                || max_duration_ms.is_some()
            {
                Some(RawLimitsConfig {
                    max_request_bytes: max_request_bytes.unwrap_or_else(default_max_request_bytes),
                    max_concurrent_executions: max_concurrent_executions
                        .unwrap_or_else(default_max_concurrent_executions),
                    max_duration_ms: max_duration_ms.unwrap_or_else(default_max_duration_ms),
                })
            } else {
                None
            },
        })
    }
}

#[derive(Debug, Clone, Default)]
struct CliOverlay {
    policy: Option<RawPolicyConfig>,
    limits: Option<RawLimitsConfig>,
}

impl From<&CliOverrides> for CliOverlay {
    fn from(value: &CliOverrides) -> Self {
        Self {
            policy: if value.allow.is_some() || value.deny.is_some() {
                Some(RawPolicyConfig {
                    allow: value.allow.clone().unwrap_or_default(),
                    deny: value.deny.clone().unwrap_or_default(),
                })
            } else {
                None
            },
            limits: if value.max_request_bytes.is_some()
                || value.max_concurrent_executions.is_some()
                || value.max_duration_ms.is_some()
            {
                Some(RawLimitsConfig {
                    max_request_bytes: value
                        .max_request_bytes
                        .unwrap_or_else(default_max_request_bytes),
                    max_concurrent_executions: value
                        .max_concurrent_executions
                        .unwrap_or_else(default_max_concurrent_executions),
                    max_duration_ms: value
                        .max_duration_ms
                        .unwrap_or_else(default_max_duration_ms),
                })
            } else {
                None
            },
        }
    }
}

fn normalize_tokens(values: Vec<String>) -> Vec<String> {
    let mut unique = BTreeSet::new();
    for value in values {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            unique.insert(trimmed.to_string());
        }
    }
    unique.into_iter().collect()
}

fn parse_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn parse_number<T>(key: &'static str, value: &str) -> Result<T, RufloError>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    value.parse::<T>().map_err(|error| {
        RufloError::invalid_input(key, format!("invalid numeric value `{value}`: {error}"))
    })
}

const fn default_max_request_bytes() -> usize {
    64 * 1024
}

const fn default_max_concurrent_executions() -> usize {
    4
}

const fn default_max_duration_ms() -> u64 {
    30_000
}

#[derive(Debug, Clone, Default)]
struct ProjectConfigFile {
    policy: Option<RawPolicyConfig>,
    limits: Option<RawLimitsConfig>,
}

impl ProjectConfigFile {
    fn parse(contents: &str) -> Result<Self, RufloError> {
        let mut file = Self::default();
        let mut section = "";

        for raw_line in contents.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if line.starts_with('[') && line.ends_with(']') {
                section = &line[1..line.len() - 1];
                continue;
            }

            let Some((key, value)) = line.split_once('=') else {
                return Err(RufloError::invalid_input(
                    "config.invalid",
                    format!("invalid config line `{line}`"),
                ));
            };

            let key = key.trim();
            let value = value.trim();

            match (section, key) {
                ("policy", "allow") => {
                    file.policy.get_or_insert_with(Default::default).allow = parse_array(value)?;
                }
                ("policy", "deny") => {
                    file.policy.get_or_insert_with(Default::default).deny = parse_array(value)?;
                }
                ("limits", "max_request_bytes") => {
                    file.limits
                        .get_or_insert_with(RawLimitsConfig::default)
                        .max_request_bytes = parse_number("max_request_bytes", value)?;
                }
                ("limits", "max_concurrent_executions") => {
                    file.limits
                        .get_or_insert_with(RawLimitsConfig::default)
                        .max_concurrent_executions =
                        parse_number("max_concurrent_executions", value)?;
                }
                ("limits", "max_duration_ms") => {
                    file.limits
                        .get_or_insert_with(RawLimitsConfig::default)
                        .max_duration_ms = parse_number("max_duration_ms", value)?;
                }
                _ => {}
            }
        }

        Ok(file)
    }
}

impl RawConfig {
    fn merge_project(&mut self, project: ProjectConfigFile) {
        if let Some(policy) = project.policy {
            self.policy = policy;
        }
        if let Some(limits) = project.limits {
            self.limits = limits;
        }
    }

    fn merge_env(&mut self, env: EnvOverlay) {
        if let Some(policy) = env.policy {
            self.policy = policy;
        }
        if let Some(limits) = env.limits {
            self.limits = limits;
        }
    }

    fn merge_cli(&mut self, cli: CliOverlay) {
        if let Some(policy) = cli.policy {
            self.policy = policy;
        }
        if let Some(limits) = cli.limits {
            self.limits = limits;
        }
    }
}

fn parse_array(value: &str) -> Result<Vec<String>, RufloError> {
    let trimmed = value.trim();
    if !trimmed.starts_with('[') || !trimmed.ends_with(']') {
        return Err(RufloError::invalid_input(
            "config.invalid",
            format!("expected array literal, got `{value}`"),
        ));
    }

    let inner = &trimmed[1..trimmed.len() - 1];
    if inner.trim().is_empty() {
        return Ok(Vec::new());
    }

    inner
        .split(',')
        .map(|item| {
            let item = item.trim();
            if item.len() < 2 || !item.starts_with('"') || !item.ends_with('"') {
                return Err(RufloError::invalid_input(
                    "config.invalid",
                    format!("expected string element, got `{item}`"),
                ));
            }
            Ok(item[1..item.len() - 1].to_string())
        })
        .collect()
}
