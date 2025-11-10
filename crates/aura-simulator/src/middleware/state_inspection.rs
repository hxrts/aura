//! State inspection middleware for monitoring and querying simulation state

use super::{
    Result, SimulatorContext, SimulatorError, SimulatorHandler, SimulatorMiddleware,
    SimulatorOperation, StateQuery,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Middleware for inspecting and monitoring simulation state
pub struct StateInspectionMiddleware {
    /// State snapshots for historical queries
    state_snapshots: HashMap<String, StateSnapshot>,
    /// Active state watchers
    watchers: HashMap<String, StateWatcher>,
    /// State change triggers
    triggers: Vec<StateTrigger>,
    /// Enable automatic state capturing
    auto_capture: bool,
    /// Capture interval in ticks
    capture_interval: u64,
    /// Maximum number of snapshots to retain
    max_snapshots: usize,
    /// Enable detailed state diff tracking
    enable_diff_tracking: bool,
}

impl StateInspectionMiddleware {
    /// Create new state inspection middleware
    pub fn new() -> Self {
        Self {
            state_snapshots: HashMap::new(),
            watchers: HashMap::new(),
            triggers: Vec::new(),
            auto_capture: true,
            capture_interval: 10,
            max_snapshots: 100,
            enable_diff_tracking: true,
        }
    }

    /// Configure automatic state capturing
    pub fn with_auto_capture(mut self, enable: bool, interval: u64) -> Self {
        self.auto_capture = enable;
        self.capture_interval = interval;
        self
    }

    /// Set maximum number of snapshots to retain
    pub fn with_max_snapshots(mut self, max: usize) -> Self {
        self.max_snapshots = max;
        self
    }

    /// Enable or disable diff tracking
    pub fn with_diff_tracking(mut self, enable: bool) -> Self {
        self.enable_diff_tracking = enable;
        self
    }

    /// Add state watcher
    pub fn with_watcher(mut self, watcher: StateWatcher) -> Self {
        self.watchers.insert(watcher.id.clone(), watcher);
        self
    }

    /// Add state change trigger
    pub fn with_trigger(mut self, trigger: StateTrigger) -> Self {
        self.triggers.push(trigger);
        self
    }

    /// Inspect simulation state
    fn inspect_state(
        &self,
        component: &str,
        query: StateQuery,
        context: &SimulatorContext,
    ) -> Result<Value> {
        match query {
            StateQuery::GetAll => {
                // Return all available state information
                Ok(json!({
                    "component": component,
                    "query_type": "get_all",
                    "timestamp": context.timestamp.as_millis(),
                    "tick": context.tick,
                    "state": {
                        "scenario_id": context.scenario_id,
                        "participant_count": context.participant_count,
                        "threshold": context.threshold,
                        "metadata": context.metadata,
                        "snapshots_available": self.state_snapshots.len(),
                        "watchers_active": self.watchers.len()
                    }
                }))
            }

            StateQuery::GetField { field } => {
                // Return specific field value
                let value = match field.as_str() {
                    "tick" => json!(context.tick),
                    "timestamp" => json!(context.timestamp.as_millis()),
                    "scenario_id" => json!(context.scenario_id),
                    "participant_count" => json!(context.participant_count),
                    "threshold" => json!(context.threshold),
                    _ => {
                        // Check metadata
                        context
                            .metadata
                            .get(&field)
                            .map(|v| json!(v))
                            .unwrap_or(json!(null))
                    }
                };

                Ok(json!({
                    "component": component,
                    "query_type": "get_field",
                    "field": field,
                    "value": value,
                    "timestamp": context.timestamp.as_millis()
                }))
            }

            StateQuery::Query { filter } => {
                // Apply filter to state data
                let filtered_state = self.apply_filter(&filter, context)?;

                Ok(json!({
                    "component": component,
                    "query_type": "query",
                    "filter": filter,
                    "result": filtered_state,
                    "timestamp": context.timestamp.as_millis()
                }))
            }

            StateQuery::GetHistory { since } => {
                // Return state history
                let history = self.get_state_history(since, context);

                Ok(json!({
                    "component": component,
                    "query_type": "get_history",
                    "since": since.map(|d| d.as_millis()),
                    "history": history,
                    "timestamp": context.timestamp.as_millis()
                }))
            }

            StateQuery::GetDiff { from, to } => {
                // Return diff between two snapshots
                let diff = self.get_state_diff(&from, &to)?;

                Ok(json!({
                    "component": component,
                    "query_type": "get_diff",
                    "from": from,
                    "to": to,
                    "diff": diff,
                    "timestamp": context.timestamp.as_millis()
                }))
            }
        }
    }

    /// Apply filter to state data
    fn apply_filter(&self, filter: &str, context: &SimulatorContext) -> Result<Value> {
        // Simple filter implementation - in a real system this would be more sophisticated
        match filter {
            "active_participants" => Ok(json!({
                "count": context.participant_count,
                "threshold": context.threshold
            })),

            "timing_info" => Ok(json!({
                "tick": context.tick,
                "timestamp": context.timestamp.as_millis(),
                "scenario_duration": context.timestamp.as_millis()
            })),

            "metadata" => Ok(json!(context.metadata)),

            _ => {
                // Unknown filter
                Ok(json!({
                    "error": "unknown_filter",
                    "filter": filter
                }))
            }
        }
    }

    /// Get state history
    fn get_state_history(&self, since: Option<Duration>, _context: &SimulatorContext) -> Value {
        let cutoff_time = since.unwrap_or(Duration::from_secs(0));

        let relevant_snapshots: Vec<_> = self
            .state_snapshots
            .values()
            .filter(|snapshot| snapshot.timestamp >= cutoff_time)
            .map(|snapshot| {
                json!({
                    "id": snapshot.id,
                    "timestamp": snapshot.timestamp.as_millis(),
                    "tick": snapshot.tick,
                    "component_count": snapshot.state_data.len()
                })
            })
            .collect();

        json!({
            "snapshots": relevant_snapshots,
            "total_count": relevant_snapshots.len(),
            "cutoff_time": cutoff_time.as_millis()
        })
    }

    /// Get diff between two state snapshots
    fn get_state_diff(&self, from: &str, to: &str) -> Result<Value> {
        let from_snapshot = self.state_snapshots.get(from).ok_or_else(|| {
            SimulatorError::StateInspectionFailed(format!("From snapshot not found: {}", from))
        })?;

        let to_snapshot = self.state_snapshots.get(to).ok_or_else(|| {
            SimulatorError::StateInspectionFailed(format!("To snapshot not found: {}", to))
        })?;

        // Simple diff - TODO fix - In a real implementation this would be more sophisticated
        let mut changes = Vec::new();

        // Check for changed values
        for (key, to_value) in &to_snapshot.state_data {
            if let Some(from_value) = from_snapshot.state_data.get(key) {
                if from_value != to_value {
                    changes.push(json!({
                        "type": "changed",
                        "key": key,
                        "from": from_value,
                        "to": to_value
                    }));
                }
            } else {
                changes.push(json!({
                    "type": "added",
                    "key": key,
                    "value": to_value
                }));
            }
        }

        // Check for removed values
        for (key, from_value) in &from_snapshot.state_data {
            if !to_snapshot.state_data.contains_key(key) {
                changes.push(json!({
                    "type": "removed",
                    "key": key,
                    "value": from_value
                }));
            }
        }

        Ok(json!({
            "from_snapshot": from,
            "to_snapshot": to,
            "from_tick": from_snapshot.tick,
            "to_tick": to_snapshot.tick,
            "changes": changes,
            "change_count": changes.len()
        }))
    }

    /// Capture current state snapshot
    fn capture_state_snapshot(&mut self, context: &SimulatorContext) -> String {
        let snapshot_id = format!(
            "snapshot_{}_{}",
            context.tick,
            context.timestamp.as_millis()
        );

        let mut state_data = HashMap::new();
        state_data.insert("scenario_id".to_string(), json!(context.scenario_id));
        state_data.insert(
            "participant_count".to_string(),
            json!(context.participant_count),
        );
        state_data.insert("threshold".to_string(), json!(context.threshold));
        state_data.insert("metadata".to_string(), json!(context.metadata));

        let snapshot = StateSnapshot {
            id: snapshot_id.clone(),
            timestamp: context.timestamp,
            tick: context.tick,
            state_data,
            captured_at: Instant::now(),
        };

        self.state_snapshots.insert(snapshot_id.clone(), snapshot);

        // Cleanup old snapshots if we exceed the limit
        if self.state_snapshots.len() > self.max_snapshots {
            // Remove oldest snapshots (TODO fix - Simplified - would use a more efficient data structure in practice)
            let mut snapshots: Vec<_> = self.state_snapshots.iter().collect();
            snapshots.sort_by_key(|(_, snapshot)| snapshot.tick);

            if let Some((oldest_id, _)) = snapshots.first() {
                let oldest_id = (*oldest_id).clone();
                self.state_snapshots.remove(&oldest_id);
            }
        }

        snapshot_id
    }

    /// Process state watchers
    fn process_watchers(&self, context: &SimulatorContext) -> Vec<WatcherAlert> {
        let mut alerts = Vec::new();

        for watcher in self.watchers.values() {
            if let Some(alert) = self.check_watcher(watcher, context) {
                alerts.push(alert);
            }
        }

        alerts
    }

    /// Check individual watcher for alerts
    fn check_watcher(
        &self,
        watcher: &StateWatcher,
        context: &SimulatorContext,
    ) -> Option<WatcherAlert> {
        match &watcher.condition {
            WatcherCondition::FieldEquals { field, value } => {
                let current_value = match field.as_str() {
                    "tick" => json!(context.tick),
                    "participant_count" => json!(context.participant_count),
                    "threshold" => json!(context.threshold),
                    _ => context
                        .metadata
                        .get(field)
                        .map(|v| json!(v))
                        .unwrap_or(json!(null)),
                };

                if &current_value == value {
                    Some(WatcherAlert {
                        watcher_id: watcher.id.clone(),
                        message: format!("Field '{}' equals expected value", field),
                        timestamp: context.timestamp,
                        tick: context.tick,
                        details: json!({
                            "field": field,
                            "value": current_value
                        }),
                    })
                } else {
                    None
                }
            }

            WatcherCondition::TickReached { target_tick } => {
                if context.tick >= *target_tick {
                    Some(WatcherAlert {
                        watcher_id: watcher.id.clone(),
                        message: format!("Reached target tick {}", target_tick),
                        timestamp: context.timestamp,
                        tick: context.tick,
                        details: json!({
                            "target_tick": target_tick,
                            "current_tick": context.tick
                        }),
                    })
                } else {
                    None
                }
            }

            WatcherCondition::Custom { expression } => {
                // TODO fix - Simplified custom expression evaluation
                // TODO fix - In a real implementation, this would use a proper expression evaluator
                Some(WatcherAlert {
                    watcher_id: watcher.id.clone(),
                    message: format!("Custom condition triggered: {}", expression),
                    timestamp: context.timestamp,
                    tick: context.tick,
                    details: json!({
                        "expression": expression
                    }),
                })
            }
        }
    }

    /// Check if auto-capture should occur
    fn should_auto_capture(&self, context: &SimulatorContext) -> bool {
        self.auto_capture && (context.tick % self.capture_interval == 0)
    }
}

impl Default for StateInspectionMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl SimulatorMiddleware for StateInspectionMiddleware {
    fn process(
        &self,
        operation: SimulatorOperation,
        context: &SimulatorContext,
        next: &dyn SimulatorHandler,
    ) -> Result<Value> {
        match &operation {
            SimulatorOperation::InspectState { component, query } => {
                // Handle state inspection request
                let inspection_result = self.inspect_state(component, query.clone(), context)?;

                // Add inspection info to context
                let mut enhanced_context = context.clone();
                enhanced_context
                    .metadata
                    .insert("state_inspected".to_string(), component.clone());

                // Call next handler
                let mut result = next.handle(operation, &enhanced_context)?;

                // Add inspection results
                if let Some(obj) = result.as_object_mut() {
                    obj.insert("state_inspection".to_string(), inspection_result);
                }

                Ok(result)
            }

            SimulatorOperation::ExecuteTick { .. } => {
                // Check if we should auto-capture state
                let should_capture = self.should_auto_capture(context);

                // Process watchers
                let watcher_alerts = self.process_watchers(context);

                // Add state inspection info to context
                let mut enhanced_context = context.clone();
                enhanced_context.metadata.insert(
                    "snapshots_available".to_string(),
                    self.state_snapshots.len().to_string(),
                );
                enhanced_context.metadata.insert(
                    "watchers_active".to_string(),
                    self.watchers.len().to_string(),
                );

                if should_capture {
                    enhanced_context
                        .metadata
                        .insert("auto_capture_scheduled".to_string(), "true".to_string());
                }

                // Call next handler
                let mut result = next.handle(operation, &enhanced_context)?;

                // Add state inspection information
                if let Some(obj) = result.as_object_mut() {
                    obj.insert(
                        "state_inspection".to_string(),
                        json!({
                            "snapshots_count": self.state_snapshots.len(),
                            "watchers_count": self.watchers.len(),
                            "auto_capture": should_capture,
                            "watcher_alerts": watcher_alerts.len(),
                            "alerts": watcher_alerts.iter().map(|alert| json!({
                                "watcher_id": alert.watcher_id,
                                "message": alert.message,
                                "tick": alert.tick
                            })).collect::<Vec<_>>()
                        }),
                    );
                }

                Ok(result)
            }

            _ => {
                // For other operations, just add state inspection metadata
                let mut enhanced_context = context.clone();
                enhanced_context
                    .metadata
                    .insert("state_inspection_available".to_string(), "true".to_string());

                next.handle(operation, &enhanced_context)
            }
        }
    }

    fn name(&self) -> &str {
        "state_inspection"
    }
}

/// State snapshot for historical queries
#[derive(Debug, Clone)]
struct StateSnapshot {
    id: String,
    timestamp: Duration,
    tick: u64,
    state_data: HashMap<String, Value>,
    captured_at: Instant,
}

/// State watcher for monitoring specific conditions
#[derive(Debug, Clone)]
pub struct StateWatcher {
    /// Unique watcher identifier
    pub id: String,
    /// Human-readable description
    pub description: String,
    /// Condition to watch for
    pub condition: WatcherCondition,
    /// Whether the watcher is active
    pub active: bool,
}

/// Conditions that watchers can monitor
#[derive(Debug, Clone)]
pub enum WatcherCondition {
    /// Field equals specific value
    FieldEquals { field: String, value: Value },
    /// Specific tick is reached
    TickReached { target_tick: u64 },
    /// Custom expression evaluation
    Custom { expression: String },
}

/// Alert generated by state watcher
#[derive(Debug, Clone)]
struct WatcherAlert {
    watcher_id: String,
    message: String,
    timestamp: Duration,
    tick: u64,
    details: Value,
}

/// State change triggers
#[derive(Debug, Clone)]
pub struct StateTrigger {
    /// Trigger identifier
    pub id: String,
    /// Condition for triggering
    pub condition: WatcherCondition,
    /// Action to take when triggered
    pub action: TriggerAction,
}

/// Actions that can be triggered by state changes
#[derive(Debug, Clone)]
pub enum TriggerAction {
    /// Capture state snapshot
    CaptureSnapshot { id: String },
    /// Send notification
    Notify { message: String },
    /// Execute custom action
    Custom {
        action: String,
        parameters: HashMap<String, Value>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::handler::NoOpSimulatorHandler;

    #[test]
    fn test_state_inspection_creation() {
        let middleware = StateInspectionMiddleware::new()
            .with_auto_capture(true, 5)
            .with_max_snapshots(50)
            .with_diff_tracking(true);

        assert!(middleware.auto_capture);
        assert_eq!(middleware.capture_interval, 5);
        assert_eq!(middleware.max_snapshots, 50);
        assert!(middleware.enable_diff_tracking);
    }

    #[test]
    fn test_state_inspection_operation() {
        let middleware = StateInspectionMiddleware::new();
        let handler = NoOpSimulatorHandler;
        let context = SimulatorContext::new("test".to_string(), "run1".to_string());

        let result = middleware.process(
            SimulatorOperation::InspectState {
                component: "simulation".to_string(),
                query: StateQuery::GetAll,
            },
            &context,
            &handler,
        );

        assert!(result.is_ok());
        let value = result.unwrap();
        assert!(value.get("state_inspection").is_some());
    }

    #[test]
    fn test_state_watcher() {
        let watcher = StateWatcher {
            id: "test_watcher".to_string(),
            description: "Test watcher".to_string(),
            condition: WatcherCondition::TickReached { target_tick: 100 },
            active: true,
        };

        assert_eq!(watcher.id, "test_watcher");
        assert!(watcher.active);
    }

    #[test]
    fn test_auto_capture_logic() {
        let middleware = StateInspectionMiddleware::new().with_auto_capture(true, 10);

        let mut context = SimulatorContext::new("test".to_string(), "run1".to_string());

        // Should not capture at tick 5
        context.tick = 5;
        assert!(!middleware.should_auto_capture(&context));

        // Should capture at tick 10
        context.tick = 10;
        assert!(middleware.should_auto_capture(&context));

        // Should capture at tick 20
        context.tick = 20;
        assert!(middleware.should_auto_capture(&context));
    }
}
