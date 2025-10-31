//! Quint Formal Verification Integration
//!
//! This module provides integration between Quint formal specifications and the
//! Aura simulation framework. It enables property-based testing driven by formal
//! verification specifications.

pub mod types;
pub mod bridge;
pub mod byzantine_mapper;
pub mod properties;
pub mod chaos_generator;
pub mod trace_converter;
pub mod cli_runner;
pub mod ast_parser;
pub mod evaluator;

pub use types::*;
pub use bridge::{QuintBridge, QuintBridgeError};
pub use byzantine_mapper::{ByzantineMapper, ByzantineMappingResult, EnhancedByzantineStrategy};
pub use properties::{PropertyExtractor, PropertyMonitor, PropertyType, VerifiableProperty, PropertyPriority, PropertyError, PropertyExtractionConfig};
pub use chaos_generator::{ChaosGenerator, ChaosScenario, ChaosType, ChaosGeneratorError, ChaosGenerationConfig};
pub use trace_converter::{TraceConverter, QuintTrace, TraceFragment, TraceConversionResult, TraceConversionConfig};
pub use evaluator::{PropertyEvaluator, EvaluatorConfig, EvaluationMode, StateHistory, StateSnapshot, EvaluationStatistics, EvaluationError, WorldStateAdapter};