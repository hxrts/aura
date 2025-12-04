//! Simple recovery status formatter for the CLI.
//!
//! This module drives the `recovery status` command output. Keeping the CLI channel
//! minimal means we only summarize facts available in the journal rather than
//! recreating the richer TUI visualization.

use std::fmt::Write;

/// Format a recovery status report from journal fact keys.
pub fn format_recovery_status(active: &[String], completed: &[String]) -> String {
    let mut output = String::new();

    if active.is_empty() {
        let _ = writeln!(output, "No active recovery sessions found.");
    } else {
        let _ = writeln!(output, "Found {} active recovery session(s):", active.len());
        for (idx, key) in active.iter().enumerate() {
            let _ = writeln!(output, "  {}. {}", idx + 1, key);
        }
    }

    if !completed.is_empty() {
        let _ = writeln!(output, "Completed recovery sessions ({}):", completed.len());
        for key in completed {
            let _ = writeln!(output, "  - {}", key);
        }
    }

    output
}
