use std::io::{self, BufRead, BufReader, Read, Write};

use serde_json::{json, Value};

use crate::dispatcher::{map_error, Dispatcher, RequestContext, ToolCall};
use ruflo_types::RufloError;

pub fn serve_stdio(dispatcher: Dispatcher) -> Result<(), RufloError> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let stderr = io::stderr();
    serve_stdio_with(dispatcher, stdin.lock(), stdout.lock(), stderr.lock())
}

pub fn serve_stdio_with<R, W, E>(
    dispatcher: Dispatcher,
    input: R,
    mut output: W,
    mut diagnostics: E,
) -> Result<(), RufloError>
where
    R: Read,
    W: Write,
    E: Write,
{
    let mut reader = BufReader::new(input);
    let mut line = String::new();

    loop {
        line.clear();
        if reader.read_line(&mut line).map_err(io_error)? == 0 {
            break;
        }

        if line.trim().is_empty() {
            continue;
        }

        let frame = line.trim_end_matches(&['\r', '\n'][..]).to_string();
        let response = match serde_json::from_str::<Value>(&frame) {
            Ok(request) => handle_request(&dispatcher, &request, frame.len()),
            Err(error) => {
                let _ = writeln!(diagnostics, "mcp parse error: {error}");
                json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": {
                        "code": -32700,
                        "message": "Parse error",
                        "data": {
                            "correlationId": "corr-parse",
                            "details": {
                                "message": error.to_string()
                            }
                        }
                    }
                })
            }
        };

        serde_json::to_writer(&mut output, &response).map_err(io_error)?;
        output.write_all(b"\n").map_err(io_error)?;
        output.flush().map_err(io_error)?;
    }

    Ok(())
}

fn handle_request(dispatcher: &Dispatcher, request: &Value, request_bytes: usize) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    match request_to_response(dispatcher, request, request_bytes) {
        Ok(result) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        }),
        Err(error) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": error.into_json()
        }),
    }
}

fn request_to_response(
    dispatcher: &Dispatcher,
    request: &Value,
    request_bytes: usize,
) -> Result<Value, crate::dispatcher::ErrorObject> {
    if request.get("jsonrpc") != Some(&Value::String("2.0".to_string())) {
        return Err(invalid_request("jsonrpc must equal `2.0`"));
    }

    let method = request
        .get("method")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_request("method must be a string"))?;
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
    if !params.is_object() {
        return Err(invalid_request("params must be an object"));
    }

    match method {
        "tools/list" => {
            Ok(dispatcher
                .list_tools(&crate::dispatcher::RequestContext::local(request_bytes).caller))
        }
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).ok_or_else(|| {
                map_error(RufloError::invalid_input(
                    "tool.invalid_name",
                    "missing tools/call name",
                ))
            })?;
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            if !arguments.is_object() {
                return Err(map_error(RufloError::invalid_input(
                    "tool.invalid_arguments",
                    "tools/call arguments must be an object",
                )));
            }

            dispatcher
                .call(
                    RequestContext::local(request_bytes),
                    ToolCall {
                        name: name.to_string(),
                        arguments,
                    },
                )
                .map(|result| result.into_json())
                .map_err(map_error)
        }
        _ => Err(method_not_found(method)),
    }
}

fn invalid_request(message: &str) -> crate::dispatcher::ErrorObject {
    crate::dispatcher::ErrorObject {
        code: -32600,
        message: "Invalid Request".to_string(),
        data: crate::dispatcher::ErrorResponseData {
            correlation_id: "corr-invalid-request".to_string(),
            details: json!({ "message": message }),
        },
    }
}

fn method_not_found(method: &str) -> crate::dispatcher::ErrorObject {
    crate::dispatcher::ErrorObject {
        code: -32601,
        message: "Method not found".to_string(),
        data: crate::dispatcher::ErrorResponseData {
            correlation_id: "corr-method-not-found".to_string(),
            details: json!({ "method": method }),
        },
    }
}

fn io_error(error: impl ToString) -> RufloError {
    RufloError::UpstreamAdapter {
        message: error.to_string(),
    }
}
