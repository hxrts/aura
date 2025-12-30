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
            let _ = writeln!(output, "  - {key}");
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_recovery_status_no_sessions() {
        let result = format_recovery_status(&[], &[]);
        assert!(result.contains("No active recovery sessions found."));
        assert!(!result.contains("Completed"));
    }

    #[test]
    fn test_format_recovery_status_active_only() {
        let active = vec!["session-1".to_string(), "session-2".to_string()];
        let result = format_recovery_status(&active, &[]);

        assert!(result.contains("Found 2 active recovery session(s):"));
        assert!(result.contains("1. session-1"));
        assert!(result.contains("2. session-2"));
        assert!(!result.contains("Completed"));
    }

    #[test]
    fn test_format_recovery_status_completed_only() {
        let completed = vec!["old-session".to_string()];
        let result = format_recovery_status(&[], &completed);

        assert!(result.contains("No active recovery sessions found."));
        assert!(result.contains("Completed recovery sessions (1):"));
        assert!(result.contains("- old-session"));
    }

    #[test]
    fn test_format_recovery_status_mixed() {
        let active = vec!["active-1".to_string()];
        let completed = vec!["done-1".to_string(), "done-2".to_string()];
        let result = format_recovery_status(&active, &completed);

        assert!(result.contains("Found 1 active recovery session(s):"));
        assert!(result.contains("1. active-1"));
        assert!(result.contains("Completed recovery sessions (2):"));
        assert!(result.contains("- done-1"));
        assert!(result.contains("- done-2"));
    }
}
