//! Chaos coordination middleware for orchestrating complex chaos engineering scenarios

use super::{
    ByzantineStrategy, ChaosStrategy, FaultType, Result, SimulatorContext, SimulatorError,
    SimulatorHandler, SimulatorMiddleware, SimulatorOperation,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Middleware for coordinating chaos engineering scenarios
pub struct ChaosCoordinationMiddleware {
    /// Shared state with interior mutability
    state: Arc<Mutex<ChaosCoordinationState>>,
}

/// Internal state for chaos coordination with thread-safe access
#[derive(Debug)]
struct ChaosCoordinationState {
    /// Active chaos scenarios
    active_scenarios: HashMap<String, ChaosScenario>,
    /// Chaos strategy templates
    strategy_templates: HashMap<String, ChaosStrategyTemplate>,
    /// Chaos coordination rules
    coordination_rules: Vec<ChaosRule>,
    /// Enable adaptive chaos intensity
    adaptive_intensity: bool,
    /// Chaos intensity factor (0.0 to 1.0)
    intensity_factor: f64,
    /// Maximum concurrent chaos scenarios
    max_concurrent_scenarios: usize,
    /// Chaos recovery settings
    recovery_settings: ChaosRecoverySettings,
}

impl ChaosCoordinationMiddleware {
    /// Create new chaos coordination middleware
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(ChaosCoordinationState {
                active_scenarios: HashMap::new(),
                strategy_templates: HashMap::new(),
                coordination_rules: Vec::new(),
                adaptive_intensity: false,
                intensity_factor: 0.3,
                max_concurrent_scenarios: 3,
                recovery_settings: ChaosRecoverySettings::default(),
            })),
        }
    }

    /// Enable adaptive chaos intensity
    pub fn with_adaptive_intensity(self, enable: bool, base_intensity: f64) -> Self {
        if let Ok(mut state) = self.state.lock() {
            state.adaptive_intensity = enable;
            state.intensity_factor = base_intensity.clamp(0.0, 1.0);
        }
        self
    }

    /// Set maximum concurrent chaos scenarios
    pub fn with_max_concurrent_scenarios(self, max: usize) -> Self {
        if let Ok(mut state) = self.state.lock() {
            state.max_concurrent_scenarios = max;
        }
        self
    }

    /// Add chaos strategy template
    pub fn with_strategy_template(self, template: ChaosStrategyTemplate) -> Self {
        if let Ok(mut state) = self.state.lock() {
            state
                .strategy_templates
                .insert(template.id.clone(), template);
        }
        self
    }

    /// Add chaos coordination rule
    pub fn with_coordination_rule(self, rule: ChaosRule) -> Self {
        if let Ok(mut state) = self.state.lock() {
            state.coordination_rules.push(rule);
        }
        self
    }

    /// Set chaos recovery settings
    pub fn with_recovery_settings(self, settings: ChaosRecoverySettings) -> Self {
        if let Ok(mut state) = self.state.lock() {
            state.recovery_settings = settings;
        }
        self
    }

    /// Coordinate chaos testing
    fn coordinate_chaos(
        &self,
        strategy: ChaosStrategy,
        intensity: f64,
        duration: Duration,
        context: &SimulatorContext,
    ) -> Result<Value> {
        let scenario_id = format!("chaos_{}_{}", context.tick, context.timestamp.as_millis());

        if let Ok(mut state) = self.state.lock() {
            // Apply intensity factor
            let effective_intensity = if state.adaptive_intensity {
                self.calculate_adaptive_intensity(intensity, context, &state)
            } else {
                (intensity * state.intensity_factor).clamp(0.0, 1.0)
            };

            // Create chaos scenario
            let scenario = ChaosScenario {
                _id: scenario_id.clone(),
                _strategy: strategy.clone(),
                _intensity: effective_intensity,
                duration,
                start_tick: context.tick,
                actions_performed: Vec::new(),
                _active_faults: HashMap::new(),
                _created_at: Instant::now(),
            };

            // Check if we can add more scenarios
            if state.active_scenarios.len() >= state.max_concurrent_scenarios {
                return Err(SimulatorError::ChaosCoordinationError(
                    "Maximum concurrent chaos scenarios reached".to_string(),
                ));
            }

            // Generate chaos actions based on strategy
            let actions =
                self.generate_chaos_actions(&strategy, effective_intensity, context, &state)?;

            state.active_scenarios.insert(scenario_id.clone(), scenario);

            Ok(json!({
                "scenario_id": scenario_id,
                "strategy": format!("{:?}", strategy),
                "requested_intensity": intensity,
                "effective_intensity": effective_intensity,
                "duration_ms": duration.as_millis(),
                "actions_planned": actions.len(),
                "start_tick": context.tick,
                "status": "coordinated"
            }))
        } else {
            Err(SimulatorError::OperationFailed(
                "Failed to acquire chaos coordination lock".to_string(),
            ))
        }
    }

    /// Calculate adaptive chaos intensity based on simulation state
    fn calculate_adaptive_intensity(
        &self,
        base_intensity: f64,
        context: &SimulatorContext,
        state: &ChaosCoordinationState,
    ) -> f64 {
        let mut adaptive_factor = 1.0;

        // Reduce intensity if there are already many active scenarios
        if state.active_scenarios.len() > 1 {
            adaptive_factor *= 0.7;
        }

        // Adjust based on simulation progress
        let progress_factor = (context.tick as f64 / 1000.0).min(1.0);
        adaptive_factor *= 0.5 + (progress_factor * 0.5);

        // Adjust based on participant count
        if context.participant_count < context.threshold * 2 {
            adaptive_factor *= 0.8; // Reduce intensity with fewer participants
        }

        (base_intensity * state.intensity_factor * adaptive_factor).clamp(0.0, 1.0)
    }

    /// Generate chaos actions based on strategy
    fn generate_chaos_actions(
        &self,
        strategy: &ChaosStrategy,
        intensity: f64,
        context: &SimulatorContext,
        _state: &ChaosCoordinationState,
    ) -> Result<Vec<ChaosAction>> {
        let mut actions = Vec::new();

        match strategy {
            ChaosStrategy::RandomFaults => {
                let fault_count = ((intensity * 5.0) as usize).max(1);

                for i in 0..fault_count {
                    let fault_type = self.generate_random_fault(i, context);
                    actions.push(ChaosAction::InjectFault {
                        fault_type,
                        target: format!("participant_{}", i % context.participant_count),
                        delay_ticks: (i as u64) * 2,
                    });
                }
            }

            ChaosStrategy::NetworkPartitions => {
                let partition_count = ((intensity * 3.0) as usize).max(1);

                for i in 0..partition_count {
                    let participants = self.select_partition_participants(i, context);
                    actions.push(ChaosAction::CreateNetworkPartition {
                        participants,
                        duration: Duration::from_secs((10.0 * intensity) as u64),
                        delay_ticks: (i as u64) * 5,
                    });
                }
            }

            ChaosStrategy::ResourceExhaustion => {
                let resources = ["memory", "cpu", "disk", "network"];
                let resource_count = ((intensity * resources.len() as f64) as usize).max(1);

                for i in 0..resource_count {
                    let resource = resources[i % resources.len()].to_string();
                    actions.push(ChaosAction::ExhaustResource {
                        resource,
                        factor: 0.8 + (intensity * 0.2),
                        delay_ticks: (i as u64) * 3,
                    });
                }
            }

            ChaosStrategy::ByzantineBehavior => {
                let byzantine_count =
                    ((intensity * context.participant_count as f64 * 0.3) as usize).max(1);

                for i in 0..byzantine_count {
                    let strategy = self.select_byzantine_strategy(i);
                    actions.push(ChaosAction::InjectByzantine {
                        participant: format!("participant_{}", i),
                        strategy,
                        delay_ticks: (i as u64) * 4,
                    });
                }
            }

            ChaosStrategy::Combined { strategies } => {
                for sub_strategy in strategies {
                    let sub_intensity = intensity / strategies.len() as f64;
                    let sub_actions =
                        self.generate_chaos_actions(sub_strategy, sub_intensity, context, _state)?;
                    actions.extend(sub_actions);
                }
            }
        }

        Ok(actions)
    }

    /// Generate random fault for chaos
    fn generate_random_fault(&self, seed: usize, context: &SimulatorContext) -> FaultType {
        let fault_types = [
            FaultType::MessageDrop { probability: 0.2 },
            FaultType::MessageDelay {
                delay: Duration::from_millis(200),
            },
            FaultType::MessageCorruption { probability: 0.1 },
            FaultType::NodeCrash {
                node_id: format!("node_{}", seed),
                duration: Some(Duration::from_secs(5)),
            },
        ];

        let index = (seed + context.tick as usize) % fault_types.len();
        fault_types[index].clone()
    }

    /// Select participants for network partition
    fn select_partition_participants(
        &self,
        partition_index: usize,
        context: &SimulatorContext,
    ) -> Vec<String> {
        let mut participants = Vec::new();
        let partition_size = (context.participant_count / 2).max(1);

        for i in 0..partition_size {
            let participant_index =
                (partition_index * partition_size + i) % context.participant_count;
            participants.push(format!("participant_{}", participant_index));
        }

        participants
    }

    /// Select Byzantine strategy
    fn select_byzantine_strategy(&self, seed: usize) -> ByzantineStrategy {
        let strategies = [
            ByzantineStrategy::RandomMessages,
            ByzantineStrategy::DuplicateMessages,
            ByzantineStrategy::DelayedMessages {
                delay: Duration::from_millis(500),
            },
            ByzantineStrategy::CorruptedSignatures,
        ];

        strategies[seed % strategies.len()].clone()
    }

    /// Process active chaos scenarios
    fn process_active_scenarios(&self, context: &SimulatorContext) -> Vec<ChaosEvent> {
        let mut events = Vec::new();

        if let Ok(mut state) = self.state.lock() {
            let enable_auto_recovery = state.recovery_settings.enable_auto_recovery;
            let recovery_settings = state.recovery_settings.clone();

            // Collect scenario IDs to remove
            let mut to_remove = Vec::new();

            for (scenario_id, scenario) in &state.active_scenarios {
                // Check if scenario has expired
                let elapsed_ticks = context.tick - scenario.start_tick;
                let elapsed_time = Duration::from_millis(elapsed_ticks * 100); // Assume 100ms per tick

                if elapsed_time >= scenario.duration {
                    events.push(ChaosEvent {
                        _scenario_id: scenario_id.clone(),
                        _event_type: ChaosEventType::ScenarioCompleted,
                        _timestamp: context.timestamp,
                        _tick: context.tick,
                        _details: json!({
                            "duration_ms": elapsed_time.as_millis(),
                            "actions_performed": scenario.actions_performed.len()
                        }),
                    });
                    to_remove.push(scenario_id.clone());
                    continue;
                }

                // Check for recovery conditions
                if enable_auto_recovery {
                    // Inline recovery check to avoid borrow conflict
                    let should_recover = elapsed_ticks >= recovery_settings.min_recovery_ticks;

                    if should_recover {
                        events.push(ChaosEvent {
                            _scenario_id: scenario_id.clone(),
                            _event_type: ChaosEventType::ScenarioRecovered,
                            _timestamp: context.timestamp,
                            _tick: context.tick,
                            _details: json!({
                                "recovery_reason": "auto_recovery",
                                "elapsed_ticks": elapsed_ticks
                            }),
                        });
                        to_remove.push(scenario_id.clone());
                    }
                }
            }

            // Remove scenarios that need to be removed
            for scenario_id in to_remove {
                state.active_scenarios.remove(&scenario_id);
            }
        }

        events
    }

    /// Check if scenario should be recovered
    fn _should_recover_scenario(
        &self,
        scenario: &ChaosScenario,
        context: &SimulatorContext,
        recovery_settings: &ChaosRecoverySettings,
    ) -> bool {
        let elapsed_ticks = context.tick - scenario.start_tick;

        // Auto-recover based on elapsed time
        if elapsed_ticks >= recovery_settings.min_recovery_ticks {
            // Use deterministic randomness for recovery decision
            let mut seed = context.seed.wrapping_add(scenario.start_tick);
            seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
            let random_value = (seed >> 16) as f64 / u16::MAX as f64;

            return random_value < recovery_settings.recovery_probability;
        }

        false
    }

    /// Apply coordination rules
    fn apply_coordination_rules(
        &self,
        context: &SimulatorContext,
        state: &ChaosCoordinationState,
    ) -> Vec<ChaosRuleAction> {
        let mut actions = Vec::new();

        for rule in &state.coordination_rules {
            if Self::evaluate_rule_condition(&rule.condition, context, state) {
                actions.push(rule.action.clone());
            }
        }

        actions
    }

    /// Evaluate rule condition
    fn evaluate_rule_condition(
        condition: &ChaosRuleCondition,
        context: &SimulatorContext,
        state: &ChaosCoordinationState,
    ) -> bool {
        match condition {
            ChaosRuleCondition::TickCount { min_tick } => context.tick >= *min_tick,
            ChaosRuleCondition::ActiveScenarios { max_count } => {
                state.active_scenarios.len() <= *max_count
            }
            ChaosRuleCondition::ParticipantThreshold { min_participants } => {
                context.participant_count >= *min_participants
            }
            ChaosRuleCondition::IntensityLevel { max_intensity } => {
                state.intensity_factor <= *max_intensity
            }
            ChaosRuleCondition::Combined {
                conditions,
                operator,
            } => match operator {
                ChaosRuleOperator::And => conditions
                    .iter()
                    .all(|c| Self::evaluate_rule_condition(c, context, state)),
                ChaosRuleOperator::Or => conditions
                    .iter()
                    .any(|c| Self::evaluate_rule_condition(c, context, state)),
            },
        }
    }
}

impl Default for ChaosCoordinationMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl SimulatorMiddleware for ChaosCoordinationMiddleware {
    fn process(
        &self,
        operation: SimulatorOperation,
        context: &SimulatorContext,
        next: &dyn SimulatorHandler,
    ) -> Result<Value> {
        match &operation {
            SimulatorOperation::CoordinateChaos {
                strategy,
                intensity,
                duration,
            } => {
                // Handle chaos coordination request with real implementation
                let coordination_result =
                    self.coordinate_chaos(strategy.clone(), *intensity, *duration, context)?;

                // Add chaos coordination info to context
                let mut enhanced_context = context.clone();
                enhanced_context
                    .metadata
                    .insert("chaos_coordinated".to_string(), format!("{:?}", strategy));
                enhanced_context
                    .metadata
                    .insert("chaos_intensity".to_string(), intensity.to_string());

                // Call next handler
                let mut result = next.handle(operation, &enhanced_context)?;

                // Add coordination results
                if let Some(obj) = result.as_object_mut() {
                    obj.insert("chaos_coordination".to_string(), coordination_result);
                }

                Ok(result)
            }

            SimulatorOperation::ExecuteTick { .. } => {
                // Process active chaos scenarios with real implementation
                let chaos_events = self.process_active_scenarios(context);

                // Add chaos coordination info to context
                let mut enhanced_context = context.clone();

                if let Ok(state) = self.state.lock() {
                    let rule_actions = self.apply_coordination_rules(context, &state);

                    enhanced_context.metadata.insert(
                        "active_chaos_scenarios".to_string(),
                        state.active_scenarios.len().to_string(),
                    );
                    enhanced_context.metadata.insert(
                        "chaos_intensity_factor".to_string(),
                        state.intensity_factor.to_string(),
                    );
                    enhanced_context
                        .metadata
                        .insert("chaos_events".to_string(), chaos_events.len().to_string());

                    // Call next handler
                    let mut result = next.handle(operation, &enhanced_context)?;

                    // Add chaos coordination information
                    if let Some(obj) = result.as_object_mut() {
                        obj.insert(
                            "chaos_coordination".to_string(),
                            json!({
                                "active_scenarios": state.active_scenarios.len(),
                                "intensity_factor": state.intensity_factor,
                                "adaptive_intensity": state.adaptive_intensity,
                                "max_concurrent": state.max_concurrent_scenarios,
                                "events_processed": chaos_events.len(),
                                "rules_triggered": rule_actions.len(),
                                "strategy_templates": state.strategy_templates.len()
                            }),
                        );
                    }

                    Ok(result)
                } else {
                    // Fallback if lock fails
                    next.handle(operation, &enhanced_context)
                }
            }

            _ => {
                // For other operations, just add chaos coordination metadata
                let mut enhanced_context = context.clone();
                enhanced_context.metadata.insert(
                    "chaos_coordination_available".to_string(),
                    "true".to_string(),
                );

                next.handle(operation, &enhanced_context)
            }
        }
    }

    fn name(&self) -> &str {
        "chaos_coordination"
    }
}

/// Chaos scenario tracking
#[derive(Debug, Clone)]
struct ChaosScenario {
    _id: String,
    _strategy: ChaosStrategy,
    _intensity: f64,
    duration: Duration,
    start_tick: u64,
    actions_performed: Vec<ChaosAction>,
    _active_faults: HashMap<String, FaultType>,
    _created_at: Instant,
}

/// Chaos strategy template
#[derive(Debug, Clone)]
pub struct ChaosStrategyTemplate {
    /// Template identifier
    pub id: String,
    /// Template name
    pub name: String,
    /// Template description
    pub description: String,
    /// Base strategy
    pub strategy: ChaosStrategy,
    /// Default intensity
    pub default_intensity: f64,
    /// Default duration
    pub default_duration: Duration,
    /// Template parameters
    pub parameters: HashMap<String, Value>,
}

/// Chaos coordination rules
#[derive(Debug, Clone)]
pub struct ChaosRule {
    /// Rule identifier
    pub id: String,
    /// Rule condition
    pub condition: ChaosRuleCondition,
    /// Action to take when condition is met
    pub action: ChaosRuleAction,
    /// Whether the rule is active
    pub active: bool,
}

/// Conditions for chaos rules
#[derive(Debug, Clone)]
pub enum ChaosRuleCondition {
    /// Minimum tick count reached
    TickCount { min_tick: u64 },
    /// Maximum active scenarios
    ActiveScenarios { max_count: usize },
    /// Minimum participant threshold
    ParticipantThreshold { min_participants: usize },
    /// Maximum intensity level
    IntensityLevel { max_intensity: f64 },
    /// Combined conditions
    Combined {
        conditions: Vec<ChaosRuleCondition>,
        operator: ChaosRuleOperator,
    },
}

/// Logical operators for combining conditions
#[derive(Debug, Clone)]
pub enum ChaosRuleOperator {
    And,
    Or,
}

/// Actions that chaos rules can trigger
#[derive(Debug, Clone)]
pub enum ChaosRuleAction {
    /// Adjust intensity factor
    AdjustIntensity { factor: f64 },
    /// Stop all chaos scenarios
    StopAllScenarios,
    /// Start specific scenario
    StartScenario { template_id: String },
    /// Send notification
    Notify { message: String },
}

/// Actions that can be part of chaos scenarios
#[derive(Debug, Clone)]
pub enum ChaosAction {
    /// Inject a fault
    InjectFault {
        fault_type: FaultType,
        target: String,
        delay_ticks: u64,
    },
    /// Create network partition
    CreateNetworkPartition {
        participants: Vec<String>,
        duration: Duration,
        delay_ticks: u64,
    },
    /// Exhaust system resource
    ExhaustResource {
        resource: String,
        factor: f64,
        delay_ticks: u64,
    },
    /// Inject Byzantine behavior
    InjectByzantine {
        participant: String,
        strategy: ByzantineStrategy,
        delay_ticks: u64,
    },
}

/// Events generated by chaos coordination
#[derive(Debug, Clone)]
struct ChaosEvent {
    _scenario_id: String,
    _event_type: ChaosEventType,
    _timestamp: Duration,
    _tick: u64,
    _details: Value,
}

/// Types of chaos events
#[derive(Debug, Clone)]
enum ChaosEventType {
    _ScenarioStarted,
    ScenarioCompleted,
    ScenarioRecovered,
    _ActionExecuted,
    _FaultInjected,
    _FaultRecovered,
}

/// Chaos recovery settings
#[derive(Debug, Clone)]
pub struct ChaosRecoverySettings {
    /// Enable automatic chaos recovery
    pub enable_auto_recovery: bool,
    /// Minimum ticks before recovery
    pub min_recovery_ticks: u64,
    /// Recovery probability per tick
    pub recovery_probability: f64,
    /// Enable coordinated recovery
    pub enable_coordinated_recovery: bool,
}

impl Default for ChaosRecoverySettings {
    fn default() -> Self {
        Self {
            enable_auto_recovery: true,
            min_recovery_ticks: 50,
            recovery_probability: 0.05,
            enable_coordinated_recovery: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::handler::NoOpSimulatorHandler;

    #[test]
    fn test_chaos_coordination_creation() {
        let template = ChaosStrategyTemplate {
            id: "test_template".to_string(),
            name: "Test Template".to_string(),
            description: "Test chaos template".to_string(),
            strategy: ChaosStrategy::RandomFaults,
            default_intensity: 0.5,
            default_duration: Duration::from_secs(30),
            parameters: HashMap::new(),
        };

        let middleware = ChaosCoordinationMiddleware::new()
            .with_adaptive_intensity(true, 0.4)
            .with_max_concurrent_scenarios(2)
            .with_strategy_template(template);

        if let Ok(state) = middleware.state.lock() {
            assert!(state.adaptive_intensity);
            assert_eq!(state.intensity_factor, 0.4);
            assert_eq!(state.max_concurrent_scenarios, 2);
            assert_eq!(state.strategy_templates.len(), 1);
        };
    }

    #[test]
    fn test_chaos_coordination_operation() {
        let middleware = ChaosCoordinationMiddleware::new();
        let handler = NoOpSimulatorHandler;
        let context =
            SimulatorContext::new("test".to_string(), "run1".to_string()).with_participants(5, 3);

        let result = middleware.process(
            SimulatorOperation::CoordinateChaos {
                strategy: ChaosStrategy::RandomFaults,
                intensity: 0.5,
                duration: Duration::from_secs(30),
            },
            &context,
            &handler,
        );

        assert!(result.is_ok());
        let value = result.unwrap();
        assert!(value.get("chaos_coordination").is_some());
    }

    #[test]
    fn test_adaptive_intensity_calculation() {
        let middleware = ChaosCoordinationMiddleware::new().with_adaptive_intensity(true, 0.5);

        let context =
            SimulatorContext::new("test".to_string(), "run1".to_string()).with_participants(10, 5);

        if let Ok(state) = middleware.state.lock() {
            let adaptive_intensity = middleware.calculate_adaptive_intensity(1.0, &context, &state);
            // Should be reduced from 1.0 due to adaptive factors
            assert!(adaptive_intensity < 1.0);
            assert!(adaptive_intensity > 0.0);
        };
    }

    #[test]
    fn test_chaos_rule_evaluation() {
        let middleware = ChaosCoordinationMiddleware::new();
        let context = SimulatorContext::new("test".to_string(), "run1".to_string());

        let condition = ChaosRuleCondition::TickCount { min_tick: 5 };
        if let Ok(state) = middleware.state.lock() {
            assert!(!ChaosCoordinationMiddleware::evaluate_rule_condition(
                &condition, &context, &state
            )); // tick is 0

            let mut context_later = context.clone();
            context_later.tick = 10;
            assert!(ChaosCoordinationMiddleware::evaluate_rule_condition(
                &condition,
                &context_later,
                &state
            ));
            // tick is 10
        };
    }

    #[test]
    fn test_chaos_recovery_settings() {
        let settings = ChaosRecoverySettings {
            enable_auto_recovery: true,
            min_recovery_ticks: 100,
            recovery_probability: 0.1,
            enable_coordinated_recovery: false,
        };

        assert!(settings.enable_auto_recovery);
        assert_eq!(settings.min_recovery_ticks, 100);
        assert_eq!(settings.recovery_probability, 0.1);
        assert!(!settings.enable_coordinated_recovery);
    }
}
