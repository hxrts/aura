//! Context isolation and epoch replay prevention contracts.
//!
//! These tests verify InvariantContextIsolation and
//! InvariantCrossEpochReplayPrevention at the transport type level.

use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_transport::amp::{derive_for_recv, AmpHeader, AmpRatchetState};
use aura_transport::types::{Envelope, ScopedEnvelope};

// ============================================================================
// Context isolation (InvariantContextIsolation)
// ============================================================================

/// A ScopedEnvelope created for context A must not accept an inner envelope
/// created for context B. If this passes, messages can be routed to the
/// wrong relational context — a privacy violation.
#[test]
fn scoped_envelope_rejects_context_mismatch() {
    let context_a = ContextId::new_from_entropy([1u8; 32]);
    let context_b = ContextId::new_from_entropy([2u8; 32]);
    let sender = AuthorityId::new_from_entropy([3u8; 32]);
    let recipient = AuthorityId::new_from_entropy([4u8; 32]);

    // Envelope scoped to context A
    let envelope_a = Envelope::new_scoped(b"secret message".to_vec(), context_a, None);

    // Attempt to wrap in a ScopedEnvelope claiming context B
    let result = ScopedEnvelope::new(envelope_a, context_b, sender, recipient);
    assert!(
        result.is_err(),
        "envelope for context A must be rejected when wrapped as context B"
    );
}

/// A ScopedEnvelope with matching contexts must succeed.
#[test]
fn scoped_envelope_accepts_matching_context() {
    let context = ContextId::new_from_entropy([5u8; 32]);
    let sender = AuthorityId::new_from_entropy([6u8; 32]);
    let recipient = AuthorityId::new_from_entropy([7u8; 32]);

    let envelope = Envelope::new_scoped(b"ok message".to_vec(), context, None);
    let result = ScopedEnvelope::new(envelope, context, sender, recipient);
    assert!(result.is_ok(), "matching context must be accepted");
}

// ============================================================================
// Cross-epoch replay prevention (InvariantCrossEpochReplayPrevention)
// ============================================================================

/// A message from an old epoch must be rejected after the receiver has
/// advanced to a new epoch. This prevents replay of old messages that
/// were valid in a previous epoch.
#[test]
fn old_epoch_message_rejected_after_epoch_advance() {
    // Receiver has advanced to epoch 2
    let state = AmpRatchetState {
        chan_epoch: 2,
        last_checkpoint_gen: 0,
        skip_window: 4,
        pending_epoch: None,
    };

    // Message from epoch 1 (old)
    let old_header = AmpHeader {
        context: ContextId::new_from_entropy([10u8; 32]),
        channel: ChannelId::from_bytes([11u8; 32]),
        chan_epoch: 1,
        ratchet_gen: 2,
    };

    let result = derive_for_recv(&state, old_header);
    assert!(
        result.is_err(),
        "message from old epoch must be rejected after epoch advance"
    );
}

/// A message from a future epoch that is NOT the pending epoch must be
/// rejected — only the current epoch and the pending epoch are valid.
#[test]
fn future_epoch_message_rejected_unless_pending() {
    let state = AmpRatchetState {
        chan_epoch: 1,
        last_checkpoint_gen: 0,
        skip_window: 4,
        pending_epoch: Some(2), // epoch 2 is pending
    };

    // Epoch 2 is accepted (pending)
    let pending_header = AmpHeader {
        context: ContextId::new_from_entropy([12u8; 32]),
        channel: ChannelId::from_bytes([13u8; 32]),
        chan_epoch: 2,
        ratchet_gen: 1,
    };
    assert!(
        derive_for_recv(&state, pending_header).is_ok(),
        "pending epoch must be accepted"
    );

    // Epoch 3 is rejected (neither current nor pending)
    let far_future_header = AmpHeader {
        context: ContextId::new_from_entropy([12u8; 32]),
        channel: ChannelId::from_bytes([13u8; 32]),
        chan_epoch: 3,
        ratchet_gen: 1,
    };
    assert!(
        derive_for_recv(&state, far_future_header).is_err(),
        "epoch beyond pending must be rejected"
    );
}

// ============================================================================
// Sequence gap detection
// ============================================================================

/// Messages outside the generation window must be rejected — this is
/// how sequence gaps are enforced. A generation below the checkpoint
/// or above 2× the window is out of bounds.
#[test]
fn generation_outside_window_rejected() {
    let state = AmpRatchetState {
        chan_epoch: 0,
        last_checkpoint_gen: 10,
        skip_window: 4,
        pending_epoch: None,
    };

    // Below checkpoint (gen 9 < min 10)
    let too_old = AmpHeader {
        context: ContextId::new_from_entropy([20u8; 32]),
        channel: ChannelId::from_bytes([21u8; 32]),
        chan_epoch: 0,
        ratchet_gen: 9,
    };
    assert!(
        derive_for_recv(&state, too_old).is_err(),
        "generation below checkpoint must be rejected"
    );

    // Above window (gen 19 > max 18 = 10 + 2*4)
    let too_new = AmpHeader {
        context: ContextId::new_from_entropy([20u8; 32]),
        channel: ChannelId::from_bytes([21u8; 32]),
        chan_epoch: 0,
        ratchet_gen: 19,
    };
    assert!(
        derive_for_recv(&state, too_new).is_err(),
        "generation above window must be rejected"
    );

    // At boundary (gen 10 = min, gen 18 = max) — both accepted
    let at_min = AmpHeader {
        context: ContextId::new_from_entropy([20u8; 32]),
        channel: ChannelId::from_bytes([21u8; 32]),
        chan_epoch: 0,
        ratchet_gen: 10,
    };
    assert!(
        derive_for_recv(&state, at_min).is_ok(),
        "generation at window min must be accepted"
    );

    let at_max = AmpHeader {
        context: ContextId::new_from_entropy([20u8; 32]),
        channel: ChannelId::from_bytes([21u8; 32]),
        chan_epoch: 0,
        ratchet_gen: 18,
    };
    assert!(
        derive_for_recv(&state, at_max).is_ok(),
        "generation at window max must be accepted"
    );
}
