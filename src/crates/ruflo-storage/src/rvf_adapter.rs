use std::path::{Path, PathBuf};

use ruflo_types::RufloError;
use rvf_adapter_agentdb::vector_store::{
    AgentDbMetric as UpstreamAgentDbMetric, VectorStoreConfig,
};
use rvf_adapter_agentdb::RvfVectorStore;
use rvf_adapter_agentic_flow::{AgenticFlowConfig, RvfSwarmStore};

pub const RVF_UPSTREAM_GIT_URL: &str = "https://github.com/ruvnet/RuVector.git";
pub const RVF_UPSTREAM_GIT_REV: &str = "597be6a753472f0521fe2def097116e717ed4332";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentDbMetric {
    Cosine,
    L2,
    InnerProduct,
}

impl Default for AgentDbMetric {
    fn default() -> Self {
        Self::Cosine
    }
}

impl From<AgentDbMetric> for UpstreamAgentDbMetric {
    fn from(value: AgentDbMetric) -> Self {
        match value {
            AgentDbMetric::Cosine => UpstreamAgentDbMetric::Cosine,
            AgentDbMetric::L2 => UpstreamAgentDbMetric::L2,
            AgentDbMetric::InnerProduct => UpstreamAgentDbMetric::InnerProduct,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentDbFixtureConfig {
    pub dimension: u16,
    pub metric: AgentDbMetric,
    pub ef_search: u16,
}

impl AgentDbFixtureConfig {
    pub fn new(dimension: u16) -> Self {
        Self {
            dimension,
            metric: AgentDbMetric::default(),
            ef_search: 100,
        }
    }

    fn into_upstream(self) -> VectorStoreConfig {
        VectorStoreConfig {
            dimension: self.dimension,
            metric: self.metric.into(),
            ef_search: self.ef_search,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgenticFlowFixtureConfig {
    pub data_dir: PathBuf,
    pub agent_id: String,
    pub dimension: u16,
}

impl AgenticFlowFixtureConfig {
    pub fn new(data_dir: impl Into<PathBuf>, agent_id: impl Into<String>, dimension: u16) -> Self {
        Self {
            data_dir: data_dir.into(),
            agent_id: agent_id.into(),
            dimension,
        }
    }

    fn into_upstream(self) -> AgenticFlowConfig {
        AgenticFlowConfig::new(self.data_dir, self.agent_id).with_dimension(self.dimension)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentDbVectorRecord {
    pub id: u64,
    pub vector: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RvfSearchMatch {
    pub id: u64,
    pub distance: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RvfStoreStatus {
    pub total_vectors: u64,
    pub current_epoch: u32,
    pub read_only: bool,
}

pub enum RvfPersistencePort {
    AgentDb(Box<RvfVectorStore>),
    AgenticFlow(Box<RvfSwarmStore>),
}

impl RvfPersistencePort {
    pub fn create_agentdb(
        path: impl AsRef<Path>,
        config: AgentDbFixtureConfig,
    ) -> Result<Self, RufloError> {
        let store = RvfVectorStore::create(path.as_ref(), config.into_upstream())
            .map_err(map_upstream_error)?;
        Ok(Self::AgentDb(Box::new(store)))
    }

    pub fn open_agentdb(
        path: impl AsRef<Path>,
        config: AgentDbFixtureConfig,
    ) -> Result<Self, RufloError> {
        let store = RvfVectorStore::open(path.as_ref(), config.into_upstream())
            .map_err(map_upstream_error)?;
        Ok(Self::AgentDb(Box::new(store)))
    }

    pub fn create_agentic_flow(config: AgenticFlowFixtureConfig) -> Result<Self, RufloError> {
        let store =
            RvfSwarmStore::create(config.into_upstream()).map_err(map_upstream_swarm_error)?;
        Ok(Self::AgenticFlow(Box::new(store)))
    }

    pub fn open_agentic_flow(config: AgenticFlowFixtureConfig) -> Result<Self, RufloError> {
        let store =
            RvfSwarmStore::open(config.into_upstream()).map_err(map_upstream_swarm_error)?;
        Ok(Self::AgenticFlow(Box::new(store)))
    }

    pub fn ingest_agentdb(&mut self, records: &[AgentDbVectorRecord]) -> Result<u64, RufloError> {
        match self {
            Self::AgentDb(store) => {
                let refs = records
                    .iter()
                    .map(|record| record.vector.as_slice())
                    .collect::<Vec<_>>();
                let ids = records.iter().map(|record| record.id).collect::<Vec<_>>();
                store
                    .add_vectors(&refs, &ids, None)
                    .map_err(map_upstream_error)
            }
            Self::AgenticFlow(_) => Err(RufloError::invalid_input(
                "storage.rvf.backend",
                "agentdb ingest requires an AgentDB RVF backend",
            )),
        }
    }

    pub fn search_agentdb(
        &self,
        query: &[f32],
        limit: usize,
    ) -> Result<Vec<RvfSearchMatch>, RufloError> {
        match self {
            Self::AgentDb(store) => store
                .search(query, limit, None)
                .map(|results| {
                    results
                        .into_iter()
                        .map(|result| RvfSearchMatch {
                            id: result.id,
                            distance: result.distance,
                        })
                        .collect()
                })
                .map_err(map_upstream_error),
            Self::AgenticFlow(_) => Err(RufloError::invalid_input(
                "storage.rvf.backend",
                "agentdb search requires an AgentDB RVF backend",
            )),
        }
    }

    pub fn compact_agentdb(&mut self) -> Result<u64, RufloError> {
        match self {
            Self::AgentDb(store) => store.compact().map_err(map_upstream_error),
            Self::AgenticFlow(_) => Err(RufloError::invalid_input(
                "storage.rvf.backend",
                "agentdb compaction requires an AgentDB RVF backend",
            )),
        }
    }

    pub fn share_agentic_memory(
        &mut self,
        key: &str,
        value: &str,
        namespace: &str,
        embedding: &[f32],
    ) -> Result<u64, RufloError> {
        match self {
            Self::AgenticFlow(store) => store
                .share_memory(key, value, namespace, embedding)
                .map_err(map_upstream_swarm_error),
            Self::AgentDb(_) => Err(RufloError::invalid_input(
                "storage.rvf.backend",
                "agentic-flow memory sharing requires an Agentic Flow RVF backend",
            )),
        }
    }

    pub fn status(&self) -> RvfStoreStatus {
        let status = match self {
            Self::AgentDb(store) => {
                let vectors = store.len();
                return RvfStoreStatus {
                    total_vectors: vectors,
                    current_epoch: 0,
                    read_only: false,
                };
            }
            Self::AgenticFlow(store) => store.status(),
        };
        RvfStoreStatus {
            total_vectors: status.total_vectors,
            current_epoch: status.current_epoch,
            read_only: status.read_only,
        }
    }

    pub fn close(self) -> Result<(), RufloError> {
        match self {
            Self::AgentDb(mut store) => store.save().map_err(map_upstream_error),
            Self::AgenticFlow(store) => store.close().map_err(map_upstream_swarm_error),
        }
    }
}

fn map_upstream_error(error: impl std::fmt::Display) -> RufloError {
    RufloError::UpstreamAdapter {
        message: error.to_string(),
    }
}

fn map_upstream_swarm_error(error: impl std::fmt::Display) -> RufloError {
    RufloError::UpstreamAdapter {
        message: error.to_string(),
    }
}
