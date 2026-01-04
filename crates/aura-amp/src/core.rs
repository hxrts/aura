//! Pure AMP helpers.
//!
//! This module contains effect-free utilities for AMP state derivation.

use aura_journal::ChannelEpochState;
use aura_transport::amp::{AmpHeader, AmpRatchetState};

/// Derive nonce from AMP header using centralized crypto utilities.
pub fn nonce_from_header(header: &AmpHeader) -> [u8; 12] {
    aura_core::crypto::amp::derive_nonce_from_ratchet(header.ratchet_gen, header.chan_epoch)
}

/// Convert reduced channel epoch state into ratchet state.
pub fn ratchet_from_epoch_state(state: &ChannelEpochState) -> AmpRatchetState {
    AmpRatchetState {
        chan_epoch: state.chan_epoch,
        last_checkpoint_gen: state.last_checkpoint_gen,
        skip_window: state.skip_window as u64,
        pending_epoch: state.pending_bump.as_ref().map(|p| p.new_epoch),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_journal::fact::ChannelBumpReason;
    use aura_journal::reduction::PendingBump;
    use aura_journal::ChannelEpochState;

    #[test]
    fn ratchet_from_epoch_state_preserves_fields() {
        let state = ChannelEpochState {
            chan_epoch: 3,
            current_gen: 10,
            last_checkpoint_gen: 8,
            skip_window: 64,
            pending_bump: Some(PendingBump {
                parent_epoch: 3,
                new_epoch: 4,
                bump_id: aura_core::Hash32::new([3u8; 32]),
                reason: ChannelBumpReason::Routine,
            }),
            bootstrap: None,
        };

        let ratchet = ratchet_from_epoch_state(&state);
        assert_eq!(ratchet.chan_epoch, 3);
        assert_eq!(ratchet.last_checkpoint_gen, 8);
        assert_eq!(ratchet.skip_window, 64);
        assert_eq!(ratchet.pending_epoch, Some(4));
    }

    #[test]
    fn nonce_from_header_is_deterministic() {
        let header = aura_transport::amp::AmpHeader {
            context: aura_core::identifiers::ContextId::new_from_entropy([4u8; 32]),
            channel: aura_core::identifiers::ChannelId::from_bytes([5u8; 32]),
            chan_epoch: 2,
            ratchet_gen: 7,
        };
        let a = nonce_from_header(&header);
        let b = nonce_from_header(&header);
        assert_eq!(a, b);
    }
}
