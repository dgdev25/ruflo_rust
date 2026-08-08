//! Native V3 funnel state layer + `advisor` command (ADR-302/305/316).
//!
//! Source of truth:
//! - `v3/@claude-flow/cli/src/funnel/state.ts` — user-level JSON under ~/.ruflo
//!   (RUFLO_STATE_DIR override), 0600, write-then-rename.
//! - `v3/@claude-flow/cli/src/funnel/consent.ts` — versioned consent receipts.
//! - `v3/@claude-flow/cli/src/funnel/advisor-tip.ts` — cached co-pilot tip.
//! - `v3/@claude-flow/cli/src/commands/advisor.ts` — the command surface.

use std::fs::{self, File};
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

const CONSENT_POLICY_VERSION: u64 = 1;
const ADVISOR_REFRESH_TTL_MS: u128 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdvisorCommand {
    Status,
    Enable { yes: bool },
    Disable,
    Help { subcommand: Option<String> },
}

pub fn run(command: AdvisorCommand) -> u8 {
    match command {
        AdvisorCommand::Status => status(),
        AdvisorCommand::Enable { yes } => enable(yes),
        AdvisorCommand::Disable => disable(),
        AdvisorCommand::Help { subcommand } => {
            print!("{}", help(subcommand.as_deref()));
            0
        }
    }
}

fn status() -> u8 {
    let consented = has_consent("advisor-tips");
    println!(
        "Advisor tip consent: {}",
        if consented { "granted" } else { "not granted" }
    );
    if consented {
        if let Some(tip) = read_advisor_tip() {
            println!("Current tip: {}", tip.headline);
            if !tip.detail.is_empty() {
                println!("  {}", tip.detail);
            }
        } else {
            println!("No cached tip yet (refreshes at most once/day on session-restore).");
        }
    }
    0
}

fn enable(yes: bool) -> u8 {
    if has_consent("advisor-tips") {
        println!("Advisor tip is already enabled.");
        return 0;
    }
    println!("{ADVISOR_DISCLOSURE}");
    println!();
    if !yes {
        println!("Re-run with --yes to confirm: ruflo advisor enable --yes");
        return 0;
    }
    record_consent("advisor-tips", true, "advisor-enable");
    // recordFunnelEvent is a no-op unless telemetry is separately consented
    // (funnel/events.ts:130). Faithfully mirrored here.
    record_funnel_event("advisor_tip_enabled", "statusline");
    println_success("Advisor tip enabled.");
    println!("It refreshes at most once/day, in the background, on session-restore.");
    println!("Disable anytime: ruflo advisor disable");
    0
}

fn disable() -> u8 {
    revoke_consent("advisor-tips", "advisor-disable");
    record_funnel_event("advisor_tip_disabled", "statusline");
    println_success("Advisor tip disabled.");
    0
}

fn println_success(message: &str) {
    // output.printSuccess -> bold green check + message.
    if color_enabled() {
        println!("\x1b[32m\x1b[1m\u{2714} {message}\x1b[0m");
    } else {
        println!("\u{2714} {message}");
    }
}

fn color_enabled() -> bool {
    std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal()
}

// ─── funnel/state.ts ────────────────────────────────────────────────────────

fn funnel_state_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("RUFLO_STATE_DIR") {
        let dir = dir.to_string_lossy().into_owned();
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    home_dir().join(".ruflo")
}

fn home_dir() -> PathBuf {
    // TS uses os.homedir() (HOME, else passwd). The native CLI reads HOME; when it
    // is unset we fall back to "/". This diverges from passwd lookup only in
    // non-standard environments where HOME is unset — tests always set HOME or
    // RUFLO_STATE_DIR, so the divergence is not observable there.
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Public home accessor for sibling commands that target ~/.claude (settings).
pub fn home_dir_pub() -> PathBuf {
    home_dir()
}

/// Public ISO-8601 accessor for sibling commands' backup filenames.
pub fn now_iso_pub() -> String {
    now_iso8601()
}

fn state_path(name: &str) -> PathBuf {
    funnel_state_dir().join(name)
}

fn read_state_json(name: &str) -> Option<Value> {
    let raw = fs::read_to_string(state_path(name)).ok()?;
    serde_json::from_str(&raw).ok()
}

/// write-then-rename, 0600 file, 0700 dir (state.ts:34-47).
fn write_state_json(name: &str, value: &Value) -> bool {
    let dir = funnel_state_dir();
    if create_dir_mode_0700(&dir).is_err() {
        return false;
    }
    let target = state_path(name);
    let tmp = PathBuf::from(format!("{}.tmp", target.display()));
    let bytes = match serde_json::to_vec_pretty(value) {
        Ok(b) => b,
        Err(_) => return false,
    };
    let mut file = match open_private(&tmp) {
        Ok(f) => f,
        Err(_) => return false,
    };
    if file.write_all(&bytes).is_err() || file.write_all(b"\n").is_err() {
        let _ = fs::remove_file(&tmp);
        return false;
    }
    let _ = file.sync_all();
    drop(file);
    if fs::rename(&tmp, &target).is_err() {
        let _ = fs::remove_file(&tmp);
        return false;
    }
    set_mode_0600(&target);
    true
}

// ─── funnel/consent.ts ──────────────────────────────────────────────────────

fn read_consents() -> Value {
    read_state_json("consent.json").unwrap_or_else(|| json!({}))
}

pub fn has_consent(domain: &str) -> bool {
    let file = read_consents();
    let receipt = file.get(domain);
    receipt
        .and_then(|r| r.as_object())
        .map(|r| {
            let granted = r.get("granted").and_then(Value::as_bool) == Some(true);
            let at = r.get("at").and_then(Value::as_str).is_some();
            let policy = r.get("policyVersion").and_then(Value::as_u64);
            granted && at && policy == Some(CONSENT_POLICY_VERSION)
        })
        .unwrap_or(false)
}

pub fn record_consent(domain: &str, granted: bool, surface: &str) {
    let mut file = read_consents();
    if !file.is_object() {
        file = json!({});
    }
    if let Some(obj) = file.as_object_mut() {
        obj.insert(
            domain.to_string(),
            json!({
                "granted": granted,
                "policyVersion": CONSENT_POLICY_VERSION,
                "at": now_iso8601(),
                "surface": surface,
            }),
        );
    }
    write_state_json("consent.json", &file);
}

pub fn revoke_consent(domain: &str, surface: &str) {
    record_consent(domain, false, surface);
}

// ─── funnel/advisor-tip.ts (read path only) ─────────────────────────────────

struct CachedTip {
    headline: String,
    detail: String,
}

fn read_advisor_tip() -> Option<CachedTip> {
    let cache = read_state_json("advisor-tip.json")?;
    // advisor-tip.ts:59 rejects an empty headline (`!cache.headline`).
    let headline = cache.get("headline")?.as_str()?.to_string();
    if headline.is_empty() {
        return None;
    }
    let ts = cache.get("_ts").and_then(Value::as_u64)?;
    let detail = cache
        .get("detail")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let now = unix_millis();
    if now.saturating_sub(ts) >= ADVISOR_REFRESH_TTL_MS as u64 {
        return None;
    }
    Some(CachedTip { headline, detail })
}

// ─── funnel/events.ts (recordFunnelEvent — telemetry-gated no-op unless consented) ─

const FUNNEL_ID_TTL_MS: u64 = 90 * 24 * 60 * 60 * 1000;
const MAX_QUEUE_BYTES: usize = 256 * 1024;
const MAX_QUEUE_EVENTS: usize = 1000;
const EVENTS_FILE: &str = "funnel-events.jsonl";
const FUNNEL_ID_FILE: &str = "funnel-id.json";

/// Closed event set (events.ts:36). Unknown events are dropped.
const EVENT_NAMES: &[&str] = &[
    "disclosure_shown",
    "funnel_disabled",
    "signup_opened",
    "account_created",
    "proxy_activated",
    "promo_impression",
    "promo_open",
    "sponsor_mode_enabled",
    "sponsor_mode_disabled",
    "sponsor_capacity_exhausted",
    "power_saver_enabled",
    "power_saver_disabled",
    "toggle_cooldown_blocked",
    "training_share_enabled",
    "training_share_disabled",
    "advisor_tip_enabled",
    "advisor_tip_disabled",
];

const SURFACES: &[&str] = &["statusline", "init", "credit_exhaustion"];

/// Lazily created pseudonymous funnel ID (events.ts:75-86). UUID-shaped, derived
/// from nothing but time+pid+counter (native build has no crypto RNG dep); rotates
/// every 90 days. Exists only while telemetry consent is granted.
fn get_funnel_id() -> Option<String> {
    if !has_consent("telemetry") {
        return None;
    }
    let now = unix_millis();
    if let Some(record) = read_state_json(FUNNEL_ID_FILE) {
        if let Some(id) = record.get("id").and_then(Value::as_str) {
            if let Some(created) = record.get("createdAt").and_then(Value::as_str) {
                if parse_iso_millis(created)
                    .map(|c| now.saturating_sub(c) < FUNNEL_ID_TTL_MS)
                    .unwrap_or(false)
                {
                    return Some(id.to_string());
                }
            }
        }
    }
    let id = uuid_like();
    let record = json!({ "id": id, "createdAt": now_iso8601() });
    write_state_json(FUNNEL_ID_FILE, &record);
    Some(id)
}

fn record_funnel_event(event: &str, surface: &str) -> bool {
    // events.ts: validate closed sets, then gate on telemetry consent.
    if !EVENT_NAMES.contains(&event) || !SURFACES.contains(&surface) {
        return false;
    }
    if !has_consent("telemetry") {
        return false;
    }
    let mut payload = json!({
        "schemaVersion": 1,
        "event": event,
        "surface": surface,
        "release": env!("CARGO_PKG_VERSION"),
        "timestampBucket": daily_bucket(),
    });
    if let Some(id) = get_funnel_id() {
        payload["pseudonymousId"] = Value::String(id);
    }
    let dir = funnel_state_dir();
    if create_dir_mode_0700(&dir).is_err() {
        return false;
    }
    let path = state_path(EVENTS_FILE);
    let existing = fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> = existing
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect();
    let new_line = match serde_json::to_string(&payload) {
        Ok(s) => s,
        Err(_) => return false,
    };
    lines.push(new_line);
    // bounded queue: ≤1000 events then ≤256 KiB (events.ts:148-159)
    let start = lines.len().saturating_sub(MAX_QUEUE_EVENTS);
    let mut kept: Vec<String> = lines[start..].to_vec();
    while kept.iter().map(|l| l.len() + 1).sum::<usize>() > MAX_QUEUE_BYTES && kept.len() > 1 {
        kept.remove(0);
    }
    let mut out = kept.join("\n");
    out.push('\n');
    let mut file = match open_private(&path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    if file.write_all(out.as_bytes()).is_err() {
        return false;
    }
    set_mode_0600(&path);
    true
}

/// Parse an ISO-8601 `YYYY-MM-DDTHH:MM:SS.mmmZ` timestamp to unix millis.
fn parse_iso_millis(value: &str) -> Option<u64> {
    let b = value.as_bytes();
    if value.len() < 24 || b[4] != b'-' || b[10] != b'T' || b[23] != b'Z' {
        return None;
    }
    let year: i64 = std::str::from_utf8(&b[0..4]).ok()?.parse().ok()?;
    let month: i64 = std::str::from_utf8(&b[5..7]).ok()?.parse().ok()?;
    let day: i64 = std::str::from_utf8(&b[8..10]).ok()?.parse().ok()?;
    let hour: i64 = std::str::from_utf8(&b[11..13]).ok()?.parse().ok()?;
    let minute: i64 = std::str::from_utf8(&b[14..16]).ok()?.parse().ok()?;
    let second: i64 = std::str::from_utf8(&b[17..19]).ok()?.parse().ok()?;
    let millis: i64 = std::str::from_utf8(&b[20..23]).ok()?.parse().ok()?;
    // Date.parse rejects out-of-range components — validate them.
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=59).contains(&second)
        || !(0..=999).contains(&millis)
    {
        return None;
    }
    let days = days_from_civil(year, month, day);
    let secs = days * 86_400 + hour * 3600 + minute * 60 + second;
    Some((secs * 1000 + millis) as u64)
}

/// Inverse of civil_from_days (Hinnant days_from_civil).
fn days_from_civil(mut year: i64, mut month: i64, day: i64) -> i64 {
    if month <= 2 {
        year -= 1;
        month += 12;
    }
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let doy = (153 * (month - 3) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// UUID-shaped pseudonymous id (native build has no crypto RNG dep). Derived from
/// time + pid + counter — not cryptographically random, but unique enough for the
/// local-only attribution queue and never sent off-host in this build.
fn uuid_like() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let seed = nanos
        ^ (std::process::id() as u128).rotate_left(37)
        ^ (COUNTER.fetch_add(1, Ordering::Relaxed) as u128).wrapping_mul(0x9e37_79b9_1444_0aaf);
    let bytes = seed.to_le_bytes();
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    )
}

// ─── helpers ────────────────────────────────────────────────────────────────

#[cfg(unix)]
fn create_dir_mode_0700(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

#[cfg(not(unix))]
fn create_dir_mode_0700(dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir)
}

#[cfg(unix)]
fn set_mode_0600(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn set_mode_0600(_path: &Path) {}

/// Open a new file 0600 from creation (state.ts:39 writes with mode 0600), so the
/// sensitive state is never briefly world-readable before a post-hoc chmod.
#[cfg(unix)]
fn open_private(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn open_private(path: &Path) -> std::io::Result<File> {
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
}

fn daily_bucket() -> String {
    // events.ts dailyBucket — YYYY-MM-DD (UTC).
    let millis = unix_millis() as i64;
    let seconds = millis.div_euclid(1000);
    let days = seconds.div_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

fn now_iso8601() -> String {
    let millis = unix_millis() as i64;
    let seconds = millis.div_euclid(1000);
    let sub_millis = millis.rem_euclid(1000);
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3600;
    let minute = seconds_of_day % 3600 / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{sub_millis:03}Z")
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

const ADVISOR_DISCLOSURE: &str = "Enabling the co-pilot advisor tip.\n\nAt most once per day, ruflo will send a small STRUCTURAL snapshot of your\nsession — security scan status, swarm/agent state, git uncommitted-file\nCOUNT — never raw prompts, file contents, or commands — to a headless\nFable model (via `claude -p`) and cache one short, actionable tip for the\nstatusline insight ticker.\n\nThis is a real, metered API call (~$0.40 budget cap per\nrefresh, at most once/day). Disable anytime: ruflo advisor disable";

fn help(subcommand: Option<&str>) -> &'static str {
    match subcommand {
        Some("enable") => "\nruflo advisor enable\nOpt into the co-pilot advisor tip (ADR-316)\n\nOPTIONS:\n      --yes  Skip the confirmation prompt [default: false]\n",
        Some("disable") => "\nruflo advisor disable\nRevoke advisor-tip consent and stop generating new tips\n",
        Some("status") => "\nruflo advisor status\nShow advisor-tip consent state and the current cached tip\n",
        _ => "\nruflo advisor\nFable co-pilot advisor tip in the statusline insight ticker (ADR-316)\n\nSUBCOMMANDS:\n  enable   Opt into the co-pilot advisor tip (ADR-316)\n  disable  Revoke advisor-tip consent and stop generating new tips\n  status   Show advisor-tip consent state and the current cached tip\n",
    }
}

// ─── funnel/precedence.ts + disclosure.ts + funnel.json (settings command) ───

const TOGGLE_COOLDOWN_MS: u64 = 10 * 60 * 1000;
const NOTIFIER_TTL_MS: u64 = 6 * 60 * 60 * 1000;

pub fn funnel_state_dir_pub() -> PathBuf {
    funnel_state_dir()
}

/// ADR-314 §D1 — toggle cooldown active when lastToggleAt is within 10 min.
fn cooldown_active(last_toggle_at: Option<&str>, now: u64) -> bool {
    let Some(ts) = last_toggle_at.and_then(parse_iso_millis) else {
        return false;
    };
    now.saturating_sub(ts) < TOGGLE_COOLDOWN_MS
}

#[derive(Debug, Clone)]
pub struct NotifierStatus {
    pub limited: bool,
    pub since: Option<String>,
    pub cleared: Option<String>,
    pub last_toggle_at: Option<String>,
}

impl NotifierStatus {
    fn from_value(raw: &Value, field: &str) -> Self {
        NotifierStatus {
            limited: raw.get(field).and_then(Value::as_bool).unwrap_or(false),
            since: raw.get("since").and_then(Value::as_str).map(str::to_owned),
            cleared: raw
                .get("cleared")
                .and_then(Value::as_str)
                .map(str::to_owned),
            last_toggle_at: raw
                .get("lastToggleAt")
                .and_then(Value::as_str)
                .map(str::to_owned),
        }
    }
}

/// Read a notifier status file, applying the 6h TTL auto-expire. `field` is the
/// boolean flag key — "limited" for rate-limit, "low" for power-saver.
fn read_notifier(file: &str, field: &str) -> NotifierStatus {
    let now = unix_millis();
    let mut status = read_state_json(file)
        .map(|v| NotifierStatus::from_value(&v, field))
        .unwrap_or(NotifierStatus {
            limited: false,
            since: None,
            cleared: None,
            last_toggle_at: None,
        });
    if status.limited {
        if let Some(since) = status.since.as_deref().and_then(parse_iso_millis) {
            if now.saturating_sub(since) >= NOTIFIER_TTL_MS {
                status.limited = false;
                status.cleared = Some(now_iso8601());
            }
        }
    }
    status
}

fn mark_notifier(file: &str, field: &str) -> bool {
    let now = unix_millis();
    let current = read_notifier(file, field);
    if current.limited && current.since.is_some() {
        return true; // already flagged, not a change
    }
    if cooldown_active(current.last_toggle_at.as_deref(), now) {
        return false;
    }
    let mut rec = serde_json::Map::new();
    rec.insert(field.to_string(), Value::Bool(true));
    rec.insert(
        "since".into(),
        Value::String(current.since.clone().unwrap_or_else(now_iso8601)),
    );
    rec.insert("cleared".into(), Value::Null);
    rec.insert("lastToggleAt".into(), Value::String(now_iso8601()));
    write_state_json(file, &Value::Object(rec));
    true
}

fn clear_notifier(file: &str, field: &str) -> bool {
    let now = unix_millis();
    let current = read_notifier(file, field);
    if !current.limited {
        return true; // already clear
    }
    if cooldown_active(current.last_toggle_at.as_deref(), now) {
        return false;
    }
    let mut rec = serde_json::Map::new();
    rec.insert(field.to_string(), Value::Bool(false));
    rec.insert(
        "since".into(),
        current
            .since
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    rec.insert("cleared".into(), Value::String(now_iso8601()));
    rec.insert("lastToggleAt".into(), Value::String(now_iso8601()));
    write_state_json(file, &Value::Object(rec));
    true
}

#[allow(dead_code)]
pub fn read_rate_limit_status() -> NotifierStatus {
    read_notifier("rate-limit-status.json", "limited")
}
pub fn mark_rate_limited() -> bool {
    mark_notifier("rate-limit-status.json", "limited")
}
pub fn clear_rate_limit_status() -> bool {
    clear_notifier("rate-limit-status.json", "limited")
}
#[allow(dead_code)]
pub fn read_quota_low_status() -> NotifierStatus {
    read_notifier("quota-status.json", "low")
}
pub fn mark_quota_low() -> bool {
    mark_notifier("quota-status.json", "low")
}
pub fn clear_quota_low_status() -> bool {
    clear_notifier("quota-status.json", "low")
}

// ─── disclosure.ts (3-state: never_seen / disclosed_enabled / disclosed_disabled) ─

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisclosureState {
    NeverSeen,
    DisclosedEnabled,
    DisclosedDisabled,
}

impl DisclosureState {
    pub fn as_str(self) -> &'static str {
        match self {
            DisclosureState::NeverSeen => "never_seen",
            DisclosureState::DisclosedEnabled => "disclosed_enabled",
            DisclosureState::DisclosedDisabled => "disclosed_disabled",
        }
    }
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "never_seen" => DisclosureState::NeverSeen,
            "disclosed_enabled" => DisclosureState::DisclosedEnabled,
            "disclosed_disabled" => DisclosureState::DisclosedDisabled,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct DisclosureRecord {
    pub state: DisclosureState,
    pub first_shown_at: Option<String>,
}

pub fn get_disclosure() -> DisclosureRecord {
    match read_state_json("funnel-disclosure.json") {
        Some(v) => {
            let state = v
                .get("state")
                .and_then(Value::as_str)
                .and_then(DisclosureState::parse);
            match state {
                Some(state) => DisclosureRecord {
                    state,
                    first_shown_at: v
                        .get("firstShownAt")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                },
                None => DisclosureRecord {
                    state: DisclosureState::NeverSeen,
                    first_shown_at: None,
                },
            }
        }
        None => DisclosureRecord {
            state: DisclosureState::NeverSeen,
            first_shown_at: None,
        },
    }
}

pub fn record_disclosure_declined() {
    let first = get_disclosure().first_shown_at;
    let rec = json!({
        "state": "disclosed_disabled",
        "firstShownAt": first.unwrap_or_else(now_iso8601),
    });
    write_state_json("funnel-disclosure.json", &rec);
}

pub fn record_disclosure_reenabled() {
    let rec = json!({ "state": "disclosed_enabled", "firstShownAt": now_iso8601() });
    write_state_json("funnel-disclosure.json", &rec);
}

// ─── funnel.json user config + deleteFunnelData + resolveFunnelEnabled ──────

pub fn set_user_config_enabled(enabled: bool) {
    let mut cfg = read_state_json("funnel.json").unwrap_or(json!({}));
    if !cfg.is_object() {
        cfg = json!({});
    }
    if let Some(obj) = cfg.as_object_mut() {
        obj.insert("enabled".into(), Value::Bool(enabled));
    }
    write_state_json("funnel.json", &cfg);
}

/// deleteFunnelData: drop the pseudonymous id + the local event queue (ADR-305).
pub fn delete_funnel_data() {
    let _ = fs::remove_file(state_path(FUNNEL_ID_FILE));
    let _ = fs::remove_file(state_path(EVENTS_FILE));
}

#[derive(Debug, Clone)]
pub struct FunnelEnabledDecision {
    pub enabled: bool,
    pub decided_by: &'static str,
}

fn env_disabled() -> bool {
    std::env::var("RUFLO_FUNNEL")
        .ok()
        .map(|v| {
            let t = v.trim().to_ascii_lowercase();
            matches!(t.as_str(), "0" | "false" | "off" | "no")
        })
        .unwrap_or(false)
}

fn enterprise_policy_disabled() -> bool {
    let mut candidates = Vec::new();
    if let Some(p) = std::env::var_os("RUFLO_ENTERPRISE_POLICY") {
        candidates.push(PathBuf::from(p));
    }
    if cfg!(target_os = "windows") {
        // precedence.ts:30 — %ProgramData%\ruflo\policy.json
        if let Some(program_data) = std::env::var_os("ProgramData") {
            candidates.push(
                PathBuf::from(program_data)
                    .join("ruflo")
                    .join("policy.json"),
            );
        }
    } else {
        candidates.push(PathBuf::from("/etc/ruflo/policy.json"));
    }
    for p in candidates {
        if let Ok(raw) = fs::read_to_string(&p) {
            if let Ok(v) = serde_json::from_str::<Value>(&raw) {
                if v.get("funnel").and_then(|f| f.get("enabled")) == Some(&Value::Bool(false)) {
                    return true;
                }
            }
        }
    }
    false
}

fn user_config_disabled() -> bool {
    read_state_json("funnel.json").and_then(|c| c.get("enabled").and_then(Value::as_bool))
        == Some(false)
}

fn project_config_disabled(cwd: &Path) -> bool {
    fs::read_to_string(cwd.join("claude-flow.config.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|c| {
            c.get("funnel")
                .and_then(|f| f.get("enabled"))
                .and_then(Value::as_bool)
        })
        == Some(false)
}

fn remote_policy_disabled() -> bool {
    read_state_json("funnel-remote-policy.json")
        .and_then(|p| p.get("funnelEnabled").and_then(Value::as_bool))
        == Some(false)
}

/// ADR-305 precedence: env > enterprise > user-config > project-config >
/// disclosure-declined > remote-policy > package-default. Strict AND chain.
pub fn resolve_funnel_enabled(cwd: &Path) -> FunnelEnabledDecision {
    if env_disabled() {
        return FunnelEnabledDecision {
            enabled: false,
            decided_by: "env",
        };
    }
    if enterprise_policy_disabled() {
        return FunnelEnabledDecision {
            enabled: false,
            decided_by: "enterprise-policy",
        };
    }
    if user_config_disabled() {
        return FunnelEnabledDecision {
            enabled: false,
            decided_by: "user-config",
        };
    }
    if project_config_disabled(cwd) {
        return FunnelEnabledDecision {
            enabled: false,
            decided_by: "project-config",
        };
    }
    if get_disclosure().state == DisclosureState::DisclosedDisabled {
        return FunnelEnabledDecision {
            enabled: false,
            decided_by: "disclosure-declined",
        };
    }
    if remote_policy_disabled() {
        return FunnelEnabledDecision {
            enabled: false,
            decided_by: "remote-policy",
        };
    }
    FunnelEnabledDecision {
        enabled: true,
        decided_by: "package-default",
    }
}

/// Public consents map accessor for the settings overview.
pub fn read_consents_pub() -> Value {
    read_consents()
}

/// Public pseudonymous funnel-id accessor for `settings notices id`.
pub fn get_funnel_id_pub() -> Option<String> {
    get_funnel_id()
}

// ─── disclosure accept/promoEligible + payout enrollment (funnel command) ────

const DISCLOSURE_GRACE_MS: u64 = 24 * 60 * 60 * 1000;

/// Backdate firstShownAt past the 24h grace so promo rotation starts at once.
pub fn record_disclosure_accepted() -> DisclosureRecord {
    let backdated = unix_millis().saturating_sub(DISCLOSURE_GRACE_MS + 1000);
    let iso = iso_from_millis(backdated as i64);
    let rec = json!({ "state": "disclosed_enabled", "firstShownAt": iso });
    write_state_json("funnel-disclosure.json", &rec);
    DisclosureRecord {
        state: DisclosureState::DisclosedEnabled,
        first_shown_at: Some(iso),
    }
}

/// promoEligible (disclosure.ts:122): disclosed_enabled + firstShownAt + age ≥ 24h.
pub fn promo_eligible() -> bool {
    let rec = get_disclosure();
    if rec.state != DisclosureState::DisclosedEnabled {
        return false;
    }
    let Some(first) = rec.first_shown_at.as_deref().and_then(parse_iso_millis) else {
        return false;
    };
    unix_millis().saturating_sub(first) >= DISCLOSURE_GRACE_MS
}

#[derive(Debug, Clone)]
pub struct PayoutEnrollment {
    pub kyc_status: String,
    pub enrolled_at: String,
    pub payout_account_last4: String,
    #[allow(dead_code)]
    pub enrollment_token: Option<String>,
}

pub fn get_enrollment() -> Option<PayoutEnrollment> {
    let v = read_state_json("funnel-payout.json")?;
    Some(PayoutEnrollment {
        kyc_status: v.get("kyc_status").and_then(Value::as_str)?.to_string(),
        enrolled_at: v.get("enrolled_at").and_then(Value::as_str)?.to_string(),
        payout_account_last4: v
            .get("payout_account_last4")
            .and_then(Value::as_str)?
            .to_string(),
        enrollment_token: v
            .get("enrollment_token")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

pub fn delete_enrollment() -> bool {
    // payout.ts:58 writes JSON null (idempotent success). get_enrollment treats
    // a null file the same as absent (fields missing → None).
    write_state_json("funnel-payout.json", &Value::Null)
}

/// isEarningEligible (payout.ts:34): consent + verified KYC + enrollment token.
pub fn is_earning_eligible() -> bool {
    if !has_consent("rev-share-payout") {
        return false;
    }
    match get_enrollment() {
        Some(rec) => {
            rec.kyc_status == "verified"
                && rec
                    .enrollment_token
                    .as_deref()
                    .map(|t| !t.is_empty())
                    .unwrap_or(false)
        }
        None => false,
    }
}

fn iso_from_millis(millis: i64) -> String {
    let seconds = millis.div_euclid(1000);
    let sub_millis = millis.rem_euclid(1000);
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3600;
    let minute = seconds_of_day % 3600 / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{sub_millis:03}Z")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static STATE_LOCK: Mutex<()> = Mutex::new(());

    fn isolated_state(test_name: &str) -> (tempfile::TempDir, std::sync::MutexGuard<'static, ()>) {
        let guard = STATE_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("RUFLO_STATE_DIR", dir.path());
        let _ = test_name;
        (dir, guard)
    }

    #[test]
    fn consent_round_trip_and_policy_version_gate() {
        let (_dir, _guard) = isolated_state("consent");
        assert!(!has_consent("advisor-tips"));
        record_consent("advisor-tips", true, "test");
        assert!(has_consent("advisor-tips"));
        // stale policy version breaks effective consent
        let mut file = read_consents();
        file["advisor-tips"]["policyVersion"] = json!(99);
        write_state_json("consent.json", &file);
        assert!(!has_consent("advisor-tips"));
    }

    #[test]
    fn revoke_sets_granted_false() {
        let (_dir, _guard) = isolated_state("revoke");
        record_consent("advisor-tips", true, "test");
        assert!(has_consent("advisor-tips"));
        revoke_consent("advisor-tips", "test");
        assert!(!has_consent("advisor-tips"));
    }

    #[test]
    fn advisor_tip_read_respects_ttl_and_shape() {
        let (_dir, _guard) = isolated_state("tip");
        assert!(read_advisor_tip().is_none());
        let fresh = json!({"_ts": unix_millis(), "headline": "h", "detail": "d"});
        write_state_json("advisor-tip.json", &fresh);
        let tip = read_advisor_tip().unwrap();
        assert_eq!(tip.headline, "h");
        assert_eq!(tip.detail, "d");
        // stale (>24h) -> None
        let stale = json!({"_ts": unix_millis().saturating_sub(2 * ADVISOR_REFRESH_TTL_MS as u64), "headline": "old", "detail": ""});
        write_state_json("advisor-tip.json", &stale);
        assert!(read_advisor_tip().is_none());
    }

    #[test]
    fn record_funnel_event_noop_without_telemetry_consent() {
        let (_dir, _guard) = isolated_state("events");
        // telemetry not consented -> event is a no-op, no file written
        assert!(!record_funnel_event("advisor_tip_enabled", "statusline"));
        assert!(!state_path("funnel-events.jsonl").exists());
    }

    #[test]
    fn record_funnel_event_with_telemetry_carries_pseudonymous_id_and_caps_queue() {
        let (_dir, _guard) = isolated_state("events_on");
        record_consent("telemetry", true, "test");
        assert!(record_funnel_event("advisor_tip_enabled", "statusline"));
        // a pseudonymous funnel-id file is created
        let id_rec = read_state_json(FUNNEL_ID_FILE).expect("funnel-id written");
        let id = id_rec.get("id").and_then(Value::as_str).unwrap();
        assert_eq!(id.len(), 36); // 8-4-4-4-12 hex
                                  // the event line carries pseudonymousId
        let raw = fs::read_to_string(state_path(EVENTS_FILE)).unwrap();
        assert!(raw.contains("pseudonymousId"));
        assert!(raw.contains("advisor_tip_enabled"));

        // unknown event / surface rejected
        assert!(!record_funnel_event("nonsense", "statusline"));
        assert!(!record_funnel_event("advisor_tip_enabled", "nowhere"));

        // queue capped at MAX_QUEUE_EVENTS
        for _ in 0..(MAX_QUEUE_EVENTS + 50) {
            record_funnel_event("advisor_tip_disabled", "statusline");
        }
        let count = fs::read_to_string(state_path(EVENTS_FILE))
            .unwrap()
            .lines()
            .filter(|l| !l.is_empty())
            .count();
        assert!(count <= MAX_QUEUE_EVENTS, "queue grew to {count}");
        let bytes = fs::read_to_string(state_path(EVENTS_FILE)).unwrap().len();
        assert!(bytes <= MAX_QUEUE_BYTES + 512, "queue {bytes} bytes");
    }

    #[test]
    fn state_files_use_atomic_rename() {
        let (_dir, _guard) = isolated_state("atomic");
        write_state_json("consent.json", &json!({"x": 1}));
        assert!(state_path("consent.json").is_file());
        assert!(!state_path("consent.json.tmp").exists());
    }
}
