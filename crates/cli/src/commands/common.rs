// Common utilities for CLI command handling
//
// This module provides shared functionality for command handlers to eliminate
// code duplication and centralize common patterns like agent creation and
// capability scope parsing.
//
// NOTE: Temporarily simplified - agent dependencies disabled
// TODO: complete this implementation

use crate::config::Config;
use anyhow::Context;
use aura_agent::{Agent, AgentFactory, BootstrapConfig, ProductionFactory, StorageAgent};
use aura_types::{AccountId, DeviceId};
use std::sync::Arc;
use uuid::Uuid;

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

/// Create an agent instance from the configuration
/// This provides a consistent way to create production agents across CLI commands
pub async fn create_agent(config: &Config) -> anyhow::Result<impl Agent + StorageAgent> {
    // Get device and account IDs from config
    let device_id = config.device_id;
    let account_id = config.account_id;

    // Create production transport and storage
    let transport = Arc::new(
        ProductionFactory::create_transport(device_id, "127.0.0.1:0".to_string())
            .await
            .context("Failed to create transport")?,
    );

    let storage_path = config
        .data_dir
        .join("storage")
        .join(account_id.0.to_string());
    let storage = Arc::new(
        ProductionFactory::create_storage(account_id, storage_path)
            .await
            .context("Failed to create storage")?,
    );

    // Create production agent using factory
    let uninit_agent = AgentFactory::create_production(device_id, account_id, transport, storage)
        .await
        .context("Failed to create agent from configuration")?;

    // Bootstrap the agent with default configuration
    let bootstrap_config = BootstrapConfig {
        threshold: 2,   // Default threshold for multi-device accounts
        share_count: 3, // Default share count
        parameters: std::collections::HashMap::new(),
    };

    let idle_agent = uninit_agent
        .bootstrap(bootstrap_config)
        .await
        .context("Failed to bootstrap agent")?;

    Ok(idle_agent)
}

/// Parse a device ID from string format
pub fn parse_device_id(device_id_str: &str) -> anyhow::Result<Uuid> {
    Uuid::parse_str(device_id_str).context("Invalid device ID format - expected UUID")
}

/// Parse operation scope from "namespace:operation" format
pub fn parse_operation_scope(scope: &str) -> anyhow::Result<(String, String)> {
    let parts: Vec<&str> = scope.split(':').collect();
    if parts.len() != 2 {
        anyhow::bail!("Invalid operation scope format - expected 'namespace:operation'");
    }
    Ok((parts[0].to_string(), parts[1].to_string()))
}

/// Parse a peer list from comma-separated device IDs
pub fn parse_peer_list(peer_str: &str) -> Vec<Uuid> {
    peer_str
        .split(',')
        .filter_map(|s| Uuid::parse_str(s.trim()).ok())
        .collect()
}

/// Parse capability scope with optional resource
pub fn parse_capability_scope(scope: &str, resource: Option<&str>) -> anyhow::Result<String> {
    if let Some(res) = resource {
        Ok(format!("{}:{}", scope, res))
    } else {
        Ok(scope.to_string())
    }
}

/// Parse attributes from key=value pairs
pub fn parse_attributes(
    attr_str: &str,
) -> anyhow::Result<std::collections::HashMap<String, String>> {
    let mut attributes = std::collections::HashMap::new();

    for pair in attr_str.split(',') {
        let parts: Vec<&str> = pair.split('=').collect();
        if parts.len() != 2 {
            anyhow::bail!("Invalid attribute format - expected 'key=value'");
        }
        attributes.insert(parts[0].trim().to_string(), parts[1].trim().to_string());
    }

    Ok(attributes)
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
