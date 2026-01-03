//! Transport types round-trip tests.

#![allow(clippy::expect_used, missing_docs)]

use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::time::{OrderTime, TimeStamp};
use aura_core::util::serialization::{from_slice, to_vec};
use aura_transport::types::{
    ConnectionCloseReason, ConnectionInfo, ConnectionState, Envelope, FrameType, PrivacyContext,
    PrivacyLevel,
};

#[test]
fn envelope_round_trip() {
    let payload = b"transport payload".to_vec();
    let context_id = ContextId::new_from_entropy([7u8; 32]);
    let envelope = Envelope::new_scoped(payload.clone(), context_id, Some("cap.read".to_string()));

    let encoded = to_vec(&envelope).expect("serialize envelope");
    let decoded: Envelope = from_slice(&encoded).expect("deserialize envelope");

    assert_eq!(decoded.message_id, envelope.message_id);
    assert!(matches!(
        decoded.header.frame_type,
        FrameType::ContextScoped
    ));
    assert!(matches!(
        decoded.header.privacy_level,
        PrivacyLevel::ContextScoped
    ));
    assert_eq!(decoded.header.capability_hint.as_deref(), Some("cap.read"));
    assert_eq!(decoded.header.frame_size, payload.len() as u32);
    assert_eq!(decoded.payload, payload);
    assert_eq!(decoded.context_id, Some(context_id));
}

#[test]
fn connection_state_transitions() {
    let peer = AuthorityId::new_from_entropy([3u8; 32]);
    let mut info = ConnectionInfo::new(peer, PrivacyLevel::Clear);

    assert!(matches!(info.state, ConnectionState::Connecting { .. }));

    let privacy_context = PrivacyContext {
        privacy_level: PrivacyLevel::Clear,
        context_id: None,
        capability_filtering: true,
        message_blinding: false,
    };
    let timestamp = TimeStamp::OrderClock(OrderTime([4u8; 32]));

    info.establish_with_timestamp(privacy_context, timestamp.clone());
    assert!(info.is_established());

    info.state = ConnectionState::Closing {
        closing_at: timestamp.clone(),
        reason: ConnectionCloseReason::Graceful,
    };
    assert!(matches!(
        info.state,
        ConnectionState::Closing {
            reason: ConnectionCloseReason::Graceful,
            ..
        }
    ));

    info.state = ConnectionState::Closed {
        closed_at: timestamp,
        reason: ConnectionCloseReason::RemoteClosed,
    };
    assert!(matches!(
        info.state,
        ConnectionState::Closed {
            reason: ConnectionCloseReason::RemoteClosed,
            ..
        }
    ));
}
