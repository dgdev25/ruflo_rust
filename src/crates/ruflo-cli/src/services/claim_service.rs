//! Auto-split from services.rs
use super::*;

    use super::*;

    /// State file name — `.claude-flow/services/claim-service.json`.
    const STATE_NAME: &str = "claim-service";

    /// Ensure state is a JSON array; return a mutable reference to it.
    /// `read_state` returns `json!({})` for a missing file, so we normalize
    /// any non-array state to an empty array on first contact.
    fn claims_array_mut(state: &mut Value) -> Result<&mut Vec<Value>, String> {
        if !state.is_array() {
            *state = json!([]);
        }
        state
            .as_array_mut()
            .ok_or_else(|| "claim-service state corrupted (not an array)".to_string())
    }

    fn push_history(entry: &mut Value, event: Value) {
        if entry["history"].is_null() {
            entry["history"] = json!([]);
        }
        if let Some(hist) = entry["history"].as_array_mut() {
            hist.push(event);
        }
    }

    /// Claim an issue for `claimant_agent_id`. Fails if the issue is already
    /// actively claimed or has a pending handoff.
    pub fn claim(
        issue_id: &str,
        claimant_agent_id: &str,
        claimant_agent_type: &str,
    ) -> Result<Value, String> {
        let _guard = LockGuard::acquire(STATE_NAME)
            .ok_or_else(|| "claim-service lock contention".to_string())?;
        let mut state = read_state(STATE_NAME);
        let arr = claims_array_mut(&mut state)?;
        if let Some(existing) = arr.iter().find(|c| c["issueId"].as_str() == Some(issue_id)) {
            let status = existing["status"].as_str().unwrap_or("");
            if status == "active" || status == "handoff_pending" {
                return Err(format!(
                    "issue `{issue_id}` already claimed (status: {status})"
                ));
            }
        }
        let entry = json!({
            "issueId": issue_id,
            "claimant": {"id": claimant_agent_id, "type": claimant_agent_type},
            "status": "active",
            "history": [
                {"event": "claimed", "at": now_ms(), "by": claimant_agent_id, "type": claimant_agent_type}
            ],
        });
        // Drop any prior (released/stolen) entry for this issue before appending.
        arr.retain(|c| c["issueId"].as_str() != Some(issue_id));
        arr.push(entry.clone());
        if !write_state(STATE_NAME, &state) {
            return Err("failed to write claim-service state".to_string());
        }
        Ok(entry)
    }

    /// Release a claim. Only the current claimant may release.
    pub fn release(issue_id: &str, claimant_agent_id: &str) -> Result<(), String> {
        let _guard = LockGuard::acquire(STATE_NAME)
            .ok_or_else(|| "claim-service lock contention".to_string())?;
        let mut state = read_state(STATE_NAME);
        let arr = claims_array_mut(&mut state)?;
        let entry = arr
            .iter_mut()
            .find(|c| c["issueId"].as_str() == Some(issue_id))
            .ok_or_else(|| format!("issue `{issue_id}` not found"))?;
        if entry["claimant"]["id"].as_str() != Some(claimant_agent_id) {
            return Err(format!(
                "issue `{issue_id}` not claimed by `{claimant_agent_id}`"
            ));
        }
        entry["status"] = json!("released");
        push_history(
            entry,
            json!({"event": "released", "at": now_ms(), "by": claimant_agent_id}),
        );
        if !write_state(STATE_NAME, &state) {
            return Err("failed to write claim-service state".to_string());
        }
        Ok(())
    }

    /// Request handoff of an issue from one agent to another. Sets status
    /// `handoff_pending`; the target must call `accept_handoff` to complete.
    pub fn handoff(
        issue_id: &str,
        from_agent: &str,
        to_agent: &str,
        reason: &str,
    ) -> Result<(), String> {
        let _guard = LockGuard::acquire(STATE_NAME)
            .ok_or_else(|| "claim-service lock contention".to_string())?;
        let mut state = read_state(STATE_NAME);
        let arr = claims_array_mut(&mut state)?;
        let entry = arr
            .iter_mut()
            .find(|c| c["issueId"].as_str() == Some(issue_id))
            .ok_or_else(|| format!("issue `{issue_id}` not found"))?;
        if entry["claimant"]["id"].as_str() != Some(from_agent) {
            return Err(format!(
                "issue `{issue_id}` not claimed by `{from_agent}`"
            ));
        }
        entry["status"] = json!("handoff_pending");
        entry["pendingHandoffTo"] = json!(to_agent);
        push_history(
            entry,
            json!({
                "event": "handoff_requested",
                "at": now_ms(),
                "from": from_agent,
                "to": to_agent,
                "reason": reason,
            }),
        );
        if !write_state(STATE_NAME, &state) {
            return Err("failed to write claim-service state".to_string());
        }
        Ok(())
    }

    /// Accept a pending handoff. Only the agent the issue was handed off to
    /// may accept. On success the claimant becomes the new agent and status
    /// returns to `active`.
    pub fn accept_handoff(issue_id: &str, agent_id: &str) -> Result<(), String> {
        let _guard = LockGuard::acquire(STATE_NAME)
            .ok_or_else(|| "claim-service lock contention".to_string())?;
        let mut state = read_state(STATE_NAME);
        let arr = claims_array_mut(&mut state)?;
        let entry = arr
            .iter_mut()
            .find(|c| c["issueId"].as_str() == Some(issue_id))
            .ok_or_else(|| format!("issue `{issue_id}` not found"))?;
        if entry["status"].as_str() != Some("handoff_pending") {
            return Err(format!("issue `{issue_id}` is not pending handoff"));
        }
        if entry["pendingHandoffTo"].as_str() != Some(agent_id) {
            return Err(format!(
                "issue `{issue_id}` handoff is not intended for `{agent_id}`"
            ));
        }
        let old_claimant = entry["claimant"]["id"].as_str().unwrap_or("").to_string();
        let old_type = entry["claimant"]["type"].clone();
        entry["claimant"] = json!({"id": agent_id, "type": old_type});
        entry["status"] = json!("active");
        if let Some(obj) = entry.as_object_mut() {
            obj.remove("pendingHandoffTo");
        }
        push_history(
            entry,
            json!({
                "event": "handoff_accepted",
                "at": now_ms(),
                "from": old_claimant,
                "to": agent_id,
            }),
        );
        if !write_state(STATE_NAME, &state) {
            return Err("failed to write claim-service state".to_string());
        }
        Ok(())
    }

    /// Mark an actively-claimed issue as available for theft by another agent
    /// (e.g. the claimant is overloaded or stale). Status becomes `stealable`.
    pub fn mark_stealable(issue_id: &str, reason: &str) -> Result<(), String> {
        let _guard = LockGuard::acquire(STATE_NAME)
            .ok_or_else(|| "claim-service lock contention".to_string())?;
        let mut state = read_state(STATE_NAME);
        let arr = claims_array_mut(&mut state)?;
        let entry = arr
            .iter_mut()
            .find(|c| c["issueId"].as_str() == Some(issue_id))
            .ok_or_else(|| format!("issue `{issue_id}` not found"))?;
        entry["status"] = json!("stealable");
        push_history(
            entry,
            json!({"event": "marked_stealable", "at": now_ms(), "reason": reason}),
        );
        if !write_state(STATE_NAME, &state) {
            return Err("failed to write claim-service state".to_string());
        }
        Ok(())
    }

    /// List all stealable issues (status == `stealable`). Optionally filtered
    /// by preferred agent type. Drives the swarm work-stealing path.
    pub fn stealable(preferred_type: Option<&str>) -> Result<Vec<Value>, String> {
        let state = read_state(STATE_NAME);
        let arr = state["claims"].as_array().cloned().unwrap_or_default();
        let filtered: Vec<Value> = arr
            .into_iter()
            .filter(|c| c["status"].as_str() == Some("stealable"))
            .filter(|c| match preferred_type {
                Some(t) => c["preferredTypes"].as_array()
                    .map(|a| a.iter().any(|x| x.as_str() == Some(t)))
                    .unwrap_or(true),
                None => true,
            })
            .collect();
        Ok(filtered)
    }

    /// Steal a stealable issue. The claimant becomes `stealer_agent_id` and
    /// status becomes `stolen` (terminal — must be re-claimed after release).
    pub fn steal(
        issue_id: &str,
        stealer_agent_id: &str,
        stealer_agent_type: &str,
    ) -> Result<(), String> {
        let _guard = LockGuard::acquire(STATE_NAME)
            .ok_or_else(|| "claim-service lock contention".to_string())?;
        let mut state = read_state(STATE_NAME);
        let arr = claims_array_mut(&mut state)?;
        let entry = arr
            .iter_mut()
            .find(|c| c["issueId"].as_str() == Some(issue_id))
            .ok_or_else(|| format!("issue `{issue_id}` not found"))?;
        if entry["status"].as_str() != Some("stealable") {
            return Err(format!(
                "issue `{issue_id}` is not stealable (status: {})",
                entry["status"].as_str().unwrap_or("?")
            ));
        }
        let old_claimant = entry["claimant"]["id"].as_str().unwrap_or("").to_string();
        entry["claimant"] = json!({"id": stealer_agent_id, "type": stealer_agent_type});
        entry["status"] = json!("stolen");
        push_history(
            entry,
            json!({
                "event": "stolen",
                "at": now_ms(),
                "from": old_claimant,
                "by": stealer_agent_id,
            }),
        );
        if !write_state(STATE_NAME, &state) {
            return Err("failed to write claim-service state".to_string());
        }
        Ok(())
    }

    /// Load the full claim status list — every claim (active or otherwise).
    pub fn load_status() -> Vec<Value> {
        read_state(STATE_NAME)
            .as_array()
            .cloned()
            .unwrap_or_default()
    }
