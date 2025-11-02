// Common utilities for CLI command handling
//
// This module provides shared functionality for command handlers to eliminate
// code duplication and centralize common patterns like agent creation and
// capability scope parsing.
//
use crate::config::Config;
use anyhow::Context;
use aura_types::DeviceId;

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

/// Parse operation scope from "namespace:operation" format
#[allow(dead_code)]
pub fn parse_operation_scope(scope: &str) -> anyhow::Result<(String, String)> {
    let parts: Vec<&str> = scope.split(':').collect();
    if parts.len() != 2 {
        anyhow::bail!("Invalid operation scope format - expected 'namespace:operation'");
    }
    Ok((parts[0].to_string(), parts[1].to_string()))
}

/// Parse capability scope with optional resource
#[allow(dead_code)]
pub fn parse_capability_scope(scope: &str, resource: Option<&str>) -> anyhow::Result<String> {
    if let Some(res) = resource {
        Ok(format!("{}:{}", scope, res))
    } else {
        Ok(scope.to_string())
    }
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
