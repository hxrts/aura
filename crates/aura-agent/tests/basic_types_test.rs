//! Basic types test that doesn't depend on coordination crate
//!
//! This test verifies that the basic agent types work correctly
//! without requiring the coordination crate to compile.

use aura_agent::{DerivedIdentity, DeviceAttestation, KeyShare, SecurityLevel};
use aura_types::{AccountId, DeviceId};

#[test]
fn test_derived_identity_creation() {
    let identity = DerivedIdentity {
        app_id: "test-app".to_string(),
        context: "test-context".to_string(),
        identity_key: vec![1, 2, 3, 4],
        proof: vec![5, 6, 7, 8],
    };

    assert_eq!(identity.app_id, "test-app");
    assert_eq!(identity.context, "test-context");
    assert_eq!(identity.identity_key.len(), 4);
    assert_eq!(identity.proof.len(), 4);
}

#[test]
fn test_key_share_creation() {
    let device_id = DeviceId::new();
    let key_share = KeyShare {
        device_id,
        share_data: vec![1, 2, 3, 4, 5, 6, 7, 8],
    };

    assert_eq!(key_share.device_id, device_id);
    assert_eq!(key_share.share_data.len(), 8);
}

#[test]
fn test_device_attestation_creation() {
    let device_id = DeviceId::new();
    let attestation = DeviceAttestation {
        platform: "Test Platform".to_string(),
        device_id: device_id.to_string(),
        security_features: vec!["Hardware keys".to_string()],
        security_level: SecurityLevel::TEE,
        attestation_data: [("test".to_string(), "value".to_string())]
            .into_iter()
            .collect(),
    };

    assert_eq!(attestation.platform, "Test Platform");
    assert_eq!(attestation.security_level, SecurityLevel::TEE);
    assert_eq!(attestation.security_features.len(), 1);
    assert_eq!(attestation.attestation_data.len(), 1);
}

#[test]
fn test_security_levels() {
    let levels = vec![
        SecurityLevel::Software,
        SecurityLevel::TEE,
        SecurityLevel::StrongBox,
    ];

    for level in levels {
        // Test that all security levels can be created and compared
        let other_level = level.clone();
        assert_eq!(level, other_level);
    }
}

#[test]
fn test_json_serialization() {
    let identity = DerivedIdentity {
        app_id: "json-test".to_string(),
        context: "serialization".to_string(),
        identity_key: vec![0xFF, 0xAB],
        proof: vec![0xCD, 0xEF],
    };

    // Test JSON serialization
    let json = serde_json::to_string(&identity).unwrap();
    assert!(json.contains("json-test"));
    assert!(json.contains("serialization"));

    // Test JSON deserialization
    let deserialized: DerivedIdentity = serde_json::from_str(&json).unwrap();
    assert_eq!(identity.app_id, deserialized.app_id);
    assert_eq!(identity.context, deserialized.context);
    assert_eq!(identity.identity_key, deserialized.identity_key);
    assert_eq!(identity.proof, deserialized.proof);
}

#[test]
fn test_types_integration() {
    // Test that all types work together
    let device_id = DeviceId::new();
    let account_id = AccountId::new();

    let identity = DerivedIdentity {
        app_id: "integration-test".to_string(),
        context: format!("device-{}", device_id),
        identity_key: vec![1, 2, 3],
        proof: vec![4, 5, 6],
    };

    let key_share = KeyShare {
        device_id,
        share_data: vec![10, 20, 30],
    };

    let attestation = DeviceAttestation {
        platform: "Integration Test".to_string(),
        device_id: device_id.to_string(),
        security_features: vec!["Test feature".to_string()],
        security_level: SecurityLevel::Software,
        attestation_data: [
            ("account_id".to_string(), account_id.to_string()),
            ("app_id".to_string(), identity.app_id.clone()),
        ]
        .into_iter()
        .collect(),
    };

    // Verify everything is connected properly
    assert_eq!(key_share.device_id, device_id);
    assert_eq!(attestation.device_id, device_id.to_string());
    assert!(attestation.attestation_data.contains_key("account_id"));
    assert!(attestation.attestation_data.contains_key("app_id"));
}
