//! Aura Simulation Engine
//!
//! A deterministic, in-process simulation harness for testing distributed protocols.
//!
//! This crate implements a simulation engine that can run production `DeviceAgent` code
//! in a controlled, deterministic environment. By injecting interfaces for time, randomness,
//! and side effects, the simulation engine enables:
//!
//! - **Deterministic Testing**: Same seed → same execution path
//! - **Byzantine Testing**: Inject faults via effect interception
//! - **Network Simulation**: Control latency, partitions, and message delivery
//! - **Time Travel**: Fast-forward through protocol execution
//!
//! # Architecture
//!
//! The simulation engine consists of several layers:
//!
//! 1. **Simulation**: Top-level harness that owns the simulated world
//! 2. **SimulatedParticipant**: Wrapper around `DeviceAgent` with injected effects
//! 3. **SideEffectRuntime**: Central hub that processes effects from all participants
//! 4. **SimulatedNetwork**: Network fabric that simulates latency and partitions
//! 5. **EffectInterceptor**: Hooks for Byzantine testing and fault injection
//!
//! # Example
//!
//! ```ignore
//! use aura_simulator::{Simulation, ParticipantId};
//!
//! let mut sim = Simulation::new(42); // Seed for determinism
//!
//! let alice = sim.add_participant("alice").await;
//! let bob = sim.add_participant("bob").await;
//! let carol = sim.add_participant("carol").await;
//!
//! // Initiate a protocol
//! sim.tell(alice, Action::InitiateDkd { participants: vec![alice, bob, carol] }).await;
//!
//! // Run until quiescent
//! sim.run_until_idle().await;
//!
//! // Assert final state
//! let alice_ledger = sim.ledger_snapshot(alice);
//! assert!(alice_ledger.dkd_session_completed());
//! ```

pub mod builder;
pub mod engine;
pub mod network;
pub mod runners;

pub use builder::*;
pub use engine::*;
pub use network::*;
pub use runners::*;

use thiserror::Error;

/// Simulation framework error types
///
/// Comprehensive error handling for the deterministic simulation framework
/// covering participant management, network simulation, and effect processing.
#[derive(Error, Debug, Clone)]
pub enum SimError {
    /// Requested participant not found in simulation
    #[error("Participant not found: {0}")]
    ParticipantNotFound(ParticipantId),

    /// Error in participant agent operation
    #[error("Agent error: {0}")]
    AgentError(String),

    /// Network simulation or transport error
    #[error("Network error: {0}")]
    NetworkError(String),

    /// General simulation runtime error
    #[error("Runtime error: {0}")]
    RuntimeError(String),

    /// Error processing simulation effects
    #[error("Effect processing error: {0}")]
    EffectError(String),

    /// Time simulation or scheduling error
    #[error("Time error: {0}")]
    TimeError(String),
}

/// Result type alias for simulation operations
///
/// Provides a convenient Result<T> that defaults to SimError for error cases.
pub type Result<T> = std::result::Result<T, SimError>;
