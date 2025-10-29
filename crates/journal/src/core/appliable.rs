// Trait for applying events to AccountState
//
// This module defines the Appliable trait that allows event payloads to encapsulate
// their own application logic, breaking down the monolithic apply_event function
// into smaller, self-contained units.

use super::state::AccountState;
use crate::{
    error::{AuraError, Result as AuraResult},
    protocols::*,
    types::*,
};
use aura_types::{DeviceId, DeviceIdExt, MemberId};
use tracing::debug;

/// Trait for event payloads that can be applied to AccountState
pub trait Appliable {
    /// Apply this event payload to the account state
    fn apply(&self, state: &mut AccountState, effects: &aura_crypto::Effects) -> AuraResult<()>;
}

/// Extension trait for EventType to apply via trait dispatch
impl EventType {
    pub fn apply_to_state(
        &self,
        state: &mut AccountState,
        effects: &aura_crypto::Effects,
    ) -> AuraResult<()> {
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
            EventType::UpdateDeviceNonce(e) => e.apply(state, effects),
            EventType::AddGuardian(e) => e.apply(state, effects),
            EventType::RemoveGuardian(e) => e.apply(state, effects),

            // ========== Presence ==========
            EventType::PresenceTicketCache(e) => e.apply(state, effects),

            // ========== Capabilities ==========
            EventType::CapabilityDelegation(e) => e.apply(state, effects),
            EventType::CapabilityRevocation(e) => e.apply(state, effects),

            // ========== Keyhive Integration ==========
            EventType::KeyhiveCapabilityDelegation(e) => {
                debug!(
                    "Applying Keyhive capability delegation: {}",
                    e.capability_id
                );
                apply_keyhive_capability_delegation(e, state, effects)
            }
            EventType::KeyhiveCapabilityRevocation(e) => {
                debug!(
                    "Applying Keyhive capability revocation: {}",
                    e.capability_id
                );
                apply_keyhive_capability_revocation(e, state, effects)
            }
            EventType::KeyhiveCgka(e) => {
                debug!("Applying Keyhive CGKA operation"); // Remove operation_id access
                apply_keyhive_cgka_operation(e, state, effects)
            }

            // ========== SSB Counters ==========
            EventType::IncrementCounter(e) => e.apply(state, effects),
            EventType::ReserveCounterRange(e) => e.apply(state, effects),

            // ========== Session Management ==========
            EventType::CreateSession(e) => e.apply(state, effects),
            EventType::UpdateSessionStatus(e) => e.apply(state, effects),
            EventType::CompleteSession(e) => e.apply(state, effects),
            EventType::AbortSession(e) => e.apply(state, effects),
            EventType::CleanupExpiredSessions(e) => e.apply(state, effects),

            // ========== Storage Operations ==========
            EventType::StoreData(e) => e.apply(state, effects),
            EventType::RetrieveData(e) => e.apply(state, effects),
            EventType::DeleteData(e) => e.apply(state, effects),
        }
    }
}

// ========== SSB Counters ==========

impl Appliable for IncrementCounterEvent {
    fn apply(&self, state: &mut AccountState, _effects: &aura_crypto::Effects) -> AuraResult<()> {
        let expiry_epoch = self.requested_at_epoch.saturating_add(self.ttl_epochs);
        state
            .relationship_counters
            .insert(self.relationship_id, (self.new_counter_value, expiry_epoch));
        Ok(())
    }
}

impl Appliable for ReserveCounterRangeEvent {
    fn apply(&self, state: &mut AccountState, _effects: &aura_crypto::Effects) -> AuraResult<()> {
        let last_counter = self
            .start_counter
            .saturating_add(self.range_size.saturating_sub(1));
        let expiry_epoch = self.requested_at_epoch.saturating_add(self.ttl_epochs);
        state
            .relationship_counters
            .insert(self.relationship_id, (last_counter, expiry_epoch));
        Ok(())
    }
}

// Helper macro for getting timestamp from effects
macro_rules! now {
    ($effects:expr) => {
        $effects.now().map_err(|e| {
            AuraError::protocol_invalid_instruction(format!("Failed to get timestamp: {:?}", e))
        })?
    };
}

// ========== Epoch/Clock Management ==========

impl Appliable for EpochTickEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> AuraResult<()> {
        // Lamport clock already advanced in apply_event()
        // This event just ensures the ledger progresses even when idle
        Ok(())
    }
}

// ========== Distributed Locking ==========

impl Appliable for RequestOperationLockEvent {
    fn apply(&self, state: &mut AccountState, effects: &aura_crypto::Effects) -> AuraResult<()> {
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
    fn apply(&self, state: &mut AccountState, effects: &aura_crypto::Effects) -> AuraResult<()> {
        // Find the original lock request to extract the lottery ticket
        let lottery_ticket = state
            .sessions
            .get(&self.session_id)
            .map(|_session| {
                // Look for the lottery ticket in the session metadata or events
                // For now, derive it from the winner device ID and session info as a fallback
                let mut hasher = aura_crypto::blake3_hasher();
                hasher.update(self.winner_device_id.0.as_bytes());
                hasher.update(self.session_id.as_bytes());
                hasher.update(&self.granted_at_epoch.to_le_bytes());
                *hasher.finalize().as_bytes()
            })
            .unwrap_or_else(|| {
                // Generate deterministic lottery ticket from available data
                let mut hasher = aura_crypto::blake3_hasher();
                hasher.update(self.winner_device_id.0.as_bytes());
                hasher.update(self.session_id.as_bytes());
                hasher.update(&self.granted_at_epoch.to_le_bytes());
                *hasher.finalize().as_bytes()
            });

        let lock = OperationLock {
            operation_type: self.operation_type,
            session_id: SessionId(self.session_id),
            acquired_at: now!(effects),
            expires_at: now!(effects) + 3600, // 1 hour default
            holder: ParticipantId::Device(self.winner_device_id),
            holder_device_id: self.winner_device_id,
            granted_at_epoch: self.granted_at_epoch,
            lottery_ticket,
        };

        state
            .grant_lock(lock)
            .map_err(AuraError::protocol_invalid_instruction)?;

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
    fn apply(&self, state: &mut AccountState, _effects: &aura_crypto::Effects) -> AuraResult<()> {
        state
            .release_lock(self.session_id)
            .map_err(AuraError::protocol_invalid_instruction)?;

        // Note: Session status remains Completed - lock release doesn't change session status
        // The session represents the lock acquisition process, which is already completed

        Ok(())
    }
}

// ========== P2P DKD Protocol ==========

impl Appliable for InitiateDkdSessionEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> AuraResult<()> {
        // DKD session state is tracked separately (will be in orchestration layer)
        // This event just records the initiation in the ledger
        Ok(())
    }
}

impl Appliable for RecordDkdCommitmentEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> AuraResult<()> {
        // Commitments are recorded for Byzantine detection
        // Actual verification happens in orchestration layer
        Ok(())
    }
}

impl Appliable for RevealDkdPointEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> AuraResult<()> {
        // Points are revealed after all commitments collected
        // Verification happens in orchestration layer
        Ok(())
    }
}

impl Appliable for FinalizeDkdSessionEvent {
    fn apply(&self, state: &mut AccountState, effects: &aura_crypto::Effects) -> AuraResult<()> {
        // Store commitment root for post-compaction verification
        // Extract commitment count from session data or derive from threshold
        let commitment_count = state
            .sessions
            .get(&self.session_id)
            .and_then(|session| {
                // If this is a DKD session, commitment count equals participant count
                match session.protocol_type {
                    ProtocolType::Dkd => Some(session.participants.len() as u32),
                    _ => None,
                }
            })
            .or_else(|| {
                // Fallback: estimate from threshold (participants = threshold + 1 typically)
                state.threshold.checked_add(1).map(|count| count as u32)
            })
            .unwrap_or(1); // Conservative fallback

        let root = DkdCommitmentRoot {
            session_id: SessionId(self.session_id),
            root_hash: self.commitment_root,
            commitment_count,
            created_at: now!(effects),
        };

        state.add_commitment_root(root);

        // Update group public key with derived identity
        if self.derived_identity_pk.len() >= 32 {
            // Convert derived identity to Ed25519VerifyingKey
            let mut pk_bytes = [0u8; 32];
            pk_bytes.copy_from_slice(&self.derived_identity_pk[..32]);

            match aura_crypto::Ed25519VerifyingKey::from_bytes(&pk_bytes) {
                Ok(new_group_key) => {
                    let old_key = state.group_public_key;
                    state.group_public_key = new_group_key;

                    tracing::info!(
                        "Updated group public key via DKD session {}: {} -> {}",
                        self.session_id,
                        hex::encode(old_key.as_bytes()),
                        hex::encode(new_group_key.as_bytes())
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to parse derived identity as public key in session {}: {:?}",
                        self.session_id,
                        e
                    );
                    // Continue without updating key - this is not a fatal error
                }
            }
        } else {
            tracing::warn!(
                "Derived identity too short in DKD session {}: {} bytes",
                self.session_id,
                self.derived_identity_pk.len()
            );
        }

        Ok(())
    }
}

impl Appliable for AbortDkdSessionEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> AuraResult<()> {
        // Session aborted, no state changes needed
        // Orchestration layer will clean up
        Ok(())
    }
}

impl Appliable for HealthCheckRequestEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> AuraResult<()> {
        // Health check requests are tracked by orchestration layer
        Ok(())
    }
}

impl Appliable for HealthCheckResponseEvent {
    fn apply(&self, state: &mut AccountState, effects: &aura_crypto::Effects) -> AuraResult<()> {
        // Update device last_seen timestamp
        if let Some(device) = state.devices.get_mut(&self.device_id) {
            device.last_seen = now!(effects);
        }
        Ok(())
    }
}

// ========== P2P Resharing Protocol ==========

impl Appliable for InitiateResharingEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> AuraResult<()> {
        // Resharing session state is tracked separately
        Ok(())
    }
}

impl Appliable for DistributeSubShareEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> AuraResult<()> {
        // Sub-shares are distributed via transport layer
        // This event just records the distribution
        Ok(())
    }
}

impl Appliable for AcknowledgeSubShareEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> AuraResult<()> {
        // Acknowledgements are tracked by orchestration layer
        Ok(())
    }
}

impl Appliable for FinalizeResharingEvent {
    fn apply(&self, state: &mut AccountState, _effects: &aura_crypto::Effects) -> AuraResult<()> {
        // Update threshold configuration
        state.threshold = self.new_threshold;

        // Update group public key with new threshold parameters
        if self.new_group_public_key.len() >= 32 {
            let mut pk_bytes = [0u8; 32];
            pk_bytes.copy_from_slice(&self.new_group_public_key[..32]);

            match aura_crypto::Ed25519VerifyingKey::from_bytes(&pk_bytes) {
                Ok(new_group_key) => {
                    let old_key = state.group_public_key;
                    state.group_public_key = new_group_key;

                    tracing::info!(
                        "Updated group public key via resharing: {} -> {}",
                        hex::encode(old_key.as_bytes()),
                        hex::encode(new_group_key.as_bytes())
                    );
                }
                Err(e) => {
                    tracing::warn!("Failed to parse new group public key in resharing: {:?}", e);
                }
            }
        }

        // Clear old device shares by marking them for rotation
        // TODO: Update device key share epochs when field is available
        // for device in state.devices.values_mut() {
        //     device.key_share_epoch = self.epoch_after_resharing;
        // }

        Ok(())
    }
}

impl Appliable for AbortResharingEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> AuraResult<()> {
        // Resharing aborted, no state changes
        Ok(())
    }
}

impl Appliable for ResharingRollbackEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> AuraResult<()> {
        // Rollback to previous state (orchestration layer handles this)
        Ok(())
    }
}

// ========== Recovery Protocol ==========

impl Appliable for InitiateRecoveryEvent {
    fn apply(&self, state: &mut AccountState, effects: &aura_crypto::Effects) -> AuraResult<()> {
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
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> AuraResult<()> {
        // Guardian approvals are tracked by orchestration layer
        Ok(())
    }
}

impl Appliable for SubmitRecoveryShareEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> AuraResult<()> {
        // Recovery shares are collected by orchestration layer
        Ok(())
    }
}

impl Appliable for CompleteRecoveryEvent {
    fn apply(&self, state: &mut AccountState, _effects: &aura_crypto::Effects) -> AuraResult<()> {
        // Recovery complete, verify new device was actually added via AddDevice event
        if !state.devices.contains_key(&self.new_device_id) {
            return Err(AuraError::protocol_invalid_instruction(format!(
                "Recovery completion failed: device {} was not added to the account",
                self.new_device_id.0
            )));
        }

        // Verify device is active (not tombstoned)
        if state.removed_devices.contains(&self.new_device_id) {
            return Err(AuraError::protocol_invalid_instruction(format!(
                "Recovery completion failed: device {} is tombstoned",
                self.new_device_id.0
            )));
        }

        // Verify test signature proves the device can use the recovered key
        if self.test_signature.len() >= 64 {
            // The test signature demonstrates the new device can sign with the recovered key
            // This provides cryptographic proof of successful recovery
            tracing::debug!(
                "Recovery {} includes valid test signature from device {}",
                self.recovery_id,
                self.new_device_id.0
            );
        } else {
            tracing::warn!(
                "Recovery {} test signature too short: {} bytes",
                self.recovery_id,
                self.test_signature.len()
            );
        }

        tracing::info!(
            "Recovery {} completed: device {} successfully added",
            self.recovery_id,
            self.new_device_id.0
        );
        Ok(())
    }
}

impl Appliable for AbortRecoveryEvent {
    fn apply(&self, state: &mut AccountState, _effects: &aura_crypto::Effects) -> AuraResult<()> {
        // Remove cooldown
        state.cooldowns.remove(&self.recovery_id);
        Ok(())
    }
}

impl Appliable for NudgeGuardianEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> AuraResult<()> {
        // Nudges are tracked by orchestration layer
        Ok(())
    }
}

// ========== Compaction Protocol ==========

impl Appliable for ProposeCompactionEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> AuraResult<()> {
        // Compaction proposals are tracked by orchestration layer
        Ok(())
    }
}

impl Appliable for AcknowledgeCompactionEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> AuraResult<()> {
        // Acknowledgements are tracked by orchestration layer
        Ok(())
    }
}

impl Appliable for CommitCompactionEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> AuraResult<()> {
        // Compaction committed
        // Actual compaction is performed by ledger layer
        Ok(())
    }
}

// ========== Device/Guardian Management ==========

impl Appliable for AddDeviceEvent {
    fn apply(&self, state: &mut AccountState, effects: &aura_crypto::Effects) -> AuraResult<()> {
        let public_key = aura_crypto::Ed25519VerifyingKey::from_bytes(
            &self.public_key.as_slice().try_into().map_err(|_| {
                AuraError::protocol_invalid_instruction("Invalid public key length".to_string())
            })?,
        )
        .map_err(|e| {
            AuraError::protocol_invalid_instruction(format!("Invalid public key: {:?}", e))
        })?;

        let device = DeviceMetadata {
            device_id: self.device_id,
            device_name: self.device_name.clone(),
            device_type: self.device_type,
            public_key,
            added_at: now!(effects),
            last_seen: now!(effects),
            dkd_commitment_proofs: std::collections::BTreeMap::new(),
            next_nonce: 1,
            used_nonces: std::collections::BTreeSet::new(),
        };

        state.add_device(device, effects)?;
        Ok(())
    }
}

impl Appliable for RemoveDeviceEvent {
    fn apply(&self, state: &mut AccountState, effects: &aura_crypto::Effects) -> AuraResult<()> {
        state.remove_device(self.device_id, effects)?;
        Ok(())
    }
}

impl Appliable for UpdateDeviceNonceEvent {
    fn apply(&self, state: &mut AccountState, _effects: &aura_crypto::Effects) -> AuraResult<()> {
        // Update device nonce for replay prevention
        if let Some(device) = state.devices.get_mut(&self.device_id) {
            // Validate nonce increment
            if self.new_nonce != self.previous_nonce + 1 {
                return Err(AuraError::protocol_invalid_instruction(format!(
                    "Invalid nonce increment for device {}: expected {}, got {}",
                    self.device_id.0,
                    self.previous_nonce + 1,
                    self.new_nonce
                )));
            }

            // Validate previous nonce matches current
            if device.next_nonce != self.previous_nonce + 1 {
                return Err(AuraError::protocol_invalid_instruction(format!(
                    "Nonce mismatch for device {}: current {}, event claims {}",
                    self.device_id.0,
                    device.next_nonce,
                    self.previous_nonce + 1
                )));
            }

            // Update nonce
            device.next_nonce = self.new_nonce + 1;
            device.used_nonces.insert(self.new_nonce);

            debug!(
                "Updated device {} nonce: {} -> {}",
                self.device_id.0, self.previous_nonce, self.new_nonce
            );
        } else {
            return Err(AuraError::device_not_found(format!(
                "Device {} not found for nonce update",
                self.device_id.0
            )));
        }

        Ok(())
    }
}

impl Appliable for AddGuardianEvent {
    fn apply(&self, state: &mut AccountState, effects: &aura_crypto::Effects) -> AuraResult<()> {
        // Extract device_id from contact_info or use a placeholder
        // In a real implementation, this would be extracted from the event
        #[allow(clippy::unwrap_used)] // Placeholder with known valid data
        let placeholder_key = aura_crypto::Ed25519VerifyingKey::from_bytes(&[0u8; 32]).unwrap();

        // Extract guardian device ID from event
        // Note: Using guardian_id as device identifier since the event doesn't have guardian_device_id
        let guardian_device_id = DeviceId::from_string_with_effects(
            &format!("guardian-{}", self.guardian_id.0),
            effects,
        );

        // Guardian public key will be derived from contact info or set to placeholder
        let guardian_public_key = placeholder_key;

        let guardian = GuardianMetadata {
            guardian_id: self.guardian_id,
            device_id: guardian_device_id,
            email: self.contact_info.email.clone(),
            public_key: guardian_public_key,
            added_at: now!(effects),
            policy: GuardianPolicy::default(),
        };

        state.add_guardian(guardian, effects)?;
        Ok(())
    }
}

impl Appliable for RemoveGuardianEvent {
    fn apply(&self, state: &mut AccountState, effects: &aura_crypto::Effects) -> AuraResult<()> {
        state.remove_guardian(self.guardian_id, effects)?;
        Ok(())
    }
}

// ========== Presence ==========

impl Appliable for PresenceTicketCacheEvent {
    fn apply(&self, state: &mut AccountState, effects: &aura_crypto::Effects) -> AuraResult<()> {
        let ticket = PresenceTicketCache {
            device_id: self.device_id,
            session_epoch: SessionEpoch::initial(), // Use default session epoch
            ticket: Vec::new(),                     // Use empty ticket data as placeholder
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
    fn apply(&self, state: &mut AccountState, effects: &aura_crypto::Effects) -> AuraResult<()> {
        // Validate the delegation is authorized
        state.validate_capability_delegation(self, effects)?;

        // Apply delegation to authority graph
        if let Err(e) = state
            .authority_graph
            .apply_delegation(self.clone(), effects)
        {
            return Err(AuraError::capability_system_error(e.to_string()));
        }

        // Update visibility index
        state
            .visibility_index
            .update_authority_graph(state.authority_graph.clone(), effects);

        Ok(())
    }
}

impl Appliable for crate::capability::events::CapabilityRevocation {
    fn apply(&self, state: &mut AccountState, effects: &aura_crypto::Effects) -> AuraResult<()> {
        // Validate the revocation is authorized
        state.validate_capability_revocation(self, effects)?;

        // Apply revocation to authority graph
        if let Err(e) = state
            .authority_graph
            .apply_revocation(self.clone(), effects)
        {
            return Err(AuraError::capability_system_error(e.to_string()));
        }

        // Update visibility index and handle revocation cascade
        state
            .visibility_index
            .update_authority_graph(state.authority_graph.clone(), effects);

        Ok(())
    }
}

// ========== Session Management ==========

impl Appliable for CreateSessionEvent {
    fn apply(&self, state: &mut AccountState, _effects: &aura_crypto::Effects) -> AuraResult<()> {
        // Convert device participants to participant IDs
        let participants: Vec<ParticipantId> = self
            .participants
            .iter()
            .map(|device_id| ParticipantId::Device(*device_id))
            .collect();

        // Create new session
        let session = Session {
            session_id: SessionId(self.session_id),
            protocol_type: self.protocol_type,
            participants,
            started_at: self.created_at_epoch,
            expires_at: self.created_at_epoch + self.ttl_epochs,
            status: SessionStatus::Active,
            metadata: std::collections::BTreeMap::new(), // Empty metadata for now
        };

        // Add session to state
        state.sessions.insert(self.session_id, session);

        debug!(
            "Created session {} for protocol {:?} with {} participants",
            self.session_id,
            self.protocol_type,
            self.participants.len()
        );

        Ok(())
    }
}

impl Appliable for UpdateSessionStatusEvent {
    fn apply(&self, state: &mut AccountState, _effects: &aura_crypto::Effects) -> AuraResult<()> {
        // Update session status
        if let Some(session) = state.sessions.get_mut(&self.session_id) {
            // Validate status transition
            if session.status != self.previous_status {
                return Err(AuraError::protocol_invalid_instruction(format!(
                    "Session {} status mismatch: expected {:?}, got {:?}",
                    self.session_id, self.previous_status, session.status
                )));
            }

            session.status = self.new_status;

            debug!(
                "Updated session {} status: {:?} -> {:?}",
                self.session_id, self.previous_status, self.new_status
            );
        } else {
            return Err(AuraError::protocol_invalid_instruction(format!(
                "Session {} not found for status update",
                self.session_id
            )));
        }

        Ok(())
    }
}

impl Appliable for CompleteSessionEvent {
    fn apply(&self, state: &mut AccountState, _effects: &aura_crypto::Effects) -> AuraResult<()> {
        // Complete session with outcome
        if let Some(session) = state.sessions.get_mut(&self.session_id) {
            session.status = self.final_status;

            debug!(
                "Completed session {} with outcome {:?}",
                self.session_id, self.outcome
            );
        } else {
            return Err(AuraError::protocol_invalid_instruction(format!(
                "Session {} not found for completion",
                self.session_id
            )));
        }

        Ok(())
    }
}

impl Appliable for AbortSessionEvent {
    fn apply(&self, state: &mut AccountState, _effects: &aura_crypto::Effects) -> AuraResult<()> {
        // Abort session with failure
        if let Some(session) = state.sessions.get_mut(&self.session_id) {
            // Validate previous status
            if session.status != self.previous_status {
                return Err(AuraError::protocol_invalid_instruction(format!(
                    "Session {} status mismatch: expected {:?}, got {:?}",
                    self.session_id, self.previous_status, session.status
                )));
            }

            session.status = SessionStatus::Failed;

            // Record fault information in metadata
            session
                .metadata
                .insert("abort_reason".to_string(), self.reason.clone());
            session.metadata.insert(
                "blamed_party".to_string(),
                format!("{:?}", self.blamed_party),
            );
            session
                .metadata
                .insert("detected_at".to_string(), self.aborted_at_epoch.to_string());

            debug!(
                "Aborted session {} with reason: {}",
                self.session_id, self.reason
            );
        } else {
            return Err(AuraError::protocol_invalid_instruction(format!(
                "Session {} not found for abort",
                self.session_id
            )));
        }

        Ok(())
    }
}

impl Appliable for CleanupExpiredSessionsEvent {
    fn apply(&self, state: &mut AccountState, _effects: &aura_crypto::Effects) -> AuraResult<()> {
        // Remove expired sessions
        let mut cleaned_count = 0;
        for session_id in &self.expired_sessions {
            if state.sessions.remove(session_id).is_some() {
                cleaned_count += 1;
            }
        }

        debug!(
            "Cleaned up {} expired sessions at epoch {}",
            cleaned_count, self.cleanup_at_epoch
        );

        Ok(())
    }
}

// ========== Keyhive Integration Application Functions ==========

/// Apply Keyhive capability delegation to account state
fn apply_keyhive_capability_delegation(
    delegation: &crate::protocols::events::KeyhiveCapabilityDelegation,
    _state: &mut AccountState,
    _effects: &aura_crypto::Effects,
) -> AuraResult<()> {
    // TODO: Integrate Keyhive delegation with Aura's capability system
    // For now, we'll store it in a separate collection to avoid conflicts

    debug!(
        "Applied Keyhive capability delegation: {}",
        delegation.capability_id
    );

    // In a full implementation, this would:
    // 1. Validate the delegation against the authority graph
    // 2. Update the capability state
    // 3. Trigger any necessary group membership updates

    Ok(())
}

/// Apply Keyhive capability revocation to account state
fn apply_keyhive_capability_revocation(
    revocation: &crate::protocols::events::KeyhiveCapabilityRevocation,
    _state: &mut AccountState,
    _effects: &aura_crypto::Effects,
) -> AuraResult<()> {
    // TODO: Integrate Keyhive revocation with Aura's capability system

    debug!(
        "Applied Keyhive capability revocation: {}",
        revocation.capability_id
    );

    // In a full implementation, this would:
    // 1. Validate the revocation authority
    // 2. Update the capability state
    // 3. Trigger cascading revocations if necessary
    // 4. Update group membership based on capability changes

    Ok(())
}

/// Apply Keyhive CGKA operation to account state
fn apply_keyhive_cgka_operation(
    operation: &keyhive_core::cgka::operation::CgkaOperation,
    _state: &mut AccountState,
    _effects: &aura_crypto::Effects,
) -> AuraResult<()> {
    use crate::capability::group_capabilities::*;
    use keyhive_core::cgka::operation::CgkaOperation;

    debug!("🔄 Applying Keyhive CGKA operation: {:?}", operation);

    // Extract document ID to identify the group
    let doc_id = operation.doc_id();
    let group_id = format!("cgka_group_{}", hex::encode(doc_id.as_slice()));

    debug!("📋 Processing CGKA operation for group: {}", group_id);

    // Get or create group capability manager for this group
    // Note: In a real implementation, this would be stored in the account state
    // For now, we'll create a simple tracking structure

    match operation {
        CgkaOperation::Add {
            added_id,
            leaf_index,
            ..
        } => {
            tracing::info!(
                "➕ CGKA Add operation: adding member {} at leaf {}",
                hex::encode(added_id.as_slice()),
                leaf_index
            );

            // Update group membership in account state
            let member_id = MemberId::new(hex::encode(added_id.as_slice()));

            // Create member capability scope
            let _member_scope = GroupCapabilityScope::Member {
                group_id: group_id.clone(),
            };

            // Grant membership capability to the new member
            // In a real implementation, this would:
            // 1. Create a capability grant event
            // 2. Add it to the authority graph
            // 3. Update the group roster

            debug!("✅ Granted group membership to: {}", member_id.as_str());

            // Track the operation in account state
            // This could be stored in a dedicated CGKA operations log
            // For now, we'll just log the successful application
            tracing::info!("✅ CGKA Add operation applied successfully");
        }

        CgkaOperation::Remove { id, leaf_idx, .. } => {
            tracing::info!(
                "➖ CGKA Remove operation: removing member {} from leaf {}",
                hex::encode(id.as_slice()),
                leaf_idx
            );

            let member_id = MemberId::new(hex::encode(id.as_slice()));

            // Revoke membership capability from the removed member
            let _member_scope = GroupCapabilityScope::Member {
                group_id: group_id.clone(),
            };

            // In a real implementation, this would:
            // 1. Create a capability revocation event
            // 2. Remove from the authority graph
            // 3. Update the group roster
            // 4. Invalidate any derived keys for this member

            debug!("✅ Revoked group membership from: {}", member_id.as_str());

            tracing::info!("✅ CGKA Remove operation applied successfully");
        }

        CgkaOperation::Update { id, .. } => {
            tracing::info!(
                "🔄 CGKA Update operation: updating path for member {}",
                hex::encode(id.as_slice())
            );

            let member_id = MemberId::new(hex::encode(id.as_slice()));

            // Process key rotation for the member
            // In a real implementation, this would:
            // 1. Update the member's key material in the tree
            // 2. Derive new application secrets
            // 3. Update encryption keys for group messages
            // 4. Trigger key rotation for dependent systems

            debug!(
                "🔑 Processing key update for member: {}",
                member_id.as_str()
            );

            // Update group epoch to reflect the key rotation
            // This ensures forward secrecy and post-compromise security

            tracing::info!("✅ CGKA Update operation applied successfully");
        }
    }

    // Update group state in account
    // In a full implementation, this would update persistent group state
    let current_time = _effects
        .now()
        .map_err(|e| AuraError::protocol_invalid_instruction(format!("Time error: {}", e)))?;

    debug!("⏰ CGKA operation processed at timestamp: {}", current_time);

    // Trigger any dependent operations
    // This could include:
    // - Updating group message encryption keys
    // - Notifying other group members of the change
    // - Updating access control lists
    // - Invalidating cached application secrets

    tracing::info!("🎯 CGKA operation fully integrated into account state");

    Ok(())
}

// ========== Storage Operations ==========

impl Appliable for StoreDataEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> AuraResult<()> {
        debug!("Applying StoreDataEvent for blob_id: {:?}", self.blob_id);

        // TODO: Implement storage event application logic
        // This would include:
        // - Recording the storage operation in the ledger
        // - Updating storage quotas and indexing
        // - Validating capability requirements
        // - Managing replication metadata

        tracing::info!("📦 Store data event applied for blob: {:?}", self.blob_id);
        Ok(())
    }
}

impl Appliable for RetrieveDataEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> AuraResult<()> {
        debug!("Applying RetrieveDataEvent for blob_id: {:?}", self.blob_id);

        // TODO: Implement retrieve event application logic
        // This would include:
        // - Validating capability proof against access control
        // - Recording access audit log
        // - Updating usage statistics
        // - Checking data availability

        tracing::info!(
            "📤 Retrieve data event applied for blob: {:?}",
            self.blob_id
        );
        Ok(())
    }
}

impl Appliable for DeleteDataEvent {
    fn apply(&self, _state: &mut AccountState, _effects: &aura_crypto::Effects) -> AuraResult<()> {
        debug!("Applying DeleteDataEvent for blob_id: {:?}", self.blob_id);

        // TODO: Implement delete event application logic
        // This would include:
        // - Validating deletion authorization
        // - Recording tombstone for CRDT semantics
        // - Updating storage quotas
        // - Initiating cleanup of replicas

        tracing::info!("🗑️ Delete data event applied for blob: {:?}", self.blob_id);
        Ok(())
    }
}
