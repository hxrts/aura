//! Observability tools for simulation analysis
//!
//! This module provides passive observation and introspection tools for analyzing
//! simulations without affecting their execution. These tools act as external observers,
//! maintaining separation between core simulation logic and debugging infrastructure.
//!
//! # Architecture
//!
//! - **TraceRecorder**: Passive listener that records events without affecting simulation
//! - **CheckpointManager**: Standalone utility for saving/loading WorldState snapshots
//! - **TimeTravelDebugger**: Powerful standalone tool for debugging simulation failures
//! - **ObservabilityEngine**: Coordinates debugging tools as external observers
//!
//! # Benefits
//!
//! - Core simulation remains fast and simple
//! - Debugging overhead only when explicitly used
//! - Modular design for easy maintenance and extension
//! - No coupling between simulation and debugging logic

pub mod checkpoint_manager;
pub mod observability_engine;
pub mod passive_trace_recorder;
pub mod time_travel_debugger;

pub use checkpoint_manager::*;
pub use observability_engine::*;
pub use passive_trace_recorder::*;
pub use time_travel_debugger::*;
