//! Tests for typed identifier functionality (CapabilityId, ResourceRef).

use aura_journal::effect_api::capability::{CapabilityId, ResourceRef};
use uuid::Uuid;

#[test]
fn capability_id_display_is_prefixed() {
    let uuid = Uuid::from_bytes(*b"1234567890abcdef");
    let cap_id = CapabilityId::from_uuid(uuid);
    let rendered = cap_id.to_string();
    assert!(
        rendered.starts_with("cap-"),
        "display should prefix with cap-"
    );
    assert!(
        rendered.contains(&uuid.to_string()),
        "display should include underlying uuid"
    );
}

#[test]
fn capability_id_deterministic_from_uuid() {
    let uuid_a = Uuid::from_bytes(*b"aaaa1111aaaa1111");
    let uuid_b = Uuid::from_bytes(*b"bbbb2222bbbb2222");

    let cap_a1 = CapabilityId::from_uuid(uuid_a);
    let cap_a2 = CapabilityId::from_uuid(uuid_a);
    let cap_b = CapabilityId::from_uuid(uuid_b);

    assert_eq!(
        cap_a1, cap_a2,
        "same UUID should yield identical CapabilityId"
    );
    assert_ne!(
        cap_a1, cap_b,
        "different UUIDs must produce distinct CapabilityIds"
    );
}

#[test]
fn resource_ref_constructors_cover_domains() {
    let recovery = ResourceRef::recovery(5, 9);
    assert!(recovery.is_recovery());
    assert!(recovery.as_str().contains("recovery/5#9"));

    let storage = ResourceRef::storage("path/to/file");
    assert!(storage.is_storage());
    assert_eq!(storage.as_str(), "storage://path/to/file");

    let relay = ResourceRef::relay("session-123");
    assert!(relay.is_relay());
    assert_eq!(relay.as_str(), "relay://session-123");
}
