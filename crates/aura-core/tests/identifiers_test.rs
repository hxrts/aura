//! Comprehensive tests for identifier types in aura-core
//!
//! Tests all core identifier types for creation, uniqueness, serialization, and conversions.

#![allow(clippy::expect_used)]

use aura_core::{
    AccountId, DataId, DeviceId, EventId, EventNonce, GuardianId, IndividualId, MemberId,
    OperationId, SessionId,
};

/// Test basic identifier creation and uniqueness
#[test]
fn test_identifier_creation() {
    // Test AccountId
    let account1 = AccountId::new();
    let account2 = AccountId::new();
    assert_ne!(account1, account2, "AccountIds should be unique");

    // Test DeviceId
    let device1 = DeviceId::new();
    let device2 = DeviceId::new();
    assert_ne!(device1, device2, "DeviceIds should be unique");

    // Test SessionId
    let session1 = SessionId::new();
    let session2 = SessionId::new();
    assert_ne!(session1, session2, "SessionIds should be unique");

    // Test EventId
    let event1 = EventId::new();
    let event2 = EventId::new();
    assert_ne!(event1, event2, "EventIds should be unique");

    // Test GuardianId
    let guardian1 = GuardianId::new();
    let guardian2 = GuardianId::new();
    assert_ne!(guardian1, guardian2, "GuardianIds should be unique");
}

/// Test string representations
#[test]
fn test_identifier_string_representations() {
    let account = AccountId::new();
    let device = DeviceId::new();
    let session = SessionId::new();
    let event = EventId::new();
    let guardian = GuardianId::new();

    // Test string formatting
    let account_str = account.to_string();
    let device_str = device.to_string();
    let session_str = session.to_string();
    let event_str = event.to_string();
    let guardian_str = guardian.to_string();

    // Verify non-empty and proper prefixes where they exist
    assert!(!account_str.is_empty());
    assert!(!device_str.is_empty());
    assert!(
        session_str.starts_with("session-"),
        "SessionId should have session- prefix"
    );
    assert!(
        event_str.starts_with("event-"),
        "EventId should have event- prefix"
    );
    assert!(!guardian_str.is_empty()); // GuardianId displays raw UUID
}

/// Test EventNonce operations
#[test]
fn test_event_nonce() {
    let nonce1 = EventNonce::new(100);
    let nonce2 = EventNonce::new(200);
    let nonce3 = EventNonce::new(100);

    // Test ordering
    assert!(nonce1 < nonce2);
    assert!(nonce2 > nonce1);
    assert_eq!(nonce1, nonce3);

    // Test increment
    let next_nonce = nonce1.next();
    assert_eq!(next_nonce.value(), 101);

    // Test value retrieval
    assert_eq!(nonce1.value(), 100);
    assert_eq!(nonce2.value(), 200);
}

/// Test string-based identifier types
#[test]
fn test_string_identifiers() {
    // Test MemberId
    let member1 = MemberId::new("member1".to_string());
    let member2 = MemberId::new("member2".to_string());
    let member1_dup = MemberId::new("member1".to_string());

    assert_ne!(member1, member2);
    assert_eq!(member1, member1_dup);
    assert_eq!(member1.as_str(), "member1");

    // Test IndividualId
    let individual1 = IndividualId::new("individual1".to_string());
    let individual2 = IndividualId::new("individual2".to_string());

    assert_ne!(individual1, individual2);
    assert_eq!(individual1.as_str(), "individual1");

    // Test OperationId (UUID-based)
    let device1 = DeviceId::new();
    let device2 = DeviceId::new();
    let op1 = OperationId::new(device1, 1);
    let op2 = OperationId::new(device2, 2);

    assert_ne!(op1, op2);

    // Test DataId
    let data1 = DataId::new();
    let data2 = DataId::new();

    assert_ne!(data1, data2);
    assert!(
        data1.as_str().starts_with("data:"),
        "DataId should start with data: prefix"
    );
}

/// Test identifier serialization/deserialization
#[test]
fn test_identifier_serialization() {
    let account = AccountId::new();
    let device = DeviceId::new();
    let session = SessionId::new();
    let event = EventId::new();
    let guardian = GuardianId::new();
    let operation = OperationId::new(device, 123);
    let nonce = EventNonce::new(42);

    // Test DAG-CBOR serialization
    let account_cbor =
        aura_core::serialization::to_vec(&account).expect("Should serialize AccountId");
    let device_cbor = aura_core::serialization::to_vec(&device).expect("Should serialize DeviceId");
    let session_cbor =
        aura_core::serialization::to_vec(&session).expect("Should serialize SessionId");
    let event_cbor = aura_core::serialization::to_vec(&event).expect("Should serialize EventId");
    let guardian_cbor =
        aura_core::serialization::to_vec(&guardian).expect("Should serialize GuardianId");
    let operation_cbor =
        aura_core::serialization::to_vec(&operation).expect("Should serialize OperationId");
    let nonce_cbor = aura_core::serialization::to_vec(&nonce).expect("Should serialize EventNonce");

    // Test DAG-CBOR deserialization
    let account_restored: AccountId =
        aura_core::serialization::from_slice(&account_cbor).expect("Should deserialize AccountId");
    let device_restored: DeviceId =
        aura_core::serialization::from_slice(&device_cbor).expect("Should deserialize DeviceId");
    let session_restored: SessionId =
        aura_core::serialization::from_slice(&session_cbor).expect("Should deserialize SessionId");
    let event_restored: EventId =
        aura_core::serialization::from_slice(&event_cbor).expect("Should deserialize EventId");
    let guardian_restored: GuardianId = aura_core::serialization::from_slice(&guardian_cbor)
        .expect("Should deserialize GuardianId");
    let operation_restored: OperationId = aura_core::serialization::from_slice(&operation_cbor)
        .expect("Should deserialize OperationId");
    let nonce_restored: EventNonce =
        aura_core::serialization::from_slice(&nonce_cbor).expect("Should deserialize EventNonce");

    // Verify round-trip consistency
    assert_eq!(account, account_restored);
    assert_eq!(device, device_restored);
    assert_eq!(session, session_restored);
    assert_eq!(event, event_restored);
    assert_eq!(guardian, guardian_restored);
    assert_eq!(operation, operation_restored);
    assert_eq!(nonce, nonce_restored);
}

/// Test UUID conversions for UUID-based identifiers
#[test]
fn test_uuid_conversions() {
    let account = AccountId::new();
    let device = DeviceId::new();
    let session = SessionId::new();
    let event = EventId::new();
    let guardian = GuardianId::new();
    let operation = OperationId::new(device, 456);

    // Test UUID extraction for types that have uuid() method
    let session_uuid = session.uuid();
    let event_uuid = event.uuid();
    let operation_uuid = operation.uuid();

    // Test round-trip UUID conversion
    let account_from_uuid = AccountId::from_uuid(account.0);
    let device_from_uuid = DeviceId::from_uuid(device.0);
    let session_from_uuid = SessionId::from_uuid(session_uuid);
    let event_from_uuid = EventId::from_uuid(event_uuid);
    let guardian_from_uuid = GuardianId::from_uuid(guardian.0);
    let operation_from_uuid = OperationId::from_uuid(operation_uuid);

    assert_eq!(account, account_from_uuid);
    assert_eq!(device, device_from_uuid);
    assert_eq!(session, session_from_uuid);
    assert_eq!(event, event_from_uuid);
    assert_eq!(guardian, guardian_from_uuid);

    // OperationId round-trip is not perfect due to information loss
    // (UUID can't hold both DeviceId and sequence), so just test that
    // the conversion works without panicking and produces a valid OperationId
    assert_ne!(operation_from_uuid.actor, device); // actor is derived from operation UUID, not original device
    assert_eq!(operation_from_uuid.sequence, 0); // sequence is lost in conversion
}

/// Test string conversions for string-based identifiers
#[test]
fn test_string_conversions() {
    // Test From<String> implementations for string-based types
    let member_from_string = MemberId::from("test_member".to_string());
    let individual_from_string = IndividualId::from("test_individual".to_string());
    let data_from_string = DataId::from("test_data".to_string());

    assert_eq!(member_from_string.as_str(), "test_member");
    assert_eq!(individual_from_string.as_str(), "test_individual");
    assert_eq!(data_from_string.as_str(), "test_data");

    // Test From<&str> implementations
    let member_from_str = MemberId::from("test_member2");
    let individual_from_str = IndividualId::from("test_individual2");
    let data_from_str = DataId::from("test_data2");

    assert_eq!(member_from_str.as_str(), "test_member2");
    assert_eq!(individual_from_str.as_str(), "test_individual2");
    assert_eq!(data_from_str.as_str(), "test_data2");
}

/// Test identifier equality and ordering
#[test]
fn test_identifier_equality_and_ordering() {
    // Test EventNonce ordering
    let nonces = vec![
        EventNonce::new(300),
        EventNonce::new(100),
        EventNonce::new(200),
        EventNonce::new(50),
    ];

    let mut sorted_nonces = nonces.clone();
    sorted_nonces.sort();

    assert_eq!(sorted_nonces[0].value(), 50);
    assert_eq!(sorted_nonces[1].value(), 100);
    assert_eq!(sorted_nonces[2].value(), 200);
    assert_eq!(sorted_nonces[3].value(), 300);

    // Test string identifier ordering
    let members = vec![
        MemberId::new("charlie".to_string()),
        MemberId::new("alice".to_string()),
        MemberId::new("bob".to_string()),
    ];

    let mut sorted_members = members.clone();
    sorted_members.sort();

    assert_eq!(sorted_members[0].as_str(), "alice");
    assert_eq!(sorted_members[1].as_str(), "bob");
    assert_eq!(sorted_members[2].as_str(), "charlie");
}

/// Test identifier determinism with known inputs
#[test]
fn test_identifier_determinism() {
    // Create identifiers from known UUIDs
    let known_uuid = uuid::Uuid::from_u128(12345);

    let account1 = AccountId::from_uuid(known_uuid);
    let account2 = AccountId::from_uuid(known_uuid);
    assert_eq!(
        account1, account2,
        "Same UUID should produce same AccountId"
    );

    let device1 = DeviceId::from_uuid(known_uuid);
    let device2 = DeviceId::from_uuid(known_uuid);
    assert_eq!(device1, device2, "Same UUID should produce same DeviceId");

    // Test string identifiers determinism
    let member1 = MemberId::new("same_id".to_string());
    let member2 = MemberId::new("same_id".to_string());
    assert_eq!(member1, member2, "Same string should produce same MemberId");
}
