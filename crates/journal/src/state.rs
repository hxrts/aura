// Account state managed by the CRDT

use crate::types::*;
use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Account state stored in the CRDT
///
/// Reference: 080 spec Part 3: CRDT Choreography & State Management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountState {
    pub account_id: AccountId,
    #[serde(with = "verifying_key_serde")]
    pub group_public_key: VerifyingKey,
    
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
    
    /// Policy reference
    pub policy: Option<PolicyReference>,
    
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
}

impl AccountState {
    /// Create a new account state with initial device
    pub fn new(
        account_id: AccountId,
        group_public_key: VerifyingKey,
        initial_device: DeviceMetadata,
        threshold: u16,
        total_participants: u16,
    ) -> Self {
        let mut devices = BTreeMap::new();
        devices.insert(initial_device.device_id, initial_device);
        
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
            policy: None,
            threshold,
            total_participants,
            used_nonces: BTreeSet::new(),
            next_nonce: 0,
            last_event_hash: None,
            updated_at: current_timestamp(),
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
            return Err(format!("Nonce {} is too old (expected >= {})", nonce, self.next_nonce));
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
    pub fn add_device(&mut self, device: DeviceMetadata) -> crate::Result<()> {
        if self.removed_devices.contains(&device.device_id) {
            return Err(crate::LedgerError::InvalidEvent(
                "Cannot re-add a removed device".to_string(),
            ));
        }
        
        self.devices.insert(device.device_id, device);
        self.updated_at = current_timestamp();
        Ok(())
    }
    
    /// Remove a device (tombstone)
    pub fn remove_device(&mut self, device_id: DeviceId) -> crate::Result<()> {
        if !self.devices.contains_key(&device_id) {
            return Err(crate::LedgerError::DeviceNotFound(device_id.to_string()));
        }
        
        self.devices.remove(&device_id);
        self.removed_devices.insert(device_id);
        self.presence_tickets.remove(&device_id);
        self.updated_at = current_timestamp();
        Ok(())
    }
    
    /// Add a guardian
    pub fn add_guardian(&mut self, guardian: GuardianMetadata) -> crate::Result<()> {
        if self.removed_guardians.contains(&guardian.guardian_id) {
            return Err(crate::LedgerError::InvalidEvent(
                "Cannot re-add a removed guardian".to_string(),
            ));
        }
        
        self.guardians.insert(guardian.guardian_id, guardian);
        self.updated_at = current_timestamp();
        Ok(())
    }
    
    /// Remove a guardian (tombstone)
    pub fn remove_guardian(&mut self, guardian_id: GuardianId) -> crate::Result<()> {
        if !self.guardians.contains_key(&guardian_id) {
            return Err(crate::LedgerError::GuardianNotFound(format!("{:?}", guardian_id)));
        }
        
        self.guardians.remove(&guardian_id);
        self.removed_guardians.insert(guardian_id);
        self.updated_at = current_timestamp();
        Ok(())
    }
    
    /// Bump session epoch
    pub fn bump_session_epoch(&mut self) -> SessionEpoch {
        let old_epoch = self.session_epoch;
        self.session_epoch = old_epoch.increment();
        
        // Clear all presence tickets on epoch bump
        self.presence_tickets.clear();
        
        self.updated_at = current_timestamp();
        self.session_epoch
    }
    
    /// Cache a presence ticket
    pub fn cache_presence_ticket(&mut self, ticket: PresenceTicketCache) {
        self.presence_tickets.insert(ticket.device_id, ticket);
        self.updated_at = current_timestamp();
    }
    
    /// Update policy reference
    pub fn update_policy(&mut self, policy: PolicyReference) {
        self.policy = Some(policy);
        self.updated_at = current_timestamp();
    }
    
    /// Start a cooldown
    pub fn start_cooldown(&mut self, cooldown: CooldownCounter) {
        self.cooldowns.insert(cooldown.operation_id, cooldown);
        self.updated_at = current_timestamp();
    }
    
    /// Remove completed cooldowns
    pub fn cleanup_cooldowns(&mut self) {
        let current_time = current_timestamp();
        self.cooldowns.retain(|_, cooldown| !cooldown.is_complete(current_time));
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
    pub fn advance_lamport_clock(&mut self, received_timestamp: u64) {
        // Lamport clock rule: max(local, received) + 1
        self.lamport_clock = self.lamport_clock.max(received_timestamp) + 1;
        self.updated_at = current_timestamp();
    }
    
    /// Increment local Lamport clock (for locally-generated events)
    ///
    /// Call this when creating a new event on this device.
    /// The returned value should be used as the event's epoch_at_write.
    pub fn increment_lamport_clock(&mut self) -> u64 {
        self.lamport_clock += 1;
        self.updated_at = current_timestamp();
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
            Some(lock) if lock.session_id == session_id => {
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
        self.dkd_commitment_roots.insert(root.session_id, root);
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
        let device = self.devices.get_mut(&device_id).ok_or_else(|| {
            format!("Device {} not found", device_id)
        })?;
        
        device.dkd_commitment_proofs.insert(session_id, proof);
        // Note: Lamport clock advancement should happen at event application time,
        // not during proof storage. This is a no-op for now.
        Ok(())
    }
    
    // ========== Session Management ==========
    
    /// Add a new session to the account state
    pub fn add_session(&mut self, session: Session) {
        self.sessions.insert(session.session_id, session);
        self.updated_at = current_timestamp();
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
    pub fn update_session_status(&mut self, session_id: uuid::Uuid, status: SessionStatus) -> Result<(), String> {
        let session = self.sessions.get_mut(&session_id)
            .ok_or_else(|| format!("Session {} not found", session_id))?;
        
        let timestamp = current_timestamp();
        session.update_status(status, timestamp);
        self.updated_at = timestamp;
        Ok(())
    }
    
    /// Complete a session with outcome
    pub fn complete_session(&mut self, session_id: uuid::Uuid, outcome: SessionOutcome) -> Result<(), String> {
        let session = self.sessions.get_mut(&session_id)
            .ok_or_else(|| format!("Session {} not found", session_id))?;
        
        let timestamp = current_timestamp();
        session.complete(outcome, timestamp);
        self.updated_at = timestamp;
        Ok(())
    }
    
    /// Abort a session with failure
    pub fn abort_session(&mut self, session_id: uuid::Uuid, reason: String, blamed_party: Option<ParticipantId>) -> Result<(), String> {
        let session = self.sessions.get_mut(&session_id)
            .ok_or_else(|| format!("Session {} not found", session_id))?;
        
        let timestamp = current_timestamp();
        session.abort(reason, blamed_party, timestamp);
        self.updated_at = timestamp;
        Ok(())
    }
    
    /// Get all active sessions (non-terminal)
    pub fn active_sessions(&self) -> Vec<&Session> {
        self.sessions.values()
            .filter(|session| !session.is_terminal())
            .collect()
    }
    
    /// Get sessions by protocol type
    pub fn sessions_by_protocol(&self, protocol_type: ProtocolType) -> Vec<&Session> {
        self.sessions.values()
            .filter(|session| session.protocol_type == protocol_type)
            .collect()
    }
    
    /// Check if any session of given protocol type is active
    pub fn has_active_session_of_type(&self, protocol_type: ProtocolType) -> bool {
        self.sessions.values()
            .any(|session| session.protocol_type == protocol_type && !session.is_terminal())
    }
    
    /// Clean up expired sessions
    pub fn cleanup_expired_sessions(&mut self, current_epoch: u64) {
        let expired_sessions: Vec<uuid::Uuid> = self.sessions.iter()
            .filter(|(_, session)| session.is_timed_out(current_epoch))
            .map(|(id, _)| *id)
            .collect();
        
        for session_id in &expired_sessions {
            if let Some(session) = self.sessions.get_mut(session_id) {
                if !session.is_terminal() {
                    let timestamp = current_timestamp();
                    session.update_status(SessionStatus::TimedOut, timestamp);
                }
            }
        }
        
        if !expired_sessions.is_empty() {
            self.updated_at = current_timestamp();
        }
    }
    
    // Lamport clock methods moved earlier in file (lines 271-285)
}

/// Get current Unix timestamp in seconds
pub fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

mod verifying_key_serde {
    use ed25519_dalek::VerifyingKey;
    use serde::{Deserialize, Deserializer, Serializer};
    
    pub fn serialize<S>(key: &VerifyingKey, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(key.as_bytes())
    }
    
    pub fn deserialize<'de, D>(deserializer: D) -> Result<VerifyingKey, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: Vec<u8> = Deserialize::deserialize(deserializer)?;
        VerifyingKey::from_bytes(bytes.as_slice().try_into().map_err(serde::de::Error::custom)?)
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_device(id: u16) -> DeviceMetadata {
        use ed25519_dalek::SigningKey;
        
        let signing_key = SigningKey::from_bytes(&rand::random());
        let public_key = signing_key.verifying_key();
        
        DeviceMetadata {
            device_id: DeviceId(uuid::Uuid::from_u128(id as u128)),
            device_name: format!("Device {}", id),
            device_type: DeviceType::Native,
            public_key,
            added_at: current_timestamp(),
            last_seen: current_timestamp(),
            dkd_commitment_proofs: std::collections::BTreeMap::new(),
        }
    }

    #[test]
    fn test_account_state_device_lifecycle() {
        use ed25519_dalek::SigningKey;
        
        let account_id = AccountId::new();
        let signing_key = SigningKey::from_bytes(&rand::random());
        let group_public_key = signing_key.verifying_key();
        
        let device1 = mock_device(1);
        let device_id1 = device1.device_id;
        
        let mut state = AccountState::new(account_id, group_public_key, device1, 2, 3);
        
        // Add second device
        let device2 = mock_device(2);
        let device_id2 = device2.device_id;
        state.add_device(device2).unwrap();
        
        assert_eq!(state.active_devices().len(), 2);
        assert!(state.is_device_active(&device_id1));
        assert!(state.is_device_active(&device_id2));
        
        // Remove first device
        state.remove_device(device_id1).unwrap();
        
        assert_eq!(state.active_devices().len(), 1);
        assert!(!state.is_device_active(&device_id1));
        assert!(state.is_device_active(&device_id2));
        assert!(state.removed_devices.contains(&device_id1));
    }

    #[test]
    fn test_session_epoch_bump() {
        use ed25519_dalek::SigningKey;
        
        let account_id = AccountId::new();
        let signing_key = SigningKey::from_bytes(&rand::random());
        let group_public_key = signing_key.verifying_key();
        let device = mock_device(1);
        
        let mut state = AccountState::new(account_id, group_public_key, device, 2, 3);
        
        let initial_epoch = state.session_epoch;
        assert_eq!(initial_epoch.0, 1);
        
        // Add presence ticket
        let ticket = PresenceTicketCache {
            device_id: DeviceId::new(),
            issued_at: current_timestamp(),
            expires_at: current_timestamp() + 3600,
            ticket_digest: [0u8; 32],
        };
        state.cache_presence_ticket(ticket);
        assert_eq!(state.presence_tickets.len(), 1);
        
        // Bump epoch
        let new_epoch = state.bump_session_epoch();
        assert_eq!(new_epoch.0, 2);
        assert_eq!(state.session_epoch.0, 2);
        
        // Presence tickets should be cleared
        assert_eq!(state.presence_tickets.len(), 0);
    }
}

