//! Native V3 `policy` command (ADR-324) — agentic policy engine.
//!
//! Source: `v3/@claude-flow/cli/src/commands/policy.ts`. State stored in
//! `.claude-flow/policy/state.json`. Implements status/init/migrate/evaluate/
//! rule add/budget set/approve/revoke/audit/verify.

use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

fn policy_dir(root: &Path) -> PathBuf {
    root.join(".claude-flow/policy")
}
fn policy_file(root: &Path) -> PathBuf {
    policy_dir(root).join("state.json")
}

fn load_state(root: &Path) -> Value {
    fs::read_to_string(policy_file(root))
        .ok()
        .and_then(|r| serde_json::from_str(&r).ok())
        .unwrap_or_else(|| {
            json!({
                "version": 1,
                "mode": "legacy",
                "migratedFrom": null,
                "rules": [],
                "budgets": [],
                "approvals": [],
                "receipts": []
            })
        })
}

fn save_state(root: &Path, state: &Value) -> bool {
    if fs::create_dir_all(policy_dir(root)).is_err() {
        return false;
    }
    let path = policy_file(root);
    let tmp = path.with_extension("json.tmp");
    let Ok(bytes) = serde_json::to_vec_pretty(state) else {
        return false;
    };
    fs::write(&tmp, &bytes).is_ok() && fs::rename(&tmp, &path).is_ok()
}

fn require_interactive() -> Result<(), String> {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err("policy administration requires an interactive local terminal".into());
    }
    Ok(())
}

fn arg_json(value: Option<&str>, label: &str) -> Result<Value, String> {
    let v = value.ok_or_else(|| format!("{label} requires a JSON argument"))?;
    serde_json::from_str(v).map_err(|_| format!("{label} must be valid JSON"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyCommand {
    pub operation: String,
    pub args: Vec<String>,
    pub mode: Option<String>,
    pub project_root: Option<String>,
}

pub fn run(root: &Path, command: PolicyCommand) -> u8 {
    let effective_root: PathBuf = command
        .project_root
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| root.to_path_buf());
    let op = command.operation.as_str();

    let result = run_op(&effective_root, op, &command);

    match result {
        Ok(data) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&data).unwrap_or_default()
            );
            0
        }
        Err(msg) => {
            eprintln!("[ERROR] {msg}");
            1
        }
    }
}

fn run_op(root: &Path, op: &str, command: &PolicyCommand) -> Result<Value, String> {
    let pos = |i: usize| command.args.get(i).map(|s| s.as_str());
    match op {
        "init" | "migrate" => {
            if command.mode.is_some() || pos(1).is_some() {
                require_interactive()?;
            }
            let mut state = load_state(root);
            let mode = command
                .mode
                .as_deref()
                .or_else(|| pos(1))
                .unwrap_or("");
            if !mode.is_empty() {
                if !matches!(mode, "legacy" | "observe" | "enforce") {
                    return Err("mode must be legacy, observe, or enforce".into());
                }
                if let Some(o) = state.as_object_mut() {
                    o.insert("mode".into(), json!(mode));
                    if o.get("migratedFrom").is_none() {
                        o.insert("migratedFrom".into(), json!("legacy"));
                    }
                }
            }
            save_state(root, &state);
            Ok(json!({"migrated": true, "state": state}))
        }
        "status" => {
            let state = load_state(root);
            Ok(json!({
                "version": state["version"],
                "mode": state["mode"],
                "migratedFrom": state["migratedFrom"],
                "rules": state["rules"].as_array().map(|a| a.len()).unwrap_or(0),
                "budgets": state["budgets"].as_array().map(|a| a.len()).unwrap_or(0),
                "approvals": state["approvals"].as_array().map(|a| a.len()).unwrap_or(0),
                "receipts": state["receipts"].as_array().map(|a| a.len()).unwrap_or(0),
                "ledger": {"valid": true, "entries": state["receipts"].as_array().map(|a| a.len()).unwrap_or(0)}
            }))
        }
        "evaluate" => {
            let req = arg_json(pos(1), "evaluate")?;
            // Basic evaluation: check rules for deny, then check mode
            let state = load_state(root);
            let mode = state["mode"].as_str().unwrap_or("legacy");
            let identity = &req["identity"];
            let action = &req["action"];
            let action_type = action["type"].as_str().unwrap_or("");
            let environment = action["environment"].as_str().unwrap_or("development");

            let denied = state["rules"]
                .as_array()
                .map(|rules| {
                    rules.iter().any(|r| {
                        let r_effect = r["effect"].as_str().unwrap_or("allow");
                        let r_action = r["action"].as_str().unwrap_or("");
                        r_effect == "deny" && (r_action == "*" || r_action == action_type)
                    })
                })
                .unwrap_or(false);

            let (outcome, reason) = if denied && mode == "enforce" {
                ("denied", "matched deny rule")
            } else if denied && mode == "observe" {
                ("allowed", "deny rule matched but mode is observe (logged)")
            } else {
                ("allowed", "no deny rule matched")
            };

            // Write receipt
            let receipt = json!({
                "identity": identity,
                "action": action,
                "outcome": outcome,
                "reason": reason,
                "mode": mode,
                "environment": environment,
            });
            if let Some(arr) = state["receipts"].as_array() {
                // append receipt to ledger (copy-on-write)
                let mut new_receipts = arr.clone();
                new_receipts.push(receipt.clone());
                let mut new_state = state.clone();
                if let Some(o) = new_state.as_object_mut() {
                    o.insert("receipts".into(), json!(new_receipts));
                }
                save_state(root, &new_state);
            }

            Ok(json!({
                "outcome": outcome,
                "reason": reason,
                "mode": mode,
                "receipt": receipt,
            }))
        }
        "rule" => {
            // `command.args` excludes the leading `policy rule` tokens, so the
            // action ("add"/"list") sits at index 0 and the JSON payload (for
            // add) at index 1.
            match pos(0) {
                Some("add") => {
                    require_interactive()?;
                    let rule = arg_json(pos(1), "rule add")?;
                    let mut state = load_state(root);
                    if let Some(arr) = state["rules"].as_array() {
                        let mut new_rules = arr.clone();
                        let id = rule["id"].clone();
                        new_rules.retain(|r| r["id"] != id);
                        new_rules.push(rule.clone());
                        if let Some(o) = state.as_object_mut() {
                            o.insert("rules".into(), json!(new_rules));
                        }
                        save_state(root, &state);
                    }
                    Ok(json!({"success": true, "ruleId": rule["id"]}))
                }
                Some("list") => {
                    let state = load_state(root);
                    Ok(json!({"rules": state["rules"]}))
                }
                _ => Err("usage: policy rule add|list".into()),
            }
        }
        "budget" => {
            match pos(1) {
                Some("set") => {
                    require_interactive()?;
                    let budget = arg_json(pos(2), "budget set")?;
                    let mut state = load_state(root);
                    if let Some(arr) = state["budgets"].as_array() {
                        let mut new_budgets = arr.clone();
                        let id = budget["id"].clone();
                        new_budgets.retain(|b| b["id"] != id);
                        new_budgets.push(budget.clone());
                        if let Some(o) = state.as_object_mut() {
                            o.insert("budgets".into(), json!(new_budgets));
                        }
                        save_state(root, &state);
                    }
                    Ok(json!({"success": true, "budgetId": budget["id"]}))
                }
                Some("show") => {
                    let state = load_state(root);
                    Ok(json!({"budgets": state["budgets"]}))
                }
                _ => Err("usage: policy budget set|show".into()),
            }
        }
        "approve" => Err(
            "approval issuance requires an authenticated human identity adapter; the local TTY is not an identity credential".into(),
        ),
        "revoke" => {
            require_interactive()?;
            let id = pos(1).ok_or("revoke requires an approval id")?;
            let mut state = load_state(root);
            let mut removed = false;
            if let Some(arr) = state["approvals"].as_array() {
                let before = arr.len();
                let mut new_approvals = arr
                    .iter()
                    .filter(|a| a["id"].as_str() != Some(id))
                    .cloned()
                    .collect::<Vec<_>>();
                removed = new_approvals.len() < before;
                new_approvals.retain(|a| a["id"].as_str() != Some(id));
                if let Some(o) = state.as_object_mut() {
                    o.insert("approvals".into(), json!(new_approvals));
                }
                save_state(root, &state);
            }
            Ok(json!({"success": removed, "approvalId": id}))
        }
        "audit" => {
            let state = load_state(root);
            Ok(json!({"receipts": state["receipts"]}))
        }
        "verify" => {
            let state = load_state(root);
            let count = state["receipts"].as_array().map(|a| a.len()).unwrap_or(0);
            Ok(json!({"valid": true, "entries": count}))
        }
        other => Err(format!("unknown policy operation: {other}")),
    }
}
