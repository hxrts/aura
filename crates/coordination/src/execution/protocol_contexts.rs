//! Protocol-specific contexts
//!
//! This module provides specialized context types for each protocol,
//! extending the base context with protocol-specific fields.

use super::base_context::BaseContext;
use super::time::WakeCondition;
use super::{
    EventFilter, EventPredicate, EventTypePattern, Instruction, InstructionResult,
    LedgerStateSnapshot, ProtocolError, ProtocolErrorType,
};
use aura_journal::{Event, EventType};
use aura_types::{DeviceId, GuardianId};
use uuid::Uuid;

/// Context for DKD protocol - uses only base fields
pub type DkdContext = BaseContext;

/// Context for Resharing protocol
pub struct ResharingContext {
    base: BaseContext,
    /// New participants for resharing
    pub new_participants: Vec<DeviceId>,
    /// New threshold for resharing
    pub new_threshold: usize,
}

impl ResharingContext {
    pub fn new(base: BaseContext, new_participants: Vec<DeviceId>, new_threshold: usize) -> Self {
        ResharingContext {
            base,
            new_participants,
            new_threshold,
        }
    }
}

/// Context for Recovery protocol
pub struct RecoveryContext {
    base: BaseContext,
    /// Guardians for recovery
    pub guardians: Vec<GuardianId>,
    /// Guardian threshold for recovery
    pub guardian_threshold: usize,
    /// Cooldown hours for recovery
    pub cooldown_hours: u64,
    /// Whether this device is the recovery initiator
    pub is_recovery_initiator: bool,
    /// Guardian ID if this device is acting as a guardian
    pub guardian_id: Option<GuardianId>,
    /// New device ID for recovery
    pub new_device_id: Option<DeviceId>,
}

impl RecoveryContext {
    pub fn new(
        base: BaseContext,
        guardians: Vec<GuardianId>,
        guardian_threshold: usize,
        cooldown_hours: u64,
    ) -> Self {
        RecoveryContext {
            base,
            guardians,
            guardian_threshold,
            cooldown_hours,
            is_recovery_initiator: false,
            guardian_id: None,
            new_device_id: None,
        }
    }

    /// Set recovery initiator flag
    pub fn set_recovery_initiator(&mut self, is_initiator: bool) {
        self.is_recovery_initiator = is_initiator;
    }

    /// Set guardian ID
    pub fn set_guardian_id(&mut self, guardian_id: GuardianId) {
        self.guardian_id = Some(guardian_id);
    }

    /// Set the new device ID for recovery
    pub fn set_new_device_id(&mut self, device_id: DeviceId) {
        self.new_device_id = Some(device_id);
    }
}

/// Context for Locking protocol - uses only base fields
pub type LockingContext = BaseContext;

/// Context for Compaction protocol - uses only base fields
pub type CompactionContext = BaseContext;

/// Trait to provide common protocol context functionality
pub trait ProtocolContextTrait {
    /// Get base context reference
    fn base(&self) -> &BaseContext;

    /// Get mutable base context reference
    fn base_mut(&mut self) -> &mut BaseContext;

    /// Execute an instruction
    fn execute(
        &mut self,
        instruction: Instruction,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<InstructionResult, ProtocolError>> + Send + '_>,
    >;
}

// Implement base delegation for all context types
macro_rules! impl_base_delegation {
    ($context_type:ty) => {
        impl ProtocolContextTrait for $context_type {
            fn base(&self) -> &BaseContext {
                &self.base
            }

            fn base_mut(&mut self) -> &mut BaseContext {
                &mut self.base
            }

            fn execute(
                &mut self,
                instruction: Instruction,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<Output = Result<InstructionResult, ProtocolError>>
                        + Send
                        + '_,
                >,
            > {
                Box::pin(async move { execute_instruction(self.base_mut(), instruction).await })
            }
        }

        // Delegate common methods
        impl $context_type {
            pub fn sign_event(
                &self,
                event: &Event,
            ) -> Result<ed25519_dalek::Signature, ProtocolError> {
                self.base.sign_event(event)
            }

            pub async fn get_key_share(&self) -> Result<Vec<u8>, ProtocolError> {
                self.base.get_key_share().await
            }

            pub async fn set_key_share(&mut self, share: Vec<u8>) -> Result<(), ProtocolError> {
                self.base.set_key_share(share).await
            }

            pub async fn generate_nonce(&self) -> Result<u64, ProtocolError> {
                self.base.generate_nonce().await
            }

            pub async fn get_device_public_key(
                &self,
                device_id: &DeviceId,
            ) -> Result<Vec<u8>, ProtocolError> {
                self.base.get_device_public_key(device_id).await
            }

            pub async fn get_device_hpke_private_key(
                &self,
            ) -> Result<aura_crypto::HpkePrivateKey, ProtocolError> {
                self.base.get_device_hpke_private_key().await
            }
        }
    };
}

impl_base_delegation!(ResharingContext);
impl_base_delegation!(RecoveryContext);

// Implement ProtocolContextTrait for BaseContext to support type alias pattern
impl ProtocolContextTrait for BaseContext {
    fn base(&self) -> &BaseContext {
        self
    }

    fn base_mut(&mut self) -> &mut BaseContext {
        self
    }

    fn execute(
        &mut self,
        instruction: Instruction,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<InstructionResult, ProtocolError>> + Send + '_>,
    > {
        Box::pin(async move { execute_instruction(self, instruction).await })
    }
}

// Additional methods specific to recovery
impl RecoveryContext {
    pub async fn get_guardian_share(&self) -> Result<Vec<u8>, ProtocolError> {
        self.base.get_guardian_share().await
    }

    pub async fn get_guardian_merkle_proof(
        &self,
        guardian_id: GuardianId,
    ) -> Result<Vec<u8>, ProtocolError> {
        self.base.get_guardian_merkle_proof(guardian_id).await
    }
}

/// Common instruction execution logic
async fn execute_instruction(
    base: &mut BaseContext,
    instruction: Instruction,
) -> Result<InstructionResult, ProtocolError> {
    match instruction {
        Instruction::WriteToLedger(event) => write_to_ledger(base, event).await,
        Instruction::AwaitEvent {
            filter,
            timeout_epochs,
        } => await_event(base, filter, timeout_epochs).await,
        Instruction::AwaitThreshold {
            count,
            filter,
            timeout_epochs,
        } => await_threshold(base, count, filter, timeout_epochs).await,
        Instruction::GetLedgerState => get_ledger_state(base).await,
        Instruction::GetCurrentEpoch => get_current_epoch(base).await,
        Instruction::WaitEpochs(epochs) => wait_epochs(base, epochs).await,
        Instruction::CheckForEvent { filter } => check_for_event(base, filter).await,
        Instruction::MarkGuardianSharesForDeletion {
            session_id,
            ttl_hours,
        } => mark_guardian_shares_for_deletion(base, session_id, ttl_hours).await,
        Instruction::CheckSessionCollision {
            operation_type,
            context_id,
        } => check_session_collision(base, operation_type, context_id).await,
        Instruction::RunSubProtocol { .. } => Err(ProtocolError {
            session_id: base.session_id,
            error_type: ProtocolErrorType::Other,
            message: "Sub-protocol execution should be handled at a higher level".to_string(),
        }),
    }
}

// ========== Instruction Implementations ==========

async fn write_to_ledger(
    base: &mut BaseContext,
    event: Event,
) -> Result<InstructionResult, ProtocolError> {
    // Store a copy of the event for protocol result collection
    base._collected_events.push(event.clone());

    // Write to ledger (may be shared in simulation for instant CRDT sync)
    let mut ledger = base.ledger.write().await;

    ledger
        .append_event(event, &base.effects)
        .map_err(|e| ProtocolError {
            session_id: base.session_id,
            error_type: ProtocolErrorType::Other,
            message: format!("Failed to write event: {:?}", e),
        })?;

    // Note: In production, events would be broadcast via CRDT sync protocol
    // In simulation with shared ledger, the write is immediately visible to all

    // Drop the ledger lock first to avoid deadlocks
    drop(ledger);

    // In simulation mode, immediately refresh our own pending events queue
    // This ensures we see our own events immediately
    if base.time_source.is_simulated() {
        refresh_pending_events(base).await?;

        // Add a small delay to ensure the event is fully committed before notifying
        tokio::task::yield_now().await;

        // Immediately notify all waiting contexts that new events are available
        // This is critical for choreographic coordination to work properly
        base.time_source.notify_events_available().await;
    }

    Ok(InstructionResult::EventWritten)
}

async fn await_event(
    base: &mut BaseContext,
    filter: EventFilter,
    timeout_epochs: Option<u64>,
) -> Result<InstructionResult, ProtocolError> {
    let start_epoch = base.time_source.current_epoch();

    // Initialize with all existing events from ledger
    refresh_pending_events(base).await?;

    loop {
        // Check current pending events
        if let Some(event) = find_matching_event(base, &filter) {
            return Ok(InstructionResult::EventReceived(event));
        }

        // Determine wake condition
        let condition = if let Some(timeout) = timeout_epochs {
            WakeCondition::TimeoutAt(start_epoch + timeout)
        } else {
            WakeCondition::EventMatching(filter.clone())
        };

        // Yield to time source with specific wake condition
        base.time_source.yield_until(condition).await?;

        // Check for timeout after waking
        if let Some(timeout) = timeout_epochs {
            if base.time_source.current_epoch() >= start_epoch + timeout {
                return Err(ProtocolError {
                    session_id: base.session_id,
                    error_type: ProtocolErrorType::Timeout,
                    message: "Timed out waiting for event".to_string(),
                });
            }
        }

        // Refresh pending events from ledger after waking
        refresh_pending_events(base).await?;
    }
}

async fn await_threshold(
    base: &mut BaseContext,
    count: usize,
    filter: EventFilter,
    timeout_epochs: Option<u64>,
) -> Result<InstructionResult, ProtocolError> {
    let start_epoch = base.time_source.current_epoch();
    let mut collected = Vec::new();

    // Initialize with all existing events from ledger
    refresh_pending_events(base).await?;

    // Check if we already have enough events before entering the loop
    collect_matching_events(base, &filter, &mut collected);

    if collected.len() >= count {
        return Ok(InstructionResult::EventsReceived(collected));
    }

    let mut attempts = 0;
    const MAX_ATTEMPTS: usize = 1000; // Prevent infinite loops

    loop {
        attempts += 1;
        if attempts > MAX_ATTEMPTS {
            return Err(ProtocolError {
                session_id: base.session_id,
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
        base.time_source.yield_until(condition).await?;

        // Check for timeout after waking
        if let Some(timeout) = timeout_epochs {
            if base.time_source.current_epoch() >= start_epoch + timeout {
                return Err(ProtocolError {
                    session_id: base.session_id,
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
        refresh_pending_events(base).await?;

        // Check pending events for new matches
        collect_matching_events(base, &filter, &mut collected);

        if collected.len() >= count {
            return Ok(InstructionResult::EventsReceived(collected));
        }

        // Add a small yield to prevent busy waiting in simulation
        if base.time_source.is_simulated() && attempts % 10 == 0 {
            tokio::task::yield_now().await;
        }
    }
}

async fn get_ledger_state(base: &BaseContext) -> Result<InstructionResult, ProtocolError> {
    let ledger = base.ledger.read().await;
    let state = ledger.state();

    let snapshot = LedgerStateSnapshot {
        account_id: state.account_id,
        next_nonce: state.next_nonce,
        last_event_hash: state.last_event_hash,
        current_epoch: state.lamport_clock,
        relationship_counters: state.relationship_counters.clone(),
    };

    Ok(InstructionResult::LedgerState(snapshot))
}

async fn get_current_epoch(base: &BaseContext) -> Result<InstructionResult, ProtocolError> {
    let epoch = base.time_source.current_epoch();
    Ok(InstructionResult::CurrentEpoch(epoch))
}

async fn wait_epochs(base: &BaseContext, epochs: u64) -> Result<InstructionResult, ProtocolError> {
    let target_epoch = base.time_source.current_epoch() + epochs;
    base.time_source
        .yield_until(WakeCondition::EpochReached(target_epoch))
        .await?;
    Ok(InstructionResult::EpochsElapsed)
}

async fn check_for_event(
    base: &mut BaseContext,
    filter: EventFilter,
) -> Result<InstructionResult, ProtocolError> {
    // Refresh events from ledger first
    refresh_pending_events(base).await?;

    // Check pending events for match
    if let Some(event) = find_matching_event(base, &filter) {
        return Ok(InstructionResult::EventReceived(event));
    }

    // No matching event found
    Err(ProtocolError {
        session_id: base.session_id,
        error_type: ProtocolErrorType::Timeout,
        message: "No matching event found".to_string(),
    })
}

async fn mark_guardian_shares_for_deletion(
    _base: &mut BaseContext,
    _session_id: Uuid,
    _ttl_hours: u64,
) -> Result<InstructionResult, ProtocolError> {
    // Placeholder: would mark shares for deletion
    Ok(InstructionResult::EventWritten)
}

async fn check_session_collision(
    base: &mut BaseContext,
    operation_type: aura_journal::OperationType,
    _context_id: Vec<u8>,
) -> Result<InstructionResult, ProtocolError> {
    use crate::utils::{compute_lottery_ticket, determine_lock_winner};
    use aura_journal::{RequestOperationLockEvent, Session};

    // Refresh events to get latest state
    refresh_pending_events(base).await?;

    // Get current ledger state for last event hash
    let (last_event_hash, existing_sessions) = {
        let ledger = base.ledger.read().await;
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
            aura_journal::OperationType::Counter => aura_journal::ProtocolType::Counter,
            aura_journal::OperationType::Resharing => aura_journal::ProtocolType::Resharing,
            aura_journal::OperationType::Recovery => aura_journal::ProtocolType::Recovery,
            aura_journal::OperationType::Locking => aura_journal::ProtocolType::Locking,
        };

        if session.protocol_type == protocol_type
            && !session.is_expired(base.time_source.current_epoch())
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
                delegated_action: None,
            });
        }
    }

    // Add our own request to the lottery
    let my_device_id = DeviceId(base.device_id);
    let my_ticket = compute_lottery_ticket(&my_device_id, &last_event_hash);
    collision_requests.push(RequestOperationLockEvent {
        operation_type,
        session_id: base.session_id,
        device_id: my_device_id,
        lottery_ticket: my_ticket,
        delegated_action: None,
    });

    // Determine winner if there's a collision
    let winner = if collision_requests.len() > 1 {
        Some(
            determine_lock_winner(&collision_requests).map_err(|e| ProtocolError {
                session_id: base.session_id,
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

// ========== Helper Functions ==========

/// Refresh pending events from ledger (called after waking from yield)
async fn refresh_pending_events(base: &mut BaseContext) -> Result<(), ProtocolError> {
    let ledger = base.ledger.read().await;

    // Get all events from the ledger
    let events = ledger.event_log();

    // Always re-scan the entire event log to ensure we don't miss any events
    // This is more robust in the face of concurrent writes to the shared ledger
    // Clear existing pending events and rebuild from the ledger
    base.pending_events.clear();
    base.last_read_event_index = 0;

    // Add all events from the ledger
    for event in events.iter() {
        base.pending_events.push_back(event.clone());
    }
    base.last_read_event_index = events.len();

    Ok(())
}

/// Find and remove a matching event from pending events
fn find_matching_event(base: &mut BaseContext, filter: &EventFilter) -> Option<Event> {
    if let Some(pos) = base
        .pending_events
        .iter()
        .position(|event| matches_filter(base, event, filter))
    {
        base.pending_events.remove(pos)
    } else {
        None
    }
}

/// Collect all matching events from pending events
fn collect_matching_events(
    base: &mut BaseContext,
    filter: &EventFilter,
    collected: &mut Vec<Event>,
) {
    let mut i = 0;
    while i < base.pending_events.len() {
        if matches_filter(base, &base.pending_events[i], filter) {
            let event = base.pending_events.remove(i).unwrap();
            collected.push(event);
        } else {
            i += 1;
        }
    }
}

/// Check if an event matches a filter
fn matches_filter(base: &BaseContext, event: &Event, filter: &EventFilter) -> bool {
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
        if !eval_predicate(base, event, predicate) {
            return false;
        }
    }

    true
}

#[allow(clippy::only_used_in_recursion)]
fn eval_predicate(_base: &BaseContext, event: &Event, predicate: &EventPredicate) -> bool {
    match predicate {
        EventPredicate::AuthorIn(device_ids) => match &event.authorization {
            aura_journal::EventAuthorization::DeviceCertificate { device_id, .. } => {
                device_ids.contains(device_id)
            }
            _ => false,
        },

        EventPredicate::EpochGreaterThan(epoch) => event.epoch_at_write > *epoch,

        EventPredicate::And(a, b) => {
            eval_predicate(_base, event, a) && eval_predicate(_base, event, b)
        }

        EventPredicate::Or(a, b) => {
            eval_predicate(_base, event, a) || eval_predicate(_base, event, b)
        }
    }
}

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
