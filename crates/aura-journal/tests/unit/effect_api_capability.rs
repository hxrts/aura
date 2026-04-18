use crate::common::{physical_time_ms, test_device_id, test_uuid};
use aura_core::time::TimeStamp;
use aura_journal::effect_api::capability::{
    Attenuation, CapabilityId, CapabilityRef, CapabilitySignature, RecoveryCapability, ResourceRef,
};

#[test]
fn capability_id_creation() {
    let id1 = CapabilityId::new(test_uuid(1));
    let id2 = CapabilityId::new(test_uuid(2));
    assert_ne!(id1, id2);
}

#[test]
fn resource_ref_recovery() {
    let resource = ResourceRef::recovery(0, 42);
    assert!(resource.is_recovery());
    assert!(!resource.is_storage());
    assert!(resource.as_str().contains("42"));
}

#[test]
fn resource_ref_storage() {
    let resource = ResourceRef::storage("/backup/data");
    assert!(resource.is_storage());
    assert!(!resource.is_recovery());
    assert!(resource.as_str().contains("/backup/data"));
}

#[test]
fn resource_ref_relay() {
    let resource = ResourceRef::relay("session-123");
    assert!(resource.is_relay());
    assert!(!resource.is_storage());
    assert!(resource.as_str().contains("session-123"));
}

#[test]
fn capability_ref_expiration() {
    let cap = CapabilityRef::new(
        CapabilityId::new(test_uuid(3)),
        ResourceRef::recovery(0, 1),
        physical_time_ms(1000),
        CapabilitySignature::new(vec![0u8; 64], test_device_id(1)),
    );

    assert!(!cap.is_expired(&physical_time_ms(500)));
    assert!(!cap.is_expired(&physical_time_ms(1000)));
    assert!(cap.is_expired(&physical_time_ms(1500)));
}

#[test]
fn capability_ref_time_until_expiration() {
    let cap = CapabilityRef::new(
        CapabilityId::new(test_uuid(4)),
        ResourceRef::recovery(0, 1),
        physical_time_ms(1000),
        CapabilitySignature::new(vec![0u8; 64], test_device_id(1)),
    );

    assert!(cap.time_until_expiration(&physical_time_ms(500)).is_some());
    assert_eq!(cap.time_until_expiration(&physical_time_ms(1000)), Some(0));
    assert_eq!(cap.time_until_expiration(&physical_time_ms(1500)), Some(0));
}

#[test]
#[allow(clippy::disallowed_methods)]
fn capability_ref_with_attenuation() {
    let attenuation = Attenuation::new()
        .with_max_uses(5)
        .with_operations(vec!["read".to_string()]);

    let cap = CapabilityRef::new(
        CapabilityId::new(test_uuid(5)),
        ResourceRef::storage("/data"),
        physical_time_ms(1000),
        CapabilitySignature::new(vec![0u8; 64], test_device_id(1)),
    )
    .with_attenuation(attenuation);

    let att = cap.attenuation.expect("attenuation");
    assert_eq!(att.max_uses, Some(5));
    assert_eq!(att.allowed_operations, Some(vec!["read".to_string()]));
}

#[test]
fn attenuation_builder() {
    let attenuation = Attenuation::new()
        .with_max_uses(10)
        .with_operations(vec!["read".to_string(), "write".to_string()])
        .with_expiration(TimeStamp::PhysicalClock(aura_core::time::PhysicalTime {
            ts_ms: 5000,
            uncertainty: None,
        }))
        .with_metadata("purpose".to_string(), "testing".to_string());

    assert_eq!(attenuation.max_uses, Some(10));
    assert_eq!(
        attenuation.restricted_expires_at,
        Some(physical_time_ms(5000))
    );
    assert_eq!(attenuation.metadata.get("purpose").unwrap(), "testing");
}

#[test]
fn resource_ref_from_string() {
    let resource: ResourceRef = "custom://resource".into();
    assert_eq!(resource.as_str(), "custom://resource");
}

#[test]
fn capability_signature() {
    let sig = CapabilitySignature::new(vec![0u8; 64], test_device_id(0));
    assert_eq!(sig.signature.len(), 64);
}

#[test]
#[allow(clippy::disallowed_methods)]
fn recovery_capability_creation() {
    let target = test_device_id(2);
    let guardians = vec![test_device_id(3), test_device_id(4)];
    let sig = CapabilitySignature::new(vec![0u8; 64], test_device_id(0));

    let recovery_cap = RecoveryCapability::new(
        CapabilityId::new(test_uuid(6)),
        target,
        guardians,
        2,
        physical_time_ms(10000),
        0,
        1,
        sig,
    );

    assert_eq!(recovery_cap.target_device, target);
    assert_eq!(recovery_cap.issuing_guardians.len(), 2);
    assert_eq!(recovery_cap.guardian_threshold, 2);
    assert!(recovery_cap.has_guardian_quorum());
}

#[test]
#[allow(clippy::disallowed_methods)]
fn recovery_capability_expiration() {
    let sig = CapabilitySignature::new(vec![0u8; 64], test_device_id(0));
    let recovery_cap = RecoveryCapability::new(
        CapabilityId::new(test_uuid(7)),
        test_device_id(5),
        vec![test_device_id(6), test_device_id(7)],
        2,
        physical_time_ms(1000),
        0,
        1,
        sig,
    );

    assert!(recovery_cap.is_valid(&physical_time_ms(500)));
    assert!(!recovery_cap.is_valid(&physical_time_ms(1500)));
}

#[test]
#[allow(clippy::disallowed_methods)]
fn recovery_capability_insufficient_guardians() {
    let sig = CapabilitySignature::new(vec![0u8; 64], test_device_id(0));
    let recovery_cap = RecoveryCapability::new(
        CapabilityId::new(test_uuid(8)),
        test_device_id(8),
        vec![test_device_id(9)],
        2,
        physical_time_ms(10000),
        0,
        1,
        sig,
    );

    assert!(!recovery_cap.has_guardian_quorum());
    assert!(!recovery_cap.is_valid(&physical_time_ms(500)));
}

#[test]
#[allow(clippy::disallowed_methods)]
fn recovery_capability_with_reason() {
    let sig = CapabilitySignature::new(vec![0u8; 64], test_device_id(0));
    let recovery_cap = RecoveryCapability::new(
        CapabilityId::new(test_uuid(9)),
        test_device_id(10),
        vec![test_device_id(11), test_device_id(12)],
        2,
        physical_time_ms(10000),
        0,
        1,
        sig,
    )
    .with_reason("Lost device, need to rekey");

    assert_eq!(recovery_cap.recovery_reason, "Lost device, need to rekey");
}
