use std::sync::atomic::{AtomicU64, Ordering};

use ruflo_config::{Caller, DispatchRequest, EffectiveConfig, RegisteredCapability, ToolPolicy};
use ruflo_types::{Capability, RufloError};
use serde_json::{json, Value};

static CORRELATION_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestContext {
    pub caller: Caller,
    pub request_bytes: usize,
    pub active_executions: usize,
    pub duration_ms: u64,
}

impl RequestContext {
    pub fn local(request_bytes: usize) -> Self {
        Self {
            caller: Caller::local(),
            request_bytes,
            active_executions: 0,
            duration_ms: 0,
        }
    }
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
        let capabilities = registry
            .iter()
            .map(|tool| {
                RegisteredCapability::new(tool.definition.name, tool.definition.capability.clone())
            })
            .collect::<Vec<_>>();
        let policy = ToolPolicy::from_config(&config, &capabilities)?;
        Ok(Self {
            config,
            policy,
            registry,
        })
    }

    pub fn list_tools(&self, caller: &Caller) -> Value {
        let tools = self
            .registry
            .iter()
            .filter(|tool| self.policy.is_discoverable(caller, tool.definition.name))
            .map(|tool| tool.definition.json_schema())
            .collect::<Vec<_>>();
        json!({ "tools": tools })
    }

    pub fn call(&self, context: RequestContext, call: ToolCall) -> Result<ToolResult, RufloError> {
        let tool = self
            .registry
            .iter()
            .find(|tool| tool.definition.name == call.name)
            .ok_or_else(|| {
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
                name: "memory_search",
                description: "Find stored memories by meaning.",
                capability: Capability::supported("memory.search", 1),
            },
            handler: memory_search,
        },
    ]
}

fn agent_spawn(arguments: &Value) -> Result<ToolResult, RufloError> {
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

fn memory_search(arguments: &Value) -> Result<ToolResult, RufloError> {
    let query = required_string(arguments, "query")?;
    Ok(ToolResult::text(
        format!("no stored matches for `{query}`"),
        Some(json!({
            "query": query,
            "matches": []
        })),
    ))
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
