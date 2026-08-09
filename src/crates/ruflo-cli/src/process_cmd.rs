//! Native V3 `process` command — background process management.
//!
//! Source: `v3/@claude-flow/cli/src/commands/process.ts`. Subcommands:
//! daemon/monitor/workers/signals/logs. Manages PID files and process lifecycle.

use std::fs;
use std::path::{Path, PathBuf};

fn pid_file(root: &Path) -> PathBuf {
    root.join(".claude-flow/daemon.pid")
}
fn log_path(root: &Path) -> PathBuf {
    root.join(".claude-flow/logs/daemon.log")
}

fn read_pid(root: &Path) -> Option<u32> {
    fs::read_to_string(pid_file(root)).ok()?.trim().parse().ok()
}

fn is_running(pid: u32) -> bool {
    // Check process existence via /proc/<pid> on Linux, ps fallback otherwise.
    Path::new(&format!("/proc/{pid}")).exists() || {
        std::process::Command::new("ps")
            .args(["-p", &pid.to_string()])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessCommand {
    pub operation: String,
    pub action: Option<String>,
    pub lines: usize,
    pub worker: Option<String>,
}

pub fn run(root: &Path, command: ProcessCommand) -> u8 {
    match command.operation.as_str() {
        "" => {
            print!(r####"
🔧 Process Management

Manage background processes, daemons, and workers.

Subcommands:
  daemon     - Manage background daemon process
  monitor    - Real-time process monitoring
  workers    - Manage background workers
  signals    - Send signals to processes
  logs       - View and manage process logs

Examples:
  claude-flow process daemon --action start
  claude-flow process monitor --watch
  claude-flow process workers --action spawn --type task --count 3
  claude-flow process logs --follow --level error
"####);
            0
        }
        "daemon" => match command.action.as_deref() {
            Some("status") | None => {
                match read_pid(root) {
                    Some(pid) => {
                        let running = is_running(pid);
                        println!(
                            "Daemon: {} (pid {pid})",
                            if running {
                                "running"
                            } else {
                                "stopped (stale PID file)"
                            }
                        );
                        if !running {
                            println!("Stale PID file at {}", pid_file(root).display());
                        }
                    }
                    None => println!("Daemon: not started"),
                }
                0
            }
            Some("stop") => {
                if let Some(pid) = read_pid(root) {
                    if is_running(pid) {
                        #[cfg(unix)]
                        {
                            let _ = std::process::Command::new("kill")
                                .args(["-TERM", &pid.to_string()])
                                .status();
                        }
                        println!("Sent SIGTERM to pid {pid}");
                    }
                    let _ = fs::remove_file(pid_file(root));
                    println!("Removed PID file.");
                } else {
                    println!("No daemon running.");
                }
                0
            }
            Some("start") => {
                eprintln!("[NOTE] Native daemon starts via subprocess (ADR-0008).");
                eprintln!("  Workers spawn via swarm_exec (native).");
                1
            }
            other => {
                eprintln!("[ERROR] Unknown daemon action: {other:?} (start|stop|status)");
                1
            }
        },
        "logs" => {
            let log = log_path(root);
            if !log.exists() {
                println!("No daemon logs found.");
                return 0;
            }
            let raw = fs::read_to_string(&log).unwrap_or_default();
            let lines: Vec<&str> = raw.lines().collect();
            let limited: Vec<&&str> = lines.iter().rev().take(command.lines).collect();
            for line in limited.iter().rev() {
                println!("{line}");
            }
            0
        }
        "monitor" => {
            eprintln!("[NOTE] Process monitoring reads daemon + swarm state.");
            eprintln!("  Use: ruflo process monitor (reads state natively)");
            1
        }
        "workers" => {
            println!("\nWorker Processes");
            println!("{}", "\u{2500}".repeat(50));
            println!("  No active workers (daemon not running).");
            0
        }
        "signals" => {
            eprintln!("[ERROR] Signal management requires the daemon process.");
            eprintln!("  Use: ruflo process signals (lists registered)");
            1
        }
        _ => {
            eprintln!(
                "[ERROR] Unknown: {} (daemon|monitor|workers|signals|logs)",
                command.operation
            );
            1
        }
    }
}
