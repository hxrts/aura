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

impl AccountState {
    /// Apply an event to the account state
    ///
    /// This is the core state transition function that handles all 32 event types.
    /// Each event type updates the relevant part of the account state.
    ///
    /// Reference: 080 spec Part 3: CRDT Choreography & State Management
    pub fn apply_event(&mut self, event: &Event) -> Result<(), LedgerError> {
        // Validate event version
        event.validate_version().map_err(LedgerError::InvalidEvent)?;
        
        // Validate nonce to prevent replay attacks
        self.validate_nonce(event.nonce).map_err(LedgerError::InvalidEvent)?;
        
        // Validate parent hash for causal ordering
        event.validate_parent(self.last_event_hash)
            .map_err(LedgerError::InvalidEvent)?;
        
        // Advance Lamport clock on every event (Lamport rule: max(local, received) + 1)
        self.advance_lamport_clock(event.epoch_at_write);
        
        // Apply the specific event type
        match &event.event_type {
            // ========== Epoch/Clock Management ==========
            EventType::EpochTick(e) => self.apply_epoch_tick(e),
            
            // ========== Distributed Locking ==========
            EventType::RequestOperationLock(e) => self.apply_request_operation_lock(e),
            EventType::GrantOperationLock(e) => self.apply_grant_operation_lock(e),
            EventType::ReleaseOperationLock(e) => self.apply_release_operation_lock(e),
            
            // ========== P2P DKD Protocol ==========
            EventType::InitiateDkdSession(e) => self.apply_initiate_dkd_session(e),
            EventType::RecordDkdCommitment(e) => self.apply_record_dkd_commitment(e),
            EventType::RevealDkdPoint(e) => self.apply_reveal_dkd_point(e),
            EventType::FinalizeDkdSession(e) => self.apply_finalize_dkd_session(e),
            EventType::AbortDkdSession(e) => self.apply_abort_dkd_session(e),
            EventType::HealthCheckRequest(e) => self.apply_health_check_request(e),
            EventType::HealthCheckResponse(e) => self.apply_health_check_response(e),
            
            // ========== P2P Resharing Protocol ==========
            EventType::InitiateResharing(e) => self.apply_initiate_resharing(e),
            EventType::DistributeSubShare(e) => self.apply_distribute_sub_share(e),
            EventType::AcknowledgeSubShare(e) => self.apply_acknowledge_sub_share(e),
            EventType::FinalizeResharing(e) => self.apply_finalize_resharing(e),
            EventType::AbortResharing(e) => self.apply_abort_resharing(e),
            EventType::ResharingRollback(e) => self.apply_resharing_rollback(e),
            
            // ========== Recovery Protocol ==========
            EventType::InitiateRecovery(e) => self.apply_initiate_recovery(e),
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
            EventType::AddDevice(e) => self.apply_add_device(e),
            EventType::RemoveDevice(e) => self.apply_remove_device(e),
            EventType::AddGuardian(e) => self.apply_add_guardian(e),
            EventType::RemoveGuardian(e) => self.apply_remove_guardian(e),
            
            // ========== Presence & Policy ==========
            EventType::PresenceTicketCache(e) => self.apply_presence_ticket_cache(e),
            EventType::PolicyUpdate(e) => self.apply_policy_update(e),
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
    
    fn apply_request_operation_lock(&mut self, event: &RequestOperationLockEvent) -> Result<(), LedgerError> {
        // Lock requests are recorded but not yet granted
        // Update session status to Active if it exists
        if let Some(session) = self.sessions.get_mut(&event.session_id) {
            if session.protocol_type == ProtocolType::LockAcquisition {
                let timestamp = current_timestamp()?;
                session.update_status(SessionStatus::Active, timestamp);
            }
        }
        Ok(())
    }
    
    fn apply_grant_operation_lock(&mut self, event: &GrantOperationLockEvent) -> Result<(), LedgerError> {
        let lock = OperationLock {
            operation_type: event.operation_type,
            session_id: event.session_id,
            holder_device_id: event.winner_device_id,
            granted_at_epoch: event.granted_at_epoch,
            lottery_ticket: [0u8; 32], // TODO: extract from original request
        };
        
        self.grant_lock(lock).map_err(LedgerError::InvalidEvent)?;
        
        // Update session status to Completed for successful lock acquisition
        if let Some(session) = self.sessions.get_mut(&event.session_id) {
            if session.protocol_type == ProtocolType::LockAcquisition {
                let timestamp = current_timestamp()?;
                session.complete(SessionOutcome::Success, timestamp);
            }
        }
        
        Ok(())
    }
    
    fn apply_release_operation_lock(&mut self, event: &ReleaseOperationLockEvent) -> Result<(), LedgerError> {
        self.release_lock(event.session_id).map_err(LedgerError::InvalidEvent)?;
        
        // Note: Session status remains Completed - lock release doesn't change session status
        // The session represents the lock acquisition process, which is already completed
        
        Ok(())
    }
    
    fn apply_initiate_dkd_session(&mut self, _event: &InitiateDkdSessionEvent) -> Result<(), LedgerError> {
        // DKD session state is tracked separately (will be in orchestration layer)
        // This event just records the initiation in the ledger
        Ok(())
    }
    
    fn apply_record_dkd_commitment(&mut self, _event: &RecordDkdCommitmentEvent) -> Result<(), LedgerError> {
        // Commitments are recorded for Byzantine detection
        // Actual verification happens in orchestration layer
        Ok(())
    }
    
    fn apply_reveal_dkd_point(&mut self, _event: &RevealDkdPointEvent) -> Result<(), LedgerError> {
        // Points are revealed after all commitments collected
        // Verification happens in orchestration layer
        Ok(())
    }
    
    fn apply_finalize_dkd_session(&mut self, event: &FinalizeDkdSessionEvent) -> Result<(), LedgerError> {
        // Store commitment root for post-compaction verification
        let root = DkdCommitmentRoot {
            session_id: event.session_id,
            merkle_root: event.commitment_root,
            created_at: current_timestamp()?,
        };
        
        self.add_commitment_root(root);
        
        // TODO: Update group public key if this is initial DKD
        
        Ok(())
    }
    
    fn apply_abort_dkd_session(&mut self, _event: &AbortDkdSessionEvent) -> Result<(), LedgerError> {
        // Session aborted, no state changes needed
        // Orchestration layer will clean up
        Ok(())
    }
    
    fn apply_health_check_request(&mut self, _event: &HealthCheckRequestEvent) -> Result<(), LedgerError> {
        // Health check requests are tracked by orchestration layer
        Ok(())
    }
    
    fn apply_health_check_response(&mut self, event: &HealthCheckResponseEvent) -> Result<(), LedgerError> {
        // Update device last_seen timestamp
        if let Some(device) = self.devices.get_mut(&event.device_id) {
            device.last_seen = current_timestamp()?;
        }
        Ok(())
    }
    
    fn apply_initiate_resharing(&mut self, _event: &InitiateResharingEvent) -> Result<(), LedgerError> {
        // Resharing session state is tracked separately
        Ok(())
    }
    
    fn apply_distribute_sub_share(&mut self, _event: &DistributeSubShareEvent) -> Result<(), LedgerError> {
        // Sub-shares are distributed via transport layer
        // This event just records the distribution
        Ok(())
    }
    
    fn apply_acknowledge_sub_share(&mut self, _event: &AcknowledgeSubShareEvent) -> Result<(), LedgerError> {
        // Acknowledgements are tracked by orchestration layer
        Ok(())
    }
    
    fn apply_finalize_resharing(&mut self, event: &FinalizeResharingEvent) -> Result<(), LedgerError> {
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
    
    fn apply_resharing_rollback(&mut self, _event: &ResharingRollbackEvent) -> Result<(), LedgerError> {
        // Rollback to previous state (orchestration layer handles this)
        Ok(())
    }
    
    fn apply_initiate_recovery(&mut self, event: &InitiateRecoveryEvent) -> Result<(), LedgerError> {
        // Start cooldown for recovery
        let cooldown = CooldownCounter {
            operation_id: event.recovery_id,
            started_at: current_timestamp()?,
            duration_seconds: event.cooldown_seconds,
            can_cancel: true,
        };
        
        self.start_cooldown(cooldown);
        Ok(())
    }
    
    fn apply_collect_guardian_approval(&mut self, _event: &CollectGuardianApprovalEvent) -> Result<(), LedgerError> {
        // Guardian approvals are tracked by orchestration layer
        Ok(())
    }
    
    fn apply_submit_recovery_share(&mut self, _event: &SubmitRecoveryShareEvent) -> Result<(), LedgerError> {
        // Recovery shares are collected by orchestration layer
        Ok(())
    }
    
    fn apply_complete_recovery(&mut self, _event: &CompleteRecoveryEvent) -> Result<(), LedgerError> {
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
    
    fn apply_propose_compaction(&mut self, _event: &ProposeCompactionEvent) -> Result<(), LedgerError> {
        // Compaction proposals are tracked by orchestration layer
        Ok(())
    }
    
    fn apply_acknowledge_compaction(&mut self, _event: &AcknowledgeCompactionEvent) -> Result<(), LedgerError> {
        // Acknowledgements are tracked by orchestration layer
        Ok(())
    }
    
    fn apply_commit_compaction(&mut self, _event: &CommitCompactionEvent) -> Result<(), LedgerError> {
        // Compaction committed
        // Actual compaction is performed by ledger layer
        Ok(())
    }
    
    fn apply_add_device(&mut self, event: &AddDeviceEvent) -> Result<(), LedgerError> {
        use ed25519_dalek::VerifyingKey;
        
        let public_key = VerifyingKey::from_bytes(&event.public_key.as_slice().try_into().map_err(|_| {
            LedgerError::InvalidEvent("Invalid public key length".to_string())
        })?)
        .map_err(|e| LedgerError::InvalidEvent(format!("Invalid public key: {:?}", e)))?;
        
        let device = DeviceMetadata {
            device_id: event.device_id,
            device_name: event.device_name.clone(),
            device_type: event.device_type,
            public_key,
            added_at: current_timestamp()?,
            last_seen: current_timestamp()?,
            dkd_commitment_proofs: std::collections::BTreeMap::new(),
        };
        
        self.add_device(device)?;
        Ok(())
    }
    
    fn apply_remove_device(&mut self, event: &RemoveDeviceEvent) -> Result<(), LedgerError> {
        self.remove_device(event.device_id)?;
        Ok(())
    }
    
    fn apply_add_guardian(&mut self, event: &AddGuardianEvent) -> Result<(), LedgerError> {
        let guardian = GuardianMetadata {
            guardian_id: event.guardian_id,
            account_id: self.account_id,
            contact_info: event.contact_info.clone(),
            added_at: current_timestamp()?,
            share_envelope_cid: Some(event.encrypted_share_cid.clone()),
        };
        
        self.add_guardian(guardian)?;
        Ok(())
    }
    
    fn apply_remove_guardian(&mut self, event: &RemoveGuardianEvent) -> Result<(), LedgerError> {
        self.remove_guardian(event.guardian_id)?;
        Ok(())
    }
    
    fn apply_presence_ticket_cache(&mut self, event: &PresenceTicketCacheEvent) -> Result<(), LedgerError> {
        let ticket = PresenceTicketCache {
            device_id: event.device_id,
            issued_at: event.issued_at,
            expires_at: event.expires_at,
            ticket_digest: event.ticket_digest,
        };
        
        self.cache_presence_ticket(ticket);
        Ok(())
    }
    
    fn apply_policy_update(&mut self, event: &PolicyUpdateEvent) -> Result<(), LedgerError> {
        let policy = PolicyReference {
            policy_cid: event.policy_cid.0.clone(),
            version: event.version,
            updated_at: current_timestamp()?,
        };
        
        self.update_policy(policy);
        Ok(())
    }
}

/// Get current Unix timestamp in seconds
pub fn current_timestamp() -> crate::Result<u64> {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|e| LedgerError::SerializationFailed(format!(
            "System time is before UNIX epoch: {}",
            e
        )))
}

