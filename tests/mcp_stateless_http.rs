#![cfg(feature = "stateless-http")]

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use ruflo_config::{EffectiveConfig, Limits, PolicyConfig};
use ruflo_mcp::{
    serve_stateless_http, Dispatcher, HttpLimits, IdentityClaims, IdentityValidator,
    IdentityValidatorConfig,
};
use serde_json::{json, Value};
use tower::ServiceExt;

fn config() -> EffectiveConfig {
    EffectiveConfig {
        policy: PolicyConfig {
            allow: vec![],
            deny: vec![],
        },
        limits: Limits {
            max_request_bytes: 256,
            max_concurrent_executions: 8,
            max_duration_ms: 5_000,
        },
    }
}

fn validator() -> IdentityValidator {
    IdentityValidator::new(IdentityValidatorConfig {
        issuer: "ruflo-tests".to_string(),
        audience: "native-http".to_string(),
        hmac_secret: "top-secret".to_string(),
    })
}

fn valid_claims(capabilities: &[&str]) -> IdentityClaims {
    IdentityClaims {
        sub: "worker-1".to_string(),
        iss: "ruflo-tests".to_string(),
        aud: "native-http".to_string(),
        exp: now_epoch_s() + 60,
        capabilities: capabilities
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
    }
}

fn app() -> axum::Router {
    serve_stateless_http(
        Dispatcher::from_config(config()).unwrap(),
        validator(),
        HttpLimits {
            max_request_bytes: 256,
            max_concurrent_requests: 1,
            request_timeout_ms: 50,
            max_requests_per_window: 10,
            rate_window_ms: 60_000,
        },
    )
}

#[tokio::test]
async fn remote_call_requires_identity_and_issues_no_session_id() {
    let response = app()
        .oneshot(mcp_request(
            None,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {
                    "name": "agent_spawn",
                    "arguments": {"role": "coder"}
                }
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let token = validator().sign_for_tests(&valid_claims(&["agent.spawn"]));
    let response = app()
        .oneshot(mcp_request(
            Some(token),
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {
                    "name": "agent_spawn",
                    "arguments": {"role": "coder"}
                }
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(!response.headers().contains_key("mcp-session-id"));
    let body = response_json(response).await;
    assert_eq!(
        body["result"]["structuredContent"]["agentId"],
        "agent-coder"
    );
}

#[tokio::test]
async fn invalid_identity_claims_return_401() {
    let mut claims = valid_claims(&["agent.spawn"]);
    claims.exp = now_epoch_s() - 1;
    let expired = validator().sign_for_tests(&claims);
    let response = app()
        .oneshot(mcp_request(
            Some(expired),
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/list",
                "params": {}
            }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let wrong_validator = IdentityValidator::new(IdentityValidatorConfig {
        issuer: "wrong".to_string(),
        audience: "native-http".to_string(),
        hmac_secret: "top-secret".to_string(),
    });
    let wrong_issuer = wrong_validator.sign_for_tests(&IdentityClaims {
        iss: "wrong".to_string(),
        ..valid_claims(&["agent.spawn"])
    });
    let response = app()
        .oneshot(mcp_request(
            Some(wrong_issuer),
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/list",
                "params": {}
            }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn policy_denials_and_tool_schema_match_stdio() {
    let denied = Dispatcher::from_config(EffectiveConfig {
        policy: PolicyConfig {
            allow: vec![],
            deny: vec!["memory_search".to_string()],
        },
        ..config()
    })
    .unwrap();
    let validator = validator();
    let token = validator.sign_for_tests(&valid_claims(&["agent.spawn", "memory.search"]));
    let app = serve_stateless_http(
        denied,
        validator,
        HttpLimits {
            max_request_bytes: 256,
            max_concurrent_requests: 1,
            request_timeout_ms: 50,
            max_requests_per_window: 10,
            rate_window_ms: 60_000,
        },
    );

    let list = app
        .clone()
        .oneshot(mcp_request(
            Some(token.clone()),
            json!({
                "jsonrpc": "2.0",
                "id": "list",
                "method": "tools/list",
                "params": {}
            }),
        ))
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    let body = response_json(list).await;
    let tools = body["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(tools, vec!["agent_spawn"]);

    let denied_call = app
        .oneshot(mcp_request(
            Some(token),
            json!({
                "jsonrpc": "2.0",
                "id": "call",
                "method": "tools/call",
                "params": {
                    "name": "memory_search",
                    "arguments": {"query": "auth"}
                }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(denied_call.status(), StatusCode::FORBIDDEN);
    let body = response_json(denied_call).await;
    assert_eq!(body["error"]["code"], -32001);
    assert_eq!(
        body["error"]["data"]["details"]["capability"],
        "memory.search"
    );
}

#[tokio::test]
async fn request_size_rate_timeout_and_concurrency_guards_apply() {
    let identity_validator = validator();
    let token = identity_validator.sign_for_tests(&valid_claims(&["agent.spawn"]));
    let app = app();

    let oversized_body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "agent_spawn",
            "arguments": {"role": "x".repeat(400)}
        }
    });
    let response = app
        .clone()
        .oneshot(mcp_request(Some(token.clone()), oversized_body))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);

    let slow_request = mcp_request(
        Some(token.clone()),
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "agent_spawn",
                "arguments": {"role": "coder", "sleep_ms": 100}
            }
        }),
    );
    let app_for_concurrency = app.clone();
    let in_flight =
        tokio::spawn(async move { app_for_concurrency.oneshot(slow_request).await.unwrap() });
    tokio::time::sleep(Duration::from_millis(5)).await;
    let concurrent = app
        .clone()
        .oneshot(mcp_request(
            Some(token.clone()),
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {
                    "name": "agent_spawn",
                    "arguments": {"role": "reviewer"}
                }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(concurrent.status(), StatusCode::TOO_MANY_REQUESTS);
    let _ = in_flight.await.unwrap();

    let timeout = app
        .clone()
        .oneshot(mcp_request(
            Some(token.clone()),
            json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": {
                    "name": "agent_spawn",
                    "arguments": {"role": "coder", "sleep_ms": 100}
                }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(timeout.status(), StatusCode::REQUEST_TIMEOUT);

    let rate_limited_app = serve_stateless_http(
        Dispatcher::from_config(config()).unwrap(),
        validator(),
        HttpLimits {
            max_request_bytes: 256,
            max_concurrent_requests: 2,
            request_timeout_ms: 50,
            max_requests_per_window: 1,
            rate_window_ms: 60_000,
        },
    );

    let rate_limited = rate_limited_app
        .clone()
        .oneshot(mcp_request(
            Some(token.clone()),
            json!({
                "jsonrpc": "2.0",
                "id": 5,
                "method": "tools/call",
                "params": {
                    "name": "agent_spawn",
                    "arguments": {"role": "one"}
                }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(rate_limited.status(), StatusCode::OK);
    let rate_limited = rate_limited_app
        .oneshot(mcp_request(
            Some(token),
            json!({
                "jsonrpc": "2.0",
                "id": 6,
                "method": "tools/call",
                "params": {
                    "name": "agent_spawn",
                    "arguments": {"role": "two"}
                }
            }),
        ))
        .await
        .unwrap();
    assert_eq!(rate_limited.status(), StatusCode::TOO_MANY_REQUESTS);
}

fn mcp_request(token: Option<String>, body: Value) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder.body(Body::from(body.to_string())).unwrap()
}

async fn response_json(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn now_epoch_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
