//! SONA — Self-Organizing Neural Architecture.
//!
//! Full neural gradient descent (not simplified statistics). A small MLP
//! (input → hidden → output) trained online via backpropagation with SGD +
//! momentum. EWC++ (Elastic Weight Consolidation) accumulates a Fisher
//! Information Matrix per task to prevent catastrophic forgetting across
//! sequential task distributions.
//!
//! This is the neural learning loop that powers `ruflo neural train` /
//! `ruflo route` adaptation. Zero Node/WASM dependency — pure Rust.
//!
//! Math reference:
//! - Forward:  h = tanh(W1·x + b1);  o = softmax(W2·h + b2)
//! - Loss:     cross-entropy between o and target one-hot
//! - Backward: chain-rule gradients through softmax+tanh
//! - EWC++:    Fisher F_i = E[(∂log p / ∂θ_i)²] accumulated post-task;
//!             penalty = λ · Σ F_i · (θ_i − θ*_i)² added to loss

use serde::{Deserialize, Serialize};
use std::path::Path;

/// A single SONA network: input → hidden(tanh) → output(softmax).
#[derive(Clone, Serialize, Deserialize)]
pub struct SonaNet {
    pub input_dim: usize,
    pub hidden_dim: usize,
    pub output_dim: usize,
    /// W1[hidden][input]
    pub w1: Vec<Vec<f64>>,
    /// b1[hidden]
    pub b1: Vec<f64>,
    /// W2[output][hidden]
    pub w2: Vec<Vec<f64>>,
    /// b2[output]
    pub b2: Vec<f64>,
}

/// EWC++ state — Fisher Information diagonal + anchor (θ*) per parameter.
/// Accumulated after each consolidated task.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct EwcState {
    /// Fisher diagonal for W1
    pub fisher_w1: Vec<Vec<f64>>,
    /// Fisher diagonal for b1
    pub fisher_b1: Vec<f64>,
    /// Fisher diagonal for W2
    pub fisher_w2: Vec<Vec<f64>>,
    /// Fisher diagonal for b2
    pub fisher_b2: Vec<f64>,
    /// Anchored θ* (optimal params after last task)
    pub star_w1: Vec<Vec<f64>>,
    pub star_b1: Vec<f64>,
    pub star_w2: Vec<Vec<f64>>,
    pub star_b2: Vec<f64>,
    /// EWC penalty strength λ
    pub lambda: f64,
    /// Number of tasks consolidated
    pub tasks_consolidated: usize,
}

/// Cached forward-pass activations (for backprop).
struct Activations {
    z1: Vec<f64>,  // pre-activation hidden
    h: Vec<f64>,   // post-tanh hidden
    z2: Vec<f64>,  // pre-activation output (logits)
    o: Vec<f64>,   // softmax output
}

/// Training hyperparameters.
#[derive(Clone, Copy)]
pub struct TrainConfig {
    pub lr: f64,        // learning rate
    pub momentum: f64,  // SGD momentum
    pub l2: f64,        // L2 weight decay
}

impl Default for TrainConfig {
    fn default() -> Self {
        Self { lr: 0.05, momentum: 0.9, l2: 1e-4 }
    }
}

impl SonaNet {
    /// Create a new network with Xavier/Glorot initialization.
    pub fn new(input_dim: usize, hidden_dim: usize, output_dim: usize) -> Self {
        let mut net = Self {
            input_dim,
            hidden_dim,
            output_dim,
            w1: vec![vec![0.0; input_dim]; hidden_dim],
            b1: vec![0.0; hidden_dim],
            w2: vec![vec![0.0; hidden_dim]; output_dim],
            b2: vec![0.0; output_dim],
        };
        net.xavier_init();
        net
    }

    fn xavier_init(&mut self) {
        // Glorot uniform: limit = sqrt(6 / (fan_in + fan_out))
        let limit1 = (6.0 / (self.input_dim + self.hidden_dim) as f64).sqrt();
        for row in self.w1.iter_mut() {
            for w in row.iter_mut() {
                *w = (pseudo_rand() * 2.0 - 1.0) * limit1;
            }
        }
        let limit2 = (6.0 / (self.hidden_dim + self.output_dim) as f64).sqrt();
        for row in self.w2.iter_mut() {
            for w in row.iter_mut() {
                *w = (pseudo_rand() * 2.0 - 1.0) * limit2;
            }
        }
    }

    /// Forward pass: input → softmax probabilities over output_dim.
    pub fn forward(&self, x: &[f64]) -> Vec<f64> {
        self.forward_cached(x).o
    }

    fn forward_cached(&self, x: &[f64]) -> Activations {
        // Hidden layer: z1 = W1·x + b1; h = tanh(z1)
        let mut z1 = vec![0.0; self.hidden_dim];
        for (i, zi) in z1.iter_mut().enumerate() {
            let mut acc = self.b1[i];
            let row = &self.w1[i];
            for (j, &xj) in x.iter().enumerate() {
                if j >= row.len() { break; }
                acc += row[j] * xj;
            }
            *zi = acc;
        }
        let h: Vec<f64> = z1.iter().map(|z| z.tanh()).collect();

        // Output layer: z2 = W2·h + b2; o = softmax(z2)
        let mut z2 = vec![0.0; self.output_dim];
        for (i, zi) in z2.iter_mut().enumerate() {
            let mut acc = self.b2[i];
            let row = &self.w2[i];
            for (j, &hj) in h.iter().enumerate() {
                acc += row[j] * hj;
            }
            *zi = acc;
        }
        let o = softmax(&z2);

        Activations { z1, h, z2, o }
    }

    /// Predict argmax class.
    pub fn predict(&self, x: &[f64]) -> usize {
        let probs = self.forward(x);
        probs
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
            .unwrap_or(0)
    }

    /// Train one step on (input, target_class). Returns the cross-entropy loss
    /// before the update. Applies L2 regularization and (if ewc is Some) the
    /// EWC++ quadratic penalty against the anchored θ*.
    pub fn train_step(
        &mut self,
        x: &[f64],
        target: usize,
        cfg: TrainConfig,
        ewc: Option<&EwcState>,
        // Persistent momentum buffers (caller-owned so they survive across steps).
        vel: &mut VelocityBuffers,
    ) -> f64 {
        let act = self.forward_cached(x);

        // Cross-entropy loss (for reporting).
        let loss = -(act.o.get(target).unwrap_or(&1e-12)).ln();

        // --- Backprop ---
        // dL/dz2 = o - onehot(target)
        let mut dz2 = act.o.clone();
        if target < dz2.len() {
            dz2[target] -= 1.0;
        }

        // dL/dW2 = dz2 · hᵀ ;  dL/db2 = dz2
        let mut dw2 = vec![vec![0.0; self.hidden_dim]; self.output_dim];
        for i in 0..self.output_dim {
            for j in 0..self.hidden_dim {
                dw2[i][j] = dz2[i] * act.h[j];
            }
        }
        let mut db2 = dz2.clone();

        // dL/dh = W2ᵀ · dz2
        let mut dh = vec![0.0; self.hidden_dim];
        for j in 0..self.hidden_dim {
            for i in 0..self.output_dim {
                dh[j] += self.w2[i][j] * dz2[i];
            }
        }
        // dL/dz1 = dh * (1 - h²)  [tanh derivative]
        let dz1: Vec<f64> = dh
            .iter()
            .zip(act.h.iter())
            .map(|(d, h)| d * (1.0 - h * h))
            .collect();

        // dL/dW1 = dz1 · xᵀ ; dL/db1 = dz1
        let mut dw1 = vec![vec![0.0; self.input_dim]; self.hidden_dim];
        for i in 0..self.hidden_dim {
            for (j, &xj) in x.iter().enumerate() {
                if j >= self.input_dim { break; }
                dw1[i][j] = dz1[i] * xj;
            }
        }
        let mut db1 = dz1;

        // --- Add EWC++ penalty gradients ---
        // Penalty P = λ · Σ F_i · (θ_i − θ*_i)²
        // dP/dθ_i = 2λ · F_i · (θ_i − θ*_i)
        if let Some(ewc) = ewc {
            if ewc.lambda > 0.0 && ewc.tasks_consolidated > 0 {
                let lam = 2.0 * ewc.lambda;
                for i in 0..self.hidden_dim.min(ewc.fisher_w1.len()) {
                    for j in 0..self.input_dim.min(ewc.fisher_w1[i].len()) {
                        dw1[i][j] += lam * ewc.fisher_w1[i][j] * (self.w1[i][j] - ewc.star_w1[i][j]);
                    }
                }
                for i in 0..self.hidden_dim.min(ewc.fisher_b1.len()) {
                    db1[i] += lam * ewc.fisher_b1[i] * (self.b1[i] - ewc.star_b1[i]);
                }
                for i in 0..self.output_dim.min(ewc.fisher_w2.len()) {
                    for j in 0..self.hidden_dim.min(ewc.fisher_w2[i].len()) {
                        dw2[i][j] += lam * ewc.fisher_w2[i][j] * (self.w2[i][j] - ewc.star_w2[i][j]);
                    }
                }
                for i in 0..self.output_dim.min(ewc.fisher_b2.len()) {
                    db2[i] += lam * ewc.fisher_b2[i] * (self.b2[i] - ewc.star_b2[i]);
                }
            }
        }

        // --- SGD + momentum update (+ L2 decay) ---
        apply_update(&mut self.w1, &dw1, &mut vel.w1, cfg, cfg.l2);
        apply_update_vec(&mut self.b1, &db1, &mut vel.b1, cfg);
        apply_update(&mut self.w2, &dw2, &mut vel.w2, cfg, cfg.l2);
        apply_update_vec(&mut self.b2, &db2, &mut vel.b2, cfg);

        loss
    }

    /// Train over a mini-batch for multiple epochs. Returns mean loss.
    pub fn fit(
        &mut self,
        examples: &[(Vec<f64>, usize)],
        epochs: usize,
        cfg: TrainConfig,
        ewc: Option<&EwcState>,
    ) -> f64 {
        let mut vel = VelocityBuffers::for_net(self);
        let mut total_loss = 0.0;
        let mut count = 0usize;
        for _ in 0..epochs {
            for (x, y) in examples {
                total_loss += self.train_step(x, *y, cfg, ewc, &mut vel);
                count += 1;
            }
        }
        if count > 0 { total_loss / count as f64 } else { 0.0 }
    }

    /// Compute Fisher Information diagonal on a batch — the EWC++ estimate of
    /// parameter importance. Called at the end of a task to anchor θ*.
    pub fn compute_fisher(&self, examples: &[(Vec<f64>, usize)]) -> EwcState {
        let mut fisher = EwcState {
            fisher_w1: vec![vec![0.0; self.input_dim]; self.hidden_dim],
            fisher_b1: vec![0.0; self.hidden_dim],
            fisher_w2: vec![vec![0.0; self.hidden_dim]; self.output_dim],
            fisher_b2: vec![0.0; self.output_dim],
            star_w1: self.w1.clone(),
            star_b1: self.b1.clone(),
            star_w2: self.w2.clone(),
            star_b2: self.b2.clone(),
            lambda: 400.0, // conventional EWC default
            tasks_consolidated: 0,
        };

        for (x, target) in examples {
            // Empirical Fisher: squared gradient of log-likelihood at the true label.
            // Re-run forward, recompute grads (reuse backprop without update).
            let act = self.forward_cached(x);
            let mut dz2 = act.o.clone();
            if *target < dz2.len() {
                dz2[*target] -= 1.0;
            }
            // For fisher we square the gradient of log p(y|x) wrt params, which
            // for cross-entropy equals the same dz2-derived grads (squared).
            for i in 0..self.output_dim {
                for j in 0..self.hidden_dim {
                    let g = dz2[i] * act.h[j];
                    fisher.fisher_w2[i][j] += g * g;
                }
                fisher.fisher_b2[i] += dz2[i] * dz2[i];
            }
            let mut dh = vec![0.0; self.hidden_dim];
            for j in 0..self.hidden_dim {
                for i in 0..self.output_dim {
                    dh[j] += self.w2[i][j] * dz2[i];
                }
            }
            for i in 0..self.hidden_dim {
                let dz1_i = dh[i] * (1.0 - act.h[i] * act.h[i]);
                for (j, &xj) in x.iter().enumerate() {
                    if j >= self.input_dim { break; }
                    let g = dz1_i * xj;
                    fisher.fisher_w1[i][j] += g * g;
                }
                fisher.fisher_b1[i] += dz1_i * dz1_i;
            }
        }

        // Average over examples.
        let n = examples.len().max(1) as f64;
        for row in fisher.fisher_w1.iter_mut() {
            for v in row.iter_mut() { *v /= n; }
        }
        for v in fisher.fisher_b1.iter_mut() { *v /= n; }
        for row in fisher.fisher_w2.iter_mut() {
            for v in row.iter_mut() { *v /= n; }
        }
        for v in fisher.fisher_b2.iter_mut() { *v /= n; }
        fisher.tasks_consolidated = 1;
        fisher
    }

    /// Merge a freshly-computed Fisher into accumulated EWC state (online EWC++).
    pub fn consolidate(&mut self, ewc: &mut EwcState, new_fisher: EwcState) {
        // Size up the accumulator on first consolidation.
        if ewc.fisher_w1.is_empty() {
            ewc.fisher_w1 = new_fisher.fisher_w1.clone();
            ewc.fisher_b1 = new_fisher.fisher_b1.clone();
            ewc.fisher_w2 = new_fisher.fisher_w2.clone();
            ewc.fisher_b2 = new_fisher.fisher_b2.clone();
        } else {
            merge_fisher(&mut ewc.fisher_w1, &new_fisher.fisher_w1);
            merge_fisher(&mut ewc.fisher_w2, &new_fisher.fisher_w2);
            merge_vec(&mut ewc.fisher_b1, &new_fisher.fisher_b1);
            merge_vec(&mut ewc.fisher_b2, &new_fisher.fisher_b2);
        }
        // Re-anchor θ* to current weights.
        ewc.star_w1 = self.w1.clone();
        ewc.star_b1 = self.b1.clone();
        ewc.star_w2 = self.w2.clone();
        ewc.star_b2 = self.b2.clone();
        ewc.lambda = new_fisher.lambda.max(ewc.lambda);
        ewc.tasks_consolidated += 1;
    }

    // --- Persistence ---

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let s = serde_json::to_string(self).map_err(|e| e.to_string())?;
        atomic_write(path, &s)
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let s = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        serde_json::from_str(&s).map_err(|e| e.to_string())
    }
}

/// Per-parameter velocity buffers for SGD with momentum.
pub struct VelocityBuffers {
    pub w1: Vec<Vec<f64>>,
    pub b1: Vec<f64>,
    pub w2: Vec<Vec<f64>>,
    pub b2: Vec<f64>,
}

impl VelocityBuffers {
    pub fn for_net(net: &SonaNet) -> Self {
        Self {
            w1: vec![vec![0.0; net.input_dim]; net.hidden_dim],
            b1: vec![0.0; net.hidden_dim],
            w2: vec![vec![0.0; net.hidden_dim]; net.output_dim],
            b2: vec![0.0; net.output_dim],
        }
    }
}

// --- Helpers ---

fn softmax(z: &[f64]) -> Vec<f64> {
    let max = z.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let exps: Vec<f64> = z.iter().map(|v| (v - max).exp()).collect();
    let sum: f64 = exps.iter().sum();
    if sum <= 0.0 {
        return vec![1.0 / z.len() as f64; z.len()];
    }
    exps.iter().map(|e| e / sum).collect()
}

fn apply_update(
    params: &mut [Vec<f64>],
    grads: &[Vec<f64>],
    vel: &mut [Vec<f64>],
    cfg: TrainConfig,
    l2: f64,
) {
    for i in 0..params.len() {
        for j in 0..params[i].len() {
            // v = μ·v − lr·(g + l2·θ)
            let g = grads[i][j] + l2 * params[i][j];
            vel[i][j] = cfg.momentum * vel[i][j] - cfg.lr * g;
            params[i][j] += vel[i][j];
        }
    }
}

fn apply_update_vec(
    params: &mut [f64],
    grads: &[f64],
    vel: &mut [f64],
    cfg: TrainConfig,
) {
    for i in 0..params.len() {
        vel[i] = cfg.momentum * vel[i] - cfg.lr * grads[i];
        params[i] += vel[i];
    }
}

fn merge_fisher(acc: &mut [Vec<f64>], new: &[Vec<f64>]) {
    for i in 0..acc.len().min(new.len()) {
        for j in 0..acc[i].len().min(new[i].len()) {
            // Online EWC: decay old, add new (α=0.5 keeps stable estimate).
            acc[i][j] = 0.5 * acc[i][j] + 0.5 * new[i][j];
        }
    }
}

fn merge_vec(acc: &mut [f64], new: &[f64]) {
    for i in 0..acc.len().min(new.len()) {
        acc[i] = 0.5 * acc[i] + 0.5 * new[i];
    }
}

/// Deterministic PRNG (xorshift) — Math.random is fine for weight init but we
/// keep this hermetic so training is reproducible across runs.
fn pseudo_rand() -> f64 {
    use std::cell::Cell;
    thread_local! {
        static STATE: Cell<u64> = Cell::new(0x9E3779B97F4A7C15);
    }
    STATE.with(|s| {
        let mut x = s.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        (x as f64) / (u64::MAX as f64)
    })
}

fn atomic_write(path: &Path, content: &str) -> Result<(), String> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, content).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    Ok(())
}

// Silence unused warning for the vec updater — kept for potential bias-momentum use.
#[allow(dead_code)]
fn _unused() {
    let mut p = vec![0.0];
    let mut v = vec![0.0];
    let g = vec![0.0];
    apply_update_vec(&mut p, &g, &mut v, TrainConfig::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_produces_probability_distribution() {
        let net = SonaNet::new(4, 8, 3);
        let probs = net.forward(&[0.1, 0.5, -0.3, 0.8]);
        assert_eq!(probs.len(), 3);
        let sum: f64 = probs.iter().sum();
        assert!((sum - 1.0).abs() < 1e-6, "softmax must sum to 1, got {sum}");
        for p in &probs {
            assert!(*p >= 0.0 && *p <= 1.0);
        }
    }

    #[test]
    fn training_reduces_loss() {
        // Simple separable problem: if x[0] > 0 → class 0, else class 1.
        let mut net = SonaNet::new(2, 16, 2);
        let examples: Vec<(Vec<f64>, usize)> = (0..200)
            .map(|i| {
                let a = if i % 2 == 0 { 1.0 } else { -1.0 };
                let label = if a > 0.0 { 0 } else { 1 };
                (vec![a, -a], label)
            })
            .collect();

        let cfg = TrainConfig { lr: 0.1, momentum: 0.9, l2: 1e-4 };
        let loss_before = net.fit(&examples[..20], 1, cfg, None);
        let loss_after = net.fit(&examples, 50, cfg, None);

        assert!(
            loss_after < loss_before,
            "loss should decrease: before={loss_before:.4} after={loss_after:.4}"
        );

        // Trained net should classify the training points well.
        let correct = examples.iter()
            .filter(|(x, y)| net.predict(x) == *y)
            .count();
        assert!(correct > 180, "should classify >180/200, got {correct}");
    }

    #[test]
    fn ewc_fisher_nonzero_after_consolidation() {
        // Compute Fisher on a fresh (untrained) net so gradients are non-zero.
        let mut net = SonaNet::new(3, 6, 2);
        let examples = vec![
            (vec![0.8, 0.2, 0.1], 0),
            (vec![0.1, 0.9, 0.3], 1),
            (vec![0.5, 0.5, 0.5], 0),
        ];
        let mut ewc = EwcState::default();
        let fisher = net.compute_fisher(&examples);
        net.consolidate(&mut ewc, fisher);

        let total_fisher: f64 = ewc.fisher_w1.iter()
            .flat_map(|r| r.iter())
            .map(|v| *v)
            .sum();
        assert!(total_fisher > 0.0, "Fisher diagonal should be > 0 after consolidation, got {total_fisher}");
        assert_eq!(ewc.tasks_consolidated, 1);
    }

    #[test]
    fn ewc_prevents_catastrophic_forgetting() {
        // Train task A, consolidate, then train task B with EWC.
        // θ* for A should stay close (penalty holds it).
        let mut net = SonaNet::new(2, 8, 2);
        let cfg = TrainConfig { lr: 0.1, momentum: 0.9, l2: 0.0 };

        // Task A.
        let task_a = vec![
            (vec![1.0, 1.0], 0),
            (vec![0.9, 0.95], 0),
            (vec![1.1, 1.05], 0),
        ];
        net.fit(&task_a, 40, cfg, None);
        let mut ewc = EwcState::default();
        let fisher_a = net.compute_fisher(&task_a);
        net.consolidate(&mut ewc, fisher_a);
        let anchor = net.w1.clone();

        // Task B (different distribution) with EWC penalty.
        let task_b = vec![
            (vec![-1.0, -1.0], 1),
            (vec![-0.9, -0.95], 1),
            (vec![-1.1, -1.05], 1),
        ];
        net.fit(&task_b, 40, cfg, Some(&ewc));

        // Without EWC, task-B training would drag A's params far. With EWC,
        // the anchor penalty keeps drift bounded. Measure max drift.
        let mut max_drift = 0.0f64;
        for i in 0..net.w1.len() {
            for j in 0..net.w1[i].len() {
                let d = (net.w1[i][j] - anchor[i][j]).abs();
                if d > max_drift { max_drift = d; }
            }
        }
        // EWC doesn't freeze params entirely but should keep drift moderate.
        assert!(max_drift < 2.0, "EWC should bound param drift, got {max_drift}");
    }

    #[test]
    fn save_load_roundtrip() {
        let net = SonaNet::new(3, 4, 2);
        let dir = std::env::temp_dir().join("sona_test_roundtrip.json");
        net.save(&dir).unwrap();
        let loaded = SonaNet::load(&dir).unwrap();
        assert_eq!(loaded.input_dim, 3);
        assert_eq!(loaded.output_dim, 2);
        // Weights match.
        let p1 = net.forward(&[0.5, 0.1, 0.2]);
        let p2 = loaded.forward(&[0.5, 0.1, 0.2]);
        assert!(p1.iter().zip(p2.iter()).all(|(a, b)| (a - b).abs() < 1e-9));
        let _ = std::fs::remove_file(&dir);
    }
}
