//! Shared types for Aura Dev Console
//!
//! This crate contains pure data structures shared between the browser UI
//! and backend services (simulation server, live node instrumentation).
//!
//! **Design Principles**:
//! - Zero dependencies on other Aura crates
//! - All types are serializable
//! - Minimal, stable API surface

pub mod commands;
pub mod console;
pub mod network;
pub mod responses;
pub mod trace;

pub use commands::{ConsoleCommand, ReplCommand};
pub use console::{
    BranchInfo, ClientMessage, ConsoleEvent, ConsoleResponse, DeviceInfo, LedgerStateInfo,
    ServerMessage, SimulationInfo,
};
pub use network::{NetworkEdge, NetworkTopology, NodeInfo, ParticipantInfo, PartitionInfo};
pub use responses::{Response, ResponseStatus};
pub use trace::{CausalityInfo, EventType, SimulationTrace, TraceEvent, TraceMetadata};
