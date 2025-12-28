//! AMP Transport Types (Layer 2 - Pure Domain Types)
//!
//! This module provides the core AMP (Asynchronous Message Protocol) transport types
//! without dependencies on journal or authorization domains. Domain-specific logic
//! has been moved to aura-protocol (Layer 4) where it belongs.

#![allow(missing_docs)] // Macro-generated variants/fields

use aura_core::identifiers::{ChannelId, ContextId};
use aura_core::Hash32;
use aura_macros::aura_error_types;
use serde::{Deserialize, Serialize};

/// AMP message header used as AEAD associated data
///
/// Contains the contextual and ratchet state information that uniquely identifies
/// a message in the AMP (Asynchronous Message Protocol) system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

/// Derived ratchet state and message key
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
    let max_gen = last_checkpoint + (2 * skip_window);
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

    // Derive message key from header components
    let message_key = derive_message_key(&header);

    Ok(RatchetDerivation {
        header,
        message_key,
        next_gen: ratchet_gen + 1,
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

    // Derive message key
    let message_key = derive_message_key(&header);

    Ok(RatchetDerivation {
        header,
        message_key,
        next_gen: header.ratchet_gen + 1,
    })
}

/// Derive message key from AMP header
///
/// Pure cryptographic function that derives AEAD keys from message metadata.
/// Uses a deterministic KDF based on the header components.
fn derive_message_key(header: &AmpHeader) -> Hash32 {
    // Construct key material from header components
    let mut material = Vec::with_capacity(16 + 32 + 8 + 8);
    material.extend_from_slice(&header.context.to_bytes());
    material.extend_from_slice(header.channel.as_bytes());
    material.extend_from_slice(&header.chan_epoch.to_le_bytes());
    material.extend_from_slice(&header.ratchet_gen.to_le_bytes());

    // Hash the material to derive the key
    Hash32::from_bytes(&material)
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
        let context = ContextId::default();
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
            context: ContextId::default(),
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
            context: ContextId::default(),
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
            context: ContextId::default(),
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
    fn test_message_key_derivation_deterministic() {
        let header = AmpHeader {
            context: ContextId::default(),
            channel: ChannelId::from_bytes([1u8; 32]),
            chan_epoch: 0,
            ratchet_gen: 1,
        };

        let key1 = derive_message_key(&header);
        let key2 = derive_message_key(&header);
        assert_eq!(key1, key2);
    }
}
