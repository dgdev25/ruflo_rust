//! Native V3 `funnel` command (ADR-301/305/309/317) — user control surface for
//! the Cognitum lifecycle funnel.
//!
//! Source of truth: `v3/@claude-flow/cli/src/commands/funnel.ts`. Reuses the
//! funnel primitives (precedence, disclosure, payout enrollment, telemetry id)
//! implemented in `funnel.rs`.

use std::io::IsTerminal;
use std::path::Path;
use std::process::Command;

use serde_json::{json, Value};

use crate::funnel;

const ENROLL_URL: &str = "https://funnel.ruv.io/enroll";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FunnelCommand {
    Status { json: bool },
    Disable,
    Enable,
    Accept,
    Open,
    Enroll,
    Earnings { json: bool },
    Unenroll,
    Id,
    Help { subcommand: Option<String> },
}

pub fn run(root: &Path, command: FunnelCommand) -> u8 {
    match command {
        FunnelCommand::Status { json } => status(root, json),
        FunnelCommand::Disable => disable(),
        FunnelCommand::Enable => enable(root),
        FunnelCommand::Accept => accept(root),
        FunnelCommand::Open => open(),
        FunnelCommand::Enroll => enroll(root),
        FunnelCommand::Earnings { json } => earnings(json),
        FunnelCommand::Unenroll => unenroll(),
        FunnelCommand::Id => id(),
        FunnelCommand::Help { subcommand } => {
            print!("{}", help(subcommand.as_deref()));
            0
        }
    }
}

fn status(root: &Path, json: bool) -> u8 {
    let decision = funnel::resolve_funnel_enabled(root);
    let disclosure = funnel::get_disclosure();
    let consents = funnel::read_consents_pub();
    let data = json!({
        "enabled": decision.enabled,
        "decidedBy": decision.decided_by,
        "disclosure": disclosure.state.as_str(),
        "stateDir": funnel::funnel_state_dir_pub().display().to_string(),
        "consents": consents,
    });
    if json {
        println!("{data}");
        return 0;
    }
    println!(
        "Funnel: {} (decided by: {})",
        if decision.enabled {
            "enabled"
        } else {
            "disabled"
        },
        decision.decided_by
    );
    println!("Disclosure: {}", disclosure.state.as_str());
    println!("State dir: {}", funnel::funnel_state_dir_pub().display());
    let domains: Vec<String> = consents
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    if domains.is_empty() {
        println!("Consents: none recorded");
    } else {
        let parts: Vec<String> = domains
            .iter()
            .map(|d| {
                let granted = consents
                    .get(d)
                    .and_then(|r| r.get("granted"))
                    .and_then(Value::as_bool)
                    == Some(true);
                format!("{d}={}", if granted { "granted" } else { "declined" })
            })
            .collect();
        println!("Consents: {}", parts.join(", "));
    }
    0
}

fn disable() -> u8 {
    funnel::set_user_config_enabled(false);
    funnel::record_disclosure_declined();
    funnel::delete_funnel_data();
    print_success("Funnel disabled. All promotional surfaces are off; local funnel data deleted.");
    0
}

fn enable(root: &Path) -> u8 {
    funnel::set_user_config_enabled(true);
    funnel::record_disclosure_reenabled();
    let decision = funnel::resolve_funnel_enabled(root);
    if decision.enabled {
        print_success("Funnel enabled.");
    } else {
        eprintln!(
            "[WARN] User preference recorded, but the funnel stays disabled by a higher-precedence source: {}",
            decision.decided_by
        );
    }
    0
}

fn accept(root: &Path) -> u8 {
    let decision = funnel::resolve_funnel_enabled(root);
    if !decision.enabled {
        eprintln!(
            "[WARN] Funnel is currently disabled by: {}. Run 'ruflo funnel enable' first, then re-run accept.",
            decision.decided_by
        );
        return 1;
    }
    if funnel::get_disclosure().state == funnel::DisclosureState::DisclosedDisabled {
        eprintln!("[WARN] Disclosure is in a declined state. Run 'ruflo funnel enable' first, then re-run accept.");
        return 1;
    }
    let rec = funnel::record_disclosure_accepted();
    let eligible = funnel::promo_eligible();
    print_success(&format!(
        "Disclosure accepted (firstShownAt backdated to {}). Promo rotation eligible: {eligible}.",
        rec.first_shown_at.as_deref().unwrap_or("?")
    ));
    0
}

/// readCurrentPromo — ~/.ruflo/statusline-promo.json {promo:{text,url?,kind?}}.
fn read_current_promo() -> Option<Value> {
    let path = funnel::home_dir_pub()
        .join(".ruflo")
        .join("statusline-promo.json");
    let raw = std::fs::read_to_string(&path).ok()?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    v.get("promo").filter(|p| p.is_object()).cloned()
}

const OPEN_ALLOWED_HOSTS: &[&str] = &[
    "cognitum.one",
    "www.cognitum.one",
    "docs.cognitum.one",
    "agentics.org",
    "www.agentics.org",
    "funnel.ruv.io",
    "cognitum-analytics-63rzcdswba-uc.a.run.app",
];

fn open() -> u8 {
    let Some(promo) = read_current_promo() else {
        eprintln!("[WARN] No promo has been shown yet. Wait for the statusline to render one, then re-run 'ruflo funnel open'.");
        return 1;
    };
    let url = match promo.get("url").and_then(Value::as_str) {
        Some(u) => u.to_string(),
        None => {
            eprintln!(
                "[WARN] Current promo (kind={}) has no URL. Nothing to open: \"{}\"",
                promo
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown"),
                promo.get("text").and_then(Value::as_str).unwrap_or("")
            );
            return 1;
        }
    };
    // funnel.ts:199-209 — `new URL(...)` throws for malformed URLs (separate
    // error), then protocol + allowlist gate the well-formed ones.
    let Some((scheme, host)) = parse_url(&url) else {
        eprintln!("[ERROR] Promo URL is malformed: {url}");
        return 1;
    };
    if scheme != "https" || !OPEN_ALLOWED_HOSTS.contains(&host.as_str()) {
        eprintln!("[ERROR] Refusing to open URL — not on the allowlist: {scheme}//{host}");
        return 1;
    }
    match open_in_browser(&url) {
        Ok(()) => {
            print_success(&format!("Opened: {url}"));
            0
        }
        Err(e) => {
            eprintln!("[ERROR] Failed to open URL: {e}");
            println!("URL for manual copy: {url}");
            1
        }
    }
}

/// Minimal https URL parse → (scheme, host) mirroring `URL.hostname`. Handles
/// port suffixes and query/fragment before any path slash.
/// Mirrors `new URL(...)`: returns Some((scheme, host)) for any parseable URL
/// and None when `new URL` would throw. Uses the canonical WHATWG `url` crate so
/// scheme/host/port rules (IPv6 literals, userinfo, spaces, digit-led schemes)
/// match the TypeScript reference exactly. Callers gate on scheme + allowlist
/// separately from this parse.
fn parse_url(raw: &str) -> Option<(String, String)> {
    let parsed = url::Url::parse(raw).ok()?;
    let host = parsed.host_str()?.to_string();
    Some((parsed.scheme().to_string(), host))
}

#[cfg(target_os = "macos")]
fn open_in_browser(url: &str) -> std::io::Result<()> {
    Command::new("open").arg(url).status().and_then(|s| {
        s.success()
            .then_some(())
            .ok_or_else(|| std::io::Error::other("browser launcher exited non-zero"))
    })
}
#[cfg(target_os = "windows")]
fn open_in_browser(url: &str) -> std::io::Result<()> {
    Command::new("cmd")
        .args(["/c", "start", "", url])
        .status()
        .and_then(|s| {
            s.success().then_some(()).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "browser launcher exited non-zero",
                )
            })
        })
}
#[cfg(all(unix, not(target_os = "macos")))]
fn open_in_browser(url: &str) -> std::io::Result<()> {
    Command::new("xdg-open").arg(url).status().and_then(|s| {
        s.success()
            .then_some(())
            .ok_or_else(|| std::io::Error::other("browser launcher exited non-zero"))
    })
}

fn enroll(root: &Path) -> u8 {
    let decision = funnel::resolve_funnel_enabled(root);
    if !decision.enabled {
        eprintln!(
            "[WARN] Funnel is currently disabled by: {}. Enable it (ruflo funnel enable) before enrolling — there's nothing to earn from until the funnel is on.",
            decision.decided_by
        );
        return 1;
    }
    if funnel::get_disclosure().state != funnel::DisclosureState::DisclosedEnabled {
        eprintln!("[WARN] The Cognitum disclosure hasn't been shown/accepted yet. Run 'ruflo funnel accept' first, then re-run enroll.");
        return 1;
    }
    funnel::record_consent("rev-share-payout", true, "cli-funnel-enroll");
    println!("Consent recorded for rev-share-payout.");
    println!();
    println!("The backend enrollment endpoint (Stripe Connect + KYC) is not yet live.");
    println!("When it ships, this command will open your browser to:");
    println!();
    println!("  {ENROLL_URL}");
    println!();
    println!("Meanwhile, your consent is on file. Track the rollout at ADR-317 (v3/docs/adr/).");
    0
}

fn earnings(json: bool) -> u8 {
    let consented = funnel::has_consent("rev-share-payout");
    let rec = funnel::get_enrollment();
    let eligible = funnel::is_earning_eligible();
    let consent_str = if consented { "granted" } else { "not-granted" };
    let note = if rec.is_some() {
        "Earnings endpoint (funnel.ruv.io/v1/earnings) is not yet live — see ADR-317 Phase 1."
    } else {
        "Not enrolled. Run `ruflo funnel enroll` to opt in."
    };
    let enrollment_json = rec.as_ref().map(|r| {
        json!({
            "kyc_status": r.kyc_status,
            "enrolled_at": r.enrolled_at,
            "payout_account_last4": r.payout_account_last4,
        })
    });
    let summary = json!({
        "consent": consent_str,
        "enrollment": enrollment_json,
        "earning_eligible": eligible,
        "note": note,
    });
    if json {
        println!("{summary}");
        return 0;
    }
    println!("Consent: {consent_str}");
    println!("Enrolled: {}", if rec.is_some() { "yes" } else { "no" });
    if let Some(r) = &rec {
        println!("  KYC: {}", r.kyc_status);
        println!("  Payout account: ****{}", r.payout_account_last4);
        println!("  Since: {}", r.enrolled_at);
    }
    println!("Earning: {}", if eligible { "yes" } else { "no" });
    println!();
    println!("{note}");
    0
}

fn unenroll() -> u8 {
    let had_enrollment = funnel::get_enrollment().is_some();
    let had_consent = funnel::has_consent("rev-share-payout");
    funnel::record_consent("rev-share-payout", false, "cli-funnel-unenroll");
    funnel::delete_enrollment();
    if had_enrollment || had_consent {
        print_success(
            "Unenrolled locally. Funnel messages still render; your install just no longer earns.",
        );
        println!("Note: server-side revocation happens on next contact with funnel.ruv.io — the backend endpoint is not yet live (ADR-317 Phase 1).");
    } else {
        println!(
            "Nothing to unenroll — no local enrollment record and no rev-share consent was set."
        );
    }
    0
}

fn id() -> u8 {
    match funnel::get_funnel_id_pub() {
        Some(id) => println!("{id}"),
        None => println!("No funnel ID (telemetry consent not granted, or funnel data deleted)."),
    }
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
        Some("status") => "\nruflo funnel status\nShow effective funnel state and which source decided it\n\nOPTIONS:\n      --json  Output as JSON [default: false]\n",
        Some("disable") => "\nruflo funnel disable\nDisable all funnel surfaces (user-level, persists across projects)\n",
        Some("enable") => "\nruflo funnel enable\nRe-enable funnel surfaces at the user tier (env/enterprise disables still win)\n",
        Some("accept") => "\nruflo funnel accept\nAcknowledge the disclosure so rotation starts immediately (skips the 24h grace window)\n",
        Some("open") => "\nruflo funnel open\nOpen the currently-shown statusline promo URL in the default browser\n",
        Some("enroll") => "\nruflo funnel enroll\nOpt in to the 50/50 revenue share on Cognitum sponsor spend (ADR-317, Phase 0)\n",
        Some("earnings") => "\nruflo funnel earnings\nShow accrued and paid revenue-share balance (ADR-317)\n\nOPTIONS:\n      --json  Output as JSON [default: false]\n",
        Some("unenroll") => "\nruflo funnel unenroll\nRevoke rev-share enrollment locally (funnel itself stays enabled)\n",
        Some("id") => "\nruflo funnel id\nPrint the pseudonymous funnel ID (exists only with telemetry consent)\n",
        _ => "\nruflo funnel\nControl the Cognitum lifecycle funnel surfaces (tips, enrollment, notices)\n\nSUBCOMMANDS:\n  status    Show effective funnel state and which source decided it\n  disable   Disable all funnel surfaces (user-level, persists across projects)\n  enable    Re-enable funnel surfaces at the user tier (env/enterprise disables still win)\n  accept    Acknowledge the disclosure so rotation starts immediately (skips the 24h grace window)\n  open      Open the currently-shown statusline promo URL in the default browser\n  enroll    Opt in to the 50/50 revenue share on Cognitum sponsor spend (ADR-317, Phase 0)\n  earnings  Show accrued and paid revenue-share balance (ADR-317)\n  unenroll  Revoke rev-share enrollment locally (funnel itself stays enabled)\n  id        Print the pseudonymous funnel ID (exists only with telemetry consent)\n",
    }
}

#[cfg(test)]
mod tests {
    use super::parse_url;

    #[test]
    fn parse_url_extracts_host_past_port_query_fragment() {
        assert_eq!(
            parse_url("https://funnel.ruv.io/enroll"),
            Some(("https".into(), "funnel.ruv.io".into()))
        );
        assert_eq!(
            parse_url("https://funnel.ruv.io:443/enroll?x=1"),
            Some(("https".into(), "funnel.ruv.io".into()))
        );
        assert_eq!(
            parse_url("https://docs.cognitum.one/path#frag"),
            Some(("https".into(), "docs.cognitum.one".into()))
        );
        assert_eq!(
            parse_url("https://evil.example/x"),
            Some(("https".into(), "evil.example".into()))
        );
        // valid non-https URL is parseable (wrong protocol -> allowlist refusal,
        // NOT malformed).
        assert_eq!(
            parse_url("http://funnel.ruv.io/"),
            Some(("http".into(), "funnel.ruv.io".into()))
        );
        // malformed: no scheme://, invalid port, empty host, digit-led scheme,
        // space in host — all rejected by WHATWG `new URL`.
        assert_eq!(parse_url("not a url"), None);
        assert_eq!(parse_url("https://funnel.ruv.io:bogus/"), None);
        assert_eq!(parse_url("https://funnel.ruv.io:99999/"), None);
        assert_eq!(parse_url("1https://example.com/"), None);
        assert_eq!(parse_url("https://exa mple.com/"), None);
        // IPv6 literal host parses (host includes brackets, per host_str).
        assert_eq!(
            parse_url("https://[::1]/"),
            Some(("https".into(), "[::1]".into()))
        );
        // userinfo is stripped by host_str (host = funnel.ruv.io).
        assert_eq!(
            parse_url("https://user:pass@funnel.ruv.io/path"),
            Some(("https".into(), "funnel.ruv.io".into()))
        );
        // empty port is accepted/normalized (WHATWG)
        assert_eq!(
            parse_url("https://funnel.ruv.io:/path"),
            Some(("https".into(), "funnel.ruv.io".into()))
        );
    }
}
