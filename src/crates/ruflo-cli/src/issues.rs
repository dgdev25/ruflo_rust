//! Native V3 `issues` command — collaborative issue claims (ADR-016).
//!
//! Source: issues.ts + claim-service.ts. State in .claude-flow/claims/claims.json
//! as a TS-compatible array: {claims: [...], savedAt}. Board/load/rebalance/
//! GitHub-sync deferred.

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
        IssuesCommand::Handoff { issue, to, reason } => handoff(root, &issue, &to, reason),
        IssuesCommand::Stealable { agent_type } => stealable(root, agent_type),
        IssuesCommand::Steal { issue, stealer } => steal(root, &issue, &stealer),
    }
}

fn claims_dir(root: &Path) -> PathBuf {
    root.join(".claude-flow/claims")
}
fn claims_file(root: &Path) -> PathBuf {
    claims_dir(root).join("claims.json")
}

fn load_claims(root: &Path) -> Value {
    fs::read_to_string(claims_file(root))
        .ok()
        .and_then(|r| serde_json::from_str(&r).ok())
        .unwrap_or_else(|| json!({"claims": [], "savedAt": null}))
}

fn save_claims(root: &Path, data: &mut Value) -> bool {
    if fs::create_dir_all(claims_dir(root)).is_err() {
        return false;
    }
    if let Some(o) = data.as_object_mut() {
        o.insert("savedAt".into(), json!(now_iso()));
    }
    let path = claims_file(root);
    let tmp = path.with_extension("json.tmp");
    let Ok(bytes) = serde_json::to_vec_pretty(data) else {
        return false;
    };
    fs::write(&tmp, &bytes).is_ok() && fs::rename(&tmp, &path).is_ok()
}

fn get_claim(data: &Value, issue: &str) -> Option<Value> {
    data.get("claims")?
        .as_array()?
        .iter()
        .find(|c| c.get("issueId").and_then(Value::as_str) == Some(issue))
        .cloned()
}

fn set_claim(data: &mut Value, _issue: &str, claim: Value) {
    if let Some(arr) = data.get_mut("claims").and_then(Value::as_array_mut) {
        let id = claim
            .get("issueId")
            .and_then(Value::as_str)
            .map(String::from);
        if let Some(ex) = arr
            .iter_mut()
            .find(|c| c.get("issueId").and_then(Value::as_str) == id.as_deref())
        {
            *ex = claim;
        } else {
            arr.push(claim);
        }
    }
}

fn remove_claim(data: &mut Value, issue: &str) {
    if let Some(arr) = data.get_mut("claims").and_then(Value::as_array_mut) {
        arr.retain(|c| c.get("issueId").and_then(Value::as_str) != Some(issue));
    }
}

fn all_claims(data: &Value) -> Vec<Value> {
    data.get("claims")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

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

fn claimant_from_json(v: &Value) -> String {
    if v.get("type").and_then(Value::as_str) == Some("human") {
        format!(
            "human:{}",
            v.get("name").and_then(Value::as_str).unwrap_or("?")
        )
    } else {
        format!(
            "agent:{}:{}",
            v.get("agentType").and_then(Value::as_str).unwrap_or("?"),
            v.get("agentId").and_then(Value::as_str).unwrap_or("?")
        )
    }
}

fn now_iso() -> String {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let s = ms.div_euclid(1000);
    let sub = ms.rem_euclid(1000);
    let d = s.div_euclid(86400);
    let sod = s.rem_euclid(86400);
    let (y, mo, dy) = civil(d);
    format!(
        "{y:04}-{mo:02}-{dy:02}T{:02}:{:02}:{:02}.{sub:03}Z",
        sod / 3600,
        sod % 3600 / 60,
        sod % 60
    )
}
fn civil(d: i64) -> (i64, i64, i64) {
    let z = d + 719_468;
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

fn list(root: &Path, status: Option<String>, mine: bool, json: bool) -> u8 {
    let data = load_claims(root);
    let mut claims = all_claims(&data);
    if let Some(ref s) = status {
        claims.retain(|c| c.get("status").and_then(Value::as_str) == Some(s.as_str()));
    }
    let _ = mine;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!(claims)).unwrap_or_default()
        );
        return 0;
    }
    if claims.is_empty() {
        println!("No claims found");
        return 0;
    }
    println!("\nIssue Claims (ADR-016)\n");
    for c in &claims {
        let issue = c.get("issueId").and_then(Value::as_str).unwrap_or("?");
        let cl = c
            .get("claimant")
            .map(|v| {
                if v.get("type").and_then(Value::as_str) == Some("human") {
                    format!(
                        "\u{1f464} {}",
                        v.get("name").and_then(Value::as_str).unwrap_or("?")
                    )
                } else {
                    format!(
                        "\u{1f916} {}",
                        v.get("agentType").and_then(Value::as_str).unwrap_or("?")
                    )
                }
            })
            .unwrap_or_default();
        let st = c.get("status").and_then(Value::as_str).unwrap_or("active");
        let pct = c.get("progress").and_then(Value::as_i64).unwrap_or(0);
        println!("  {issue:<20} {cl:<25} {st:<18} {pct}%");
    }
    0
}

fn claim(root: &Path, issue: &str, claimant: &Claimant) -> u8 {
    let mut data = load_claims(root);
    if let Some(ex) = get_claim(&data, issue) {
        if ex.get("status").and_then(Value::as_str).unwrap_or("active") != "stealable" {
            eprintln!(
                "[ERROR] Issue {issue} is already claimed by {}",
                ex.get("claimant")
                    .map(claimant_from_json)
                    .unwrap_or_default()
            );
            return 1;
        }
    }
    let now = now_iso();
    set_claim(
        &mut data,
        issue,
        json!({"issueId": issue, "claimant": claimant_to_json(claimant), "claimedAt": now, "status": "active", "statusChangedAt": now, "progress": 0}),
    );
    if !save_claims(root, &mut data) {
        eprintln!("[ERROR] Failed to save claims");
        return 1;
    }
    println!("Claimed issue {issue} for {}", format_claimant(claimant));
    0
}

fn release(root: &Path, issue: &str, claimant: &Claimant) -> u8 {
    let mut data = load_claims(root);
    let Some(ex) = get_claim(&data, issue) else {
        eprintln!("[ERROR] Issue {issue} is not claimed");
        return 1;
    };
    let ec = ex.get("claimant").cloned().unwrap_or(json!(null));
    let et = ec.get("type").and_then(Value::as_str).unwrap_or("");
    let ok = match (claimant, et) {
        (Claimant::human { user_id, .. }, "human") => {
            ec.get("userId").and_then(Value::as_str) == Some(user_id.as_str())
        }
        (Claimant::agent { agent_id, .. }, "agent") => {
            ec.get("agentId").and_then(Value::as_str) == Some(agent_id.as_str())
        }
        _ => false,
    };
    if !ok {
        eprintln!(
            "[ERROR] Issue {issue} is not claimed by {}",
            format_claimant(claimant)
        );
        return 1;
    }
    remove_claim(&mut data, issue);
    if !save_claims(root, &mut data) {
        eprintln!("[ERROR] Failed to save claims");
        return 1;
    }
    println!("Released issue {issue}");
    0
}

fn update_status(root: &Path, issue: &str, status: &str, note: Option<String>) -> u8 {
    let Some(parsed) = ClaimStatus::parse(status) else {
        eprintln!("[ERROR] Invalid status: {status}\n  Valid: active, paused, blocked, completed, handoff-pending, review-requested, stealable");
        return 1;
    };
    let mut data = load_claims(root);
    let Some(mut c) = get_claim(&data, issue) else {
        eprintln!("[ERROR] Issue {issue} is not claimed");
        return 1;
    };
    if let Some(o) = c.as_object_mut() {
        o.insert("status".into(), json!(parsed.as_str()));
        o.insert("statusChangedAt".into(), json!(now_iso()));
        match parsed {
            ClaimStatus::Blocked => {
                if let Some(n) = &note {
                    o.insert("blockReason".into(), json!(n));
                }
            }
            ClaimStatus::Completed => {
                o.insert("progress".into(), json!(100));
            }
            _ => {
                if let Some(n) = note {
                    o.insert("context".into(), json!(n));
                }
            }
        }
    }
    set_claim(&mut data, issue, c);
    if !save_claims(root, &mut data) {
        eprintln!("[ERROR] Failed to save claims");
        return 1;
    }
    println!("Status updated: {issue} → {}", parsed.as_str());
    0
}

fn handoff(root: &Path, issue: &str, to: &Claimant, reason: Option<String>) -> u8 {
    let mut data = load_claims(root);
    let Some(mut c) = get_claim(&data, issue) else {
        eprintln!("[ERROR] Issue {issue} is not claimed");
        return 1;
    };
    // `from` derived from existing claim (claim-service.ts:337).
    if let Some(o) = c.as_object_mut() {
        o.insert("status".into(), json!("handoff-pending"));
        o.insert("handoffTo".into(), claimant_to_json(to));
        if let Some(r) = reason {
            o.insert("handoffReason".into(), json!(r));
        }
        o.insert("statusChangedAt".into(), json!(now_iso()));
    }
    set_claim(&mut data, issue, c);
    if !save_claims(root, &mut data) {
        eprintln!("[ERROR] Failed to save claims");
        return 1;
    }
    println!("Handoff requested: {issue} → {}", format_claimant(to));
    0
}

fn stealable(root: &Path, agent_type: Option<String>) -> u8 {
    let data = load_claims(root);
    let items: Vec<_> = all_claims(&data)
        .into_iter()
        .filter(|c| c.get("status").and_then(Value::as_str) == Some("stealable"))
        .collect();
    if items.is_empty() {
        println!("No stealable issues");
        return 0;
    }
    let _ = agent_type;
    println!("Stealable issues:");
    for c in &items {
        let issue = c.get("issueId").and_then(Value::as_str).unwrap_or("?");
        let reason = c
            .get("blockReason")
            .or_else(|| c.get("context"))
            .and_then(Value::as_str)
            .unwrap_or("available");
        println!("  {issue} — {reason}");
    }
    0
}

fn steal(root: &Path, issue: &str, stealer: &Claimant) -> u8 {
    let mut data = load_claims(root);
    let Some(mut c) = get_claim(&data, issue) else {
        eprintln!("[ERROR] Issue {issue} is not claimed");
        return 1;
    };
    if c.get("status").and_then(Value::as_str).unwrap_or("active") != "stealable" {
        eprintln!("[ERROR] Issue {issue} is not stealable");
        return 1;
    }
    let now = now_iso();
    if let Some(o) = c.as_object_mut() {
        o.insert("claimant".into(), claimant_to_json(stealer));
        o.insert("status".into(), json!("active"));
        o.insert("statusChangedAt".into(), json!(now));
        o.insert("claimedAt".into(), json!(now));
    }
    set_claim(&mut data, issue, c);
    if !save_claims(root, &mut data) {
        eprintln!("[ERROR] Failed to save claims");
        return 1;
    }
    println!("Stolen issue {issue} for {}", format_claimant(stealer));
    0
}

fn help(sub: Option<&str>) -> &'static str {
    match sub {
        Some("list") => "\nruflo issues list\nList all issue claims\n\nOPTIONS:\n  -s, --status <value>  Filter by status\n  -m, --mine            Show only my claims\n      --json            Output as JSON\n",
        Some("claim") => "\nruflo issues claim\nClaim an issue\n\nOPTIONS:\n  -i, --issue <value>   Issue ID (required)\n  -a, --agent <value>   Agent as type:id\n  -u, --user <value>    User as id:name\n",
        Some("release") => "\nruflo issues release\nRelease a claim\n\nOPTIONS:\n  -i, --issue <value>   Issue ID (required)\n  -a, --agent <value>   Agent as type:id\n  -u, --user <value>    User as id:name\n",
        Some("status") => "\nruflo issues status\nUpdate claim status\n\nOPTIONS:\n  -i, --issue <value>   Issue ID (required)\n  -s, --status <value>  New status (required)\n  -n, --note <value>    Optional note\n",
        Some("handoff") => "\nruflo issues handoff\nRequest handoff\n\nOPTIONS:\n  -i, --issue <value>   Issue ID (required)\n  -a, --agent <value>   Agent to hand off TO (type:id)\n  -u, --user <value>    User to hand off TO (id:name)\n  -r, --reason <value>  Handoff reason\n",
        Some("stealable") => "\nruflo issues stealable\nList stealable issues\n",
        Some("steal") => "\nruflo issues steal\nSteal a stealable issue\n\nOPTIONS:\n  -i, --issue <value>   Issue ID (required)\n  -a, --agent <value>   Agent as type:id\n  -u, --user <value>    User as id:name\n",
        _ => "\nruflo issues\nCollaborative issue claims for human-agent workflows (ADR-016)\n\nSUBCOMMANDS:\n  list       List all issue claims\n  claim      Claim an issue\n  release    Release a claim\n  status     Update claim status\n  handoff    Request handoff\n  stealable  List stealable issues\n  steal      Steal a stealable issue\n",
    }
}
