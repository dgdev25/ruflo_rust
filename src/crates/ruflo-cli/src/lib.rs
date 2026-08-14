//! Shared native CLI entrypoint for thin Ruflo-compatible binaries.

// Suppress warnings during the native port — many services have dead code
// (state-only modules), unused imports (feature-gated paths), and test-only
// variables. Individual cleanup tracked in audit #3.
#![allow(dead_code, unused_imports, unused_variables, unused_mut)]
#![allow(unreachable_patterns, private_interfaces)]
// Comprehensive clippy allow — the port has many style lints from rapid
// development. Individual cleanup tracked for future.
#![allow(clippy::all, clippy::pedantic)]

pub(crate) mod analyze;
pub(crate) mod announcements;
pub(crate) mod ast;
pub(crate) mod agentic_bridge;
pub(crate) mod appliance;
pub(crate) mod appliance_advanced;
pub(crate) mod auth;
pub(crate) mod autopilot;
pub(crate) mod benchmark;
pub(crate) mod benchmarks;
pub(crate) mod claims;
pub(crate) mod cleanup;
pub(crate) mod command;
pub(crate) mod completions;
pub(crate) mod compressor;
pub(crate) mod config_file;
pub(crate) mod daemon;
pub(crate) mod deployment;
pub(crate) mod distillation;
pub(crate) mod eject;
pub(crate) mod embeddings;
pub(crate) mod funnel;
pub(crate) mod funnel_command;
pub(crate) mod funnel_extra;
pub(crate) mod flywheel_ledger;
pub(crate) mod gaia_bench;
pub(crate) mod graph_algo;
pub(crate) mod guidance;
pub(crate) mod harness_exec;
pub(crate) mod hooks;
pub(crate) mod hive_mind;
pub(crate) mod init_wizard;
pub(crate) mod issues;
pub(crate) mod lifecycle;
pub(crate) mod metaharness;
pub(crate) mod neural;
#[cfg(feature = "onnx")]
pub(crate) mod onnx_embeddings;
#[cfg(not(feature = "onnx"))]
pub(crate) mod onnx_embed_stub;
#[cfg(not(feature = "onnx"))]
pub(crate) use onnx_embed_stub as onnx_embeddings;
pub(crate) mod output;
pub(crate) mod performance;
pub(crate) mod plugins;
pub(crate) mod policy;
pub(crate) mod prompt;
pub(crate) mod process_cmd;
pub(crate) mod prod_modules;
pub(crate) mod repo_supervisor;
pub(crate) mod providers;
pub(crate) mod proxy;
pub(crate) mod registry_api;
pub(crate) mod route;
pub(crate) mod security;
pub(crate) mod services;
pub(crate) mod settings;
pub(crate) mod small_modules;
pub(crate) mod sona;
pub(crate) mod spend;
mod dispatch;
pub(crate) mod swarm_exec;
pub(crate) mod spinner;
pub(crate) mod transfer_store;
pub(crate) mod update_cmd;
pub(crate) mod verify;
pub(crate) mod version;
pub(crate) mod workflow;

use std::ffi::OsString;
use std::process::ExitCode;

pub use command::ParsedCommand;


pub fn run(argv: impl IntoIterator<Item = std::ffi::OsString>) -> std::process::ExitCode {
    dispatch::run(argv)
}
