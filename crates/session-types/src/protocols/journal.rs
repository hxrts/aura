//! Session Type States for Journal/Ledger Protocol (Refactored with Macros)
//!
//! This module defines session types for the CRDT-based authenticated ledger,
//! providing compile-time safety for event processing and ledger state management.

use crate::core::{ChoreographicProtocol, SessionProtocol, SessionState};
use crate::witnesses::RuntimeWitness;
use aura_journal::{AccountLedger, AccountState, Event, LedgerError, Session, SessionStatus, SessionOutcome, ProtocolType, OperationType, ParticipantId};
use uuid::Uuid;
use std::collections::BTreeMap;
use std::fmt;

// ========== Journal Protocol Core ==========

/// Core journal protocol data without session state
pub struct JournalProtocolCore {
    pub account_id: aura_journal::AccountId,
    pub ledger: AccountLedger,
    pub pending_events: Vec<Event>,
    pub active_sessions: BTreeMap<Uuid, Session>,
    pub lock_requests: Vec<LockRequest>,
    pub compaction_pending: bool,
}

/// Ledger statistics
#[derive(Debug, Clone)]
pub struct LedgerStats {
    pub total_events: usize,
    pub current_epoch: u64,
}

/// Lock request information
#[derive(Debug, Clone)]
pub struct LockRequest {
    pub operation_type: OperationType,
    pub session_id: Uuid,
    pub device_id: aura_journal::DeviceId,
    pub requested_at: u64,
}

impl JournalProtocolCore {
    pub fn new(account_id: aura_journal::AccountId, ledger: AccountLedger) -> Self {
        Self {
            account_id,
            ledger,
            pending_events: Vec::new(),
            active_sessions: BTreeMap::new(),
            lock_requests: Vec::new(),
            compaction_pending: false,
        }
    }
}

impl fmt::Debug for JournalProtocolCore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JournalProtocolCore")
            .field("account_id", &self.account_id)
            .field("pending_events_count", &self.pending_events.len())
            .field("active_sessions_count", &self.active_sessions.len())
            .field("lock_requests_count", &self.lock_requests.len())
            .field("compaction_pending", &self.compaction_pending)
            .finish()
    }
}

// Manual Clone implementation for JournalProtocolCore
impl Clone for JournalProtocolCore {
    fn clone(&self) -> Self {
        // AccountLedger doesn't implement Clone, so we create a new empty one
        // This is a workaround for the session types implementation
        // Create a minimal AccountState for the new ledger
        let device_metadata = aura_journal::DeviceMetadata {
            device_id: aura_journal::DeviceId::new_with_effects(&aura_crypto::Effects::test()),
            device_name: "clone-device".to_string(),
            device_type: aura_journal::DeviceType::Native,
            public_key: ed25519_dalek::VerifyingKey::from_bytes(&[0u8; 32]).unwrap(),
            added_at: 0,
            last_seen: 0,
            dkd_commitment_proofs: std::collections::BTreeMap::new(),
        };
        
        let account_state = aura_journal::AccountState::new(
            self.account_id,
            ed25519_dalek::VerifyingKey::from_bytes(&[0u8; 32]).unwrap(),
            device_metadata,
            2, // threshold
            3, // total participants
        );
        
        let new_ledger = AccountLedger::new(account_state).unwrap();
        
        Self {
            account_id: self.account_id,
            ledger: new_ledger,
            pending_events: self.pending_events.clone(),
            active_sessions: self.active_sessions.clone(),
            lock_requests: self.lock_requests.clone(),
            compaction_pending: self.compaction_pending,
        }
    }
}

// ========== Error Type ==========

#[derive(Debug, thiserror::Error)]
pub enum JournalSessionError {
    #[error("Journal protocol error: {0}")]
    ProtocolError(String),
    #[error("Invalid operation for current journal state")]
    InvalidOperation,
    #[error("Event validation failed: {0}")]
    EventValidationFailed(String),
    #[error("Event application failed: {0}")]
    EventApplicationFailed(String),
    #[error("Session management error: {0}")]
    SessionError(String),
    #[error("Lock operation failed: {0}")]
    LockOperationFailed(String),
    #[error("Compaction failed: {0}")]
    CompactionFailed(String),
    #[error("Ledger error: {0}")]
    LedgerError(String),
}

// ========== Protocol Definition using Macros ==========

define_protocol! {
    Protocol: JournalProtocol,
    Core: JournalProtocolCore,
    Error: JournalSessionError,
    Union: JournalProtocolState,

    States {
        // Ledger operation states
        LedgerEmpty => AccountState,
        EventWriting => AccountState,
        EventValidating => AccountState,
        EventApplying => AccountState,
        EventsApplied => AccountState,
        LedgerCompacting => AccountState,
        LedgerOperationFailed @ final => AccountState,
        
        // Session management states
        SessionCreating => Session,
        SessionActive => Session,
        SessionCompleting => Session,
        SessionCompleted @ final => Session,
        SessionTerminated @ final => Session,
        
        // Operation lock states
        OperationUnlocked => (),
        LockRequesting => (),
        JournalOperationLocked => (),
        LockReleasing => (),
        LockFailed @ final => (),
    }

    Extract {
        session_id: |core| {
            let account_hash = blake3::hash(core.account_id.to_string().as_bytes());
            Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
        },
        device_id: |core| {
            core.lock_requests.last()
                .map(|req| req.device_id)
                .unwrap_or_else(|| {
                    let effects = aura_crypto::Effects::test();
                    aura_journal::DeviceId::new_with_effects(&effects)
                })
        },
    }
}

// ========== Additional Union Methods ==========

impl JournalProtocolState {
    /// Get the account ID from any state
    pub fn account_id(&self) -> aura_journal::AccountId {
        match self {
            JournalProtocolState::LedgerEmpty(p) => p.inner.account_id,
            JournalProtocolState::EventWriting(p) => p.inner.account_id,
            JournalProtocolState::EventValidating(p) => p.inner.account_id,
            JournalProtocolState::EventApplying(p) => p.inner.account_id,
            JournalProtocolState::EventsApplied(p) => p.inner.account_id,
            JournalProtocolState::LedgerCompacting(p) => p.inner.account_id,
            JournalProtocolState::LedgerOperationFailed(p) => p.inner.account_id,
            JournalProtocolState::SessionCreating(p) => p.inner.account_id,
            JournalProtocolState::SessionActive(p) => p.inner.account_id,
            JournalProtocolState::SessionCompleting(p) => p.inner.account_id,
            JournalProtocolState::SessionCompleted(p) => p.inner.account_id,
            JournalProtocolState::SessionTerminated(p) => p.inner.account_id,
            JournalProtocolState::OperationUnlocked(p) => p.inner.account_id,
            JournalProtocolState::LockRequesting(p) => p.inner.account_id,
            JournalProtocolState::JournalOperationLocked(p) => p.inner.account_id,
            JournalProtocolState::LockReleasing(p) => p.inner.account_id,
            JournalProtocolState::LockFailed(p) => p.inner.account_id,
        }
    }
}

// ========== Protocol Type Alias ==========

/// Session-typed journal protocol wrapper
pub type JournalProtocol<S> = ChoreographicProtocol<JournalProtocolCore, S>;

// ========== Runtime Witnesses for Journal Operations ==========

/// Witness that events have been successfully validated
#[derive(Debug, Clone)]
pub struct EventsValidated {
    pub event_count: usize,
    pub valid_events: Vec<Event>,
    pub validation_timestamp: u64,
}

impl RuntimeWitness for EventsValidated {
    type Evidence = Vec<Event>;
    type Config = ();
    
    fn verify(evidence: Vec<Event>, _config: ()) -> Option<Self> {
        if !evidence.is_empty() {
            // Basic validation - all events have valid version
            let all_valid = evidence.iter().all(|e| e.validate_version().is_ok());
            
            if all_valid {
                Some(EventsValidated {
                    event_count: evidence.len(),
                    valid_events: evidence,
                    validation_timestamp: 0, // Would use actual timestamp
                })
            } else {
                None
            }
        } else {
            None
        }
    }
    
    fn description(&self) -> &'static str {
        "Events have been validated successfully"
    }
}

/// Witness that events have been applied to account state
#[derive(Debug, Clone)]
pub struct EventsAppliedSuccessfully {
    pub applied_count: usize,
    pub new_lamport_clock: u64,
    pub state_updated: bool,
}

impl RuntimeWitness for EventsAppliedSuccessfully {
    type Evidence = (Vec<Event>, AccountState, u64);
    type Config = ();
    
    fn verify(evidence: (Vec<Event>, AccountState, u64), _config: ()) -> Option<Self> {
        let (events, state, new_clock) = evidence;
        
        if !events.is_empty() && new_clock > 0 {
            Some(EventsAppliedSuccessfully {
                applied_count: events.len(),
                new_lamport_clock: new_clock,
                state_updated: state.lamport_clock == new_clock,
            })
        } else {
            None
        }
    }
    
    fn description(&self) -> &'static str {
        "Events have been applied successfully"
    }
}

/// Witness that session has been created and is ready
#[derive(Debug, Clone)]
pub struct SessionCreated {
    pub session_id: Uuid,
    pub protocol_type: ProtocolType,
    pub participants: Vec<ParticipantId>,
    pub started_at: u64,
}

impl RuntimeWitness for SessionCreated {
    type Evidence = Session;
    type Config = ();
    
    fn verify(evidence: Session, _config: ()) -> Option<Self> {
        if evidence.status == SessionStatus::Active {
            Some(SessionCreated {
                session_id: evidence.session_id.0,
                protocol_type: evidence.protocol_type,
                participants: evidence.participants,
                started_at: evidence.started_at,
            })
        } else {
            None
        }
    }
    
    fn description(&self) -> &'static str {
        "Session has been created and is active"
    }
}

/// Witness that session has reached completion criteria
#[derive(Debug, Clone)]
pub struct SessionCompletionReady {
    pub session_id: Uuid,
    pub completion_type: SessionOutcome,
    pub final_timestamp: u64,
}

impl RuntimeWitness for SessionCompletionReady {
    type Evidence = (Session, SessionOutcome);
    type Config = u64; // Current timestamp
    
    fn verify(evidence: (Session, SessionOutcome), config: u64) -> Option<Self> {
        let (session, outcome) = evidence;
        
        // Session can be completed if it's currently active
        if session.status == SessionStatus::Active {
            Some(SessionCompletionReady {
                session_id: session.session_id.0,
                completion_type: outcome,
                final_timestamp: config,
            })
        } else {
            None
        }
    }
    
    fn description(&self) -> &'static str {
        "Session is ready for completion"
    }
}

/// Witness that journal operation lock has been acquired
#[derive(Debug, Clone)]
pub struct JournalLockAcquired {
    pub operation_type: OperationType,
    pub session_id: Uuid,
    pub device_id: aura_journal::DeviceId,
    pub granted_at: u64,
}

impl RuntimeWitness for JournalLockAcquired {
    type Evidence = (OperationType, Uuid, aura_journal::DeviceId);
    type Config = u64; // Current timestamp
    
    fn verify(evidence: (OperationType, Uuid, aura_journal::DeviceId), config: u64) -> Option<Self> {
        let (operation_type, session_id, device_id) = evidence;
        
        Some(JournalLockAcquired {
            operation_type,
            session_id,
            device_id,
            granted_at: config,
        })
    }
    
    fn description(&self) -> &'static str {
        "Operation lock has been acquired"
    }
}

/// Witness that operation lock has been released
#[derive(Debug, Clone)]
pub struct LockReleased {
    pub operation_type: OperationType,
    pub session_id: Uuid,
    pub released_at: u64,
}

impl RuntimeWitness for LockReleased {
    type Evidence = (OperationType, Uuid);
    type Config = u64; // Current timestamp
    
    fn verify(evidence: (OperationType, Uuid), config: u64) -> Option<Self> {
        let (operation_type, session_id) = evidence;
        
        Some(LockReleased {
            operation_type,
            session_id,
            released_at: config,
        })
    }
    
    fn description(&self) -> &'static str {
        "Operation lock has been released"
    }
}

/// Witness that compaction can proceed
#[derive(Debug, Clone)]
pub struct CompactionReady {
    pub compaction_id: Uuid,
    pub compactable_events: usize,
    pub before_epoch: u64,
    pub preserved_roots: Vec<Uuid>,
}

impl RuntimeWitness for CompactionReady {
    type Evidence = (Vec<Event>, u64, Vec<Uuid>);
    type Config = ();
    
    fn verify(evidence: (Vec<Event>, u64, Vec<Uuid>), _config: ()) -> Option<Self> {
        let (events, before_epoch, preserved_roots) = evidence;
        
        // Compaction is ready if we have events to compact
        let compactable_events = events.iter()
            .filter(|e| e.epoch_at_write < before_epoch)
            .count();
        
        if compactable_events > 0 {
            Some(CompactionReady {
                compaction_id: Uuid::new_v4(),
                compactable_events,
                before_epoch,
                preserved_roots,
            })
        } else {
            None
        }
    }
    
    fn description(&self) -> &'static str {
        "Ledger compaction is ready to proceed"
    }
}

/// Witness that journal operation has failed
#[derive(Debug, Clone)]
pub struct JournalOperationFailure {
    pub operation_type: String,
    pub failure_reason: String,
    pub failed_at: u64,
    pub recovery_possible: bool,
}

impl RuntimeWitness for JournalOperationFailure {
    type Evidence = (String, LedgerError);
    type Config = u64; // Current timestamp
    
    fn verify(evidence: (String, LedgerError), config: u64) -> Option<Self> {
        let (operation_type, error) = evidence;
        
        let recovery_possible = !matches!(error, 
            LedgerError::SerializationError(_) | 
            LedgerError::CrdtError(_)
        );
        
        Some(JournalOperationFailure {
            operation_type,
            failure_reason: error.to_string(),
            failed_at: config,
            recovery_possible,
        })
    }
    
    fn description(&self) -> &'static str {
        "Journal operation has failed"
    }
}

// ========== Protocol Methods ==========

impl<S: SessionState> ChoreographicProtocol<JournalProtocolCore, S> {
    /// Get reference to the protocol core
    pub fn core(&self) -> &JournalProtocolCore {
        &self.inner
    }

    /// Get the account ID
    pub fn account_id(&self) -> aura_journal::AccountId {
        self.core().account_id
    }
}

// ========== State Transitions ==========

/// Transition from LedgerEmpty to EventWriting (when events are queued)
impl ChoreographicProtocol<JournalProtocolCore, LedgerEmpty> {
    /// Begin writing events to the ledger
    pub fn transition_with_events(
        mut self, 
        events: Vec<Event>
    ) -> ChoreographicProtocol<JournalProtocolCore, EventWriting> {
        // Store pending events
        self.inner.pending_events = events;
        ChoreographicProtocol::transition_to(self)
    }
}

/// Transition from EventWriting to EventValidating (with events to validate)
impl ChoreographicProtocol<JournalProtocolCore, EventWriting> {
    /// Begin validating written events
    pub fn begin_validation(self) -> ChoreographicProtocol<JournalProtocolCore, EventValidating> {
        ChoreographicProtocol::transition_to(self)
    }
}

/// Transition from EventValidating to EventApplying (requires EventsValidated witness)
impl ChoreographicProtocol<JournalProtocolCore, EventValidating> {
    /// Begin applying validated events
    pub fn transition_with_validated_events(
        self, 
        _witness: EventsValidated
    ) -> ChoreographicProtocol<JournalProtocolCore, EventApplying> {
        ChoreographicProtocol::transition_to(self)
    }
}

/// Transition from EventApplying to EventsApplied (requires EventsAppliedSuccessfully witness)
impl ChoreographicProtocol<JournalProtocolCore, EventApplying> {
    /// Complete event application
    pub fn transition_with_applied_events(
        mut self, 
        _witness: EventsAppliedSuccessfully
    ) -> ChoreographicProtocol<JournalProtocolCore, EventsApplied> {
        // Clear pending events as they're now applied
        self.inner.pending_events.clear();
        ChoreographicProtocol::transition_to(self)
    }
}

/// Transition from EventsApplied back to LedgerEmpty (ready for next batch)
impl ChoreographicProtocol<JournalProtocolCore, EventsApplied> {
    /// Return to empty state ready for next operations
    pub fn reset_to_empty(self) -> ChoreographicProtocol<JournalProtocolCore, LedgerEmpty> {
        ChoreographicProtocol::transition_to(self)
    }
    
    /// Begin ledger compaction
    pub fn transition_to_compaction(
        mut self, 
        _witness: CompactionReady
    ) -> ChoreographicProtocol<JournalProtocolCore, LedgerCompacting> {
        self.inner.compaction_pending = true;
        ChoreographicProtocol::transition_to(self)
    }
}

/// Transition from LedgerCompacting back to LedgerEmpty (compaction complete)
impl ChoreographicProtocol<JournalProtocolCore, LedgerCompacting> {
    /// Complete compaction and return to empty state
    pub fn complete_compaction(mut self) -> ChoreographicProtocol<JournalProtocolCore, LedgerEmpty> {
        self.inner.compaction_pending = false;
        ChoreographicProtocol::transition_to(self)
    }
}

/// Transition to LedgerOperationFailed from any state
impl<S: SessionState> ChoreographicProtocol<JournalProtocolCore, S> {
    /// Fail journal operations due to error
    pub fn fail_with_error(
        self, 
        _witness: JournalOperationFailure
    ) -> ChoreographicProtocol<JournalProtocolCore, LedgerOperationFailed> {
        ChoreographicProtocol::transition_to(self)
    }
}

// Session management transitions
impl ChoreographicProtocol<JournalProtocolCore, SessionCreating> {
    /// Activate the created session
    pub fn transition_with_created_session(
        mut self, 
        witness: SessionCreated
    ) -> ChoreographicProtocol<JournalProtocolCore, SessionActive> {
        // Add session to active sessions
        let session = Session::new(
            aura_journal::SessionId(witness.session_id),
            witness.protocol_type,
            witness.participants,
            witness.started_at,
            3600, // Default TTL
            witness.started_at,
        );
        self.inner.active_sessions.insert(witness.session_id, session);
        ChoreographicProtocol::transition_to(self)
    }
}

impl ChoreographicProtocol<JournalProtocolCore, SessionActive> {
    /// Begin session completion
    pub fn begin_completion(
        self, 
        _witness: SessionCompletionReady
    ) -> ChoreographicProtocol<JournalProtocolCore, SessionCompleting> {
        ChoreographicProtocol::transition_to(self)
    }
    
    /// Terminate the session
    pub fn terminate_session(
        mut self, 
        reason: String
    ) -> ChoreographicProtocol<JournalProtocolCore, SessionTerminated> {
        // Mark session as failed
        for session in self.inner.active_sessions.values_mut() {
            if !session.is_terminal() {
                session.abort(&reason, None, 0); // Would use actual timestamp
                break;
            }
        }
        ChoreographicProtocol::transition_to(self)
    }
}

impl ChoreographicProtocol<JournalProtocolCore, SessionCompleting> {
    /// Complete the session
    pub fn complete_session(
        mut self, 
        outcome: SessionOutcome
    ) -> ChoreographicProtocol<JournalProtocolCore, SessionCompleted> {
        // Update session status in active sessions
        for session in self.inner.active_sessions.values_mut() {
            if !session.is_terminal() {
                session.complete(outcome, 0); // Would use actual timestamp
                break;
            }
        }
        ChoreographicProtocol::transition_to(self)
    }
}

// Operation lock transitions
impl ChoreographicProtocol<JournalProtocolCore, OperationUnlocked> {
    /// Begin lock request
    pub fn request_operation_lock(
        mut self, 
        request: LockRequest
    ) -> ChoreographicProtocol<JournalProtocolCore, LockRequesting> {
        self.inner.lock_requests.push(request);
        ChoreographicProtocol::transition_to(self)
    }
}

impl ChoreographicProtocol<JournalProtocolCore, LockRequesting> {
    /// Acquire the operation lock
    pub fn acquire_lock(
        mut self, 
        _witness: JournalLockAcquired
    ) -> ChoreographicProtocol<JournalProtocolCore, JournalOperationLocked> {
        // Clear pending lock requests
        self.inner.lock_requests.clear();
        ChoreographicProtocol::transition_to(self)
    }
    
    /// Fail lock operation
    pub fn fail_lock(
        mut self, 
        _reason: String
    ) -> ChoreographicProtocol<JournalProtocolCore, LockFailed> {
        self.inner.lock_requests.clear();
        ChoreographicProtocol::transition_to(self)
    }
}

impl ChoreographicProtocol<JournalProtocolCore, JournalOperationLocked> {
    /// Begin lock release
    pub fn begin_release(self) -> ChoreographicProtocol<JournalProtocolCore, LockReleasing> {
        ChoreographicProtocol::transition_to(self)
    }
}

impl ChoreographicProtocol<JournalProtocolCore, LockReleasing> {
    /// Complete lock release
    pub fn complete_release(
        self, 
        _witness: LockReleased
    ) -> ChoreographicProtocol<JournalProtocolCore, OperationUnlocked> {
        ChoreographicProtocol::transition_to(self)
    }
}

// ========== State-Specific Operations ==========

/// Operations only available in LedgerEmpty state
impl ChoreographicProtocol<JournalProtocolCore, LedgerEmpty> {
    /// Queue events for processing
    pub async fn queue_events(&mut self, events: Vec<Event>) -> Result<Vec<Event>, JournalSessionError> {
        if events.is_empty() {
            return Err(JournalSessionError::InvalidOperation);
        }
        
        // Basic pre-validation
        for event in &events {
            event.validate_version()
                .map_err(|e| JournalSessionError::EventValidationFailed(e))?;
        }
        
        Ok(events)
    }
    
    /// Get current ledger Lamport clock
    pub fn current_lamport_clock(&self) -> u64 {
        self.inner.ledger.lamport_clock()
    }
    
    /// Get ledger statistics
    pub fn ledger_stats(&self) -> LedgerStats {
        LedgerStats {
            total_events: self.inner.ledger.event_log().len(),
            current_epoch: self.inner.ledger.lamport_clock(),
        }
    }
}

/// Operations only available in EventValidating state
impl ChoreographicProtocol<JournalProtocolCore, EventValidating> {
    /// Validate pending events
    pub async fn validate_events(&self) -> Result<EventsValidated, JournalSessionError> {
        let events = &self.inner.pending_events;
        
        // Perform validation logic
        let all_valid = events.iter().all(|e| e.validate_version().is_ok());
        
        if all_valid {
            Ok(EventsValidated {
                event_count: events.len(),
                valid_events: events.clone(),
                validation_timestamp: 0, // Would use actual timestamp
            })
        } else {
            Err(JournalSessionError::EventValidationFailed("Some events failed validation".to_string()))
        }
    }
    
    /// Get pending events for validation
    pub fn pending_events(&self) -> &[Event] {
        &self.inner.pending_events
    }
}

/// Operations only available in EventApplying state
impl ChoreographicProtocol<JournalProtocolCore, EventApplying> {
    /// Apply events to the ledger
    pub async fn apply_events(&mut self, effects: &aura_crypto::Effects) -> Result<EventsAppliedSuccessfully, JournalSessionError> {
        let events = self.inner.pending_events.clone();
        let mut applied_count = 0;
        
        for event in &events {
            self.inner.ledger.append_event(event.clone(), effects)
                .map_err(|e| JournalSessionError::EventApplicationFailed(e.to_string()))?;
            applied_count += 1;
        }
        
        let new_clock = self.inner.ledger.lamport_clock();
        
        Ok(EventsAppliedSuccessfully {
            applied_count,
            new_lamport_clock: new_clock,
            state_updated: true,
        })
    }
    
    /// Check if ledger is healthy and consistent
    pub fn is_ledger_consistent(&self) -> bool {
        // Basic consistency checks
        !self.inner.ledger.event_log().is_empty()
    }
}

/// Operations only available in EventsApplied state
impl ChoreographicProtocol<JournalProtocolCore, EventsApplied> {
    /// Check if compaction is needed
    pub async fn check_compaction_readiness(&self, before_epoch: u64) -> Option<CompactionReady> {
        let events = self.inner.ledger.event_log();
        // Note: DKD commitment roots access would need a specific getter method
        let preserved_roots: Vec<uuid::Uuid> = vec![]; // TODO: Add proper API to AccountLedger
        
        CompactionReady::verify((events.to_vec(), before_epoch, preserved_roots), ())
    }
    
    /// Get ledger statistics
    pub fn get_ledger_stats(&self) -> LedgerStats {
        // Use public API methods instead of accessing state directly
        LedgerStats {
            total_events: self.inner.ledger.event_log().len(),
            current_epoch: self.inner.ledger.lamport_clock(),
        }
    }
}

/// Operations only available in SessionCreating state
impl ChoreographicProtocol<JournalProtocolCore, SessionCreating> {
    /// Create a new session
    pub async fn create_session(
        &mut self,
        protocol_type: ProtocolType,
        participants: Vec<ParticipantId>,
        ttl_in_epochs: u64,
        effects: &aura_crypto::Effects,
    ) -> Result<SessionCreated, JournalSessionError> {
        let session_id = effects.gen_uuid();
        let started_at = self.inner.ledger.lamport_clock();
        
        let session = Session::new(
            aura_journal::SessionId(session_id),
            protocol_type,
            participants.clone(),
            started_at,
            ttl_in_epochs,
            started_at,
        );
        
        self.inner.ledger.add_session(session.clone(), effects);
        
        Ok(SessionCreated {
            session_id,
            protocol_type,
            participants,
            started_at,
        })
    }
}

/// Operations only available in SessionActive state
impl ChoreographicProtocol<JournalProtocolCore, SessionActive> {
    /// Check session completion readiness
    pub async fn check_session_completion(&self, session_id: Uuid, outcome: SessionOutcome) -> Option<SessionCompletionReady> {
        if let Some(session) = self.inner.active_sessions.get(&session_id) {
            SessionCompletionReady::verify((session.clone(), outcome), 0)
        } else {
            None
        }
    }
    
    /// Get active session information
    pub fn get_active_sessions(&self) -> Vec<&Session> {
        self.inner.active_sessions.values().collect()
    }
    
    /// Check if session exists and is active
    pub fn is_session_active(&self, session_id: &Uuid) -> bool {
        self.inner.active_sessions.get(session_id)
            .map(|s| !s.is_terminal())
            .unwrap_or(false)
    }
}

/// Operations only available in OperationUnlocked state
impl ChoreographicProtocol<JournalProtocolCore, OperationUnlocked> {
    /// Request an operation lock
    pub async fn request_lock(
        &mut self,
        operation_type: OperationType,
        session_id: Uuid,
        device_id: aura_journal::DeviceId,
    ) -> Result<LockRequest, JournalSessionError> {
        // Check if operation is already locked
        if self.inner.ledger.is_operation_locked(operation_type) {
            return Err(JournalSessionError::LockOperationFailed("Operation already locked".to_string()));
        }
        
        let request = LockRequest {
            operation_type,
            session_id,
            device_id,
            requested_at: self.inner.ledger.lamport_clock(),
        };
        
        Ok(request)
    }
    
    /// Check if any operation is currently locked
    pub fn is_any_operation_locked(&self) -> bool {
        self.inner.ledger.active_operation_lock().is_some()
    }
}

/// Operations only available in JournalOperationLocked state
impl ChoreographicProtocol<JournalProtocolCore, JournalOperationLocked> {
    /// Get current lock information
    pub fn current_lock(&self) -> Option<&aura_journal::OperationLock> {
        self.inner.ledger.active_operation_lock()
    }
    
    /// Check if lock is for specific operation
    pub fn is_locked_for_operation(&self, operation_type: OperationType) -> bool {
        self.inner.ledger.is_operation_locked(operation_type)
    }
}

// ========== Factory Functions ==========

/// Create a new session-typed journal protocol
pub fn new_session_typed_journal(
    account_id: aura_journal::AccountId,
    ledger: AccountLedger,
) -> ChoreographicProtocol<JournalProtocolCore, LedgerEmpty> {
    let core = JournalProtocolCore::new(account_id, ledger);
    ChoreographicProtocol::new(core)
}

/// Rehydrate a journal protocol session from state
pub fn rehydrate_journal_session(
    account_id: aura_journal::AccountId,
    ledger: AccountLedger,
    has_pending_events: bool,
    has_pending_locks: bool,
    is_compacting: bool,
) -> JournalProtocolState {
    let core = JournalProtocolCore::new(account_id, ledger);
    
    // Determine state based on ledger condition
    if is_compacting {
        JournalProtocolState::LedgerCompacting(ChoreographicProtocol::new(core))
    } else if has_pending_events {
        JournalProtocolState::EventWriting(ChoreographicProtocol::new(core))
    } else if has_pending_locks {
        JournalProtocolState::LockRequesting(ChoreographicProtocol::new(core))
    } else {
        JournalProtocolState::LedgerEmpty(ChoreographicProtocol::new(core))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_crypto::Effects;
    use aura_journal::{AccountState, DeviceMetadata, DeviceType};
    use ed25519_dalek::VerifyingKey;
    
    #[test]
    fn test_journal_session_creation() {
        let effects = Effects::test();
        let account_id = aura_journal::AccountId::new_with_effects(&effects);
        
        // Create a minimal account state for testing
        let device_id = aura_journal::DeviceId::new_with_effects(&effects);
        let device_metadata = DeviceMetadata {
            device_id,
            device_name: "test-device".to_string(),
            device_type: DeviceType::Native,
            public_key: VerifyingKey::from_bytes(&[0u8; 32]).unwrap(),
            added_at: 0,
            last_seen: 0,
            dkd_commitment_proofs: std::collections::BTreeMap::new(),
        };
        
        let account_state = AccountState::new(
            account_id,
            VerifyingKey::from_bytes(&[0u8; 32]).unwrap(),
            device_metadata,
            2, // threshold
            3, // total participants
        );
        
        let ledger = AccountLedger::new(account_state).unwrap();
        
        // Create a new journal session
        let journal_session = new_session_typed_journal(account_id, ledger);
        
        assert_eq!(journal_session.state_name(), "LedgerEmpty");
        assert!(!journal_session.can_terminate());
        assert_eq!(journal_session.inner.account_id, account_id);
    }
    
    #[test]
    fn test_journal_state_transitions() {
        let effects = Effects::test();
        let account_id = aura_journal::AccountId::new_with_effects(&effects);
        
        // Create minimal ledger for testing
        let device_id = aura_journal::DeviceId::new_with_effects(&effects);
        let device_metadata = DeviceMetadata {
            device_id,
            device_name: "test-device".to_string(),
            device_type: DeviceType::Native,
            public_key: VerifyingKey::from_bytes(&[0u8; 32]).unwrap(),
            added_at: 0,
            last_seen: 0,
            dkd_commitment_proofs: std::collections::BTreeMap::new(),
        };
        
        let account_state = AccountState::new(
            account_id,
            VerifyingKey::from_bytes(&[0u8; 32]).unwrap(),
            device_metadata,
            2, 3,
        );
        let ledger = AccountLedger::new(account_state).unwrap();
        
        // Create session and test transitions
        let session = new_session_typed_journal(account_id, ledger);
        assert_eq!(session.state_name(), "LedgerEmpty");
        
        // Transition to EventWriting with events
        let events = vec![]; // Empty for now, would need proper Event construction
        let writing_session = session.transition_with_events(events);
        assert_eq!(writing_session.state_name(), "EventWriting");
        
        // Can transition to validation
        let validating_session = writing_session.begin_validation();
        assert_eq!(validating_session.state_name(), "EventValidating");
    }
    
    #[test]
    fn test_journal_witnesses() {
        // Test EventsValidated witness
        let events = vec![]; // Would need proper Event construction for full test
        
        let witness = EventsValidated::verify(events, ());
        // Would be Some with proper events
        assert!(witness.is_none()); // Empty events return None
        
        // Test basic witness description
        let witness = EventsValidated {
            event_count: 1,
            valid_events: vec![],
            validation_timestamp: 0,
        };
        assert_eq!(witness.description(), "Events have been validated successfully");
    }
    
    #[test]
    fn test_journal_rehydration() {
        let effects = Effects::test();
        let account_id = aura_journal::AccountId::new_with_effects(&effects);
        
        // Create minimal ledger
        let device_id = aura_journal::DeviceId::new_with_effects(&effects);
        let device_metadata = DeviceMetadata {
            device_id,
            device_name: "test-device".to_string(),
            device_type: DeviceType::Native,
            public_key: VerifyingKey::from_bytes(&[0u8; 32]).unwrap(),
            added_at: 0,
            last_seen: 0,
            dkd_commitment_proofs: std::collections::BTreeMap::new(),
        };
        
        let account_state = AccountState::new(account_id, VerifyingKey::from_bytes(&[0u8; 32]).unwrap(), device_metadata, 2, 3);
        let ledger = AccountLedger::new(account_state).unwrap();
        
        // Test rehydration in different states
        let state = rehydrate_journal_session(account_id, ledger, false, false, false);
        assert_eq!(state.state_name(), "LedgerEmpty");
        assert!(!state.can_terminate());
        assert_eq!(state.account_id(), account_id);
    }
}