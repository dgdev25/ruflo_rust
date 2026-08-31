use std::ffi::OsString;

mod types;
mod parse;
#[cfg(test)]
mod tests;

pub use types::{ParsedCommand, UNSUPPORTED_COMMAND_ERROR_CODE};
pub use parse::parse;
