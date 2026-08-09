//! Shared output formatting helpers — mirrors TS `output.ts`.
//!
//! Provides print_box, print_table, print_list, ANSI color helpers, and a
//! simple spinner. All ANSI is gated by NO_COLOR / non-TTY detection, matching
//! the TS output module's behavior. Used across command modules to reduce
//! per-module duplication and align formatting with the V3 reference.

use std::io::IsTerminal;

/// True if ANSI color should be emitted (respects NO_COLOR, non-TTY).
pub fn color_enabled() -> bool {
    std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal()
}

fn paint(code: &str, text: &str) -> String {
    if color_enabled() {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

pub fn bold(text: &str) -> String { paint("1", text) }
pub fn dim(text: &str) -> String { paint("2", text) }
pub fn red(text: &str) -> String { paint("31", text) }
pub fn green(text: &str) -> String { paint("32", text) }
pub fn yellow(text: &str) -> String { paint("33", text) }
pub fn blue(text: &str) -> String { paint("34", text) }
pub fn cyan(text: &str) -> String { paint("36", text) }

/// Print a box with a title and multi-line content.
pub fn print_box(lines: &[&str], title: &str) {
    let all: Vec<&str> = std::iter::once(title).chain(lines.iter().copied()).collect();
    let max_len = all.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let width = max_len + 4;
    let top = format!("\u{256d}{} \u{256e}", "\u{2500}".repeat(width.saturating_sub(title.chars().count() + 2)));
    let bottom = format!("\u{2570}{} \u{256f}", "\u{2500}".repeat(width.saturating_sub(title.chars().count() + 2)));
    println!("{top} {title}");
    for line in lines {
        let pad = width.saturating_sub(line.chars().count() + 2);
        println!("\u{2502} {line}{} \u{2502}", " ".repeat(pad));
    }
    println!("{bottom}");
}

/// Print a simple table with headers and rows (char-width aware).
pub fn print_table(headers: &[&str], rows: &[Vec<String>]) {
    let n_cols = headers.len();
    let mut widths = vec![0usize; n_cols];
    for (i, h) in headers.iter().enumerate() {
        widths[i] = h.chars().count();
    }
    for row in rows {
        for (i, cell) in row.iter().take(n_cols).enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }
    // Header
    let hdr: Vec<String> = headers.iter().enumerate()
        .map(|(i, h)| format!("{:<w$}", h, w = widths[i]))
        .collect();
    println!("  {}", hdr.join("  "));
    let sep: Vec<String> = widths.iter().map(|w| "\u{2500}".repeat(*w)).collect();
    println!("  {}", sep.join("  "));
    // Rows
    for row in rows {
        let cells: Vec<String> = row.iter().take(n_cols).enumerate()
            .map(|(i, c)| format!("{:<w$}", c, w = widths[i]))
            .collect();
        println!("  {}", cells.join("  "));
    }
}

/// Print a bulleted list (`  - item`).
pub fn print_list(items: &[&str]) {
    for item in items {
        println!("  {dim}{item}{reset}",
            dim = if color_enabled() { "\x1b[2m" } else { "" },
            reset = if color_enabled() { "\x1b[0m" } else { "" });
    }
}

/// Print a numbered list.
pub fn print_numbered(items: &[&str]) {
    for (i, item) in items.iter().enumerate() {
        println!("  {}. {item}", i + 1);
    }
}

/// A simple spinner that prints a message and overwrites it. Non-ANSI
/// terminals just print the message once.
pub struct Spinner {
    msg: String,
    started: bool,
}

impl Spinner {
    pub fn new(msg: impl Into<String>) -> Self {
        Self { msg: msg.into(), started: false }
    }

    pub fn start(&mut self) {
        if color_enabled() {
            eprint!("\u{2299} {}...\r", self.msg);
            self.started = true;
        } else {
            eprintln!("{}...", self.msg);
        }
    }

    pub fn succeed(&mut self, msg: &str) {
        if self.started {
            eprint!("\u{2714} {}\n", msg);
        } else {
            eprintln!("\u{2714} {msg}");
        }
    }

    pub fn fail(&mut self, msg: &str) {
        if self.started {
            eprint!("\u{2718} {}\n", msg);
        } else {
            eprintln!("\u{2718} {msg}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_distance_basic() {
        // (not here — just testing color gating + table)
        assert!(!bold("x").is_empty());
    }

    #[test]
    fn color_disabled_under_no_color() {
        std::env::set_var("NO_COLOR", "1");
        assert!(!color_enabled());
        assert_eq!(bold("hello"), "hello");
        std::env::remove_var("NO_COLOR");
    }

    #[test]
    fn table_formats() {
        // Just verify it doesn't panic.
        print_table(&["A", "B"], &[
            vec!["1".into(), "2".into()],
            vec!["longer".into(), "x".into()],
        ]);
    }
}
