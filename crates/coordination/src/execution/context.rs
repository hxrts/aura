//! Protocol Context - Execution environment for protocol scripts
//!
//! The ProtocolContext provides the execution environment for Phase 2 protocol scripts.
//! It allows scripts to `yield` instructions (write events, await messages, etc.) and
//! the context handles the actual I/O, resuming the script with results.
//!
//! This enables writing distributed protocols as linear, async "scripts" that look like
//! single-threaded code but can wait for messages from peers.
//!
//! Reference: work/04_declarative_protocol_evolution.md - Phase 2

use super::time::{TimeSource, WakeCondition};
use super::types::*;
use aura_crypto::Effects;
use aura_journal::{AccountLedger, Event, EventType, Session};
/// Transport abstraction for protocol execution
/// 
/// This trait defines the minimal interface that coordination protocols
/// need from the transport layer. Transport implementations provide this.
#[async_trait::async_trait]
pub trait Transport: Send + Sync {
    /// Send a message to a peer
    async fn send_message(&self, peer_id: &str, message: &[u8]) -> Result<(), String>;
    
    /// Broadcast a message to all known peers
    async fn broadcast_message(&self, message: &[u8]) -> Result<(), String>;
    
    /// Check if a peer is reachable
    async fn is_peer_reachable(&self, peer_id: &str) -> bool;
}
/// Stub transport implementation for testing and development
/// 
/// This provides a no-op transport that can be used when testing protocols
/// without actual network communication.
#[derive(Debug, Default, Clone)]
pub struct StubTransport;

#[async_trait::async_trait]
impl Transport for StubTransport {
    async fn send_message(&self, _peer_id: &str, _message: &[u8]) -> Result<(), String> {
        // No-op for testing
        Ok(())
    }
    
    async fn broadcast_message(&self, _message: &[u8]) -> Result<(), String> {
        // No-op for testing
        Ok(())
    }
    
    async fn is_peer_reachable(&self, _peer_id: &str) -> bool {
        // Always return true for testing
        true
    }
}

use ed25519_dalek::SigningKey;
use rand::Rng;
// Note: Using ctx.effects.rng() instead of direct rand usage for injectable effects
use std::collections::VecDeque;
use std::hash::Hasher;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Protocol execution context
///
/// Provides the execution environment for protocol scripts. Scripts yield
/// instructions to this context, which performs the I/O and resumes the script.
///
/// This is the bridge between pure protocol logic (scripts) and side effects
/// (CRDT writes, network communication).
pub struct ProtocolContext {
    /// Session/protocol ID
    pub session_id: Uuid,

    /// This device's ID
    pub device_id: Uuid,

    /// Device signing key for event authentication
    device_key: SigningKey,

    /// Participants in this protocol
    pub participants: Vec<aura_journal::DeviceId>,

    /// Threshold (if applicable)
    pub threshold: Option<usize>,

    /// CRDT ledger
    ledger: Arc<RwLock<AccountLedger>>,

    /// Network transport
    _transport: Arc<dyn Transport>,

    /// Injectable effects (time, randomness)
    pub effects: Effects,

    /// Time source for cooperative yielding (simulation or production)
    time_source: Box<dyn TimeSource>,

    /// Pending events waiting to be processed
    pending_events: VecDeque<Event>,

    /// Events collected by await operations
    _collected_events: Vec<Event>,

    /// Index of last event we've read from the ledger
    last_read_event_index: usize,

    // ========== Protocol-specific fields ==========
    /// New participants for resharing (if applicable)
    pub new_participants: Option<Vec<aura_journal::DeviceId>>,

    /// New threshold for resharing (if applicable)
    pub new_threshold: Option<usize>,

    /// Guardians for recovery (if applicable)
    pub guardians: Option<Vec<aura_journal::GuardianId>>,

    /// Guardian threshold for recovery (if applicable)
    pub guardian_threshold: Option<usize>,

    /// Cooldown hours for recovery (if applicable)
    pub cooldown_hours: Option<u64>,

    /// Whether this device is the recovery initiator
    pub is_recovery_initiator: bool,

    /// Guardian ID if this device is acting as a guardian
    pub guardian_id: Option<aura_journal::GuardianId>,

    /// New device ID for recovery
    pub new_device_id: Option<aura_journal::DeviceId>,

    /// Device secret key for HPKE decryption
    pub device_secret: aura_crypto::HpkePrivateKey,
}

impl ProtocolContext {
    /// Create a new protocol context
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        session_id: Uuid,
        device_id: Uuid,
        participants: Vec<aura_journal::DeviceId>,
        threshold: Option<usize>,
        ledger: Arc<RwLock<AccountLedger>>,
        transport: Arc<dyn Transport>,
        effects: Effects,
        device_key: SigningKey,
        time_source: Box<dyn TimeSource>,
    ) -> Self {
        // Generate a placeholder device secret using injected effects
        let mut rng = effects.rng();
        let device_keypair = aura_crypto::HpkeKeyPair::generate(&mut rng);

        // Register this context with the time source
        time_source.register_context(session_id);

        ProtocolContext {
            session_id,
            device_id,
            device_key,
            participants,
            threshold,
            ledger,
            _transport: transport,
            effects,
            time_source,
            pending_events: VecDeque::new(),
            _collected_events: Vec::new(),
            last_read_event_index: 0,
            new_participants: None,
            new_threshold: None,
            guardians: None,
            guardian_threshold: None,
            cooldown_hours: None,
            is_recovery_initiator: false,
            guardian_id: None,
            new_device_id: None,
            device_secret: device_keypair.private_key,
        }
    }

    /// Sign an event with this device's key
    ///
    /// Computes the signable hash (excluding authorization) and signs it with the device signing key.
    pub fn sign_event(&self, event: &Event) -> Result<ed25519_dalek::Signature, ProtocolError> {
        use ed25519_dalek::Signer;

        let event_hash = event.signable_hash().map_err(|e| ProtocolError {
            session_id: self.session_id,
            error_type: ProtocolErrorType::Other,
            message: format!("Failed to hash event for signing: {:?}", e),
        })?;

        Ok(self.device_key.sign(&event_hash))
    }

    // ========== Phase 2: Instruction Execution ==========

    /// Yield an instruction and wait for its completion
    ///
    /// This is the core of the script execution model. The script yields
    /// an instruction, the context executes it, and returns the result.
    pub async fn execute(
        &mut self,
        instruction: Instruction,
    ) -> Result<InstructionResult, ProtocolError> {
        match instruction {
            Instruction::WriteToLedger(event) => self.write_to_ledger(event).await,

            Instruction::AwaitEvent {
                filter,
                timeout_epochs,
            } => self.await_event(filter, timeout_epochs).await,

            Instruction::AwaitThreshold {
                count,
                filter,
                timeout_epochs,
            } => self.await_threshold(count, filter, timeout_epochs).await,

            Instruction::GetLedgerState => self.get_ledger_state().await,

            Instruction::GetCurrentEpoch => self.get_current_epoch().await,

            Instruction::WaitEpochs(epochs) => self.wait_epochs(epochs).await,

            Instruction::RunSubProtocol {
                protocol_type,
                config,
            } => self.run_sub_protocol(protocol_type, config).await,

            Instruction::CheckForEvent { filter } => self.check_for_event(filter).await,

            Instruction::MarkGuardianSharesForDeletion {
                session_id,
                ttl_hours,
            } => {
                self.mark_guardian_shares_for_deletion(session_id, ttl_hours)
                    .await
            }
            Instruction::CheckSessionCollision {
                operation_type,
                context_id,
            } => {
                self.check_session_collision(operation_type, context_id)
                    .await
            }
        }
    }

    // ========== Instruction Implementations ==========

    async fn write_to_ledger(&mut self, event: Event) -> Result<InstructionResult, ProtocolError> {
        // Write to ledger (may be shared in simulation for instant CRDT sync)
        let mut ledger = self.ledger.write().await;

        ledger
            .append_event(event, &self.effects)
            .map_err(|e| ProtocolError {
                session_id: self.session_id,
                error_type: ProtocolErrorType::Other,
                message: format!("Failed to write event: {:?}", e),
            })?;

        // Note: In production, events would be broadcast via CRDT sync protocol
        // In simulation with shared ledger, the write is immediately visible to all

        // Drop the ledger lock first to avoid deadlocks
        drop(ledger);

        // In simulation mode, immediately refresh our own pending events queue
        // This ensures we see our own events immediately
        if self.time_source.is_simulated() {
            self.refresh_pending_events().await?;

            // Add a small delay to ensure the event is fully committed before notifying
            tokio::task::yield_now().await;

            // Immediately notify all waiting contexts that new events are available
            // This is critical for choreographic coordination to work properly
            self.time_source.notify_events_available().await;
        }

        Ok(InstructionResult::EventWritten)
    }

    async fn await_event(
        &mut self,
        filter: EventFilter,
        timeout_epochs: Option<u64>,
    ) -> Result<InstructionResult, ProtocolError> {
        let start_epoch = self.time_source.current_epoch();

        // Initialize with all existing events from ledger
        self.refresh_pending_events().await?;

        loop {
            // Check current pending events
            if let Some(event) = self.find_matching_event(&filter) {
                return Ok(InstructionResult::EventReceived(event));
            }

            // Determine wake condition
            let condition = if let Some(timeout) = timeout_epochs {
                WakeCondition::TimeoutAt(start_epoch + timeout)
            } else {
                WakeCondition::EventMatching(filter.clone())
            };

            // Yield to time source with specific wake condition
            self.time_source.yield_until(condition).await?;

            // Check for timeout after waking
            if let Some(timeout) = timeout_epochs {
                if self.time_source.current_epoch() >= start_epoch + timeout {
                    return Err(ProtocolError {
                        session_id: self.session_id,
                        error_type: ProtocolErrorType::Timeout,
                        message: "Timed out waiting for event".to_string(),
                    });
                }
            }

            // Refresh pending events from ledger after waking
            self.refresh_pending_events().await?;
        }
    }

    async fn await_threshold(
        &mut self,
        count: usize,
        filter: EventFilter,
        timeout_epochs: Option<u64>,
    ) -> Result<InstructionResult, ProtocolError> {
        let start_epoch = self.time_source.current_epoch();
        let mut collected = Vec::new();

        // Initialize with all existing events from ledger
        self.refresh_pending_events().await?;

        // Check if we already have enough events before entering the loop
        self.collect_matching_events(&filter, &mut collected);

        if collected.len() >= count {
            return Ok(InstructionResult::EventsReceived(collected));
        }

        let mut attempts = 0;
        const MAX_ATTEMPTS: usize = 1000; // Prevent infinite loops

        loop {
            attempts += 1;
            if attempts > MAX_ATTEMPTS {
                return Err(ProtocolError {
                    session_id: self.session_id,
                    error_type: ProtocolErrorType::Timeout,
                    message: format!(
                        "Exceeded maximum attempts ({}) waiting for {} events (got {})",
                        MAX_ATTEMPTS,
                        count,
                        collected.len()
                    ),
                });
            }

            // Use NewEvents wake condition instead of specific conditions for better coordination
            let condition = if let Some(timeout) = timeout_epochs {
                WakeCondition::TimeoutAt(start_epoch + timeout)
            } else {
                WakeCondition::NewEvents // Wait for any new events, not specific patterns
            };

            // Yield to time source
            self.time_source.yield_until(condition).await?;

            // Check for timeout after waking
            if let Some(timeout) = timeout_epochs {
                if self.time_source.current_epoch() >= start_epoch + timeout {
                    return Err(ProtocolError {
                        session_id: self.session_id,
                        error_type: ProtocolErrorType::Timeout,
                        message: format!(
                            "Timed out waiting for {} events (got {}) after {} epochs",
                            count,
                            collected.len(),
                            timeout
                        ),
                    });
                }
            }

            // Refresh pending events from ledger after waking
            self.refresh_pending_events().await?;

            // Check pending events for new matches
            self.collect_matching_events(&filter, &mut collected);

            if collected.len() >= count {
                return Ok(InstructionResult::EventsReceived(collected));
            }

            // Add a small yield to prevent busy waiting in simulation
            if self.time_source.is_simulated() && attempts % 10 == 0 {
                tokio::task::yield_now().await;
            }
        }
    }

    async fn get_ledger_state(&self) -> Result<InstructionResult, ProtocolError> {
        let ledger = self.ledger.read().await;
        let state = ledger.state();

        let snapshot = LedgerStateSnapshot {
            account_id: state.account_id,
            next_nonce: state.next_nonce,
            last_event_hash: state.last_event_hash,
            current_epoch: state.lamport_clock,
        };

        Ok(InstructionResult::LedgerState(snapshot))
    }

    async fn get_current_epoch(&self) -> Result<InstructionResult, ProtocolError> {
        let epoch = self.time_source.current_epoch();
        Ok(InstructionResult::CurrentEpoch(epoch))
    }

    async fn wait_epochs(&self, epochs: u64) -> Result<InstructionResult, ProtocolError> {
        let target_epoch = self.time_source.current_epoch() + epochs;
        self.time_source
            .yield_until(WakeCondition::EpochReached(target_epoch))
            .await?;
        Ok(InstructionResult::EpochsElapsed)
    }

    fn run_sub_protocol(
        &mut self,
        protocol_type: ProtocolType,
        config: ProtocolConfig,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<InstructionResult, ProtocolError>> + Send + '_>,
    > {
        Box::pin(async move {
            use super::types::{ProtocolResult, ProtocolType as PT};

            // Execute sub-protocol using choreographic implementations
            let result = match (protocol_type, config) {
                (
                    PT::Dkd,
                    ProtocolConfig::Dkd {
                        participants: _,
                        threshold: _,
                    },
                ) => {
                    let context_id = vec![]; // Default empty context for protocol execution
                    let key = crate::choreography::dkd_choreography(self, context_id).await?;
                    ProtocolResult::DkdComplete {
                        session_id: self.session_id,
                        derived_key: key,
                    }
                }

                (
                    PT::Resharing,
                    ProtocolConfig::Resharing {
                        new_participants,
                        new_threshold,
                    },
                ) => {
                    let participants_vec: Vec<_> = new_participants.into_iter().collect();
                    let shares = crate::choreography::resharing_choreography(
                        self,
                        Some(new_threshold),
                        Some(participants_vec),
                    )
                    .await?;
                    ProtocolResult::ResharingComplete {
                        session_id: self.session_id,
                        new_share: shares,
                    }
                }

                (
                    PT::Recovery,
                    ProtocolConfig::Recovery {
                        guardians,
                        threshold,
                    },
                ) => {
                    let guardians_vec: Vec<_> = guardians
                        .into_iter()
                        .map(aura_journal::GuardianId)
                        .collect();
                    let shares = crate::choreography::recovery_choreography(
                        self,
                        guardians_vec,
                        threshold as u16,
                    )
                    .await?;
                    ProtocolResult::RecoveryComplete {
                        recovery_id: self.session_id,
                        recovered_share: shares,
                    }
                }

                (PT::Locking, ProtocolConfig::Locking { operation_type }) => {
                    // Parse operation type from string
                    let op_type = match operation_type.as_str() {
                        "dkd" => aura_journal::OperationType::Dkd,
                        "resharing" => aura_journal::OperationType::Resharing,
                        "recovery" => aura_journal::OperationType::Recovery,
                        _ => aura_journal::OperationType::Dkd, // Default
                    };

                    crate::choreography::locking_choreography(self, op_type).await?;
                    ProtocolResult::LockAcquired {
                        session_id: self.session_id,
                    }
                }

                (PT::Compaction, _) => {
                    return Err(ProtocolError {
                        session_id: self.session_id,
                        error_type: ProtocolErrorType::Other,
                        message: "Compaction protocol not yet implemented".to_string(),
                    });
                }

                // Mismatched protocol type and config
                (ptype, _) => {
                    return Err(ProtocolError {
                        session_id: self.session_id,
                        error_type: ProtocolErrorType::Other,
                        message: format!("Mismatched protocol type {:?} and config", ptype),
                    });
                }
            };

            Ok(InstructionResult::SubProtocolComplete(result))
        })
    }

    // ========== Helper Methods ==========

    /// Refresh pending events from ledger (called after waking from yield)
    async fn refresh_pending_events(&mut self) -> Result<(), ProtocolError> {
        let ledger = self.ledger.read().await;

        // Get all events from the ledger
        let events = ledger.event_log();

        // Always re-scan the entire event log to ensure we don't miss any events
        // This is more robust in the face of concurrent writes to the shared ledger
        // Clear existing pending events and rebuild from the ledger
        self.pending_events.clear();
        self.last_read_event_index = 0;

        // Add all events from the ledger
        for event in events.iter() {
            self.pending_events.push_back(event.clone());
        }
        self.last_read_event_index = events.len();

        Ok(())
    }

    /// Find and remove a matching event from pending events
    fn find_matching_event(&mut self, filter: &EventFilter) -> Option<Event> {
        if let Some(pos) = self
            .pending_events
            .iter()
            .position(|event| self.matches_filter(event, filter))
        {
            self.pending_events.remove(pos)
        } else {
            None
        }
    }

    /// Collect all matching events from pending events
    fn collect_matching_events(&mut self, filter: &EventFilter, collected: &mut Vec<Event>) {
        let mut i = 0;
        while i < self.pending_events.len() {
            if self.matches_filter(&self.pending_events[i], filter) {
                let event = self.pending_events.remove(i).unwrap();
                collected.push(event);
            } else {
                i += 1;
            }
        }
    }

    /// Check if an event matches a filter
    fn matches_filter(&self, event: &Event, filter: &EventFilter) -> bool {
        // Check session ID
        if let Some(session_id) = &filter.session_id {
            let event_session = extract_session_id(event);
            if event_session.as_ref() != Some(session_id) {
                return false;
            }
        }

        // Check event types
        if let Some(event_types) = &filter.event_types {
            if !event_types
                .iter()
                .any(|pat| matches_event_type(&event.event_type, pat))
            {
                return false;
            }
        }

        // Check authors
        if let Some(authors) = &filter.authors {
            let author = match &event.authorization {
                aura_journal::EventAuthorization::DeviceCertificate { device_id, .. } => device_id,
                _ => return false,
            };

            if !authors.contains(author) {
                return false;
            }
        }

        // Check predicate
        if let Some(predicate) = &filter.predicate {
            if !self.eval_predicate(event, predicate) {
                return false;
            }
        }

        true
    }

    #[allow(clippy::only_used_in_recursion)]
    fn eval_predicate(&self, event: &Event, predicate: &EventPredicate) -> bool {
        match predicate {
            EventPredicate::AuthorIn(device_ids) => match &event.authorization {
                aura_journal::EventAuthorization::DeviceCertificate { device_id, .. } => {
                    device_ids.contains(device_id)
                }
                _ => false,
            },

            EventPredicate::EpochGreaterThan(epoch) => event.epoch_at_write > *epoch,

            EventPredicate::And(a, b) => {
                self.eval_predicate(event, a) && self.eval_predicate(event, b)
            }

            EventPredicate::Or(a, b) => {
                self.eval_predicate(event, a) || self.eval_predicate(event, b)
            }
        }
    }

    /// Add an event to the pending queue (called by event watcher)
    pub fn push_event(&mut self, event: Event) {
        self.pending_events.push_back(event);
    }

    /// Notify this context that new events are available
    pub fn notify_new_events(&mut self, events: Vec<Event>) {
        self.pending_events.extend(events);
        // Note: In simulation, the scheduler will handle waking this context
        // In production, the time source will handle notifications via event bus
    }

    // ========== Placeholder Methods for MVP ==========

    /// Get key share (placeholder implementation)
    pub async fn get_key_share(&self) -> Result<Vec<u8>, ProtocolError> {
        // Placeholder: return dummy key share
        Ok(vec![0u8; 32])
    }

    /// Set key share (placeholder implementation)
    pub async fn set_key_share(&mut self, _share: Vec<u8>) -> Result<(), ProtocolError> {
        // Placeholder: would store the new share
        Ok(())
    }

    /// Get guardian share (placeholder implementation)
    pub async fn get_guardian_share(&self) -> Result<Vec<u8>, ProtocolError> {
        // Placeholder: return dummy guardian share
        Ok(vec![0u8; 32])
    }

    /// Generate nonce (placeholder implementation)
    pub async fn generate_nonce(&self) -> Result<u64, ProtocolError> {
        // Generate truly unique nonce by combining multiple sources of entropy
        let timestamp = self.effects.time.current_timestamp().unwrap_or(0);
        let device_hash = {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            hasher.write(self.device_id.as_bytes());
            hasher.finish()
        };

        // Add random component for true uniqueness
        let mut rng = self.effects.rng();
        let random_component: u64 = rng.gen();

        // Combine all sources: timestamp + device_hash + random + session_id hash
        let session_hash = {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            hasher.write(self.session_id.as_bytes());
            hasher.finish()
        };

        let unique_nonce = timestamp
            .wrapping_add(device_hash)
            .wrapping_add(random_component)
            .wrapping_add(session_hash);

        Ok(unique_nonce)
    }

    /// Get Merkle proof (placeholder implementation)
    pub async fn get_merkle_proof(&self) -> Result<Vec<u8>, ProtocolError> {
        // Placeholder: return dummy proof
        Ok(vec![0u8; 32])
    }

    /// Get guardian Merkle proof (placeholder implementation)
    pub async fn get_guardian_merkle_proof(
        &self,
        _guardian_id: aura_journal::GuardianId,
    ) -> Result<Vec<u8>, ProtocolError> {
        // Placeholder: return dummy proof
        Ok(vec![0u8; 32])
    }

    /// Get DKD commitment root (placeholder implementation)
    pub async fn get_dkd_commitment_root(&self) -> Result<[u8; 32], ProtocolError> {
        // Placeholder: return dummy root
        Ok([0u8; 32])
    }

    // ========== Setter Methods for Protocol Configuration ==========

    /// Set new participants for resharing
    pub fn set_new_participants(
        &mut self,
        participants: Vec<aura_journal::DeviceId>,
    ) -> Result<(), ProtocolError> {
        self.new_participants = Some(participants);
        Ok(())
    }

    /// Set new threshold for resharing
    pub fn set_new_threshold(&mut self, threshold: usize) -> Result<(), ProtocolError> {
        self.new_threshold = Some(threshold);
        Ok(())
    }

    /// Set guardians for recovery
    pub fn set_guardians(
        &mut self,
        guardians: Vec<aura_journal::GuardianId>,
    ) -> Result<(), ProtocolError> {
        self.guardians = Some(guardians);
        Ok(())
    }

    /// Set guardian threshold for recovery
    pub fn set_guardian_threshold(&mut self, threshold: usize) -> Result<(), ProtocolError> {
        self.guardian_threshold = Some(threshold);
        Ok(())
    }

    /// Set cooldown hours for recovery
    pub fn set_cooldown_hours(&mut self, hours: u64) -> Result<(), ProtocolError> {
        self.cooldown_hours = Some(hours);
        Ok(())
    }

    /// Set recovery initiator flag
    pub fn set_recovery_initiator(&mut self, is_initiator: bool) -> Result<(), ProtocolError> {
        self.is_recovery_initiator = is_initiator;
        Ok(())
    }

    /// Set guardian ID
    pub fn set_guardian_id(
        &mut self,
        guardian_id: aura_journal::GuardianId,
    ) -> Result<(), ProtocolError> {
        self.guardian_id = Some(guardian_id);
        Ok(())
    }

    /// Set the new device ID for recovery
    pub fn set_new_device_id(
        &mut self,
        device_id: aura_journal::DeviceId,
    ) -> Result<(), ProtocolError> {
        self.new_device_id = Some(device_id);
        Ok(())
    }

    /// Set context capsule (placeholder)
    pub fn set_context_capsule(
        &mut self,
        _capsule: std::collections::BTreeMap<String, String>,
    ) -> Result<(), ProtocolError> {
        // Placeholder: would store capsule for DKD
        // Using generic map to avoid circular dependency
        Ok(())
    }

    /// Create a copy of the context for sub-protocol execution
    /// Get the HPKE public key for a specific device
    pub async fn get_device_public_key(
        &self,
        device_id: &aura_journal::DeviceId,
    ) -> Result<Vec<u8>, ProtocolError> {
        // For now, generate a deterministic key based on device ID
        // In production, this would fetch from the device metadata in the ledger
        use aura_crypto::Effects;

        // Create deterministic effects based on device ID
        let device_seed = device_id
            .0
            .as_bytes()
            .iter()
            .fold(0u64, |acc, &b| acc.wrapping_mul(31).wrapping_add(b as u64));
        let effects = Effects::deterministic(device_seed, 0);
        let mut rng = effects.rng();

        let keypair = aura_crypto::HpkeKeyPair::generate(&mut rng);
        Ok(keypair.public_key.to_bytes())
    }

    /// Get this device's HPKE private key
    pub async fn get_device_hpke_private_key(
        &self,
    ) -> Result<aura_crypto::HpkePrivateKey, ProtocolError> {
        // Generate the same deterministic key based on this device's ID
        // In production, this would be stored in secure device storage
        use aura_crypto::Effects;

        // Create deterministic effects based on device ID
        let device_seed = self
            .device_id
            .as_bytes()
            .iter()
            .fold(0u64, |acc, &b| acc.wrapping_mul(31).wrapping_add(b as u64));
        let effects = Effects::deterministic(device_seed, 0);
        let mut rng = effects.rng();

        let keypair = aura_crypto::HpkeKeyPair::generate(&mut rng);
        Ok(keypair.private_key)
    }

    pub fn clone_for_subprotocol(&self) -> Self {
        // Generate a new device secret for the cloned context using injected effects
        let mut rng = self.effects.rng();
        let device_keypair = aura_crypto::HpkeKeyPair::generate(&mut rng);

        // Clone the time source (this will create a new context registration)
        let time_source = dyn_clone::clone_box(&*self.time_source);

        ProtocolContext {
            session_id: self.session_id,
            device_id: self.device_id,
            device_key: SigningKey::from_bytes(&self.device_key.to_bytes()),
            participants: self.participants.clone(),
            threshold: self.threshold,
            ledger: self.ledger.clone(),
            _transport: self._transport.clone(),
            effects: self.effects.clone(),
            time_source,
            pending_events: VecDeque::new(), // Fresh queue for sub-protocol
            _collected_events: Vec::new(),
            last_read_event_index: 0,
            new_participants: self.new_participants.clone(),
            new_threshold: self.new_threshold,
            guardians: self.guardians.clone(),
            guardian_threshold: self.guardian_threshold,
            cooldown_hours: self.cooldown_hours,
            is_recovery_initiator: self.is_recovery_initiator,
            guardian_id: self.guardian_id,
            new_device_id: self.new_device_id,
            device_secret: device_keypair.private_key,
        }
    }

    /// Check for existing event without waiting
    async fn check_for_event(
        &mut self,
        filter: EventFilter,
    ) -> Result<InstructionResult, ProtocolError> {
        // Refresh events from ledger first
        self.refresh_pending_events().await?;

        // Check pending events for match
        if let Some(event) = self.find_matching_event(&filter) {
            return Ok(InstructionResult::EventReceived(event));
        }

        // No matching event found
        Err(ProtocolError {
            session_id: self.session_id,
            error_type: ProtocolErrorType::Timeout,
            message: "No matching event found".to_string(),
        })
    }

    /// Mark guardian shares for deletion
    async fn mark_guardian_shares_for_deletion(
        &mut self,
        _session_id: uuid::Uuid,
        _ttl_hours: u64,
    ) -> Result<InstructionResult, ProtocolError> {
        // Placeholder: would mark shares for deletion
        Ok(InstructionResult::EventWritten)
    }

    /// Check for session collision and determine winner via lottery
    async fn check_session_collision(
        &mut self,
        operation_type: aura_journal::OperationType,
        _context_id: Vec<u8>,
    ) -> Result<InstructionResult, ProtocolError> {
        use crate::utils::{compute_lottery_ticket, determine_lock_winner};
        use aura_journal::RequestOperationLockEvent;

        // Refresh events to get latest state
        self.refresh_pending_events().await?;

        // Get current ledger state for last event hash
        let (last_event_hash, existing_sessions) = {
            let ledger = self.ledger.read().await;
            let last_event_hash = ledger.last_event_hash().unwrap_or([0u8; 32]);
            let existing_sessions: Vec<Session> =
                ledger.active_sessions().into_iter().cloned().collect();
            (last_event_hash, existing_sessions)
        };

        // Find all active sessions for this operation type and context
        let mut collision_sessions = Vec::new();
        let mut collision_requests = Vec::new();

        for session in existing_sessions {
            // Check if this session matches our operation type and context
            let protocol_type = match operation_type {
                aura_journal::OperationType::Dkd => aura_journal::ProtocolType::Dkd,
                aura_journal::OperationType::Resharing => aura_journal::ProtocolType::Resharing,
                aura_journal::OperationType::Recovery => aura_journal::ProtocolType::Recovery,
                aura_journal::OperationType::Locking => aura_journal::ProtocolType::Locking,
            };

            if session.protocol_type == protocol_type
                && !session.is_expired(self.time_source.current_epoch())
            {
                // For now, we assume context_id is embedded in session metadata
                // In practice, you'd need to check the actual context from session events
                collision_sessions.push(session.clone());

                // Create a lottery request for this session
                let device_id = if let Some(aura_journal::ParticipantId::Device(id)) =
                    session.participants.first()
                {
                    id
                } else {
                    continue;
                };

                let lottery_ticket = compute_lottery_ticket(device_id, &last_event_hash);
                collision_requests.push(RequestOperationLockEvent {
                    operation_type,
                    session_id: session.session_id.0,
                    device_id: *device_id,
                    lottery_ticket,
                });
            }
        }

        // Add our own request to the lottery
        let my_device_id = aura_journal::DeviceId(self.device_id);
        let my_ticket = compute_lottery_ticket(&my_device_id, &last_event_hash);
        collision_requests.push(RequestOperationLockEvent {
            operation_type,
            session_id: self.session_id,
            device_id: my_device_id,
            lottery_ticket: my_ticket,
        });

        // Determine winner if there's a collision
        let winner = if collision_requests.len() > 1 {
            Some(
                determine_lock_winner(&collision_requests).map_err(|e| ProtocolError {
                    session_id: self.session_id,
                    error_type: ProtocolErrorType::Other,
                    message: format!("Failed to determine lottery winner: {:?}", e),
                })?,
            )
        } else {
            None
        };

        Ok(InstructionResult::SessionStatus {
            existing_sessions: collision_sessions,
            winner,
        })
    }
}

// ========== Helper Functions ==========

fn extract_session_id(event: &Event) -> Option<Uuid> {
    match &event.event_type {
        EventType::InitiateDkdSession(e) => Some(e.session_id),
        EventType::RecordDkdCommitment(e) => Some(e.session_id),
        EventType::RevealDkdPoint(e) => Some(e.session_id),
        EventType::FinalizeDkdSession(e) => Some(e.session_id),
        EventType::InitiateResharing(e) => Some(e.session_id),
        EventType::FinalizeResharing(e) => Some(e.session_id),
        EventType::RequestOperationLock(e) => Some(e.session_id),
        EventType::GrantOperationLock(e) => Some(e.session_id),
        EventType::ReleaseOperationLock(e) => Some(e.session_id),
        _ => None,
    }
}

fn matches_event_type(event_type: &EventType, pattern: &EventTypePattern) -> bool {
    matches!(
        (event_type, pattern),
        (
            EventType::RecordDkdCommitment(_),
            EventTypePattern::DkdCommitment
        ) | (EventType::RevealDkdPoint(_), EventTypePattern::DkdReveal)
            | (
                EventType::FinalizeDkdSession(_),
                EventTypePattern::DkdFinalize
            )
            | (
                EventType::InitiateResharing(_),
                EventTypePattern::InitiateResharing
            )
            | (
                EventType::DistributeSubShare(_),
                EventTypePattern::DistributeSubShare
            )
            | (
                EventType::AcknowledgeSubShare(_),
                EventTypePattern::AcknowledgeSubShare
            )
            | (
                EventType::FinalizeResharing(_),
                EventTypePattern::FinalizeResharing
            )
            | (
                EventType::RequestOperationLock(_),
                EventTypePattern::LockRequest
            )
            | (
                EventType::GrantOperationLock(_),
                EventTypePattern::LockGrant
            )
            | (
                EventType::ReleaseOperationLock(_),
                EventTypePattern::LockRelease
            )
            | (
                EventType::InitiateRecovery(_),
                EventTypePattern::InitiateRecovery
            )
            | (
                EventType::CollectGuardianApproval(_),
                EventTypePattern::CollectGuardianApproval
            )
            | (
                EventType::SubmitRecoveryShare(_),
                EventTypePattern::SubmitRecoveryShare
            )
            | (
                EventType::CompleteRecovery(_),
                EventTypePattern::CompleteRecovery
            )
            | (EventType::AbortRecovery(_), EventTypePattern::AbortRecovery)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_filter_session_id() {
        let effects = aura_crypto::Effects::for_test("test_event_filter_session_id");
        let session_id = Uuid::new_v4();
        let filter = EventFilter {
            session_id: Some(session_id),
            event_types: None,
            authors: None,
            predicate: None,
        };

        // Create mock event
        let event = Event {
            version: 1,
            event_id: aura_journal::EventId::new_with_effects(&effects),
            account_id: aura_journal::AccountId(effects.gen_uuid()),
            timestamp: 1000,
            nonce: 1,
            parent_hash: None,
            epoch_at_write: 100,
            event_type: EventType::InitiateDkdSession(aura_journal::InitiateDkdSessionEvent {
                session_id,
                context_id: vec![],
                participants: vec![],
                threshold: 2,
                ttl_in_epochs: 10,
                start_epoch: 100,
            }),
            authorization: aura_journal::EventAuthorization::DeviceCertificate {
                device_id: aura_journal::DeviceId(Uuid::new_v4()),
                signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
            },
        };

        let device_key = SigningKey::from_bytes(&[0u8; 32]);
        let ledger = Arc::new(RwLock::new(
            AccountLedger::new(aura_journal::AccountState::new(
                aura_journal::AccountId(Uuid::from_bytes([1u8; 16])),
                ed25519_dalek::VerifyingKey::from_bytes(&[0u8; 32]).unwrap(),
                aura_journal::DeviceMetadata {
                    device_id: aura_journal::DeviceId(Uuid::from_bytes([2u8; 16])),
                    device_name: "test-device".to_string(),
                    device_type: aura_journal::DeviceType::Native,
                    public_key: ed25519_dalek::VerifyingKey::from_bytes(&[0u8; 32]).unwrap(),
                    added_at: 0,
                    last_seen: 0,
                    dkd_commitment_proofs: std::collections::BTreeMap::new(),
                },
                2,
                3,
            ))
            .unwrap(),
        ));

        let ctx = ProtocolContext::new(
            session_id,
            Uuid::from_bytes([3u8; 16]),
            vec![],
            None,
            ledger,
            Arc::new(StubTransport::default()),
            Effects::test(),
            device_key,
            Box::new(crate::ProductionTimeSource::new()),
        );

        assert!(ctx.matches_filter(&event, &filter));
    }
}

// TODO: Add Drop implementation once unregister_context is available
// impl Drop for ProtocolContext {
//     fn drop(&mut self) {
//         // Unregister this context from the time source when it's dropped
//         self.time_source.unregister_context(self.session_id);
//     }
// }
