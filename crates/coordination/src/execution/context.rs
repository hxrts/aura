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

use super::types::*;
use super::time::{TimeSource, WakeCondition};
use aura_crypto::Effects;
use aura_journal::{AccountLedger, Event, EventType};
use aura_transport::Transport;
use ed25519_dalek::SigningKey;
use std::collections::VecDeque;
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
    pub async fn execute(&mut self, instruction: Instruction) -> Result<InstructionResult, ProtocolError> {
        match instruction {
            Instruction::WriteToLedger(event) => {
                self.write_to_ledger(event).await
            }
            
            Instruction::AwaitEvent { filter, timeout_epochs } => {
                self.await_event(filter, timeout_epochs).await
            }
            
            Instruction::AwaitThreshold {
                count,
                filter,
                timeout_epochs,
            } => {
                self.await_threshold(count, filter, timeout_epochs).await
            }
            
            Instruction::GetLedgerState => {
                self.get_ledger_state().await
            }
            
            Instruction::GetCurrentEpoch => {
                self.get_current_epoch().await
            }
            
            Instruction::WaitEpochs(epochs) => {
                self.wait_epochs(epochs).await
            }
            
            Instruction::RunSubProtocol { protocol_type, config } => {
                self.run_sub_protocol(protocol_type, config).await
            }
            
            Instruction::CheckForEvent { filter } => {
                self.check_for_event(filter).await
            }
            
            Instruction::MarkGuardianSharesForDeletion { session_id, ttl_hours } => {
                self.mark_guardian_shares_for_deletion(session_id, ttl_hours).await
            }
        }
    }
    
    // ========== Instruction Implementations ==========
    
    async fn write_to_ledger(&mut self, event: Event) -> Result<InstructionResult, ProtocolError> {
        // Write to ledger (may be shared in simulation for instant CRDT sync)
        let mut ledger = self.ledger.write().await;
        
        ledger
            .append_event(event)
            .map_err(|e| ProtocolError {
                session_id: self.session_id,
                error_type: ProtocolErrorType::Other,
                message: format!("Failed to write event: {:?}", e),
            })?;
        
        // Note: In production, events would be broadcast via CRDT sync protocol
        // In simulation with shared ledger, the write is immediately visible to all
        
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
        
        loop {
            // Check pending events for matches
            self.collect_matching_events(&filter, &mut collected);
            
            if collected.len() >= count {
                return Ok(InstructionResult::EventsReceived(collected));
            }
            
            // Determine wake condition
            let condition = if let Some(timeout) = timeout_epochs {
                WakeCondition::TimeoutAt(start_epoch + timeout)
            } else {
                WakeCondition::ThresholdEvents { 
                    count: count - collected.len(), 
                    filter: filter.clone() 
                }
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
                            "Timed out waiting for {} events (got {})",
                            count,
                            collected.len()
                        ),
                    });
                }
            }
            
            // Refresh pending events from ledger after waking
            self.refresh_pending_events().await?;
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
        self.time_source.yield_until(WakeCondition::EpochReached(target_epoch)).await?;
        Ok(InstructionResult::EpochsElapsed)
    }
    
    async fn run_sub_protocol(
        &mut self,
        _protocol_type: ProtocolType,
        _config: ProtocolConfig,
    ) -> Result<InstructionResult, ProtocolError> {
        // TODO: Implement sub-protocol execution
        // This would create a new context and run the corresponding script
        Err(ProtocolError {
            session_id: self.session_id,
            error_type: ProtocolErrorType::Other,
            message: "Sub-protocols not yet implemented".to_string(),
        })
    }
    
    // ========== Helper Methods ==========
    
    /// Refresh pending events from ledger (called after waking from yield)
    async fn refresh_pending_events(&mut self) -> Result<(), ProtocolError> {
        let ledger = self.ledger.read().await;
        
        // Get all events from the ledger
        let events = ledger.event_log();
        
        // In shared ledger simulation, completely rebuild pending queue from scratch
        // This ensures we see all events that other participants have written
        self.pending_events.clear();
        for event in events.iter() {
            self.pending_events.push_back(event.clone());
        }
        
        // Update last read index
        self.last_read_event_index = events.len();
        
        Ok(())
    }
    
    
    /// Find and remove a matching event from pending events
    fn find_matching_event(&mut self, filter: &EventFilter) -> Option<Event> {
        if let Some(pos) = self.pending_events.iter().position(|event| self.matches_filter(event, filter)) {
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
            if !event_types.iter().any(|pat| matches_event_type(&event.event_type, pat)) {
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
            EventPredicate::AuthorIn(device_ids) => {
                match &event.authorization {
                    aura_journal::EventAuthorization::DeviceCertificate { device_id, .. } => {
                        device_ids.contains(device_id)
                    }
                    _ => false,
                }
            }
            
            EventPredicate::EpochGreaterThan(epoch) => {
                event.epoch_at_write > *epoch
            }
            
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
        // Placeholder: return current timestamp as nonce
        Ok(self.effects.time.current_timestamp().unwrap_or(0))
    }
    
    /// Get Merkle proof (placeholder implementation)
    pub async fn get_merkle_proof(&self) -> Result<Vec<u8>, ProtocolError> {
        // Placeholder: return dummy proof
        Ok(vec![0u8; 32])
    }
    
    /// Get guardian Merkle proof (placeholder implementation)
    pub async fn get_guardian_merkle_proof(&self, _guardian_id: aura_journal::GuardianId) -> Result<Vec<u8>, ProtocolError> {
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
    pub fn set_new_participants(&mut self, participants: Vec<aura_journal::DeviceId>) -> Result<(), ProtocolError> {
        self.new_participants = Some(participants);
        Ok(())
    }
    
    /// Set new threshold for resharing
    pub fn set_new_threshold(&mut self, threshold: usize) -> Result<(), ProtocolError> {
        self.new_threshold = Some(threshold);
        Ok(())
    }
    
    /// Set guardians for recovery
    pub fn set_guardians(&mut self, guardians: Vec<aura_journal::GuardianId>) -> Result<(), ProtocolError> {
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
    pub fn set_guardian_id(&mut self, guardian_id: aura_journal::GuardianId) -> Result<(), ProtocolError> {
        self.guardian_id = Some(guardian_id);
        Ok(())
    }
    
    /// Set context capsule (placeholder)
    pub fn set_context_capsule(&mut self, _capsule: std::collections::BTreeMap<String, String>) -> Result<(), ProtocolError> {
        // Placeholder: would store capsule for DKD
        // Using generic map to avoid circular dependency
        Ok(())
    }
    
    /// Create a copy of the context for sub-protocol execution
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
    async fn check_for_event(&mut self, filter: EventFilter) -> Result<InstructionResult, ProtocolError> {
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
    matches!((event_type, pattern), 
        (EventType::RecordDkdCommitment(_), EventTypePattern::DkdCommitment) |
        (EventType::RevealDkdPoint(_), EventTypePattern::DkdReveal) |
        (EventType::FinalizeDkdSession(_), EventTypePattern::DkdFinalize) |
        (EventType::InitiateResharing(_), EventTypePattern::InitiateResharing) |
        (EventType::DistributeSubShare(_), EventTypePattern::DistributeSubShare) |
        (EventType::AcknowledgeSubShare(_), EventTypePattern::AcknowledgeSubShare) |
        (EventType::FinalizeResharing(_), EventTypePattern::FinalizeResharing) |
        (EventType::RequestOperationLock(_), EventTypePattern::LockRequest) |
        (EventType::GrantOperationLock(_), EventTypePattern::LockGrant) |
        (EventType::ReleaseOperationLock(_), EventTypePattern::LockRelease) |
        (EventType::InitiateRecovery(_), EventTypePattern::InitiateRecovery) |
        (EventType::CollectGuardianApproval(_), EventTypePattern::CollectGuardianApproval) |
        (EventType::CompleteRecovery(_), EventTypePattern::CompleteRecovery) |
        (EventType::AbortRecovery(_), EventTypePattern::AbortRecovery)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_event_filter_session_id() {
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
            event_id: aura_journal::EventId::new(),
            account_id: aura_journal::AccountId(Uuid::new_v4()),
            timestamp: 1000,
            nonce: 1,
            parent_hash: None,
            epoch_at_write: 100,
            event_type: EventType::InitiateDkdSession(aura_journal::InitiateDkdSessionEvent {
                session_id,
                context_id: vec![],
                participants: vec![],
                threshold: 2,
                start_epoch: 100,
                ttl_in_epochs: 100,
            }),
            authorization: aura_journal::EventAuthorization::DeviceCertificate {
                device_id: aura_journal::DeviceId(Uuid::new_v4()),
                signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
            },
        };
        
        let device_key = SigningKey::from_bytes(&[0u8; 32]);
        let ledger = Arc::new(RwLock::new(AccountLedger::new(
            aura_journal::AccountState::new(
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
            ),
        ).unwrap()));
        
        let ctx = ProtocolContext::new(
            session_id,
            Uuid::from_bytes([3u8; 16]),
            vec![],
            None,
            ledger,
            Arc::new(aura_transport::StubTransport::default()),
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

