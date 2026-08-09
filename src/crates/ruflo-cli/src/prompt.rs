//! Interactive prompts — mirrors TS `prompt.ts` using `dialoguer`.
//!
//! Provides select, input, confirm, and multi-select prompts for the init
//! wizard, interactive config, swarm topology selection, etc. All prompts
//! degrade gracefully in non-TTY environments (return default values).

use dialoguer::{Confirm, Input, MultiSelect, Select};

/// Check if stdin is a TTY (interactive). If not, prompts are skipped and
/// defaults are returned — matching TS behavior when not interactive.
pub fn is_interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

use std::io::IsTerminal;

/// Show a selection prompt and return the chosen index. Returns `default_idx`
/// if non-interactive.
pub fn select(prompt: &str, items: &[&str], default_idx: usize) -> usize {
    if !is_interactive() {
        return default_idx;
    }
    Select::new()
        .with_prompt(prompt)
        .items(items)
        .default(default_idx)
        .interact_opt()
        .ok()
        .flatten()
        .unwrap_or(default_idx)
}

/// Show a text input prompt. Returns `default_val` if non-interactive.
pub fn input(prompt: &str, default_val: &str) -> String {
    if !is_interactive() {
        return default_val.to_string();
    }
    Input::new()
        .with_prompt(prompt)
        .default(default_val.to_string())
        .interact_text()
        .ok()
        .unwrap_or(default_val.to_string())
}

/// Show a yes/no confirm. Returns `default_val` if non-interactive.
pub fn confirm(prompt: &str, default_val: bool) -> bool {
    if !is_interactive() {
        return default_val;
    }
    Confirm::new()
        .with_prompt(prompt)
        .default(default_val)
        .interact_opt()
        .ok()
        .flatten()
        .unwrap_or(default_val)
}

/// Show a multi-select. Returns indices of chosen items. Returns all if
/// non-interactive.
pub fn multi_select(prompt: &str, items: &[&str], defaults: &[bool]) -> Vec<usize> {
    if !is_interactive() {
        return defaults
            .iter()
            .enumerate()
            .filter(|(_, &d)| d)
            .map(|(i, _)| i)
            .collect();
    }
    MultiSelect::new()
        .with_prompt(prompt)
        .items(items)
        .defaults(defaults)
        .interact_opt()
        .ok()
        .flatten()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_interactive_returns_defaults() {
        // When piped (non-TTY), all prompts return defaults without blocking.
        // This test runs under cargo test (piped stdin) so is_interactive()
        // returns false.
        assert!(!is_interactive());
        assert_eq!(select("pick", &["a", "b"], 1), 1);
        assert_eq!(input("name", "default"), "default");
        assert!(!confirm("sure?", false));
    }
}
