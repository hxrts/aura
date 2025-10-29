// Account state managed by the CRDT

use crate::types::*;
use aura_crypto::Ed25519VerifyingKey;
use aura_crypto::MerkleProof;
use aura_types::{AccountId, DeviceId, GuardianId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Account state stored in the CRDT
///
/// Reference: 080 spec Part 3: CRDT Choreography & State Management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountState {
    pub account_id: AccountId,
    #[serde(with = "verifying_key_serde")]
    pub group_public_key: Ed25519VerifyingKey,

    /// Active devices (G-Set with metadata)
    pub devices: BTreeMap<DeviceId, DeviceMetadata>,
    /// Tombstoned (removed) devices
    pub removed_devices: BTreeSet<DeviceId>,

    /// Active guardians (G-Set with metadata)
    pub guardians: BTreeMap<GuardianId, GuardianMetadata>,
    /// Tombstoned (removed) guardians
    pub removed_guardians: BTreeSet<GuardianId>,

    /// Current session epoch (monotonic counter)
    pub session_epoch: SessionEpoch,

    /// Lamport clock for causal ordering (080 spec Part 1: Logical Clock)
    ///
    /// This implements a Lamport timestamp that provides:
    /// - Partial causal ordering: if event A happened before B, lamport(A) < lamport(B)
    /// - Total ordering: all events have a consistent total order
    /// - Session timeouts without wall-clock sync
    ///
    /// Lamport clock rules:
    /// 1. On local event creation: increment local clock
    /// 2. On remote event receipt: max(local, received) + 1
    ///
    /// This is sufficient for Aura's needs:
    /// - CRDT merges handle concurrent updates automatically
    /// - Timeouts use logical counter, not concurrency detection
    /// - Lock lottery uses deterministic tie-breaking
    ///
    /// A vector clock would be overkill and add unnecessary overhead.
    pub lamport_clock: u64,

    /// DKD commitment roots persisted for post-compaction verification
    /// (080 spec Part 3: Ledger Compaction)
    pub dkd_commitment_roots: BTreeMap<uuid::Uuid, DkdCommitmentRoot>,

    /// Active distributed operation lock (080 spec Part 3: Distributed Locking)
    /// Only one operation (DKD, Resharing, Recovery) can run at a time
    pub active_operation_lock: Option<OperationLock>,

    /// Cached presence tickets
    pub presence_tickets: BTreeMap<DeviceId, PresenceTicketCache>,

    /// Active cooldown counters
    pub cooldowns: BTreeMap<uuid::Uuid, CooldownCounter>,

    /// Active protocol sessions (unified session management)
    pub sessions: BTreeMap<uuid::Uuid, Session>,

    /// Authority graph for capability management
    pub authority_graph: crate::capability::authority_graph::AuthorityGraph,
    /// Visibility index for operation materialization
    pub visibility_index: crate::capability::visibility::VisibilityIndex,

    /// Threshold configuration
    pub threshold: u16,
    pub total_participants: u16,

    /// Used nonces to prevent replay attacks
    pub used_nonces: BTreeSet<u64>,
    /// Next expected nonce (monotonic counter)
    pub next_nonce: u64,

    /// Last event hash for causal ordering (None for initial state)
    pub last_event_hash: Option<[u8; 32]>,

    /// Last updated timestamp
    pub updated_at: u64,

    // ===== SSB (Social Bulletin Board) State =====
    // Added for Phase 1.2: Unified Journal State Integration
    /// SSB envelopes stored in the unified CRDT
    /// Map: CID -> SealedEnvelope (stored as serialized bytes for now)
    pub sbb_envelopes: BTreeMap<String, Vec<u8>>,

    /// SSB neighbor peers for envelope flooding
    /// Set of peer account IDs that we exchange envelopes with
    pub sbb_neighbors: BTreeSet<AccountId>,

    /// Relationship keys for pairwise communications
    /// Map: RelationshipId (as hex string) -> Encrypted RelationshipKeys
    pub relationship_keys: BTreeMap<String, Vec<u8>>,

    /// SSB relationship counter tracking
    /// Map: RelationshipId -> (last_seen_counter, ttl_epoch)
    /// Used for envelope uniqueness and replay protection
    pub relationship_counters: BTreeMap<crate::events::RelationshipId, (u64, u64)>,
}

impl AccountState {
    /// Create a new account state with initial device
    pub fn new(
        account_id: AccountId,
        group_public_key: Ed25519VerifyingKey,
        initial_device: DeviceMetadata,
        threshold: u16,
        total_participants: u16,
    ) -> Self {
        let mut devices = BTreeMap::new();
        devices.insert(initial_device.device_id, initial_device);

        // Initialize capability system with effects (use placeholder for construction)
        let effects = aura_crypto::Effects::for_test("account_state_new");
        let authority_graph = crate::capability::authority_graph::AuthorityGraph::new();
        let visibility_index =
            crate::capability::visibility::VisibilityIndex::new(authority_graph.clone(), &effects);

        AccountState {
            account_id,
            group_public_key,
            devices,
            removed_devices: BTreeSet::new(),
            guardians: BTreeMap::new(),
            removed_guardians: BTreeSet::new(),
            session_epoch: SessionEpoch::initial(),
            lamport_clock: 0,
            dkd_commitment_roots: BTreeMap::new(),
            active_operation_lock: None,
            presence_tickets: BTreeMap::new(),
            cooldowns: BTreeMap::new(),
            sessions: BTreeMap::new(),
            authority_graph,
            visibility_index,
            threshold,
            total_participants,
            used_nonces: BTreeSet::new(),
            next_nonce: 0,
            last_event_hash: None,
            updated_at: current_timestamp_with_effects(&effects),

            // Initialize SSB state (Phase 1.2)
            sbb_envelopes: BTreeMap::new(),
            sbb_neighbors: BTreeSet::new(),
            relationship_keys: BTreeMap::new(),
            relationship_counters: BTreeMap::new(),
        }
    }

    /// Validate and consume a nonce to prevent replay attacks
    pub fn validate_nonce(&mut self, nonce: u64) -> Result<(), String> {
        // Check if nonce was already used
        if self.used_nonces.contains(&nonce) {
            return Err(format!("Nonce {} already used (replay attack)", nonce));
        }

        // Accept nonce if it's >= next_nonce (allow for out-of-order delivery)
        if nonce < self.next_nonce {
            return Err(format!(
                "Nonce {} is too old (expected >= {})",
                nonce, self.next_nonce
            ));
        }

        // Mark nonce as used
        self.used_nonces.insert(nonce);

        // Update next_nonce if this nonce is higher
        if nonce >= self.next_nonce {
            self.next_nonce = nonce + 1;
        }

        // Prune old nonces (keep last 1000 to handle reordering)
        if self.used_nonces.len() > 1000 {
            let threshold = self.next_nonce.saturating_sub(1000);
            self.used_nonces.retain(|&n| n >= threshold);
        }

        Ok(())
    }

    /// Add a device
    pub fn add_device(
        &mut self,
        device: DeviceMetadata,
        effects: &aura_crypto::Effects,
    ) -> crate::Result<()> {
        if self.removed_devices.contains(&device.device_id) {
            return Err(crate::AuraError::protocol_invalid_instruction(
                "Cannot re-add a removed device".to_string(),
            ));
        }

        self.devices.insert(device.device_id, device);
        self.updated_at = current_timestamp_with_effects(effects);
        Ok(())
    }

    /// Remove a device (tombstone)
    pub fn remove_device(
        &mut self,
        device_id: DeviceId,
        effects: &aura_crypto::Effects,
    ) -> crate::Result<()> {
        if !self.devices.contains_key(&device_id) {
            return Err(crate::AuraError::device_not_found(device_id.to_string()));
        }

        self.devices.remove(&device_id);
        self.removed_devices.insert(device_id);
        self.presence_tickets.remove(&device_id);
        self.updated_at = current_timestamp_with_effects(effects);
        Ok(())
    }

    /// Add a guardian
    pub fn add_guardian(
        &mut self,
        guardian: GuardianMetadata,
        effects: &aura_crypto::Effects,
    ) -> crate::Result<()> {
        if self.removed_guardians.contains(&guardian.guardian_id) {
            return Err(crate::AuraError::protocol_invalid_instruction(
                "Cannot re-add a removed guardian",
            ));
        }

        self.guardians.insert(guardian.guardian_id, guardian);
        self.updated_at = current_timestamp_with_effects(effects);
        Ok(())
    }

    /// Remove a guardian (tombstone)
    pub fn remove_guardian(
        &mut self,
        guardian_id: GuardianId,
        effects: &aura_crypto::Effects,
    ) -> crate::Result<()> {
        if !self.guardians.contains_key(&guardian_id) {
            return Err(crate::AuraError::authority_not_found(format!(
                "Guardian not found: {:?}",
                guardian_id
            )));
        }

        self.guardians.remove(&guardian_id);
        self.removed_guardians.insert(guardian_id);
        self.updated_at = current_timestamp_with_effects(effects);
        Ok(())
    }

    /// Bump session epoch
    pub fn bump_session_epoch(&mut self, effects: &aura_crypto::Effects) -> SessionEpoch {
        let old_epoch = self.session_epoch;
        self.session_epoch = old_epoch.next();

        // Clear all presence tickets on epoch bump
        self.presence_tickets.clear();

        self.updated_at = current_timestamp_with_effects(effects);
        self.session_epoch
    }

    /// Cache a presence ticket
    pub fn cache_presence_ticket(
        &mut self,
        ticket: PresenceTicketCache,
        effects: &aura_crypto::Effects,
    ) {
        self.presence_tickets.insert(ticket.device_id, ticket);
        self.updated_at = current_timestamp_with_effects(effects);
    }

    /// Start a cooldown
    pub fn start_cooldown(&mut self, cooldown: CooldownCounter, effects: &aura_crypto::Effects) {
        // Use a UUID based on participant_id and operation_type for now
        // In a real implementation, this should be provided by the caller
        let operation_id = effects.gen_uuid();
        self.cooldowns.insert(operation_id, cooldown);
        self.updated_at = effects.now().unwrap_or(0);
    }

    /// Remove completed cooldowns
    pub fn cleanup_cooldowns(&mut self, effects: &aura_crypto::Effects) {
        let current_time = effects.now().unwrap_or(0);
        self.cooldowns
            .retain(|_, cooldown| current_time < cooldown.reset_at);
    }

    /// Get active devices (non-tombstoned)
    pub fn active_devices(&self) -> Vec<&DeviceMetadata> {
        self.devices.values().collect()
    }

    /// Get active guardians (non-tombstoned)
    pub fn active_guardians(&self) -> Vec<&GuardianMetadata> {
        self.guardians.values().collect()
    }

    /// Check if a device is active
    pub fn is_device_active(&self, device_id: &DeviceId) -> bool {
        self.devices.contains_key(device_id) && !self.removed_devices.contains(device_id)
    }

    /// Get device metadata
    pub fn get_device(&self, device_id: &DeviceId) -> Option<&DeviceMetadata> {
        self.devices.get(device_id)
    }

    /// Get guardian metadata
    pub fn get_guardian(&self, guardian_id: &GuardianId) -> Option<&GuardianMetadata> {
        self.guardians.get(guardian_id)
    }

    // ========== New Operations for 080 Spec ==========

    /// Advance logical epoch (080 Part 1: Logical Clock)
    ///
    /// This should be called on every CRDT write to advance the logical clock
    /// Advance Lamport clock using Lamport timestamp rules
    ///
    /// When receiving an event from the network, we update our clock to ensure
    /// causal ordering: lamport_clock = max(local_clock, received_timestamp) + 1
    ///
    /// This ensures:
    /// - If event A causally precedes B, then lamport(A) < lamport(B)
    /// - All participants converge on the same total ordering
    pub fn advance_lamport_clock(
        &mut self,
        received_timestamp: u64,
        effects: &aura_crypto::Effects,
    ) {
        // Lamport clock rule: max(local, received) + 1
        self.lamport_clock = self.lamport_clock.max(received_timestamp) + 1;
        self.updated_at = current_timestamp_with_effects(effects);
    }

    /// Increment local Lamport clock (for locally-generated events)
    ///
    /// Call this when creating a new event on this device.
    /// The returned value should be used as the event's epoch_at_write.
    pub fn increment_lamport_clock(&mut self, effects: &aura_crypto::Effects) -> u64 {
        self.lamport_clock += 1;
        self.updated_at = current_timestamp_with_effects(effects);
        self.lamport_clock
    }

    /// Request operation lock (080 Part 3: Distributed Locking)
    ///
    /// Returns Ok(()) if lock can be requested (no existing lock)
    pub fn can_request_lock(&self, _operation_type: OperationType) -> Result<(), String> {
        if let Some(existing_lock) = &self.active_operation_lock {
            return Err(format!(
                "Operation lock already held by {:?} for {:?}",
                existing_lock.holder_device_id, existing_lock.operation_type
            ));
        }
        Ok(())
    }

    /// Grant operation lock (080 Part 3: Distributed Locking)
    pub fn grant_lock(&mut self, lock: OperationLock) -> Result<(), String> {
        if self.active_operation_lock.is_some() {
            return Err("Lock already granted".to_string());
        }

        self.active_operation_lock = Some(lock);
        // Note: Lamport clock is advanced in apply_event when GrantOperationLock event is applied
        Ok(())
    }

    /// Release operation lock (080 Part 3: Distributed Locking)
    pub fn release_lock(&mut self, session_id: uuid::Uuid) -> Result<(), String> {
        match &self.active_operation_lock {
            Some(lock) if lock.session_id.0 == session_id => {
                self.active_operation_lock = None;
                // Note: Lamport clock is advanced in apply_event when ReleaseOperationLock event is applied
                Ok(())
            }
            Some(lock) => Err(format!(
                "Cannot release lock: session_id mismatch (expected {:?}, got {:?})",
                lock.session_id, session_id
            )),
            None => Err("No active lock to release".to_string()),
        }
    }

    /// Add DKD commitment root (080 Part 3: Ledger Compaction)
    pub fn add_commitment_root(&mut self, root: DkdCommitmentRoot) {
        self.dkd_commitment_roots.insert(root.session_id.0, root);
        // Note: Lamport clock is advanced in apply_event when FinalizeDkdSession event is applied
    }

    /// Get DKD commitment root
    pub fn get_commitment_root(&self, session_id: &uuid::Uuid) -> Option<&DkdCommitmentRoot> {
        self.dkd_commitment_roots.get(session_id)
    }

    /// Store Merkle proof for device
    pub fn store_device_merkle_proof(
        &mut self,
        device_id: DeviceId,
        session_id: uuid::Uuid,
        proof: MerkleProof,
    ) -> Result<(), String> {
        let device = self
            .devices
            .get_mut(&device_id)
            .ok_or_else(|| format!("Device {} not found", device_id))?;

        device.dkd_commitment_proofs.insert(session_id, proof);
        // Note: Lamport clock advancement should happen at event application time,
        // not during proof storage. This is a no-op for now.
        Ok(())
    }

    // ========== Session Management ==========

    /// Add a new session to the account state
    pub fn add_session(&mut self, session: Session, effects: &aura_crypto::Effects) {
        self.sessions.insert(session.session_id.0, session);
        self.updated_at = current_timestamp_with_effects(effects);
    }

    /// Get a session by ID
    pub fn get_session(&self, session_id: &uuid::Uuid) -> Option<&Session> {
        self.sessions.get(session_id)
    }

    /// Get a mutable reference to a session by ID
    pub fn get_session_mut(&mut self, session_id: &uuid::Uuid) -> Option<&mut Session> {
        self.sessions.get_mut(session_id)
    }

    /// Update session status
    pub fn update_session_status(
        &mut self,
        session_id: uuid::Uuid,
        status: SessionStatus,
        effects: &aura_crypto::Effects,
    ) -> Result<(), String> {
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or_else(|| format!("Session {} not found", session_id))?;

        let timestamp = current_timestamp_with_effects(effects);
        session.update_status(status, timestamp);
        self.updated_at = timestamp;
        Ok(())
    }

    /// Complete a session with outcome
    pub fn complete_session(
        &mut self,
        session_id: uuid::Uuid,
        outcome: SessionOutcome,
        effects: &aura_crypto::Effects,
    ) -> Result<(), String> {
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or_else(|| format!("Session {} not found", session_id))?;

        let timestamp = current_timestamp_with_effects(effects);
        session.complete(outcome, timestamp);
        self.updated_at = timestamp;
        Ok(())
    }

    /// Abort a session with failure
    pub fn abort_session(
        &mut self,
        session_id: uuid::Uuid,
        reason: String,
        blamed_party: Option<ParticipantId>,
        effects: &aura_crypto::Effects,
    ) -> Result<(), String> {
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or_else(|| format!("Session {} not found", session_id))?;

        let timestamp = current_timestamp_with_effects(effects);
        session.abort(&reason, blamed_party, timestamp);
        self.updated_at = timestamp;
        Ok(())
    }

    /// Get all active sessions (non-terminal)
    pub fn active_sessions(&self) -> Vec<&Session> {
        self.sessions
            .values()
            .filter(|session| !session.is_terminal())
            .collect()
    }

    /// Get sessions by protocol type
    pub fn sessions_by_protocol(&self, protocol_type: ProtocolType) -> Vec<&Session> {
        self.sessions
            .values()
            .filter(|session| session.protocol_type == protocol_type)
            .collect()
    }

    /// Check if any session of given protocol type is active
    pub fn has_active_session_of_type(&self, protocol_type: ProtocolType) -> bool {
        self.sessions
            .values()
            .any(|session| session.protocol_type == protocol_type && !session.is_terminal())
    }

    /// Clean up expired sessions
    pub fn cleanup_expired_sessions(&mut self, current_epoch: u64, effects: &aura_crypto::Effects) {
        let expired_sessions: Vec<uuid::Uuid> = self
            .sessions
            .iter()
            .filter(|(_, session)| session.is_timed_out(current_epoch))
            .map(|(id, _)| *id)
            .collect();

        for session_id in &expired_sessions {
            if let Some(session) = self.sessions.get_mut(session_id) {
                if !session.is_terminal() {
                    let timestamp = current_timestamp_with_effects(effects);
                    session.update_status(SessionStatus::TimedOut, timestamp);
                }
            }
        }

        if !expired_sessions.is_empty() {
            self.updated_at = current_timestamp_with_effects(effects);
        }
    }

    // Lamport clock methods moved earlier in file (lines 271-285)

    /// Validate that a capability delegation is authorized
    pub fn validate_capability_delegation(
        &self,
        event: &crate::capability::events::CapabilityDelegation,
        effects: &aura_crypto::Effects,
    ) -> crate::Result<()> {
        use crate::capability::types::{CapabilityResult, CapabilityScope, Subject};

        // Convert issuing device to subject
        let issuer_subject = Subject::new(&event.issued_by.0.to_string());

        // For root authorities, only threshold signature authorization is allowed
        if event.parent_id.is_none() {
            return Ok(()); // Root authority creation is always allowed with threshold signature
        }

        // For derived capabilities, check that issuer has delegation authority
        let delegation_scope = CapabilityScope::simple("capability", "delegate");
        let result =
            self.authority_graph
                .evaluate_capability(&issuer_subject, &delegation_scope, effects);

        match result {
            CapabilityResult::Granted => Ok(()),
            CapabilityResult::Revoked => Err(crate::AuraError::capability_system_error(format!(
                "Issuer {} capability was revoked",
                event.issued_by.0
            ))),
            CapabilityResult::Expired => Err(crate::AuraError::capability_system_error(format!(
                "Issuer {} capability has expired",
                event.issued_by.0
            ))),
            CapabilityResult::NotFound => Err(crate::AuraError::capability_system_error(format!(
                "Issuer {} does not have delegation authority",
                event.issued_by.0
            ))),
        }
    }

    /// Validate that a capability revocation is authorized
    pub fn validate_capability_revocation(
        &self,
        event: &crate::capability::events::CapabilityRevocation,
        effects: &aura_crypto::Effects,
    ) -> crate::Result<()> {
        use crate::capability::types::{CapabilityResult, CapabilityScope, Subject};

        // Convert issuing device to subject
        let issuer_subject = Subject::new(&event.issued_by.0.to_string());

        // Check that issuer has revocation authority
        let revocation_scope = CapabilityScope::simple("capability", "revoke");
        let result =
            self.authority_graph
                .evaluate_capability(&issuer_subject, &revocation_scope, effects);

        match result {
            CapabilityResult::Granted => Ok(()),
            CapabilityResult::Revoked => Err(crate::AuraError::capability_system_error(format!(
                "Issuer {} capability was revoked",
                event.issued_by.0
            ))),
            CapabilityResult::Expired => Err(crate::AuraError::capability_system_error(format!(
                "Issuer {} capability has expired",
                event.issued_by.0
            ))),
            CapabilityResult::NotFound => Err(crate::AuraError::capability_system_error(format!(
                "Issuer {} does not have revocation authority",
                event.issued_by.0
            ))),
        }
    }
}

/// Get current Unix timestamp in seconds using injected effects
/// Wrapper that returns 0 on error (for state management where timestamp is not critical)
fn current_timestamp_with_effects(effects: &aura_crypto::Effects) -> u64 {
    aura_crypto::current_timestamp_with_effects(effects).unwrap_or(0)
}

mod verifying_key_serde {
    use aura_crypto::Ed25519VerifyingKey;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(key: &Ed25519VerifyingKey, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(key.as_bytes())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Ed25519VerifyingKey, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: Vec<u8> = Deserialize::deserialize(deserializer)?;
        aura_crypto::Ed25519VerifyingKey::from_bytes(
            bytes
                .as_slice()
                .try_into()
                .map_err(serde::de::Error::custom)?,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)] // Test code
mod tests {
    use super::*;
    use aura_types::{AccountIdExt, DeviceIdExt};

    fn mock_device(id: u16, effects: &aura_crypto::Effects) -> DeviceMetadata {
        use aura_crypto::Ed25519SigningKey;

        let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&rand::random());
        let public_key = signing_key.verifying_key();

        DeviceMetadata {
            device_id: DeviceId(uuid::Uuid::from_u128(id as u128)),
            device_name: format!("Device {}", id),
            device_type: DeviceType::Native,
            public_key,
            added_at: current_timestamp_with_effects(effects),
            last_seen: current_timestamp_with_effects(effects),
            dkd_commitment_proofs: std::collections::BTreeMap::new(),
            next_nonce: 0,
            used_nonces: std::collections::BTreeSet::new(),
        }
    }

    #[test]
    fn test_account_state_device_lifecycle() {
        use aura_crypto::Ed25519SigningKey;

        let effects = aura_crypto::Effects::for_test("test_account_state_device_lifecycle");
        let account_id = AccountId::new_with_effects(&effects);
        let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&rand::random());
        let group_public_key = signing_key.verifying_key();

        let device1 = mock_device(1, &effects);
        let device_id1 = device1.device_id;

        let mut state = AccountState::new(account_id, group_public_key, device1, 2, 3);

        // Add second device
        let device2 = mock_device(2, &effects);
        let device_id2 = device2.device_id;
        state.add_device(device2, &effects).unwrap();

        assert_eq!(state.active_devices().len(), 2);
        assert!(state.is_device_active(&device_id1));
        assert!(state.is_device_active(&device_id2));

        // Remove first device
        state.remove_device(device_id1, &effects).unwrap();

        assert_eq!(state.active_devices().len(), 1);
        assert!(!state.is_device_active(&device_id1));
        assert!(state.is_device_active(&device_id2));
        assert!(state.removed_devices.contains(&device_id1));
    }

    #[test]
    fn test_session_epoch_bump() {
        use aura_crypto::Ed25519SigningKey;

        let effects = aura_crypto::Effects::for_test("test_session_epoch_bump");
        let account_id = AccountId::new_with_effects(&effects);
        let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&rand::random());
        let group_public_key = signing_key.verifying_key();
        let device = mock_device(1, &effects);

        let mut state = AccountState::new(account_id, group_public_key, device, 2, 3);

        let initial_epoch = state.session_epoch;
        assert_eq!(initial_epoch.0, 0);

        // Add presence ticket
        let ticket = PresenceTicketCache {
            device_id: DeviceId::new_with_effects(&effects),
            session_epoch: SessionEpoch::initial(),
            ticket: Vec::new(),
            issued_at: current_timestamp_with_effects(&effects),
            expires_at: current_timestamp_with_effects(&effects) + 3600,
            ticket_digest: [0u8; 32],
        };
        state.cache_presence_ticket(ticket, &effects);
        assert_eq!(state.presence_tickets.len(), 1);

        // Bump epoch
        let new_epoch = state.bump_session_epoch(&effects);
        assert_eq!(new_epoch.0, 1);
        assert_eq!(state.session_epoch.0, 1);

        // Presence tickets should be cleared
        assert_eq!(state.presence_tickets.len(), 0);
    }
}
