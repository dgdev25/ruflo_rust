use std::collections::{HashMap, VecDeque};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::body::{to_bytes, Body};
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Response, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use ruflo_types::RufloError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Sha256;
use tokio::sync::Semaphore;
use tokio::time::timeout;

use crate::dispatcher::{map_error, Dispatcher, RequestContext, RequestIdentity, ToolCall};

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpLimits {
    pub max_request_bytes: usize,
    pub max_concurrent_requests: usize,
    pub request_timeout_ms: u64,
    pub max_requests_per_window: usize,
    pub rate_window_ms: u64,
}

impl Default for HttpLimits {
    fn default() -> Self {
        Self {
            max_request_bytes: 64 * 1024,
            max_concurrent_requests: 4,
            request_timeout_ms: 30_000,
            max_requests_per_window: 60,
            rate_window_ms: 60_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityValidatorConfig {
    pub issuer: String,
    pub audience: String,
    pub hmac_secret: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityClaims {
    pub sub: String,
    pub iss: String,
    pub aud: String,
    pub exp: u64,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct IdentityValidator {
    config: IdentityValidatorConfig,
}

impl IdentityValidator {
    pub fn new(config: IdentityValidatorConfig) -> Self {
        Self { config }
    }

    pub fn validate_headers(&self, headers: &HeaderMap) -> Result<RequestIdentity, RufloError> {
        let token = bearer_token(headers).ok_or(RufloError::Unauthenticated)?;
        self.validate_token(token)
    }

    pub fn validate_token(&self, token: &str) -> Result<RequestIdentity, RufloError> {
        let (payload_segment, signature_segment) =
            token.split_once('.').ok_or(RufloError::Unauthenticated)?;
        let expected_signature =
            sign_segment(payload_segment.as_bytes(), &self.config.hmac_secret)?;
        // Constant-time comparison to avoid leaking signature information via
        // early-exit timing differences. XOR all bytes (and length mismatch)
        // into an accumulator and check the accumulator is zero.
        let sig_bytes = signature_segment.as_bytes();
        let exp_bytes = expected_signature.as_bytes();
        let mut diff: u8 = 0;
        diff |= (sig_bytes.len() ^ exp_bytes.len()) as u8;
        let max = sig_bytes.len().max(exp_bytes.len());
        for i in 0..max {
            let s = sig_bytes.get(i).copied().unwrap_or(0);
            let e = exp_bytes.get(i).copied().unwrap_or(0);
            diff |= s ^ e;
        }
        if diff != 0 {
            return Err(RufloError::Unauthenticated);
        }

        let payload = URL_SAFE_NO_PAD
            .decode(payload_segment)
            .map_err(|_| RufloError::Unauthenticated)?;
        let claims: IdentityClaims =
            serde_json::from_slice(&payload).map_err(|_| RufloError::Unauthenticated)?;

        if claims.iss != self.config.issuer || claims.aud != self.config.audience {
            return Err(RufloError::Unauthenticated);
        }

        let now = unix_timestamp_s()?;
        if claims.exp < now {
            return Err(RufloError::Unauthenticated);
        }

        Ok(RequestIdentity {
            subject: claims.sub,
            issuer: claims.iss,
            audience: claims.aud,
            expires_at_epoch_s: claims.exp,
            capabilities: claims.capabilities.into_iter().collect(),
        })
    }

    pub fn sign_for_tests(&self, claims: &IdentityClaims) -> String {
        let payload = serde_json::to_vec(claims).expect("claims serialize");
        let payload_segment = URL_SAFE_NO_PAD.encode(payload);
        let signature =
            sign_segment(payload_segment.as_bytes(), &self.config.hmac_secret).expect("hmac");
        format!("{payload_segment}.{signature}")
    }
}

pub fn serve_stateless_http(
    dispatcher: Dispatcher,
    authn: IdentityValidator,
    limits: HttpLimits,
) -> Router {
    let shared_limits = limits.clone();
    Router::new()
        .route("/mcp", post(handle_mcp))
        .with_state(HttpState {
            dispatcher,
            authn,
            limits,
            discovery_cache: Arc::new(Mutex::new(HashMap::new())),
            rate_limiter: Arc::new(Mutex::new(HashMap::new())),
            semaphore: Arc::new(Semaphore::new(shared_limits.max_concurrent_requests)),
            active_requests: Arc::new(AtomicUsize::new(0)),
        })
}

#[derive(Clone)]
struct HttpState {
    dispatcher: Dispatcher,
    authn: IdentityValidator,
    limits: HttpLimits,
    discovery_cache: Arc<Mutex<HashMap<String, Value>>>,
    rate_limiter: Arc<Mutex<HashMap<String, VecDeque<Instant>>>>,
    semaphore: Arc<Semaphore>,
    active_requests: Arc<AtomicUsize>,
}

async fn handle_mcp(State(state): State<HttpState>, request: Request) -> Response<Body> {
    match handle_mcp_inner(state, request).await {
        Ok(response) => response,
        Err((status, id, error, method, tool, cacheable)) => jsonrpc_http_response(
            status,
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": error.into_json()
            }),
            &method,
            tool,
            cacheable,
        ),
    }
}

async fn handle_mcp_inner(
    state: HttpState,
    request: Request,
) -> Result<
    Response<Body>,
    (
        StatusCode,
        Value,
        crate::dispatcher::ErrorObject,
        String,
        Option<String>,
        bool,
    ),
> {
    if request.headers().contains_key("mcp-session-id") {
        return Err((
            StatusCode::BAD_REQUEST,
            Value::Null,
            map_error(RufloError::invalid_input(
                "session.unsupported",
                "stateless HTTP MCP does not accept Mcp-Session-Id",
            )),
            "transport/session".to_string(),
            None,
            false,
        ));
    }

    let identity = state
        .authn
        .validate_headers(request.headers())
        .map_err(|error| {
            (
                StatusCode::UNAUTHORIZED,
                Value::Null,
                map_error(error),
                "transport/auth".to_string(),
                None,
                false,
            )
        })?;

    let _permit = state.semaphore.clone().try_acquire_owned().map_err(|_| {
        (
            StatusCode::TOO_MANY_REQUESTS,
            Value::Null,
            map_error(RufloError::RateLimited { retry_after_ms: 0 }),
            "transport/concurrency".to_string(),
            None,
            false,
        )
    })?;

    check_rate_limit(&state, &identity.subject).map_err(|error| {
        (
            StatusCode::TOO_MANY_REQUESTS,
            Value::Null,
            map_error(error),
            "transport/rate".to_string(),
            None,
            false,
        )
    })?;

    let (_parts, body) = request.into_parts();
    let bytes = to_bytes(body, state.limits.max_request_bytes)
        .await
        .map_err(|_| {
            (
                StatusCode::PAYLOAD_TOO_LARGE,
                Value::Null,
                map_error(RufloError::invalid_input(
                    "request_too_large",
                    format!(
                        "request size exceeds limit {}",
                        state.limits.max_request_bytes
                    ),
                )),
                "transport/body".to_string(),
                None,
                false,
            )
        })?;

    let request_value: Value = serde_json::from_slice(&bytes).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Value::Null,
            map_error(RufloError::invalid_input(
                "request.invalid_json",
                error.to_string(),
            )),
            "transport/parse".to_string(),
            None,
            false,
        )
    })?;

    let id = request_value.get("id").cloned().unwrap_or(Value::Null);
    let method = request_value
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("transport/invalid")
        .to_string();
    let tool = request_value
        .get("params")
        .and_then(|value| value.get("name"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);

    let cache_key = discovery_cache_key(&identity);
    if method == "tools/list" {
        if let Some(cached) = state
            .discovery_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&cache_key)
            .cloned()
        {
            return Ok(jsonrpc_http_response(
                StatusCode::OK,
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": cached
                }),
                &method,
                None,
                true,
            ));
        }
    }

    let started = Instant::now();
    let active_executions = state.active_requests.fetch_add(1, Ordering::SeqCst);
    let _active_guard = ActiveGuard::new(state.active_requests.clone());
    let context = RequestContext::remote(identity.clone(), bytes.len(), active_executions, 0);

    let response = match method.as_str() {
        "tools/list" => {
            let result = state.dispatcher.list_tools(&context);
            state
                .discovery_cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(cache_key, result.clone());
            Ok(jsonrpc_http_response(
                StatusCode::OK,
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": result
                }),
                &method,
                None,
                true,
            ))
        }
        "tools/call" => {
            let params = request_value
                .get("params")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let name = params.get("name").and_then(Value::as_str).ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    id.clone(),
                    map_error(RufloError::invalid_input(
                        "tool.invalid_name",
                        "missing tools/call name",
                    )),
                    method.clone(),
                    tool.clone(),
                    false,
                )
            })?;
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            if !arguments.is_object() {
                return Err((
                    StatusCode::BAD_REQUEST,
                    id.clone(),
                    map_error(RufloError::invalid_input(
                        "tool.invalid_arguments",
                        "tools/call arguments must be an object",
                    )),
                    method.clone(),
                    Some(name.to_string()),
                    false,
                ));
            }

            let dispatcher = state.dispatcher.clone();
            let tool_name = name.to_string();
            let tool_args = arguments.clone();
            let timeout_ms = state.limits.request_timeout_ms;
            let request_bytes = bytes.len();
            let identity = identity.clone();
            let elapsed_before_dispatch = started.elapsed().as_millis() as u64;

            let task = tokio::task::spawn_blocking(move || {
                dispatcher.call(
                    RequestContext::remote(
                        identity,
                        request_bytes,
                        active_executions,
                        elapsed_before_dispatch,
                    ),
                    ToolCall {
                        name: tool_name,
                        arguments: tool_args,
                    },
                )
            });

            let result = timeout(Duration::from_millis(timeout_ms), task)
                .await
                .map_err(|_| {
                    (
                        StatusCode::REQUEST_TIMEOUT,
                        id.clone(),
                        map_error(RufloError::Timeout),
                        method.clone(),
                        Some(name.to_string()),
                        false,
                    )
                })?
                .map_err(|error| {
                    (
                        StatusCode::BAD_GATEWAY,
                        id.clone(),
                        map_error(RufloError::UpstreamAdapter {
                            message: error.to_string(),
                        }),
                        method.clone(),
                        Some(name.to_string()),
                        false,
                    )
                })?;

            match result {
                Ok(result) => Ok(jsonrpc_http_response(
                    StatusCode::OK,
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": result.into_json()
                    }),
                    &method,
                    Some(name.to_string()),
                    false,
                )),
                Err(error) => Err((
                    status_for_error(&error),
                    id.clone(),
                    map_error(error),
                    method.clone(),
                    Some(name.to_string()),
                    false,
                )),
            }
        }
        _ => Err((
            StatusCode::NOT_FOUND,
            id.clone(),
            crate::dispatcher::ErrorObject {
                code: -32601,
                message: "Method not found".to_string(),
                data: crate::dispatcher::ErrorResponseData {
                    correlation_id: "corr-method-not-found".to_string(),
                    details: json!({ "method": method }),
                },
            },
            method,
            tool,
            false,
        )),
    };

    // _active_guard drops here, decrementing active_requests on every path
    // (including early `?` returns inside the match arms above).
    response
}

/// RAII guard that decrements `active_requests` on drop. Ensures the counter
/// is always released on every return path (happy path, `?` early returns,
/// and panics) so the active-request count never leaks.
struct ActiveGuard {
    counter: Arc<AtomicUsize>,
}

impl ActiveGuard {
    fn new(counter: Arc<AtomicUsize>) -> Self {
        Self { counter }
    }
}

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::SeqCst);
    }
}

fn check_rate_limit(state: &HttpState, subject: &str) -> Result<(), RufloError> {
    let mut limiter = state
        .rate_limiter
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let now = Instant::now();
    let window = Duration::from_millis(state.limits.rate_window_ms);
    let queue = limiter.entry(subject.to_string()).or_default();
    while queue
        .front()
        .map(|instant| now.duration_since(*instant) >= window)
        .unwrap_or(false)
    {
        queue.pop_front();
    }
    if queue.len() >= state.limits.max_requests_per_window {
        let retry_after_ms = queue
            .front()
            .map(|instant| {
                window
                    .saturating_sub(now.duration_since(*instant))
                    .as_millis() as u64
            })
            .unwrap_or(0);
        return Err(RufloError::RateLimited { retry_after_ms });
    }
    queue.push_back(now);
    Ok(())
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let header = headers.get(header::AUTHORIZATION)?;
    let value = header.to_str().ok()?;
    value.strip_prefix("Bearer ")
}

fn sign_segment(segment: &[u8], secret: &str) -> Result<String, RufloError> {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).map_err(|error| {
        RufloError::UpstreamAdapter {
            message: error.to_string(),
        }
    })?;
    mac.update(segment);
    Ok(URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
}

fn unix_timestamp_s() -> Result<u64, RufloError> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| RufloError::UpstreamAdapter {
            message: error.to_string(),
        })?
        .as_secs())
}

fn status_for_error(error: &RufloError) -> StatusCode {
    match error {
        RufloError::Unauthenticated => StatusCode::UNAUTHORIZED,
        RufloError::Unauthorized { .. } => StatusCode::FORBIDDEN,
        RufloError::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
        RufloError::Timeout => StatusCode::REQUEST_TIMEOUT,
        RufloError::InvalidInput { .. } => StatusCode::BAD_REQUEST,
        _ => StatusCode::BAD_REQUEST,
    }
}

fn discovery_cache_key(identity: &RequestIdentity) -> String {
    let capabilities = identity
        .capabilities
        .iter()
        .cloned()
        .collect::<Vec<_>>()
        .join(",");
    format!("{}|{}|{}", identity.subject, identity.issuer, capabilities)
}

fn jsonrpc_http_response(
    status: StatusCode,
    body: Value,
    method: &str,
    tool: Option<String>,
    cacheable: bool,
) -> Response<Body> {
    let mut response = Json(body).into_response();
    *response.status_mut() = status;
    response.headers_mut().insert(
        HeaderName::from_static("x-ruflo-mcp-method"),
        HeaderValue::from_str(method).unwrap_or_else(|_| HeaderValue::from_static("invalid")),
    );
    if let Some(tool) = tool {
        if let Ok(value) = HeaderValue::from_str(&tool) {
            response
                .headers_mut()
                .insert(HeaderName::from_static("x-ruflo-mcp-tool"), value);
        }
    }
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(if cacheable {
            "private, max-age=60"
        } else {
            "no-store"
        }),
    );
    response
}

use axum::http::HeaderName;
