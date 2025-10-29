// Note: This crate contains experimental and test code with intentional patterns
// that may trigger certain clippy warnings. Specific allows are applied at the
// item level where necessary for testing utilities and simulation code.

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

// ================================================================
// UNIFIED ARCHITECTURE (Phases 0-5 Complete)
// ================================================================

// Shared types and interfaces (eliminates circular dependencies)
pub mod types;

// Core simulation engine (pure functional logic)
pub mod simulation_engine;
pub mod world_state;

// Unified framework modules (infrastructure)
pub mod config;
pub mod metrics;
pub mod results;
pub mod state;
pub mod utils;

// Analysis and debugging tools (external observers)
pub mod analysis;
pub mod observability;

// Scenario execution and testing
pub mod scenario;
pub mod testing;

// Legacy support
pub mod logging;

// Quint integration
pub mod quint;

// ================================================================
// CLEAN ARCHITECTURE NOTES
// ================================================================
//
// Legacy modules have been removed as part of the deduplication effort.
// The following modules were identified as either:
// - Superseded by unified framework modules
// - Creating circular dependencies
// - Having unresolved external dependencies
//
// All functionality has been consolidated into the unified architecture.

// ================================================================
// CLEAN PUBLIC API (Explicit Exports - No Ambiguous Globs)
// ================================================================

// Core simulation functionality
pub use simulation_engine::tick;
pub use world_state::{
    ByzantineStrategy, NetworkPartition, ParticipantState, QueuedProtocol, WorldState,
    WorldStateSnapshot,
};

// Unified framework (primary API)
pub use types::{
    current_unix_timestamp_millis,
    // Common utilities
    current_unix_timestamp_secs,
    generate_checkpoint_id,
    generate_random_uuid,
    CheckpointManager,

    ConfigBuilder,
    ConfigValidation,

    ExecutionTrace,

    MetricCategory,

    MetricsCollector,
    MetricsProvider,
    PropertyCheckResult,
    // Property types (moved from testing to eliminate circular deps)
    PropertyViolation,
    PropertyViolationType,
    ResultExt,
    // Configuration system
    SimulationConfig,
    // Core results and metrics
    SimulationExecutionResult,
    SimulationMetrics,
    SimulationRunResult,
    SimulationState,
    // State management
    StateManager,
    UnifiedStateManager,
    ValidationResult,
    ViolationDetails,
    ViolationDetectionReport,
    ViolationSeverity,
};

// Analysis and debugging tools
pub use analysis::{FailureAnalysisResult, FailureAnalyzer, FocusedTester, MinimalReproduction};

// Observability tools
pub use observability::{PassiveTraceRecorder, TimeTravelDebugger};

// Testing framework
pub use testing::{FunctionalRunner, PropertyMonitor};

// Scenario execution
// pub use scenario::{
//     Scenario, ScenarioEngine, ChoreographyAction, NetworkConditions
// };

// Error types and Result are provided by aura_types unified system
pub use aura_types::AuraError;

/// Result type alias for simulator operations
pub type Result<T> = std::result::Result<T, AuraError>;
