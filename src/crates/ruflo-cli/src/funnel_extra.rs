//! Funnel completion — ports the remaining TS funnel/ files not yet covered
//! by funnel.rs. Adds: payout, disclosure, insights, events, promo, messages,
//! attribution, enrollment, rotation, precedence, credit/power-saver notifiers.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn state_dir() -> PathBuf {
    let home = std::env::var("RUFLO_STATE_DIR")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".ruflo")
}

fn read_state(name: &str) -> Value {
    fs::read_to_string(state_dir().join(format!("{name}.json")))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}))
}

fn write_state(name: &str, v: &Value) -> bool {
    let dir = state_dir();
    let _ = fs::create_dir_all(&dir);
    let path = dir.join(format!("{name}.json"));
    let tmp = dir.join(format!("{name}.json.tmp"));
    let bytes = serde_json::to_vec_pretty(v).unwrap_or_default();
    if fs::write(&tmp, &bytes).is_err() { return false; }
    let ok = fs::rename(&tmp, &path).is_ok();
    if !ok { let _ = fs::remove_file(&tmp); }
    ok
}

// ---- payout.ts ---- payout tracking

pub mod payout {
    use super::*;
    pub fn record(amount: f64, currency: &str, reason: &str) -> Value {
        let mut state = read_state("payout");
        let entry = json!({"amount": amount, "currency": currency, "reason": reason, "at": now_ms()});
        if state["history"].is_null() { state["history"] = json!([]); }
        state["history"].as_array_mut().unwrap().push(entry.clone());
        state["total"] = json!(state["history"].as_array().map(|a| a.iter()
            .filter_map(|e| e["amount"].as_f64()).sum::<f64>()).unwrap_or(0.0));
        write_state("payout", &state);
        entry
    }
    pub fn total() -> f64 {
        read_state("payout")["total"].as_f64().unwrap_or(0.0)
    }
    pub fn history() -> Vec<Value> {
        read_state("payout")["history"].as_array().cloned().unwrap_or_default()
    }
}

// ---- disclosure.ts ---- disclosure text + templates

pub mod disclosure {
    pub const FULL_DISCLOSURE: &str = "Ruflo V3 includes optional telemetry and funnel features. \
        Anonymous usage data may be collected to improve the product. \
        No personal information is transmitted. You can disable this at any time \
        with `ruflo funnel disable`.";

    pub const SHORT_DISCLOSURE: &str = "Ruflo collects anonymous telemetry. Disable: `ruflo funnel disable`.";

    pub fn for_surface(surface: &str) -> String {
        match surface {
            "statusline" | "compact" => SHORT_DISCLOSURE.into(),
            _ => FULL_DISCLOSURE.into(),
        }
    }
}

// ---- insights.ts ---- usage aggregation

pub mod insights {
    use super::*;
    pub fn record(event: &str, metadata: Value) {
        let mut state = read_state("insights");
        if state["events"].is_null() { state["events"] = json!([]); }
        state["events"].as_array_mut().unwrap().push(json!({
            "event": event, "metadata": metadata, "at": now_ms()
        }));
        write_state("insights", &state);
    }
    pub fn summary() -> Value {
        let state = read_state("insights");
        let events = state["events"].as_array().cloned().unwrap_or_default();
        let mut by_type: HashMap<String, u64> = HashMap::new();
        for e in &events {
            if let Some(et) = e["event"].as_str() {
                *by_type.entry(et.into()).or_insert(0) += 1;
            }
        }
        json!({"totalEvents": events.len(), "byType": by_type, "windowHours": 24})
    }
}

// ---- events.ts ---- funnel event types + queue

pub mod events {
    use super::*;
    pub fn enqueue(event: &str, surface: &str, payload: Value) -> Value {
        let mut state = read_state("funnel-events");
        if state["queue"].is_null() { state["queue"] = json!([]); }
        let entry = json!({"event": event, "surface": surface, "payload": payload, "at": now_ms()});
        state["queue"].as_array_mut().unwrap().push(entry.clone());
        write_state("funnel-events", &state);
        entry
    }
    pub fn drain() -> Vec<Value> {
        let state = read_state("funnel-events");
        let queue = state["queue"].as_array().cloned().unwrap_or_default();
        if !queue.is_empty() {
            write_state("funnel-events", &json!({"queue": []}));
        }
        queue
    }
    pub fn pending_count() -> usize {
        read_state("funnel-events")["queue"].as_array().map(|q| q.len()).unwrap_or(0)
    }
}

// ---- promo.ts ---- promotional messaging

pub mod promo {
    use super::*;
    pub fn record_impression(campaign: &str) {
        let mut state = read_state("promo");
        let key = campaign;
        let count = state[key]["impressions"].as_u64().unwrap_or(0) + 1;
        state[key] = json!({"impressions": count, "lastShown": now_ms()});
        write_state("promo", &state);
    }
    pub fn record_dismissal(campaign: &str) {
        let mut state = read_state("promo");
        if state[campaign].is_null() { state[campaign] = json!({}); }
        state[campaign]["dismissed"] = json!(true);
        state[campaign]["dismissedAt"] = json!(now_ms());
        write_state("promo", &state);
    }
    pub fn is_dismissed(campaign: &str) -> bool {
        read_state("promo")[campaign]["dismissed"].as_bool().unwrap_or(false)
    }
}

// ---- attribution.ts ---- attribution tracking

pub mod attribution {
    use super::*;
    pub fn record(source: &str, campaign: &str, action: &str) -> Value {
        let mut state = read_state("attribution");
        if state["entries"].is_null() { state["entries"] = json!([]); }
        let entry = json!({"source": source, "campaign": campaign, "action": action, "at": now_ms()});
        state["entries"].as_array_mut().unwrap().push(entry.clone());
        write_state("attribution", &state);
        entry
    }
    pub fn entries() -> Vec<Value> {
        read_state("attribution")["entries"].as_array().cloned().unwrap_or_default()
    }
}

// ---- enrollment.ts ---- enrollment state

pub mod enrollment {
    use super::*;
    pub fn enroll(user_id: &str, tier: &str) -> Value {
        let state = json!({"userId": user_id, "tier": tier, "enrolledAt": now_ms(), "status": "active"});
        write_state("enrollment", &state);
        state
    }
    pub fn unenroll() -> bool {
        let mut state = read_state("enrollment");
        if state.is_null() { return false; }
        state["status"] = json!("unenrolled");
        state["unenrolledAt"] = json!(now_ms());
        write_state("enrollment", &state);
        true
    }
    pub fn status() -> Value { read_state("enrollment") }
}

// ---- rotation.ts ---- content rotation

pub mod rotation {
    use super::*;
    pub fn next_item(pool: &str, items: &[String]) -> Option<String> {
        if items.is_empty() { return None; }
        let mut state = read_state("rotation");
        let idx = state[pool]["index"].as_u64().unwrap_or(0) as usize;
        let item = items[idx % items.len()].clone();
        state[pool] = json!({"index": (idx + 1) % items.len().max(1)});
        write_state("rotation", &state);
        Some(item)
    }
}

// ---- precedence.ts ---- message precedence ordering

pub mod precedence {
    use serde_json::Value;
    pub fn order_messages(messages: &mut [Value]) {
        messages.sort_by(|a, b| {
            let pa = a["priority"].as_u64().unwrap_or(0);
            let pb = b["priority"].as_u64().unwrap_or(0);
            pb.cmp(&pa) // higher priority first
        });
    }
    pub fn should_show(message: &Value, dismissed: &[String]) -> bool {
        let id = message["id"].as_str().unwrap_or("");
        !dismissed.iter().any(|d| d == id)
    }
}

// ---- credit-notifier.ts + power-saver-notifier.ts ---- notification state

pub mod credit_notifier {
    use super::*;
    pub fn notify(amount: f64, threshold: f64) -> Option<Value> {
        if amount < threshold {
            return Some(json!({"type": "credit-low", "amount": amount, "threshold": threshold, "at": now_ms()}));
        }
        None
    }
}

pub mod power_saver {
    use super::*;
    pub fn should_throttle() -> bool {
        let state = read_state("power-saver");
        state["enabled"].as_bool().unwrap_or(false) && state["active"].as_bool().unwrap_or(false)
    }
    pub fn set_enabled(enabled: bool) {
        let mut state = read_state("power-saver");
        state["enabled"] = json!(enabled);
        state["updatedAt"] = json!(now_ms());
        write_state("power-saver", &state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Reuse the process-wide env lock from funnel so RUFLO_STATE_DIR mutation
    // is serialized across both modules (and any other test touching the env).
    use crate::funnel::TEST_STATE_LOCK as LOCK;

    #[test]
    fn payout_record_total() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: LOCK serializes all RUFLO_STATE_DIR access process-wide.
        std::env::set_var("RUFLO_STATE_DIR", dir.path());
        payout::record(10.0, "USD", "tip");
        payout::record(5.0, "USD", "bonus");
        assert!((payout::total() - 15.0).abs() < 0.01);
        assert_eq!(payout::history().len(), 2);
        // SAFETY: see above.
        std::env::remove_var("RUFLO_STATE_DIR");
    }

    #[test]
    fn events_enqueue_drain() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: LOCK serializes all RUFLO_STATE_DIR access process-wide.
        std::env::set_var("RUFLO_STATE_DIR", dir.path());
        events::enqueue("click", "statusline", json!({"x": 1}));
        assert_eq!(events::pending_count(), 1);
        let drained = events::drain();
        assert_eq!(drained.len(), 1);
        assert_eq!(events::pending_count(), 0);
        // SAFETY: see above.
        std::env::remove_var("RUFLO_STATE_DIR");
    }

    #[test]
    fn promo_dismiss() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: LOCK serializes all RUFLO_STATE_DIR access process-wide.
        std::env::set_var("RUFLO_STATE_DIR", dir.path());
        assert!(!promo::is_dismissed("campaign1"));
        promo::record_dismissal("campaign1");
        assert!(promo::is_dismissed("campaign1"));
        // SAFETY: see above.
        std::env::remove_var("RUFLO_STATE_DIR");
    }

    #[test]
    fn precedence_orders() {
        let mut msgs = vec![
            json!({"id": "a", "priority": 1}),
            json!({"id": "b", "priority": 5}),
            json!({"id": "c", "priority": 3}),
        ];
        precedence::order_messages(&mut msgs);
        assert_eq!(msgs[0]["id"].as_str(), Some("b")); // highest first
    }

    #[test]
    fn rotation_cycles() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: LOCK serializes all RUFLO_STATE_DIR access process-wide.
        std::env::set_var("RUFLO_STATE_DIR", dir.path());
        let items = vec!["a".into(), "b".into(), "c".into()];
        assert_eq!(rotation::next_item("pool1", &items).unwrap(), "a");
        assert_eq!(rotation::next_item("pool1", &items).unwrap(), "b");
        assert_eq!(rotation::next_item("pool1", &items).unwrap(), "c");
        assert_eq!(rotation::next_item("pool1", &items).unwrap(), "a"); // wraps
        // SAFETY: see above.
        std::env::remove_var("RUFLO_STATE_DIR");
    }
}
