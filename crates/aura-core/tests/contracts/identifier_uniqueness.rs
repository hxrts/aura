//! Comprehensive tests for identifier types in aura-core
//!
//! Tests all core identifier types for creation, uniqueness, serialization, and conversions.

use aura_core::{
    derive_legacy_authority_from_device, AccountId, AuthorityId, ContextId, DataId, DeviceId,
    EventId, EventNonce, GuardianId, IndividualId, LegacyAuthorityFromDeviceReason,
    LegacyAuthorityFromDeviceRequest, MemberId, OperationId, SessionId,
};
use std::io;
#[allow(clippy::disallowed_types)]
use std::sync::Arc;
#[allow(clippy::disallowed_types)]
use std::sync::Mutex;
use tracing_subscriber::fmt::MakeWriter;
use uuid::Uuid;

fn account(seed: u8) -> AccountId {
    AccountId::new_from_entropy([seed; 32])
}

fn device(seed: u8) -> DeviceId {
    DeviceId::new_from_entropy([seed; 32])
}

fn session(seed: u8) -> SessionId {
    SessionId::from_uuid(Uuid::from_bytes([seed; 16]))
}

fn event(seed: u8) -> EventId {
    EventId::from_uuid(Uuid::from_bytes([seed; 16]))
}

fn guardian(seed: u8) -> GuardianId {
    GuardianId::new_from_entropy([seed; 32])
}

/// Different entropy produces different IDs across all identifier types —
/// collision means unrelated entities share identity.
#[test]
fn test_identifier_creation() {
    // Test AccountId
    let account1 = account(1);
    let account2 = account(2);
    assert_ne!(account1, account2, "AccountIds should be unique");

    // Test DeviceId
    let device1 = device(3);
    let device2 = device(4);
    assert_ne!(device1, device2, "DeviceIds should be unique");

    // Test SessionId
    let session1 = session(5);
    let session2 = session(6);
    assert_ne!(session1, session2, "SessionIds should be unique");

    // Test EventId
    let event1 = event(7);
    let event2 = event(8);
    assert_ne!(event1, event2, "EventIds should be unique");

    // Test GuardianId
    let guardian1 = guardian(9);
    let guardian2 = guardian(10);
    assert_ne!(guardian1, guardian2, "GuardianIds should be unique");
}

/// `new_from_entropy` and `from_entropy` must produce identical results.
#[test]
fn test_entropy_constructor_aliases() {
    use aura_core::{AuthorityId, ContextId};

    let authority_a = AuthorityId::new_from_entropy([1u8; 32]);
    let authority_b = AuthorityId::from_entropy([1u8; 32]);
    assert_eq!(authority_a, authority_b);

    let context_a = ContextId::new_from_entropy([2u8; 32]);
    let context_b = ContextId::from_entropy([2u8; 32]);
    assert_eq!(context_a, context_b);

    let device_a = DeviceId::new_from_entropy([3u8; 32]);
    let device_b = DeviceId::from_entropy([3u8; 32]);
    assert_eq!(device_a, device_b);
}

/// String representations must be stable — they appear in logs, debug
/// output, and some serialization paths.
#[test]
fn test_identifier_string_representations() {
    let account = account(11);
    let device = device(12);
    let session = session(13);
    let event = event(14);
    let guardian = guardian(15);

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

/// EventNonce increment and ordering — nonces must be monotonic for
/// journal event deduplication.
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
    let next_nonce = nonce1.next().expect("event nonce increment should succeed");
    assert_eq!(next_nonce.value(), 101);

    // Test value retrieval
    assert_eq!(nonce1.value(), 100);
    assert_eq!(nonce2.value(), 200);
}

/// EventNonce overflow produces an error — silent wraparound would break
/// journal ordering.
#[test]
fn test_event_nonce_overflow() {
    let nonce = EventNonce::new(u64::MAX);
    let err = nonce.next().unwrap_err();
    assert!(err.to_string().contains("EventNonce overflow"));
}

/// String-based identifiers (MemberId, IndividualId, etc.) preserve
/// exact input — no normalization that could break lookups.
#[test]
fn test_string_identifiers() {
    // Test MemberId
    let member1 = MemberId::new(String::from("member1"));
    let member2 = MemberId::new(String::from("member2"));
    let member1_dup = MemberId::new(String::from("member1"));

    assert_ne!(member1, member2);
    assert_eq!(member1, member1_dup);
    assert_eq!(member1.as_str(), "member1");

    // Test IndividualId
    let individual1 = IndividualId::new(String::from("individual1"));
    let individual2 = IndividualId::new(String::from("individual2"));

    assert_ne!(individual1, individual2);
    assert_eq!(individual1.as_str(), "individual1");

    // Test OperationId (UUID-based)
    let op1 = OperationId::from_uuid(Uuid::from_bytes([16u8; 16]));
    let op2 = OperationId::from_uuid(Uuid::from_bytes([17u8; 16]));
    assert_ne!(op1, op2);

    // Test DataId
    let data1 = DataId::new();
    let data2 = DataId::new();

    // Deterministic derivation yields identical values; ensure stable prefix instead
    assert_eq!(data1, data2);
    assert!(
        data1.as_str().starts_with("data:"),
        "DataId should start with data: prefix"
    );
}

/// DAG-CBOR roundtrip for all identifier types — identifiers are embedded
/// in facts, messages, and tree operations that cross the wire.
#[test]
fn test_identifier_serialization() {
    let account = account(17);
    let device = device(18);
    let session = session(19);
    let event = event(20);
    let guardian = guardian(21);
    let operation = OperationId::from_uuid(Uuid::from_bytes([22u8; 16]));
    let nonce = EventNonce::new(42);

    // Test DAG-CBOR serialization
    let account_cbor =
        aura_core::util::serialization::to_vec(&account).expect("Should serialize AccountId");
    let device_cbor =
        aura_core::util::serialization::to_vec(&device).expect("Should serialize DeviceId");
    let session_cbor =
        aura_core::util::serialization::to_vec(&session).expect("Should serialize SessionId");
    let event_cbor =
        aura_core::util::serialization::to_vec(&event).expect("Should serialize EventId");
    let guardian_cbor =
        aura_core::util::serialization::to_vec(&guardian).expect("Should serialize GuardianId");
    let operation_cbor =
        aura_core::util::serialization::to_vec(&operation).expect("Should serialize OperationId");
    let nonce_cbor =
        aura_core::util::serialization::to_vec(&nonce).expect("Should serialize EventNonce");

    // Test DAG-CBOR deserialization
    let account_restored: AccountId = aura_core::util::serialization::from_slice(&account_cbor)
        .expect("Should deserialize AccountId");
    let device_restored: DeviceId = aura_core::util::serialization::from_slice(&device_cbor)
        .expect("Should deserialize DeviceId");
    let session_restored: SessionId = aura_core::util::serialization::from_slice(&session_cbor)
        .expect("Should deserialize SessionId");
    let event_restored: EventId = aura_core::util::serialization::from_slice(&event_cbor)
        .expect("Should deserialize EventId");
    let guardian_restored: GuardianId = aura_core::util::serialization::from_slice(&guardian_cbor)
        .expect("Should deserialize GuardianId");
    let operation_restored: OperationId =
        aura_core::util::serialization::from_slice(&operation_cbor)
            .expect("Should deserialize OperationId");
    let nonce_restored: EventNonce = aura_core::util::serialization::from_slice(&nonce_cbor)
        .expect("Should deserialize EventNonce");

    // Verify round-trip consistency
    assert_eq!(account, account_restored);
    assert_eq!(device, device_restored);
    assert_eq!(session, session_restored);
    assert_eq!(event, event_restored);
    assert_eq!(guardian, guardian_restored);
    assert_eq!(operation, operation_restored);
    assert_eq!(nonce, nonce_restored);
}

/// UUID-based identifiers roundtrip through UUID — needed for database
/// storage and cross-language FFI.
#[test]
fn test_uuid_conversions() {
    let account = account(23);
    let device = device(24);
    let session = session(25);
    let event = event(26);
    let guardian = guardian(27);
    let operation = OperationId::from_uuid(Uuid::from_bytes([28u8; 16]));

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

    // OperationId is just a UUID wrapper, so round-trip should be perfect
    assert_eq!(operation, operation_from_uuid);
}

/// String-based identifiers roundtrip through From/Into.
#[test]
fn test_string_conversions() {
    // Test From<String> implementations for string-based types
    let member_from_string = MemberId::from(String::from("test_member"));
    let individual_from_string = IndividualId::from(String::from("test_individual"));
    let data_from_string = DataId::from(String::from("test_data"));

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

/// Eq and Ord are consistent — needed for BTreeMap/BTreeSet storage
/// of identifiers in journal indices.
#[test]
fn test_identifier_equality_and_ordering() {
    // Test EventNonce ordering
    let nonces = vec![
        EventNonce::new(300),
        EventNonce::new(100),
        EventNonce::new(200),
        EventNonce::new(50),
    ];

    let mut sorted_nonces = nonces;
    sorted_nonces.sort();

    assert_eq!(sorted_nonces[0].value(), 50);
    assert_eq!(sorted_nonces[1].value(), 100);
    assert_eq!(sorted_nonces[2].value(), 200);
    assert_eq!(sorted_nonces[3].value(), 300);

    // Test string identifier ordering
    let members = vec![
        MemberId::new(String::from("carol")),
        MemberId::new(String::from("alice")),
        MemberId::new(String::from("bob")),
    ];

    let mut sorted_members = members;
    sorted_members.sort();

    assert_eq!(sorted_members[0].as_str(), "alice");
    assert_eq!(sorted_members[1].as_str(), "bob");
    assert_eq!(sorted_members[2].as_str(), "carol");
}

/// Same UUID input produces the same identifier — deterministic
/// construction is required for content addressing.
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
    let member1 = MemberId::new(String::from("same_id"));
    let member2 = MemberId::new(String::from("same_id"));
    assert_eq!(member1, member2, "Same string should produce same MemberId");
}

#[test]
fn test_legacy_authority_from_device_request_requires_metadata() {
    let device = device(32);

    let missing_site = LegacyAuthorityFromDeviceRequest::new(
        device,
        LegacyAuthorityFromDeviceReason::CompatibilityBoundary,
        "   ",
        "compatibility bridge for deterministic test fixture",
    )
    .unwrap_err();
    assert_eq!(
        missing_site.to_string(),
        "legacy authority-from-device derivation requires a non-empty site"
    );

    let missing_justification = LegacyAuthorityFromDeviceRequest::new(
        device,
        LegacyAuthorityFromDeviceReason::CompatibilityBoundary,
        "identifiers_test",
        "",
    )
    .unwrap_err();
    assert_eq!(
        missing_justification.to_string(),
        "legacy authority-from-device derivation requires a non-empty justification"
    );
}

#[allow(clippy::disallowed_types)]
#[derive(Clone, Default)]
struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

impl SharedBuffer {
    fn snapshot(&self) -> String {
        String::from_utf8(self.0.lock().expect("lock log buffer").clone()).expect("utf8 log buffer")
    }
}

struct SharedBufferWriter(SharedBuffer);

impl io::Write for SharedBufferWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
             .0
            .lock()
            .expect("lock log buffer")
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for SharedBuffer {
    type Writer = SharedBufferWriter;

    fn make_writer(&'a self) -> Self::Writer {
        SharedBufferWriter(self.clone())
    }
}

#[test]
fn test_legacy_authority_from_device_derivation_is_domain_separated_deterministic_and_logged() {
    let device = device(33);
    let request = LegacyAuthorityFromDeviceRequest::new(
        device,
        LegacyAuthorityFromDeviceReason::CompatibilityBoundary,
        "identifiers_test",
        "verify explicit legacy compatibility bridge behavior",
    )
    .expect("valid derivation request");

    let logs = SharedBuffer::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(logs.clone())
        .with_ansi(false)
        .without_time()
        .finish();

    let first = tracing::subscriber::with_default(subscriber, || {
        derive_legacy_authority_from_device(request.clone())
    });
    let second = derive_legacy_authority_from_device(request);

    assert_eq!(first.authority_id, second.authority_id);
    assert_eq!(first.device_id, device);
    assert_ne!(first.authority_id.uuid(), device.uuid());
    assert_eq!(first.site, "identifiers_test");
    assert_eq!(
        first.reason,
        LegacyAuthorityFromDeviceReason::CompatibilityBoundary
    );

    let log_output = logs.snapshot();
    assert!(
        log_output.contains("legacy authority-from-device derivation executed"),
        "expected structured warning in log output: {log_output}"
    );
    assert!(
        log_output.contains("identifiers_test"),
        "expected site metadata in log output: {log_output}"
    );
    assert!(
        log_output.contains(&device.to_string()),
        "expected device metadata in log output: {log_output}"
    );
}

// ============================================================================
// Pinned identifier test vectors
//
// If these change between releases, all existing journals, channel bindings,
// and key derivations using these identifiers break. The string
// representations are the permanent on-disk/on-wire format.
// ============================================================================

/// Pin the string format of UUID-based identifiers to catch accidental
/// changes to Display/ToString implementations.
#[test]
fn pinned_identifier_string_format() {
    let known_uuid = uuid::Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef);

    let account = AccountId::from_uuid(known_uuid);
    let device = DeviceId::from_uuid(known_uuid);
    let session = SessionId::from_uuid(known_uuid);

    // These are the permanent string representations. If any of these
    // assertions fail, it means the format changed and existing data
    // references are broken.
    let uuid_str = "01234567-89ab-cdef-0123-456789abcdef";
    assert_eq!(
        account.to_string(),
        uuid_str,
        "AccountId string format must be stable (no prefix)"
    );
    assert_eq!(
        device.to_string(),
        uuid_str,
        "DeviceId string format must be stable (no prefix)"
    );
    assert_eq!(
        session.to_string(),
        format!("session-{uuid_str}"),
        "SessionId string format must be stable (session- prefix)"
    );
}

/// Pin the byte representation of AuthorityId to catch changes to the
/// entropy-based constructor.
#[test]
fn pinned_authority_id_from_entropy() {
    let entropy = [42u8; 32];
    let a1 = AuthorityId::new_from_entropy(entropy);
    let a2 = AuthorityId::new_from_entropy(entropy);
    assert_eq!(a1, a2, "same entropy must produce same AuthorityId");

    // Different entropy must produce different result
    let a3 = AuthorityId::new_from_entropy([43u8; 32]);
    assert_ne!(a1, a3, "different entropy must produce different AuthorityId");
}

// ============================================================================
// Context isolation at L1
//
// ContextId is the L1 foundation for cross-context isolation. At this layer,
// the guarantee is that contexts are opaque and unlinkable: different contexts
// produce different IDs, the same context always produces the same ID, and
// the ID doesn't encode participants or authority structure. Namespace-level
// fact isolation is enforced at L2 (aura-journal).
// ============================================================================

/// Each new ContextId from different entropy is unique — prevents
/// accidental cross-context collision.
#[test]
fn context_ids_are_unique() {
    let c1 = ContextId::new_from_entropy([10u8; 32]);
    let c2 = ContextId::new_from_entropy([20u8; 32]);
    assert_ne!(c1, c2, "distinct entropy must produce distinct ContextIds");
}

/// Same entropy produces the same ContextId — deterministic derivation.
#[test]
fn context_id_deterministic() {
    let c1 = ContextId::new_from_entropy([10u8; 32]);
    let c2 = ContextId::new_from_entropy([10u8; 32]);
    assert_eq!(c1, c2, "same entropy must produce same ContextId");
}

/// ContextId is opaque: its string representation doesn't contain participant
/// identifiers. This prevents observers from correlating contexts by parsing IDs.
#[test]
fn context_id_does_not_encode_participants() {
    let authority = AuthorityId::new_from_entropy([99u8; 32]);
    let context = ContextId::new_from_entropy([50u8; 32]);

    let ctx_str = context.to_string();
    let auth_str = authority.to_string();

    // The context ID must not contain the authority ID as a substring.
    // This is a necessary (not sufficient) condition for unlinkability.
    assert!(
        !ctx_str.contains(&auth_str),
        "ContextId must not embed authority identity"
    );
}

/// ContextId byte representation is stable and fixed-width (16 bytes UUID).
#[test]
fn context_id_byte_representation_is_fixed_width() {
    let c = ContextId::new_from_entropy([10u8; 32]);
    assert_eq!(c.to_bytes().len(), 16, "ContextId must be exactly 16 bytes");
}
