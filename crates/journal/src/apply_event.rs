// Event application logic for AccountState
//
// Reference: 080_architecture_protocol_integration.md - Part 3: CRDT Choreography
//
// This module implements the apply_event() function that takes an Event and
// applies it to the AccountState, handling all 32 event types.

use crate::events::*;
use crate::state::AccountState;
use crate::LedgerError;


impl AccountState {
    /// Apply an event to the account state
    ///
    /// This is the core state transition function that handles all 32 event types.
    /// Each event type updates the relevant part of the account state.
    ///
    /// Reference: 080 spec Part 3: CRDT Choreography & State Management
    pub fn apply_event(&mut self, event: &Event, effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // Validate event version
        event
            .validate_version()
            .map_err(LedgerError::InvalidEvent)?;

        // Validate nonce to prevent replay attacks
        self.validate_nonce(event.nonce)
            .map_err(LedgerError::InvalidEvent)?;

        // Validate parent hash for causal ordering
        event
            .validate_parent(self.last_event_hash)
            .map_err(LedgerError::InvalidEvent)?;

        // Advance Lamport clock on every event (Lamport rule: max(local, received) + 1)
        self.advance_lamport_clock(event.epoch_at_write, effects);

        // Apply the specific event type using trait dispatch
        event.event_type.apply_to_state(self, effects)?;

        // Update last event hash for causal chain
        self.last_event_hash = Some(event.hash()?);

        Ok(())
    }

    
    /// Validate that a capability delegation is authorized
    pub fn validate_capability_delegation(&self, event: &crate::capability::events::CapabilityDelegation, effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        use crate::capability::types::{CapabilityResult, CapabilityScope, Subject};
        
        // Convert issuing device to subject
        let issuer_subject = Subject::new(&event.issued_by.0.to_string());
        
        // For root authorities, only threshold signature authorization is allowed
        if event.parent_id.is_none() {
            return Ok(()); // Root authority creation is always allowed with threshold signature
        }
        
        // For derived capabilities, check that issuer has delegation authority
        let delegation_scope = CapabilityScope::simple("capability", "delegate");
        let result = self.authority_graph.evaluate_capability(&issuer_subject, &delegation_scope, effects);
        
        match result {
            CapabilityResult::Granted => Ok(()),
            CapabilityResult::Revoked => Err(LedgerError::CapabilityError(
                format!("Issuer {} capability was revoked", event.issued_by.0)
            )),
            CapabilityResult::Expired => Err(LedgerError::CapabilityError(
                format!("Issuer {} capability has expired", event.issued_by.0)
            )),
            CapabilityResult::NotFound => Err(LedgerError::CapabilityError(
                format!("Issuer {} does not have delegation authority", event.issued_by.0)
            )),
        }
    }
    
    /// Validate that a capability revocation is authorized
    pub fn validate_capability_revocation(&self, event: &crate::capability::events::CapabilityRevocation, effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        use crate::capability::types::{CapabilityResult, CapabilityScope, Subject};
        
        // Convert issuing device to subject
        let issuer_subject = Subject::new(&event.issued_by.0.to_string());
        
        // Check that issuer has revocation authority
        let revocation_scope = CapabilityScope::simple("capability", "revoke");
        let result = self.authority_graph.evaluate_capability(&issuer_subject, &revocation_scope, effects);
        
        match result {
            CapabilityResult::Granted => Ok(()),
            CapabilityResult::Revoked => Err(LedgerError::CapabilityError(
                format!("Issuer {} capability was revoked", event.issued_by.0)
            )),
            CapabilityResult::Expired => Err(LedgerError::CapabilityError(
                format!("Issuer {} capability has expired", event.issued_by.0)
            )),
            CapabilityResult::NotFound => Err(LedgerError::CapabilityError(
                format!("Issuer {} does not have revocation authority", event.issued_by.0)
            )),
        }
    }

}

/// Get current Unix timestamp in seconds using injected effects
pub fn current_timestamp_with_effects(effects: &aura_crypto::Effects) -> crate::Result<u64> {
    effects.now().map_err(|e| {
        LedgerError::SerializationFailed(format!("Failed to get current timestamp: {}", e))
    })
}

