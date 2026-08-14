#![allow(clippy::all)]

mod appliance_state;
mod memory_sqlite;
mod migration;
mod port;
mod rvf_adapter;

pub use appliance_state::{AgentRow, ApplianceStore, JobRow, SpendLedger};
pub use memory_sqlite::{MemoryEntry, MemoryStats, MemoryStoreInput, SqliteMemoryStore};
pub use migration::{
    MigrationMetadata, MigrationOutcome, MigrationPlan, MigrationSession, RecoveryMetadata,
};
pub use port::PersistencePort;
pub use rvf_adapter::{
    AgentDbFixtureConfig, AgentDbMetric, AgentDbVectorRecord, AgenticFlowFixtureConfig,
    RvfPersistencePort, RvfSearchMatch, RvfStoreStatus, RVF_UPSTREAM_GIT_REV, RVF_UPSTREAM_GIT_URL,
};
