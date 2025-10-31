//! Testing infrastructure and validation framework
//!
//! This module provides property-based testing, functional test runners,
//! and test utilities for simulation validation.

pub mod functional_runner;
pub mod property_monitor;
pub mod test_utils;

pub use functional_runner::*;
pub use property_monitor::*;
pub use test_utils::*;

// Re-export unified types from results module
pub use crate::results::{
    PropertyEvaluationResult, PropertyViolation, PropertyViolationType, ValidationResult,
    ViolationDetails, ViolationDetectionReport, ViolationSeverity,
};

// Re-export unified types from types module
pub use crate::types::{
    ExecutionTrace, MessageDeliveryStats, NetworkFailureConditions, NetworkStateSnapshot,
    ParticipantStateSnapshot, ProtocolExecutionState, SessionInfo, SimulationState,
};

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

// Missing types for property monitoring

/// Quint invariant property
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuintInvariant {
    /// Property name
    pub name: String,
    /// Property expression
    pub expression: String,
    /// Property description
    pub description: Option<String>,
}

/// Quint temporal property
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuintTemporalProperty {
    /// Property name
    pub name: String,
    /// Property expression
    pub expression: String,
    /// Temporal property type
    pub property_type: TemporalPropertyType,
    /// Property description
    pub description: Option<String>,
}

/// Quint safety property
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuintSafetyProperty {
    /// Property name
    pub name: String,
    /// Property expression
    pub expression: String,
    /// Property description
    pub description: Option<String>,
}

/// Types of temporal properties
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TemporalPropertyType {
    /// Eventually property (F)
    Eventually,
    /// Always property (G)
    Always,
    /// Until property (U)
    Until,
    /// Next property (X)
    Next,
}

/// Quint value types (keep for Quint integration)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QuintValue {
    /// Boolean value
    Bool(bool),
    /// Integer value
    Int(i64),
    /// String value
    String(String),
    /// Set value
    Set(Vec<QuintValue>),
    /// Record value
    Record(HashMap<String, QuintValue>),
}

/// Property priority for monitoring
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum PropertyPriority {
    Low,
    Medium,
    High,
    Critical,
}

/// Violation pattern detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViolationPattern {
    /// Pattern name
    pub name: String,
    /// Pattern description
    pub description: String,
    /// Pattern confidence level
    pub confidence: f64,
}

impl PartialEq for ViolationPattern {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.description == other.description
    }
}

impl Eq for ViolationPattern {}

impl std::hash::Hash for ViolationPattern {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.description.hash(state);
    }
}

// QuintEvaluationConfig removed - use unified PropertyMonitoringConfig from config module
// Simulation state types moved to unified types module - imported above

// ViolationDetectionReport moved to unified types - imported from lib.rs

// ViolationDetails and PropertyViolation removed - use unified types from results module

// Implementation blocks for missing types

/// Violation detection state implementation
#[derive(Debug, Clone)]
pub struct ViolationDetectionState {
    /// Detected violations
    pub violations: Vec<PropertyViolation>,
    /// Violation patterns detected
    pub detected_patterns: HashMap<String, ViolationPattern>,
    /// False positive tracking
    pub false_positives: Vec<PropertyViolation>,
    /// Violation history for pattern analysis
    pub violation_history: VecDeque<PropertyViolation>,
}

impl Default for ViolationDetectionState {
    fn default() -> Self {
        Self::new()
    }
}

impl ViolationDetectionState {
    pub fn new() -> Self {
        Self {
            violations: Vec::new(),
            detected_patterns: HashMap::new(),
            false_positives: Vec::new(),
            violation_history: VecDeque::new(),
        }
    }
}

// MonitoringStatistics removed - use unified SimulationMetrics from metrics module

// EfficiencyMetrics removed - use performance counters in unified metrics system

// CheckPerformanceMetrics removed - use PerformanceMetrics from unified results module

// PropertyCheckResult removed - use unified PropertyCheckResult from results module

/// Trace metadata for execution analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceMetadata {
    /// Trace unique identifier
    pub trace_id: String,
    /// Scenario name that generated this trace
    pub scenario_name: String,
    /// Trace start time
    pub start_time: u64,
    /// Trace end time
    pub end_time: Option<u64>,
    /// Properties monitored during this trace
    pub monitored_properties: Vec<String>,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}
