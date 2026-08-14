//! Auto-split from services.rs
use super::*;

    use super::*;
    pub fn record_champion(fitness: f64, generation: usize, surface: &str) -> Value {
        let mut state = read_state("evolve-proof");
        let champ = json!({"fitness": fitness, "generation": generation, "surface": surface, "at": now_ms()});
        ensure_arr(&mut state, "champions").push(champ.clone());
        write_state("evolve-proof", &state);
        champ
    }
    pub fn champions() -> Vec<Value> {
        read_state("evolve-proof")["champions"].as_array().cloned().unwrap_or_default()
    }
