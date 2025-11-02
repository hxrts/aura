// Event application logic for AccountState
//
// Reference: 080_architecture_protocol_integration.md - Part 3: CRDT Choreography
//
// This module implements the apply_event() function that takes an Event and
// applies it to the AccountState, handling all 32 event types.

use super::state::AccountState;
use crate::error::{AuraError, Result as AuraResult};
use crate::protocols::*;

impl AccountState {
    /// Apply an event to the account state
    ///
    /// This is the core state transition function that handles all 32 event types.
    /// Each event type updates the relevant part of the account state.
    ///
    /// Reference: 080 spec Part 3: CRDT Choreography & State Management
    pub fn apply_event(&mut self, event: &Event, effects: &aura_crypto::Effects) -> AuraResult<()> {
        // Validate event version
        event
            .validate_version()
            .map_err(|e| AuraError::coordination_failed(e.to_string()))?;

        // Validate nonce to prevent replay attacks
        self.validate_nonce(event.nonce)
            .map_err(|e| AuraError::coordination_failed(e.to_string()))?;

        // Validate parent hash for causal ordering
        event
            .validate_parent(self.last_event_hash)
            .map_err(|e| AuraError::coordination_failed(e.to_string()))?;

        // Advance Lamport clock on every event (Lamport rule: max(local, received) + 1)
        self.advance_lamport_clock(event.epoch_at_write, effects);

        // Apply the specific event type using trait dispatch
        event.event_type.apply_to_state(self, effects)?;

        // Update last event hash for causal chain
        self.last_event_hash = Some(event.hash()?);

        Ok(())
    }
}

// Use aura_crypto::current_timestamp_with_effects directly
