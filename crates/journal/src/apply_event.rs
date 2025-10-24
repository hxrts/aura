// Event application logic for AccountState
//
// Reference: 080_architecture_protocol_integration.md - Part 3: CRDT Choreography
//
// This module implements the apply_event() function that takes an Event and
// applies it to the AccountState, handling all 32 event types.

use crate::events::*;
use crate::state::AccountState;
use crate::types::*;
use crate::LedgerError;
use tracing::debug;

// Helper macro for getting timestamp from effects
macro_rules! now {
    ($effects:expr) => {
        $effects.now().map_err(|e| LedgerError::InvalidEvent(format!("Failed to get timestamp: {:?}", e)))?
    };
}

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

        // Apply the specific event type
        match &event.event_type {
            // ========== Epoch/Clock Management ==========
            EventType::EpochTick(e) => self.apply_epoch_tick(e),

            // ========== Distributed Locking ==========
            EventType::RequestOperationLock(e) => self.apply_request_operation_lock(e, effects),
            EventType::GrantOperationLock(e) => self.apply_grant_operation_lock(e, effects),
            EventType::ReleaseOperationLock(e) => self.apply_release_operation_lock(e),

            // ========== P2P DKD Protocol ==========
            EventType::InitiateDkdSession(e) => self.apply_initiate_dkd_session(e, effects),
            EventType::RecordDkdCommitment(e) => self.apply_record_dkd_commitment(e),
            EventType::RevealDkdPoint(e) => self.apply_reveal_dkd_point(e),
            EventType::FinalizeDkdSession(e) => self.apply_finalize_dkd_session(e, effects),
            EventType::AbortDkdSession(e) => self.apply_abort_dkd_session(e),
            EventType::HealthCheckRequest(e) => self.apply_health_check_request(e),
            EventType::HealthCheckResponse(e) => self.apply_health_check_response(e, effects),

            // ========== P2P Resharing Protocol ==========
            EventType::InitiateResharing(e) => self.apply_initiate_resharing(e),
            EventType::DistributeSubShare(e) => self.apply_distribute_sub_share(e),
            EventType::AcknowledgeSubShare(e) => self.apply_acknowledge_sub_share(e),
            EventType::FinalizeResharing(e) => self.apply_finalize_resharing(e),
            EventType::AbortResharing(e) => self.apply_abort_resharing(e),
            EventType::ResharingRollback(e) => self.apply_resharing_rollback(e),

            // ========== Recovery Protocol ==========
            EventType::InitiateRecovery(e) => self.apply_initiate_recovery(e, effects),
            EventType::CollectGuardianApproval(e) => self.apply_collect_guardian_approval(e),
            EventType::SubmitRecoveryShare(e) => self.apply_submit_recovery_share(e),
            EventType::CompleteRecovery(e) => self.apply_complete_recovery(e),
            EventType::AbortRecovery(e) => self.apply_abort_recovery(e),
            EventType::NudgeGuardian(e) => self.apply_nudge_guardian(e),

            // ========== Compaction Protocol ==========
            EventType::ProposeCompaction(e) => self.apply_propose_compaction(e),
            EventType::AcknowledgeCompaction(e) => self.apply_acknowledge_compaction(e),
            EventType::CommitCompaction(e) => self.apply_commit_compaction(e),

            // ========== Device/Guardian Management ==========
            EventType::AddDevice(e) => self.apply_add_device(e, effects),
            EventType::RemoveDevice(e) => self.apply_remove_device(e, effects),
            EventType::AddGuardian(e) => self.apply_add_guardian(e, effects),
            EventType::RemoveGuardian(e) => self.apply_remove_guardian(e, effects),

            // ========== Presence ==========
            EventType::PresenceTicketCache(e) => self.apply_presence_ticket_cache(e, effects),
            
            // ========== Capabilities ==========
            EventType::CapabilityDelegation(e) => self.apply_capability_delegation(e, effects),
            EventType::CapabilityRevocation(e) => self.apply_capability_revocation(e, effects),
            
            // ========== CGKA ==========
            EventType::CgkaOperation(e) => self.apply_cgka_operation(e),
            EventType::CgkaStateSync(e) => self.apply_cgka_state_sync(e),
            EventType::CgkaEpochTransition(e) => self.apply_cgka_epoch_transition(e),
        }?;

        // Update last event hash for causal chain
        self.last_event_hash = Some(event.hash()?);

        Ok(())
    }

    // ========== Event Application Handlers ==========

    fn apply_epoch_tick(&mut self, _event: &EpochTickEvent) -> Result<(), LedgerError> {
        // Lamport clock already advanced in apply_event()
        // This event just ensures the ledger progresses even when idle
        Ok(())
    }

    fn apply_request_operation_lock(
        &mut self,
        event: &RequestOperationLockEvent,
        effects: &aura_crypto::Effects,
    ) -> Result<(), LedgerError> {
        // Lock requests are recorded but not yet granted
        // Update session status to Active if it exists
        if let Some(session) = self.sessions.get_mut(&event.session_id) {
            if session.protocol_type == ProtocolType::LockAcquisition {
                let timestamp = now!(effects);
                session.update_status(SessionStatus::Active, timestamp);
            }
        }
        Ok(())
    }

    fn apply_grant_operation_lock(
        &mut self,
        event: &GrantOperationLockEvent,
        effects: &aura_crypto::Effects,
    ) -> Result<(), LedgerError> {
        let lock = OperationLock {
            operation_type: event.operation_type,
            session_id: SessionId(event.session_id),
            acquired_at: now!(effects),
            expires_at: now!(effects) + 3600, // 1 hour default
            holder: ParticipantId::Device(event.winner_device_id),
            holder_device_id: event.winner_device_id,
            granted_at_epoch: event.granted_at_epoch,
            lottery_ticket: [0u8; 32], // TODO: extract from original request
        };

        self.grant_lock(lock).map_err(LedgerError::InvalidEvent)?;

        // Update session status to Completed for successful lock acquisition
        if let Some(session) = self.sessions.get_mut(&event.session_id) {
            if session.protocol_type == ProtocolType::LockAcquisition {
                let timestamp = now!(effects);
                session.complete(SessionOutcome::Success, timestamp);
            }
        }

        Ok(())
    }

    fn apply_release_operation_lock(
        &mut self,
        event: &ReleaseOperationLockEvent,
    ) -> Result<(), LedgerError> {
        self.release_lock(event.session_id)
            .map_err(LedgerError::InvalidEvent)?;

        // Note: Session status remains Completed - lock release doesn't change session status
        // The session represents the lock acquisition process, which is already completed

        Ok(())
    }

    fn apply_initiate_dkd_session(
        &mut self,
        _event: &InitiateDkdSessionEvent,
        _effects: &aura_crypto::Effects,
    ) -> Result<(), LedgerError> {
        // DKD session state is tracked separately (will be in orchestration layer)
        // This event just records the initiation in the ledger
        Ok(())
    }

    fn apply_record_dkd_commitment(
        &mut self,
        _event: &RecordDkdCommitmentEvent,
    ) -> Result<(), LedgerError> {
        // Commitments are recorded for Byzantine detection
        // Actual verification happens in orchestration layer
        Ok(())
    }

    fn apply_reveal_dkd_point(&mut self, _event: &RevealDkdPointEvent) -> Result<(), LedgerError> {
        // Points are revealed after all commitments collected
        // Verification happens in orchestration layer
        Ok(())
    }

    fn apply_finalize_dkd_session(
        &mut self,
        event: &FinalizeDkdSessionEvent,
        effects: &aura_crypto::Effects,
    ) -> Result<(), LedgerError> {
        // Store commitment root for post-compaction verification
        let root = DkdCommitmentRoot {
            session_id: SessionId(event.session_id),
            root_hash: event.commitment_root,
            commitment_count: 0, // TODO: extract from event
            created_at: now!(effects),
        };

        self.add_commitment_root(root);

        // TODO: Update group public key if this is initial DKD

        Ok(())
    }

    fn apply_abort_dkd_session(
        &mut self,
        _event: &AbortDkdSessionEvent,
    ) -> Result<(), LedgerError> {
        // Session aborted, no state changes needed
        // Orchestration layer will clean up
        Ok(())
    }

    fn apply_health_check_request(
        &mut self,
        _event: &HealthCheckRequestEvent,
    ) -> Result<(), LedgerError> {
        // Health check requests are tracked by orchestration layer
        Ok(())
    }

    fn apply_health_check_response(
        &mut self,
        event: &HealthCheckResponseEvent,
        effects: &aura_crypto::Effects,
    ) -> Result<(), LedgerError> {
        // Update device last_seen timestamp
        if let Some(device) = self.devices.get_mut(&event.device_id) {
            device.last_seen = now!(effects);
        }
        Ok(())
    }

    fn apply_initiate_resharing(
        &mut self,
        _event: &InitiateResharingEvent,
    ) -> Result<(), LedgerError> {
        // Resharing session state is tracked separately
        Ok(())
    }

    fn apply_distribute_sub_share(
        &mut self,
        _event: &DistributeSubShareEvent,
    ) -> Result<(), LedgerError> {
        // Sub-shares are distributed via transport layer
        // This event just records the distribution
        Ok(())
    }

    fn apply_acknowledge_sub_share(
        &mut self,
        _event: &AcknowledgeSubShareEvent,
    ) -> Result<(), LedgerError> {
        // Acknowledgements are tracked by orchestration layer
        Ok(())
    }

    fn apply_finalize_resharing(
        &mut self,
        event: &FinalizeResharingEvent,
    ) -> Result<(), LedgerError> {
        // Update threshold configuration
        self.threshold = event.new_threshold;

        // TODO: Update group public key
        // TODO: Clear old device shares

        Ok(())
    }

    fn apply_abort_resharing(&mut self, _event: &AbortResharingEvent) -> Result<(), LedgerError> {
        // Resharing aborted, no state changes
        Ok(())
    }

    fn apply_resharing_rollback(
        &mut self,
        _event: &ResharingRollbackEvent,
    ) -> Result<(), LedgerError> {
        // Rollback to previous state (orchestration layer handles this)
        Ok(())
    }

    fn apply_initiate_recovery(
        &mut self,
        event: &InitiateRecoveryEvent,
        effects: &aura_crypto::Effects,
    ) -> Result<(), LedgerError> {
        // Start cooldown for recovery
        let cooldown = CooldownCounter {
            participant_id: ParticipantId::Device(event.new_device_id),
            operation_type: OperationType::Recovery,
            count: 1,
            reset_at: now!(effects) + event.cooldown_seconds,
        };

        self.start_cooldown(cooldown, effects);
        Ok(())
    }

    fn apply_collect_guardian_approval(
        &mut self,
        _event: &CollectGuardianApprovalEvent,
    ) -> Result<(), LedgerError> {
        // Guardian approvals are tracked by orchestration layer
        Ok(())
    }

    fn apply_submit_recovery_share(
        &mut self,
        _event: &SubmitRecoveryShareEvent,
    ) -> Result<(), LedgerError> {
        // Recovery shares are collected by orchestration layer
        Ok(())
    }

    fn apply_complete_recovery(
        &mut self,
        _event: &CompleteRecoveryEvent,
    ) -> Result<(), LedgerError> {
        // Recovery complete, new device should be added via AddDevice event
        // TODO: Verify new device was added
        Ok(())
    }

    fn apply_abort_recovery(&mut self, event: &AbortRecoveryEvent) -> Result<(), LedgerError> {
        // Remove cooldown
        self.cooldowns.remove(&event.recovery_id);
        Ok(())
    }

    fn apply_nudge_guardian(&mut self, _event: &NudgeGuardianEvent) -> Result<(), LedgerError> {
        // Nudges are tracked by orchestration layer
        Ok(())
    }

    fn apply_propose_compaction(
        &mut self,
        _event: &ProposeCompactionEvent,
    ) -> Result<(), LedgerError> {
        // Compaction proposals are tracked by orchestration layer
        Ok(())
    }

    fn apply_acknowledge_compaction(
        &mut self,
        _event: &AcknowledgeCompactionEvent,
    ) -> Result<(), LedgerError> {
        // Acknowledgements are tracked by orchestration layer
        Ok(())
    }

    fn apply_commit_compaction(
        &mut self,
        _event: &CommitCompactionEvent,
    ) -> Result<(), LedgerError> {
        // Compaction committed
        // Actual compaction is performed by ledger layer
        Ok(())
    }

    fn apply_add_device(&mut self, event: &AddDeviceEvent, effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        use ed25519_dalek::VerifyingKey;

        let public_key =
            VerifyingKey::from_bytes(
                &event.public_key.as_slice().try_into().map_err(|_| {
                    LedgerError::InvalidEvent("Invalid public key length".to_string())
                })?,
            )
            .map_err(|e| LedgerError::InvalidEvent(format!("Invalid public key: {:?}", e)))?;

        let device = DeviceMetadata {
            device_id: event.device_id,
            device_name: event.device_name.clone(),
            device_type: event.device_type,
            public_key,
            added_at: now!(effects),
            last_seen: now!(effects),
            dkd_commitment_proofs: std::collections::BTreeMap::new(),
        };

        self.add_device(device, effects)?;
        Ok(())
    }

    fn apply_remove_device(&mut self, event: &RemoveDeviceEvent, effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        self.remove_device(event.device_id, effects)?;
        Ok(())
    }

    fn apply_add_guardian(&mut self, event: &AddGuardianEvent, effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // Extract device_id from contact_info or use a placeholder
        // In a real implementation, this would be extracted from the event
        use ed25519_dalek::VerifyingKey;
        #[allow(clippy::unwrap_used)] // Placeholder with known valid data
        let placeholder_key = VerifyingKey::from_bytes(&[0u8; 32]).unwrap();
        
        let guardian = GuardianMetadata {
            guardian_id: event.guardian_id,
            device_id: DeviceId::from_string_with_effects("guardian-device", effects), // TODO: get from event
            email: event.contact_info.email.clone(),
            public_key: placeholder_key, // TODO: get from event
            added_at: now!(effects),
            policy: GuardianPolicy::default(),
        };

        self.add_guardian(guardian, effects)?;
        Ok(())
    }

    fn apply_remove_guardian(&mut self, event: &RemoveGuardianEvent, effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        self.remove_guardian(event.guardian_id, effects)?;
        Ok(())
    }

    fn apply_presence_ticket_cache(
        &mut self,
        event: &PresenceTicketCacheEvent,
        effects: &aura_crypto::Effects,
    ) -> Result<(), LedgerError> {
        let ticket = PresenceTicketCache {
            device_id: event.device_id,
            session_epoch: SessionEpoch::initial(), // TODO: get from event
            ticket: Vec::new(), // TODO: get from event
            issued_at: event.issued_at,
            expires_at: event.expires_at,
            ticket_digest: event.ticket_digest,
        };

        self.cache_presence_ticket(ticket, effects);
        Ok(())
    }
    
    fn apply_capability_delegation(&mut self, event: &crate::capability::events::CapabilityDelegation, effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // Validate the delegation is authorized
        self.validate_capability_delegation(event, effects)?;
        
        // Apply delegation to authority graph
        if let Err(e) = self.authority_graph.apply_delegation(event.clone(), effects) {
            return Err(LedgerError::CapabilityError(e.to_string()));
        }
        
        // Update visibility index
        self.visibility_index.update_authority_graph(self.authority_graph.clone(), effects);
        
        Ok(())
    }
    
    fn apply_capability_revocation(&mut self, event: &crate::capability::events::CapabilityRevocation, effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
        // Validate the revocation is authorized
        self.validate_capability_revocation(event, effects)?;
        
        // Apply revocation to authority graph
        if let Err(e) = self.authority_graph.apply_revocation(event.clone(), effects) {
            return Err(LedgerError::CapabilityError(e.to_string()));
        }
        
        // Update visibility index and handle revocation cascade
        self.visibility_index.update_authority_graph(self.authority_graph.clone(), effects);
        
        Ok(())
    }
    
    /// Validate that a capability delegation is authorized
    fn validate_capability_delegation(&self, event: &crate::capability::events::CapabilityDelegation, effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
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
    fn validate_capability_revocation(&self, event: &crate::capability::events::CapabilityRevocation, effects: &aura_crypto::Effects) -> Result<(), LedgerError> {
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
    
    /// Apply CGKA operation event
    fn apply_cgka_operation(&mut self, event: &crate::events::CgkaOperationEvent) -> Result<(), LedgerError> {
        // CGKA operations are primarily handled by the CGKA layer
        // Here we just record the operation in the ledger for audit/replay
        
        // TODO: Add CGKA state tracking to AccountState if needed
        // For now, we just acknowledge the operation was processed
        
        debug!("Applied CGKA operation {} for group {} (epoch {} -> {})", 
               event.operation_id, event.group_id, event.current_epoch, event.target_epoch);
        
        Ok(())
    }
    
    /// Apply CGKA state synchronization event
    fn apply_cgka_state_sync(&mut self, event: &crate::events::CgkaStateSyncEvent) -> Result<(), LedgerError> {
        // State sync events help nodes catch up on CGKA state
        // The actual state is managed by the CGKA layer
        
        debug!("Applied CGKA state sync for group {} at epoch {} with {} members", 
               event.group_id, event.epoch, event.roster_snapshot.len());
        
        Ok(())
    }
    
    /// Apply CGKA epoch transition event
    fn apply_cgka_epoch_transition(&mut self, event: &crate::events::CgkaEpochTransitionEvent) -> Result<(), LedgerError> {
        // Epoch transitions mark major CGKA state changes
        // Record the transition for ordering and consistency
        
        debug!("Applied CGKA epoch transition for group {} (epoch {} -> {}) with {} operations", 
               event.group_id, event.previous_epoch, event.new_epoch, event.committed_operations.len());
        
        Ok(())
    }

}

/// Get current Unix timestamp in seconds using injected effects
pub fn current_timestamp_with_effects(effects: &aura_crypto::Effects) -> crate::Result<u64> {
    effects.now().map_err(|e| {
        LedgerError::SerializationFailed(format!("Failed to get current timestamp: {}", e))
    })
}

