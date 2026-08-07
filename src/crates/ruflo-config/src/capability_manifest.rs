use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use ruflo_types::Capability;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredCapability {
    pub tool: String,
    pub capability: Capability,
}

impl RegisteredCapability {
    pub fn new(tool: impl Into<String>, capability: Capability) -> Self {
        Self {
            tool: tool.into(),
            capability,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityManifest {
    pub capabilities: Vec<Capability>,
}

impl CapabilityManifest {
    pub fn from_registry(registry: &[RegisteredCapability]) -> Self {
        let mut capabilities = registry
            .iter()
            .map(|entry| entry.capability.clone())
            .collect::<Vec<_>>();
        capabilities.sort_by(|left, right| left.name.cmp(&right.name));
        capabilities.dedup_by(|left, right| left.name == right.name);
        Self { capabilities }
    }

    pub fn by_name(&self) -> BTreeMap<&str, &Capability> {
        self.capabilities
            .iter()
            .map(|capability| (capability.name.as_str(), capability))
            .collect()
    }
}
