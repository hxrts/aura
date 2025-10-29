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
use aura_agent::traits::TransportAdapter;
use aura_agent::{AgentFactory, ProductionFactory, ProductionStorage};
use aura_transport::MemoryTransport;
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

/// Create an agent core from the configuration
/// This provides a consistent way to create production agents across CLI commands
#[allow(dead_code)]
pub async fn create_agent_core(
    config: &Config,
) -> anyhow::Result<aura_agent::AgentCore<TransportAdapter<MemoryTransport>, ProductionStorage>> {
    // Get device and account IDs from config
    let device_id = config.device_id;
    let account_id = config.account_id;

    // Create memory transport for testing
    // For production, use NoiseTcpTransport from aura-transport
    let inner_transport = Arc::new(MemoryTransport::default());
    let transport = Arc::new(TransportAdapter::new(inner_transport, device_id));

    // Create production storage
    let storage_path = config.data_dir.join("storage").join(account_id.to_string());
    let storage = Arc::new(
        ProductionFactory::create_storage(account_id, storage_path)
            .await
            .context("Failed to create storage")?,
    );

    // Create agent using factory
    let agent = AgentFactory::create_with_dependencies(device_id, account_id, transport, storage)
        .context("Failed to create agent from configuration")?;

    Ok(agent)
}

/// Parse a device ID from string format
#[allow(dead_code)]
pub fn parse_device_id(device_id_str: &str) -> anyhow::Result<Uuid> {
    Uuid::parse_str(device_id_str).context("Invalid device ID format - expected UUID")
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

/// Parse a peer list from comma-separated device IDs
#[allow(dead_code)]
pub fn parse_peer_list(peer_str: &str) -> Vec<Uuid> {
    peer_str
        .split(',')
        .filter_map(|s| Uuid::parse_str(s.trim()).ok())
        .collect()
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

/// Parse attributes from key=value pairs
#[allow(dead_code)]
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
