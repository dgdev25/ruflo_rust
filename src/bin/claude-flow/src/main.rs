use std::process::ExitCode;

fn main() -> ExitCode {
    ruflo_cli::run(std::env::args_os())
}
