//! Capability contract shared by every facade and runtime crate.
//!
//! A [`Capability`] describes one named unit of functionality, the wave it
//! belongs to, whether it is currently honored, and (when deferred) the
//! migration note a consumer should act on. The machine-readable fields
//! (`name`, `wave`, `status`) are stable: consumers pattern-match on them and
//! the MCP capability manifest serializes them onto the wire.

use serde::{Deserialize, Serialize};

/// Lifecycle status of a [`Capability`] in the current build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityStatus {
    /// Honored in the current wave.
    Supported,
    /// Implemented behind an in-progress migration; not yet stable.
    Migrating,
    /// Deferred to a later wave; invoking it yields
    /// [`RufloError::UnsupportedInWave`](crate::RufloError::UnsupportedInWave).
    Unsupported,
}

/// A named unit of functionality and its wave/status contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    /// Stable dotted identifier, e.g. `"workflow.run"`.
    pub name: String,
    /// Wave (1, 2, 3, …) this capability is scheduled for.
    pub wave: u8,
    /// Current lifecycle status.
    pub status: CapabilityStatus,
    /// Actionable migration note, present when the capability is not yet
    /// supported. `None` for supported capabilities.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migration: Option<String>,
}

impl Capability {
    /// Construct a supported capability for `wave`.
    pub fn supported(name: impl Into<String>, wave: u8) -> Self {
        Self {
            name: name.into(),
            wave,
            status: CapabilityStatus::Supported,
            migration: None,
        }
    }

    /// Construct a capability that is implemented but still migrating.
    pub fn migrating(name: impl Into<String>, wave: u8, migration: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            wave,
            status: CapabilityStatus::Migrating,
            migration: Some(migration.into()),
        }
    }

    /// Construct an unsupported capability deferred to `wave`, carrying the
    /// migration note a caller should act on to enable it.
    pub fn unsupported(name: impl Into<String>, wave: u8, migration: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            wave,
            status: CapabilityStatus::Unsupported,
            migration: Some(migration.into()),
        }
    }
}
