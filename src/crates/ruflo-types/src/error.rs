//! Stable error contract shared across native Ruflo crates.

use crate::Capability;
use std::fmt;

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

impl fmt::Display for RufloError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput { code, message } => write!(f, "{code}: {message}"),
            Self::Unauthenticated => f.write_str("unauthenticated"),
            Self::Unauthorized { capability } => {
                write!(f, "unauthorized capability `{capability}`")
            }
            Self::UnsupportedInWave { capability } => write!(
                f,
                "unsupported capability `{}` in wave {}",
                capability.name, capability.wave
            ),
            Self::RateLimited { retry_after_ms } => {
                write!(f, "rate limited; retry_after_ms={retry_after_ms}")
            }
            Self::Timeout => f.write_str("timeout"),
            Self::Cancelled => f.write_str("cancelled"),
            Self::LockConflict => f.write_str("lock conflict"),
            Self::MigrationFailed { message } => write!(f, "migration failed: {message}"),
            Self::UpstreamAdapter { message } => write!(f, "upstream adapter failure: {message}"),
        }
    }
}

impl std::error::Error for RufloError {}
