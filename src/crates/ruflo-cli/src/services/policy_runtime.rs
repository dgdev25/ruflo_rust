//! Auto-split from services.rs
use super::*;

    use super::*;

    pub fn evaluate(action: &str, identity: &str) -> Value {
        let state = read_state("policy-runtime");
        let rules = state["rules"].as_array().cloned().unwrap_or_default();
        let mut decision = "allow".to_string();
        for rule in &rules {
            if rule["action"].as_str() == Some(action) || rule["action"].as_str() == Some("*") {
                if rule["effect"].as_str() == Some("deny") {
                    decision = "deny".to_string();
                    break;
                }
            }
        }
        let result = json!({
            "action": action,
            "identity": identity,
            "decision": decision,
            "evaluatedAt": now_ms(),
        });
        let mut st = read_state("policy-runtime");
        ensure_arr(&mut st, "ledger").push(result.clone());
        write_state("policy-runtime", &st);
        result
    }

    pub fn add_rule(action: &str, effect: &str) -> bool {
        let mut state = read_state("policy-runtime");
        let rules = ensure_arr(&mut state, "rules");
        rules.push(json!({"action": action, "effect": effect, "addedAt": now_ms()}));
        write_state("policy-runtime", &state);
        true
    }

    pub fn ledger() -> Vec<Value> {
        read_state("policy-runtime")["ledger"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }
