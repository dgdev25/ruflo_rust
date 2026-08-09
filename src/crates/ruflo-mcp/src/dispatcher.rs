use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use ruflo_config::{Caller, DispatchRequest, EffectiveConfig, RegisteredCapability, ToolPolicy};
use ruflo_types::{Capability, RufloError};
use serde_json::{json, Value};

static CORRELATION_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestContext {
    pub caller: Caller,
    pub identity: Option<RequestIdentity>,
    pub request_bytes: usize,
    pub active_executions: usize,
    pub duration_ms: u64,
}

impl RequestContext {
    pub fn local(request_bytes: usize) -> Self {
        Self {
            caller: Caller::local(),
            identity: None,
            request_bytes,
            active_executions: 0,
            duration_ms: 0,
        }
    }

    pub fn remote(
        identity: RequestIdentity,
        request_bytes: usize,
        active_executions: usize,
        duration_ms: u64,
    ) -> Self {
        Self {
            caller: Caller::named(identity.subject.clone()),
            identity: Some(identity),
            request_bytes,
            active_executions,
            duration_ms,
        }
    }

    fn allows_capability(&self, capability: &str) -> bool {
        self.identity
            .as_ref()
            .map(|identity| identity.capabilities.contains(capability))
            .unwrap_or(true)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestIdentity {
    pub subject: String,
    pub issuer: String,
    pub audience: String,
    pub expires_at_epoch_s: u64,
    pub capabilities: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDefinition {
    pub name: &'static str,
    pub description: &'static str,
    pub capability: Capability,
}

impl ToolDefinition {
    pub fn json_schema(&self) -> Value {
        json!({
            "name": self.name,
            "description": self.description,
            "inputSchema": {
                "type": "object",
                "additionalProperties": false
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolResult {
    pub content: Vec<ToolResponseContent>,
    pub structured_content: Option<Value>,
}

impl ToolResult {
    pub fn text(text: impl Into<String>, structured_content: Option<Value>) -> Self {
        Self {
            content: vec![ToolResponseContent::Text { text: text.into() }],
            structured_content,
        }
    }

    pub fn into_json(self) -> Value {
        let mut result = json!({
            "content": self.content.into_iter().map(ToolResponseContent::into_json).collect::<Vec<_>>()
        });

        if let Some(structured_content) = self.structured_content {
            result["structuredContent"] = structured_content;
        }

        result
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolResponseContent {
    Text { text: String },
}

impl ToolResponseContent {
    fn into_json(self) -> Value {
        match self {
            Self::Text { text } => json!({
                "type": "text",
                "text": text
            }),
        }
    }
}

type ToolHandler = fn(&Value) -> Result<ToolResult, RufloError>;

#[derive(Clone)]
struct RegisteredTool {
    definition: ToolDefinition,
    handler: ToolHandler,
}

#[derive(Clone)]
pub struct Dispatcher {
    config: EffectiveConfig,
    policy: ToolPolicy,
    registry: Vec<RegisteredTool>,
}

impl Dispatcher {
    pub fn from_config(config: EffectiveConfig) -> Result<Self, RufloError> {
        let registry = build_registry();
        let mut capabilities = registry
            .iter()
            .map(|tool| {
                RegisteredCapability::new(tool.definition.name, tool.definition.capability.clone())
            })
            .collect::<Vec<_>>();
        // Register extended tools in the policy so their calls pass authorization.
        for (name, _desc) in crate::tools_extra::definitions() {
            capabilities.push(RegisteredCapability::new(
                name,
                Capability::supported(name, 1),
            ));
        }
        let policy = ToolPolicy::from_config(&config, &capabilities)?;
        Ok(Self {
            config,
            policy,
            registry,
        })
    }

    pub fn list_tools(&self, context: &RequestContext) -> Value {
        let mut tools = self
            .registry
            .iter()
            .filter(|tool| {
                self.policy
                    .is_discoverable(&context.caller, tool.definition.name)
            })
            .filter(|tool| context.allows_capability(&tool.definition.capability.name))
            .map(|tool| tool.definition.json_schema())
            .collect::<Vec<_>>();
        // Append the extended tool set definitions — respecting deny policy.
        for (name, desc) in crate::tools_extra::definitions() {
            if self.policy.is_discoverable(&context.caller, name) {
                tools.push(json!({
                    "name": name,
                    "description": desc,
                    "inputSchema": {"type": "object", "properties": {}},
                }));
            }
        }
        json!({ "tools": tools })
    }

    pub fn call(&self, context: RequestContext, call: ToolCall) -> Result<ToolResult, RufloError> {
        // Check core registry first.
        let tool = self
            .registry
            .iter()
            .find(|tool| tool.definition.name == call.name);

        // If not in core registry, check the extended tool set (tools_extra).
        let is_extra = tool.is_none()
            && crate::tools_extra::definitions()
                .iter()
                .any(|(name, _)| *name == call.name);
        if is_extra {
            // Authorize + capability-check then dispatch to tools_extra.
            self.policy.authorize_request(
                &context.caller,
                &call.name,
                DispatchRequest {
                    request_bytes: context.request_bytes,
                    active_executions: context.active_executions,
                    duration_ms: context.duration_ms,
                },
            )?;
            // Apply the same per-request capability gate the core path uses.
            // The capability name matches the tool name (registered above).
            if !context.allows_capability(&call.name) {
                return Err(RufloError::unauthorized(&call.name));
            }
            return crate::tools_extra::handle(&call.name, &call.arguments);
        }

        let tool = tool.ok_or_else(|| {
            RufloError::invalid_input("tool.not_found", format!("unknown tool `{}`", call.name))
        })?;

        self.policy.authorize_request(
            &context.caller,
            &call.name,
            DispatchRequest {
                request_bytes: context.request_bytes,
                active_executions: context.active_executions,
                duration_ms: context.duration_ms,
            },
        )?;
        if !context.allows_capability(&tool.definition.capability.name) {
            return Err(RufloError::unauthorized(
                tool.definition.capability.name.clone(),
            ));
        }

        (tool.handler)(&call.arguments)
    }

    pub fn config(&self) -> &EffectiveConfig {
        &self.config
    }
}

fn build_registry() -> Vec<RegisteredTool> {
    vec![
        RegisteredTool {
            definition: ToolDefinition {
                name: "agent_spawn",
                description: "Spawn a Ruflo-tracked agent.",
                capability: Capability::supported("agent.spawn", 1),
            },
            handler: agent_spawn,
        },
        RegisteredTool {
            definition: ToolDefinition {
                name: "memory_store",
                description: "Store a persistent memory entry.",
                capability: Capability::supported("memory.store", 1),
            },
            handler: memory_store,
        },
        RegisteredTool {
            definition: ToolDefinition {
                name: "memory_retrieve",
                description: "Retrieve a persistent memory entry by key.",
                capability: Capability::supported("memory.retrieve", 1),
            },
            handler: memory_retrieve,
        },
        RegisteredTool {
            definition: ToolDefinition {
                name: "memory_search",
                description: "Find persistent memories with keyword fallback.",
                capability: Capability::supported("memory.search", 1),
            },
            handler: memory_search,
        },
    ]
}

fn agent_spawn(arguments: &Value) -> Result<ToolResult, RufloError> {
    maybe_sleep(arguments)?;
    let role = optional_string(arguments, "role")?.unwrap_or_else(|| "generalist".to_string());
    let agent_id = format!("agent-{role}");
    Ok(ToolResult::text(
        format!("spawned agent `{agent_id}`"),
        Some(json!({
            "agentId": agent_id,
            "role": role,
            "status": "spawned"
        })),
    ))
}

/// Open a cached SQLite memory store. The store is re-created per call today;
/// a future optimization would cache it in the Dispatcher. For now the
/// open_from_current_dir cost is bounded (CREATE TABLE IF NOT EXISTS is a
/// no-op after the first call, and WAL mode keeps it fast).
fn open_memory_store() -> Result<ruflo_storage::SqliteMemoryStore, RufloError> {
    ruflo_storage::SqliteMemoryStore::open_from_current_dir()
}

fn memory_store(arguments: &Value) -> Result<ToolResult, RufloError> {
    let key = required_string(arguments, "key")?;
    let content = required_value_content(arguments, "value")?;
    let namespace =
        optional_string(arguments, "namespace")?.unwrap_or_else(|| "default".to_string());
    let memory_type = optional_string(arguments, "type")?.unwrap_or_else(|| "semantic".to_string());
    let provenance_type =
        optional_string(arguments, "provenance_type")?.unwrap_or_else(|| "unknown".to_string());
    let tags_json = optional_tags(arguments)?;
    let upsert = optional_bool(arguments, "upsert")?.unwrap_or(true);
    let store = open_memory_store()?;
    let entry = store.store(&ruflo_storage::MemoryStoreInput {
        key,
        namespace,
        content,
        memory_type,
        tags_json,
        provenance_type,
        upsert,
    })?;
    Ok(ToolResult::text(
        format!("stored memory `{}`", entry.key),
        Some(json!({
            "id": entry.id,
            "key": entry.key,
            "namespace": entry.namespace,
            "stored": true,
            "backend": "sqlite-keyword-fallback"
        })),
    ))
}

fn memory_retrieve(arguments: &Value) -> Result<ToolResult, RufloError> {
    let key = required_string(arguments, "key")?;
    let namespace =
        optional_string(arguments, "namespace")?.unwrap_or_else(|| "default".to_string());
    let store = open_memory_store()?;
    let entry = store.retrieve(&namespace, &key)?;
    match entry {
        Some(entry) => Ok(ToolResult::text(
            format!("retrieved memory `{}`", entry.key),
            Some(memory_entry_json(entry)),
        )),
        None => Ok(ToolResult::text(
            format!("memory `{key}` not found"),
            Some(json!({ "key": key, "namespace": namespace, "found": false })),
        )),
    }
}

fn memory_search(arguments: &Value) -> Result<ToolResult, RufloError> {
    maybe_sleep(arguments)?;
    let query = required_string(arguments, "query")?;
    let namespace = optional_string(arguments, "namespace")?;
    let limit = optional_usize(arguments, "limit")?.unwrap_or(10);

    // Try semantic search first via the deterministic hash vectorizer, then
    // fall back to keyword search. The semantic path produces a query vector
    // from the search terms and would use the RVF adapter if an AgentDB store
    // is open; for now it delegates to keyword (the RVF adapter needs a
    // pre-populated vector index which the native build doesn't create).
    let store = open_memory_store()?;
    let matches = store.search_keyword(namespace.as_deref(), &query, limit)?;
    let structured_matches = matches
        .into_iter()
        .map(memory_entry_json)
        .collect::<Vec<_>>();
    let text = if structured_matches.is_empty() {
        format!("no stored matches for `{query}`")
    } else {
        format!(
            "found {} stored matches for `{query}`",
            structured_matches.len()
        )
    };
    Ok(ToolResult::text(
        text,
        Some(json!({
            "query": query,
            "matches": structured_matches,
            "backend": "sqlite-keyword-fallback"
        })),
    ))
}

fn memory_entry_json(entry: ruflo_storage::MemoryEntry) -> Value {
    json!({
        "id": entry.id,
        "key": entry.key,
        "namespace": entry.namespace,
        "content": entry.content,
        "type": entry.memory_type,
        "provenanceType": entry.provenance_type,
        "found": true
    })
}

fn maybe_sleep(arguments: &Value) -> Result<(), RufloError> {
    let Some(raw) = arguments.get("sleep_ms") else {
        return Ok(());
    };
    let sleep_ms = raw.as_u64().ok_or_else(|| {
        RufloError::invalid_input(
            "tool.invalid_arguments",
            "field `sleep_ms` must be an unsigned integer",
        )
    })?;
    // Cap at 5s to prevent resource exhaustion via unbounded blocking.
    let capped = sleep_ms.min(5000);
    std::thread::sleep(Duration::from_millis(capped));
    Ok(())
}

fn required_string(arguments: &Value, field: &'static str) -> Result<String, RufloError> {
    optional_string(arguments, field)?.ok_or_else(|| {
        RufloError::invalid_input(
            "tool.invalid_arguments",
            format!("missing required string field `{field}`"),
        )
    })
}

fn optional_string(arguments: &Value, field: &'static str) -> Result<Option<String>, RufloError> {
    match arguments.get(field) {
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(RufloError::invalid_input(
            "tool.invalid_arguments",
            format!("field `{field}` must be a string"),
        )),
        None => Ok(None),
    }
}

fn optional_bool(arguments: &Value, field: &'static str) -> Result<Option<bool>, RufloError> {
    match arguments.get(field) {
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(RufloError::invalid_input(
            "tool.invalid_arguments",
            format!("field `{field}` must be a boolean"),
        )),
        None => Ok(None),
    }
}

fn optional_usize(arguments: &Value, field: &'static str) -> Result<Option<usize>, RufloError> {
    match arguments.get(field) {
        Some(Value::Number(value)) => value
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .map(Some)
            .ok_or_else(|| {
                RufloError::invalid_input(
                    "tool.invalid_arguments",
                    format!("field `{field}` must be an unsigned integer"),
                )
            }),
        Some(_) => Err(RufloError::invalid_input(
            "tool.invalid_arguments",
            format!("field `{field}` must be an unsigned integer"),
        )),
        None => Ok(None),
    }
}

fn optional_tags(arguments: &Value) -> Result<Option<String>, RufloError> {
    match arguments.get("tags") {
        Some(Value::Array(tags)) if tags.iter().all(Value::is_string) => {
            serde_json::to_string(tags)
                .map(Some)
                .map_err(|error| RufloError::UpstreamAdapter {
                    message: format!("memory.tags: {error}"),
                })
        }
        Some(Value::Array(_)) => Err(RufloError::invalid_input(
            "tool.invalid_arguments",
            "field `tags` must contain only strings",
        )),
        Some(_) => Err(RufloError::invalid_input(
            "tool.invalid_arguments",
            "field `tags` must be an array",
        )),
        None => Ok(None),
    }
}

fn required_value_content(arguments: &Value, field: &'static str) -> Result<String, RufloError> {
    match arguments.get(field) {
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(value.clone()),
        Some(Value::String(_)) => Err(RufloError::invalid_input(
            "tool.invalid_arguments",
            format!("field `{field}` must not be empty"),
        )),
        Some(value) => serde_json::to_string(value).map_err(|error| RufloError::UpstreamAdapter {
            message: format!("memory.value: {error}"),
        }),
        None => Err(RufloError::invalid_input(
            "tool.invalid_arguments",
            format!("missing required field `{field}`"),
        )),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorObject {
    pub code: i64,
    pub message: String,
    pub data: ErrorResponseData,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorResponseData {
    pub correlation_id: String,
    pub details: Value,
}

impl ErrorObject {
    pub fn into_json(self) -> Value {
        json!({
            "code": self.code,
            "message": self.message,
            "data": {
                "correlationId": self.data.correlation_id,
                "details": self.data.details
            }
        })
    }
}

pub fn map_error(error: RufloError) -> ErrorObject {
    let correlation_id = format!(
        "corr-{:08}",
        CORRELATION_COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    match error {
        RufloError::InvalidInput { code, message } => ErrorObject {
            code: -32602,
            message: "Invalid params".to_string(),
            data: ErrorResponseData {
                correlation_id,
                details: json!({ "code": code, "message": message }),
            },
        },
        RufloError::Unauthenticated => ErrorObject {
            code: -32000,
            message: "Unauthenticated".to_string(),
            data: ErrorResponseData {
                correlation_id,
                details: json!({}),
            },
        },
        RufloError::Unauthorized { capability } => ErrorObject {
            code: -32001,
            message: "Unauthorized".to_string(),
            data: ErrorResponseData {
                correlation_id,
                details: json!({ "capability": capability }),
            },
        },
        RufloError::UnsupportedInWave { capability } => ErrorObject {
            code: -32002,
            message: "Unsupported capability".to_string(),
            data: ErrorResponseData {
                correlation_id,
                details: json!({
                    "capability": capability.name,
                    "wave": capability.wave,
                    "migration": capability.migration
                }),
            },
        },
        RufloError::RateLimited { retry_after_ms } => ErrorObject {
            code: -32003,
            message: "Rate limited".to_string(),
            data: ErrorResponseData {
                correlation_id,
                details: json!({ "retryAfterMs": retry_after_ms }),
            },
        },
        RufloError::Timeout => ErrorObject {
            code: -32004,
            message: "Timeout".to_string(),
            data: ErrorResponseData {
                correlation_id,
                details: json!({}),
            },
        },
        RufloError::Cancelled => ErrorObject {
            code: -32005,
            message: "Cancelled".to_string(),
            data: ErrorResponseData {
                correlation_id,
                details: json!({}),
            },
        },
        RufloError::LockConflict => ErrorObject {
            code: -32006,
            message: "Lock conflict".to_string(),
            data: ErrorResponseData {
                correlation_id,
                details: json!({}),
            },
        },
        RufloError::MigrationFailed { message } => ErrorObject {
            code: -32007,
            message: "Migration failed".to_string(),
            data: ErrorResponseData {
                correlation_id,
                details: json!({ "message": message }),
            },
        },
        RufloError::UpstreamAdapter { message } => ErrorObject {
            code: -32008,
            message: "Upstream adapter failure".to_string(),
            data: ErrorResponseData {
                correlation_id,
                details: json!({ "message": message }),
            },
        },
    }
}
