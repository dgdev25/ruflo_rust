# Node SQLite memory to native RVF

`ruflo memory migrate-node` is the safe route for a project that already has
Node Ruflo memory metadata in SQLite. It preserves the SQLite records and
builds a fresh native RVF index from their text; it does not claim that a
Node-side HNSW or RVF file can be imported directly.

1. Stop the Node process that writes the database and make a normal filesystem
   copy of the project first.
2. Inspect the exact database without changing it:

   ```bash
   ruflo memory migrate-node --path .swarm/memory.db --dry-run
   ```

3. Rebuild the native index:

   ```bash
   ruflo memory migrate-node --path .swarm/memory.db
   ```

The command rejects a missing or non-SQLite source, creates a SQLite backup
before it changes semantic bindings, then reports how many active records it
re-embedded. Exact `memory retrieve` operations remain backed by the preserved
SQLite metadata. Re-run the command whenever the embedding backend changes.

Use `memory rebuild-index` only after the project is already native; it does
not create the migration backup or validate that an existing Node source was
selected.
