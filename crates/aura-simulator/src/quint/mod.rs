//! Quint Formal Verification Integration
//!
//! This module provides integration between Quint formal specifications and the
//! Aura simulation framework. It enables property-based testing driven by formal
//! verification specifications.

pub mod ast_parser;
pub mod bridge;
pub mod byzantine_mapper;
pub mod chaos_generator;
pub mod cli_runner;
pub mod evaluator;
pub mod itf_fuzzer;
pub mod properties;
pub mod trace_converter;
pub mod types;

pub use bridge::{QuintBridge, QuintBridgeError};
pub use byzantine_mapper::{ByzantineMapper, ByzantineMappingResult, EnhancedByzantineStrategy};
pub use chaos_generator::{
    ChaosGenerationConfig, ChaosGenerator, ChaosGeneratorError, ChaosScenario, ChaosType,
};
pub use cli_runner::QuintCliRunner;
pub use evaluator::{
    EvaluationError, EvaluationMode, EvaluationStatistics, EvaluatorConfig, PropertyEvaluator,
    StateHistory, StateSnapshot, WorldStateAdapter,
};
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
    ITFBasedFuzzer,
    ITFFuzzConfig,
    ITFFuzzError,
    ITFMeta,
    ITFPropertyEvaluationResult,
    ITFState,
    ITFStateMeta,
    ITFTrace,
    IterativeDeepening,
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
};
pub use properties::{
    PropertyError, PropertyExtractionConfig, PropertyExtractor, PropertyMonitor, PropertyPriority,
    PropertyType, VerifiableProperty,
};
pub use trace_converter::{
    ItfMetadata, ItfState, ItfTrace, ItfTraceConverter, QuintTrace, TraceConversionConfig,
    TraceConversionResult, TraceConverter, TraceFragment,
};
pub use types::*;
