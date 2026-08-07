use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{EffectiveConfig, RegisteredCapability};
use ruflo_types::RufloError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caller {
    Local,
    Named(String),
}

impl Caller {
    pub fn local() -> Self {
        Self::Local
    }

    pub fn named(name: impl Into<String>) -> Self {
        Self::Named(name.into())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    pub max_request_bytes: usize,
    pub max_concurrent_executions: usize,
    pub max_duration_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispatchRequest {
    pub request_bytes: usize,
    pub active_executions: usize,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    Allow,
    Deny { capability: String },
}

#[derive(Debug, Clone)]
pub struct ToolPolicy {
    registry: BTreeMap<String, String>,
    allow: BTreeSet<String>,
    deny: BTreeSet<String>,
    limits: Limits,
}

impl ToolPolicy {
    pub fn from_env<I, K, V>(
        env_vars: I,
        registry: &[RegisteredCapability],
    ) -> Result<Self, RufloError>
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        let config = EffectiveConfig::load_with(&crate::CliOverrides::default(), env_vars, ".")?;
        Self::from_config(&config, registry)
    }

    pub fn from_config(
        config: &EffectiveConfig,
        registry: &[RegisteredCapability],
    ) -> Result<Self, RufloError> {
        let registry_map = registry
            .iter()
            .map(|entry| (entry.tool.clone(), entry.capability.name.clone()))
            .collect::<BTreeMap<_, _>>();
        validate_tokens(&config.policy.allow, &registry_map)?;
        validate_tokens(&config.policy.deny, &registry_map)?;

        Ok(Self {
            registry: registry_map,
            allow: config.policy.allow.iter().cloned().collect(),
            deny: config.policy.deny.iter().cloned().collect(),
            limits: config.limits,
        })
    }

    pub fn authorize(&self, caller: &Caller, tool: &str) -> Result<(), RufloError> {
        match self.call_decision(caller, tool) {
            PolicyDecision::Allow => Ok(()),
            PolicyDecision::Deny { capability } => Err(RufloError::unauthorized(capability)),
        }
    }

    pub fn authorize_request(
        &self,
        caller: &Caller,
        tool: &str,
        request: DispatchRequest,
    ) -> Result<(), RufloError> {
        self.authorize(caller, tool)?;
        self.enforce_limits(request)
    }

    pub fn discovery_decision(&self, caller: &Caller, tool: &str) -> PolicyDecision {
        self.call_decision(caller, tool)
    }

    pub fn call_decision(&self, _caller: &Caller, tool: &str) -> PolicyDecision {
        if !self.registry.contains_key(tool) || self.deny.contains(tool) {
            return PolicyDecision::Deny {
                capability: self
                    .registry
                    .get(tool)
                    .cloned()
                    .unwrap_or_else(|| tool.to_string()),
            };
        }

        if !self.allow.is_empty() && !self.allow.contains(tool) {
            return PolicyDecision::Deny {
                capability: self
                    .registry
                    .get(tool)
                    .cloned()
                    .unwrap_or_else(|| tool.to_string()),
            };
        }

        PolicyDecision::Allow
    }

    pub fn is_discoverable(&self, caller: &Caller, tool: &str) -> bool {
        matches!(self.discovery_decision(caller, tool), PolicyDecision::Allow)
    }

    pub fn is_callable(&self, caller: &Caller, tool: &str) -> bool {
        matches!(self.call_decision(caller, tool), PolicyDecision::Allow)
    }

    pub fn discoverable_tools(&self, caller: &Caller) -> Vec<&str> {
        self.registry
            .keys()
            .filter(|tool| self.is_discoverable(caller, tool))
            .map(|tool| tool.as_str())
            .collect()
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    fn enforce_limits(&self, request: DispatchRequest) -> Result<(), RufloError> {
        if request.request_bytes > self.limits.max_request_bytes {
            return Err(RufloError::invalid_input(
                "request_too_large",
                format!(
                    "request size {} exceeds limit {}",
                    request.request_bytes, self.limits.max_request_bytes
                ),
            ));
        }

        if request.active_executions >= self.limits.max_concurrent_executions {
            return Err(RufloError::RateLimited { retry_after_ms: 0 });
        }

        if request.duration_ms > self.limits.max_duration_ms {
            return Err(RufloError::Timeout);
        }

        Ok(())
    }
}

fn validate_tokens(
    tokens: &[String],
    registry: &BTreeMap<String, String>,
) -> Result<(), RufloError> {
    for token in tokens {
        if !registry.contains_key(token) {
            return Err(RufloError::invalid_input(
                "policy.unknown_tool",
                format!("unregistered capability token `{token}`"),
            ));
        }
    }
    Ok(())
}
