//! Wire format serialization/deserialization tests for AMP messages.

use aura_amp::wire::{deserialize_message, serialize_message, AmpMessage, AMP_WIRE_SCHEMA_VERSION};
use aura_core::effects::amp::AmpHeader;
use aura_core::identifiers::{ChannelId, ContextId};

fn test_context() -> ContextId {
    ContextId::from_uuid(uuid::Uuid::from_bytes([1u8; 16]))
}

fn test_channel() -> ChannelId {
    ChannelId::from_bytes([2u8; 32])
}

fn test_header() -> AmpHeader {
    AmpHeader {
        context: test_context(),
        channel: test_channel(),
        chan_epoch: 5,
        ratchet_gen: 42,
    }
}

fn serialize_or_panic(msg: &AmpMessage) -> Vec<u8> {
    match serialize_message(msg) {
        Ok(bytes) => bytes,
        Err(err) => panic!("serialization should succeed: {err}"),
    }
}

fn deserialize_or_panic(bytes: &[u8]) -> AmpMessage {
    match deserialize_message(bytes) {
        Ok(message) => message,
        Err(err) => panic!("deserialization should succeed: {err}"),
    }
}

#[test]
fn test_amp_message_new() {
    let header = test_header();
    let payload = vec![1, 2, 3, 4, 5];

    let msg = AmpMessage::new(header.clone(), payload.clone());

    assert_eq!(msg.schema_version, AMP_WIRE_SCHEMA_VERSION);
    assert_eq!(msg.header.context, header.context);
    assert_eq!(msg.header.channel, header.channel);
    assert_eq!(msg.header.chan_epoch, 5);
    assert_eq!(msg.header.ratchet_gen, 42);
    assert_eq!(msg.payload, payload);
}

#[test]
fn test_serialize_deserialize_roundtrip() {
    let header = test_header();
    let payload = b"Hello, AMP world!".to_vec();

    let original = AmpMessage::new(header, payload);

    // Serialize
    let bytes = serialize_or_panic(&original);
    assert!(!bytes.is_empty(), "serialized bytes should not be empty");

    // Deserialize
    let recovered = deserialize_or_panic(&bytes);

    // Verify roundtrip
    assert_eq!(recovered.schema_version, original.schema_version);
    assert_eq!(recovered.header.context, original.header.context);
    assert_eq!(recovered.header.channel, original.header.channel);
    assert_eq!(recovered.header.chan_epoch, original.header.chan_epoch);
    assert_eq!(recovered.header.ratchet_gen, original.header.ratchet_gen);
    assert_eq!(recovered.payload, original.payload);
}

#[test]
fn test_serialize_empty_payload() {
    let header = test_header();
    let payload = vec![];

    let msg = AmpMessage::new(header, payload);
    let bytes = serialize_or_panic(&msg);
    let recovered = deserialize_or_panic(&bytes);

    assert!(recovered.payload.is_empty());
}

#[test]
fn test_serialize_large_payload() {
    let header = test_header();
    // 1MB payload
    let payload = vec![0xAB; 1024 * 1024];

    let msg = AmpMessage::new(header, payload.clone());
    let bytes = serialize_or_panic(&msg);
    let recovered = deserialize_or_panic(&bytes);

    assert_eq!(recovered.payload.len(), payload.len());
    assert_eq!(recovered.payload, payload);
}

#[test]
fn test_deserialize_invalid_bytes() {
    let invalid_bytes = vec![0xFF, 0xFE, 0xFD]; // Random garbage

    let result = deserialize_message(&invalid_bytes);
    assert!(result.is_err(), "should fail to deserialize invalid bytes");
}

#[test]
fn test_deserialize_empty_bytes() {
    let result = deserialize_message(&[]);
    assert!(result.is_err(), "should fail to deserialize empty bytes");
}

#[test]
fn test_deserialize_truncated_message() {
    let header = test_header();
    let payload = b"test payload".to_vec();
    let msg = AmpMessage::new(header, payload);

    let bytes = serialize_or_panic(&msg);

    // Truncate the serialized bytes
    let truncated = &bytes[..bytes.len() / 2];

    let result = deserialize_message(truncated);
    assert!(
        result.is_err(),
        "should fail to deserialize truncated message"
    );
}

#[test]
fn test_schema_version_is_embedded() {
    let header = test_header();
    let payload = vec![42];
    let msg = AmpMessage::new(header, payload);

    assert_eq!(msg.schema_version, AMP_WIRE_SCHEMA_VERSION);
    assert_eq!(msg.schema_version, 1, "current schema version should be 1");
}

#[test]
fn test_different_headers_produce_different_bytes() {
    let payload = b"same payload".to_vec();

    let msg1 = AmpMessage::new(
        AmpHeader {
            context: test_context(),
            channel: test_channel(),
            chan_epoch: 1,
            ratchet_gen: 1,
        },
        payload.clone(),
    );

    let msg2 = AmpMessage::new(
        AmpHeader {
            context: test_context(),
            channel: test_channel(),
            chan_epoch: 2, // Different epoch
            ratchet_gen: 1,
        },
        payload,
    );

    let bytes1 = serialize_or_panic(&msg1);
    let bytes2 = serialize_or_panic(&msg2);

    assert_ne!(
        bytes1, bytes2,
        "different headers should produce different serializations"
    );
}

mod proptest_wire {
    use super::*;
    use proptest::prelude::*;

    prop_compose! {
        fn arb_header()(
            context_bytes in prop::array::uniform16(any::<u8>()),
            channel_bytes in prop::array::uniform32(any::<u8>()),
            chan_epoch in any::<u64>(),
            ratchet_gen in any::<u64>(),
        ) -> AmpHeader {
            AmpHeader {
                context: ContextId::from_uuid(uuid::Uuid::from_bytes(context_bytes)),
                channel: ChannelId::from_bytes(channel_bytes),
                chan_epoch,
                ratchet_gen,
            }
        }
    }

    proptest! {
        #[test]
        fn roundtrip_preserves_message(
            header in arb_header(),
            payload in prop::collection::vec(any::<u8>(), 0..1024),
        ) {
            let original = AmpMessage::new(header, payload);
            let bytes = serialize_message(&original)?;
            let recovered = deserialize_message(&bytes)?;

            prop_assert_eq!(recovered.schema_version, original.schema_version);
            prop_assert_eq!(recovered.header.context, original.header.context);
            prop_assert_eq!(recovered.header.channel, original.header.channel);
            prop_assert_eq!(recovered.header.chan_epoch, original.header.chan_epoch);
            prop_assert_eq!(recovered.header.ratchet_gen, original.header.ratchet_gen);
            prop_assert_eq!(recovered.payload, original.payload);
        }

        #[test]
        fn serialization_is_deterministic(
            header in arb_header(),
            payload in prop::collection::vec(any::<u8>(), 0..256),
        ) {
            let msg = AmpMessage::new(header, payload);

            let bytes1 = serialize_message(&msg)?;
            let bytes2 = serialize_message(&msg)?;

            prop_assert_eq!(bytes1, bytes2, "serialization should be deterministic");
        }
    }
}
