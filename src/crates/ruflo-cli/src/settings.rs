//! Native V3 `settings` command (ADR-311) — user-facing preferences router.
//!
//! Source of truth: `v3/@claude-flow/cli/src/commands/settings.ts`. A friendly
//! wrapper over the funnel primitives (precedence, disclosure, funnel.json user
//! config, telemetry id, rate-limit/quota-low manual flags). "settings" is the
//! user-facing name; "funnel" is internal and never shown.

use std::io::IsTerminal;
use std::path::Path;

use crate::funnel;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsCommand {
    Overview,
    NoticesStatus,
    NoticesOff,
    NoticesOn,
    NoticesId,
    NoticesRateLimited { clear: bool },
    NoticesQuotaLow { clear: bool },
    Help { subcommand: Option<String> },
}

pub fn run(root: &Path, command: SettingsCommand) -> u8 {
    match command {
        SettingsCommand::Overview => overview(root),
        SettingsCommand::NoticesStatus => notices_status(),
        SettingsCommand::NoticesOff => notices_off(),
        SettingsCommand::NoticesOn => notices_on(),
        SettingsCommand::NoticesId => notices_id(),
        SettingsCommand::NoticesRateLimited { clear } => rate_limited(clear),
        SettingsCommand::NoticesQuotaLow { clear } => quota_low(clear),
        SettingsCommand::Help { subcommand } => {
            print!("{}", help(subcommand.as_deref()));
            0
        }
    }
}

fn overview(root: &Path) -> u8 {
    let decision = funnel::resolve_funnel_enabled(root);
    let disclosure = funnel::get_disclosure();
    let consents = funnel::read_consents_pub();
    println!("ruflo settings — user preferences");
    println!();
    println!("Notices (statusline tips + product updates)");
    println!("  ruflo settings notices status    Show current state");
    println!("  ruflo settings notices off       Turn off all notices");
    println!("  ruflo settings notices on        Re-enable");
    println!("  ruflo settings notices id        Show pseudonymous notices id");
    println!();
    println!(
        "  current: {} ({})",
        if decision.enabled {
            "enabled"
        } else {
            "disabled"
        },
        decision.decided_by
    );
    println!("  disclosure: {}", disclosure.state.as_str());
    let domains: Vec<String> = consents
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    if !domains.is_empty() {
        println!("  consents: {}", domains.join(", "));
    }
    println!("  state dir: {}", funnel::funnel_state_dir_pub().display());
    0
}

fn notices_status() -> u8 {
    // resolveFunnelEnabled needs a cwd; for the notices status the decision is
    // independent of project config only in the env/enterprise/user/disclosure
    // tiers — pass the current dir so project-config is honored.
    let decision = funnel::resolve_funnel_enabled(&std::env::current_dir().unwrap_or_default());
    let disclosure = funnel::get_disclosure();
    println!(
        "Notices: {} (decided by: {})",
        if decision.enabled {
            "enabled"
        } else {
            "disabled"
        },
        decision.decided_by
    );
    println!("Disclosure: {}", disclosure.state.as_str());
    println!(
        "Telemetry: {}",
        if funnel::has_consent("telemetry") {
            "consent granted"
        } else {
            "no consent"
        }
    );
    0
}

fn notices_off() -> u8 {
    funnel::set_user_config_enabled(false);
    funnel::record_disclosure_declined();
    funnel::delete_funnel_data();
    print_success("Notices disabled. Local notice data deleted.");
    0
}

fn notices_on() -> u8 {
    funnel::set_user_config_enabled(true);
    funnel::record_disclosure_reenabled();
    let decision = funnel::resolve_funnel_enabled(&std::env::current_dir().unwrap_or_default());
    if decision.enabled {
        print_success("Notices enabled.");
    } else {
        eprintln!(
            "[WARN] User preference recorded, but notices stay off (decided by: {})",
            decision.decided_by
        );
    }
    0
}

fn notices_id() -> u8 {
    match funnel::get_funnel_id_pub() {
        Some(id) => println!("{id}"),
        None => println!("(no id — telemetry consent not granted, or notices are off)"),
    }
    0
}

fn rate_limited(clear: bool) -> u8 {
    if clear {
        if !funnel::clear_rate_limit_status() {
            eprintln!("[ERROR] Rate-limit flag was just toggled — try again in a few minutes (ADR-314 anti-abuse cooldown).");
            return 1;
        }
        print_success("Rate-limit flag cleared.");
        return 0;
    }
    if !funnel::mark_rate_limited() {
        eprintln!("[ERROR] Rate-limit flag was just toggled — try again in a few minutes (ADR-314 anti-abuse cooldown).");
        return 1;
    }
    print_success("Rate-limit flag set.");
    println!();
    println!("This is a manual, self-reported flag — ruflo cannot detect Claude's");
    println!("usage-limit state automatically today (see ADR-312). While flagged,");
    println!("the notices row may suggest sponsored Cognitum capacity as a bridge");
    println!("until your own limit resets: ruflo proxy sponsor-enable");
    println!();
    println!("Clear it any time: ruflo settings notices rate-limited --clear");
    0
}

fn quota_low(clear: bool) -> u8 {
    if clear {
        if !funnel::clear_quota_low_status() {
            eprintln!("[ERROR] Quota-low flag was just toggled — try again in a few minutes (ADR-314 anti-abuse cooldown).");
            return 1;
        }
        print_success("Quota-low flag cleared.");
        return 0;
    }
    if !funnel::mark_quota_low() {
        eprintln!("[ERROR] Quota-low flag was just toggled — try again in a few minutes (ADR-314 anti-abuse cooldown).");
        return 1;
    }
    print_success("Quota-low flag set.");
    println!();
    println!("This is a manual, self-reported flag — ruflo cannot read your actual");
    println!("quota percentage today (see ADR-312/314). While flagged, and once you");
    println!("enable power saver mode, everyday requests route through Cognitum's");
    println!("own difficulty-based router (billed to your own Cognitum account):");
    println!("  ruflo proxy power-saver-enable");
    println!();
    println!("Clear it any time: ruflo settings notices quota-low --clear");
    0
}

fn print_success(message: &str) {
    if std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none() {
        println!("\x1b[32m\x1b[1m\u{2714} {message}\x1b[0m");
    } else {
        println!("\u{2714} {message}");
    }
}

fn help(subcommand: Option<&str>) -> &'static str {
    match subcommand {
        Some("notices") => "\nruflo settings notices\nControl the statusline notices row\n\nSUBCOMMANDS:\n  status        Show whether notices are on and which source decided it\n  off           Turn off statusline notices (persistent, user-level)\n  on            Re-enable statusline notices\n  id            Print the pseudonymous notices ID (telemetry consent required)\n  rate-limited  Manually flag a Claude usage limit (ADR-312 Phase 0)\n  quota-low     Manually flag low Claude quota (ADR-314 power saver)\n",
        _ => "\nruflo settings\nView and change user preferences (notices, consents)\n\nSUBCOMMANDS:\n  notices  Control the statusline notices row\n",
    }
}
