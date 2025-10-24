//! Protocol Choreography for Aura
//!
//! This crate implements Aura's **choreographic programming** architecture for
//! coordinating distributed protocols (DKD, resharing, locking, recovery).
//!
//! # Choreographic Programming with Session Types
//!
//! **Choreographic Protocol System**
//! - **Global viewpoint**: Protocols written as single programs
//! - **Session types**: Communication patterns type-checked at compile time
//! - **Deadlock freedom**: Guaranteed by choreographic structure
//! - **Local projection**: Context automatically determines device actions
//! - **Pure & testable**: No CRDT/network required for unit tests
//!
//! **Architecture:**
//! - Choreographic programming with session types
//! - Global viewpoint protocols with automatic local projection
//!
//! # Main Components
//!
//! **Choreographic Protocols:**
//! - [`choreography`]: Protocol implementations as choreographies
//!   - [`choreography::dkd`]: P2P deterministic key derivation
//!   - [`choreography::resharing`]: Share redistribution protocol
//!   - [`choreography::recovery`]: Guardian-based recovery

#![allow(missing_docs)] // TODO: Add comprehensive documentation in future work
//!   - [`choreography::locking`]: Distributed locking protocol
//!
//! **Execution Infrastructure:**
//! - [`execution`]: Protocol execution infrastructure
//!   - [`execution::ProtocolContext`]: Choreographic execution environment
//!   - [`execution::TimeSource`]: Time abstraction for testing
//!
//! **Utilities:**
//! - [`utils`]: Coordination utilities
//!   - [`utils::EventWatcher`]: CRDT event watching
//!   - [`utils::SigningUtils`]: Event signing utilities
//!   - [`utils::LotteryProtocol`]: Distributed lottery for conflict resolution
//!
//! # Coordination Model
//!
//! Aura uses a **CRDT-based (Automerge) distributed ledger** for coordination.
//! Choreographies describe protocols from a global viewpoint, and the Context
//! performs local projection to determine which actions apply to each device.
//! No centralized coordinator needed.
//!
//! # Example Choreography
//!
//! ```rust,ignore
//! pub async fn dkd_choreography(ctx: &mut ProtocolContext) -> Result<Vec<u8>> {
//!     // All parties broadcast commitments
//!     ctx.execute(Instruction::WriteToLedger(commitment)).await?;
//!     
//!     // Wait for threshold commitments (choreographic synchronization)
//!     let peers = ctx.execute(Instruction::AwaitThreshold {
//!         count: threshold,
//!         filter: commitment_filter(),
//!     }).await?;
//!     
//!     // Continue with reveals...
//! }
//! ```
//!
//! # References
//!
//! - work/04_declarative_protocol_evolution.md - Architectural evolution
//! - Choreographic Programming: https://arxiv.org/abs/1303.0039
//! - Session Types: https://arxiv.org/abs/1603.03727

// ========== Modular Architecture ==========
pub mod channels;
pub mod choreography;
pub mod execution;
pub mod local_runtime;
pub mod utils;

// ========== Error Types ==========
pub mod error;
pub use error::{CoordinationError, Result};

// ========== Main API Exports ==========

// Choreographic protocols
pub use choreography::{dkd, locking, recovery, resharing};

// Execution infrastructure
pub use execution::{
    EventFilter, EventPredicate, EventTypePattern, Instruction, InstructionResult,
    LedgerStateSnapshot, ProductionTimeSource, ProtocolConfig, ProtocolContext, ProtocolError,
    ProtocolErrorType, ProtocolResult, ProtocolType, TimeSource,
    // SimulatedTimeSource, SimulationScheduler, // TODO: These types are not yet fully implemented
};

// Transport abstraction
pub use execution::context::{Transport, StubTransport};

// Session types for choreographic protocols (re-exported from aura-session-types)
pub use aura_session_types::{
    ChoreographicProtocol, SessionProtocol, SessionState, WitnessedTransition,
    RuntimeWitness, ProtocolRehydration, RehydrationEvidence, StateAnalysis,
    CollectedCommitments, VerifiedReveals, ApprovalThresholdMet, SharesCollected,
    ThresholdEventsMet, LedgerWriteComplete, SubProtocolComplete,
    HandshakeCompleted, TicketsValidated, MessageDelivered, BroadcastCompleted,
    CommitmentThresholdMet, SignatureShareThresholdMet, SignatureAggregated,
    EventsValidated, EventsAppliedSuccessfully, SessionCreated, SessionCompletionReady,
    KeyGenerationCompleted, FrostResharingCompleted, FrostProtocolFailure,
    CgkaGroupInitiated, MembershipChangeReady, EpochTransitionReady, GroupStabilized,
    OperationValidated, OperationAppliedSuccessfully, TreeUpdatesCompleted,
    AccountInitialized, AccountConfigLoaded, CommandCompleted,
    
    // DKD session types
    DkdProtocolState, new_dkd_protocol, rehydrate_dkd_protocol,
    InitializationPhase, CommitmentPhase, RevealPhase, FinalizationPhase, CompletionPhase, Failure,
    DkdCompleted, DkdProtocolCore, DkdSessionError,
    
    // Agent session types
    AgentIdle, DkdInProgress, RecoveryInProgress, ResharingInProgress, 
    LockingInProgress, AgentOperationLocked, AgentFailed,
    SessionTypedAgent, DeviceAgentCore, AgentSessionError, AgentSessionState,
    new_session_typed_agent, rehydrate_agent_session,
    
    // Recovery session types
    RecoveryInitialized, CollectingApprovals, EnforcingCooldown, CollectingShares,
    ReconstructingKey, RecoveryProtocolCompleted, RecoveryAborted,
    SessionTypedRecovery, RecoveryProtocolCore, RecoverySessionError, RecoverySessionState,
    new_session_typed_recovery, rehydrate_recovery_session,
    
    // Transport session types
    TransportDisconnected, ConnectionHandshaking, TicketValidating, TransportConnected,
    MessageSending, Broadcasting, ConnectionFailed, AwaitingMessage, ProcessingMessage,
    RequestResponseActive, SessionTypedTransport, TransportProtocolCore,
    TransportSessionError, TransportSessionState,
    new_session_typed_transport, rehydrate_transport_session,
    
    // FROST session types
    FrostIdle, FrostCommitmentPhase, FrostAwaitingCommitments, FrostSigningPhase,
    FrostAwaitingShares, FrostReadyToAggregate, FrostSignatureComplete, FrostSigningFailed,
    SessionTypedFrost, FrostProtocolCore, FrostSessionError, FrostSessionState,
    new_session_typed_frost, rehydrate_frost_session,
    
    // CGKA session types
    CgkaGroupInitialized, GroupMembershipChange, EpochTransition, GroupStable,
    GroupOperationFailed, OperationPending, OperationValidating, OperationApplying,
    OperationApplied, OperationFailed, TreeBuilding, TreeUpdating, TreeComplete, TreeFailed,
    SessionTypedCgka, CgkaProtocolCore, CgkaSessionError, CgkaSessionState,
    new_session_typed_cgka, rehydrate_cgka_session,
    
    // Journal session types
    LedgerEmpty, EventWriting, EventValidating, EventApplying, EventsApplied,
    LedgerCompacting, LedgerOperationFailed, SessionCreating, SessionActive,
    SessionCompleting, SessionCompleted, SessionTerminated, OperationUnlocked,
    LockRequesting, JournalOperationLocked, LockReleasing, LockFailed,
    SessionTypedJournal, JournalProtocolCore, JournalSessionError, JournalSessionState,
    new_session_typed_journal, rehydrate_journal_session,
    
    // CLI session types
    CliUninitialized, CliInitializing, CliAccountLoaded, CliDkdInProgress,
    CliRecoveryInProgress, CliNetworkOperationInProgress, CliStorageOperationInProgress,
    CliCommandFailed, SessionTypedCli, CliProtocolCore, CliSessionError, CliSessionState,
    new_session_typed_cli, rehydrate_cli_session,
    
    // Context session types
    ContextInitialized, ExecutingInstructions, AwaitingCondition, WritingToLedger,
    ExecutingSubProtocol, ExecutionComplete, ExecutionFailed,
    SessionTypedContext, ContextSessionError, ContextSessionState,
    new_session_typed_context, rehydrate_context_session,
};

// Typed channels for local runtime communication
pub use channels::{
    typed_channel, ChannelRegistry, ProtocolChannels, ResponseChannel,
    SessionCommand, SessionEvent, SessionEffect, SessionProtocolType,
    SessionProtocolResult, ProtocolStatus, TypedSender, TypedReceiver, ChannelError,
};

// Local session runtime
pub use local_runtime::{LocalSessionRuntime, ActiveSession, SessionStatus, RuntimeError};

// Utilities
pub use utils::{compute_lottery_ticket, determine_lock_winner, EventSigner, EventWatcher};

// ========== Legacy Types ==========
pub mod types;

pub use types::*;
