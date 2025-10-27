#![allow(missing_docs, dead_code)]
#![allow(clippy::disallowed_methods)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::result_large_err)]
#![allow(clippy::large_enum_variant)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::vec_init_then_push)]
#![allow(clippy::single_match_else)]
#![allow(clippy::unnecessary_cast)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::ptr_arg)]
#![allow(clippy::format_in_format_args)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::default_constructed_unit_structs)]
#![allow(clippy::only_used_in_recursion)]
#![allow(clippy::empty_line_after_doc_comments)]
#![allow(clippy::manual_async_fn)]

//! Aura Simulation Engine - Unified Test Execution Framework
//!
//! A comprehensive simulation engine that serves as the canonical entry point for all tests,
//! from simple protocol checks to complex multi-phase chaos scenarios. The engine supports
//! both imperative (programmatic) and declarative (TOML-based) test definitions.
//!
//! # Unified Test Execution Model
//!
//! The simulation engine provides three complementary approaches:
//!
//! 1. **Functional Architecture**: Pure state transitions with `tick()` function
//! 2. **Observability Tools**: Passive observers for analysis and time-travel debugging
//! 3. **Unified Scenario Engine**: Declarative TOML scenarios with choreography actions
//!
//! # Key Features
//!
//! - **Deterministic Testing**: Same seed → same execution path
//! - **Byzantine Testing**: Inject faults via declarative actions
//! - **Network Simulation**: Control latency, partitions, and message delivery
//! - **Time Travel Debugging**: Replay from checkpoints with precise control
//! - **Choreographic Actions**: Convert imperative helpers to declarative TOML
//! - **Property Checking**: Extensible validation framework
//! - **Passive Observation**: Zero-coupling debugging tools
//!
//! # Architecture
//!
//! ## Functional Core (State + Logic + Execution)
//! - **WorldState**: Pure data container (no methods, just state)
//! - **tick()**: Pure function for state transitions (logic only)
//! - **FunctionalRunner**: Execution harness (control only)
//!
//! ## Decoupled Debugging Tools
//! - **PassiveTraceRecorder**: External event observer
//! - **CheckpointManager**: Standalone state serialization
//! - **TimeTravelDebugger**: Independent replay tool
//! - **ScenarioEngine**: High-level coordination framework
//!
//! ## Unified Test Execution
//! - **UnifiedScenarioEngine**: Canonical entry point for all tests
//! - **ChoreographyActions**: Declarative protocol execution
//! - **TOML Scenarios**: Unified scenario definition format
//! - **Property Framework**: Extensible validation system
//!
//! # Example: Declarative TOML Scenario
//!
//! ```toml
//! [metadata]
//! name = "dkd_with_byzantine_test"
//! description = "DKD protocol with Byzantine participant and network partition"
//!
//! [setup]
//! participants = 3
//! threshold = 2
//! seed = 42
//!
//! [[phases]]
//! name = "dkd_execution"
//! description = "Run DKD protocol"
//!
//!   [[phases.actions]]
//!   type = "run_choreography"
//!   choreography = "dkd"
//!   threshold = 2
//!   app_id = "test_app"
//!   context = "user_auth"
//!
//!   [[phases.actions]]
//!   type = "inject_byzantine"
//!   participant = "alice"
//!   behavior = "drop_messages"
//!
//!   [[phases.actions]]
//!   type = "apply_network_condition"
//!   condition = "partition"
//!   participants = ["bob", "charlie"]
//!   duration_ticks = 10
//!
//! expected_outcome = "success"
//! ```
//!
//! # Example: Programmatic Usage
//!
//! ```ignore
//! use aura_simulator::{
//!     UnifiedScenarioEngine, UnifiedScenarioLoader,
//!     choreography_actions::register_standard_choreographies
//! };
//!
//! // Create unified engine
//! let mut engine = UnifiedScenarioEngine::new("./test_artifacts")?;
//! register_standard_choreographies(&mut engine);
//!
//! // Load declarative scenarios
//! let mut loader = UnifiedScenarioLoader::new("./scenarios");
//! let scenario = loader.load_scenario("dkd_basic.toml")?;
//!
//! // Execute with debugging and analysis
//! let result = engine.execute_scenario(&scenario)?;
//!
//! assert!(result.success);
//! assert_eq!(result.phase_results.len(), 2);
//! ```

// Core simulation modules
pub mod logging;
pub mod simulation_engine;
pub mod world_state;

// Organized modules
pub mod analysis;
pub mod observability;
pub mod scenario;
pub mod testing;

// Legacy modules (temporarily disabled due to dependencies)
// pub mod adversary;  // Temporarily disabled due to journal dependency issues
// pub mod builder;  // Temporarily disabled due to journal dependency issues
// pub mod engine;  // Temporarily disabled due to transport dependency issues
// pub mod network;  // Temporarily disabled due to journal dependency issues
// pub mod quint;  // Temporarily disabled due to journal dependency issues

// Core simulation exports
pub use simulation_engine::*;
pub use world_state::*;

// Module exports (selective to avoid naming conflicts)
pub use analysis::*;
#[allow(ambiguous_glob_reexports)]
pub use observability::*;
#[allow(ambiguous_glob_reexports)]
pub use scenario::*;
pub use testing::*;

use thiserror::Error;

/// Simulation framework error types
///
/// Comprehensive error handling for the deterministic simulation framework
/// covering participant management, network simulation, and effect processing.
#[derive(Error, Debug, Clone)]
pub enum SimError {
    /// Requested participant not found in simulation
    #[error("Participant not found: {0}")]
    ParticipantNotFound(String),

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

    /// Checkpoint operation error
    #[error("Checkpoint error: {0}")]
    CheckpointError(String),

    /// Scenario generation or processing error
    #[error("Generation error: {0}")]
    GenerationError(String),

    /// Metadata management error
    #[error("Metadata error: {0}")]
    MetadataError(String),

    /// Property monitoring or analysis error
    #[error("Property error: {0}")]
    PropertyError(String),

    /// Failure analysis error
    #[error("Analysis error: {0}")]
    AnalysisError(String),
}

/// Result type alias for simulation operations
///
/// Provides a convenient Result<T> that defaults to SimError for error cases.
pub type Result<T> = std::result::Result<T, SimError>;
