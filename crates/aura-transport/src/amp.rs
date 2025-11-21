//! AMP ratchet helpers and header definitions.
//!
//! Deterministic, fact-derived ratchet state used for AMP messaging.

use aura_core::identifiers::{ChannelId, ContextId};
use aura_core::Hash32;
use aura_journal::ChannelEpochState;

/// AMP message header used as AEAD associated data
///
/// Contains the contextual and ratchet state information that uniquely identifies
/// a message in the AMP (Asynchronous Message Protocol) system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AmpHeader {
    /// Relational context in which this message is sent
    pub context: ContextId,
    /// Channel identifier for the message
    pub channel: ChannelId,
    /// Channel epoch for epoch-based ratcheting
    pub chan_epoch: u64,
    /// Current ratchet generation counter
    pub ratchet_gen: u64,
}

/// Derived ratchet state and a derived message key placeholder
///
/// Result of deriving the ratchet state for a message. Contains the header,
/// derived message key, and the next generation counter to advance to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RatchetDerivation {
    /// The derived AMP header for this message
    pub header: AmpHeader,
    /// The derived message key for AEAD encryption/decryption
    pub message_key: Hash32,
    /// The next generation counter to advance to after this message
    pub next_gen: u64,
}

/// Error categories for AMP ratchet operations
///
/// Represents validation failures during ratchet derivation for both send and receive paths.
#[derive(Debug, thiserror::Error)]
pub enum AmpError {
    /// Channel epoch mismatch error
    ///
    /// Occurs when the received epoch does not match the current or pending epoch.
    /// This indicates an epoch desynchronization that requires recovery.
    #[error("epoch mismatch: got {got}, current {current}, pending {pending:?}")]
    EpochMismatch {
        /// The epoch received in the message header
        got: u64,
        /// The current channel epoch
        current: u64,
        /// The pending channel epoch if an epoch bump is in progress
        pending: Option<u64>,
    },
    /// Generation (ratchet counter) out of valid window error
    ///
    /// Occurs when the ratchet generation is outside the dual-window bounds [min, max].
    /// This protects against replay attacks and window violations.
    #[error("generation out of window: gen {gen}, valid min {min}, max {max}")]
    GenerationOutOfWindow {
        /// The generation number from the message
        gen: u64,
        /// The minimum valid generation (from last checkpoint)
        min: u64,
        /// The maximum valid generation (checkpoint + 2*skip_window)
        max: u64,
    },
}

/// Compute the valid generation window union (2W span) for a checkpoint.
fn window_bounds(last_checkpoint_gen: u64, skip_window: u32) -> (u64, u64) {
    let w = skip_window as u64;
    let start = last_checkpoint_gen;
    let end = start + 2 * w;
    (start, end)
}

/// Derive header and placeholder message key for send given reduced channel state.
///
/// This enforces dual-window coverage and current/pending epoch validity.
pub fn derive_for_send(
    context: ContextId,
    channel: ChannelId,
    state: &ChannelEpochState,
) -> Result<RatchetDerivation, AmpError> {
    let (min_gen, max_gen) = window_bounds(state.last_checkpoint_gen, state.skip_window);
    let ratchet_gen = state.current_gen;

    if ratchet_gen < min_gen || ratchet_gen > max_gen {
        return Err(AmpError::GenerationOutOfWindow {
            gen: ratchet_gen,
            min: min_gen,
            max: max_gen,
        });
    }

    let header = AmpHeader {
        context,
        channel,
        chan_epoch: state.chan_epoch,
        ratchet_gen,
    };

    // Placeholder: actual KDF wiring to be integrated with context roots
    let mut material = Vec::with_capacity(16 + 32 + 8 + 8);
    material.extend_from_slice(&header.context.to_bytes());
    material.extend_from_slice(header.channel.as_bytes());
    material.extend_from_slice(&header.chan_epoch.to_le_bytes());
    material.extend_from_slice(&header.ratchet_gen.to_le_bytes());
    let message_key = Hash32::from_bytes(&material);

    Ok(RatchetDerivation {
        header,
        message_key,
        next_gen: ratchet_gen + 1,
    })
}

/// Validate receive header against reduced channel state and compute key.
pub fn derive_for_recv(
    state: &ChannelEpochState,
    header: AmpHeader,
) -> Result<RatchetDerivation, AmpError> {
    let pending_epoch = state.pending_bump.as_ref().map(|p| p.new_epoch);
    let valid_epoch = header.chan_epoch == state.chan_epoch
        || pending_epoch.is_some_and(|e| header.chan_epoch == e);

    if !valid_epoch {
        return Err(AmpError::EpochMismatch {
            got: header.chan_epoch,
            current: state.chan_epoch,
            pending: pending_epoch,
        });
    }

    let (min_gen, max_gen) = window_bounds(state.last_checkpoint_gen, state.skip_window);
    if header.ratchet_gen < min_gen || header.ratchet_gen > max_gen {
        return Err(AmpError::GenerationOutOfWindow {
            gen: header.ratchet_gen,
            min: min_gen,
            max: max_gen,
        });
    }

    let mut material = Vec::with_capacity(16 + 32 + 8 + 8);
    material.extend_from_slice(&header.context.to_bytes());
    material.extend_from_slice(header.channel.as_bytes());
    material.extend_from_slice(&header.chan_epoch.to_le_bytes());
    material.extend_from_slice(&header.ratchet_gen.to_le_bytes());
    let message_key = Hash32::from_bytes(&material);

    Ok(RatchetDerivation {
        header,
        message_key,
        next_gen: header.ratchet_gen + 1,
    })
}

/// Helper for send path: derive header/key and surface the next generation callers
/// should persist via facts/reduction (no local mutation).
pub fn advance_send(
    context: ContextId,
    channel: ChannelId,
    state: &ChannelEpochState,
) -> Result<RatchetDerivation, AmpError> {
    derive_for_send(context, channel, state)
}

/// Helper for recv path: validate header and surface next generation for callers
/// to persist via facts/reduction (no local mutation).
pub fn advance_recv(
    state: &ChannelEpochState,
    header: AmpHeader,
) -> Result<RatchetDerivation, AmpError> {
    derive_for_recv(state, header)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_journal::fact::ChannelBumpReason;
    use aura_journal::reduction::PendingBump;

    #[test]
    fn send_within_window_succeeds() {
        let ctx = ContextId::new();
        let channel = ChannelId::from_bytes([7u8; 32]);
        let state = ChannelEpochState {
            chan_epoch: 0,
            pending_bump: None,
            last_checkpoint_gen: 0,
            current_gen: 10,
            skip_window: 1024,
        };

        let deriv = derive_for_send(ctx, channel, &state).unwrap();
        assert_eq!(deriv.header.ratchet_gen, 10);
        assert_eq!(deriv.next_gen, 11);
    }

    #[test]
    fn recv_rejects_gen_outside_window() {
        let ctx = ContextId::new();
        let channel = ChannelId::from_bytes([9u8; 32]);
        let state = ChannelEpochState {
            chan_epoch: 2,
            pending_bump: None,
            last_checkpoint_gen: 100,
            current_gen: 100,
            skip_window: 10,
        };

        let header = AmpHeader {
            context: ctx,
            channel,
            chan_epoch: 2,
            ratchet_gen: 200,
        };

        let err = derive_for_recv(&state, header).unwrap_err();
        matches!(err, AmpError::GenerationOutOfWindow { .. });
    }

    #[test]
    fn recv_accepts_window_edges() {
        let ctx = ContextId::new();
        let channel = ChannelId::from_bytes([8u8; 32]);
        let state = ChannelEpochState {
            chan_epoch: 0,
            pending_bump: None,
            last_checkpoint_gen: 50,
            current_gen: 50,
            skip_window: 10,
        };

        // min bound
        let header_min = AmpHeader {
            context: ctx,
            channel,
            chan_epoch: 0,
            ratchet_gen: 50,
        };
        derive_for_recv(&state, header_min).expect("min bound should succeed");

        // max bound (2W span)
        let header_max = AmpHeader {
            context: ctx,
            channel,
            chan_epoch: 0,
            ratchet_gen: 70,
        };
        derive_for_recv(&state, header_max).expect("max bound should succeed");
    }

    #[test]
    fn recv_rejects_replay_outside_window() {
        let ctx = ContextId::new();
        let channel = ChannelId::from_bytes([6u8; 32]);
        let state = ChannelEpochState {
            chan_epoch: 0,
            pending_bump: None,
            last_checkpoint_gen: 30,
            current_gen: 40,
            skip_window: 5,
        };

        let stale_header = AmpHeader {
            context: ctx,
            channel,
            chan_epoch: 0,
            ratchet_gen: 20,
        };

        let err = derive_for_recv(&state, stale_header).unwrap_err();
        matches!(err, AmpError::GenerationOutOfWindow { .. });
    }

    #[test]
    fn recv_accepts_pending_epoch() {
        let ctx = ContextId::new();
        let channel = ChannelId::from_bytes([5u8; 32]);
        let state = ChannelEpochState {
            chan_epoch: 1,
            pending_bump: Some(PendingBump {
                parent_epoch: 1,
                new_epoch: 2,
                bump_id: Hash32::default(),
                reason: ChannelBumpReason::Routine,
            }),
            last_checkpoint_gen: 0,
            current_gen: 0,
            skip_window: 16,
        };

        let header = AmpHeader {
            context: ctx,
            channel,
            chan_epoch: 2,
            ratchet_gen: 1,
        };

        let deriv = derive_for_recv(&state, header).unwrap();
        assert_eq!(deriv.header.chan_epoch, 2);
    }

    #[test]
    fn advance_send_matches_derive() {
        let ctx = ContextId::new();
        let channel = ChannelId::from_bytes([2u8; 32]);
        let state = ChannelEpochState {
            chan_epoch: 0,
            pending_bump: None,
            last_checkpoint_gen: 5,
            current_gen: 6,
            skip_window: 16,
        };

        let via_send = advance_send(ctx, channel, &state).unwrap();
        let via_direct = derive_for_send(ctx, channel, &state).unwrap();
        assert_eq!(via_send.header, via_direct.header);
        assert_eq!(via_send.next_gen, 7);
    }
}
