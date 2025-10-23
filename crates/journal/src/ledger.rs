// AccountLedger wrapper - coordinates event application and validation
//
// Reference: 080_architecture_protocol_integration.md - Part 3: CRDT Choreography
//
// This module provides a high-level wrapper around AccountState that:
// - Validates events before applying them
// - Maintains event log
// - Provides query methods
// - Handles signature verification

use crate::{
    events::*, state::AccountState, types::*, LedgerError, Result,
};
use ed25519_dalek::{Signature, Verifier};

/// AccountLedger - manages account state and event log
///
/// This is the main interface for interacting with the ledger.
/// It wraps AccountState and provides validation, event logging, and queries.
pub struct AccountLedger {
    /// Current account state (derived from event log)
    state: AccountState,
    
    /// Event log (append-only)
    event_log: Vec<Event>,
}

impl AccountLedger {
    /// Create a new ledger with initial state
    pub fn new(initial_state: AccountState) -> Result<Self> {
        Ok(AccountLedger {
            state: initial_state,
            event_log: Vec::new(),
        })
    }
    
    /// Append and apply an event to the ledger
    ///
    /// Validates the event before applying it to state
    pub fn append_event(&mut self, event: Event) -> Result<()> {
        // Validate event
        self.validate_event(&event)?;
        
        // Apply event to state
        self.state.apply_event(&event)?;
        
        // Append to event log
        self.event_log.push(event);
        
        Ok(())
    }
    
    /// Validate an event before applying
    ///
    /// Checks:
    /// - Signature validity (threshold or device)
    /// - Authorization matches event requirements
    /// - Event-specific preconditions
    fn validate_event(&self, event: &Event) -> Result<()> {
        // Validate authorization
        match &event.authorization {
            EventAuthorization::ThresholdSignature(threshold_sig) => {
                self.validate_threshold_signature(event, threshold_sig)?;
            }
            EventAuthorization::DeviceCertificate { device_id, signature } => {
                self.validate_device_signature(event, *device_id, signature)?;
            }
            EventAuthorization::GuardianSignature { guardian_id, signature } => {
                self.validate_guardian_signature(event, *guardian_id, signature)?;
            }
        }
        
        // Event-specific validation
        if let EventType::EpochTick(e) = &event.event_type {
            self.validate_epoch_tick(e)?;
        }
        // Add more event-specific validation as needed
        
        Ok(())
    }
    
    /// Validate threshold signature on an event
    fn validate_threshold_signature(
        &self,
        event: &Event,
        threshold_sig: &ThresholdSig,
    ) -> Result<()> {
        // Check we have enough signers
        if threshold_sig.signers.len() < self.state.threshold as usize {
            return Err(LedgerError::ThresholdNotMet {
                current: threshold_sig.signers.len(),
                required: self.state.threshold as usize,
            });
        }
        
        // Compute event hash (what was signed)
        let event_hash = event.hash()?;
        
        // Verify signature against group public key
        self.state.group_public_key
            .verify(&event_hash, &threshold_sig.signature)
            .map_err(|_| LedgerError::InvalidSignature)?;
        
        Ok(())
    }
    
    /// Validate device signature on an event
    fn validate_device_signature(
        &self,
        event: &Event,
        device_id: DeviceId,
        signature: &Signature,
    ) -> Result<()> {
        // Get device metadata
        let device = self.state.get_device(&device_id)
            .ok_or_else(|| LedgerError::DeviceNotFound(device_id.to_string()))?;
        
        // Check device is active
        if !self.state.is_device_active(&device_id) {
            return Err(LedgerError::DeviceNotFound(format!(
                "Device {} is not active (tombstoned)",
                device_id
            )));
        }
        
        // Compute event hash (excluding authorization field for signing)
        let event_hash = event.signable_hash()?;
        
        // Verify signature
        device.public_key
            .verify(&event_hash, signature)
            .map_err(|_| LedgerError::InvalidSignature)?;
        
        Ok(())
    }
    
    /// Validate guardian signature on an event
    fn validate_guardian_signature(
        &self,
        _event: &Event,
        guardian_id: GuardianId,
        _signature: &Signature,
    ) -> Result<()> {
        // Get guardian metadata
        let _guardian = self.state.get_guardian(&guardian_id)
            .ok_or_else(|| LedgerError::GuardianNotFound(format!("{:?}", guardian_id)))?;
        
        // TODO: Guardians don't have public keys in current design
        // This needs to be addressed in the spec
        // For now, we just check the guardian exists
        
        Ok(())
    }
    
    /// Validate EpochTick event
    ///
    /// Reference: 080 spec Part 1: Logical Clock for Epoch-Based Timeouts
    fn validate_epoch_tick(&self, tick: &EpochTickEvent) -> Result<()> {
        // Verify new epoch is monotonically increasing
        if tick.new_epoch <= self.state.lamport_clock {
            return Err(LedgerError::StaleEpoch {
                provided: tick.new_epoch,
                current: self.state.lamport_clock,
            });
        }
        
        // TODO: Verify evidence_hash matches current state hash
        // TODO: Implement rate limiting (minimum gap between ticks)
        
        Ok(())
    }
    
    // ========== Query Methods ==========
    
    /// Get current account state
    pub fn state(&self) -> &AccountState {
        &self.state
    }
    
    /// Get mutable account state (for direct mutations in tests)
    #[cfg(test)]
    pub fn state_mut(&mut self) -> &mut AccountState {
        &mut self.state
    }
    
    /// Get current logical epoch
    /// Get current Lamport clock value
    ///
    /// The Lamport clock provides a total ordering of events that respects causality.
    /// Use this for:
    /// - Session timeout checks (start_epoch + ttl < current)
    /// - Reading the current timestamp value
    pub fn lamport_clock(&self) -> u64 {
        self.state.lamport_clock
    }
    
    /// Increment Lamport clock and return new timestamp for locally-created event
    ///
    /// **Important:** Call this BEFORE creating an event on this device.
    /// This ensures proper Lamport timestamp semantics:
    /// - Local events: increment clock first, then create event with new timestamp
    /// - Remote events: receive event, then max(local, received) + 1
    ///
    /// Returns the new Lamport timestamp to use as epoch_at_write in the event.
    pub fn next_lamport_timestamp(&mut self) -> u64 {
        self.state.increment_lamport_clock()
    }
    
    /// Legacy alias for backwards compatibility
    /// TODO: Remove once all code is updated to use lamport_clock()
    pub fn logical_epoch(&self) -> u64 {
        self.lamport_clock()
    }
    
    /// Get last event hash
    pub fn last_event_hash(&self) -> Option<[u8; 32]> {
        self.state.last_event_hash
    }
    
    /// Get event log
    pub fn event_log(&self) -> &[Event] {
        &self.event_log
    }
    
    /// Compute state hash (for EpochTick verification)
    pub fn compute_state_hash(&self) -> Result<[u8; 32]> {
        // Serialize account state and hash
        let serialized = crate::serialization::serialize_cbor(&self.state)?;
        Ok(*blake3::hash(&serialized).as_bytes())
    }
    
    /// Get active operation lock
    pub fn active_operation_lock(&self) -> Option<&OperationLock> {
        self.state.active_operation_lock.as_ref()
    }
    
    /// Check if a specific operation type is locked
    pub fn is_operation_locked(&self, operation_type: OperationType) -> bool {
        self.state.active_operation_lock
            .as_ref()
            .is_some_and(|lock| lock.operation_type == operation_type)
    }
    
    // ========== Session Management ==========
    
    /// Get a session by ID
    pub fn get_session(&self, session_id: &uuid::Uuid) -> Option<&Session> {
        self.state.get_session(session_id)
    }
    
    /// Get all active sessions (non-terminal)
    pub fn active_sessions(&self) -> Vec<&Session> {
        self.state.active_sessions()
    }
    
    /// Get sessions by protocol type
    pub fn sessions_by_protocol(&self, protocol_type: ProtocolType) -> Vec<&Session> {
        self.state.sessions_by_protocol(protocol_type)
    }
    
    /// Check if any session of given protocol type is active
    pub fn has_active_session_of_type(&self, protocol_type: ProtocolType) -> bool {
        self.state.has_active_session_of_type(protocol_type)
    }
    
    /// Add a new session to the ledger
    pub fn add_session(&mut self, session: Session) {
        self.state.add_session(session);
    }
    
    /// Update session status
    pub fn update_session_status(&mut self, session_id: uuid::Uuid, status: SessionStatus) -> Result<()> {
        self.state.update_session_status(session_id, status)
            .map_err(LedgerError::InvalidEvent)
    }
    
    /// Complete a session with outcome
    pub fn complete_session(&mut self, session_id: uuid::Uuid, outcome: SessionOutcome) -> Result<()> {
        self.state.complete_session(session_id, outcome)
            .map_err(LedgerError::InvalidEvent)
    }
    
    /// Abort a session with failure
    pub fn abort_session(&mut self, session_id: uuid::Uuid, reason: String, blamed_party: Option<ParticipantId>) -> Result<()> {
        self.state.abort_session(session_id, reason, blamed_party)
            .map_err(LedgerError::InvalidEvent)
    }
    
    /// Clean up expired sessions based on current epoch
    pub fn cleanup_expired_sessions(&mut self) {
        let current_epoch = self.lamport_clock();
        self.state.cleanup_expired_sessions(current_epoch);
    }
    
    // ========== Compaction Protocol (Part 3: Quorum-Authorized Compaction) ==========
    
    /// Propose compaction of events before a certain epoch
    /// 
    /// This creates a compaction proposal that includes which DKD commitment roots
    /// should be preserved for post-compaction recovery verification.
    pub fn propose_compaction(
        &self,
        before_epoch: u64,
        session_ids_to_preserve: Vec<uuid::Uuid>,
    ) -> Result<CompactionProposal> {
        // Validate proposal
        if before_epoch >= self.lamport_clock() {
            return Err(LedgerError::InvalidEvent(
                "Cannot compact events from current or future epochs".to_string()
            ));
        }
        
        // Collect commitment roots for sessions to preserve
        let mut commitment_roots = Vec::new();
        for session_id in &session_ids_to_preserve {
            if let Some(root) = self.state.dkd_commitment_roots.get(session_id) {
                commitment_roots.push(root.clone());
            }
        }
        
        // Count events that would be compacted
        let events_to_compact = self.event_log.iter()
            .filter(|e| e.epoch_at_write < before_epoch)
            .count();
        
        Ok(CompactionProposal {
            compaction_id: uuid::Uuid::new_v4(),
            compact_before_epoch: before_epoch,
            preserved_roots: commitment_roots,
            events_affected: events_to_compact,
            proposed_at: crate::state::current_timestamp(),
        })
    }
    
    /// Acknowledge a compaction proposal
    /// 
    /// Devices acknowledge they have stored the necessary Merkle proofs
    /// and are ready for the events to be compacted.
    pub fn acknowledge_compaction(
        &self,
        _proposal_id: uuid::Uuid,
        has_required_proofs: bool,
    ) -> Result<()> {
        if !has_required_proofs {
            return Err(LedgerError::InvalidEvent(
                "Cannot acknowledge compaction without required Merkle proofs".to_string()
            ));
        }
        
        // In a real implementation, this would:
        // 1. Verify the device has stored all necessary proofs
        // 2. Track acknowledgements in protocol state
        // 3. Check if threshold acknowledgements reached
        
        Ok(())
    }
    
    /// Commit compaction with threshold authorization
    /// 
    /// After threshold acknowledgements, compaction is committed and
    /// events before the epoch are pruned from the log.
    pub fn commit_compaction(
        &mut self,
        _proposal_id: uuid::Uuid,
        _threshold_signature: crate::ThresholdSig,
    ) -> Result<()> {
        // Validate threshold signature
        // (In practice, this would verify the signature covers the compaction proposal)
        
        // Find the proposal (in a real implementation, this would be tracked)
        let before_epoch = self.lamport_clock().saturating_sub(100); // Placeholder
        
        // Perform the actual pruning
        let _pruned_count = self.prune_events(before_epoch)?;
        
        Ok(())
    }
    
    /// Prune events before the specified epoch
    /// 
    /// This is the actual compaction operation that removes old events
    /// from the event log. Only call after threshold authorization.
    pub fn prune_events(&mut self, before_epoch: u64) -> Result<usize> {
        let initial_count = self.event_log.len();
        
        // Remove events before the compaction epoch
        self.event_log.retain(|event| event.epoch_at_write >= before_epoch);
        
        let final_count = self.event_log.len();
        let pruned_count = initial_count - final_count;
        
        // Note: Compaction complete (logging removed for minimal implementation)
        
        Ok(pruned_count)
    }
    
    /// Get compaction statistics
    pub fn compaction_stats(&self) -> CompactionStats {
        let total_events = self.event_log.len();
        let current_epoch = self.lamport_clock();
        
        // Calculate storage sizes (approximation)
        let estimated_storage_bytes = total_events * 256; // Rough estimate
        
        CompactionStats {
            total_events,
            current_epoch,
            estimated_storage_bytes,
            commitment_roots_count: self.state.dkd_commitment_roots.len(),
        }
    }
}

/// Proposal for ledger compaction
#[derive(Debug, Clone)]
pub struct CompactionProposal {
    pub compaction_id: uuid::Uuid,
    pub compact_before_epoch: u64,
    pub preserved_roots: Vec<crate::DkdCommitmentRoot>,
    pub events_affected: usize,
    pub proposed_at: u64,
}

/// Statistics about ledger compaction
#[derive(Debug, Clone)]
pub struct CompactionStats {
    pub total_events: usize,
    pub current_epoch: u64,
    pub estimated_storage_bytes: usize,
    pub commitment_roots_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    
    fn create_test_ledger() -> AccountLedger {
        let account_id = AccountId::new();
        let signing_key = SigningKey::from_bytes(&rand::random());
        let group_public_key = signing_key.verifying_key();
        let device_id = DeviceId::new();
        
        let device = DeviceMetadata {
            device_id,
            device_name: "Test Device".to_string(),
            device_type: DeviceType::Native,
            public_key: group_public_key,
            added_at: 0,
            last_seen: 0,
            dkd_commitment_proofs: std::collections::BTreeMap::new(),
        };
        
        let state = AccountState::new(account_id, group_public_key, device, 2, 3);
        AccountLedger::new(state).unwrap()
    }
    
    #[test]
    fn test_ledger_creation() {
        let ledger = create_test_ledger();
        assert_eq!(ledger.logical_epoch(), 0);
        assert_eq!(ledger.event_log().len(), 0);
    }
    
    #[test]
    fn test_compute_state_hash() {
        let ledger = create_test_ledger();
        let hash1 = ledger.compute_state_hash().unwrap();
        let hash2 = ledger.compute_state_hash().unwrap();
        
        // Hash should be deterministic
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 32);
    }
    
    #[test]
    fn test_query_methods() {
        let ledger = create_test_ledger();
        
        assert!(!ledger.state().devices.is_empty());
        assert_eq!(ledger.logical_epoch(), 0);
        assert_eq!(ledger.last_event_hash(), None);
        assert!(ledger.active_operation_lock().is_none());
        assert!(!ledger.is_operation_locked(OperationType::Dkd));
    }
}

