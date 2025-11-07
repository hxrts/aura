//! Fault simulation middleware for injecting various types of faults during simulation

use super::{
    ByzantineStrategy, FaultType, Result, SimulatorContext, SimulatorHandler, SimulatorMiddleware,
    SimulatorOperation,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Middleware for simulating various types of faults in the system
pub struct FaultSimulationMiddleware {
    /// Active faults being simulated
    active_faults: HashMap<String, ActiveFault>,
    /// Fault injection rules
    injection_rules: Vec<FaultInjectionRule>,
    /// Enable automatic fault injection
    auto_injection: bool,
    /// Fault injection probability per tick
    injection_probability: f64,
    /// Maximum concurrent faults
    max_concurrent_faults: usize,
    /// Fault recovery settings
    recovery_settings: FaultRecoverySettings,
}

impl FaultSimulationMiddleware {
    /// Create new fault simulation middleware
    pub fn new() -> Self {
        Self {
            active_faults: HashMap::new(),
            injection_rules: Vec::new(),
            auto_injection: false,
            injection_probability: 0.05,
            max_concurrent_faults: 5,
            recovery_settings: FaultRecoverySettings::default(),
        }
    }

    /// Enable automatic fault injection
    pub fn with_auto_injection(mut self, enable: bool, probability: f64) -> Self {
        self.auto_injection = enable;
        self.injection_probability = probability.clamp(0.0, 1.0);
        self
    }

    /// Add fault injection rule
    pub fn with_injection_rule(mut self, rule: FaultInjectionRule) -> Self {
        self.injection_rules.push(rule);
        self
    }

    /// Set maximum concurrent faults
    pub fn with_max_concurrent_faults(mut self, max: usize) -> Self {
        self.max_concurrent_faults = max;
        self
    }

    /// Set fault recovery settings
    pub fn with_recovery_settings(mut self, settings: FaultRecoverySettings) -> Self {
        self.recovery_settings = settings;
        self
    }

    /// Check if automatic fault injection should occur
    fn should_auto_inject(&self, context: &SimulatorContext) -> bool {
        if !self.auto_injection {
            return false;
        }

        if self.active_faults.len() >= self.max_concurrent_faults {
            return false;
        }

        // Use deterministic randomness
        let mut seed = context.seed.wrapping_add(context.tick * 3);
        seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        let random_value = (seed >> 16) as f64 / u16::MAX as f64;

        random_value < self.injection_probability
    }

    /// Generate random fault for auto-injection
    fn generate_random_fault(&self, context: &SimulatorContext) -> FaultType {
        let mut seed = context.seed.wrapping_add(context.tick * 7);
        seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        let fault_index = (seed as usize) % 6;

        match fault_index {
            0 => FaultType::MessageDrop { probability: 0.1 },
            1 => FaultType::MessageDelay {
                delay: Duration::from_millis(100),
            },
            2 => FaultType::MessageCorruption { probability: 0.05 },
            3 => FaultType::Byzantine {
                strategy: ByzantineStrategy::RandomMessages,
            },
            4 => FaultType::NetworkPartition {
                participants: vec!["node_1".to_string(), "node_2".to_string()],
                duration: Duration::from_secs(10),
            },
            _ => FaultType::NodeCrash {
                node_id: "random_node".to_string(),
                duration: Some(Duration::from_secs(5)),
            },
        }
    }

    /// Process fault injection operation
    fn inject_fault(
        &mut self,
        fault_type: FaultType,
        target: String,
        duration: Option<Duration>,
        context: &SimulatorContext,
    ) -> Result<Value> {
        let fault_id = format!("fault_{}_{}", target, context.tick);

        let active_fault = ActiveFault {
            id: fault_id.clone(),
            fault_type: fault_type.clone(),
            target: target.clone(),
            start_tick: context.tick,
            duration,
            injected_at: Instant::now(),
            effects: HashMap::new(),
        };

        self.active_faults.insert(fault_id.clone(), active_fault);

        Ok(json!({
            "fault_id": fault_id,
            "fault_type": format!("{:?}", fault_type),
            "target": target,
            "start_tick": context.tick,
            "duration_ms": duration.map(|d| d.as_millis()),
            "status": "injected"
        }))
    }

    /// Update active faults and remove expired ones
    fn update_active_faults(&mut self, context: &SimulatorContext) -> Vec<String> {
        let mut removed_faults = Vec::new();
        let enable_auto_recovery = self.recovery_settings.enable_auto_recovery;
        let recovery_settings = self.recovery_settings.clone();

        // Collect fault IDs to remove
        let mut to_remove = Vec::new();

        for (fault_id, fault) in &self.active_faults {
            // Check if fault has expired
            if let Some(duration) = fault.duration {
                let elapsed_ticks = context.tick - fault.start_tick;
                let elapsed_time = Duration::from_millis(elapsed_ticks * 100); // Assume 100ms per tick

                if elapsed_time >= duration {
                    removed_faults.push(fault_id.clone());
                    to_remove.push(fault_id.clone());
                    continue;
                }
            }

            // Check recovery conditions
            if enable_auto_recovery {
                // Inline recovery check to avoid borrow conflict
                let elapsed_ticks = context.tick - fault.start_tick;
                let should_recover = elapsed_ticks >= recovery_settings.min_recovery_ticks;

                if should_recover {
                    removed_faults.push(fault_id.clone());
                    to_remove.push(fault_id.clone());
                }
            }
        }

        // Remove faults that need to be removed
        for fault_id in to_remove {
            self.active_faults.remove(&fault_id);
        }

        removed_faults
    }

    /// Check if a fault should be recovered
    fn should_recover_fault(&self, fault: &ActiveFault, context: &SimulatorContext) -> bool {
        match &fault.fault_type {
            FaultType::NodeCrash { .. } => {
                // Recovery based on tick count
                let elapsed_ticks = context.tick - fault.start_tick;
                elapsed_ticks >= self.recovery_settings.min_recovery_ticks
            }

            FaultType::NetworkPartition { .. } => {
                // Network partitions auto-recover after some time
                let elapsed_ticks = context.tick - fault.start_tick;
                elapsed_ticks >= 50 // 5 seconds at 100ms/tick
            }

            _ => false, // Other faults don't auto-recover
        }
    }

    /// Apply fault effects to operation
    fn apply_fault_effects(
        &self,
        _operation: &SimulatorOperation,
        _context: &SimulatorContext,
    ) -> Result<Value> {
        let mut effects = HashMap::new();

        for (fault_id, fault) in &self.active_faults {
            match &fault.fault_type {
                FaultType::MessageDrop { probability } => {
                    effects.insert(
                        fault_id.clone(),
                        json!({
                            "type": "message_drop",
                            "probability": probability,
                            "target": fault.target
                        }),
                    );
                }

                FaultType::MessageDelay { delay } => {
                    effects.insert(
                        fault_id.clone(),
                        json!({
                            "type": "message_delay",
                            "delay_ms": delay.as_millis(),
                            "target": fault.target
                        }),
                    );
                }

                FaultType::MessageCorruption { probability } => {
                    effects.insert(
                        fault_id.clone(),
                        json!({
                            "type": "message_corruption",
                            "probability": probability,
                            "target": fault.target
                        }),
                    );
                }

                FaultType::Byzantine { strategy } => {
                    effects.insert(
                        fault_id.clone(),
                        json!({
                            "type": "byzantine",
                            "strategy": format!("{:?}", strategy),
                            "target": fault.target
                        }),
                    );
                }

                FaultType::NetworkPartition {
                    participants,
                    duration,
                } => {
                    effects.insert(
                        fault_id.clone(),
                        json!({
                            "type": "network_partition",
                            "participants": participants,
                            "duration_ms": duration.as_millis(),
                            "active": true
                        }),
                    );
                }

                FaultType::NodeCrash { node_id, duration } => {
                    effects.insert(
                        fault_id.clone(),
                        json!({
                            "type": "node_crash",
                            "node_id": node_id,
                            "duration_ms": duration.map(|d| d.as_millis()),
                            "crashed": true
                        }),
                    );
                }

                FaultType::ResourceExhaustion { resource, factor } => {
                    effects.insert(
                        fault_id.clone(),
                        json!({
                            "type": "resource_exhaustion",
                            "resource": resource,
                            "factor": factor,
                            "target": fault.target
                        }),
                    );
                }
            }
        }

        Ok(json!({
            "active_fault_count": self.active_faults.len(),
            "fault_effects": effects
        }))
    }
}

impl Default for FaultSimulationMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl SimulatorMiddleware for FaultSimulationMiddleware {
    fn process(
        &self,
        operation: SimulatorOperation,
        context: &SimulatorContext,
        next: &dyn SimulatorHandler,
    ) -> Result<Value> {
        match &operation {
            SimulatorOperation::InjectFault {
                fault_type,
                target,
                duration,
            } => {
                // Handle explicit fault injection
                let injection_result = json!({
                    "fault_type": format!("{:?}", fault_type),
                    "target": target,
                    "duration_ms": duration.map(|d| d.as_millis()),
                    "tick": context.tick,
                    "status": "injected"
                });

                // Add fault injection info to context
                let mut enhanced_context = context.clone();
                enhanced_context
                    .metadata
                    .insert("fault_injected".to_string(), target.clone());

                // Call next handler
                let mut result = next.handle(operation, &enhanced_context)?;

                // Add injection results
                if let Some(obj) = result.as_object_mut() {
                    obj.insert("fault_injection".to_string(), injection_result);
                }

                Ok(result)
            }

            SimulatorOperation::ExecuteTick { .. } => {
                // Check for automatic fault injection
                let should_inject = self.should_auto_inject(context);
                let fault_effects = self.apply_fault_effects(&operation, context)?;

                let mut enhanced_context = context.clone();
                enhanced_context.metadata.insert(
                    "active_fault_count".to_string(),
                    self.active_faults.len().to_string(),
                );

                if should_inject {
                    let random_fault = self.generate_random_fault(context);
                    enhanced_context.metadata.insert(
                        "auto_fault_injected".to_string(),
                        format!("{:?}", random_fault),
                    );
                }

                // Call next handler
                let mut result = next.handle(operation, &enhanced_context)?;

                // Add fault effects to result
                if let Some(obj) = result.as_object_mut() {
                    obj.insert("fault_simulation".to_string(), fault_effects);
                }

                Ok(result)
            }

            _ => {
                // For other operations, apply fault effects if relevant
                let fault_effects = self.apply_fault_effects(&operation, context)?;

                let mut enhanced_context = context.clone();
                if !self.active_faults.is_empty() {
                    enhanced_context
                        .metadata
                        .insert("faults_active".to_string(), "true".to_string());
                }

                let mut result = next.handle(operation, &enhanced_context)?;

                // Add fault effects if there are active faults
                if !self.active_faults.is_empty() {
                    if let Some(obj) = result.as_object_mut() {
                        obj.insert("fault_effects".to_string(), fault_effects);
                    }
                }

                Ok(result)
            }
        }
    }

    fn name(&self) -> &str {
        "fault_simulation"
    }
}

/// Active fault tracking
#[derive(Debug, Clone)]
struct ActiveFault {
    id: String,
    fault_type: FaultType,
    target: String,
    start_tick: u64,
    duration: Option<Duration>,
    injected_at: Instant,
    effects: HashMap<String, Value>,
}

/// Fault injection rules
#[derive(Debug, Clone)]
pub struct FaultInjectionRule {
    /// Rule identifier
    pub id: String,
    /// Fault type to inject
    pub fault_type: FaultType,
    /// Target for fault injection
    pub target: String,
    /// Conditions for triggering the rule
    pub conditions: Vec<FaultCondition>,
    /// Duration of the fault
    pub duration: Option<Duration>,
    /// Maximum number of times this rule can trigger
    pub max_triggers: Option<usize>,
    /// Current trigger count
    pub trigger_count: usize,
}

/// Conditions for fault injection
#[derive(Debug, Clone)]
pub enum FaultCondition {
    /// Trigger after specific tick
    TickCount { min_tick: u64 },
    /// Trigger based on participant count
    ParticipantCount { count: usize },
    /// Trigger based on protocol state
    ProtocolState { protocol: String, state: String },
    /// Trigger randomly
    Random { probability: f64 },
}

/// Fault recovery settings
#[derive(Debug, Clone)]
pub struct FaultRecoverySettings {
    /// Enable automatic fault recovery
    pub enable_auto_recovery: bool,
    /// Minimum ticks before recovery
    pub min_recovery_ticks: u64,
    /// Recovery probability per tick
    pub recovery_probability: f64,
    /// Enable recovery based on system state
    pub enable_state_based_recovery: bool,
}

impl Default for FaultRecoverySettings {
    fn default() -> Self {
        Self {
            enable_auto_recovery: true,
            min_recovery_ticks: 20,
            recovery_probability: 0.1,
            enable_state_based_recovery: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::handler::NoOpSimulatorHandler;

    #[test]
    fn test_fault_simulation_creation() {
        let middleware = FaultSimulationMiddleware::new()
            .with_auto_injection(true, 0.1)
            .with_max_concurrent_faults(3);

        assert!(middleware.auto_injection);
        assert_eq!(middleware.injection_probability, 0.1);
        assert_eq!(middleware.max_concurrent_faults, 3);
    }

    #[test]
    fn test_fault_injection_handling() {
        let middleware = FaultSimulationMiddleware::new();
        let handler = NoOpSimulatorHandler;
        let context = SimulatorContext::new("test".to_string(), "run1".to_string());

        let result = middleware.process(
            SimulatorOperation::InjectFault {
                fault_type: FaultType::MessageDrop { probability: 0.5 },
                target: "node_1".to_string(),
                duration: Some(Duration::from_secs(10)),
            },
            &context,
            &handler,
        );

        assert!(result.is_ok());
        let value = result.unwrap();
        assert!(value.get("fault_injection").is_some());
    }

    #[test]
    fn test_fault_recovery_settings() {
        let settings = FaultRecoverySettings {
            enable_auto_recovery: true,
            min_recovery_ticks: 50,
            recovery_probability: 0.2,
            enable_state_based_recovery: true,
        };

        assert!(settings.enable_auto_recovery);
        assert_eq!(settings.min_recovery_ticks, 50);
        assert_eq!(settings.recovery_probability, 0.2);
        assert!(settings.enable_state_based_recovery);
    }

    #[test]
    fn test_fault_injection_rule() {
        let rule = FaultInjectionRule {
            id: "test_rule".to_string(),
            fault_type: FaultType::NetworkPartition {
                participants: vec!["node_1".to_string(), "node_2".to_string()],
                duration: Duration::from_secs(30),
            },
            target: "network".to_string(),
            conditions: vec![FaultCondition::TickCount { min_tick: 100 }],
            duration: Some(Duration::from_secs(30)),
            max_triggers: Some(1),
            trigger_count: 0,
        };

        assert_eq!(rule.id, "test_rule");
        assert_eq!(rule.trigger_count, 0);
        assert_eq!(rule.max_triggers, Some(1));
    }
}
