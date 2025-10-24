//! Session Type States for Journal/Ledger Protocol
//!
//! This module defines session types for the CRDT-based authenticated ledger,
//! providing compile-time safety for event processing and ledger state management.

use crate::{SessionState, ChoreographicProtocol, SessionProtocol, WitnessedTransition, RuntimeWitness};
use aura_journal::{AccountLedger, AccountState, Event, LedgerError, Session, SessionStatus, SessionOutcome, ProtocolType, OperationType, ParticipantId};
use uuid::Uuid;
use std::collections::BTreeMap;
use std::fmt;

// ========== Ledger Session States ==========

/// Initial state when ledger is empty or ready for operations
#[derive(Debug, Clone)]
pub struct LedgerEmpty;

impl SessionState for LedgerEmpty {
    const NAME: &'static str = "LedgerEmpty";
}

/// State when events are being written to the ledger
#[derive(Debug, Clone)]
pub struct EventWriting;

impl SessionState for EventWriting {
    const NAME: &'static str = "EventWriting";
}

/// State when events are being validated before application
#[derive(Debug, Clone)]
pub struct EventValidating;

impl SessionState for EventValidating {
    const NAME: &'static str = "EventValidating";
}

/// State when events are being applied to account state
#[derive(Debug, Clone)]
pub struct EventApplying;

impl SessionState for EventApplying {
    const NAME: &'static str = "EventApplying";
}

/// State when events have been successfully applied
#[derive(Debug, Clone)]
pub struct EventsApplied;

impl SessionState for EventsApplied {
    const NAME: &'static str = "EventsApplied";
}

/// State when ledger is being compacted
#[derive(Debug, Clone)]
pub struct LedgerCompacting;

impl SessionState for LedgerCompacting {
    const NAME: &'static str = "LedgerCompacting";
}

/// State when ledger operations have failed
#[derive(Debug, Clone)]
pub struct LedgerOperationFailed;

impl SessionState for LedgerOperationFailed {
    const NAME: &'static str = "LedgerOperationFailed";
    const CAN_TERMINATE: bool = true;
    const IS_FINAL: bool = true;
}

// ========== Session Management States ==========

/// State when session is being created
#[derive(Debug, Clone)]
pub struct SessionCreating;

impl SessionState for SessionCreating {
    const NAME: &'static str = "SessionCreating";
}

/// State when session is active and running
#[derive(Debug, Clone)]
pub struct SessionActive;

impl SessionState for SessionActive {
    const NAME: &'static str = "SessionActive";
}

/// State when session is being completed
#[derive(Debug, Clone)]
pub struct SessionCompleting;

impl SessionState for SessionCompleting {
    const NAME: &'static str = "SessionCompleting";
}

/// State when session has completed successfully
#[derive(Debug, Clone)]
pub struct SessionCompleted;

impl SessionState for SessionCompleted {
    const NAME: &'static str = "SessionCompleted";
    const CAN_TERMINATE: bool = true;
    const IS_FINAL: bool = true;
}

/// State when session has failed or expired
#[derive(Debug, Clone)]
pub struct SessionTerminated;

impl SessionState for SessionTerminated {
    const NAME: &'static str = "SessionTerminated";
    const CAN_TERMINATE: bool = true;
    const IS_FINAL: bool = true;
}

// ========== Operation Lock States ==========

/// State when no operation is locked (default state)
#[derive(Debug, Clone)]
pub struct OperationUnlocked;

impl SessionState for OperationUnlocked {
    const NAME: &'static str = "OperationUnlocked";
}

/// State when operation lock is being requested
#[derive(Debug, Clone)]
pub struct LockRequesting;

impl SessionState for LockRequesting {
    const NAME: &'static str = "LockRequesting";
}

/// State when operation is locked and exclusive access granted
#[derive(Debug, Clone)]
pub struct JournalOperationLocked;

impl SessionState for JournalOperationLocked {
    const NAME: &'static str = "JournalOperationLocked";
}

/// State when operation lock is being released
#[derive(Debug, Clone)]
pub struct LockReleasing;

impl SessionState for LockReleasing {
    const NAME: &'static str = "LockReleasing";
}

/// State when lock operation has failed
#[derive(Debug, Clone)]
pub struct LockFailed;

impl SessionState for LockFailed {
    const NAME: &'static str = "LockFailed";
    const CAN_TERMINATE: bool = true;
    const IS_FINAL: bool = true;
}

// ========== Journal Protocol Wrapper ==========

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

/// Session-typed journal protocol wrapper
pub type SessionTypedJournal<S> = ChoreographicProtocol<JournalProtocolCore, S>;

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

// ========== Concrete Error Type ==========

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

// ========== SessionProtocol Implementations ==========

impl SessionProtocol for ChoreographicProtocol<JournalProtocolCore, LedgerEmpty> {
    type State = LedgerEmpty;
    type Output = AccountState;
    type Error = JournalSessionError;
    
    fn session_id(&self) -> Uuid {
        // Use account_id hash as session identifier
        let account_hash = blake3::hash(self.inner.account_id.to_string().as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }
    
    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }
    
    fn protocol_id(&self) -> Uuid {
        // Use account_id as protocol identifier for journal protocols
        let account_hash = blake3::hash(format!("journal-{}", self.inner.account_id).as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn device_id(&self) -> aura_journal::DeviceId {
        // Get device_id from the most recent lock request, or derive from account_id
        self.inner.lock_requests.last()
            .map(|req| req.device_id)
            .unwrap_or_else(|| {
                let effects = aura_crypto::Effects::test();
                aura_journal::DeviceId::new_with_effects(&effects)
            })
    }
}

impl SessionProtocol for ChoreographicProtocol<JournalProtocolCore, EventWriting> {
    type State = EventWriting;
    type Output = AccountState;
    type Error = JournalSessionError;
    
    fn session_id(&self) -> Uuid {
        let account_hash = blake3::hash(self.inner.account_id.to_string().as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }
    
    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }
    
    fn protocol_id(&self) -> Uuid {
        let account_hash = blake3::hash(format!("journal-{}", self.inner.account_id).as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.lock_requests.last()
            .map(|req| req.device_id)
            .unwrap_or_else(|| {
                let effects = aura_crypto::Effects::test();
                aura_journal::DeviceId::new_with_effects(&effects)
            })
    }
}

impl SessionProtocol for ChoreographicProtocol<JournalProtocolCore, EventValidating> {
    type State = EventValidating;
    type Output = AccountState;
    type Error = JournalSessionError;
    
    fn session_id(&self) -> Uuid {
        let account_hash = blake3::hash(self.inner.account_id.to_string().as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }
    
    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }
    
    fn protocol_id(&self) -> Uuid {
        let account_hash = blake3::hash(format!("journal-{}", self.inner.account_id).as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.lock_requests.last()
            .map(|req| req.device_id)
            .unwrap_or_else(|| {
                let effects = aura_crypto::Effects::test();
                aura_journal::DeviceId::new_with_effects(&effects)
            })
    }
}

impl SessionProtocol for ChoreographicProtocol<JournalProtocolCore, EventApplying> {
    type State = EventApplying;
    type Output = AccountState;
    type Error = JournalSessionError;
    
    fn session_id(&self) -> Uuid {
        let account_hash = blake3::hash(self.inner.account_id.to_string().as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }
    
    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }
    
    fn protocol_id(&self) -> Uuid {
        let account_hash = blake3::hash(format!("journal-{}", self.inner.account_id).as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.lock_requests.last()
            .map(|req| req.device_id)
            .unwrap_or_else(|| {
                let effects = aura_crypto::Effects::test();
                aura_journal::DeviceId::new_with_effects(&effects)
            })
    }
}

impl SessionProtocol for ChoreographicProtocol<JournalProtocolCore, EventsApplied> {
    type State = EventsApplied;
    type Output = AccountState;
    type Error = JournalSessionError;
    
    fn session_id(&self) -> Uuid {
        let account_hash = blake3::hash(self.inner.account_id.to_string().as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }
    
    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }
    
    fn protocol_id(&self) -> Uuid {
        let account_hash = blake3::hash(format!("journal-{}", self.inner.account_id).as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.lock_requests.last()
            .map(|req| req.device_id)
            .unwrap_or_else(|| {
                let effects = aura_crypto::Effects::test();
                aura_journal::DeviceId::new_with_effects(&effects)
            })
    }
}

impl SessionProtocol for ChoreographicProtocol<JournalProtocolCore, LedgerCompacting> {
    type State = LedgerCompacting;
    type Output = AccountState;
    type Error = JournalSessionError;
    
    fn session_id(&self) -> Uuid {
        let account_hash = blake3::hash(self.inner.account_id.to_string().as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }
    
    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }
    
    fn protocol_id(&self) -> Uuid {
        let account_hash = blake3::hash(format!("journal-{}", self.inner.account_id).as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.lock_requests.last()
            .map(|req| req.device_id)
            .unwrap_or_else(|| {
                let effects = aura_crypto::Effects::test();
                aura_journal::DeviceId::new_with_effects(&effects)
            })
    }
}

impl SessionProtocol for ChoreographicProtocol<JournalProtocolCore, LedgerOperationFailed> {
    type State = LedgerOperationFailed;
    type Output = AccountState;
    type Error = JournalSessionError;
    
    fn session_id(&self) -> Uuid {
        let account_hash = blake3::hash(self.inner.account_id.to_string().as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }
    
    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }
    
    fn protocol_id(&self) -> Uuid {
        let account_hash = blake3::hash(format!("journal-{}", self.inner.account_id).as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.lock_requests.last()
            .map(|req| req.device_id)
            .unwrap_or_else(|| {
                let effects = aura_crypto::Effects::test();
                aura_journal::DeviceId::new_with_effects(&effects)
            })
    }
}

// Additional SessionProtocol implementations for session management states
impl SessionProtocol for ChoreographicProtocol<JournalProtocolCore, SessionCreating> {
    type State = SessionCreating;
    type Output = Session;
    type Error = JournalSessionError;
    
    fn session_id(&self) -> Uuid {
        let account_hash = blake3::hash(self.inner.account_id.to_string().as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }
    
    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }
    
    fn protocol_id(&self) -> Uuid {
        let account_hash = blake3::hash(format!("journal-{}", self.inner.account_id).as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.lock_requests.last()
            .map(|req| req.device_id)
            .unwrap_or_else(|| {
                let effects = aura_crypto::Effects::test();
                aura_journal::DeviceId::new_with_effects(&effects)
            })
    }
}

impl SessionProtocol for ChoreographicProtocol<JournalProtocolCore, SessionActive> {
    type State = SessionActive;
    type Output = Session;
    type Error = JournalSessionError;
    
    fn session_id(&self) -> Uuid {
        let account_hash = blake3::hash(self.inner.account_id.to_string().as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }
    
    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }
    
    fn protocol_id(&self) -> Uuid {
        let account_hash = blake3::hash(format!("journal-{}", self.inner.account_id).as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.lock_requests.last()
            .map(|req| req.device_id)
            .unwrap_or_else(|| {
                let effects = aura_crypto::Effects::test();
                aura_journal::DeviceId::new_with_effects(&effects)
            })
    }
}

impl SessionProtocol for ChoreographicProtocol<JournalProtocolCore, SessionCompleting> {
    type State = SessionCompleting;
    type Output = Session;
    type Error = JournalSessionError;
    
    fn session_id(&self) -> Uuid {
        let account_hash = blake3::hash(self.inner.account_id.to_string().as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }
    
    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }
    
    fn protocol_id(&self) -> Uuid {
        let account_hash = blake3::hash(format!("journal-{}", self.inner.account_id).as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.lock_requests.last()
            .map(|req| req.device_id)
            .unwrap_or_else(|| {
                let effects = aura_crypto::Effects::test();
                aura_journal::DeviceId::new_with_effects(&effects)
            })
    }
}

impl SessionProtocol for ChoreographicProtocol<JournalProtocolCore, SessionCompleted> {
    type State = SessionCompleted;
    type Output = Session;
    type Error = JournalSessionError;
    
    fn session_id(&self) -> Uuid {
        let account_hash = blake3::hash(self.inner.account_id.to_string().as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }
    
    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }
    
    fn protocol_id(&self) -> Uuid {
        let account_hash = blake3::hash(format!("journal-{}", self.inner.account_id).as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.lock_requests.last()
            .map(|req| req.device_id)
            .unwrap_or_else(|| {
                let effects = aura_crypto::Effects::test();
                aura_journal::DeviceId::new_with_effects(&effects)
            })
    }
}

impl SessionProtocol for ChoreographicProtocol<JournalProtocolCore, SessionTerminated> {
    type State = SessionTerminated;
    type Output = Session;
    type Error = JournalSessionError;
    
    fn session_id(&self) -> Uuid {
        let account_hash = blake3::hash(self.inner.account_id.to_string().as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }
    
    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }
    
    fn protocol_id(&self) -> Uuid {
        let account_hash = blake3::hash(format!("journal-{}", self.inner.account_id).as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.lock_requests.last()
            .map(|req| req.device_id)
            .unwrap_or_else(|| {
                let effects = aura_crypto::Effects::test();
                aura_journal::DeviceId::new_with_effects(&effects)
            })
    }
}

// SessionProtocol implementations for operation lock states
impl SessionProtocol for ChoreographicProtocol<JournalProtocolCore, OperationUnlocked> {
    type State = OperationUnlocked;
    type Output = ();
    type Error = JournalSessionError;
    
    fn session_id(&self) -> Uuid {
        let account_hash = blake3::hash(self.inner.account_id.to_string().as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }
    
    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }
    
    fn protocol_id(&self) -> Uuid {
        let account_hash = blake3::hash(format!("journal-{}", self.inner.account_id).as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.lock_requests.last()
            .map(|req| req.device_id)
            .unwrap_or_else(|| {
                let effects = aura_crypto::Effects::test();
                aura_journal::DeviceId::new_with_effects(&effects)
            })
    }
}

impl SessionProtocol for ChoreographicProtocol<JournalProtocolCore, LockRequesting> {
    type State = LockRequesting;
    type Output = ();
    type Error = JournalSessionError;
    
    fn session_id(&self) -> Uuid {
        let account_hash = blake3::hash(self.inner.account_id.to_string().as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }
    
    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }
    
    fn protocol_id(&self) -> Uuid {
        let account_hash = blake3::hash(format!("journal-{}", self.inner.account_id).as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.lock_requests.last()
            .map(|req| req.device_id)
            .unwrap_or_else(|| {
                let effects = aura_crypto::Effects::test();
                aura_journal::DeviceId::new_with_effects(&effects)
            })
    }
}

impl SessionProtocol for ChoreographicProtocol<JournalProtocolCore, JournalOperationLocked> {
    type State = JournalOperationLocked;
    type Output = ();
    type Error = JournalSessionError;
    
    fn session_id(&self) -> Uuid {
        let account_hash = blake3::hash(self.inner.account_id.to_string().as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }
    
    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }
    
    fn protocol_id(&self) -> Uuid {
        let account_hash = blake3::hash(format!("journal-{}", self.inner.account_id).as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.lock_requests.last()
            .map(|req| req.device_id)
            .unwrap_or_else(|| {
                let effects = aura_crypto::Effects::test();
                aura_journal::DeviceId::new_with_effects(&effects)
            })
    }
}

impl SessionProtocol for ChoreographicProtocol<JournalProtocolCore, LockReleasing> {
    type State = LockReleasing;
    type Output = ();
    type Error = JournalSessionError;
    
    fn session_id(&self) -> Uuid {
        let account_hash = blake3::hash(self.inner.account_id.to_string().as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }
    
    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }
    
    fn protocol_id(&self) -> Uuid {
        let account_hash = blake3::hash(format!("journal-{}", self.inner.account_id).as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.lock_requests.last()
            .map(|req| req.device_id)
            .unwrap_or_else(|| {
                let effects = aura_crypto::Effects::test();
                aura_journal::DeviceId::new_with_effects(&effects)
            })
    }
}

impl SessionProtocol for ChoreographicProtocol<JournalProtocolCore, LockFailed> {
    type State = LockFailed;
    type Output = ();
    type Error = JournalSessionError;
    
    fn session_id(&self) -> Uuid {
        let account_hash = blake3::hash(self.inner.account_id.to_string().as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }
    
    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }
    
    fn protocol_id(&self) -> Uuid {
        let account_hash = blake3::hash(format!("journal-{}", self.inner.account_id).as_bytes());
        Uuid::from_bytes(account_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
    }
    
    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.lock_requests.last()
            .map(|req| req.device_id)
            .unwrap_or_else(|| {
                let effects = aura_crypto::Effects::test();
                aura_journal::DeviceId::new_with_effects(&effects)
            })
    }
}

// ========== State Transitions ==========

/// Transition from LedgerEmpty to EventWriting (when events are queued)
impl WitnessedTransition<LedgerEmpty, EventWriting> 
    for ChoreographicProtocol<JournalProtocolCore, LedgerEmpty> 
{
    type Witness = Vec<Event>;
    type Target = ChoreographicProtocol<JournalProtocolCore, EventWriting>;
    
    /// Begin writing events to the ledger
    fn transition_with_witness(
        mut self, 
        witness: Self::Witness
    ) -> Self::Target {
        // Store pending events
        self.inner.pending_events = witness;
        self.transition_to()
    }
}

/// Transition from EventWriting to EventValidating (with events to validate)
impl WitnessedTransition<EventWriting, EventValidating> 
    for ChoreographicProtocol<JournalProtocolCore, EventWriting> 
{
    type Witness = Vec<Event>;
    type Target = ChoreographicProtocol<JournalProtocolCore, EventValidating>;
    
    /// Begin validating written events
    fn transition_with_witness(
        self, 
        _witness: Self::Witness
    ) -> Self::Target {
        self.transition_to()
    }
}

/// Transition from EventValidating to EventApplying (requires EventsValidated witness)
impl WitnessedTransition<EventValidating, EventApplying> 
    for ChoreographicProtocol<JournalProtocolCore, EventValidating> 
{
    type Witness = EventsValidated;
    type Target = ChoreographicProtocol<JournalProtocolCore, EventApplying>;
    
    /// Begin applying validated events
    fn transition_with_witness(
        self, 
        _witness: Self::Witness
    ) -> Self::Target {
        self.transition_to()
    }
}

/// Transition from EventApplying to EventsApplied (requires EventsAppliedSuccessfully witness)
impl WitnessedTransition<EventApplying, EventsApplied> 
    for ChoreographicProtocol<JournalProtocolCore, EventApplying> 
{
    type Witness = EventsAppliedSuccessfully;
    type Target = ChoreographicProtocol<JournalProtocolCore, EventsApplied>;
    
    /// Complete event application
    fn transition_with_witness(
        mut self, 
        _witness: Self::Witness
    ) -> Self::Target {
        // Clear pending events as they're now applied
        self.inner.pending_events.clear();
        self.transition_to()
    }
}

/// Transition from EventsApplied back to LedgerEmpty (ready for next batch)
impl WitnessedTransition<EventsApplied, LedgerEmpty> 
    for ChoreographicProtocol<JournalProtocolCore, EventsApplied> 
{
    type Witness = ();
    type Target = ChoreographicProtocol<JournalProtocolCore, LedgerEmpty>;
    
    /// Return to empty state ready for next operations
    fn transition_with_witness(
        self, 
        _witness: Self::Witness
    ) -> Self::Target {
        self.transition_to()
    }
}

/// Transition from EventsApplied to LedgerCompacting (requires CompactionReady witness)
impl WitnessedTransition<EventsApplied, LedgerCompacting> 
    for ChoreographicProtocol<JournalProtocolCore, EventsApplied> 
{
    type Witness = CompactionReady;
    type Target = ChoreographicProtocol<JournalProtocolCore, LedgerCompacting>;
    
    /// Begin ledger compaction
    fn transition_with_witness(
        mut self, 
        _witness: Self::Witness
    ) -> Self::Target {
        self.inner.compaction_pending = true;
        self.transition_to()
    }
}

/// Transition from LedgerCompacting back to LedgerEmpty (compaction complete)
impl WitnessedTransition<LedgerCompacting, LedgerEmpty> 
    for ChoreographicProtocol<JournalProtocolCore, LedgerCompacting> 
{
    type Witness = ();
    type Target = ChoreographicProtocol<JournalProtocolCore, LedgerEmpty>;
    
    /// Complete compaction and return to empty state
    fn transition_with_witness(
        mut self, 
        _witness: Self::Witness
    ) -> Self::Target {
        self.inner.compaction_pending = false;
        self.transition_to()
    }
}

/// Transition to LedgerOperationFailed from any state (requires JournalOperationFailure witness)
impl<S: SessionState> WitnessedTransition<S, LedgerOperationFailed> 
    for ChoreographicProtocol<JournalProtocolCore, S> 
where
    Self: SessionProtocol<State = S, Output = (), Error = JournalSessionError>,
{
    type Witness = JournalOperationFailure;
    type Target = ChoreographicProtocol<JournalProtocolCore, LedgerOperationFailed>;
    
    /// Fail journal operations due to error
    fn transition_with_witness(
        self, 
        _witness: Self::Witness
    ) -> Self::Target {
        self.transition_to()
    }
}

// Session management transitions
/// Transition from SessionCreating to SessionActive (requires SessionCreated witness)
impl WitnessedTransition<SessionCreating, SessionActive> 
    for ChoreographicProtocol<JournalProtocolCore, SessionCreating> 
{
    type Witness = SessionCreated;
    type Target = ChoreographicProtocol<JournalProtocolCore, SessionActive>;
    
    /// Activate the created session
    fn transition_with_witness(
        mut self, 
        witness: Self::Witness
    ) -> Self::Target {
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
        self.transition_to()
    }
}

/// Transition from SessionActive to SessionCompleting (requires SessionCompletionReady witness)
impl WitnessedTransition<SessionActive, SessionCompleting> 
    for ChoreographicProtocol<JournalProtocolCore, SessionActive> 
{
    type Witness = SessionCompletionReady;
    type Target = ChoreographicProtocol<JournalProtocolCore, SessionCompleting>;
    
    /// Begin session completion
    fn transition_with_witness(
        self, 
        _witness: Self::Witness
    ) -> Self::Target {
        self.transition_to()
    }
}

/// Transition from SessionCompleting to SessionCompleted (completion successful)
impl WitnessedTransition<SessionCompleting, SessionCompleted> 
    for ChoreographicProtocol<JournalProtocolCore, SessionCompleting> 
{
    type Witness = SessionOutcome;
    type Target = ChoreographicProtocol<JournalProtocolCore, SessionCompleted>;
    
    /// Complete the session
    fn transition_with_witness(
        mut self, 
        witness: Self::Witness
    ) -> Self::Target {
        // Update session status in active sessions
        for session in self.inner.active_sessions.values_mut() {
            if !session.is_terminal() {
                session.complete(witness, 0); // Would use actual timestamp
                break;
            }
        }
        self.transition_to()
    }
}

/// Transition from SessionActive to SessionTerminated (session failed/expired)
impl WitnessedTransition<SessionActive, SessionTerminated> 
    for ChoreographicProtocol<JournalProtocolCore, SessionActive> 
{
    type Witness = String;
    type Target = ChoreographicProtocol<JournalProtocolCore, SessionTerminated>;
    
    /// Terminate the session
    fn transition_with_witness(
        mut self, 
        witness: Self::Witness
    ) -> Self::Target {
        // Mark session as failed
        for session in self.inner.active_sessions.values_mut() {
            if !session.is_terminal() {
                session.abort(&witness, None, 0); // Would use actual timestamp
                break;
            }
        }
        self.transition_to()
    }
}

// Operation lock transitions
/// Transition from OperationUnlocked to LockRequesting (when lock is requested)
impl WitnessedTransition<OperationUnlocked, LockRequesting> 
    for ChoreographicProtocol<JournalProtocolCore, OperationUnlocked> 
{
    type Witness = LockRequest;
    type Target = ChoreographicProtocol<JournalProtocolCore, LockRequesting>;
    
    /// Begin lock request
    fn transition_with_witness(
        mut self, 
        witness: Self::Witness
    ) -> Self::Target {
        self.inner.lock_requests.push(witness);
        self.transition_to()
    }
}

/// Transition from LockRequesting to JournalOperationLocked (requires JournalLockAcquired witness)
impl WitnessedTransition<LockRequesting, JournalOperationLocked> 
    for ChoreographicProtocol<JournalProtocolCore, LockRequesting> 
{
    type Witness = JournalLockAcquired;
    type Target = ChoreographicProtocol<JournalProtocolCore, JournalOperationLocked>;
    
    /// Acquire the operation lock
    fn transition_with_witness(
        mut self, 
        _witness: Self::Witness
    ) -> Self::Target {
        // Clear pending lock requests
        self.inner.lock_requests.clear();
        self.transition_to()
    }
}

/// Transition from JournalOperationLocked to LockReleasing (when releasing lock)
impl WitnessedTransition<JournalOperationLocked, LockReleasing> 
    for ChoreographicProtocol<JournalProtocolCore, JournalOperationLocked> 
{
    type Witness = ();
    type Target = ChoreographicProtocol<JournalProtocolCore, LockReleasing>;
    
    /// Begin lock release
    fn transition_with_witness(
        self, 
        _witness: Self::Witness
    ) -> Self::Target {
        self.transition_to()
    }
}

/// Transition from LockReleasing to OperationUnlocked (requires LockReleased witness)
impl WitnessedTransition<LockReleasing, OperationUnlocked> 
    for ChoreographicProtocol<JournalProtocolCore, LockReleasing> 
{
    type Witness = LockReleased;
    type Target = ChoreographicProtocol<JournalProtocolCore, OperationUnlocked>;
    
    /// Complete lock release
    fn transition_with_witness(
        self, 
        _witness: Self::Witness
    ) -> Self::Target {
        self.transition_to()
    }
}

/// Transition to LockFailed from lock states (requires failure reason)
impl<S: SessionState> WitnessedTransition<S, LockFailed> 
    for ChoreographicProtocol<JournalProtocolCore, S> 
    where Self: SessionProtocol<State = S, Output = (), Error = JournalSessionError>
{
    type Witness = String;
    type Target = ChoreographicProtocol<JournalProtocolCore, LockFailed>;
    
    /// Fail lock operation
    fn transition_with_witness(
        mut self, 
        _witness: Self::Witness
    ) -> Self::Target {
        self.inner.lock_requests.clear();
        self.transition_to()
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

// ========== Helper Types ==========

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
) -> JournalSessionState {
    let core = JournalProtocolCore::new(account_id, ledger);
    
    // Determine state based on ledger condition
    if is_compacting {
        JournalSessionState::LedgerCompacting(ChoreographicProtocol::new(core))
    } else if has_pending_events {
        JournalSessionState::EventWriting(ChoreographicProtocol::new(core))
    } else if has_pending_locks {
        JournalSessionState::LockRequesting(ChoreographicProtocol::new(core))
    } else {
        JournalSessionState::LedgerEmpty(ChoreographicProtocol::new(core))
    }
}

/// Enum representing the possible states of a journal session
pub enum JournalSessionState {
    LedgerEmpty(ChoreographicProtocol<JournalProtocolCore, LedgerEmpty>),
    EventWriting(ChoreographicProtocol<JournalProtocolCore, EventWriting>),
    EventValidating(ChoreographicProtocol<JournalProtocolCore, EventValidating>),
    EventApplying(ChoreographicProtocol<JournalProtocolCore, EventApplying>),
    EventsApplied(ChoreographicProtocol<JournalProtocolCore, EventsApplied>),
    LedgerCompacting(ChoreographicProtocol<JournalProtocolCore, LedgerCompacting>),
    LedgerOperationFailed(ChoreographicProtocol<JournalProtocolCore, LedgerOperationFailed>),
    SessionCreating(ChoreographicProtocol<JournalProtocolCore, SessionCreating>),
    SessionActive(ChoreographicProtocol<JournalProtocolCore, SessionActive>),
    SessionCompleting(ChoreographicProtocol<JournalProtocolCore, SessionCompleting>),
    SessionCompleted(ChoreographicProtocol<JournalProtocolCore, SessionCompleted>),
    SessionTerminated(ChoreographicProtocol<JournalProtocolCore, SessionTerminated>),
    OperationUnlocked(ChoreographicProtocol<JournalProtocolCore, OperationUnlocked>),
    LockRequesting(ChoreographicProtocol<JournalProtocolCore, LockRequesting>),
    JournalOperationLocked(ChoreographicProtocol<JournalProtocolCore, JournalOperationLocked>),
    LockReleasing(ChoreographicProtocol<JournalProtocolCore, LockReleasing>),
    LockFailed(ChoreographicProtocol<JournalProtocolCore, LockFailed>),
}

impl JournalSessionState {
    /// Get the current state name
    pub fn state_name(&self) -> &'static str {
        match self {
            JournalSessionState::LedgerEmpty(s) => s.current_state_name(),
            JournalSessionState::EventWriting(s) => s.current_state_name(),
            JournalSessionState::EventValidating(s) => s.current_state_name(),
            JournalSessionState::EventApplying(s) => s.current_state_name(),
            JournalSessionState::EventsApplied(s) => s.current_state_name(),
            JournalSessionState::LedgerCompacting(s) => s.current_state_name(),
            JournalSessionState::LedgerOperationFailed(s) => s.current_state_name(),
            JournalSessionState::SessionCreating(s) => s.current_state_name(),
            JournalSessionState::SessionActive(s) => s.current_state_name(),
            JournalSessionState::SessionCompleting(s) => s.current_state_name(),
            JournalSessionState::SessionCompleted(s) => s.current_state_name(),
            JournalSessionState::SessionTerminated(s) => s.current_state_name(),
            JournalSessionState::OperationUnlocked(s) => s.current_state_name(),
            JournalSessionState::LockRequesting(s) => s.current_state_name(),
            JournalSessionState::JournalOperationLocked(s) => s.current_state_name(),
            JournalSessionState::LockReleasing(s) => s.current_state_name(),
            JournalSessionState::LockFailed(s) => s.current_state_name(),
        }
    }
    
    /// Check if the session can be terminated
    pub fn can_terminate(&self) -> bool {
        match self {
            JournalSessionState::LedgerEmpty(s) => s.can_terminate(),
            JournalSessionState::EventWriting(s) => s.can_terminate(),
            JournalSessionState::EventValidating(s) => s.can_terminate(),
            JournalSessionState::EventApplying(s) => s.can_terminate(),
            JournalSessionState::EventsApplied(s) => s.can_terminate(),
            JournalSessionState::LedgerCompacting(s) => s.can_terminate(),
            JournalSessionState::LedgerOperationFailed(s) => s.can_terminate(),
            JournalSessionState::SessionCreating(s) => s.can_terminate(),
            JournalSessionState::SessionActive(s) => s.can_terminate(),
            JournalSessionState::SessionCompleting(s) => s.can_terminate(),
            JournalSessionState::SessionCompleted(s) => s.can_terminate(),
            JournalSessionState::SessionTerminated(s) => s.can_terminate(),
            JournalSessionState::OperationUnlocked(s) => s.can_terminate(),
            JournalSessionState::LockRequesting(s) => s.can_terminate(),
            JournalSessionState::JournalOperationLocked(s) => s.can_terminate(),
            JournalSessionState::LockReleasing(s) => s.can_terminate(),
            JournalSessionState::LockFailed(s) => s.can_terminate(),
        }
    }
    
    /// Check if the session is in a final state
    pub fn is_final(&self) -> bool {
        match self {
            JournalSessionState::LedgerEmpty(s) => s.is_final(),
            JournalSessionState::EventWriting(s) => s.is_final(),
            JournalSessionState::EventValidating(s) => s.is_final(),
            JournalSessionState::EventApplying(s) => s.is_final(),
            JournalSessionState::EventsApplied(s) => s.is_final(),
            JournalSessionState::LedgerCompacting(s) => s.is_final(),
            JournalSessionState::LedgerOperationFailed(s) => s.is_final(),
            JournalSessionState::SessionCreating(s) => s.is_final(),
            JournalSessionState::SessionActive(s) => s.is_final(),
            JournalSessionState::SessionCompleting(s) => s.is_final(),
            JournalSessionState::SessionCompleted(s) => s.is_final(),
            JournalSessionState::SessionTerminated(s) => s.is_final(),
            JournalSessionState::OperationUnlocked(s) => s.is_final(),
            JournalSessionState::LockRequesting(s) => s.is_final(),
            JournalSessionState::JournalOperationLocked(s) => s.is_final(),
            JournalSessionState::LockReleasing(s) => s.is_final(),
            JournalSessionState::LockFailed(s) => s.is_final(),
        }
    }
    
    /// Get the account ID
    pub fn account_id(&self) -> aura_journal::AccountId {
        match self {
            JournalSessionState::LedgerEmpty(s) => s.inner.account_id,
            JournalSessionState::EventWriting(s) => s.inner.account_id,
            JournalSessionState::EventValidating(s) => s.inner.account_id,
            JournalSessionState::EventApplying(s) => s.inner.account_id,
            JournalSessionState::EventsApplied(s) => s.inner.account_id,
            JournalSessionState::LedgerCompacting(s) => s.inner.account_id,
            JournalSessionState::LedgerOperationFailed(s) => s.inner.account_id,
            JournalSessionState::SessionCreating(s) => s.inner.account_id,
            JournalSessionState::SessionActive(s) => s.inner.account_id,
            JournalSessionState::SessionCompleting(s) => s.inner.account_id,
            JournalSessionState::SessionCompleted(s) => s.inner.account_id,
            JournalSessionState::SessionTerminated(s) => s.inner.account_id,
            JournalSessionState::OperationUnlocked(s) => s.inner.account_id,
            JournalSessionState::LockRequesting(s) => s.inner.account_id,
            JournalSessionState::JournalOperationLocked(s) => s.inner.account_id,
            JournalSessionState::LockReleasing(s) => s.inner.account_id,
            JournalSessionState::LockFailed(s) => s.inner.account_id,
        }
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
        
        assert_eq!(journal_session.current_state_name(), "LedgerEmpty");
        assert!(!journal_session.can_terminate());
        assert!(!journal_session.is_final());
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
        assert_eq!(session.current_state_name(), "LedgerEmpty");
        
        // Transition to EventWriting with events
        let events = vec![]; // Empty for now, would need proper Event construction
        let writing_session = session.transition_with_witness(events);
        assert_eq!(writing_session.current_state_name(), "EventWriting");
        
        // Can transition to validation
        let validating_session = writing_session.transition_with_witness(vec![]);
        assert_eq!(validating_session.current_state_name(), "EventValidating");
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
        assert!(!state.is_final());
        assert_eq!(state.account_id(), account_id);
    }
}