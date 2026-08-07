use std::fs;

use ruflo_storage::{
    AgentDbFixtureConfig, AgentDbMetric, AgentDbVectorRecord, AgenticFlowFixtureConfig,
    RvfPersistencePort,
};
use tempfile::TempDir;

#[test]
fn agentdb_round_trip_preserves_stable_order() {
    let temp = TempDir::new().unwrap();
    let fixture_path = temp.path().join("agentdb-compatible.rvf");
    let config = AgentDbFixtureConfig {
        dimension: 2,
        metric: AgentDbMetric::L2,
        ef_search: 32,
    };

    {
        let mut store = RvfPersistencePort::create_agentdb(&fixture_path, config.clone()).unwrap();
        let accepted = store
            .ingest_agentdb(&[
                AgentDbVectorRecord {
                    id: 30,
                    vector: vec![1.0, 0.0],
                },
                AgentDbVectorRecord {
                    id: 10,
                    vector: vec![1.0, 0.0],
                },
                AgentDbVectorRecord {
                    id: 20,
                    vector: vec![1.0, 0.0],
                },
            ])
            .unwrap();
        assert_eq!(accepted, 3);
        store.close().unwrap();
    }

    let store = RvfPersistencePort::open_agentdb(&fixture_path, config).unwrap();
    let results = store.search_agentdb(&[1.0, 0.0], 3).unwrap();
    let ordered_ids = results
        .into_iter()
        .map(|match_| match_.id)
        .collect::<Vec<_>>();

    assert_eq!(ordered_ids, vec![10, 20, 30]);
}

#[test]
fn agentdb_round_trip_survives_compaction() {
    let temp = TempDir::new().unwrap();
    let fixture_path = temp.path().join("agentdb-compact.rvf");
    let config = AgentDbFixtureConfig {
        dimension: 4,
        metric: AgentDbMetric::L2,
        ef_search: 64,
    };

    {
        let mut store = RvfPersistencePort::create_agentdb(&fixture_path, config.clone()).unwrap();
        let accepted = store
            .ingest_agentdb(&[
                AgentDbVectorRecord {
                    id: 1,
                    vector: vec![1.0, 0.0, 0.0, 0.0],
                },
                AgentDbVectorRecord {
                    id: 2,
                    vector: vec![0.0, 1.0, 0.0, 0.0],
                },
                AgentDbVectorRecord {
                    id: 3,
                    vector: vec![0.0, 0.0, 1.0, 0.0],
                },
            ])
            .unwrap();
        assert_eq!(accepted, 3);
        let reclaimed = store.compact_agentdb().unwrap();
        assert_eq!(reclaimed, 0);
        store.close().unwrap();
    }

    let store = RvfPersistencePort::open_agentdb(&fixture_path, config).unwrap();
    let results = store.search_agentdb(&[0.0, 1.0, 0.0, 0.0], 3).unwrap();

    assert_eq!(results.first().unwrap().id, 2);
}

#[test]
fn agentic_flow_fixture_reopens_existing_store() {
    let temp = TempDir::new().unwrap();
    let data_dir = temp.path().join("agentic-flow");
    fs::create_dir_all(&data_dir).unwrap();
    let config = AgenticFlowFixtureConfig::new(&data_dir, "agent-007", 4);

    {
        let mut store = RvfPersistencePort::create_agentic_flow(config.clone()).unwrap();
        let id = store
            .share_agentic_memory("handoff", "persisted", "shared", &[1.0, 0.5, 0.25, 0.125])
            .unwrap();
        assert_eq!(id, 1);
        let status = store.status();
        assert_eq!(status.total_vectors, 1);
        store.close().unwrap();
    }

    let reopened = RvfPersistencePort::open_agentic_flow(config).unwrap();
    let status = reopened.status();

    assert_eq!(status.total_vectors, 1);
    assert!(!status.read_only);
}
