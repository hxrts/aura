//! Core types for native Quint API

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Result of property verification using native Rust evaluator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// Whether verification succeeded.
    pub success: bool,
    /// Time taken for verification.
    pub duration: Duration,
    /// Results for individual properties.
    pub properties: HashMap<String, serde_json::Value>,
    /// Counterexample if verification failed.
    pub counterexample: Option<serde_json::Value>,
    /// Verification statistics.
    pub statistics: serde_json::Value,
}

/// Details about successful verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationDetails {
    /// Number of states explored.
    pub states_explored: Option<u64>,
    /// Maximum depth reached.
    pub max_depth: Option<u32>,
    /// Verification strategy used.
    pub strategy: Option<String>,
    /// Additional metadata.
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Counterexample showing property violation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CounterExample {
    /// Execution trace leading to violation.
    pub trace: ExecutionTrace,
    /// State where violation occurred.
    pub violation_state: StateSnapshot,
    /// Description of the violation.
    pub violation_description: String,
}

/// Execution trace in the verification model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionTrace {
    /// Sequence of steps in the trace.
    pub steps: Vec<TraceStep>,
    /// Total length of the trace.
    pub length: u64,
    /// Whether this is a complete trace.
    pub is_complete: bool,
}

/// Individual step in an execution trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceStep {
    /// Step number (0-indexed).
    pub step_number: u64,
    /// Action taken in this step.
    pub action: String,
    /// State before the action.
    pub pre_state: StateSnapshot,
    /// State after the action.
    pub post_state: StateSnapshot,
    /// Additional step metadata.
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Snapshot of system state at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSnapshot {
    /// Variable bindings in this state.
    pub variables: HashMap<String, serde_json::Value>,
    /// Additional state metadata.
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Configuration for Quint verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationConfig {
    /// Maximum number of steps to explore.
    pub max_steps: Option<u32>,
    /// Timeout in milliseconds.
    pub timeout_ms: Option<u64>,
    /// Random seed for reproducible verification.
    pub random_seed: Option<u32>,
    /// Verification strategy.
    pub strategy: VerificationStrategy,
    /// Additional options.
    pub options: HashMap<String, serde_json::Value>,
}

impl Default for VerificationConfig {
    fn default() -> Self {
        Self {
            max_steps: Some(1000),
            timeout_ms: Some(30_000), // 30 seconds
            random_seed: None,
            strategy: VerificationStrategy::Bfs,
            options: HashMap::new(),
        }
    }
}

/// Verification strategy for property checking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VerificationStrategy {
    /// Breadth-first search.
    Bfs,
    /// Depth-first search.
    Dfs,
    /// Random exploration.
    Random,
    /// Bounded model checking.
    Bmc,
    /// Custom strategy with parameters.
    Custom {
        /// Strategy name.
        name: String,
        /// Strategy parameters.
        params: HashMap<String, serde_json::Value>,
    },
}

/// Specification module information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleInfo {
    /// Module name.
    pub name: String,
    /// Module file path.
    pub file_path: String,
    /// Module dependencies.
    pub dependencies: Vec<String>,
    /// Exported definitions.
    pub exports: Vec<String>,
    /// Module metadata.
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Type information for Quint expressions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QuintType {
    /// Boolean type.
    Bool,
    /// Integer type.
    Int,
    /// String type.
    Str,
    /// Set type.
    Set(Box<QuintType>),
    /// Record type.
    Record(HashMap<String, QuintType>),
    /// Function type.
    Function {
        /// Parameter types.
        params: Vec<QuintType>,
        /// Return type.
        result: Box<QuintType>,
    },
    /// Union type.
    Union(Vec<QuintType>),
    /// Custom type.
    Custom {
        /// Type name.
        name: String,
        /// Type parameters.
        params: Vec<QuintType>,
    },
}

/// Status of the Quint bridge connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BridgeStatus {
    /// Bridge is disconnected.
    Disconnected,
    /// Bridge is connecting.
    Connecting,
    /// Bridge is connected and ready.
    Connected,
    /// Bridge encountered an error.
    Error {
        /// Error message.
        message: String,
    },
}

/// Statistics about bridge usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeStats {
    /// Total number of verification requests.
    pub total_verifications: u64,
    /// Number of successful verifications.
    pub successful_verifications: u64,
    /// Number of failed verifications.
    pub failed_verifications: u64,
    /// Average verification time (in milliseconds).
    pub avg_verification_time_ms: f64,
    /// Total time spent in verification (in milliseconds).
    pub total_verification_time_ms: u64,
}
