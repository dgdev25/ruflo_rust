//! Auto-split from services.rs
use super::*;

    use super::*;
    use std::sync::Mutex as StdMutex;
    /// In-process lock — the file lock's 2s deadline starves under heavy
    /// parallel test load. Process-local serialization is enough for the
    /// budget (multi-process safety is best-effort via the state file).
    static PROC_LOCK: StdMutex<()> = StdMutex::new(());

    /// Per-model cost rates (USD per 1M tokens, blended in/out).
    fn rate_per_mtok(model: &str) -> f64 {
        match model {
            "haiku" => 1.25,
            "sonnet" => 9.0,
            "opus" => 45.0,
            "gpt-4o" | "gpt4o" => 10.0,
            "gemini-pro" | "gemini" => 3.5,
            _ => 5.0,
        }
    }

    /// Default limits (overridable via state). Concurrent=8, hourly=$5, daily=$50.
    fn defaults() -> Value {
        json!({
            "maxConcurrent": 8,
            "hourlyBudgetUsd": 5.0,
            "dailyBudgetUsd": 50.0,
            "concurrent": 0,
            "hourSpentUsd": 0.0,
            "daySpentUsd": 0.0,
            "hourStart": now_ms(),
            "dayStart": now_ms(),
            "circuitOpen": false,
        })
    }

    fn load() -> Value {
        let mut s = read_state("global-budget");
        if s.is_null() || s.as_object().map(|o| o.is_empty()).unwrap_or(true) {
            s = defaults();
            write_state("global-budget", &s);
        }
        s
    }

    fn rollover(s: &mut Value) {
        let now = now_ms();
        let hour_ms = 3_600_000u64;
        let day_ms = 86_400_000u64;
        if now.saturating_sub(s["hourStart"].as_u64().unwrap_or(now)) > hour_ms {
            s["hourSpentUsd"] = json!(0.0);
            s["hourStart"] = json!(now);
        }
        if now.saturating_sub(s["dayStart"].as_u64().unwrap_or(now)) > day_ms {
            s["daySpentUsd"] = json!(0.0);
            s["dayStart"] = json!(now);
        }
    }

    /// Check whether a spawn is allowed. Returns Ok(cost-so-far) or Err(reason).
    /// Does NOT book spend — call record() after the worker finishes.
    pub fn check() -> Result<Value, String> {
        let _g = PROC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut s = load();
        rollover(&mut s);
        let concurrent = s["concurrent"].as_u64().unwrap_or(0);
        let max_concurrent = s["maxConcurrent"].as_u64().unwrap_or(8);
        if s["circuitOpen"].as_bool() == Some(true) {
            return Err("circuit open (budget breaker tripped)".into());
        }
        if concurrent >= max_concurrent {
            return Err(format!(
                "concurrent cap reached ({concurrent}/{max_concurrent})"
            ));
        }
        let hour = s["hourSpentUsd"].as_f64().unwrap_or(0.0);
        let hour_max = s["hourlyBudgetUsd"].as_f64().unwrap_or(5.0);
        if hour >= hour_max {
            s["circuitOpen"] = json!(true);
            write_state("global-budget", &s);
            return Err(format!("hourly budget exhausted (${hour:.2}/${hour_max:.2})"));
        }
        let day = s["daySpentUsd"].as_f64().unwrap_or(0.0);
        let day_max = s["dailyBudgetUsd"].as_f64().unwrap_or(50.0);
        if day >= day_max {
            s["circuitOpen"] = json!(true);
            write_state("global-budget", &s);
            return Err(format!("daily budget exhausted (${day:.2}/${day_max:.2})"));
        }
        // Reserve a concurrent slot.
        s["concurrent"] = json!(concurrent + 1);
        write_state("global-budget", &s);
        Ok(json!({"concurrent": concurrent + 1, "hourSpentUsd": hour, "daySpentUsd": day}))
    }

    /// Book actual spend after a worker completes. Releases the concurrent slot.
    pub fn record(model: &str, tokens: u64, success: bool) -> Value {
        let _g = PROC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut s = load();
        rollover(&mut s);
        let cost = (tokens as f64 / 1_000_000.0) * rate_per_mtok(model);
        let hour = s["hourSpentUsd"].as_f64().unwrap_or(0.0) + cost;
        let day = s["daySpentUsd"].as_f64().unwrap_or(0.0) + cost;
        s["hourSpentUsd"] = json!(hour);
        s["daySpentUsd"] = json!(day);
        // Release the concurrent slot.
        let c = s["concurrent"].as_u64().unwrap_or(0).saturating_sub(1);
        s["concurrent"] = json!(c);
        // Trip the breaker on hard failure.
        if !success {
            s["circuitOpen"] = json!(true);
        }
        write_state("global-budget", &s);
        json!({"costUsd": cost, "hourSpentUsd": hour, "daySpentUsd": day,
               "concurrent": c, "model": model, "tokens": tokens})
    }

    pub fn status() -> Value {
        let mut s = load();
        rollover(&mut s);
        write_state("global-budget", &s);
        s
    }

    pub fn reset_breaker() -> bool {
        let _g = PROC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut s = load();
        s["circuitOpen"] = json!(false);
        write_state("global-budget", &s);
        true
    }
