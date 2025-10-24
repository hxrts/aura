// Common utilities for CLI command handling
//
// This module provides shared functionality for command handlers to eliminate
// code duplication and centralize common patterns like agent creation and
// capability scope parsing.

use crate::config::Config;
use aura_agent::IntegratedAgent;
use aura_journal::capability::types::CapabilityScope;
use std::collections::BTreeMap;

/// Create an IntegratedAgent instance from configuration
///
/// This centralizes the agent creation logic that was duplicated across
/// multiple command handlers. For compatibility with existing commands.
pub async fn create_agent(config: &Config) -> anyhow::Result<IntegratedAgent> {
    let device_id = config.device_id;
    let account_id = config.account_id;
    let storage_root = config.data_dir.join("storage");
    let effects = aura_crypto::Effects::test(); // Use test effects for CLI

    IntegratedAgent::new(device_id, account_id, storage_root, effects)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create agent: {}", e))
}

/// Parse capability scope from string format "namespace:operation"
///
/// This centralizes the capability scope parsing logic that was duplicated
/// across storage, network, and capability command handlers.
pub fn parse_capability_scope(
    scope_str: &str,
    resource: Option<&str>,
) -> anyhow::Result<CapabilityScope> {
    let parts: Vec<&str> = scope_str.split(':').collect();
    if parts.len() != 2 {
        return Err(anyhow::anyhow!(
            "Capability scope must be in format 'namespace:operation'"
        ));
    }

    let namespace = parts[0].to_string();
    let operation = parts[1].to_string();

    let mut scope = CapabilityScope::simple(&namespace, &operation);
    if let Some(res) = resource {
        scope.resource = Some(res.to_string());
    }

    Ok(scope)
}

/// Parse attributes from string format "key=value,key2=value2"
///
/// This provides a common utility for parsing key-value attributes used
/// in various command handlers.
pub fn parse_attributes(attr_str: &str) -> anyhow::Result<BTreeMap<String, String>> {
    let mut attributes = BTreeMap::new();

    for pair in attr_str.split(',') {
        let parts: Vec<&str> = pair.trim().split('=').collect();
        if parts.len() != 2 {
            return Err(anyhow::anyhow!(
                "Attributes must be in format 'key=value,key2=value2'"
            ));
        }
        let key = parts[0].trim();
        let value = parts[1].trim();
        if key.is_empty() || value.is_empty() {
            return Err(anyhow::anyhow!(
                "Attributes must be in format 'key=value,key2=value2'"
            ));
        }
        attributes.insert(key.to_string(), value.to_string());
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

    /// Standard error message for agent creation failures
    pub fn agent_creation_failed(error: &dyn std::fmt::Display) -> String {
        format!("Failed to create agent: {}", error)
    }
}

/// Standard success messages for consistent user experience
pub mod success {
    /// Format a standard success message with checkmark
    pub fn operation_completed(operation: &str, details: &[(&str, &str)]) -> String {
        let mut msg = format!("✓ {}", operation);
        for (key, value) in details {
            msg.push_str(&format!("\n  {}: {}", key, value));
        }
        msg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_capability_scope() {
        // Test basic scope parsing
        let scope = parse_capability_scope("storage:read", None).unwrap();
        assert_eq!(scope.namespace, "storage");
        assert_eq!(scope.operation, "read");
        assert_eq!(scope.resource, None);

        // Test scope with resource
        let scope = parse_capability_scope("storage:read", Some("file123")).unwrap();
        assert_eq!(scope.namespace, "storage");
        assert_eq!(scope.operation, "read");
        assert_eq!(scope.resource, Some("file123".to_string()));

        // Test invalid format
        assert!(parse_capability_scope("invalid", None).is_err());
        assert!(parse_capability_scope("too:many:parts", None).is_err());
    }

    #[test]
    fn test_parse_attributes() {
        // Test single attribute
        let attrs = parse_attributes("key=value").unwrap();
        assert_eq!(attrs.get("key"), Some(&"value".to_string()));

        // Test multiple attributes
        let attrs = parse_attributes("key1=value1,key2=value2").unwrap();
        assert_eq!(attrs.get("key1"), Some(&"value1".to_string()));
        assert_eq!(attrs.get("key2"), Some(&"value2".to_string()));

        // Test with spaces
        let attrs = parse_attributes("key1 = value1 , key2 = value2").unwrap();
        assert_eq!(attrs.get("key1"), Some(&"value1".to_string()));
        assert_eq!(attrs.get("key2"), Some(&"value2".to_string()));

        // Test invalid format
        assert!(parse_attributes("invalid").is_err());
        assert!(parse_attributes("key=").is_err());
        assert!(parse_attributes("=value").is_err());
    }
}
