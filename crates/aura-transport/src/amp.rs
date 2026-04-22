//! AMP Transport Types (Layer 2 - Pure Domain Types)
//!
//! This module provides the core AMP (Asynchronous Message Protocol) transport types
//! without dependencies on journal or authorization domains.

#![allow(missing_docs)] // Macro-generated variants/fields

pub use aura_core::effects::amp::AmpHeader;
use aura_core::types::identifiers::{ChannelId, ContextId};
use aura_core::Hash32;
use aura_macros::aura_error_types;
use serde::{Deserialize, Serialize};

/// Simplified ratchet state for AMP operations
///
/// Contains only the fields needed for AMP transport operations, extracted
/// from the journal domain state to maintain clean architectural boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AmpRatchetState {
    /// Current channel epoch
    pub chan_epoch: u64,
    /// Last checkpoint generation
    pub last_checkpoint_gen: u64,
    /// Skip window size for out-of-order messages
    pub skip_window: u64,
    /// Pending epoch (for epoch transitions)
    pub pending_epoch: Option<u64>,
}

/// Public AMP message identifier derived from wire-visible header fields.
///
/// This is not secret key material and must never be used as an AEAD key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AmpMessageId(pub Hash32);

/// Derived ratchet state and public message identifier
///
/// Result of deriving the ratchet state for a message. Contains the header, a
/// public message identifier, and the next generation counter to advance to.
///
/// The derivation intentionally does not expose AEAD key material. Message
/// encryption keys must be derived by the protocol layer from secure channel
/// epoch or bootstrap secrets.
///
/// ```compile_fail
/// # use aura_core::effects::amp::AmpHeader;
/// # use aura_core::types::identifiers::{ChannelId, ContextId};
/// # use aura_core::Hash32;
/// # use aura_transport::amp::{AmpMessageId, RatchetDerivation};
/// # let derivation = RatchetDerivation {
/// #     header: AmpHeader {
/// #         context: ContextId::new_from_entropy([1u8; 32]),
/// #         channel: ChannelId::from_bytes([2u8; 32]),
/// #         chan_epoch: 0,
/// #         ratchet_gen: 0,
/// #     },
/// #     message_id: AmpMessageId(Hash32::default()),
/// #     next_gen: 1,
/// # };
/// let _aead_key = derivation.message_key;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RatchetDerivation {
    /// The derived AMP header for this message
    pub header: AmpHeader,
    /// Public message identifier derived from the header. Not an AEAD key.
    pub message_id: AmpMessageId,
    /// The next generation counter to advance to after this message
    pub next_gen: u64,
}

aura_error_types! {
    /// Error categories for AMP ratchet operations
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[allow(missing_docs)]
    pub enum AmpError {
        #[category = "protocol"]
        EpochMismatch { got: u64, current: u64, pending: Option<u64> } =>
            "Channel epoch mismatch: got {got}, current {current}, pending {pending:?}",

        #[category = "protocol"]
        GenerationOutOfWindow { gen: u64, min: u64, max: u64 } =>
            "Generation {gen} outside window [{min}, {max}]",

        #[category = "system"]
        Core { details: String } =>
            "Core error: {details}",
    }
}

/// Calculate the valid generation window for message acceptance
pub fn window_bounds(last_checkpoint: u64, skip_window: u64) -> (u64, u64) {
    let min_gen = last_checkpoint;
    let max_gen = last_checkpoint.saturating_add(skip_window.saturating_mul(2));
    (min_gen, max_gen)
}

/// Derive AMP header and message key for sending
///
/// This is a pure function that only handles the cryptographic derivation.
/// The caller (in aura-protocol) is responsible for managing state updates.
pub fn derive_for_send(
    context: ContextId,
    channel: ChannelId,
    state: &AmpRatchetState,
    ratchet_gen: u64,
) -> Result<RatchetDerivation, AmpError> {
    // Validate generation is within acceptable bounds
    let (min_gen, max_gen) = window_bounds(state.last_checkpoint_gen, state.skip_window);

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

    // Derive public message identifier from header components.
    let message_id = derive_message_id(&header);

    Ok(RatchetDerivation {
        header,
        message_id,
        next_gen: ratchet_gen.saturating_add(1),
    })
}

/// Validate receive header and derive message key
///
/// This is a pure function that only validates and derives keys.
/// The caller (in aura-protocol) is responsible for state management.
pub fn derive_for_recv(
    state: &AmpRatchetState,
    header: AmpHeader,
) -> Result<RatchetDerivation, AmpError> {
    // Validate epoch
    let valid_epoch = header.chan_epoch == state.chan_epoch
        || state.pending_epoch.is_some_and(|e| header.chan_epoch == e);

    if !valid_epoch {
        return Err(AmpError::EpochMismatch {
            got: header.chan_epoch,
            current: state.chan_epoch,
            pending: state.pending_epoch,
        });
    }

    // Validate generation window
    let (min_gen, max_gen) = window_bounds(state.last_checkpoint_gen, state.skip_window);
    if header.ratchet_gen < min_gen || header.ratchet_gen > max_gen {
        return Err(AmpError::GenerationOutOfWindow {
            gen: header.ratchet_gen,
            min: min_gen,
            max: max_gen,
        });
    }

    // Derive public message identifier
    let message_id = derive_message_id(&header);

    Ok(RatchetDerivation {
        header,
        message_id,
        next_gen: header.ratchet_gen.saturating_add(1),
    })
}

/// Derive a public message identifier from AMP header
///
/// This identifier is deterministic and public. It is useful for deduplication
/// and tracing, but it is not secret key material.
fn derive_message_id(header: &AmpHeader) -> AmpMessageId {
    // Construct identifier material from header components
    let mut material = Vec::with_capacity(16 + 32 + 8 + 8);
    material.extend_from_slice(&header.context.to_bytes());
    material.extend_from_slice(header.channel.as_bytes());
    material.extend_from_slice(&header.chan_epoch.to_le_bytes());
    material.extend_from_slice(&header.ratchet_gen.to_le_bytes());

    AmpMessageId(Hash32::from_bytes(&material))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_bounds() {
        let (min, max) = window_bounds(10, 4);
        assert_eq!(min, 10);
        assert_eq!(max, 18); // 10 + (2 * 4)
    }

    #[test]
    fn test_derive_for_send() {
        let context = ContextId::new_from_entropy([2u8; 32]);
        let channel = ChannelId::from_bytes([1u8; 32]);
        let state = AmpRatchetState {
            chan_epoch: 0,
            last_checkpoint_gen: 0,
            skip_window: 4,
            pending_epoch: None,
        };

        let result = derive_for_send(context, channel, &state, 2).unwrap();
        assert_eq!(result.header.context, context);
        assert_eq!(result.header.channel, channel);
        assert_eq!(result.header.chan_epoch, 0);
        assert_eq!(result.header.ratchet_gen, 2);
        assert_eq!(result.next_gen, 3);
    }

    #[test]
    fn test_derive_for_recv_valid() {
        let state = AmpRatchetState {
            chan_epoch: 0,
            last_checkpoint_gen: 0,
            skip_window: 4,
            pending_epoch: None,
        };

        let header = AmpHeader {
            context: ContextId::new_from_entropy([2u8; 32]),
            channel: ChannelId::from_bytes([1u8; 32]),
            chan_epoch: 0,
            ratchet_gen: 2,
        };

        let result = derive_for_recv(&state, header).unwrap();
        assert_eq!(result.header, header);
        assert_eq!(result.next_gen, 3);
    }

    #[test]
    fn test_derive_for_recv_epoch_mismatch() {
        let state = AmpRatchetState {
            chan_epoch: 0,
            last_checkpoint_gen: 0,
            skip_window: 4,
            pending_epoch: None,
        };

        let header = AmpHeader {
            context: ContextId::new_from_entropy([2u8; 32]),
            channel: ChannelId::from_bytes([1u8; 32]),
            chan_epoch: 1, // Wrong epoch
            ratchet_gen: 2,
        };

        let result = derive_for_recv(&state, header);
        assert!(matches!(result, Err(AmpError::EpochMismatch { .. })));
    }

    #[test]
    fn test_derive_for_recv_generation_out_of_window() {
        let state = AmpRatchetState {
            chan_epoch: 0,
            last_checkpoint_gen: 0,
            skip_window: 4,
            pending_epoch: None,
        };

        let header = AmpHeader {
            context: ContextId::new_from_entropy([2u8; 32]),
            channel: ChannelId::from_bytes([1u8; 32]),
            chan_epoch: 0,
            ratchet_gen: 20, // Outside window [0, 8]
        };

        let result = derive_for_recv(&state, header);
        assert!(matches!(
            result,
            Err(AmpError::GenerationOutOfWindow { .. })
        ));
    }

    #[test]
    fn test_message_id_derivation_deterministic() {
        let header = AmpHeader {
            context: ContextId::new_from_entropy([2u8; 32]),
            channel: ChannelId::from_bytes([1u8; 32]),
            chan_epoch: 0,
            ratchet_gen: 1,
        };

        let id1 = derive_message_id(&header);
        let id2 = derive_message_id(&header);
        assert_eq!(id1, id2);
    }
}
