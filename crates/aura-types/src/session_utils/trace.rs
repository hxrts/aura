//! Trace and execution types

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Unique identifier for a trace
pub type TraceId = Uuid;

/// Unique identifier for an execution step
pub type StepId = u64;

/// Lamport timestamp for ordering events
pub type LamportTimestamp = u64;

/// A trace represents a sequence of execution steps
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trace {
    /// Unique trace identifier
    pub id: TraceId,
    /// Human-readable trace name
    pub name: String,
    /// Description of what this trace represents
    pub description: String,
    /// All execution steps in temporal order
    pub steps: Vec<ExecutionStep>,
    /// Metadata about the trace
    pub metadata: HashMap<String, String>,
    /// Timestamp when trace was created
    pub created_at: u64,
}

/// A single step in the execution trace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionStep {
    /// Unique step identifier
    pub id: StepId,
    /// Lamport timestamp for ordering
    pub timestamp: LamportTimestamp,
    /// The actor (node/device) that performed this step
    pub actor: String,
    /// The action that was performed
    pub action: String,
    /// Input data for the action
    pub inputs: HashMap<String, serde_json::Value>,
    /// Output data from the action
    pub outputs: HashMap<String, serde_json::Value>,
    /// State snapshot before this step
    pub pre_state: Option<StateSnapshot>,
    /// State snapshot after this step
    pub post_state: Option<StateSnapshot>,
    /// Any errors that occurred during this step
    pub errors: Vec<String>,
}

/// Snapshot of system state at a particular point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSnapshot {
    /// Timestamp of this snapshot
    pub timestamp: LamportTimestamp,
    /// State variables and their values
    pub variables: HashMap<String, serde_json::Value>,
    /// Checksums or hashes for verification
    pub checksums: HashMap<String, String>,
}

/// Summary statistics about a trace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceSummary {
    /// Trace identifier
    pub trace_id: TraceId,
    /// Total number of steps
    pub step_count: usize,
    /// Number of unique actors
    pub actor_count: usize,
    /// Duration of the trace (logical time)
    pub duration: LamportTimestamp,
    /// Number of errors encountered
    pub error_count: usize,
    /// Property evaluation summary
    pub property_summary: Option<crate::properties::PropertyEvaluationSet>,
}

/// Query parameters for trace filtering
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceQuery {
    /// Filter by actor name
    pub actor: Option<String>,
    /// Filter by action name
    pub action: Option<String>,
    /// Filter by timestamp range
    pub timestamp_range: Option<(LamportTimestamp, LamportTimestamp)>,
    /// Filter by metadata key-value pairs
    pub metadata_filters: HashMap<String, String>,
    /// Maximum number of results
    pub limit: Option<usize>,
}

impl Trace {
    /// Create a new empty trace
    #[allow(clippy::disallowed_methods)]
    pub fn new(name: String, description: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            description,
            steps: Vec::new(),
            metadata: HashMap::new(),
            created_at: 0, // Should be set by the caller
        }
    }

    /// Add a step to the trace
    pub fn add_step(&mut self, step: ExecutionStep) {
        self.steps.push(step);
    }

    /// Get steps by actor
    pub fn get_steps_by_actor(&self, actor: &str) -> Vec<&ExecutionStep> {
        self.steps
            .iter()
            .filter(|step| step.actor == actor)
            .collect()
    }

    /// Get steps by action
    pub fn get_steps_by_action(&self, action: &str) -> Vec<&ExecutionStep> {
        self.steps
            .iter()
            .filter(|step| step.action == action)
            .collect()
    }

    /// Generate a summary of this trace
    pub fn summarize(&self) -> TraceSummary {
        let unique_actors: std::collections::HashSet<_> =
            self.steps.iter().map(|step| &step.actor).collect();

        let error_count = self.steps.iter().map(|step| step.errors.len()).sum();

        let duration = self
            .steps
            .iter()
            .map(|step| step.timestamp)
            .max()
            .unwrap_or(0);

        TraceSummary {
            trace_id: self.id,
            step_count: self.steps.len(),
            actor_count: unique_actors.len(),
            duration,
            error_count,
            property_summary: None, // Can be populated separately
        }
    }
}
