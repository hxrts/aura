//! Session Types for Aura's Choreographic Programming Model
//!
//! This crate provides compile-time safe session types for Aura's distributed protocols.
//! Session types ensure that protocol state transitions follow correct choreographic patterns
//! and provide compile-time guarantees about protocol safety.
//!
//! # Architecture
//!
//! **Core Traits:**
//! - [`SessionProtocol`]: Base trait for all session-typed protocols
//! - [`WitnessedTransition`]: Compile-time safe state transitions
//! - [`RuntimeWitness`]: Runtime validation of distributed protocol invariants
//!
//! **Protocol State Machines:**
//! - [`protocols::dkd`]: Deterministic Key Derivation protocol states
//! - [`protocols::recovery`]: Guardian-based recovery protocol states
//! - [`protocols::transport`]: Transport connection lifecycle states
//! - [`protocols::agent`]: Agent operation lifecycle states
//! - [`protocols::frost`]: FROST threshold signature states
//! - [`protocols::cgka`]: Continuous Group Key Agreement states
//! - [`protocols::journal`]: Journal/ledger operation states
//! - [`protocols::cli`]: CLI command execution states
//!
//! **Key Features:**
//! - **Zero-cost abstractions**: Session types compile to zero runtime overhead
//! - **Crash safety**: Protocol state can be rehydrated from journal evidence
//! - **Deadlock freedom**: Session types prevent impossible state transitions
//! - **Deterministic testing**: All effects are injectable for testing
//!
//! # Example Usage
//!
//! ```rust,ignore
//! use aura_session_types::protocols::dkd::*;
//! 
//! // Create new DKD protocol in initial state
//! let dkd = new_dkd_protocol(device_id, app_id, context)?;
//! assert_eq!(dkd.state_name(), "InitializationPhase");
//! 
//! // Type-safe state transitions
//! let dkd = dkd.transition_to::<CommitmentPhase>();
//! assert_eq!(dkd.state_name(), "CommitmentPhase");
//! 
//! // Compile-time prevention of invalid transitions
//! // let invalid = dkd.transition_to::<FinalizationPhase>(); // Won't compile!
//! ```

#![allow(missing_docs)] // TODO: Add comprehensive documentation

pub mod core;
pub mod protocols;
pub mod witnesses;
pub mod rehydration;

// Re-export core types
pub use core::{
    SessionProtocol, SessionState, WitnessedTransition, ProtocolRehydration,
    StateAnalysis, ChoreographicProtocol, SessionError,
};

// Re-export witnesses
pub use witnesses::{
    RuntimeWitness, RehydrationEvidence, 
    CollectedCommitments, VerifiedReveals, ApprovalThresholdMet, SharesCollected,
    ThresholdEventsMet, LedgerWriteComplete, SubProtocolComplete,
    HandshakeCompleted, TicketsValidated, MessageDelivered, BroadcastCompleted,
    CommitmentThresholdMet, SignatureShareThresholdMet, SignatureAggregated,
    EventsValidated, EventsAppliedSuccessfully, SessionCreated, SessionCompletionReady,
    KeyGenerationCompleted, FrostResharingCompleted, FrostProtocolFailure,
    CgkaGroupInitiated, MembershipChangeReady, EpochTransitionReady, GroupStabilized,
    OperationValidated, OperationAppliedSuccessfully, TreeUpdatesCompleted,
    AccountInitialized, AccountConfigLoaded, CommandCompleted,
};

// Re-export available protocol state machines
pub use protocols::{
    // DKD protocol states
    dkd::{
        DkdProtocolState, new_dkd_protocol, rehydrate_dkd_protocol,
        InitializationPhase, CommitmentPhase, RevealPhase, FinalizationPhase, 
        CompletionPhase, Failure,
        DkdCompleted, DkdProtocolCore, DkdSessionError,
    },
    
    // Agent protocol states
    agent::{
        AgentIdle, DkdInProgress, RecoveryInProgress, ResharingInProgress, 
        LockingInProgress, AgentOperationLocked, AgentFailed,
        SessionTypedAgent, DeviceAgentCore, AgentSessionError, AgentSessionState,
        new_session_typed_agent, rehydrate_agent_session,
    },
    
    // Recovery protocol states
    recovery::{
        RecoveryInitialized, CollectingApprovals, EnforcingCooldown, CollectingShares,
        ReconstructingKey, RecoveryProtocolCompleted, RecoveryAborted,
        SessionTypedRecovery, RecoveryProtocolCore, RecoverySessionError, RecoverySessionState,
        new_session_typed_recovery, rehydrate_recovery_session,
    },
    
    // Transport protocol states
    transport::{
        TransportDisconnected, ConnectionHandshaking, TicketValidating, TransportConnected,
        MessageSending, Broadcasting, ConnectionFailed, AwaitingMessage, ProcessingMessage,
        RequestResponseActive, SessionTypedTransport, TransportProtocolCore,
        TransportSessionError, TransportSessionState,
        new_session_typed_transport, rehydrate_transport_session,
    },
    
    // FROST protocol states
    frost::{
        FrostIdle, FrostCommitmentPhase, FrostAwaitingCommitments, FrostSigningPhase,
        FrostAwaitingShares, FrostReadyToAggregate, FrostSignatureComplete, FrostSigningFailed,
        SessionTypedFrost, FrostProtocolCore, FrostSessionError, FrostSessionState,
        new_session_typed_frost, rehydrate_frost_session,
    },
    
    // CGKA protocol states
    cgka::{
        CgkaGroupInitialized, GroupMembershipChange, EpochTransition, GroupStable,
        GroupOperationFailed, OperationPending, OperationValidating, OperationApplying,
        OperationApplied, OperationFailed, TreeBuilding, TreeUpdating, TreeComplete, TreeFailed,
        SessionTypedCgka, CgkaProtocolCore, CgkaSessionError, CgkaSessionState,
        new_session_typed_cgka, rehydrate_cgka_session,
    },
    
    // Journal protocol states
    journal::{
        LedgerEmpty, EventWriting, EventValidating, EventApplying, EventsApplied,
        LedgerCompacting, LedgerOperationFailed, SessionCreating, SessionActive,
        SessionCompleting, SessionCompleted, SessionTerminated, OperationUnlocked,
        LockRequesting, JournalOperationLocked, LockReleasing, LockFailed,
        SessionTypedJournal, JournalProtocolCore, JournalSessionError, JournalSessionState,
        new_session_typed_journal, rehydrate_journal_session,
    },
    
    // CLI protocol states
    cli::{
        CliUninitialized, CliInitializing, CliAccountLoaded, CliDkdInProgress,
        CliRecoveryInProgress, CliNetworkOperationInProgress, CliStorageOperationInProgress,
        CliCommandFailed, SessionTypedCli, CliProtocolCore, CliSessionError, CliSessionState,
        new_session_typed_cli, rehydrate_cli_session,
    },
    
    // Context protocol states
    context::{
        ContextInitialized, ExecutingInstructions, AwaitingCondition, WritingToLedger,
        ExecutingSubProtocol, ExecutionComplete, ExecutionFailed,
        SessionTypedContext, ContextSessionError, ContextSessionState,
        new_session_typed_context, rehydrate_context_session,
    },
};

// Re-export rehydration utilities
pub use rehydration::{
    RehydrationManager, ProtocolEvidence, StateRecovery, CrashRecoveryError,
};