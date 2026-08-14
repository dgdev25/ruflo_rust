//! Auto-split from services.rs
use super::*;

    use super::*;
    pub fn record_story(name: &str, steps: Vec<String>) -> Value {
        let mut state = read_state("fable-harness");
        let story = json!({"name": name, "steps": steps, "recordedAt": now_ms()});
        ensure_arr(&mut state, "stories").push(story.clone());
        write_state("fable-harness", &state);
        story
    }
    pub fn list() -> Vec<Value> {
        read_state("fable-harness")["stories"].as_array().cloned().unwrap_or_default()
    }
