//! Shared fail-closed spend gate for every AI spawn path.

use std::path::Path;

use ruflo_storage::SpendLedger;

pub fn check() -> Result<(), String> {
    SpendLedger::open_default()
        .map_err(|e| e.to_string())?
        .check()
}

pub fn reserve(kind: &str, worker_type: &str, workspace: &Path) -> Result<String, String> {
    SpendLedger::open_default()
        .map_err(|e| e.to_string())?
        .reserve(kind, worker_type, &workspace.display().to_string())
}

pub fn release(permit: &str) {
    if let Ok(ledger) = SpendLedger::open_default() {
        let _ = ledger.release(permit);
    }
}

pub fn pause(reason: &str) -> Result<(), String> {
    SpendLedger::open_default()
        .map_err(|e| e.to_string())?
        .pause(reason)
}

pub fn resume() -> Result<(), String> {
    SpendLedger::open_default()
        .map_err(|e| e.to_string())?
        .resume()
}

pub fn is_paused() -> bool {
    SpendLedger::open_default()
        .map(|l| l.is_paused())
        .unwrap_or(true)
}

pub struct PermitGuard(Option<String>);

impl PermitGuard {
    pub fn new(permit: Option<String>) -> Self {
        Self(permit)
    }
}

impl Drop for PermitGuard {
    fn drop(&mut self) {
        if let Some(p) = self.0.take() {
            release(&p);
        }
    }
}
