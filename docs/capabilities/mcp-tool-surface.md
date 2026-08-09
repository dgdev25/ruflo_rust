# Native MCP tool surface

Status: enforced by tests.

`tools/list` is an executable compatibility contract. The native MCP server
advertises only tools with both a native handler and a typed `inputSchema`:

| Tool | Required input | Native behavior |
| --- | --- | --- |
| `agent_spawn` | none | Creates a Ruflo-tracked agent record. |
| `memory_store` | `key`, `value` | Stores a durable memory record. |
| `memory_retrieve` | `key` | Reads a durable memory record. |
| `memory_search` | `query` | Searches durable memory records. |

Historical TypeScript catalog names are deliberately not listed. Calling one
returns the deterministic `tool.unsupported` invalid-parameters error rather
than a generic state-file response. A name may be added only with a native
handler, a complete JSON input schema, and direct dispatcher tests.
