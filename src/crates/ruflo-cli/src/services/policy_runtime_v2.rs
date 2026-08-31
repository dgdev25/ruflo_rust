//! Auto-split from services.rs
use super::*;

    use super::*;
    use sha2::{Digest, Sha256};

    pub fn evaluate_signed(action: &str, identity: &str) -> Value {
        let state = read_state("policy-runtime");
        let rules = state["rules"].as_array().cloned().unwrap_or_default();
        let mut decision = "allow".to_string();
        for rule in &rules {
            if rule["action"].as_str() == Some(action) || rule["action"].as_str() == Some("*") {
                if rule["effect"].as_str() == Some("deny") {
                    decision = "deny".into();
                    break;
                }
            }
        }
        // HMAC sign the decision.
        let msg = format!("{action}|{identity}|{decision}|{}", now_ms());
        let key = std::env::var("RUFLO_POLICY_KEY").unwrap_or_else(|_| "default-policy-key".into());
        let mut h = Sha256::new();
        h.update(key.as_bytes());
        h.update(msg.as_bytes());
        let sig: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
        let receipt = json!({
            "action": action, "identity": identity, "decision": decision,
            "signature": sig, "signedAt": now_ms(),
        });
        let mut st = read_state("policy-runtime");
        ensure_arr(&mut st, "signedLedger").push(receipt.clone());
        write_state("policy-runtime", &st);
        receipt
    }
    pub fn signed_ledger() -> Vec<Value> {
        read_state("policy-runtime")["signedLedger"].as_array().cloned().unwrap_or_default()
    }
