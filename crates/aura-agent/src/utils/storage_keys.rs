//! Storage key formatting utilities
//!
//! Centralized storage key construction to ensure consistency and prevent typos.
//! All storage keys follow a consistent format: `{type}:{identifier}`.

use aura_types::DeviceId;

/// FROST key share storage key
pub fn frost_keys(device_id: DeviceId) -> String {
    format!("frost_keys:{}", device_id.0)
}

/// Bootstrap metadata storage key
pub fn bootstrap_metadata(device_id: DeviceId) -> String {
    format!("bootstrap_metadata:{}", device_id.0)
}

/// Derived identity storage key
pub fn derived_identity(app_id: &str, context: &str) -> String {
    format!("derived_identity:{}:{}", app_id, context)
}

/// Protected data storage key
pub fn protected_data(data_id: &str) -> String {
    format!("protected_data:{}", data_id)
}

/// Metadata storage key
pub fn metadata(data_id: &str) -> String {
    format!("metadata:{}", data_id)
}

/// Capability storage key
pub fn capability(capability_id: &str) -> String {
    format!("capability:{}", capability_id)
}

/// Quota limit storage key
pub fn quota_limit(scope: &str) -> String {
    format!("quota_limit:{}", scope)
}

/// Replica storage key
pub fn replica(peer_device_id: &str, data_id: &str) -> String {
    format!("replica:{}:{}", peer_device_id, data_id)
}

/// Generic data storage key
pub fn data(data_id: &str) -> String {
    format!("data:{}", data_id)
}

/// Capability metadata storage key for data
pub fn capabilities_for_data(data_id: &str) -> String {
    format!("capabilities:{}", data_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_frost_keys() {
        let device_id = DeviceId(Uuid::new_v4());
        let key = frost_keys(device_id);
        assert!(key.starts_with("frost_keys:"));
        assert!(key.contains(&device_id.0.to_string()));
    }

    #[test]
    fn test_derived_identity() {
        let key = derived_identity("my-app", "user-123");
        assert_eq!(key, "derived_identity:my-app:user-123");
    }

    #[test]
    fn test_protected_data() {
        let key = protected_data("data-456");
        assert_eq!(key, "protected_data:data-456");
    }
}
