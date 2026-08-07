use std::process::ExitCode;

fn main() -> ExitCode {
    ruflo_codex_cli::run(std::env::args_os())
}
