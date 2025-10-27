// Common utilities for CLI command handling
//
// This module provides shared functionality for command handlers to eliminate
// code duplication and centralize common patterns like agent creation and
// capability scope parsing.
//
// NOTE: Temporarily simplified - agent dependencies disabled

use crate::config::Config;
use anyhow::Context;

/// Load config from path with consistent error handling
/// This eliminates the repeated Config::load() pattern in main.rs
pub async fn load_config(config_path: &std::path::Path) -> anyhow::Result<Config> {
    Config::load(config_path)
        .await
        .context(errors::config_load_failed(
            config_path,
            &"config load failed",
        ))
}

/// Common error messages for consistent user experience
pub mod errors {
    /// Standard error message for config loading failures
    pub fn config_load_failed(
        config_path: &std::path::Path,
        error: &dyn std::fmt::Display,
    ) -> String {
        format!(
            "Error loading config from {}: {}\nRun 'aura init' first to create an account",
            config_path.display(),
            error
        )
    }
}
