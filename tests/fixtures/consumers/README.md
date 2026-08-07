# Consumer Fixture Backlog

Task 3 inventories the real consumer contracts before native feature work claims support.

Current checked-in consumer fixtures:

- `../cli/version.json` for the `ruflo --version` fast path
- `../cli/help.json` for the `ruflo --quiet --help` surface
- `../mcp/tools-list.json` for stdio MCP discovery

Still missing curated fixtures:

- `claude-flow` alias parity
- stdio MCP `tools/call`
- paired memory round trips
- migration and RVF/RVFA compatibility samples
- policy-denial transcripts
- POSIX and Windows platform hook samples

Until those fixtures exist, the contract matrix records the rows as blocked instead of supported.
