//! Auto-split from services.rs
use super::*;

    use super::*;
    pub fn run_training(epochs: usize) -> Value {
        // Delegate to the native distillation pipeline (SONA MLP + EWC++).
        let result = crate::distillation::run(384, 64, epochs);
        let entry = json!({
            "pipeline": "native-sona-ewc++", "epochs": epochs,
            "result": result, "at": now_ms(),
        });
        let mut state = read_state("ruvector-training");
        ensure_arr(&mut state, "sessions").push(entry.clone());
        write_state("ruvector-training", &state);
        entry
    }
