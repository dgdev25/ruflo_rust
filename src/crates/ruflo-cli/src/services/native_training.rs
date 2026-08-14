//! Auto-split from services.rs
use super::*;

    use super::*;
    pub fn record_checkpoint(model: &str, epoch: usize, loss: f64) -> Value {
        let mut state = read_state("native-training");
        let ckpt = json!({"model": model, "epoch": epoch, "loss": loss, "savedAt": now_ms()});
        ensure_arr(&mut state, "checkpoints").push(ckpt.clone());
        write_state("native-training", &state);
        ckpt
    }
    pub fn checkpoints() -> Vec<Value> {
        read_state("native-training")["checkpoints"].as_array().cloned().unwrap_or_default()
    }
