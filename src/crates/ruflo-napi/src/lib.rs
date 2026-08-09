//! Thin N-API adapter. All business behaviour belongs to `ruflo-core`.

use napi::{Error, Result, Status};
use napi_derive::napi;
use ruflo_core::{self as core, EmbedRequest, RouteRequest};

fn core_error(error: core::CoreError) -> Error {
    Error::new(Status::InvalidArg, error.to_string())
}

#[napi(object)]
pub struct EmbeddingResult {
    pub dimensions: u32,
    pub vector: Vec<f64>,
    pub provider: String,
}

#[napi(object)]
pub struct RouteResult {
    pub agent: String,
    pub score: u32,
    pub strategy: String,
}

#[napi]
pub fn embed(text: String, dimensions: Option<u32>) -> Result<EmbeddingResult> {
    let answer = core::embed(EmbedRequest {
        text,
        dimensions: dimensions.map(|value| value as usize),
    })
    .map_err(core_error)?;
    Ok(EmbeddingResult {
        dimensions: answer.dimensions as u32,
        vector: answer.vector,
        provider: answer.provider.to_owned(),
    })
}

#[napi]
pub fn cosine_similarity(left: Vec<f64>, right: Vec<f64>) -> Result<f64> {
    core::cosine_similarity(&left, &right).map_err(core_error)
}

#[napi]
pub fn route(task: String, candidates: Vec<String>) -> Result<RouteResult> {
    let answer = core::route(RouteRequest { task, candidates }).map_err(core_error)?;
    Ok(RouteResult {
        agent: answer.agent,
        score: answer.score,
        strategy: answer.strategy.to_owned(),
    })
}
