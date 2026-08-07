use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use ruflo_types::{Capability, CapabilityStatus, RufloError};

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
    #[serde(default, skip_serializing_if = "ReleaseManifest::is_empty")]
    pub release: ReleaseManifest,
    #[serde(skip)]
    root: Option<PathBuf>,
}

impl CapabilityManifest {
    pub fn from_registry(registry: &[RegisteredCapability]) -> Self {
        let mut capabilities = registry
            .iter()
            .map(|entry| entry.capability.clone())
            .collect::<Vec<_>>();
        capabilities.sort_by(|left, right| left.name.cmp(&right.name));
        capabilities.dedup_by(|left, right| left.name == right.name);
        Self {
            capabilities,
            release: ReleaseManifest::default(),
            root: None,
        }
    }

    pub fn from_test_fixture(path: impl AsRef<Path>) -> Result<Self, RufloError> {
        let path = path.as_ref();
        let raw = fs::read_to_string(path).map_err(|error| {
            RufloError::invalid_input(
                "capability_manifest.read",
                format!("failed to read {}: {error}", path.display()),
            )
        })?;
        let mut manifest: Self = serde_json::from_str(&raw).map_err(|error| {
            RufloError::invalid_input(
                "capability_manifest.parse",
                format!("failed to parse {}: {error}", path.display()),
            )
        })?;
        manifest.root = Some(
            std::env::current_dir()
                .map_err(|error| {
                    RufloError::invalid_input(
                        "capability_manifest.cwd",
                        format!("failed to resolve cwd: {error}"),
                    )
                })?
                .canonicalize()
                .map_err(|error| {
                    RufloError::invalid_input(
                        "capability_manifest.cwd",
                        format!("failed to canonicalize cwd: {error}"),
                    )
                })?,
        );
        Ok(manifest)
    }

    pub fn by_name(&self) -> BTreeMap<&str, &Capability> {
        self.capabilities
            .iter()
            .map(|capability| (capability.name.as_str(), capability))
            .collect()
    }

    pub fn validate_release(&self, wave: u8) -> Result<(), RufloError> {
        let promoted = self
            .capabilities
            .iter()
            .filter(|capability| capability.wave == wave)
            .filter(|capability| capability.status == CapabilityStatus::Supported)
            .map(|capability| capability.name.as_str())
            .collect::<Vec<_>>();
        if promoted.is_empty() {
            return Ok(());
        }

        let evidence = self.release.wave(wave).ok_or_else(|| {
            RufloError::invalid_input(
                "release.validation.missing_wave_evidence",
                format!(
                    "wave {wave} has supported capabilities but no release evidence: {}",
                    promoted.join(", ")
                ),
            )
        })?;
        let root = self.root_path()?;
        evidence.validate(&root, &promoted)
    }

    fn root_path(&self) -> Result<PathBuf, RufloError> {
        if let Some(root) = &self.root {
            return Ok(root.clone());
        }
        std::env::current_dir().map_err(|error| {
            RufloError::invalid_input(
                "capability_manifest.cwd",
                format!("failed to resolve cwd: {error}"),
            )
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseManifest {
    #[serde(default)]
    pub waves: Vec<WaveReleaseEvidence>,
}

impl ReleaseManifest {
    fn is_empty(&self) -> bool {
        self.waves.is_empty()
    }

    fn wave(&self, wave: u8) -> Option<&WaveReleaseEvidence> {
        self.waves.iter().find(|entry| entry.wave == wave)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaveReleaseEvidence {
    pub wave: u8,
    pub criteria_doc: String,
    #[serde(default)]
    pub consumer_fixtures: Vec<NamedEvidence>,
    #[serde(default)]
    pub security_tests: Vec<NamedEvidence>,
    #[serde(default)]
    pub native_platform_evidence: Vec<NamedEvidence>,
    #[serde(default)]
    pub migration_tests: Vec<NamedEvidence>,
    #[serde(default)]
    pub rvf_tests: Vec<NamedEvidence>,
    #[serde(default)]
    pub supply_chain_review: SupplyChainEvidence,
    #[serde(default)]
    pub long_lived_integrations: Vec<String>,
    #[serde(default)]
    pub adrs: Vec<AdrEvidence>,
}

impl WaveReleaseEvidence {
    fn validate(&self, root: &Path, promoted: &[&str]) -> Result<(), RufloError> {
        let mut problems = Vec::new();

        self.require_path(root, "criteria_doc", &self.criteria_doc, &mut problems);
        require_named_entries(
            root,
            "consumer_fixtures",
            &self.consumer_fixtures,
            &mut problems,
        );
        require_named_entries(root, "security_tests", &self.security_tests, &mut problems);
        require_named_entries(
            root,
            "native_platform_evidence",
            &self.native_platform_evidence,
            &mut problems,
        );
        require_named_entries(
            root,
            "migration_tests",
            &self.migration_tests,
            &mut problems,
        );
        require_named_entries(root, "rvf_tests", &self.rvf_tests, &mut problems);
        self.supply_chain_review.validate(root, &mut problems);
        self.validate_adrs(root, &mut problems);

        if problems.is_empty() {
            Ok(())
        } else {
            Err(RufloError::invalid_input(
                "release.validation.incomplete_evidence",
                format!(
                    "wave {} release evidence is incomplete for supported capabilities [{}]: {}",
                    self.wave,
                    promoted.join(", "),
                    problems.join("; ")
                ),
            ))
        }
    }

    fn require_path(&self, root: &Path, label: &str, value: &str, problems: &mut Vec<String>) {
        if value.trim().is_empty() {
            problems.push(format!("{label} must not be empty"));
            return;
        }
        let path = root.join(value);
        if !path.exists() {
            problems.push(format!("{label} is missing `{}`", value));
        }
    }

    fn validate_adrs(&self, root: &Path, problems: &mut Vec<String>) {
        let integration_set = self
            .long_lived_integrations
            .iter()
            .filter(|value| !value.trim().is_empty())
            .map(|value| value.as_str())
            .collect::<BTreeSet<_>>();
        if integration_set.is_empty() {
            problems.push("long_lived_integrations must name at least one integration".to_string());
        }

        if self.adrs.is_empty() {
            problems
                .push("adrs must include one ADR record per long-lived integration".to_string());
            return;
        }

        let mut covered = BTreeSet::new();
        for adr in &self.adrs {
            adr.validate(root, problems);
            if integration_set.contains(adr.integration.as_str()) {
                covered.insert(adr.integration.as_str());
            }
        }

        for integration in integration_set {
            if !covered.contains(integration) {
                problems.push(format!(
                    "missing ADR record for long-lived integration `{integration}`"
                ));
            }
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedEvidence {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupplyChainEvidence {
    #[serde(default)]
    pub audits: Vec<NamedEvidence>,
    #[serde(default)]
    pub sboms: Vec<NamedEvidence>,
    #[serde(default)]
    pub dependency_reviews: Vec<NamedEvidence>,
}

impl SupplyChainEvidence {
    fn validate(&self, root: &Path, problems: &mut Vec<String>) {
        require_named_entries(root, "supply_chain_review.audits", &self.audits, problems);
        require_named_entries(root, "supply_chain_review.sboms", &self.sboms, problems);
        require_named_entries(
            root,
            "supply_chain_review.dependency_reviews",
            &self.dependency_reviews,
            problems,
        );
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdrEvidence {
    pub id: String,
    pub integration: String,
    pub status: String,
    pub path: String,
}

impl AdrEvidence {
    fn validate(&self, root: &Path, problems: &mut Vec<String>) {
        for (field, value) in [
            ("id", self.id.as_str()),
            ("integration", self.integration.as_str()),
            ("status", self.status.as_str()),
            ("path", self.path.as_str()),
        ] {
            if value.trim().is_empty() {
                problems.push(format!("adr.{field} must not be empty"));
            }
        }
        if !self.path.trim().is_empty() && !root.join(&self.path).exists() {
            problems.push(format!("adr path is missing `{}`", self.path));
        }
    }
}

fn require_named_entries(
    root: &Path,
    label: &str,
    entries: &[NamedEvidence],
    problems: &mut Vec<String>,
) {
    if entries.is_empty() {
        problems.push(format!("{label} must not be empty"));
        return;
    }

    for entry in entries {
        if entry.name.trim().is_empty() {
            problems.push(format!("{label} contains an unnamed entry"));
        }
        if entry.path.trim().is_empty() {
            problems.push(format!("{label} entry `{}` has an empty path", entry.name));
            continue;
        }
        if !root.join(&entry.path).exists() {
            problems.push(format!(
                "{label} entry `{}` is missing `{}`",
                entry.name, entry.path
            ));
        }
    }
}
