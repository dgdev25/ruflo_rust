//! Native V3 `issues` command — collaborative issue claims (ADR-016).
//!
//! Source: `v3/@claude-flow/cli/src/commands/issues.ts` +
//! `v3/@claude-flow/cli/src/services/claim-service.ts`. State machine stored in
//! `.claude-flow/claims/claims.json`. Implements claim/release/status/handoff/
//! stealable/steal/list. Board/load/rebalance are secondary surfaces deferred.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
#[allow(non_camel_case_types)]
pub enum Claimant {
    human {
        user_id: String,
        name: String,
    },
    agent {
        agent_id: String,
        agent_type: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimStatus {
    Active,
    Paused,
    HandoffPending,
    ReviewRequested,
    Blocked,
    Stealable,
    Completed,
}

impl ClaimStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::HandoffPending => "handoff-pending",
            Self::ReviewRequested => "review-requested",
            Self::Blocked => "blocked",
            Self::Stealable => "stealable",
            Self::Completed => "completed",
        }
    }
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "active" => Self::Active,
            "paused" => Self::Paused,
            "handoff-pending" => Self::HandoffPending,
            "review-requested" => Self::ReviewRequested,
            "blocked" => Self::Blocked,
            "stealable" => Self::Stealable,
            "completed" => Self::Completed,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssuesCommand {
    List {
        status: Option<String>,
        mine: bool,
        json: bool,
    },
    Claim {
        issue: String,
        claimant: Claimant,
    },
    Release {
        issue: String,
        claimant: Claimant,
    },
    Status {
        issue: String,
        status: String,
        note: Option<String>,
    },
    Handoff {
        issue: String,
        from: Claimant,
        to: Claimant,
        reason: Option<String>,
    },
    Stealable {
        agent_type: Option<String>,
    },
    Steal {
        issue: String,
        stealer: Claimant,
    },
    Help {
        subcommand: Option<String>,
    },
}

pub fn run(root: &Path, command: IssuesCommand) -> u8 {
    match command {
        IssuesCommand::Help { subcommand } => {
            print!("{}", help(subcommand.as_deref()));
            0
        }
        IssuesCommand::List { status, mine, json } => list(root, status, mine, json),
        IssuesCommand::Claim { issue, claimant } => claim(root, &issue, &claimant),
        IssuesCommand::Release { issue, claimant } => release(root, &issue, &claimant),
        IssuesCommand::Status {
            issue,
            status,
            note,
        } => update_status(root, &issue, &status, note),
        IssuesCommand::Handoff {
            issue,
            from,
            to,
            reason,
        } => handoff(root, &issue, &from, &to, reason),
        IssuesCommand::Stealable { agent_type } => stealable(root, agent_type),
        IssuesCommand::Steal { issue, stealer } => steal(root, &issue, &stealer),
    }
}

// ─── store ───────────────────────────────────────────────────────────────────

fn claims_dir(root: &Path) -> PathBuf {
    root.join(".claude-flow/claims")
}
fn claims_file(root: &Path) -> PathBuf {
    claims_dir(root).join("claims.json")
}

fn load_claims(root: &Path) -> Value {
    fs::read_to_string(claims_file(root))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_else(|| json!({"claims": {}}))
}

fn save_claims(root: &Path, data: &Value) {
    let dir = claims_dir(root);
    let _ = fs::create_dir_all(&dir);
    let path = claims_file(root);
    let tmp = path.with_extension("json.tmp");
    if let Ok(bytes) = serde_json::to_vec_pretty(data) {
        if fs::write(&tmp, bytes).is_ok() {
            let _ = fs::rename(&tmp, &path);
        }
    }
}

fn get_claim(data: &Value, issue: &str) -> Option<Value> {
    data.get("claims")?.get(issue).cloned()
}

fn set_claim(data: &mut Value, issue: &str, claim: Value) {
    if let Some(claims) = data.get_mut("claims").and_then(Value::as_object_mut) {
        claims.insert(issue.into(), claim);
    }
}

fn remove_claim(data: &mut Value, issue: &str) {
    if let Some(claims) = data.get_mut("claims").and_then(Value::as_object_mut) {
        claims.remove(issue);
    }
}

fn all_claims(data: &Value) -> Vec<(String, Value)> {
    data.get("claims")
        .and_then(Value::as_object)
        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default()
}

// ─── claimant helpers ────────────────────────────────────────────────────────

fn format_claimant(c: &Claimant) -> String {
    match c {
        Claimant::human { name, .. } => format!("human:{name}"),
        Claimant::agent {
            agent_id,
            agent_type,
        } => format!("agent:{agent_type}:{agent_id}"),
    }
}

fn claimant_to_json(c: &Claimant) -> Value {
    match c {
        Claimant::human { user_id, name } => json!({"type":"human","userId":user_id,"name":name}),
        Claimant::agent {
            agent_id,
            agent_type,
        } => json!({"type":"agent","agentId":agent_id,"agentType":agent_type}),
    }
}

// ─── now (deterministic ISO for CLI runtime) ─────────────────────────────────

fn now_iso() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let seconds = millis.div_euclid(1000);
    let sub = millis.rem_euclid(1000);
    let days = seconds.div_euclid(86400);
    let sod = seconds.rem_euclid(86400);
    let (y, mo, d) = civil(days);
    format!(
        "{y:04}-{mo:02}-{d:02}T{:02}:{:02}:{:02}.{sub:03}Z",
        sod / 3600,
        sod % 3600 / 60,
        sod % 60
    )
}
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    y += (month <= 2) as i64;
    (y, month, day)
}

// ─── actions ─────────────────────────────────────────────────────────────────

fn list(root: &Path, status: Option<String>, mine: bool, json: bool) -> u8 {
    let data = load_claims(root);
    let mut claims = all_claims(&data);
    if let Some(ref s) = status {
        claims.retain(|(_, c)| c.get("status").and_then(Value::as_str) == Some(s.as_str()));
    }
    if mine {
        // "mine" filter needs a claimant identity — for CLI, filter by the
        // current agent/user. This is a simplification (TS checks ctx identity).
    }
    if json {
        let arr: Vec<Value> = claims.iter().map(|(_, v)| v.clone()).collect();
        println!("{}", serde_json::to_string_pretty(&json!(arr)).unwrap());
        return 0;
    }
    if claims.is_empty() {
        println!("No claims found");
        return 0;
    }
    println!();
    println!("Issue Claims (ADR-016)");
    println!();
    for (issue, claim) in &claims {
        let claimant = claim
            .get("claimant")
            .map(|c| {
                if c.get("type") == Some(&json!("human")) {
                    format!(
                        "👤 {}",
                        c.get("name").and_then(Value::as_str).unwrap_or("?")
                    )
                } else {
                    format!(
                        "🤖 {}",
                        c.get("agentType").and_then(Value::as_str).unwrap_or("?")
                    )
                }
            })
            .unwrap_or_default();
        let status = claim
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("active");
        let progress = claim.get("progress").and_then(Value::as_i64).unwrap_or(0);
        println!("  {issue:<20} {claimant:<25} {status:<18} {progress}%");
    }
    0
}

fn claim(root: &Path, issue: &str, claimant: &Claimant) -> u8 {
    let mut data = load_claims(root);
    if let Some(existing) = get_claim(&data, issue) {
        let existing_status = existing
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("active");
        if existing_status != "stealable" {
            let existing_claimant = existing
                .get("claimant")
                .map(|c| {
                    if c.get("type") == Some(&json!("human")) {
                        format!(
                            "human:{}",
                            c.get("name").and_then(Value::as_str).unwrap_or("?")
                        )
                    } else {
                        format!(
                            "agent:{}:{}",
                            c.get("agentType").and_then(Value::as_str).unwrap_or("?"),
                            c.get("agentId").and_then(Value::as_str).unwrap_or("?")
                        )
                    }
                })
                .unwrap_or_default();
            eprintln!("[ERROR] Issue {issue} is already claimed by {existing_claimant}");
            return 1;
        }
    }
    let now = now_iso();
    let claim = json!({
        "issueId": issue,
        "claimant": claimant_to_json(claimant),
        "claimedAt": now,
        "status": "active",
        "statusChangedAt": now,
        "progress": 0,
    });
    set_claim(&mut data, issue, claim);
    save_claims(root, &data);
    println!("Claimed issue {issue} for {}", format_claimant(claimant));
    0
}

fn release(root: &Path, issue: &str, claimant: &Claimant) -> u8 {
    let mut data = load_claims(root);
    let Some(existing) = get_claim(&data, issue) else {
        eprintln!("[ERROR] Issue {issue} is not claimed");
        return 1;
    };
    let existing_claimant_json = existing.get("claimant").cloned().unwrap_or(json!(null));
    let existing_type = existing_claimant_json
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("");
    let matches = match (claimant, existing_type) {
        (Claimant::human { user_id, .. }, "human") => {
            existing_claimant_json.get("userId").and_then(Value::as_str) == Some(user_id.as_str())
        }
        (Claimant::agent { agent_id, .. }, "agent") => {
            existing_claimant_json
                .get("agentId")
                .and_then(Value::as_str)
                == Some(agent_id.as_str())
        }
        _ => false,
    };
    if !matches {
        eprintln!(
            "[ERROR] Issue {issue} is not claimed by {}",
            format_claimant(claimant)
        );
        return 1;
    }
    remove_claim(&mut data, issue);
    save_claims(root, &data);
    println!("Released issue {issue}");
    0
}

fn update_status(root: &Path, issue: &str, status: &str, note: Option<String>) -> u8 {
    let Some(parsed) = ClaimStatus::parse(status) else {
        eprintln!("[ERROR] Invalid status: {status}");
        eprintln!("  Valid: active, paused, blocked, completed, handoff-pending, review-requested, stealable");
        return 1;
    };
    let mut data = load_claims(root);
    let Some(mut claim) = get_claim(&data, issue) else {
        eprintln!("[ERROR] Issue {issue} is not claimed");
        return 1;
    };
    if let Some(obj) = claim.as_object_mut() {
        obj.insert("status".into(), json!(parsed.as_str()));
        obj.insert("statusChangedAt".into(), json!(now_iso()));
        if let Some(n) = note {
            obj.insert("context".into(), json!(n));
        }
    }
    set_claim(&mut data, issue, claim);
    save_claims(root, &data);
    println!("Status updated: {issue} → {}", parsed.as_str());
    0
}

fn handoff(root: &Path, issue: &str, from: &Claimant, to: &Claimant, reason: Option<String>) -> u8 {
    let mut data = load_claims(root);
    let Some(mut claim) = get_claim(&data, issue) else {
        eprintln!("[ERROR] Issue {issue} is not claimed");
        return 1;
    };
    // Verify current owner is `from`
    let current = claim.get("claimant").cloned().unwrap_or(json!(null));
    let current_type = current.get("type").and_then(Value::as_str).unwrap_or("");
    let matches = match (from, current_type) {
        (Claimant::human { user_id, .. }, "human") => {
            current.get("userId").and_then(Value::as_str) == Some(user_id.as_str())
        }
        (Claimant::agent { agent_id, .. }, "agent") => {
            current.get("agentId").and_then(Value::as_str) == Some(agent_id.as_str())
        }
        _ => false,
    };
    if !matches {
        eprintln!(
            "[ERROR] Issue {issue} is not claimed by {}",
            format_claimant(from)
        );
        return 1;
    }
    if let Some(obj) = claim.as_object_mut() {
        obj.insert("status".into(), json!("handoff-pending"));
        obj.insert("handoffTo".into(), claimant_to_json(to));
        if let Some(r) = reason {
            obj.insert("handoffReason".into(), json!(r));
        }
        obj.insert("statusChangedAt".into(), json!(now_iso()));
    }
    set_claim(&mut data, issue, claim);
    save_claims(root, &data);
    println!("Handoff requested: {issue} → {}", format_claimant(to));
    0
}

fn stealable(root: &Path, agent_type: Option<String>) -> u8 {
    let data = load_claims(root);
    let claims = all_claims(&data);
    let stealable: Vec<_> = claims
        .into_iter()
        .filter(|(_, c)| c.get("status").and_then(Value::as_str) == Some("stealable"))
        .collect();
    if stealable.is_empty() {
        println!("No stealable issues");
        return 0;
    }
    let _ = agent_type; // type filter deferred
    println!("Stealable issues:");
    for (issue, claim) in &stealable {
        let reason = claim
            .get("blockReason")
            .or_else(|| claim.get("context"))
            .and_then(Value::as_str)
            .unwrap_or("available");
        println!("  {issue} — {reason}");
    }
    0
}

fn steal(root: &Path, issue: &str, stealer: &Claimant) -> u8 {
    let mut data = load_claims(root);
    let Some(mut claim) = get_claim(&data, issue) else {
        eprintln!("[ERROR] Issue {issue} is not claimed");
        return 1;
    };
    let status = claim
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("active");
    if status != "stealable" {
        eprintln!("[ERROR] Issue {issue} is not stealable (status: {status})");
        return 1;
    }
    let now = now_iso();
    if let Some(obj) = claim.as_object_mut() {
        obj.insert("claimant".into(), claimant_to_json(stealer));
        obj.insert("status".into(), json!("active"));
        obj.insert("statusChangedAt".into(), json!(now));
        obj.insert("claimedAt".into(), json!(now));
    }
    set_claim(&mut data, issue, claim);
    save_claims(root, &data);
    println!("Stolen issue {issue} for {}", format_claimant(stealer));
    0
}

// ─── help ────────────────────────────────────────────────────────────────────

fn help(sub: Option<&str>) -> &'static str {
    match sub {
        Some("list") => "\nruflo issues list\nList all issue claims\n\nOPTIONS:\n  -s, --status <value>  Filter by status\n  -m, --mine            Show only my claims\n      --json            Output as JSON\n",
        Some("claim") => "\nruflo issues claim\nClaim an issue\n\nOPTIONS:\n  -i, --issue <value>   Issue ID (required)\n  -a, --agent <value>   Agent ID\n  -u, --user <value>    User name\n",
        Some("release") => "\nruflo issues release\nRelease a claim\n\nOPTIONS:\n  -i, --issue <value>   Issue ID (required)\n  -a, --agent <value>   Agent ID\n  -u, --user <value>    User name\n",
        Some("status") => "\nruflo issues status\nUpdate claim status\n\nOPTIONS:\n  -i, --issue <value>   Issue ID (required)\n  -s, --status <value>  New status (required)\n  -n, --note <value>    Optional note\n",
        Some("handoff") => "\nruflo issues handoff\nRequest handoff\n\nOPTIONS:\n  -i, --issue <value>   Issue ID (required)\n  -a, --agent <value>   Agent to hand off TO\n  -u, --user <value>    User to hand off TO\n  -r, --reason <value>  Handoff reason\n",
        Some("stealable") => "\nruflo issues stealable\nList stealable issues\n",
        Some("steal") => "\nruflo issues steal\nSteal a stealable issue\n\nOPTIONS:\n  -i, --issue <value>   Issue ID (required)\n  -a, --agent <value>   Agent ID\n  -u, --user <value>    User name\n",
        _ => "\nruflo issues\nCollaborative issue claims for human-agent workflows (ADR-016)\n\nSUBCOMMANDS:\n  list       List all issue claims\n  claim      Claim an issue\n  release    Release a claim\n  status     Update claim status\n  handoff    Request handoff\n  stealable  List stealable issues\n  steal      Steal a stealable issue\n",
    }
}
