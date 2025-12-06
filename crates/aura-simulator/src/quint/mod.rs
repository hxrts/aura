//! Quint Formal Verification Integration
//!
//! This module provides integration between Quint formal specifications and the
//! Aura simulation framework. It enables property-based testing driven by formal
//! verification specifications.
//!
//! # Generative Simulation Infrastructure
//!
//! The module provides infrastructure for **generative simulations** where Quint
//! specifications drive actual Aura effect execution:
//!
//! - `action_registry`: Maps Quint action names to Aura effect handlers
//! - `state_mapper`: Bidirectional state conversion using `QuintMappable`
//!
//! # Architecture
//!
//! ```text
//! Quint Spec (.qnt) → ActionRegistry → Aura Effects → StateMapper → Property Eval
//! ```

pub mod action_registry;
pub mod ast_parser;
pub mod aura_state_extractors;
pub mod byzantine_mapper;
pub mod chaos_generator;
pub mod cli_runner;
pub mod domain_handlers;
pub mod generative_simulator;
pub mod itf_fuzzer;
pub mod itf_loader;
pub mod properties;
pub mod simulation_evaluator;
pub mod state_mapper;
pub mod trace_converter;
pub mod types;

pub use byzantine_mapper::{ByzantineMapper, ByzantineMappingResult, EnhancedByzantineStrategy};
pub use chaos_generator::{
    ChaosGenerationConfig, ChaosGenerator, ChaosGeneratorError, ChaosScenario, ChaosType,
};
pub use cli_runner::QuintCliRunner;
pub use itf_fuzzer::{
    CIIntegrationConfig,
    CIOutputFormat,
    ChoicePoint,
    CoverageSummary,
    Decision,
    // Phase 4: Campaign orchestration and CI/CD integration
    FuzzingCampaignConfig,
    FuzzingCampaignResult,
    GeneratedTestCase,
    // Phase 6: Generative simulation integration
    GenerativePropertyViolation,
    GenerativeSimulationResult,
    ITFBasedFuzzer,
    ITFFuzzConfig,
    ITFFuzzError,
    ITFMeta,
    ITFPropertyEvaluationResult,
    ITFState,
    ITFStateMeta,
    ITFTrace,
    IterativeDeepening,
    MBTExecutionResult,
    MBTMetadata,
    MemoryUsage,
    ModelCheckingReport,
    ModelCheckingResult,
    PerformanceMonitor,
    PerformanceReport,
    PhaseTimings,
    PropertyViolation,
    ResourceUtilization,
    SimulationConfig,
    SimulationResult,
    TestAction,
    TestCaseMetadata,
    TestCoverageAnalysis,
    TestGenerationMethod,
    TestSuite,
    ThroughputMetrics,
    ValidatedTestCase,
};
pub use properties::{
    PropertyError, PropertyExtractionConfig, PropertyExtractor, PropertyMonitor, PropertyPriority,
    PropertyType, VerifiableProperty,
};
pub use simulation_evaluator::{
    NetworkState, ParticipantState, SimulationEvaluatorConfig, SimulationPropertyEvaluator,
    SimulationWorldState,
};
pub use trace_converter::{
    ItfMetadata, ItfState, ItfTrace, ItfTraceConverter, QuintTrace, TraceConversionConfig,
    TraceConversionResult, TraceConverter, TraceFragment,
};
pub use types::*;

// Generative simulation infrastructure
pub use action_registry::{
    ActionBuilder, ActionHandler, ActionRegistry, ClosureActionHandler, FailHandler, LogHandler,
    NoOpHandler,
};
pub use domain_handlers::{
    capability_properties_initial_state, capability_properties_registry, AttenuateTokenHandler,
    CompleteTransportOpHandler, InitAuthorityHandler, InitContextHandler,
};
pub use state_mapper::{
    AuraStateMapper, SimulationStateMapper, StateDiff, StateMapper, StateSnapshot,
};

// Generative simulation
pub use generative_simulator::{
    GenerativeSimulator, GenerativeSimulatorConfig, SeededRandomProvider, SimulationStep,
};

// Aura-specific state extraction
pub use aura_state_extractors::{
    extract_authority_state, extract_budgets_state, extract_completed_ops, extract_flow_budget,
    extract_tokens_state, extract_tree_state, extract_tree_state_simple, CapabilityToken,
    QuintSimulationState, QuintStateExtractor, TransportOpRecord,
};

// ITF trace loading and replay
pub use itf_loader::{
    ITFLoader, ITFTraceBuilder, InferredAction, SimulationSequence, SimulationSequenceStep,
};
