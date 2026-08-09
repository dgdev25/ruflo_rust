# Node-to-Rust parity remediation checklist

This is the live checked implementation ledger. A check means source-derived
contracts and executable evidence exist; it never means a command name merely
parses.

- [x] Secure Node-compatible auth persistence and token handling
- [x] Deterministic errors for unsupported CLI subcommands
- [x] MCP only advertises implemented, typed tool contracts
- [x] Machine-derived capability manifest and release gate
- [ ] Source-equivalent AgentDB/RVF semantic retrieval fixtures
  - [x] BGE-base tokenizer, query-prefix, CLS pooling, and 768-dimensional RVF path
  - [x] Node hybrid policy: multi-field BM25, meta-record penalty, and MMR reranking
  - [ ] Node-produced ranking corpus and populated cross-runtime RVF interoperability fixture
- [x] Safe native plugin lifecycle (without executable JavaScript plugins)
- [x] Bounded provider connectivity verification and classifications
- [x] Populated Node V3 memory migration/interoperability fixture
- [x] Shared process-independent Rust core API
- [x] Real napi-rs Ruflo addon, Node ABI tests, and equality-gated persistent benchmark
- [ ] Platform release matrix for CLI and addon packages
- [x] Reconciled release docs, audit evidence, and ADR implementation notes
