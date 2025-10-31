//! Checkpoint creation utilities

use crate::utils::{current_unix_timestamp_secs, generate_checkpoint_id};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Standard checkpoint metadata structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointMetadata {
    /// Unique checkpoint identifier
    pub id: String,
    /// User-provided label (optional)
    pub label: Option<String>,
    /// Tick when checkpoint was created
    pub tick: u64,
    /// Timestamp when checkpoint was created
    pub created_at: u64,
    /// Additional context data
    pub context: HashMap<String, String>,
    /// Checkpoint creation reason
    pub reason: CheckpointReason,
}

/// Reason why a checkpoint was created
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CheckpointReason {
    /// Manual checkpoint requested by user
    Manual,
    /// Automatic checkpoint based on interval
    Automatic,
    /// Checkpoint before significant event
    BeforeEvent(String),
    /// Checkpoint after significant event
    AfterEvent(String),
    /// Emergency checkpoint due to error
    Emergency(String),
    /// Checkpoint before test execution
    BeforeTest,
    /// Checkpoint after test completion
    AfterTest,
    /// Checkpoint for debugging purposes
    Debug,
}

impl Default for CheckpointReason {
    fn default() -> Self {
        CheckpointReason::Automatic
    }
}

impl CheckpointReason {
    /// Get a human-readable description of the checkpoint reason
    pub fn description(&self) -> String {
        match self {
            CheckpointReason::Manual => "Manual checkpoint".to_string(),
            CheckpointReason::Automatic => "Automatic checkpoint".to_string(),
            CheckpointReason::BeforeEvent(event) => format!("Before event: {}", event),
            CheckpointReason::AfterEvent(event) => format!("After event: {}", event),
            CheckpointReason::Emergency(context) => format!("Emergency: {}", context),
            CheckpointReason::BeforeTest => "Before test execution".to_string(),
            CheckpointReason::AfterTest => "After test completion".to_string(),
            CheckpointReason::Debug => "Debug checkpoint".to_string(),
        }
    }

    /// Get a short tag for the checkpoint reason
    pub fn tag(&self) -> &'static str {
        match self {
            CheckpointReason::Manual => "manual",
            CheckpointReason::Automatic => "auto",
            CheckpointReason::BeforeEvent(_) => "before_event",
            CheckpointReason::AfterEvent(_) => "after_event",
            CheckpointReason::Emergency(_) => "emergency",
            CheckpointReason::BeforeTest => "before_test",
            CheckpointReason::AfterTest => "after_test",
            CheckpointReason::Debug => "debug",
        }
    }
}

/// Builder for creating checkpoint metadata
#[derive(Debug, Default)]
pub struct CheckpointMetadataBuilder {
    label: Option<String>,
    tick: u64,
    context: HashMap<String, String>,
    reason: CheckpointReason,
}

impl CheckpointMetadataBuilder {
    /// Create a new checkpoint metadata builder
    pub fn new(tick: u64) -> Self {
        Self {
            tick,
            reason: CheckpointReason::Manual,
            ..Default::default()
        }
    }

    /// Set the checkpoint label
    pub fn with_label(mut self, label: String) -> Self {
        self.label = Some(label);
        self
    }

    /// Set the checkpoint reason
    pub fn with_reason(mut self, reason: CheckpointReason) -> Self {
        self.reason = reason;
        self
    }

    /// Add context information
    pub fn with_context(mut self, key: String, value: String) -> Self {
        self.context.insert(key, value);
        self
    }

    /// Add multiple context entries
    pub fn with_context_map(mut self, context: HashMap<String, String>) -> Self {
        self.context.extend(context);
        self
    }

    /// Build the checkpoint metadata
    pub fn build(self) -> CheckpointMetadata {
        CheckpointMetadata {
            id: generate_checkpoint_id(),
            label: self.label,
            tick: self.tick,
            created_at: current_unix_timestamp_secs(),
            context: self.context,
            reason: self.reason,
        }
    }
}

/// Utility functions for common checkpoint operations
pub struct CheckpointHelper;

impl CheckpointHelper {
    /// Create manual checkpoint metadata
    pub fn manual_checkpoint(tick: u64, label: Option<String>) -> CheckpointMetadata {
        let mut builder =
            CheckpointMetadataBuilder::new(tick).with_reason(CheckpointReason::Manual);

        if let Some(label) = label {
            builder = builder.with_label(label);
        }

        builder.build()
    }

    /// Create automatic checkpoint metadata
    pub fn automatic_checkpoint(tick: u64, interval: u64) -> CheckpointMetadata {
        CheckpointMetadataBuilder::new(tick)
            .with_reason(CheckpointReason::Automatic)
            .with_context("interval".to_string(), interval.to_string())
            .build()
    }

    /// Create event-based checkpoint metadata
    pub fn event_checkpoint(tick: u64, event_name: String, before: bool) -> CheckpointMetadata {
        let reason = if before {
            CheckpointReason::BeforeEvent(event_name.clone())
        } else {
            CheckpointReason::AfterEvent(event_name.clone())
        };

        CheckpointMetadataBuilder::new(tick)
            .with_reason(reason)
            .with_context("event_name".to_string(), event_name)
            .with_context(
                "timing".to_string(),
                if before { "before" } else { "after" }.to_string(),
            )
            .build()
    }

    /// Create emergency checkpoint metadata
    pub fn emergency_checkpoint(tick: u64, error_context: String) -> CheckpointMetadata {
        CheckpointMetadataBuilder::new(tick)
            .with_reason(CheckpointReason::Emergency(error_context.clone()))
            .with_context("error_context".to_string(), error_context)
            .with_context("emergency".to_string(), "true".to_string())
            .build()
    }

    /// Create test checkpoint metadata
    pub fn test_checkpoint(tick: u64, test_name: String, before: bool) -> CheckpointMetadata {
        let reason = if before {
            CheckpointReason::BeforeTest
        } else {
            CheckpointReason::AfterTest
        };

        CheckpointMetadataBuilder::new(tick)
            .with_reason(reason)
            .with_context("test_name".to_string(), test_name)
            .with_context(
                "timing".to_string(),
                if before { "before" } else { "after" }.to_string(),
            )
            .build()
    }

    /// Create debug checkpoint metadata
    pub fn debug_checkpoint(tick: u64, debug_context: String) -> CheckpointMetadata {
        CheckpointMetadataBuilder::new(tick)
            .with_reason(CheckpointReason::Debug)
            .with_label(format!("Debug: {}", debug_context))
            .with_context("debug_context".to_string(), debug_context)
            .build()
    }
}

/// Checkpoint filtering utilities
pub struct CheckpointFilter;

impl CheckpointFilter {
    /// Filter checkpoints by reason
    pub fn by_reason<'a>(
        checkpoints: &'a [CheckpointMetadata],
        reason: &CheckpointReason,
    ) -> Vec<&'a CheckpointMetadata> {
        checkpoints
            .iter()
            .filter(|cp| std::mem::discriminant(&cp.reason) == std::mem::discriminant(reason))
            .collect()
    }

    /// Filter checkpoints by tag
    pub fn by_tag<'a>(
        checkpoints: &'a [CheckpointMetadata],
        tag: &str,
    ) -> Vec<&'a CheckpointMetadata> {
        checkpoints
            .iter()
            .filter(|cp| cp.reason.tag() == tag)
            .collect()
    }

    /// Filter checkpoints by tick range
    pub fn by_tick_range(
        checkpoints: &[CheckpointMetadata],
        min_tick: u64,
        max_tick: u64,
    ) -> Vec<&CheckpointMetadata> {
        checkpoints
            .iter()
            .filter(|cp| cp.tick >= min_tick && cp.tick <= max_tick)
            .collect()
    }

    /// Filter checkpoints by time range
    pub fn by_time_range(
        checkpoints: &[CheckpointMetadata],
        min_time: u64,
        max_time: u64,
    ) -> Vec<&CheckpointMetadata> {
        checkpoints
            .iter()
            .filter(|cp| cp.created_at >= min_time && cp.created_at <= max_time)
            .collect()
    }

    /// Get the most recent checkpoint
    pub fn most_recent(checkpoints: &[CheckpointMetadata]) -> Option<&CheckpointMetadata> {
        checkpoints.iter().max_by_key(|cp| cp.created_at)
    }

    /// Get the oldest checkpoint
    pub fn oldest(checkpoints: &[CheckpointMetadata]) -> Option<&CheckpointMetadata> {
        checkpoints.iter().min_by_key(|cp| cp.created_at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checkpoint_reason_description() {
        let manual = CheckpointReason::Manual;
        let event = CheckpointReason::BeforeEvent("test_event".to_string());
        let emergency = CheckpointReason::Emergency("error occurred".to_string());

        assert_eq!(manual.description(), "Manual checkpoint");
        assert_eq!(event.description(), "Before event: test_event");
        assert_eq!(emergency.description(), "Emergency: error occurred");

        assert_eq!(manual.tag(), "manual");
        assert_eq!(event.tag(), "before_event");
        assert_eq!(emergency.tag(), "emergency");
    }

    #[test]
    fn test_checkpoint_metadata_builder() {
        let metadata = CheckpointMetadataBuilder::new(100)
            .with_label("test checkpoint".to_string())
            .with_reason(CheckpointReason::Manual)
            .with_context("key1".to_string(), "value1".to_string())
            .with_context("key2".to_string(), "value2".to_string())
            .build();

        assert_eq!(metadata.tick, 100);
        assert_eq!(metadata.label, Some("test checkpoint".to_string()));
        assert_eq!(metadata.reason, CheckpointReason::Manual);
        assert_eq!(metadata.context.get("key1"), Some(&"value1".to_string()));
        assert_eq!(metadata.context.get("key2"), Some(&"value2".to_string()));
        assert!(!metadata.id.is_empty());
        assert!(metadata.created_at > 0);
    }

    #[test]
    fn test_checkpoint_helper() {
        let manual = CheckpointHelper::manual_checkpoint(100, Some("test".to_string()));
        assert_eq!(manual.reason, CheckpointReason::Manual);
        assert_eq!(manual.label, Some("test".to_string()));

        let auto = CheckpointHelper::automatic_checkpoint(200, 50);
        assert_eq!(auto.reason, CheckpointReason::Automatic);
        assert_eq!(auto.context.get("interval"), Some(&"50".to_string()));

        let event = CheckpointHelper::event_checkpoint(300, "test_event".to_string(), true);
        assert!(matches!(event.reason, CheckpointReason::BeforeEvent(_)));

        let emergency = CheckpointHelper::emergency_checkpoint(400, "error".to_string());
        assert!(matches!(emergency.reason, CheckpointReason::Emergency(_)));
    }

    #[test]
    fn test_checkpoint_filter() {
        let checkpoints = vec![
            CheckpointHelper::manual_checkpoint(100, None),
            CheckpointHelper::automatic_checkpoint(200, 50),
            CheckpointHelper::event_checkpoint(300, "test".to_string(), true),
            CheckpointHelper::manual_checkpoint(400, None),
        ];

        let manual_checkpoints = CheckpointFilter::by_tag(&checkpoints, "manual");
        assert_eq!(manual_checkpoints.len(), 2);

        let auto_checkpoints = CheckpointFilter::by_tag(&checkpoints, "auto");
        assert_eq!(auto_checkpoints.len(), 1);

        let tick_range = CheckpointFilter::by_tick_range(&checkpoints, 150, 350);
        assert_eq!(tick_range.len(), 2);

        let most_recent = CheckpointFilter::most_recent(&checkpoints);
        assert!(most_recent.is_some());
        assert_eq!(most_recent.unwrap().tick, 400);
    }
}
