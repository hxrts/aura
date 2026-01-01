//! Simple recovery status formatter for the CLI.
//!
//! This module drives the `recovery status` command output. Keeping the CLI channel
//! minimal means we only summarize facts available in the journal rather than
//! recreating the richer TUI visualization.
//!
//! ## Note on Portable Implementation
//!
//! The core formatting logic is now in `aura_app::ui::types::format_recovery_status`.
//! This module re-exports it for backwards compatibility with existing terminal code.

// Re-export the portable implementation from aura-app
pub use aura_app::ui::types::format_recovery_status;

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify the re-export works correctly
    #[test]
    fn test_reexport_works() {
        let result = format_recovery_status(&[], &[]);
        assert!(result.contains("No active recovery sessions found."));
    }
}
