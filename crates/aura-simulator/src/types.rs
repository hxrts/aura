//! Shared Types and Common Interfaces
//!
//! This module provides all shared data structures and types used across
//! the simulation crate. By centralizing these types, we eliminate circular
//! dependencies and create a clear single source of truth.

// Re-export all shared types to eliminate circular dependencies
pub use crate::results::{
    ErrorDetails, ExecutionStatus, PerformanceMetrics, PropertyCheckResult,
    PropertyEvaluationResult, PropertyViolation, PropertyViolationType, SimulationExecutionResult,
    SimulationRunResult, SimulationStateSnapshot, StopReason, ViolationDetails,
    ViolationDetectionReport, ViolationSeverity,
};

pub use crate::metrics::{
    MetricCategory, MetricValue, MetricsCollector, MetricsProvider, MetricsSnapshot,
    MetricsSummary, PerformanceCounter, SimulationMetrics,
};

pub use crate::config::{
    ByzantineConfig, ConfigBuilder, ConfigValidation, NetworkConfig, PerformanceConfig,
    PropertyMonitoringConfig, ScenarioConfig, SimulationConfig, SimulationCoreConfig,
};

pub use crate::state::{
    CheckpointId, CheckpointManager, SnapshotId, StateError, StateManager, StateSnapshot,
    UnifiedSnapshot, UnifiedStateManager,
};

// Simulation-specific types
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Simulation state snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationState {
    /// Current simulation tick
    pub tick: u64,
    /// Current simulation time
    pub time: u64,
    /// State variables
    pub variables: HashMap<String, String>,
    /// Participant states
    pub participants: Vec<ParticipantStateSnapshot>,
    /// Protocol execution state
    pub protocol_state: ProtocolExecutionState,
    /// Network state
    pub network_state: NetworkStateSnapshot,
}

/// Participant state snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantStateSnapshot {
    /// Participant ID
    pub id: String,
    /// Participant status
    pub status: String,
    /// Message count
    pub message_count: u64,
    /// Active sessions
    pub active_sessions: Vec<String>,
}

/// Protocol execution state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolExecutionState {
    /// Active protocol sessions
    pub active_sessions: Vec<SessionInfo>,
    /// Completed protocol sessions
    pub completed_sessions: Vec<SessionInfo>,
    /// Queued protocols
    pub queued_protocols: Vec<String>,
}

/// Session information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    /// Session ID
    pub session_id: String,
    /// Protocol type
    pub protocol_type: String,
    /// Current phase
    pub current_phase: String,
    /// Participants
    pub participants: Vec<String>,
    /// Session status
    pub status: String,
}

/// Network state snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkStateSnapshot {
    /// Network partitions
    pub partitions: Vec<String>,
    /// Message delivery statistics
    pub message_stats: MessageDeliveryStats,
    /// Network failure conditions
    pub failure_conditions: NetworkFailureConditions,
}

/// Message delivery statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageDeliveryStats {
    /// Total messages sent
    pub messages_sent: u64,
    /// Total messages delivered
    pub messages_delivered: u64,
    /// Total messages dropped
    pub messages_dropped: u64,
    /// Average delivery latency
    pub average_latency_ms: f64,
}

/// Network failure conditions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkFailureConditions {
    /// Drop rate
    pub drop_rate: f64,
    /// Latency range in milliseconds
    pub latency_range_ms: (u64, u64),
    /// Partitions active flag
    pub partitions_active: bool,
}

// Trace-related types
/// Execution trace for debugging
#[derive(Debug, Clone)]
pub struct ExecutionTrace {
    /// Sequence of simulation states
    pub states: std::collections::VecDeque<SimulationState>,
    /// Maximum trace length
    pub max_length: usize,
    /// Current position in trace
    pub current_position: usize,
}

impl ExecutionTrace {
    /// Create a new execution trace
    pub fn new(max_length: usize) -> Self {
        Self {
            states: std::collections::VecDeque::new(),
            max_length,
            current_position: 0,
        }
    }

    /// Add a state to the trace
    pub fn add_state(&mut self, state: SimulationState) {
        self.states.push_back(state);
        if self.states.len() > self.max_length {
            self.states.pop_front();
        }
        self.current_position = self.states.len().saturating_sub(1);
    }

    /// Get the current state
    pub fn current_state(&self) -> Option<&SimulationState> {
        self.states.back()
    }

    /// Get the trace length
    pub fn len(&self) -> usize {
        self.states.len()
    }

    /// Check if trace is empty
    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }

    /// Get all states in the trace
    pub fn get_all_states(&self) -> &std::collections::VecDeque<SimulationState> {
        &self.states
    }
}

/// Common result type aliases for consistency
/// Error type used throughout the simulation crate
pub type SimulationError = crate::AuraError;
/// Result type alias using SimulationError
pub type Result<T> = std::result::Result<T, SimulationError>;

// Export utility types for common use
pub use crate::utils::{
    current_unix_timestamp_millis, current_unix_timestamp_secs, generate_checkpoint_id,
    generate_random_uuid, generate_session_id, map_to_config_error, map_to_time_error,
    validate_positive, validate_range_inclusive, ResultExt, ValidationResult,
};
