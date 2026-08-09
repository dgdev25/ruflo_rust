#![allow(dead_code, unused_imports, unused_variables, unused_mut)]
#![allow(clippy::derivable_impls, clippy::useless_format)]
#![allow(unused_must_use)]

mod dispatcher;
mod tools_extra;
pub mod tools_catalog;
#[cfg(feature = "stateless-http")]
mod http;
mod stdio;

pub use dispatcher::{
    map_error, Dispatcher, ErrorObject, ErrorResponseData, RequestContext, RequestIdentity,
    ToolCall, ToolDefinition, ToolResponseContent, ToolResult,
};
#[cfg(feature = "stateless-http")]
pub use http::{
    serve_stateless_http, HttpLimits, IdentityClaims, IdentityValidator, IdentityValidatorConfig,
};
pub use stdio::{serve_stdio, serve_stdio_with};
