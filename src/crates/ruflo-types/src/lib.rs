//! Shared native Ruflo contract types.
//!
//! This crate hosts the machine-stable public contracts consumed by facade,
//! runtime, and compatibility layers.

mod capability;
mod error;

pub use capability::{Capability, CapabilityStatus};
pub use error::RufloError;
