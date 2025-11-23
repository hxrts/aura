//! Scenario management effect handler for simulation
//!
//! This module provides simulation-specific scenario injection and management
//! capabilities. Replaces the former ScenarioInjectionMiddleware with proper
//! effect system integration.

use async_trait::async_trait;
use aura_core::effects::{TestingEffects, TestingError};
use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Scenario definition for dynamic injection
#[derive(Debug, Clone)]
pub struct ScenarioDefinition {
    /// Unique identifier for this scenario
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Actions to perform when scenario triggers
    pub actions: Vec<InjectionAction>,
    /// Conditions that trigger this scenario
    pub trigger: TriggerCondition,
    /// Duration this scenario remains active
    pub duration: Option<Duration>,
    /// Priority level for conflict resolution
    pub priority: u32,
}

/// Action to perform during scenario injection
#[derive(Debug, Clone)]
pub enum InjectionAction {
    /// Modify simulation parameter
    ModifyParameter { key: String, value: String },
    /// Inject custom event
    InjectEvent {
        event_type: String,
        data: HashMap<String, String>,
    },
    /// Change simulation behavior
    ModifyBehavior { component: String, behavior: String },
    /// Trigger fault injection
    TriggerFault {
        fault_type: String,
        parameters: HashMap<String, String>,
    },
    /// Create chat group for multi-actor scenarios
    CreateChatGroup {
        group_name: String,
        creator: String,
        initial_members: Vec<String>,
    },
    /// Send chat message in scenario
    SendChatMessage {
        group_id: String,
        sender: String,
        message: String,
    },
    /// Simulate account data loss
    SimulateDataLoss {
        target_participant: String,
        loss_type: String,
        recovery_required: bool,
    },
    /// Validate message history across recovery
    ValidateMessageHistory {
        participant: String,
        expected_message_count: usize,
        include_pre_recovery: bool,
    },
    /// Initiate guardian recovery process
    InitiateGuardianRecovery {
        target: String,
        guardians: Vec<String>,
        threshold: usize,
    },
    /// Verify recovery completion
    VerifyRecoverySuccess {
        target: String,
        validation_steps: Vec<String>,
    },
}

/// Conditions for triggering scenarios
#[derive(Debug, Clone)]
pub enum TriggerCondition {
    /// Trigger immediately
    Immediate,
    /// Trigger after specific time
    AfterTime(Duration),
    /// Trigger when simulation reaches tick count
    AtTick(u64),
    /// Trigger when specific event occurs
    OnEvent(String),
    /// Trigger randomly based on probability
    Random(f64),
}

/// Currently active scenario injection
#[derive(Debug)]
struct ActiveInjection {
    scenario_id: String,
    start_time: Instant,
    duration: Option<Duration>,
    actions_applied: Vec<String>,
}

/// Internal state for scenario management
#[derive(Debug)]
struct ScenarioState {
    scenarios: HashMap<String, ScenarioDefinition>,
    active_injections: Vec<ActiveInjection>,
    checkpoints: HashMap<String, ScenarioCheckpoint>,
    events: Vec<SimulationEvent>,
    metrics: HashMap<String, MetricValue>,
    enable_randomization: bool,
    injection_probability: f64,
    max_concurrent_injections: usize,
    total_injections: u64,
    seed: u64,
    // Multi-actor chat support
    chat_groups: HashMap<String, ChatGroup>,
    message_history: HashMap<String, Vec<ChatMessage>>, // group_id -> messages
    participant_data_loss: HashMap<String, DataLossInfo>,
    recovery_state: HashMap<String, RecoveryInfo>,
}

#[derive(Debug, Clone)]
struct ChatGroup {
    id: String,
    name: String,
    creator: String,
    members: Vec<String>,
    created_at: Instant,
}

#[derive(Debug, Clone)]
struct ChatMessage {
    id: String,
    group_id: String,
    sender: String,
    content: String,
    timestamp: Instant,
}

#[derive(Debug, Clone)]
struct DataLossInfo {
    participant: String,
    loss_type: String,
    occurred_at: Instant,
    recovery_required: bool,
    pre_loss_message_count: usize,
}

#[derive(Debug, Clone)]
struct RecoveryInfo {
    target: String,
    guardians: Vec<String>,
    threshold: usize,
    initiated_at: Instant,
    completed: bool,
    validation_steps: Vec<String>,
}

#[derive(Debug, Clone)]
struct ScenarioCheckpoint {
    id: String,
    label: String,
    timestamp: Instant,
    state_snapshot: HashMap<String, String>,
}

#[derive(Debug, Clone)]
struct SimulationEvent {
    event_type: String,
    timestamp: Instant,
    data: HashMap<String, String>,
}

#[derive(Debug, Clone)]
struct MetricValue {
    value: f64,
    unit: String,
    timestamp: Instant,
}

/// Simulation-specific scenario management handler
pub struct SimulationScenarioHandler {
    state: Arc<Mutex<ScenarioState>>,
}

impl SimulationScenarioHandler {
    /// Create a new scenario handler
    pub fn new(seed: u64) -> Self {
        Self {
            state: Arc::new(Mutex::new(ScenarioState {
                scenarios: HashMap::new(),
                active_injections: Vec::new(),
                checkpoints: HashMap::new(),
                events: Vec::new(),
                metrics: HashMap::new(),
                enable_randomization: false,
                injection_probability: 0.1,
                max_concurrent_injections: 3,
                total_injections: 0,
                seed,
                chat_groups: HashMap::new(),
                message_history: HashMap::new(),
                participant_data_loss: HashMap::new(),
                recovery_state: HashMap::new(),
            })),
        }
    }

    /// Register a scenario for potential injection
    pub fn register_scenario(&self, scenario: ScenarioDefinition) -> Result<(), TestingError> {
        let mut state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {}", e)))
        })?;

        state.scenarios.insert(scenario.id.clone(), scenario);
        Ok(())
    }

    /// Enable or disable random scenario injection
    pub fn set_randomization(&self, enable: bool, probability: f64) -> Result<(), TestingError> {
        let mut state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {}", e)))
        })?;

        state.enable_randomization = enable;
        state.injection_probability = probability.clamp(0.0, 1.0);
        Ok(())
    }

    /// Manually trigger a specific scenario
    pub fn trigger_scenario(&self, scenario_id: &str) -> Result<(), TestingError> {
        let mut state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {}", e)))
        })?;

        if state.active_injections.len() >= state.max_concurrent_injections {
            return Err(TestingError::EventRecordingError {
                event_type: "scenario_trigger".to_string(),
                reason: "Maximum concurrent injections reached".to_string(),
            });
        }

        let scenario =
            state
                .scenarios
                .get(scenario_id)
                .ok_or_else(|| TestingError::EventRecordingError {
                    event_type: "scenario_trigger".to_string(),
                    reason: format!("Scenario '{}' not found", scenario_id),
                })?;

        let injection = ActiveInjection {
            scenario_id: scenario_id.to_string(),
            start_time: Instant::now(),
            duration: scenario.duration,
            actions_applied: Vec::new(),
        };

        state.active_injections.push(injection);
        state.total_injections += 1;

        Ok(())
    }

    /// Get statistics about scenario injections
    pub fn get_injection_stats(&self) -> Result<HashMap<String, String>, TestingError> {
        let state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {}", e)))
        })?;

        let mut stats = HashMap::new();
        stats.insert(
            "total_injections".to_string(),
            state.total_injections.to_string(),
        );
        stats.insert(
            "active_injections".to_string(),
            state.active_injections.len().to_string(),
        );
        stats.insert(
            "registered_scenarios".to_string(),
            state.scenarios.len().to_string(),
        );
        stats.insert(
            "randomization_enabled".to_string(),
            state.enable_randomization.to_string(),
        );
        stats.insert(
            "injection_probability".to_string(),
            state.injection_probability.to_string(),
        );

        Ok(stats)
    }

    /// Clean up expired injections
    fn cleanup_expired_injections(&self) -> Result<(), TestingError> {
        let mut state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {}", e)))
        })?;

        let now = Instant::now();
        state.active_injections.retain(|injection| {
            match injection.duration {
                Some(duration) => now.duration_since(injection.start_time) < duration,
                None => true, // Permanent injections stay active
            }
        });

        Ok(())
    }

    /// Check if scenario should be randomly triggered
    fn should_trigger_random_scenario(&self) -> Result<bool, TestingError> {
        let state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {}", e)))
        })?;

        if !state.enable_randomization {
            return Ok(false);
        }

        if state.active_injections.len() >= state.max_concurrent_injections {
            return Ok(false);
        }

        // Use deterministic pseudo-random based on seed
        let mut rng_state = state.seed;
        rng_state = rng_state.wrapping_mul(1103515245).wrapping_add(12345);
        let random_value = (rng_state >> 16) as f64 / u16::MAX as f64;

        Ok(random_value < state.injection_probability)
    }

    /// Create a chat group for multi-actor scenarios
    pub fn create_chat_group(
        &self,
        group_name: &str,
        creator: &str,
        initial_members: Vec<String>,
    ) -> Result<String, TestingError> {
        let mut state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {}", e)))
        })?;

        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        group_name.hash(&mut hasher);
        creator.hash(&mut hasher);
        let group_id = format!("group_{:x}", hasher.finish());

        let mut members = initial_members;
        if !members.contains(&creator.to_string()) {
            members.insert(0, creator.to_string());
        }

        let chat_group = ChatGroup {
            id: group_id.clone(),
            name: group_name.to_string(),
            creator: creator.to_string(),
            members,
            created_at: Instant::now(),
        };

        state.chat_groups.insert(group_id.clone(), chat_group);
        state.message_history.insert(group_id.clone(), Vec::new());

        Ok(group_id)
    }

    /// Send a chat message in a scenario
    pub fn send_chat_message(
        &self,
        group_id: &str,
        sender: &str,
        message: &str,
    ) -> Result<(), TestingError> {
        let mut state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {}", e)))
        })?;

        // Verify group exists and sender is a member
        let group =
            state
                .chat_groups
                .get(group_id)
                .ok_or_else(|| TestingError::EventRecordingError {
                    event_type: "chat_message".to_string(),
                    reason: format!("Chat group '{}' not found", group_id),
                })?;

        if !group.members.contains(&sender.to_string()) {
            return Err(TestingError::EventRecordingError {
                event_type: "chat_message".to_string(),
                reason: format!(
                    "Sender '{}' is not a member of group '{}'",
                    sender, group_id
                ),
            });
        }

        let message_id = format!("msg_{}_{}", sender, state.metrics.len());
        let chat_message = ChatMessage {
            id: message_id,
            group_id: group_id.to_string(),
            sender: sender.to_string(),
            content: message.to_string(),
            timestamp: Instant::now(),
        };

        let messages = state.message_history.get_mut(group_id).unwrap();
        messages.push(chat_message);

        Ok(())
    }

    /// Simulate data loss for a participant
    pub fn simulate_data_loss(
        &self,
        target_participant: &str,
        loss_type: &str,
        recovery_required: bool,
    ) -> Result<(), TestingError> {
        let mut state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {}", e)))
        })?;

        // Count messages participant had access to before loss
        let pre_loss_count: usize = state
            .message_history
            .values()
            .map(|messages| {
                messages
                    .iter()
                    .filter(|msg| {
                        // Count messages in groups where participant is a member
                        state
                            .chat_groups
                            .values()
                            .any(|g| g.members.contains(&target_participant.to_string()))
                    })
                    .count()
            })
            .sum();

        let data_loss_info = DataLossInfo {
            participant: target_participant.to_string(),
            loss_type: loss_type.to_string(),
            occurred_at: Instant::now(),
            recovery_required,
            pre_loss_message_count: pre_loss_count,
        };

        state
            .participant_data_loss
            .insert(target_participant.to_string(), data_loss_info);

        Ok(())
    }

    /// Validate message history for a participant across recovery
    pub fn validate_message_history(
        &self,
        participant: &str,
        expected_message_count: usize,
        include_pre_recovery: bool,
    ) -> Result<bool, TestingError> {
        let state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {}", e)))
        })?;

        let actual_count: usize = state
            .message_history
            .values()
            .map(|messages| {
                messages
                    .iter()
                    .filter(|msg| {
                        // Count messages in groups where participant is a member
                        state
                            .chat_groups
                            .values()
                            .any(|g| g.members.contains(&participant.to_string()))
                    })
                    .count()
            })
            .sum();

        if include_pre_recovery {
            if let Some(loss_info) = state.participant_data_loss.get(participant) {
                // For recovery scenarios, participant should be able to see pre-loss messages
                Ok(actual_count >= loss_info.pre_loss_message_count
                    && actual_count >= expected_message_count)
            } else {
                Ok(actual_count >= expected_message_count)
            }
        } else {
            Ok(actual_count >= expected_message_count)
        }
    }

    /// Initiate guardian recovery for a participant
    pub fn initiate_guardian_recovery(
        &self,
        target: &str,
        guardians: Vec<String>,
        threshold: usize,
    ) -> Result<(), TestingError> {
        let mut state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {}", e)))
        })?;

        if guardians.len() < threshold {
            return Err(TestingError::EventRecordingError {
                event_type: "guardian_recovery".to_string(),
                reason: format!(
                    "Insufficient guardians: {} provided, {} required",
                    guardians.len(),
                    threshold
                ),
            });
        }

        let recovery_info = RecoveryInfo {
            target: target.to_string(),
            guardians,
            threshold,
            initiated_at: Instant::now(),
            completed: false,
            validation_steps: Vec::new(),
        };

        state
            .recovery_state
            .insert(target.to_string(), recovery_info);

        Ok(())
    }

    /// Verify recovery completion
    pub fn verify_recovery_success(
        &self,
        target: &str,
        validation_steps: Vec<String>,
    ) -> Result<bool, TestingError> {
        let mut state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {}", e)))
        })?;

        if let Some(recovery_info) = state.recovery_state.get_mut(target) {
            recovery_info.completed = true;
            recovery_info.validation_steps = validation_steps;

            // Clear data loss status if recovery is successful
            state.participant_data_loss.remove(target);

            Ok(true)
        } else {
            Err(TestingError::EventRecordingError {
                event_type: "recovery_verification".to_string(),
                reason: format!("No recovery process found for target '{}'", target),
            })
        }
    }

    /// Get chat group statistics
    pub fn get_chat_stats(&self) -> Result<HashMap<String, String>, TestingError> {
        let state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {}", e)))
        })?;

        let mut stats = HashMap::new();
        stats.insert(
            "chat_groups".to_string(),
            state.chat_groups.len().to_string(),
        );
        stats.insert(
            "total_messages".to_string(),
            state
                .message_history
                .values()
                .map(|msgs| msgs.len())
                .sum::<usize>()
                .to_string(),
        );
        stats.insert(
            "participants_with_data_loss".to_string(),
            state.participant_data_loss.len().to_string(),
        );
        stats.insert(
            "active_recoveries".to_string(),
            state
                .recovery_state
                .values()
                .filter(|r| !r.completed)
                .count()
                .to_string(),
        );

        Ok(stats)
    }
}

impl Default for SimulationScenarioHandler {
    fn default() -> Self {
        Self::new(42) // Default deterministic seed
    }
}

#[async_trait]
impl TestingEffects for SimulationScenarioHandler {
    async fn create_checkpoint(
        &self,
        checkpoint_id: &str,
        label: &str,
    ) -> Result<(), TestingError> {
        let mut state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {}", e)))
        })?;

        let checkpoint = ScenarioCheckpoint {
            id: checkpoint_id.to_string(),
            label: label.to_string(),
            timestamp: Instant::now(),
            state_snapshot: HashMap::new(), // TODO: Capture actual state
        };

        state
            .checkpoints
            .insert(checkpoint_id.to_string(), checkpoint);
        Ok(())
    }

    async fn restore_checkpoint(&self, checkpoint_id: &str) -> Result<(), TestingError> {
        let state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {}", e)))
        })?;

        let _checkpoint =
            state
                .checkpoints
                .get(checkpoint_id)
                .ok_or_else(|| TestingError::CheckpointError {
                    checkpoint_id: checkpoint_id.to_string(),
                    reason: "Checkpoint not found".to_string(),
                })?;

        // TODO: Implement actual state restoration
        Ok(())
    }

    async fn inspect_state(
        &self,
        component: &str,
        path: &str,
    ) -> Result<Box<dyn Any + Send>, TestingError> {
        let state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {}", e)))
        })?;

        match component {
            "scenarios" => {
                if path == "count" {
                    Ok(Box::new(state.scenarios.len()))
                } else if path == "active" {
                    Ok(Box::new(state.active_injections.len()))
                } else {
                    Err(TestingError::StateInspectionError {
                        component: component.to_string(),
                        path: path.to_string(),
                        reason: "Unknown scenario path".to_string(),
                    })
                }
            }
            "chat" => match path {
                "groups" => Ok(Box::new(state.chat_groups.len())),
                "total_messages" => Ok(Box::new(
                    state
                        .message_history
                        .values()
                        .map(|msgs| msgs.len())
                        .sum::<usize>(),
                )),
                _ => {
                    if let Some(group) = state.chat_groups.get(path) {
                        Ok(Box::new(group.members.len()))
                    } else {
                        Err(TestingError::StateInspectionError {
                            component: component.to_string(),
                            path: path.to_string(),
                            reason: "Chat group not found".to_string(),
                        })
                    }
                }
            },
            "data_loss" => {
                if let Some(loss_info) = state.participant_data_loss.get(path) {
                    Ok(Box::new(loss_info.pre_loss_message_count))
                } else {
                    Ok(Box::new(0usize)) // No data loss recorded
                }
            }
            "recovery" => {
                if let Some(recovery_info) = state.recovery_state.get(path) {
                    Ok(Box::new(recovery_info.completed))
                } else {
                    Ok(Box::new(false)) // No recovery in progress
                }
            }
            "metrics" => {
                if let Some(metric) = state.metrics.get(path) {
                    Ok(Box::new(metric.value))
                } else {
                    Err(TestingError::StateInspectionError {
                        component: component.to_string(),
                        path: path.to_string(),
                        reason: "Metric not found".to_string(),
                    })
                }
            }
            _ => Err(TestingError::StateInspectionError {
                component: component.to_string(),
                path: path.to_string(),
                reason: "Unknown component".to_string(),
            }),
        }
    }

    async fn assert_property(
        &self,
        property_id: &str,
        condition: bool,
        description: &str,
    ) -> Result<(), TestingError> {
        if !condition {
            return Err(TestingError::PropertyAssertionFailed {
                property_id: property_id.to_string(),
                description: description.to_string(),
            });
        }
        Ok(())
    }

    async fn record_event(
        &self,
        event_type: &str,
        event_data: HashMap<String, String>,
    ) -> Result<(), TestingError> {
        let mut state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {}", e)))
        })?;

        let event = SimulationEvent {
            event_type: event_type.to_string(),
            timestamp: Instant::now(),
            data: event_data,
        };

        state.events.push(event);

        // Check for scenario triggers based on events
        if event_type == "scenario_trigger_request" {
            self.cleanup_expired_injections()?;
            if self.should_trigger_random_scenario()? {
                // Could trigger a random scenario here
            }
        }

        Ok(())
    }

    async fn record_metric(
        &self,
        metric_name: &str,
        value: f64,
        unit: &str,
    ) -> Result<(), TestingError> {
        let mut state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {}", e)))
        })?;

        let metric = MetricValue {
            value,
            unit: unit.to_string(),
            timestamp: Instant::now(),
        };

        state.metrics.insert(metric_name.to_string(), metric);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_scenario_registration() {
        let handler = SimulationScenarioHandler::new(123);

        let scenario = ScenarioDefinition {
            id: "test_scenario".to_string(),
            name: "Test Scenario".to_string(),
            actions: vec![InjectionAction::ModifyParameter {
                key: "test_param".to_string(),
                value: "test_value".to_string(),
            }],
            trigger: TriggerCondition::Immediate,
            duration: Some(Duration::from_secs(10)),
            priority: 1,
        };

        let result = handler.register_scenario(scenario);
        assert!(result.is_ok());

        let stats = handler.get_injection_stats().unwrap();
        assert_eq!(stats.get("registered_scenarios"), Some(&"1".to_string()));
    }

    #[tokio::test]
    async fn test_scenario_triggering() {
        let handler = SimulationScenarioHandler::new(123);

        let scenario = ScenarioDefinition {
            id: "trigger_test".to_string(),
            name: "Trigger Test".to_string(),
            actions: vec![],
            trigger: TriggerCondition::Immediate,
            duration: Some(Duration::from_secs(10)),
            priority: 1,
        };

        handler.register_scenario(scenario).unwrap();

        let result = handler.trigger_scenario("trigger_test");
        assert!(result.is_ok());

        let stats = handler.get_injection_stats().unwrap();
        assert_eq!(stats.get("total_injections"), Some(&"1".to_string()));
    }

    #[tokio::test]
    async fn test_checkpoint_creation() {
        let handler = SimulationScenarioHandler::new(123);

        let result = handler
            .create_checkpoint("test_checkpoint", "Test checkpoint")
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_state_inspection() {
        let handler = SimulationScenarioHandler::new(123);

        let result = handler.inspect_state("scenarios", "count").await;
        assert!(result.is_ok());

        // Should return 0 scenarios
        let count = result.unwrap().downcast::<usize>().unwrap();
        assert_eq!(*count, 0);
    }

    #[tokio::test]
    async fn test_event_recording() {
        let handler = SimulationScenarioHandler::new(123);

        let mut event_data = HashMap::new();
        event_data.insert("key".to_string(), "value".to_string());

        let result = handler.record_event("test_event", event_data).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_metric_recording() {
        let handler = SimulationScenarioHandler::new(123);

        let result = handler.record_metric("test_metric", 42.0, "units").await;
        assert!(result.is_ok());

        // Verify metric was recorded
        let metric_result = handler.inspect_state("metrics", "test_metric").await;
        assert!(metric_result.is_ok());

        let metric_value = metric_result.unwrap().downcast::<f64>().unwrap();
        assert_eq!(*metric_value, 42.0);
    }

    #[tokio::test]
    async fn test_randomization_settings() {
        let handler = SimulationScenarioHandler::new(123);

        let result = handler.set_randomization(true, 0.5);
        assert!(result.is_ok());

        let stats = handler.get_injection_stats().unwrap();
        assert_eq!(
            stats.get("randomization_enabled"),
            Some(&"true".to_string())
        );
        assert_eq!(stats.get("injection_probability"), Some(&"0.5".to_string()));
    }

    #[tokio::test]
    async fn test_chat_group_creation() {
        let handler = SimulationScenarioHandler::new(123);

        let result = handler.create_chat_group(
            "Test Group",
            "alice",
            vec!["bob".to_string(), "charlie".to_string()],
        );
        assert!(result.is_ok());

        let group_id = result.unwrap();
        let stats = handler.get_chat_stats().unwrap();
        assert_eq!(stats.get("chat_groups"), Some(&"1".to_string()));

        // Test state inspection
        let group_count = handler.inspect_state("chat", "groups").await.unwrap();
        let count = group_count.downcast::<usize>().unwrap();
        assert_eq!(*count, 1);
    }

    #[tokio::test]
    async fn test_chat_messaging() {
        let handler = SimulationScenarioHandler::new(123);

        let group_id = handler
            .create_chat_group(
                "Test Group",
                "alice",
                vec!["bob".to_string(), "charlie".to_string()],
            )
            .unwrap();

        // Test sending messages
        let result1 = handler.send_chat_message(&group_id, "alice", "Hello everyone!");
        assert!(result1.is_ok());

        let result2 = handler.send_chat_message(&group_id, "bob", "Hi Alice!");
        assert!(result2.is_ok());

        let stats = handler.get_chat_stats().unwrap();
        assert_eq!(stats.get("total_messages"), Some(&"2".to_string()));

        // Test that non-members can't send messages
        let result_fail = handler.send_chat_message(&group_id, "dave", "I'm not a member");
        assert!(result_fail.is_err());
    }

    #[tokio::test]
    async fn test_data_loss_simulation() {
        let handler = SimulationScenarioHandler::new(123);

        let group_id = handler
            .create_chat_group("Test Group", "alice", vec!["bob".to_string()])
            .unwrap();

        // Send some messages before data loss
        handler
            .send_chat_message(&group_id, "alice", "Message 1")
            .unwrap();
        handler
            .send_chat_message(&group_id, "bob", "Message 2")
            .unwrap();

        // Simulate data loss for Bob
        let result = handler.simulate_data_loss("bob", "complete_device_loss", true);
        assert!(result.is_ok());

        let stats = handler.get_chat_stats().unwrap();
        assert_eq!(
            stats.get("participants_with_data_loss"),
            Some(&"1".to_string())
        );

        // Check state inspection for data loss
        let loss_count = handler.inspect_state("data_loss", "bob").await.unwrap();
        let count = loss_count.downcast::<usize>().unwrap();
        assert!(*count > 0); // Bob had messages before loss
    }

    #[tokio::test]
    async fn test_guardian_recovery() {
        let handler = SimulationScenarioHandler::new(123);

        // Initiate recovery process
        let result = handler.initiate_guardian_recovery(
            "bob",
            vec!["alice".to_string(), "charlie".to_string()],
            2,
        );
        assert!(result.is_ok());

        let stats = handler.get_chat_stats().unwrap();
        assert_eq!(stats.get("active_recoveries"), Some(&"1".to_string()));

        // Verify recovery completion
        let validation_result = handler.verify_recovery_success(
            "bob",
            vec![
                "keys_restored".to_string(),
                "account_accessible".to_string(),
            ],
        );
        assert!(validation_result.is_ok());
        assert_eq!(validation_result.unwrap(), true);

        // Check that recovery is now complete
        let recovery_complete = handler.inspect_state("recovery", "bob").await.unwrap();
        let is_complete = recovery_complete.downcast::<bool>().unwrap();
        assert_eq!(*is_complete, true);
    }

    #[tokio::test]
    async fn test_message_history_validation() {
        let handler = SimulationScenarioHandler::new(123);

        let group_id = handler
            .create_chat_group("Recovery Test", "alice", vec!["bob".to_string()])
            .unwrap();

        // Send messages before data loss
        handler
            .send_chat_message(&group_id, "alice", "Message 1")
            .unwrap();
        handler
            .send_chat_message(&group_id, "bob", "Message 2")
            .unwrap();
        handler
            .send_chat_message(&group_id, "alice", "Message 3")
            .unwrap();

        // Simulate data loss
        handler
            .simulate_data_loss("bob", "complete_device_loss", true)
            .unwrap();

        // Test message history validation
        let validation_result = handler.validate_message_history("bob", 2, true);
        assert!(validation_result.is_ok());
        assert_eq!(validation_result.unwrap(), true);

        // Test validation failure case
        let validation_fail = handler.validate_message_history("bob", 10, true);
        assert!(validation_fail.is_ok());
        assert_eq!(validation_fail.unwrap(), false);
    }

    #[tokio::test]
    async fn test_insufficient_guardians_error() {
        let handler = SimulationScenarioHandler::new(123);

        // Try to initiate recovery with insufficient guardians
        let result = handler.initiate_guardian_recovery(
            "bob",
            vec!["alice".to_string()], // Only 1 guardian
            2,                         // But need 2
        );
        assert!(result.is_err());
    }
}
