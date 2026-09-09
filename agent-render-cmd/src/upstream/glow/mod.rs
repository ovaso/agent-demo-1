//! Rust port of Glow's automatic width policy in main.go::validateOptions.
//! Source: charmbracelet/glow@7b2431d4a82428fb477eb4361e11583e1644e9ba.
//! Copyright (c) 2019-2024 Charmbracelet, Inc. MIT; see LICENSES/glow-MIT.txt.
//! Adaptation: the caller supplies terminal dimensions; no Go CLI, env or TTY code.

pub(crate) fn auto_width(terminal_columns: Option<usize>) -> usize {
    match terminal_columns {
        Some(width) if width > 0 => width.min(120),
        _ => 80,
    }
}
