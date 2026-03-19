//! Serialization round-trip tests for core newtypes and envelopes.

use aura_core::messages::{MessageSequence, MessageTimestamp, WireEnvelope};
use aura_core::types::facts::{FactEncoding, FactEnvelope, FactTypeId};
use aura_core::util::serialization::{from_slice, to_vec};
use aura_core::{
    DeviceId, FlowCost, FlowNonce, NetworkAddress, ReceiptSig, SessionId, StoragePath,
};

#[test]
fn storage_path_roundtrip_json() {
    let path = StoragePath::parse("namespace/personal/*").unwrap();
    let bytes = to_vec(&path).unwrap();
    let decoded: StoragePath = from_slice(&bytes).unwrap();
    assert_eq!(decoded, path);
}

#[test]
fn network_address_roundtrip_json() {
    let addr = NetworkAddress::parse("example.com:443").unwrap();
    let bytes = to_vec(&addr).unwrap();
    let decoded: NetworkAddress = from_slice(&bytes).unwrap();
    assert_eq!(decoded, addr);
}

#[test]
fn flow_newtypes_roundtrip_bincode() {
    let cost = FlowCost::new(42);
    let nonce = FlowNonce::new(7);
    let sig = ReceiptSig::new(vec![9u8; 64]).unwrap();

    let cost_bytes = to_vec(&cost).unwrap();
    let nonce_bytes = to_vec(&nonce).unwrap();
    let sig_bytes = to_vec(&sig).unwrap();

    let decoded_cost: FlowCost = from_slice(&cost_bytes).unwrap();
    let decoded_nonce: FlowNonce = from_slice(&nonce_bytes).unwrap();
    let decoded_sig: ReceiptSig = from_slice(&sig_bytes).unwrap();

    assert_eq!(decoded_cost, cost);
    assert_eq!(decoded_nonce, nonce);
    assert_eq!(decoded_sig, sig);
}

#[test]
fn wire_envelope_roundtrip_bincode() {
    let sender = DeviceId::new_from_entropy([5u8; 32]);
    let session = SessionId::from_entropy([6u8; 32]);
    let envelope = WireEnvelope::new(
        Some(session),
        sender,
        MessageSequence::new(11),
        MessageTimestamp::new(1234),
        "payload".to_string(),
    );

    let bytes = to_vec(&envelope).unwrap();
    let decoded: WireEnvelope<String> = from_slice(&bytes).unwrap();

    assert_eq!(decoded.session_id, Some(session));
    assert_eq!(decoded.sender_id, sender);
    assert_eq!(decoded.sequence.value(), 11);
    assert_eq!(decoded.timestamp.value(), 1234);
    assert_eq!(decoded.payload, "payload");
}

#[test]
fn fact_envelope_roundtrip_json() {
    let envelope = FactEnvelope {
        type_id: FactTypeId::from("demo"),
        schema_version: 1,
        encoding: FactEncoding::Json,
        payload: vec![1, 2, 3],
    };

    let bytes = to_vec(&envelope).unwrap();
    let decoded: FactEnvelope = from_slice(&bytes).unwrap();
    assert_eq!(decoded, envelope);
}
