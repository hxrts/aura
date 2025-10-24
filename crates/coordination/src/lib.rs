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
pub mod choreography;
pub mod execution;
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

// Utilities
pub use utils::{compute_lottery_ticket, determine_lock_winner, EventSigner, EventWatcher};

// ========== Legacy Types ==========
pub mod types;

pub use types::*;
