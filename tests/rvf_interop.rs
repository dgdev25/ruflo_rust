use std::fs;

use ruflo_storage::{
    AgentDbFixtureConfig, AgentDbMetric, AgentDbVectorRecord, AgenticFlowFixtureConfig,
    RvfPersistencePort,
};
use serde::Deserialize;
use tempfile::TempDir;

#[test]
fn agentdb_round_trip_preserves_stable_order() {
    let scenario = load_agentdb_scenario("tests/fixtures/rvf/agentdb-stable-order.json");
    let temp = TempDir::new().unwrap();
    let fixture_path = temp.path().join("agentdb-compatible.rvf");
    let config = scenario.config();

    {
        let mut store = RvfPersistencePort::create_agentdb(&fixture_path, config.clone()).unwrap();
        let accepted = store.ingest_agentdb(&scenario.records()).unwrap();
        assert_eq!(accepted, scenario.records.len() as u64);
        store.close().unwrap();
    }

    let store = RvfPersistencePort::open_agentdb(&fixture_path, config).unwrap();
    let results = store
        .search_agentdb(&scenario.query, scenario.limit)
        .unwrap();
    let ordered_ids = results
        .into_iter()
        .map(|match_| match_.id)
        .collect::<Vec<_>>();

    assert_eq!(ordered_ids, scenario.expected_ids);
}

#[test]
fn agentdb_round_trip_survives_compaction() {
    let scenario = load_agentdb_scenario("tests/fixtures/rvf/agentdb-compaction.json");
    let temp = TempDir::new().unwrap();
    let fixture_path = temp.path().join("agentdb-compact.rvf");
    let config = scenario.config();

    {
        let mut store = RvfPersistencePort::create_agentdb(&fixture_path, config.clone()).unwrap();
        let accepted = store.ingest_agentdb(&scenario.records()).unwrap();
        assert_eq!(accepted, scenario.records.len() as u64);
        let reclaimed = store.compact_agentdb().unwrap();
        assert_eq!(reclaimed, scenario.expected_reclaimed);
        store.close().unwrap();
    }

    let store = RvfPersistencePort::open_agentdb(&fixture_path, config).unwrap();
    let results = store
        .search_agentdb(&scenario.query, scenario.limit)
        .unwrap();

    assert_eq!(results.first().unwrap().id, scenario.expected_ids[0]);
}

#[test]
fn agentic_flow_fixture_reopens_existing_store() {
    let scenario = load_agentic_flow_scenario("tests/fixtures/rvf/agentic-flow-reopen.json");
    let temp = TempDir::new().unwrap();
    let data_dir = temp.path().join("agentic-flow");
    fs::create_dir_all(&data_dir).unwrap();
    let config = AgenticFlowFixtureConfig::new(&data_dir, &scenario.agent_id, scenario.dimension);

    {
        let mut store = RvfPersistencePort::create_agentic_flow(config.clone()).unwrap();
        let id = store
            .share_agentic_memory(
                &scenario.key,
                &scenario.value,
                &scenario.namespace,
                &scenario.vector,
            )
            .unwrap();
        assert_eq!(id, scenario.expected_id);
        let status = store.status();
        assert_eq!(status.total_vectors, scenario.expected_total_vectors);
        store.close().unwrap();
    }

    let reopened = RvfPersistencePort::open_agentic_flow(config).unwrap();
    let status = reopened.status();

    assert_eq!(status.total_vectors, scenario.expected_total_vectors);
    assert!(!status.read_only);
}

#[derive(Debug, Deserialize)]
struct AgentDbScenario {
    dimension: u16,
    ef_search: u16,
    records: Vec<VectorRecord>,
    query: Vec<f32>,
    limit: usize,
    expected_ids: Vec<u64>,
    #[serde(default)]
    expected_reclaimed: u64,
}

impl AgentDbScenario {
    fn config(&self) -> AgentDbFixtureConfig {
        AgentDbFixtureConfig {
            dimension: self.dimension,
            metric: AgentDbMetric::L2,
            ef_search: self.ef_search,
        }
    }

    fn records(&self) -> Vec<AgentDbVectorRecord> {
        self.records
            .iter()
            .map(|record| AgentDbVectorRecord {
                id: record.id,
                vector: record.vector.clone(),
            })
            .collect()
    }
}

#[derive(Debug, Deserialize)]
struct VectorRecord {
    id: u64,
    vector: Vec<f32>,
}

#[derive(Debug, Deserialize)]
struct AgenticFlowScenario {
    agent_id: String,
    dimension: u16,
    key: String,
    value: String,
    namespace: String,
    vector: Vec<f32>,
    expected_id: u64,
    expected_total_vectors: u64,
}

fn load_agentdb_scenario(path: &str) -> AgentDbScenario {
    load_fixture(path)
}

fn load_agentic_flow_scenario(path: &str) -> AgenticFlowScenario {
    load_fixture(path)
}

fn load_fixture<T: for<'de> Deserialize<'de>>(path: &str) -> T {
    let contents = fs::read_to_string(path).unwrap();
    serde_json::from_str(&contents).unwrap()
}
