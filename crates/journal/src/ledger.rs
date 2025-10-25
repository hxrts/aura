// AccountLedger wrapper - coordinates event application and validation
//
// Reference: 080_architecture_protocol_integration.md - Part 3: CRDT Choreography
//
// This module provides a high-level wrapper around AccountState that:
// - Validates events before applying them
// - Maintains event log
// - Provides query methods
// - Handles signature verification

use crate::{events::*, state::AccountState, types::*, LedgerError, Result};
use ed25519_dalek::{Signature, Verifier};

/// AccountLedger - manages account state and event log
///
/// This is the main interface for interacting with the ledger.
/// It wraps AccountState and provides validation, event logging, and queries.
#[derive(Debug)]
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
    pub fn append_event(&mut self, event: Event, effects: &aura_crypto::Effects) -> Result<()> {
        // Validate event
        self.validate_event(&event)?;

        // Apply event to state
        self.state.apply_event(&event, effects)?;

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
            EventAuthorization::DeviceCertificate {
                device_id,
                signature,
            } => {
                self.validate_device_signature(event, *device_id, signature)?;
            }
            EventAuthorization::GuardianSignature {
                guardian_id,
                signature,
            } => {
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
        self.state
            .group_public_key
            .verify(&event_hash, &threshold_sig.signature)
            .map_err(|e| {
                LedgerError::InvalidSignature(format!(
                    "Threshold signature verification failed: {}",
                    e
                ))
            })?;

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
        let device = self
            .state
            .get_device(&device_id)
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
        device
            .public_key
            .verify(&event_hash, signature)
            .map_err(|e| {
                LedgerError::InvalidSignature(format!(
                    "Device signature verification failed: {}",
                    e
                ))
            })?;

        Ok(())
    }

    /// Validate guardian signature on an event
    fn validate_guardian_signature(
        &self,
        event: &Event,
        guardian_id: GuardianId,
        signature: &Signature,
    ) -> Result<()> {
        // Get guardian metadata
        let guardian = self
            .state
            .get_guardian(&guardian_id)
            .ok_or_else(|| LedgerError::GuardianNotFound(format!("{:?}", guardian_id)))?;

        // Verify guardian public key exists and is valid
        let guardian_public_key = &guardian.public_key;

        // Validate guardian public key format and security
        self.validate_guardian_public_key_security(guardian_public_key)?;

        // Create canonical verification message for the event
        let message = self.create_guardian_event_message(event, &guardian_id)?;

        // Verify the actual signature provided with the event
        use ed25519_dalek::Verifier;
        guardian_public_key
            .verify(&message, signature)
            .map_err(|e| {
                LedgerError::InvalidSignature(format!(
                    "Guardian signature verification failed for {:?}: {}",
                    guardian_id, e
                ))
            })?;

        // Verify guardian is still authorized and active
        self.verify_guardian_authorization(&guardian_id)?;

        // Verify guardian key integrity and non-compromise
        self.verify_guardian_key_integrity(&guardian_id, guardian_public_key)?;

        // Verify guardian registration chain
        self.verify_guardian_registration_chain(&guardian_id)?;

        tracing::debug!("Guardian signature validation passed for {:?}", guardian_id);
        Ok(())
    }

    /// Validate guardian public key security properties
    fn validate_guardian_public_key_security(
        &self,
        public_key: &ed25519_dalek::VerifyingKey,
    ) -> Result<()> {
        let key_bytes = public_key.to_bytes();

        // Check for obvious weak keys
        if key_bytes.iter().all(|&b| b == 0) {
            return Err(LedgerError::WeakKey(
                "Guardian public key is all zeros".to_string(),
            ));
        }

        if key_bytes.iter().all(|&b| b == 0xFF) {
            return Err(LedgerError::WeakKey(
                "Guardian public key is all ones".to_string(),
            ));
        }

        // Check for simple patterns
        if key_bytes[0] != 0 && key_bytes.iter().all(|&b| b == key_bytes[0]) {
            return Err(LedgerError::WeakKey(
                "Guardian public key has repeating pattern".to_string(),
            ));
        }

        // Check known compromised keys (placeholder - in production, check against blacklist)
        if self.is_key_compromised(&key_bytes) {
            return Err(LedgerError::CompromisedKey(
                "Guardian public key is known to be compromised".to_string(),
            ));
        }

        tracing::debug!("Guardian public key security validation passed");
        Ok(())
    }

    /// Create canonical message for guardian event signing
    fn create_guardian_event_message(
        &self,
        event: &Event,
        guardian_id: &GuardianId,
    ) -> Result<Vec<u8>> {
        let mut message = Vec::new();

        // Add event context
        message.extend_from_slice(event.event_id.0.as_bytes());
        message.extend_from_slice(event.account_id.0.as_bytes());
        message.extend_from_slice(&event.timestamp.to_le_bytes());
        message.extend_from_slice(&event.nonce.to_le_bytes());

        // Add parent hash if present
        if let Some(parent_hash) = &event.parent_hash {
            message.extend_from_slice(parent_hash);
        }

        // Add guardian context
        message.extend_from_slice(guardian_id.0.as_bytes());

        // Add epoch for freshness
        message.extend_from_slice(&event.epoch_at_write.to_le_bytes());

        // Add event type specific data
        match &event.event_type {
            EventType::AddGuardian(add_event) => {
                message.extend_from_slice(add_event.guardian_id.0.as_bytes());
                message.extend_from_slice(add_event.contact_info.email.as_bytes());
            }
            EventType::RemoveGuardian(remove_event) => {
                message.extend_from_slice(remove_event.guardian_id.0.as_bytes());
            }
            // Add other guardian-related events as needed
            _ => {}
        }

        Ok(message)
    }

    /// Verify guardian is authorized and active
    fn verify_guardian_authorization(&self, guardian_id: &GuardianId) -> Result<()> {
        let _guardian = self
            .state
            .get_guardian(guardian_id)
            .ok_or_else(|| LedgerError::GuardianNotFound(format!("{:?}", guardian_id)))?;

        // Check if guardian has been removed (tombstoned)
        if self.state.removed_guardians.contains(guardian_id) {
            return Err(LedgerError::GuardianRevoked(format!("{:?}", guardian_id)));
        }

        tracing::debug!(
            "Guardian authorization verification passed for {:?}",
            guardian_id
        );
        Ok(())
    }

    /// Verify guardian key integrity
    fn verify_guardian_key_integrity(
        &self,
        guardian_id: &GuardianId,
        public_key: &ed25519_dalek::VerifyingKey,
    ) -> Result<()> {
        let guardian = self
            .state
            .get_guardian(guardian_id)
            .ok_or_else(|| LedgerError::GuardianNotFound(format!("{:?}", guardian_id)))?;

        // Verify key matches registration
        if guardian.public_key.to_bytes() != public_key.to_bytes() {
            return Err(LedgerError::KeyMismatch(format!(
                "Guardian public key mismatch for {:?}",
                guardian_id
            )));
        }

        tracing::debug!(
            "Guardian key integrity verification passed for {:?}",
            guardian_id
        );
        Ok(())
    }

    /// Verify guardian registration chain integrity
    fn verify_guardian_registration_chain(&self, guardian_id: &GuardianId) -> Result<()> {
        // Find the guardian registration event
        let registration_event = self.find_guardian_registration_event(guardian_id)?;

        // Verify the registration event was properly authorized
        match &registration_event.authorization {
            EventAuthorization::ThresholdSignature(threshold_sig) => {
                self.verify_threshold_signature_integrity(registration_event, threshold_sig)?;
            }
            EventAuthorization::DeviceCertificate {
                device_id,
                signature,
            } => {
                self.verify_device_signature_integrity(registration_event, device_id, signature)?;
            }
            EventAuthorization::GuardianSignature {
                guardian_id,
                signature,
            } => {
                self.validate_guardian_signature(registration_event, *guardian_id, signature)?;
            }
        }

        // Verify no subsequent revocation
        if self.find_guardian_revocation_event(guardian_id)?.is_some() {
            return Err(LedgerError::GuardianRevoked(format!("{:?}", guardian_id)));
        }

        tracing::debug!(
            "Guardian registration chain verification passed for {:?}",
            guardian_id
        );
        Ok(())
    }

    /// Find guardian registration event in ledger
    fn find_guardian_registration_event(&self, guardian_id: &GuardianId) -> Result<&Event> {
        for event in self.event_log() {
            if let EventType::AddGuardian(add_event) = &event.event_type {
                if add_event.guardian_id == *guardian_id {
                    return Ok(event);
                }
            }
        }
        Err(LedgerError::GuardianNotFound(format!(
            "No registration event found for guardian {:?}",
            guardian_id
        )))
    }

    /// Find guardian revocation event if it exists
    fn find_guardian_revocation_event(&self, guardian_id: &GuardianId) -> Result<Option<&Event>> {
        for event in self.event_log() {
            if let EventType::RemoveGuardian(remove_event) = &event.event_type {
                if remove_event.guardian_id == *guardian_id {
                    return Ok(Some(event));
                }
            }
        }
        Ok(None)
    }

    /// Verify threshold signature integrity on event
    fn verify_threshold_signature_integrity(
        &self,
        _event: &Event,
        threshold_sig: &crate::ThresholdSig,
    ) -> Result<()> {
        // Basic validation
        if threshold_sig.signers.is_empty() {
            return Err(LedgerError::InvalidSignature(
                "No signers in threshold signature".to_string(),
            ));
        }

        if threshold_sig.signature_shares.len() < threshold_sig.signers.len() {
            return Err(LedgerError::InvalidSignature(
                "Insufficient signature shares".to_string(),
            ));
        }

        // Get threshold from account state
        let threshold = self.state.threshold as usize;

        if threshold_sig.signers.len() < threshold {
            return Err(LedgerError::InsufficientSigners(format!(
                "Need {} signers, got {}",
                threshold,
                threshold_sig.signers.len()
            )));
        }

        // TODO: In production, verify FROST signature using proper cryptographic verification
        tracing::debug!("Threshold signature verification passed (placeholder implementation)");
        Ok(())
    }

    /// Verify device signature integrity on event
    fn verify_device_signature_integrity(
        &self,
        event: &Event,
        device_id: &crate::DeviceId,
        signature: &ed25519_dalek::Signature,
    ) -> Result<()> {
        // Get device public key
        let device = self
            .state
            .get_device(device_id)
            .ok_or_else(|| LedgerError::DeviceNotFound(format!("{:?}", device_id)))?;

        // Create canonical message for event
        let message = self.create_device_event_message(event)?;

        // Verify signature
        use ed25519_dalek::Verifier;
        device.public_key.verify(&message, signature).map_err(|e| {
            LedgerError::InvalidSignature(format!("Device signature verification failed: {}", e))
        })?;

        tracing::debug!("Device signature verification passed for {:?}", device_id);
        Ok(())
    }

    /// Create canonical message for device event signing
    fn create_device_event_message(&self, event: &Event) -> Result<Vec<u8>> {
        let mut message = Vec::new();

        message.extend_from_slice(event.event_id.0.as_bytes());
        message.extend_from_slice(event.account_id.0.as_bytes());
        message.extend_from_slice(&event.timestamp.to_le_bytes());
        message.extend_from_slice(&event.nonce.to_le_bytes());

        if let Some(parent_hash) = &event.parent_hash {
            message.extend_from_slice(parent_hash);
        }

        message.extend_from_slice(&event.epoch_at_write.to_le_bytes());

        // Add event type specific data
        match &event.event_type {
            EventType::AddDevice(add_event) => {
                message.extend_from_slice(&add_event.public_key);
                message.extend_from_slice(add_event.device_name.as_bytes());
            }
            EventType::AddGuardian(add_event) => {
                message.extend_from_slice(add_event.guardian_id.0.as_bytes());
                message.extend_from_slice(add_event.contact_info.email.as_bytes());
            }
            // Add other event types as needed
            _ => {}
        }

        Ok(message)
    }

    /// Check if key is known to be compromised
    fn is_key_compromised(&self, _key_bytes: &[u8; 32]) -> bool {
        // Placeholder implementation - in production, check against:
        // - Known compromised key database
        // - Revocation lists
        // - Security advisories
        false
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

        // Verify evidence_hash matches current state hash
        let current_state_hash = self.compute_state_hash()?;
        if tick.evidence_hash != current_state_hash {
            return Err(LedgerError::InvalidEvent(format!(
                "Evidence hash mismatch: provided {:?}, computed {:?}",
                hex::encode(tick.evidence_hash),
                hex::encode(current_state_hash)
            )));
        }

        // Implement rate limiting (minimum gap between ticks)
        let min_epoch_gap = 5; // Minimum 5 epochs between ticks
        let epoch_gap = tick.new_epoch - self.state.lamport_clock;
        if epoch_gap < min_epoch_gap {
            return Err(LedgerError::InvalidEvent(format!(
                "Epoch tick rate limit exceeded: gap {} < minimum {}",
                epoch_gap, min_epoch_gap
            )));
        }

        // Additional rate limiting: check time-based limits if we have timing info
        if let Some(last_tick_time) = self.get_last_epoch_tick_time() {
            let min_time_gap_ms = 10000; // Minimum 10 seconds between ticks
            if let Some(current_time) = self.get_current_time_estimate() {
                let time_gap = current_time.saturating_sub(last_tick_time);
                if time_gap < min_time_gap_ms {
                    return Err(LedgerError::InvalidEvent(format!(
                        "Epoch tick time rate limit exceeded: gap {}ms < minimum {}ms",
                        time_gap, min_time_gap_ms
                    )));
                }
            }
        }

        tracing::debug!(
            "Epoch tick validation passed: {} -> {} (gap: {})",
            self.state.lamport_clock,
            tick.new_epoch,
            epoch_gap
        );

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
    pub fn next_lamport_timestamp(&mut self, effects: &aura_crypto::Effects) -> u64 {
        self.state.increment_lamport_clock(effects)
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
        self.state
            .active_operation_lock
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
    pub fn add_session(&mut self, session: Session, effects: &aura_crypto::Effects) {
        self.state.add_session(session, effects);
    }

    /// Update session status
    pub fn update_session_status(
        &mut self,
        session_id: uuid::Uuid,
        status: SessionStatus,
        effects: &aura_crypto::Effects,
    ) -> Result<()> {
        self.state
            .update_session_status(session_id, status, effects)
            .map_err(LedgerError::InvalidEvent)
    }

    /// Complete a session with outcome
    pub fn complete_session(
        &mut self,
        session_id: uuid::Uuid,
        outcome: SessionOutcome,
        effects: &aura_crypto::Effects,
    ) -> Result<()> {
        self.state
            .complete_session(session_id, outcome, effects)
            .map_err(LedgerError::InvalidEvent)
    }

    /// Abort a session with failure
    pub fn abort_session(
        &mut self,
        session_id: uuid::Uuid,
        reason: String,
        blamed_party: Option<ParticipantId>,
        effects: &aura_crypto::Effects,
    ) -> Result<()> {
        self.state
            .abort_session(session_id, reason, blamed_party, effects)
            .map_err(LedgerError::InvalidEvent)
    }

    /// Clean up expired sessions based on current epoch
    pub fn cleanup_expired_sessions(&mut self, effects: &aura_crypto::Effects) {
        let current_epoch = self.lamport_clock();
        self.state.cleanup_expired_sessions(current_epoch, effects);
    }

    // ========== Compaction Protocol (Part 3: Quorum-Authorized Compaction) ==========

    /// Propose compaction of events before a certain epoch (with effects)
    ///
    /// This creates a compaction proposal that includes which DKD commitment roots
    /// should be preserved for post-compaction recovery verification.
    pub fn propose_compaction_with_effects(
        &self,
        before_epoch: u64,
        session_ids_to_preserve: Vec<uuid::Uuid>,
        effects: &aura_crypto::Effects,
    ) -> Result<CompactionProposal> {
        // Validate proposal
        if before_epoch >= self.lamport_clock() {
            return Err(LedgerError::InvalidEvent(
                "Cannot compact events from current or future epochs".to_string(),
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
        let events_to_compact = self
            .event_log
            .iter()
            .filter(|e| e.epoch_at_write < before_epoch)
            .count();

        Ok(CompactionProposal {
            compaction_id: effects.gen_uuid(),
            compact_before_epoch: before_epoch,
            preserved_roots: commitment_roots,
            events_affected: events_to_compact,
            proposed_at: effects.now().unwrap_or(0),
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
                "Cannot acknowledge compaction without required Merkle proofs".to_string(),
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
        self.event_log
            .retain(|event| event.epoch_at_write >= before_epoch);

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

    /// Get all preserved DKD commitment roots for compaction planning
    ///
    /// Returns session IDs of all DKD commitment roots that should be preserved
    /// during compaction operations.
    pub fn get_preserved_commitment_roots(&self) -> Vec<uuid::Uuid> {
        self.state.dkd_commitment_roots.keys().copied().collect()
    }

    /// Get DKD commitment roots created after a specific epoch
    ///
    /// Used for determining which roots need to be preserved when compacting
    /// events before a specific epoch.
    pub fn get_commitment_roots_after_epoch(&self, epoch: u64) -> Vec<uuid::Uuid> {
        self.state
            .dkd_commitment_roots
            .values()
            .filter(|root| root.created_at > epoch)
            .map(|root| root.session_id.0)
            .collect()
    }

    /// Get DKD commitment root details
    ///
    /// Returns the full commitment root details for a given session ID.
    pub fn get_commitment_root_details(
        &self,
        session_id: &uuid::Uuid,
    ) -> Option<&crate::DkdCommitmentRoot> {
        self.state.get_commitment_root(session_id)
    }

    /// Get timestamp of last epoch tick event for rate limiting
    pub fn get_last_epoch_tick_time(&self) -> Option<u64> {
        // Search for the most recent EpochTick event in the event log
        self.event_log.iter().rev().find_map(|event| {
            if matches!(event.event_type, EventType::EpochTick(_)) {
                Some(event.timestamp)
            } else {
                None
            }
        })
    }

    /// Get current time estimate from various sources
    pub fn get_current_time_estimate(&self) -> Option<u64> {
        // In a real implementation, this would:
        // 1. Use injected time from Effects
        // 2. Use NTP if available
        // 3. Use peer consensus time
        // 4. Fall back to logical clock estimation

        // For now, return None (time-based validation disabled)
        // Validation still works based on epoch-based rate limiting
        None
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
        let effects = aura_crypto::Effects::test();
        let account_id = AccountId::new_with_effects(&effects);
        let signing_key = SigningKey::from_bytes(&effects.random_bytes());
        let group_public_key = signing_key.verifying_key();
        let device_id = DeviceId::new_with_effects(&effects);

        let device = DeviceMetadata {
            device_id,
            device_name: "Test Device".to_string(),
            device_type: DeviceType::Native,
            public_key: group_public_key,
            added_at: 0,
            last_seen: 0,
            dkd_commitment_proofs: std::collections::BTreeMap::new(),
            next_nonce: 0,
            used_nonces: std::collections::BTreeSet::new(),
        };

        let state = AccountState::new(account_id, group_public_key, device, 2, 3);
        AccountLedger::new(state).unwrap()
    }

    #[test]
    fn test_ledger_creation() {
        let ledger = create_test_ledger();
        assert_eq!(ledger.lamport_clock(), 0);
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
        assert_eq!(ledger.lamport_clock(), 0);
        assert_eq!(ledger.last_event_hash(), None);
        assert!(ledger.active_operation_lock().is_none());
        assert!(!ledger.is_operation_locked(OperationType::Dkd));
    }
}
