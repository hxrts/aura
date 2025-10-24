// Trait for applying events to AccountState
//
// This module defines the Appliable trait that allows event payloads to encapsulate
// their own application logic, breaking down the monolithic apply_event function
// into smaller, self-contained units.

use crate::events::*;
use crate::state::AccountState;
use crate::types::*;
use crate::LedgerError;
use tracing::debug;

/// Trait for event payloads that can be applied to AccountState
pub trait Appliable {
    /// Apply this event payload to the account state
    fn apply(&self, state: &mut AccountState, effects: &aura_crypto::Effects) -> Result<(), LedgerError>;
}

/// Extension trait for EventType to apply via trait dispatch
impl EventType {
    pub fn apply_to_state(&self, state: &mut AccountState, effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        match self {
            // ========== Epoch/Clock Management ==========
            EventType::EpochTick(e) => e.apply(state, effects),

            // ========== Distributed Locking ==========
            EventType::RequestOperationLock(e) => e.apply(state, effects),
            EventType::GrantOperationLock(e) => e.apply(state, effects),
            EventType::ReleaseOperationLock(e) => e.apply(state, effects),

            // ========== P2P DKD Protocol ==========
            EventType::InitiateDkdSession(e) => e.apply(state, effects),
            EventType::RecordDkdCommitment(e) => e.apply(state, effects),
            EventType::RevealDkdPoint(e) => e.apply(state, effects),
            EventType::FinalizeDkdSession(e) => e.apply(state, effects),
            EventType::AbortDkdSession(e) => e.apply(state, effects),
            EventType::HealthCheckRequest(e) => e.apply(state, effects),
            EventType::HealthCheckResponse(e) => e.apply(state, effects),

            // ========== P2P Resharing Protocol ==========
            EventType::InitiateResharing(e) => e.apply(state, effects),
            EventType::DistributeSubShare(e) => e.apply(state, effects),
            EventType::AcknowledgeSubShare(e) => e.apply(state, effects),
            EventType::FinalizeResharing(e) => e.apply(state, effects),
            EventType::AbortResharing(e) => e.apply(state, effects),
            EventType::ResharingRollback(e) => e.apply(state, effects),

            // ========== Recovery Protocol ==========
            EventType::InitiateRecovery(e) => e.apply(state, effects),
            EventType::CollectGuardianApproval(e) => e.apply(state, effects),
            EventType::SubmitRecoveryShare(e) => e.apply(state, effects),
            EventType::CompleteRecovery(e) => e.apply(state, effects),
            EventType::AbortRecovery(e) => e.apply(state, effects),
            EventType::NudgeGuardian(e) => e.apply(state, effects),

            // ========== Compaction Protocol ==========
            EventType::ProposeCompaction(e) => e.apply(state, effects),
            EventType::AcknowledgeCompaction(e) => e.apply(state, effects),
            EventType::CommitCompaction(e) => e.apply(state, effects),

            // ========== Device/Guardian Management ==========
            EventType::AddDevice(e) => e.apply(state, effects),
            EventType::RemoveDevice(e) => e.apply(state, effects),
            EventType::AddGuardian(e) => e.apply(state, effects),
            EventType::RemoveGuardian(e) => e.apply(state, effects),

            // ========== Presence ==========
            EventType::PresenceTicketCache(e) => e.apply(state, effects),
            
            // ========== Capabilities ==========
            EventType::CapabilityDelegation(e) => e.apply(state, effects),
            EventType::CapabilityRevocation(e) => e.apply(state, effects),
            
            // ========== CGKA ==========
            EventType::CgkaOperation(e) => e.apply(state, effects),
            EventType::CgkaStateSync(e) => e.apply(state, effects),
            EventType::CgkaEpochTransition(e) => e.apply(state, effects),
        }
    }
}

// Helper macro for getting timestamp from effects
macro_rules! now {
    ($effects:expr) => {
        $effects.now().map_err(|e| LedgerError::InvalidEvent(format!("Failed to get timestamp: {:?}", e)))?
    };
}

// ========== Epoch/Clock Management ==========

impl Appliable for EpochTickEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // Lamport clock already advanced in apply_event()
        // This event just ensures the ledger progresses even when idle
        Ok(())
    }
}

// ========== Distributed Locking ==========

impl Appliable for RequestOperationLockEvent {
    fn apply(&self, state: &mut AccountState, effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // Lock requests are recorded but not yet granted
        // Update session status to Active if it exists
        if let Some(session) = state.sessions.get_mut(&self.session_id) {
            if session.protocol_type == ProtocolType::LockAcquisition {
                let timestamp = now!(effects);
                session.update_status(SessionStatus::Active, timestamp);
            }
        }
        Ok(())
    }
}

impl Appliable for GrantOperationLockEvent {
    fn apply(&self, state: &mut AccountState, effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        let lock = OperationLock {
            operation_type: self.operation_type,
            session_id: SessionId(self.session_id),
            acquired_at: now!(effects),
            expires_at: now!(effects) + 3600, // 1 hour default
            holder: ParticipantId::Device(self.winner_device_id),
            holder_device_id: self.winner_device_id,
            granted_at_epoch: self.granted_at_epoch,
            lottery_ticket: [0u8; 32], // TODO: extract from original request
        };

        state.grant_lock(lock).map_err(LedgerError::InvalidEvent)?;

        // Update session status to Completed for successful lock acquisition
        if let Some(session) = state.sessions.get_mut(&self.session_id) {
            if session.protocol_type == ProtocolType::LockAcquisition {
                let timestamp = now!(effects);
                session.complete(SessionOutcome::Success, timestamp);
            }
        }

        Ok(())
    }
}

impl Appliable for ReleaseOperationLockEvent {
    fn apply(&self, state: &mut AccountState, _effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        state.release_lock(self.session_id)
            .map_err(LedgerError::InvalidEvent)?;

        // Note: Session status remains Completed - lock release doesn't change session status
        // The session represents the lock acquisition process, which is already completed

        Ok(())
    }
}

// ========== P2P DKD Protocol ==========

impl Appliable for InitiateDkdSessionEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // DKD session state is tracked separately (will be in orchestration layer)
        // This event just records the initiation in the ledger
        Ok(())
    }
}

impl Appliable for RecordDkdCommitmentEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // Commitments are recorded for Byzantine detection
        // Actual verification happens in orchestration layer
        Ok(())
    }
}

impl Appliable for RevealDkdPointEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // Points are revealed after all commitments collected
        // Verification happens in orchestration layer
        Ok(())
    }
}

impl Appliable for FinalizeDkdSessionEvent {
    fn apply(&self, state: &mut AccountState, effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // Store commitment root for post-compaction verification
        let root = DkdCommitmentRoot {
            session_id: SessionId(self.session_id),
            root_hash: self.commitment_root,
            commitment_count: 0, // TODO: extract from event
            created_at: now!(effects),
        };

        state.add_commitment_root(root);

        // TODO: Update group public key if this is initial DKD

        Ok(())
    }
}

impl Appliable for AbortDkdSessionEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // Session aborted, no state changes needed
        // Orchestration layer will clean up
        Ok(())
    }
}

impl Appliable for HealthCheckRequestEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // Health check requests are tracked by orchestration layer
        Ok(())
    }
}

impl Appliable for HealthCheckResponseEvent {
    fn apply(&self, state: &mut AccountState, effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // Update device last_seen timestamp
        if let Some(device) = state.devices.get_mut(&self.device_id) {
            device.last_seen = now!(effects);
        }
        Ok(())
    }
}

// ========== P2P Resharing Protocol ==========

impl Appliable for InitiateResharingEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // Resharing session state is tracked separately
        Ok(())
    }
}

impl Appliable for DistributeSubShareEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // Sub-shares are distributed via transport layer
        // This event just records the distribution
        Ok(())
    }
}

impl Appliable for AcknowledgeSubShareEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // Acknowledgements are tracked by orchestration layer
        Ok(())
    }
}

impl Appliable for FinalizeResharingEvent {
    fn apply(&self, state: &mut AccountState, _effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // Update threshold configuration
        state.threshold = self.new_threshold;

        // TODO: Update group public key
        // TODO: Clear old device shares

        Ok(())
    }
}

impl Appliable for AbortResharingEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // Resharing aborted, no state changes
        Ok(())
    }
}

impl Appliable for ResharingRollbackEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // Rollback to previous state (orchestration layer handles this)
        Ok(())
    }
}

// ========== Recovery Protocol ==========

impl Appliable for InitiateRecoveryEvent {
    fn apply(&self, state: &mut AccountState, effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // Start cooldown for recovery
        let cooldown = CooldownCounter {
            participant_id: ParticipantId::Device(self.new_device_id),
            operation_type: OperationType::Recovery,
            count: 1,
            reset_at: now!(effects) + self.cooldown_seconds,
        };

        state.start_cooldown(cooldown, effects);
        Ok(())
    }
}

impl Appliable for CollectGuardianApprovalEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // Guardian approvals are tracked by orchestration layer
        Ok(())
    }
}

impl Appliable for SubmitRecoveryShareEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // Recovery shares are collected by orchestration layer
        Ok(())
    }
}

impl Appliable for CompleteRecoveryEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // Recovery complete, new device should be added via AddDevice event
        // TODO: Verify new device was added
        Ok(())
    }
}

impl Appliable for AbortRecoveryEvent {
    fn apply(&self, state: &mut AccountState, _effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // Remove cooldown
        state.cooldowns.remove(&self.recovery_id);
        Ok(())
    }
}

impl Appliable for NudgeGuardianEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // Nudges are tracked by orchestration layer
        Ok(())
    }
}

// ========== Compaction Protocol ==========

impl Appliable for ProposeCompactionEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // Compaction proposals are tracked by orchestration layer
        Ok(())
    }
}

impl Appliable for AcknowledgeCompactionEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // Acknowledgements are tracked by orchestration layer
        Ok(())
    }
}

impl Appliable for CommitCompactionEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // Compaction committed
        // Actual compaction is performed by ledger layer
        Ok(())
    }
}

// ========== Device/Guardian Management ==========

impl Appliable for AddDeviceEvent {
    fn apply(&self, state: &mut AccountState, effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        use ed25519_dalek::VerifyingKey;

        let public_key =
            VerifyingKey::from_bytes(
                &self.public_key.as_slice().try_into().map_err(|_| {
                    LedgerError::InvalidEvent("Invalid public key length".to_string())
                })?,
            )
            .map_err(|e| LedgerError::InvalidEvent(format!("Invalid public key: {:?}", e)))?;

        let device = DeviceMetadata {
            device_id: self.device_id,
            device_name: self.device_name.clone(),
            device_type: self.device_type,
            public_key,
            added_at: now!(effects),
            last_seen: now!(effects),
            dkd_commitment_proofs: std::collections::BTreeMap::new(),
        };

        state.add_device(device, effects)?;
        Ok(())
    }
}

impl Appliable for RemoveDeviceEvent {
    fn apply(&self, state: &mut AccountState, effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        state.remove_device(self.device_id, effects)?;
        Ok(())
    }
}

impl Appliable for AddGuardianEvent {
    fn apply(&self, state: &mut AccountState, effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // Extract device_id from contact_info or use a placeholder
        // In a real implementation, this would be extracted from the event
        use ed25519_dalek::VerifyingKey;
        #[allow(clippy::unwrap_used)] // Placeholder with known valid data
        let placeholder_key = VerifyingKey::from_bytes(&[0u8; 32]).unwrap();
        
        let guardian = GuardianMetadata {
            guardian_id: self.guardian_id,
            device_id: DeviceId::from_string_with_effects("guardian-device", effects), // TODO: get from event
            email: self.contact_info.email.clone(),
            public_key: placeholder_key, // TODO: get from event
            added_at: now!(effects),
            policy: GuardianPolicy::default(),
        };

        state.add_guardian(guardian, effects)?;
        Ok(())
    }
}

impl Appliable for RemoveGuardianEvent {
    fn apply(&self, state: &mut AccountState, effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        state.remove_guardian(self.guardian_id, effects)?;
        Ok(())
    }
}

// ========== Presence ==========

impl Appliable for PresenceTicketCacheEvent {
    fn apply(&self, state: &mut AccountState, effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        let ticket = PresenceTicketCache {
            device_id: self.device_id,
            session_epoch: SessionEpoch::initial(), // TODO: get from event
            ticket: Vec::new(), // TODO: get from event
            issued_at: self.issued_at,
            expires_at: self.expires_at,
            ticket_digest: self.ticket_digest,
        };

        state.cache_presence_ticket(ticket, effects);
        Ok(())
    }
}

// ========== Capabilities ==========

impl Appliable for crate::capability::events::CapabilityDelegation {
    fn apply(&self, state: &mut AccountState, effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // Validate the delegation is authorized
        state.validate_capability_delegation(self, effects)?;
        
        // Apply delegation to authority graph
        if let Err(e) = state.authority_graph.apply_delegation(self.clone(), effects) {
            return Err(LedgerError::CapabilityError(e.to_string()));
        }
        
        // Update visibility index
        state.visibility_index.update_authority_graph(state.authority_graph.clone(), effects);
        
        Ok(())
    }
}

impl Appliable for crate::capability::events::CapabilityRevocation {
    fn apply(&self, state: &mut AccountState, effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // Validate the revocation is authorized
        state.validate_capability_revocation(self, effects)?;
        
        // Apply revocation to authority graph
        if let Err(e) = state.authority_graph.apply_revocation(self.clone(), effects) {
            return Err(LedgerError::CapabilityError(e.to_string()));
        }
        
        // Update visibility index and handle revocation cascade
        state.visibility_index.update_authority_graph(state.authority_graph.clone(), effects);
        
        Ok(())
    }
}

// ========== CGKA ==========

impl Appliable for CgkaOperationEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // CGKA operations are primarily handled by the CGKA layer
        // Here we just record the operation in the ledger for audit/replay
        
        // TODO: Add CGKA state tracking to AccountState if needed
        // For now, we just acknowledge the operation was processed
        
        debug!("Applied CGKA operation {} for group {} (epoch {} -> {})", 
               self.operation_id, self.group_id, self.current_epoch, self.target_epoch);
        
        Ok(())
    }
}

impl Appliable for CgkaStateSyncEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // State sync events help nodes catch up on CGKA state
        // The actual state is managed by the CGKA layer
        
        debug!("Applied CGKA state sync for group {} at epoch {} with {} members", 
               self.group_id, self.epoch, self.roster_snapshot.len());
        
        Ok(())
    }
}

impl Appliable for CgkaEpochTransitionEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // Epoch transitions mark major CGKA state changes
        // Record the transition for ordering and consistency
        
        debug!("Applied CGKA epoch transition for group {} (epoch {} -> {}) with {} operations", 
               self.group_id, self.previous_epoch, self.new_epoch, self.committed_operations.len());
        
        Ok(())
    }
}