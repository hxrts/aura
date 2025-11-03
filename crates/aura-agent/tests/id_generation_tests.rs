//! Tests for typed identifier functionality
//!
//! These tests verify that the typed identifiers from aura-types work correctly
//! when used through the agent crate.

use aura_agent::{DataId, CapabilityId, DeviceId};

#[test]
fn test_data_id_creation() {
    let data_id = DataId::new();
    assert!(data_id.as_str().starts_with("data:"));
    
    let encrypted_id = DataId::new_encrypted();
    assert!(encrypted_id.as_str().starts_with("encrypted:"));
}

#[test]
fn test_capability_id_creation() {
    let cap_id = CapabilityId::new();
    assert!(cap_id.as_str().starts_with("cap:"));
    
    let device_id = DeviceId::new();
    let specific_cap = CapabilityId::for_data_and_grantee("test-data", device_id.into());
    assert!(specific_cap.as_str().starts_with("cap_test-data_"));
}

#[test]
fn test_id_conversion() {
    let data_id = DataId::new();
    let data_str = data_id.to_string();
    let back_to_data = DataId::from(data_str);
    assert_eq!(data_id, back_to_data);
}

#[test]
fn test_id_uniqueness() {
    let id1 = DataId::new();
    let id2 = DataId::new();
    assert_ne!(id1, id2);
    
    let cap1 = CapabilityId::new();
    let cap2 = CapabilityId::new();
    assert_ne!(cap1, cap2);
}

#[test]
fn test_encrypted_vs_regular_data_id() {
    let regular = DataId::new();
    let encrypted = DataId::new_encrypted();
    
    assert!(regular.as_str().starts_with("data:"));
    assert!(encrypted.as_str().starts_with("encrypted:"));
    assert_ne!(regular, encrypted);
}

#[test]
fn test_specific_capability_id_format() {
    let device_id = DeviceId::new();
    let data_id = "my-test-data";
    
    let cap_id = CapabilityId::for_data_and_grantee(data_id, device_id.into());
    let cap_str = cap_id.as_str();
    
    assert!(cap_str.starts_with("cap_my-test-data_"));
    assert!(cap_str.contains(&device_id.to_string()));
}