// Common utilities for CLI command handling
//
// This module provides shared functionality for command handlers to eliminate
// code duplication and centralize common patterns like agent creation and
// capability scope parsing.
//
use crate::config::Config;
use anyhow::Context;
use aura_types::{DeviceId, identifiers::AccountId};
use std::collections::HashMap;
use std::sync::Arc;

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

/// Validate that the config has required fields for operations
pub fn validate_config(config: &Config) -> anyhow::Result<()> {
    if config.device_id == DeviceId::default() {
        anyhow::bail!("Device ID not configured. Run 'aura init' first.");
    }
    if config.account_id == AccountId::default() {
        anyhow::bail!("Account ID not configured. Run 'aura init' first.");
    }
    Ok(())
}

/// Get storage path for the configured account
pub fn get_storage_path(config: &Config) -> std::path::PathBuf {
    config.data_dir.join("storage").join(config.account_id.to_string())
}

/// Get journal path for the configured account
pub fn get_journal_path(config: &Config) -> std::path::PathBuf {
    config.data_dir.join("journal").join(format!("{}.automerge", config.account_id))
}

/// Parse a device ID from string format
pub fn parse_device_id(device_id_str: &str) -> anyhow::Result<DeviceId> {
    let uuid = uuid::Uuid::parse_str(device_id_str).context("Invalid device ID format - expected UUID")?;
    Ok(DeviceId::from(uuid))
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
pub fn parse_peer_list(peer_str: &str) -> Vec<DeviceId> {
    peer_str
        .split(',')
        .filter_map(|s| {
            uuid::Uuid::parse_str(s.trim())
                .ok()
                .map(DeviceId::from)
        })
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
pub fn parse_attributes(attr_str: &str) -> anyhow::Result<HashMap<String, String>> {
    let mut attributes = HashMap::new();

    for pair in attr_str.split(',') {
        let parts: Vec<&str> = pair.split('=').collect();
        if parts.len() != 2 {
            anyhow::bail!("Invalid attribute format - expected 'key=value'");
        }
        attributes.insert(parts[0].trim().to_string(), parts[1].trim().to_string());
    }

    Ok(attributes)
}

/// Format file size in human-readable format
pub fn format_file_size(size: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    const THRESHOLD: u64 = 1024;
    
    if size == 0 {
        return "0 B".to_string();
    }

    let mut size_f = size as f64;
    let mut unit_index = 0;

    while size_f >= THRESHOLD as f64 && unit_index < UNITS.len() - 1 {
        size_f /= THRESHOLD as f64;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{} {}", size, UNITS[unit_index])
    } else {
        format!("{:.1} {}", size_f, UNITS[unit_index])
    }
}

/// Format timestamp in human-readable format
pub fn format_timestamp(timestamp: u64) -> String {
    use chrono::{DateTime, Utc, TimeZone};
    
    let dt = Utc.timestamp_opt(timestamp as i64 / 1000, ((timestamp % 1000) * 1_000_000) as u32)
        .single()
        .unwrap_or_else(|| Utc::now());
    
    dt.format("%Y-%m-%d %H:%M:%S UTC").to_string()
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
