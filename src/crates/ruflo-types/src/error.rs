//! Stable error contract shared across native Ruflo crates.

use crate::Capability;

/// Machine-stable error surface for native Ruflo crates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RufloError {
    InvalidInput { code: &'static str, message: String },
    Unauthenticated,
    Unauthorized { capability: String },
    UnsupportedInWave { capability: Capability },
    RateLimited { retry_after_ms: u64 },
    Timeout,
    Cancelled,
    LockConflict,
    MigrationFailed { message: String },
    UpstreamAdapter { message: String },
}

impl RufloError {
    /// Construct the typed unsupported-in-wave error required by deferred
    /// capabilities.
    pub fn unsupported(capability: Capability) -> Self {
        Self::UnsupportedInWave { capability }
    }

    /// Construct a stable invalid-input error.
    pub fn invalid_input(code: &'static str, message: impl Into<String>) -> Self {
        Self::InvalidInput {
            code,
            message: message.into(),
        }
    }

    /// Construct a stable unauthorized error for a named capability.
    pub fn unauthorized(capability: impl Into<String>) -> Self {
        Self::Unauthorized {
            capability: capability.into(),
        }
    }
}
