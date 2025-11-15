//! Testing Effects
//!
//! Provides testing and debugging capabilities for protocol verification and
//! property-based testing. These effects enable controlled execution environments,
//! state inspection, and property validation during testing.

use crate::AuraError;
use async_trait::async_trait;
use std::any::Any;
use std::collections::HashMap;

/// Testing operations for controlled execution and verification
///
/// This trait provides pure testing primitives that can be composed
/// to create comprehensive testing scenarios. All operations are
/// stateless and work through explicit dependency injection.
#[async_trait]
pub trait TestingEffects {
    /// Create a checkpoint of the current system state
    ///
    /// Captures a snapshot of system state that can be restored later
    /// for time-travel debugging and reproducible testing.
    ///
    /// # Arguments
    /// * `checkpoint_id` - Unique identifier for this checkpoint
    /// * `label` - Human-readable description of this checkpoint
    ///
    /// # Returns
    /// Success indication or error if checkpoint failed
    async fn create_checkpoint(
        &self,
        checkpoint_id: &str,
        label: &str,
    ) -> Result<(), TestingError>;

    /// Restore system state from a previous checkpoint
    ///
    /// Resets system state to match a previously created checkpoint,
    /// enabling time-travel debugging and scenario replay.
    ///
    /// # Arguments
    /// * `checkpoint_id` - The checkpoint to restore from
    ///
    /// # Returns
    /// Success indication or error if restoration failed
    async fn restore_checkpoint(
        &self,
        checkpoint_id: &str,
    ) -> Result<(), TestingError>;

    /// Inspect arbitrary system state for debugging
    ///
    /// Provides access to internal system state without modifying it,
    /// enabling detailed analysis during testing and debugging.
    ///
    /// # Arguments
    /// * `component` - The system component to inspect
    /// * `path` - Path to the specific state to examine
    ///
    /// # Returns
    /// The requested state data or error if not accessible
    async fn inspect_state(
        &self,
        component: &str,
        path: &str,
    ) -> Result<Box<dyn Any + Send>, TestingError>;

    /// Assert that a property holds true
    ///
    /// Verifies invariants and properties during testing, providing
    /// detailed error information when assertions fail.
    ///
    /// # Arguments
    /// * `property_id` - Unique identifier for this property
    /// * `condition` - The condition that must be true
    /// * `description` - Human-readable description of the property
    ///
    /// # Returns
    /// Success if property holds, or detailed error if it fails
    async fn assert_property(
        &self,
        property_id: &str,
        condition: bool,
        description: &str,
    ) -> Result<(), TestingError>;

    /// Record an event for later analysis
    ///
    /// Captures events during execution for post-hoc analysis,
    /// performance measurement, and debugging.
    ///
    /// # Arguments
    /// * `event_type` - Category of event being recorded
    /// * `event_data` - Structured data about the event
    ///
    /// # Returns
    /// Success indication or error if recording failed
    async fn record_event(
        &self,
        event_type: &str,
        event_data: HashMap<String, String>,
    ) -> Result<(), TestingError>;

    /// Measure performance metrics
    ///
    /// Captures timing, throughput, and resource usage metrics
    /// for performance testing and optimization.
    ///
    /// # Arguments
    /// * `metric_name` - Name of the metric being measured
    /// * `value` - The measured value
    /// * `unit` - Unit of measurement (e.g., "ms", "bytes", "ops/sec")
    ///
    /// # Returns
    /// Success indication or error if measurement failed
    async fn record_metric(
        &self,
        metric_name: &str,
        value: f64,
        unit: &str,
    ) -> Result<(), TestingError>;
}

/// Errors that can occur during testing operations
#[derive(Debug, thiserror::Error)]
pub enum TestingError {
    /// Checkpoint operation failed
    #[error("Checkpoint '{checkpoint_id}' operation failed: {reason}")]
    CheckpointError {
        checkpoint_id: String,
        reason: String,
    },

    /// State inspection failed
    #[error("Cannot inspect state at '{component}::{path}': {reason}")]
    StateInspectionError {
        component: String,
        path: String,
        reason: String,
    },

    /// Property assertion failed
    #[error("Property '{property_id}' failed: {description}")]
    PropertyAssertionFailed {
        property_id: String,
        description: String,
    },

    /// Event recording failed
    #[error("Failed to record event '{event_type}': {reason}")]
    EventRecordingError {
        event_type: String,
        reason: String,
    },

    /// Metric recording failed
    #[error("Failed to record metric '{metric_name}': {reason}")]
    MetricRecordingError {
        metric_name: String,
        reason: String,
    },

    /// System error during testing
    #[error("Testing system error: {0}")]
    SystemError(#[from] AuraError),
}