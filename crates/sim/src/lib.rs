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
//! use aura_sim::{Simulation, ParticipantId};
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

pub mod types;
pub mod network;
pub mod runtime;
pub mod participant;
pub mod simulation;
pub mod interceptor;
pub mod transport;
pub mod choreographic;
// pub mod choreographic_runner; // Temporarily disabled due to lifetime issues
pub mod protocol_executor;
pub mod tokio_integrated_executor;
pub mod tokio_choreographic;

#[cfg(test)]
mod protocol_tests;
// #[cfg(test)]
// mod dkg_choreographic_test;
#[cfg(test)]
mod debug_test;
#[cfg(test)]
mod simple_hang_test;
#[cfg(test)]
mod signature_debug;
#[cfg(test)]
mod debug_tokio_hang;

pub use types::*;
pub use network::*;
pub use runtime::*;
pub use participant::*;
pub use simulation::*;
pub use interceptor::*;
pub use transport::*;

use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum SimError {
    #[error("Participant not found: {0}")]
    ParticipantNotFound(ParticipantId),
    
    #[error("Agent error: {0}")]
    AgentError(String),
    
    #[error("Network error: {0}")]
    NetworkError(String),
    
    #[error("Runtime error: {0}")]
    RuntimeError(String),
    
    #[error("Effect processing error: {0}")]
    EffectError(String),
    
    #[error("Time error: {0}")]
    TimeError(String),
}

pub type Result<T> = std::result::Result<T, SimError>;

