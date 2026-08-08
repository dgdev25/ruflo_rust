//! Native V3 `proxy` command — Meta LLM Proxy control surface (ADR-304/307/313/314/315).
//!
//! Source: `v3/@claude-flow/cli/src/commands/proxy.ts`. Implements the
//! consent-gated subcommands (sponsor/power-saver/training-share enable/disable/
//! status/clear) using the native funnel layer. Process lifecycle (install/start/
//! stop) is deferred (no signed-installer subprocess in native build).

use std::path::{Path, PathBuf};

use crate::funnel;

fn proxy_config_file() -> PathBuf {
    funnel::funnel_state_dir_pub().join("proxy-config.toml")
}

fn read_config_raw() -> String {
    std::fs::read_to_string(proxy_config_file()).unwrap_or_default()
}

fn write_config_line(field: &str, raw_value: &str) {
    let dir = funnel::funnel_state_dir_pub();
    let _ = std::fs::create_dir_all(&dir);
    let target = proxy_config_file();
    let raw = read_config_raw();
    let line = format!("{field} = {raw_value}");
    let prefix = format!("{field} =");
    // Simple line-based replacement (no regex dependency).
    let mut found = false;
    let mut next_lines: Vec<String> = Vec::new();
    for existing in raw.lines() {
        if existing.trim_start().starts_with(&prefix) {
            next_lines.push(line.clone());
            found = true;
        } else {
            next_lines.push(existing.to_string());
        }
    }
    if !found {
        next_lines.push(line);
    }
    let next = next_lines.join("\n") + "\n";
    let _ = std::fs::write(&target, next);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyCommand {
    pub operation: String,
    pub yes: bool,
}

pub fn run(_root: &Path, command: ProxyCommand) -> u8 {
    match command.operation.as_str() {
        "" | "status" => {
            println!("Meta Proxy");
            println!("  Installation: not installed (process lifecycle deferred in native build)");
            println!("  Process: not running");
            println!();
            println!(
                "Sponsored downtime: {}",
                if funnel::has_consent("sponsored-downtime") {
                    "granted"
                } else {
                    "not granted"
                }
            );
            println!(
                "Power saver:       {}",
                if funnel::has_consent("power-saver") {
                    "granted"
                } else {
                    "not granted"
                }
            );
            println!(
                "Training share:    {}",
                if funnel::has_consent("training-data-sharing") {
                    "granted"
                } else {
                    "not granted"
                }
            );
            0
        }
        "sponsor-enable" => consent_enable(
            "sponsored-downtime",
            "proxy-sponsor-enable",
            "sponsored_consent_granted",
            "Sponsored downtime",
            command.yes,
        ),
        "sponsor-disable" => consent_disable(
            "sponsored-downtime",
            "proxy-sponsor-disable",
            "sponsored_consent_granted",
            "Sponsored downtime",
        ),
        "sponsor-status" => consent_status("sponsored-downtime", "Sponsored downtime"),
        "sponsor-clear" => consent_clear(
            "sponsored-downtime",
            "proxy-sponsor-clear",
            "sponsored_consent_granted",
        ),
        "power-saver-enable" => consent_enable(
            "power-saver",
            "proxy-power-saver-enable",
            "power_saver_enabled",
            "Power saver",
            command.yes,
        ),
        "power-saver-disable" => consent_disable(
            "power-saver",
            "proxy-power-saver-disable",
            "power_saver_enabled",
            "Power saver",
        ),
        "power-saver-status" => consent_status("power-saver", "Power saver"),
        "power-saver-clear" => consent_clear(
            "power-saver",
            "proxy-power-saver-clear",
            "power_saver_enabled",
        ),
        "training-share-enable" => consent_enable(
            "training-data-sharing",
            "proxy-training-share-enable",
            "training_consent_granted",
            "Training-data sharing",
            command.yes,
        ),
        "training-share-disable" => consent_disable(
            "training-data-sharing",
            "proxy-training-share-disable",
            "training_consent_granted",
            "Training-data sharing",
        ),
        "training-share-status" => consent_status("training-data-sharing", "Training-data sharing"),
        "config" => {
            eprintln!("[ERROR] proxy config (--cloud/--local-only) requires the proxy binary (not installed in native build)");
            1
        }
        "install" | "update" | "start" | "stop" | "uninstall" | "logs" | "supervise" => {
            eprintln!("[ERROR] proxy {op} is a process lifecycle command that requires the signed proxy binary.", op = command.operation);
            eprintln!("  Install the proxy separately (see ADR-307). Native build does not include the installer.");
            1
        }
        _ => {
            eprintln!("[ERROR] Unknown proxy operation: {}", command.operation);
            1
        }
    }
}

fn consent_enable(domain: &str, surface: &str, config_field: &str, label: &str, yes: bool) -> u8 {
    if funnel::has_consent(domain) {
        println!("{label} is already enabled.");
        return 0;
    }
    println!("{label} requires explicit opt-in. This grants the '{domain}' consent domain.");
    println!(
        "Disable anytime: ruflo proxy {}-disable",
        domain.replace('-', "-")
    );
    if !yes {
        println!("\nRe-run with --yes to confirm.");
        return 0;
    }
    funnel::record_consent(domain, true, surface);
    write_config_line(config_field, "true");
    funnel::record_funnel_event("sponsor_mode_enabled", "statusline");
    println!("\u{2714} {label} enabled.");
    0
}

fn consent_disable(domain: &str, surface: &str, config_field: &str, label: &str) -> u8 {
    funnel::revoke_consent(domain, surface);
    write_config_line(config_field, "false");
    funnel::record_funnel_event("sponsor_mode_disabled", "statusline");
    println!("\u{2714} {label} disabled.");
    0
}

fn consent_status(domain: &str, label: &str) -> u8 {
    let granted = funnel::has_consent(domain);
    println!(
        "{label} consent: {}",
        if granted { "granted" } else { "not granted" }
    );
    0
}

fn consent_clear(domain: &str, surface: &str, config_field: &str) -> u8 {
    funnel::revoke_consent(domain, surface);
    write_config_line(config_field, "false");
    println!("Cleared {domain} consent and config mirror.");
    0
}
