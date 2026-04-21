//! Pure AMP helpers.
//!
//! This module contains effect-free utilities for AMP state derivation.

use aura_core::effects::amp::AmpHeader;
use aura_journal::fact::AmpTransitionPolicy;
use aura_journal::reduction::AmpTransitionReductionStatus;
use aura_journal::ChannelEpochState;
use aura_transport::amp::AmpRatchetState;

/// Derive nonce from AMP header using centralized crypto utilities.
pub fn nonce_from_header(header: &AmpHeader) -> [u8; 12] {
    aura_core::crypto::amp::derive_nonce_from_ratchet(header.ratchet_gen, header.chan_epoch)
}

/// Convert reduced channel epoch state into receive ratchet state.
pub fn ratchet_from_epoch_state(state: &ChannelEpochState) -> AmpRatchetState {
    receive_ratchet_from_epoch_state(state)
}

/// Convert reduced channel epoch state into send ratchet state.
pub fn send_ratchet_from_epoch_state(state: &ChannelEpochState) -> AmpRatchetState {
    let send_epoch = state
        .pending_bump
        .as_ref()
        .filter(|_| transition_status(state) == Some(AmpTransitionReductionStatus::A2Live))
        .map(|pending| pending.new_epoch)
        .unwrap_or(state.chan_epoch);

    AmpRatchetState {
        chan_epoch: send_epoch,
        last_checkpoint_gen: state.last_checkpoint_gen,
        skip_window: state.skip_window as u64,
        pending_epoch: None,
    }
}

/// Convert reduced channel epoch state into receive ratchet state.
pub fn receive_ratchet_from_epoch_state(state: &ChannelEpochState) -> AmpRatchetState {
    let Some(pending) = state
        .pending_bump
        .as_ref()
        .filter(|_| transition_status(state) == Some(AmpTransitionReductionStatus::A2Live))
    else {
        return AmpRatchetState {
            chan_epoch: state.chan_epoch,
            last_checkpoint_gen: state.last_checkpoint_gen,
            skip_window: state.skip_window as u64,
            pending_epoch: None,
        };
    };

    match pending.transition_policy {
        AmpTransitionPolicy::AdditiveTransition | AmpTransitionPolicy::NormalTransition => {
            AmpRatchetState {
                chan_epoch: state.chan_epoch,
                last_checkpoint_gen: state.last_checkpoint_gen,
                skip_window: state.skip_window as u64,
                pending_epoch: Some(pending.new_epoch),
            }
        }
        AmpTransitionPolicy::SubtractiveTransition
        | AmpTransitionPolicy::EmergencyCryptoshredTransition => AmpRatchetState {
            chan_epoch: pending.new_epoch,
            last_checkpoint_gen: state.current_gen,
            skip_window: 0,
            pending_epoch: None,
        },
        AmpTransitionPolicy::EmergencyQuarantineTransition => AmpRatchetState {
            chan_epoch: pending.new_epoch,
            last_checkpoint_gen: state.current_gen,
            skip_window: 1,
            pending_epoch: Some(state.chan_epoch),
        },
    }
}

/// Return true when the sender is not explicitly excluded by emergency reducer output.
pub fn sender_allowed_by_epoch_state(
    state: &ChannelEpochState,
    sender: aura_core::types::identifiers::AuthorityId,
) -> bool {
    match state.transition.as_ref() {
        Some(transition) => !transition.emergency_suspects.contains(&sender),
        None => true,
    }
}

fn transition_status(state: &ChannelEpochState) -> Option<AmpTransitionReductionStatus> {
    state
        .transition
        .as_ref()
        .map(|transition| transition.status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
    use aura_core::Hash32;
    use aura_journal::fact::ChannelBumpReason;
    use aura_journal::reduction::{AmpTransitionParentKey, AmpTransitionReduction, PendingBump};
    use aura_journal::ChannelEpochState;
    use std::collections::BTreeSet;

    #[test]
    fn conflict_state_does_not_expose_pending_epoch() {
        let state = channel_state(None, Some(AmpTransitionReductionStatus::A2Conflict));

        let ratchet = ratchet_from_epoch_state(&state);
        assert_eq!(ratchet.chan_epoch, 3);
        assert_eq!(ratchet.last_checkpoint_gen, 8);
        assert_eq!(ratchet.skip_window, 64);
        assert_eq!(ratchet.pending_epoch, None);
    }

    #[test]
    fn send_uses_a2_live_epoch_only_when_reducer_exposes_one() {
        let live = channel_state(
            Some(AmpTransitionPolicy::NormalTransition),
            Some(AmpTransitionReductionStatus::A2Live),
        );
        let conflict = channel_state(
            Some(AmpTransitionPolicy::NormalTransition),
            Some(AmpTransitionReductionStatus::A2Conflict),
        );

        assert_eq!(send_ratchet_from_epoch_state(&live).chan_epoch, 4);
        assert_eq!(send_ratchet_from_epoch_state(&conflict).chan_epoch, 3);
    }

    #[test]
    fn additive_transition_allows_dual_epoch_receive_overlap() {
        let state = channel_state(
            Some(AmpTransitionPolicy::AdditiveTransition),
            Some(AmpTransitionReductionStatus::A2Live),
        );
        let ratchet = receive_ratchet_from_epoch_state(&state);

        assert_eq!(ratchet.chan_epoch, 3);
        assert_eq!(ratchet.pending_epoch, Some(4));
    }

    #[test]
    fn subtractive_transition_cuts_receive_to_successor_epoch() {
        let state = channel_state(
            Some(AmpTransitionPolicy::SubtractiveTransition),
            Some(AmpTransitionReductionStatus::A2Live),
        );
        let ratchet = receive_ratchet_from_epoch_state(&state);

        assert_eq!(ratchet.chan_epoch, 4);
        assert_eq!(ratchet.pending_epoch, None);
        assert_eq!(ratchet.skip_window, 0);
    }

    #[test]
    fn emergency_quarantine_uses_successor_with_minimal_old_epoch_grace() {
        let state = channel_state(
            Some(AmpTransitionPolicy::EmergencyQuarantineTransition),
            Some(AmpTransitionReductionStatus::A2Live),
        );
        let ratchet = receive_ratchet_from_epoch_state(&state);

        assert_eq!(ratchet.chan_epoch, 4);
        assert_eq!(ratchet.pending_epoch, Some(3));
        assert_eq!(ratchet.skip_window, 1);
    }

    #[test]
    fn cryptoshred_transition_cuts_over_at_a2_live_boundary() {
        let state = channel_state(
            Some(AmpTransitionPolicy::EmergencyCryptoshredTransition),
            Some(AmpTransitionReductionStatus::A2Live),
        );
        let ratchet = receive_ratchet_from_epoch_state(&state);

        assert_eq!(ratchet.chan_epoch, 4);
        assert_eq!(ratchet.pending_epoch, None);
        assert_eq!(ratchet.skip_window, 0);
    }

    #[test]
    fn emergency_suspect_is_not_allowed_to_send() {
        let suspect = AuthorityId::new_from_entropy([9u8; 32]);
        let mut state = channel_state(
            Some(AmpTransitionPolicy::EmergencyQuarantineTransition),
            Some(AmpTransitionReductionStatus::A2Live),
        );
        state
            .transition
            .as_mut()
            .unwrap()
            .emergency_suspects
            .insert(suspect);

        assert!(!sender_allowed_by_epoch_state(&state, suspect));
        assert!(sender_allowed_by_epoch_state(
            &state,
            AuthorityId::new_from_entropy([10u8; 32])
        ));
    }

    /// Same header produces the same nonce — required for decryption.
    #[test]
    fn nonce_from_header_is_deterministic() {
        let header = aura_transport::amp::AmpHeader {
            context: aura_core::types::identifiers::ContextId::new_from_entropy([4u8; 32]),
            channel: aura_core::types::identifiers::ChannelId::from_bytes([5u8; 32]),
            chan_epoch: 2,
            ratchet_gen: 7,
        };
        let a = nonce_from_header(&header);
        let b = nonce_from_header(&header);
        assert_eq!(a, b);
    }

    fn channel_state(
        policy: Option<AmpTransitionPolicy>,
        status: Option<AmpTransitionReductionStatus>,
    ) -> ChannelEpochState {
        let transition_id = Hash32::new([4u8; 32]);
        ChannelEpochState {
            chan_epoch: 3,
            current_gen: 10,
            last_checkpoint_gen: 8,
            skip_window: 64,
            pending_bump: policy.map(|transition_policy| PendingBump {
                parent_epoch: 3,
                new_epoch: 4,
                bump_id: Hash32::new([3u8; 32]),
                reason: ChannelBumpReason::Routine,
                transition_id,
                transition_policy,
            }),
            bootstrap: None,
            transition: status.map(|status| AmpTransitionReduction {
                parent: AmpTransitionParentKey {
                    context: ContextId::new_from_entropy([1u8; 32]),
                    channel: ChannelId::from_bytes([2u8; 32]),
                    parent_epoch: 3,
                    parent_commitment: Hash32::new([5u8; 32]),
                },
                status,
                observed_transition_ids: BTreeSet::new(),
                certified_transition_ids: BTreeSet::from([transition_id]),
                finalized_transition_ids: BTreeSet::new(),
                live_transition_id: (status == AmpTransitionReductionStatus::A2Live)
                    .then_some(transition_id),
                finalized_transition_id: None,
                suppressed_transition_ids: BTreeSet::new(),
                conflict_evidence_ids: BTreeSet::new(),
                emergency_alarm_ids: BTreeSet::new(),
                emergency_suspects: BTreeSet::new(),
                quarantine_epochs: BTreeSet::new(),
                prune_before_epochs: BTreeSet::new(),
            }),
        }
    }
}
