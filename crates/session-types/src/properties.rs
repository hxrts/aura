//! Property types for formal verification and monitoring

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Unique identifier for a property
pub type PropertyId = Uuid;

/// Property severity level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PropertySeverity {
    /// Critical safety properties that must never be violated
    Critical,
    /// Important liveness properties
    Important,
    /// Performance or optimization properties
    Performance,
    /// Informational properties for debugging
    Info,
}

/// Property evaluation result
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PropertyResult {
    /// Property holds true
    Satisfied,
    /// Property is violated
    Violated {
        /// Description of the violation
        reason: String,
        /// Additional context about the violation
        context: HashMap<String, String>,
    },
    /// Property evaluation is pending
    Pending,
    /// Property cannot be evaluated (missing data, etc.)
    Unknown,
}

/// A formal property specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Property {
    /// Unique property identifier
    pub id: PropertyId,
    /// Human-readable property name
    pub name: String,
    /// Detailed description of the property
    pub description: String,
    /// Property severity level
    pub severity: PropertySeverity,
    /// Quint specification of the property
    pub quint_spec: String,
    /// Property category for organization
    pub category: String,
    /// Tags for filtering and grouping
    pub tags: Vec<String>,
    /// Whether this property is currently enabled for monitoring
    pub enabled: bool,
}

/// Result of property evaluation at a specific point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyEvaluation {
    /// Property that was evaluated
    pub property_id: PropertyId,
    /// Evaluation result
    pub result: PropertyResult,
    /// Timestamp of evaluation
    pub timestamp: u64,
    /// Additional metadata about the evaluation
    pub metadata: HashMap<String, String>,
}

/// Collection of property evaluations
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PropertyEvaluationSet {
    /// All property evaluations
    pub evaluations: Vec<PropertyEvaluation>,
    /// Timestamp when this set was created
    pub timestamp: u64,
    /// Total number of satisfied properties
    pub satisfied_count: usize,
    /// Total number of violated properties
    pub violated_count: usize,
}

impl PropertyEvaluationSet {
    /// Create a new empty evaluation set
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a property evaluation to the set
    pub fn add_evaluation(&mut self, evaluation: PropertyEvaluation) {
        match &evaluation.result {
            PropertyResult::Satisfied => self.satisfied_count += 1,
            PropertyResult::Violated { .. } => self.violated_count += 1,
            _ => {}
        }
        self.evaluations.push(evaluation);
    }

    /// Get all violations in this set
    pub fn get_violations(&self) -> Vec<&PropertyEvaluation> {
        self.evaluations
            .iter()
            .filter(|eval| matches!(eval.result, PropertyResult::Violated { .. }))
            .collect()
    }
}
