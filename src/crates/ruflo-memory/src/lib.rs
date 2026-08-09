//! Semantic memory composition for Ruflo.
//!
//! SQLite remains the durable metadata and exact-read authority; RVF's
//! AgentDB adapter owns vector storage and HNSW search. Callers must provide
//! a real embedding provider. This crate intentionally has no hash-vector
//! fallback because that would misrepresent lexical similarity as semantics.

use std::path::Path;

use ruflo_storage::{
    AgentDbFixtureConfig, AgentDbVectorRecord, MemoryEntry, MemoryStoreInput, RvfPersistencePort,
    SqliteMemoryStore,
};
use ruflo_types::RufloError;

pub mod hybrid;

pub const DEFAULT_EMBEDDING_DIMENSIONS: usize = 384;

pub trait EmbeddingProvider: Send + Sync {
    fn dimensions(&self) -> usize;
    fn embed(&self, text: &str) -> Result<Vec<f32>, RufloError>;
}

pub struct SemanticMemoryStore<E> {
    metadata: SqliteMemoryStore,
    vectors: RvfPersistencePort,
    embedder: E,
}

impl<E: EmbeddingProvider> SemanticMemoryStore<E> {
    pub fn create(project_root: &Path, rvf_path: &Path, embedder: E) -> Result<Self, RufloError> {
        let config = AgentDbFixtureConfig::new(embedding_dimension(&embedder)?);
        Ok(Self {
            metadata: SqliteMemoryStore::open(project_root, project_root.join(".swarm/memory.db"))?,
            vectors: RvfPersistencePort::create_agentdb(rvf_path, config)?,
            embedder,
        })
    }

    /// Reopen a durable AgentDB/RVF-backed semantic-memory store.
    pub fn open(project_root: &Path, rvf_path: &Path, embedder: E) -> Result<Self, RufloError> {
        let config = AgentDbFixtureConfig::new(embedding_dimension(&embedder)?);
        Ok(Self {
            metadata: SqliteMemoryStore::open(project_root, project_root.join(".swarm/memory.db"))?,
            vectors: RvfPersistencePort::open_agentdb(rvf_path, config)?,
            embedder,
        })
    }

    pub fn store(&mut self, input: &MemoryStoreInput) -> Result<MemoryEntry, RufloError> {
        let existing = self.metadata.retrieve(&input.namespace, &input.key)?;
        if existing.is_some() && !input.upsert {
            return Err(RufloError::invalid_input(
                "memory.key.exists",
                format!(
                    "memory key `{}` already exists in namespace `{}`",
                    input.key, input.namespace
                ),
            ));
        }
        let vector = self.embedder.embed(&input.content)?;
        self.validate_vector(&vector)?;
        let semantic_id = semantic_id(&input.namespace, &input.key);
        if let Some(bound) = self.metadata.retrieve_semantic_id(semantic_id)? {
            if bound.namespace != input.namespace || bound.key != input.key {
                return Err(RufloError::invalid_input(
                    "memory.semantic_id.collision",
                    "semantic ID collides with a different memory entry",
                ));
            }
        }
        // RVF's AgentDB adapter treats an existing numeric ID as a vector
        // replacement and invalidates any stale HNSW graph itself. Do not
        // delete first: a tombstone followed by the same ID would hide the
        // replacement until compaction.
        self.vectors.ingest_agentdb(&[AgentDbVectorRecord {
            id: semantic_id,
            vector,
        }])?;
        let entry = self.metadata.store(input)?;
        self.metadata
            .set_semantic_id(&entry.namespace, &entry.key, semantic_id)?;
        self.metadata
            .retrieve(&entry.namespace, &entry.key)?
            .ok_or_else(|| RufloError::UpstreamAdapter {
                message: "stored semantic memory entry was not readable".into(),
            })
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<MemoryEntry>, RufloError> {
        let vector = self.embedder.embed(query)?;
        self.validate_vector(&vector)?;
        let mut entries = Vec::new();
        for result in self.vectors.search_agentdb(&vector, limit)? {
            if let Some(entry) = self.metadata.retrieve_semantic_id(result.id)? {
                entries.push(entry);
            }
        }
        Ok(entries)
    }

    /// Source-compatible hybrid retrieval over durable metadata. This is the
    /// Node V3 BM25 + dense-cosine + MMR policy; document vectors are
    /// deterministically re-derived from the configured provider so the
    /// method remains valid after reopening an RVF store.
    pub fn search_hybrid(&self, query: &str, limit: usize) -> Result<Vec<MemoryEntry>, RufloError> {
        let query_vector = self.embedder.embed(query)?;
        self.validate_vector(&query_vector)?;
        let entries = self.metadata.list(None, usize::MAX)?;
        let documents = entries.iter().map(|entry| hybrid::tokenize(&entry.content)).collect::<Vec<_>>();
        let stats = hybrid::build_corpus_stats(&documents);
        let query_tokens = hybrid::tokenize(query);
        let mut vectors = Vec::with_capacity(entries.len());
        let mut cosine = Vec::with_capacity(entries.len());
        let mut lexical = Vec::with_capacity(entries.len());
        for (entry, document) in entries.iter().zip(&documents) {
            let vector = self.embedder.embed(&entry.content)?;
            self.validate_vector(&vector)?;
            cosine.push(hybrid::cosine_similarity(&query_vector, &vector));
            lexical.push(hybrid::bm25_score(&query_tokens, document, &stats));
            vectors.push(vector);
        }
        let scores = hybrid::hybrid_scores(&cosine, &lexical, 0.6).ok_or_else(|| {
            RufloError::invalid_input("memory.hybrid", "failed to align hybrid scores")
        })?;
        let candidates = entries.into_iter().zip(vectors).zip(scores).map(|((value, embedding), relevance)| {
            hybrid::Ranked { value, embedding, relevance }
        }).collect();
        Ok(hybrid::mmr_rerank(candidates, limit, 0.5).into_iter().map(|candidate| candidate.value).collect())
    }

    pub fn retrieve(&self, namespace: &str, key: &str) -> Result<Option<MemoryEntry>, RufloError> {
        self.metadata.retrieve(namespace, key)
    }

    pub fn list(
        &self,
        namespace: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>, RufloError> {
        self.metadata.list(namespace, limit)
    }

    /// Flush and close the upstream RVF store before process shutdown or a
    /// subsequent `open`. Metadata is committed per SQLite operation; RVF
    /// requires this explicit lifecycle boundary to persist HNSW segments.
    pub fn close(self) -> Result<(), RufloError> {
        self.vectors.close()
    }

    fn validate_vector(&self, vector: &[f32]) -> Result<(), RufloError> {
        if vector.len() != self.embedder.dimensions()
            || vector.iter().any(|value| !value.is_finite())
        {
            return Err(RufloError::invalid_input(
                "memory.embedding",
                "embedding provider returned an invalid vector",
            ));
        }
        Ok(())
    }
}

fn embedding_dimension<E: EmbeddingProvider>(embedder: &E) -> Result<u16, RufloError> {
    let dimension = u16::try_from(embedder.dimensions()).map_err(|_| {
        RufloError::invalid_input(
            "memory.embedding.dimensions",
            "embedding dimensions exceed RVF limits",
        )
    })?;
    if dimension == 0 {
        return Err(RufloError::invalid_input(
            "memory.embedding.dimensions",
            "embedding dimensions must be positive",
        ));
    }
    Ok(dimension)
}

fn semantic_id(namespace: &str, key: &str) -> u64 {
    // Stable FNV-1a mapping. The value is constrained to SQLite's signed
    // range, and the metadata UNIQUE constraint detects an improbable clash.
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in namespace
        .bytes()
        .chain(std::iter::once(0))
        .chain(key.bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash & i64::MAX as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestEmbedder;
    impl EmbeddingProvider for TestEmbedder {
        fn dimensions(&self) -> usize {
            2
        }
        fn embed(&self, text: &str) -> Result<Vec<f32>, RufloError> {
            Ok(if text.contains("auth") {
                vec![1.0, 0.0]
            } else {
                vec![0.0, 1.0]
            })
        }
    }

    #[test]
    fn stores_metadata_and_retrieves_through_upstream_rvf_search() {
        let project = tempfile::tempdir().unwrap();
        let rvf = project.path().join("memory.rvf");
        let mut store = SemanticMemoryStore::create(project.path(), &rvf, TestEmbedder).unwrap();
        store
            .store(&MemoryStoreInput {
                key: "auth-pattern".into(),
                namespace: "patterns".into(),
                content: "auth tokens".into(),
                memory_type: "semantic".into(),
                tags_json: None,
                provenance_type: "tool_result".into(),
                upsert: true,
            })
            .unwrap();
        let matches = store.search("auth flow", 1).unwrap();
        assert_eq!(matches[0].key, "auth-pattern");
        assert!(matches[0].semantic_id.is_some());
    }

    #[test]
    fn reopens_and_replaces_an_upserted_vector() {
        let project = tempfile::tempdir().unwrap();
        let rvf = project.path().join("memory.rvf");
        let input = MemoryStoreInput {
            key: "pattern".into(),
            namespace: "patterns".into(),
            content: "auth tokens".into(),
            memory_type: "semantic".into(),
            tags_json: None,
            provenance_type: "tool_result".into(),
            upsert: true,
        };
        let mut store = SemanticMemoryStore::create(project.path(), &rvf, TestEmbedder).unwrap();
        store.store(&input).unwrap();
        store.close().unwrap();

        let mut reopened = SemanticMemoryStore::open(project.path(), &rvf, TestEmbedder).unwrap();
        let matches = reopened.search("auth flow", 1).unwrap();
        assert_eq!(
            matches.len(),
            1,
            "semantic ID was {}",
            semantic_id("patterns", "pattern")
        );
        assert_eq!(matches[0].content, "auth tokens");
        reopened
            .store(&MemoryStoreInput {
                content: "coordination strategy".into(),
                ..input
            })
            .unwrap();
        assert_eq!(
            reopened.search("coordination", 1).unwrap()[0].content,
            "coordination strategy"
        );
        assert_eq!(reopened.list(Some("patterns"), 10).unwrap().len(), 1);
    }

    #[test]
    fn hybrid_search_combines_lexical_and_dense_durable_records() {
        let project = tempfile::tempdir().unwrap();
        let rvf = project.path().join("memory.rvf");
        let mut store = SemanticMemoryStore::create(project.path(), &rvf, TestEmbedder).unwrap();
        for (key, content) in [
            ("auth-exact", "auth token rotation guidance"),
            ("other", "coordination strategy"),
        ] {
            store.store(&MemoryStoreInput {
                key: key.into(),
                namespace: "patterns".into(),
                content: content.into(),
                memory_type: "semantic".into(),
                tags_json: None,
                provenance_type: "tool_result".into(),
                upsert: true,
            }).unwrap();
        }
        let matches = store.search_hybrid("auth token", 1).unwrap();
        assert_eq!(matches[0].key, "auth-exact");
    }
}
