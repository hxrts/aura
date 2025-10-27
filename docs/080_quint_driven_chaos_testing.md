# RFC 080: Quint-Driven Chaos Testing with Time Travel Debugging

**Related**: [006_simulation_engine_using_injected_effects.md](006_simulation_engine_using_injected_effects.md), [070_p2p_threshold_protocols.md](070_p2p_threshold_protocols.md)

## Executive Summary

This RFC proposes upgrading Aura's simulation system with formal specification-driven chaos testing and time travel debugging capabilities. The enhancement integrates Quint formal specifications with our existing deterministic simulation framework to enable systematic property verification, targeted failure reproduction, and root cause analysis.

## Motivation

### Current State

Aura currently has a sophisticated deterministic simulation framework with:
- Injectable effects for deterministic testing
- Byzantine adversary support for fault injection
- Network simulation with controllable latency and partitions
- Property-based testing using `proptest`

### Limitations

1. **Manual Test Design**: Chaos scenarios are manually designed, potentially missing edge cases
2. **Limited Property Coverage**: Properties are tested in isolation without systematic verification
3. **Debugging Complexity**: When failures occur, reproduction and analysis is time-intensive
4. **No Formal Verification**: Protocol correctness relies on testing rather than formal guarantees
5. **Gap Between Specs and Tests**: Choreographic protocols exist in code but not as verifiable specifications

### Opportunity

By integrating Quint formal specifications with our simulation framework, we can:
- Automatically generate comprehensive chaos test scenarios
- Provide formal verification of protocol properties
- Enable precise failure reproduction through time travel debugging
- Create a feedback loop between formal specifications and practical testing

## Goals

### Primary Goals
1. **Specification-Driven Testing**: Generate chaos scenarios automatically from Quint specifications
2. **Time Travel Debugging**: Checkpoint/restore simulation state for precise failure analysis
3. **Property Verification**: Real-time monitoring of formal properties during simulation
4. **Root Cause Analysis**: Systematic isolation of minimal failure conditions

### Secondary Goals
1. **Developer Experience**: Intuitive debugging workflow for protocol failures
2. **Continuous Verification**: Integration with CI/CD for regression detection
3. **Performance**: Minimal overhead during normal simulation execution
4. **Maintainability**: Clean separation between simulation and verification concerns

## Architecture Overview

### High-Level Architecture

```
┌─────────────────┐    ┌──────────────────┐    ┌─────────────────┐
│   Quint Specs   │───▶│  Chaos Generator │───▶│   Simulation    │
│  (Properties)   │    │   (Test Cases)   │    │   (Execution)   │
└─────────────────┘    └──────────────────┘    └─────────────────┘
         ▲                       ▲                       │
         │                       │                       ▼
         │              ┌────────────────┐    ┌─────────────────┐
         └──────────────│ Time Travel    │◀───│ Trace Collector │
                        │ Debugger       │    │ (Checkpoints)   │
                        └────────────────┘    └─────────────────┘
```

### System Components

#### 1. Quint Integration Layer
- **QuintBridge**: Parse Quint specifications and extract verifiable properties
- **PropertyTracker**: Monitor property satisfaction during simulation execution
- **TraceConverter**: Transform simulation events into Quint-compatible traces

#### 2. Enhanced Simulation Framework
- **CheckpointSimulation**: Extends existing `Simulation` with state checkpointing
- **PropertyMonitor**: Real-time verification of formal properties
- **TimeTravelDebugger**: Manage checkpoint restoration and focused testing

#### 3. Chaos Test Generation
- **ChaosTestGenerator**: Generate targeted scenarios from Quint properties
- **AdversaryMapper**: Map Quint adversary models to Byzantine device behaviors
- **ScenarioVariator**: Generate focused test variations around failure points

## Detailed Design

### 1. Quint Specification Integration

#### QuintBridge Architecture

```rust
// crates/sim/src/quint/bridge.rs
pub struct QuintBridge {
    specs: HashMap<String, QuintSpec>,
    property_extractor: PropertyExtractor,
    trace_formatter: TraceFormatter,
}

pub struct QuintSpec {
    name: String,
    invariants: Vec<QuintInvariant>,
    temporal_properties: Vec<QuintTemporalProperty>,
    state_variables: Vec<QuintStateVar>,
    transitions: Vec<QuintTransition>,
}

pub struct QuintInvariant {
    name: String,
    property_type: PropertyType,
    condition: QuintExpression,
    violation_generator: fn(&QuintInvariant) -> Vec<ChaosScenario>,
}

impl QuintBridge {
    /// Load and parse Quint specifications from filesystem
    pub fn load_specs(spec_dir: &Path) -> Result<Self, QuintError> {
        let spec_files = glob::glob(&format!("{}/*.qnt", spec_dir.display()))?;
        let mut specs = HashMap::new();

        for spec_file in spec_files {
            let spec = Self::parse_quint_file(&spec_file?)?;
            specs.insert(spec.name.clone(), spec);
        }

        Ok(QuintBridge {
            specs,
            property_extractor: PropertyExtractor::new(),
            trace_formatter: TraceFormatter::new(),
        })
    }

    /// Extract all verifiable properties from loaded specifications
    pub fn extract_properties(&self) -> Vec<VerifiableProperty> {
        self.specs.values()
            .flat_map(|spec| {
                spec.invariants.iter()
                    .map(|inv| VerifiableProperty::Invariant(inv.clone()))
                    .chain(
                        spec.temporal_properties.iter()
                            .map(|prop| VerifiableProperty::Temporal(prop.clone()))
                    )
            })
            .collect()
    }

    /// Generate chaos test scenarios targeting specific properties
    pub fn generate_chaos_scenarios(&self, spec_name: &str) -> Result<Vec<ChaosScenario>, QuintError> {
        let spec = self.specs.get(spec_name)
            .ok_or_else(|| QuintError::SpecNotFound(spec_name.to_string()))?;

        let mut scenarios = Vec::new();

        // Generate invariant violation attempts
        for invariant in &spec.invariants {
            scenarios.extend((invariant.violation_generator)(invariant));
        }

        // Generate liveness violation attempts
        for temporal_prop in &spec.temporal_properties {
            scenarios.extend(self.generate_temporal_violations(temporal_prop));
        }

        Ok(scenarios)
    }

    /// Verify simulation trace against Quint specifications
    pub fn verify_trace(&self, trace: &ExecutionTrace) -> VerificationResult {
        let quint_trace = self.trace_formatter.convert_trace(trace);

        let mut violations = Vec::new();

        for spec in self.specs.values() {
            // Check all invariants at each trace step
            for (step_idx, step) in quint_trace.steps.iter().enumerate() {
                for invariant in &spec.invariants {
                    if !self.evaluate_invariant(invariant, step) {
                        violations.push(PropertyViolation {
                            property: invariant.name.clone(),
                            step: step_idx,
                            trace_fragment: quint_trace.extract_fragment(step_idx),
                            counterexample: self.generate_counterexample(invariant, step),
                        });
                    }
                }
            }

            // Check temporal properties across entire trace
            for temporal_prop in &spec.temporal_properties {
                if !self.evaluate_temporal_property(temporal_prop, &quint_trace) {
                    violations.push(PropertyViolation {
                        property: temporal_prop.name.clone(),
                        step: quint_trace.steps.len(),
                        trace_fragment: quint_trace.clone(),
                        counterexample: self.generate_temporal_counterexample(temporal_prop, &quint_trace),
                    });
                }
            }
        }

        if violations.is_empty() {
            VerificationResult::Success
        } else {
            VerificationResult::Violations(violations)
        }
    }
}
```

#### Property Type System

```rust
#[derive(Debug, Clone)]
pub enum PropertyType {
    // Safety properties (invariants)
    ThresholdSafety,        // M-of-N signatures required
    CounterUniqueness,      // No duplicate counter values
    SessionIsolation,       // Cross-session interference prevention
    CrdtConsistency,        // CRDT convergence properties
    KeyDerivationSafety,    // DKD protocol correctness

    // Liveness properties (temporal)
    ProtocolProgress,       // Protocols eventually complete
    MessageDelivery,        // Messages eventually delivered
    RecoveryCompletion,     // Recovery protocols succeed
    StateConvergence,       // CRDT states eventually converge
}

#[derive(Debug, Clone)]
pub struct ChaosScenario {
    pub name: String,
    pub target_property: PropertyType,
    pub network_conditions: NetworkConditions,
    pub byzantine_devices: Vec<ByzantineDeviceSpec>,
    pub timing_conditions: TimingConditions,
    pub expected_outcome: ExpectedOutcome,
}

#[derive(Debug, Clone)]
pub enum ExpectedOutcome {
    PropertyViolation(String),  // Property should be violated
    PropertyHolds,              // Property should be maintained
    Inconclusive,               // Scenario is exploratory
}
```

### 2. Enhanced Effects System

#### Monitored Effects Architecture

```rust
// crates/crypto/src/effects.rs (enhanced)
pub struct MonitoredEffects {
    base: Effects,
    property_monitor: Arc<Mutex<PropertyMonitor>>,
    trace_collector: Arc<Mutex<TraceCollector>>,
    checkpoint_manager: Arc<CheckpointManager>,
}

impl MonitoredEffects {
    pub fn new(base: Effects, monitor: PropertyMonitor) -> Self {
        MonitoredEffects {
            base,
            property_monitor: Arc::new(Mutex::new(monitor)),
            trace_collector: Arc::new(Mutex::new(TraceCollector::new())),
            checkpoint_manager: Arc::new(CheckpointManager::new()),
        }
    }

    /// Execute effect with property monitoring and trace collection
    pub fn execute_monitored<T>(&self, effect: impl Effect<Output = T> + Clone) -> T {
        let start_state = self.checkpoint_manager.capture_light_checkpoint();
        let start_time = self.base.time.now();

        // Execute the effect
        let result = effect.execute(&self.base);

        let end_time = self.base.time.now();
        let end_state = self.checkpoint_manager.capture_light_checkpoint();

        // Record the execution in trace
        let trace_event = TraceEvent {
            effect_type: effect.type_name(),
            start_time,
            end_time,
            start_state,
            end_state,
            result_summary: self.summarize_result(&result),
        };

        self.trace_collector.lock().unwrap().record_event(trace_event);

        // Check properties after effect execution
        if let Some(violation) = self.property_monitor.lock().unwrap().check_properties(&self.trace_collector.lock().unwrap().current_trace()) {
            // Property violation detected - trigger debugging workflow
            self.handle_property_violation(violation);
        }

        result
    }

    fn handle_property_violation(&self, violation: PropertyViolation) {
        // Create checkpoint at violation point
        let violation_checkpoint = self.checkpoint_manager.create_checkpoint(
            format!("violation_{}", violation.property)
        );

        // Emit violation event for time travel debugger
        self.trace_collector.lock().unwrap().record_violation(violation, violation_checkpoint);
    }
}

pub struct PropertyMonitor {
    quint_bridge: QuintBridge,
    active_properties: Vec<VerifiableProperty>,
    violation_handlers: Vec<Box<dyn ViolationHandler>>,
}

impl PropertyMonitor {
    pub fn check_properties(&mut self, trace: &ExecutionTrace) -> Option<PropertyViolation> {
        // Convert current trace to Quint format and verify
        let verification_result = self.quint_bridge.verify_trace(trace);

        match verification_result {
            VerificationResult::Violations(violations) => {
                // Return the first (most critical) violation
                violations.into_iter().next()
            },
            VerificationResult::Success => None,
        }
    }
}
```

### 3. Checkpointing Simulation Framework

#### CheckpointSimulation Implementation

```rust
// crates/sim/src/checkpoint_simulation.rs
pub struct CheckpointSimulation {
    base: Simulation,
    checkpoints: BTreeMap<EpochTime, SimulationCheckpoint>,
    checkpoint_index: BTreeMap<String, EpochTime>,
    trace_log: ExecutionTrace,
    property_monitor: PropertyMonitor,
}

#[derive(Clone)]
pub struct SimulationCheckpoint {
    pub id: CheckpointId,
    pub label: String,
    pub timestamp: EpochTime,
    pub participant_states: HashMap<ParticipantId, ParticipantSnapshot>,
    pub network_state: NetworkSnapshot,
    pub crdt_states: HashMap<String, CrdtSnapshot>,
    pub effects_state: EffectsSnapshot,
    pub trace_position: usize,
}

impl CheckpointSimulation {
    pub fn new(seed: u64, quint_bridge: QuintBridge) -> Self {
        let base = Simulation::new(seed);
        let property_monitor = PropertyMonitor::new(quint_bridge);

        CheckpointSimulation {
            base,
            checkpoints: BTreeMap::new(),
            checkpoint_index: BTreeMap::new(),
            trace_log: ExecutionTrace::new(),
            property_monitor,
        }
    }

    /// Create named checkpoint at current simulation state
    pub fn checkpoint(&mut self, label: String) -> CheckpointId {
        let timestamp = self.base.current_epoch();
        let checkpoint_id = CheckpointId::new();

        let checkpoint = SimulationCheckpoint {
            id: checkpoint_id,
            label: label.clone(),
            timestamp,
            participant_states: self.capture_participant_states(),
            network_state: self.base.network.snapshot(),
            crdt_states: self.capture_crdt_states(),
            effects_state: self.base.effects_runtime.snapshot(),
            trace_position: self.trace_log.events.len(),
        };

        self.checkpoints.insert(timestamp, checkpoint);
        self.checkpoint_index.insert(label, timestamp);

        checkpoint_id
    }

    /// Restore simulation to a specific checkpoint
    pub fn restore_checkpoint(&mut self, checkpoint_id: CheckpointId) -> Result<(), SimulationError> {
        // Find the checkpoint
        let checkpoint = self.checkpoints.values()
            .find(|cp| cp.id == checkpoint_id)
            .ok_or(SimulationError::CheckpointNotFound(checkpoint_id))?
            .clone();

        // Restore all simulation state
        self.restore_participant_states(&checkpoint.participant_states)?;
        self.base.network.restore_snapshot(&checkpoint.network_state)?;
        self.restore_crdt_states(&checkpoint.crdt_states)?;
        self.base.effects_runtime.restore_snapshot(&checkpoint.effects_state)?;

        // Truncate trace log to checkpoint position
        self.trace_log.events.truncate(checkpoint.trace_position);

        // Reset simulation time to checkpoint
        self.base.set_current_epoch(checkpoint.timestamp);

        Ok(())
    }

    /// Execute simulation with property monitoring
    pub fn run_with_monitoring(&mut self, steps: u64) -> ExecutionResult {
        for step in 0..steps {
            // Execute one simulation step
            match self.base.step() {
                Ok(events) => {
                    // Record events in trace
                    self.trace_log.events.extend(events);

                    // Check properties after each step
                    if let Some(violation) = self.property_monitor.check_properties(&self.trace_log) {
                        // Property violation detected
                        let violation_checkpoint = self.checkpoint(
                            format!("violation_step_{}", step)
                        );

                        return ExecutionResult::PropertyViolation {
                            violation,
                            checkpoint_id: violation_checkpoint,
                            trace: self.trace_log.clone(),
                            step_number: step,
                        };
                    }
                },
                Err(e) => {
                    return ExecutionResult::SimulationError {
                        error: e,
                        step_number: step,
                        trace: self.trace_log.clone(),
                    };
                }
            }
        }

        ExecutionResult::Success {
            trace: self.trace_log.clone(),
            final_state: self.capture_final_state(),
        }
    }

    /// Generate automatic checkpoints at regular intervals
    pub fn run_with_auto_checkpoints(&mut self, steps: u64, checkpoint_interval: u64) -> ExecutionResult {
        for step in 0..steps {
            // Create automatic checkpoint
            if step % checkpoint_interval == 0 {
                self.checkpoint(format!("auto_step_{}", step));
            }

            // Execute with monitoring
            match self.run_with_monitoring(1) {
                ExecutionResult::Success { .. } => continue,
                other => return other, // Return any violation or error immediately
            }
        }

        ExecutionResult::Success {
            trace: self.trace_log.clone(),
            final_state: self.capture_final_state(),
        }
    }
}
```

### 4. Time Travel Debugger

#### TimeTravelDebugger Implementation

```rust
// crates/sim/src/time_travel_debugger.rs
pub struct TimeTravelDebugger {
    simulation: CheckpointSimulation,
    failure_analyzer: FailureAnalyzer,
    focused_tester: FocusedTester,
    chaos_generator: ChaosTestGenerator,
}

pub struct DebugSession {
    pub violation: PropertyViolation,
    pub checkpoints: Vec<CheckpointId>,
    pub focused_tests: Vec<FocusedTest>,
    pub insights: Vec<DebugInsight>,
    pub minimal_reproduction: Option<MinimalReproduction>,
}

impl TimeTravelDebugger {
    pub fn new(simulation: CheckpointSimulation, quint_bridge: QuintBridge) -> Self {
        TimeTravelDebugger {
            simulation,
            failure_analyzer: FailureAnalyzer::new(),
            focused_tester: FocusedTester::new(),
            chaos_generator: ChaosTestGenerator::new(quint_bridge),
        }
    }

    /// Start comprehensive debugging session from a property violation
    pub fn debug_violation(&mut self, violation: PropertyViolation) -> DebugSession {
        println!("[search] Starting time travel debug session for property: {}", violation.property);

        // Phase 1: Analyze the violation and identify critical time window
        let critical_window = self.failure_analyzer.analyze_violation(&violation, &self.simulation.trace_log);
        println!("[stats] Critical failure window: {:?}", critical_window);

        // Phase 2: Create strategic checkpoints around the failure
        let debug_checkpoints = self.create_debug_checkpoints(&critical_window);
        println!("📍 Created {} debug checkpoints", debug_checkpoints.len());

        // Phase 3: Generate focused chaos tests around each checkpoint
        let focused_tests = self.generate_focused_tests(&debug_checkpoints, &violation);
        println!("[target] Generated {} focused tests", focused_tests.len());

        // Phase 4: Execute focused tests to gather insights
        let insights = self.execute_focused_tests(&focused_tests);
        println!("💡 Discovered {} debugging insights", insights.len());

        // Phase 5: Attempt to find minimal reproduction
        let minimal_reproduction = self.find_minimal_reproduction(&violation, &insights);

        DebugSession {
            violation,
            checkpoints: debug_checkpoints,
            focused_tests,
            insights,
            minimal_reproduction,
        }
    }

    /// Create strategic checkpoints around failure point
    fn create_debug_checkpoints(&mut self, window: &CriticalWindow) -> Vec<CheckpointId> {
        let mut checkpoints = Vec::new();

        // Checkpoint at start of critical window
        if let Ok(()) = self.simulation.time_travel_to(window.start_time) {
            let cp_id = self.simulation.checkpoint("critical_window_start".to_string());
            checkpoints.push(cp_id);
        }

        // Checkpoints at key events within window
        for event in &window.key_events {
            if let Ok(()) = self.simulation.time_travel_to(event.timestamp) {
                let cp_id = self.simulation.checkpoint(format!("key_event_{}", event.id));
                checkpoints.push(cp_id);
            }
        }

        // Checkpoint just before failure
        let pre_failure_time = window.failure_time - Duration::from_millis(100);
        if let Ok(()) = self.simulation.time_travel_to(pre_failure_time) {
            let cp_id = self.simulation.checkpoint("pre_failure".to_string());
            checkpoints.push(cp_id);
        }

        checkpoints
    }

    /// Generate focused test variations around each checkpoint
    fn generate_focused_tests(&self, checkpoints: &[CheckpointId], violation: &PropertyViolation) -> Vec<FocusedTest> {
        let mut focused_tests = Vec::new();

        for checkpoint_id in checkpoints {
            // Generate variations targeting the specific property that failed
            let property_specific_tests = self.chaos_generator.generate_property_specific_scenarios(&violation.property);

            for scenario in property_specific_tests {
                focused_tests.push(FocusedTest {
                    checkpoint_id: *checkpoint_id,
                    scenario,
                    execution_window: Duration::from_secs(10), // Short focused execution
                    expected_outcome: ExpectedOutcome::PropertyViolation(violation.property.clone()),
                });
            }

            // Generate environmental variations
            let environmental_tests = self.generate_environmental_variations(*checkpoint_id);
            focused_tests.extend(environmental_tests);
        }

        focused_tests
    }

    /// Execute focused tests and collect insights
    fn execute_focused_tests(&mut self, tests: &[FocusedTest]) -> Vec<DebugInsight> {
        let mut insights = Vec::new();

        for (test_idx, test) in tests.iter().enumerate() {
            println!("🧪 Executing focused test {}/{}: {}", test_idx + 1, tests.len(), test.scenario.name);

            // Restore to checkpoint
            if let Err(e) = self.simulation.restore_checkpoint(test.checkpoint_id) {
                println!("[WARN]  Failed to restore checkpoint: {:?}", e);
                continue;
            }

            // Apply test conditions
            self.simulation.apply_conditions(&test.scenario);

            // Execute for limited time window
            let result = self.simulation.run_with_monitoring_for_duration(test.execution_window);

            // Analyze result and extract insights
            let test_insight = self.analyze_test_result(test, &result);
            insights.push(test_insight);

            // Check if we reproduced the failure
            if matches!(result, ExecutionResult::PropertyViolation { .. }) {
                println!("[OK] Reproduced failure in focused test");
            }
        }

        insights
    }

    /// Attempt to find minimal conditions that reproduce the failure
    fn find_minimal_reproduction(&mut self, violation: &PropertyViolation, insights: &[DebugInsight]) -> Option<MinimalReproduction> {
        // Find all successful reproductions
        let reproductions: Vec<_> = insights.iter()
            .filter(|insight| insight.reproduced_failure)
            .collect();

        if reproductions.is_empty() {
            return None;
        }

        // Find the reproduction with minimal conditions
        let minimal = reproductions.iter()
            .min_by_key(|insight| insight.complexity_score())
            .unwrap();

        Some(MinimalReproduction {
            conditions: minimal.test_conditions.clone(),
            checkpoint_id: minimal.checkpoint_id,
            steps_to_failure: minimal.steps_to_failure,
            property: violation.property.clone(),
        })
    }
}

/// Analysis of failure patterns and root causes
pub struct FailureAnalyzer {
    causal_analyzer: CausalAnalyzer,
    pattern_detector: PatternDetector,
}

impl FailureAnalyzer {
    /// Analyze a property violation to identify critical time window and causal events
    pub fn analyze_violation(&self, violation: &PropertyViolation, trace: &ExecutionTrace) -> CriticalWindow {
        // Step 1: Identify the exact failure point
        let failure_event = trace.events.iter()
            .find(|event| event.timestamp == violation.timestamp)
            .expect("Violation timestamp should exist in trace");

        // Step 2: Perform backwards causal analysis
        let causal_events = self.causal_analyzer.find_causal_chain(failure_event, trace);

        // Step 3: Identify the minimal time window containing all causal events
        let start_time = causal_events.iter()
            .map(|event| event.timestamp)
            .min()
            .unwrap_or(violation.timestamp - Duration::from_secs(10));

        // Step 4: Identify key events that likely contributed to failure
        let key_events = self.identify_key_events(&causal_events, &violation.property);

        CriticalWindow {
            start_time,
            failure_time: violation.timestamp,
            key_events,
            causal_chain: causal_events,
            property: violation.property.clone(),
        }
    }

    fn identify_key_events(&self, causal_events: &[TraceEvent], property: &str) -> Vec<KeyEvent> {
        // Property-specific event identification
        match property {
            prop if prop.contains("threshold") => {
                // Look for signature-related events
                causal_events.iter()
                    .filter(|event| event.event_type.contains("signature") || event.event_type.contains("commitment"))
                    .map(|event| KeyEvent {
                        id: event.id,
                        timestamp: event.timestamp,
                        event_type: event.event_type.clone(),
                        significance: self.calculate_significance(event, property),
                    })
                    .collect()
            },
            prop if prop.contains("counter") => {
                // Look for counter-related events
                causal_events.iter()
                    .filter(|event| event.event_type.contains("counter") || event.event_type.contains("increment"))
                    .map(|event| KeyEvent {
                        id: event.id,
                        timestamp: event.timestamp,
                        event_type: event.event_type.clone(),
                        significance: self.calculate_significance(event, property),
                    })
                    .collect()
            },
            _ => {
                // Generic approach: identify events with high causal significance
                causal_events.iter()
                    .filter(|event| self.calculate_significance(event, property) > 0.5)
                    .map(|event| KeyEvent {
                        id: event.id,
                        timestamp: event.timestamp,
                        event_type: event.event_type.clone(),
                        significance: self.calculate_significance(event, property),
                    })
                    .collect()
            }
        }
    }
}
```

### 5. Declarative Scenarios System

#### 5.1 Replacing Imperative Tests with Declarative Scenarios

The enhanced simulation system replaces the traditional `tests/` directory with a declarative `scenarios/` directory. This system transforms complex imperative test code into human-readable, maintainable scenario specifications using TOML configuration files.

##### Scenario Directory Structure
```
crates/simulator/scenarios/
├── core_protocols/
│   ├── dkd_basic.toml
│   ├── resharing_scenarios.toml
│   └── recovery_flows.toml
├── adversarial/
│   ├── byzantine_resistance.toml
│   ├── network_attacks.toml
│   └── eclipse_scenarios.toml
├── integration/
│   ├── account_lifecycle.toml
│   └── multi_protocol_flows.toml
└── chaos_generated/  # Auto-generated from Quint specs
    ├── threshold_violations.toml
    └── counter_conflicts.toml
```

##### Declarative Scenario Format

```toml
# crates/simulator/scenarios/core_protocols/dkd_basic.toml

[[scenario]]
name = "DKD 3-of-5 Success"
description = "Basic DKD with 5 participants, 3 threshold"
expected_outcome = "success"

[scenario.setup]
participants = 5
threshold = 3
seed = 12345

[scenario.network]
latency_range = [10, 50]  # milliseconds
drop_rate = 0.0
partitions = []

[[scenario.protocols]]
type = "dkd"
timeout_epochs = 100
context = "test_context"

[[scenario.assertions]]
type = "all_participants_derive_same_key"

[[scenario.assertions]]
type = "derived_keys_non_empty"

[[scenario.assertions]]
type = "no_timeout_errors"

[[scenario.assertions]]
type = "ledger_events_consistent"

# Second scenario in same file
[[scenario]]
name = "DKD with Network Delays"
description = "DKD under high network latency"
extends = "DKD 3-of-5 Success"  # Inherit base configuration
expected_outcome = "success"

[scenario.network]
latency_range = [200, 500]  # High latency

[[scenario.assertions]]
type = "protocol_completes_within_timeout"
timeout_multiplier = 3.0
```

##### Adversarial Scenarios

```toml
# crates/simulator/scenarios/adversarial/byzantine_resistance.toml

[[scenario]]
name = "Byzantine DKD - Commitment Equivocation"
description = "Test DKD resistance to commitment equivocation attacks"
expected_outcome = "honest_majority_success"

[scenario.setup]
participants = 5
threshold = 3
seed = 54321

[scenario.byzantine]
count = 2  # f = 2, honest majority = 3
participants = [3, 4]  # Zero-indexed participant IDs

[[scenario.byzantine.strategies]]
type = "commitment_equivocation"
description = "Send different commitments to different peers"

[[scenario.byzantine.strategies]]
type = "selective_abort"
abort_after = "commitment_phase"

[[scenario.protocols]]
type = "dkd"

[[scenario.assertions]]
type = "honest_majority_succeeds"
honest_participants = [0, 1, 2]

[[scenario.assertions]]
type = "byzantine_participants_detected"
expected_detected = [3, 4]
```

##### Multi-Phase Integration Scenarios

```toml
# crates/simulator/scenarios/integration/account_lifecycle.toml

[[scenario]]
name = "Complete Account Lifecycle"
description = "Full account lifecycle: bootstrap → reshare → recovery"

# Phase 1: Bootstrap
[[scenario.phases]]
name = "bootstrap"

[scenario.phases.setup]
participants = 3
threshold = 2

[[scenario.phases.protocols]]
type = "dkd"
context = "root_identity"

[[scenario.phases.assertions]]
type = "all_participants_derive_same_key"

# Phase 2: Add Device
[[scenario.phases]]
name = "add_device"

[scenario.phases.setup]
participants = 4  # Add one device
threshold = 3     # Increase threshold

[[scenario.phases.protocols]]
type = "resharing"
new_threshold = 3

[[scenario.phases.assertions]]
type = "new_threshold_active"

[[scenario.phases.assertions]]
type = "all_shares_valid"

# Phase 3: Device Loss Recovery
[[scenario.phases]]
name = "device_loss_recovery"

[scenario.phases.setup]
participants = 4
threshold = 3
guardian_approvals = 2

[[scenario.phases.simulate]]
type = "device_loss"
lost_device = 0

[[scenario.phases.protocols]]
type = "recovery"
guardian_devices = [1, 2]

[[scenario.phases.assertions]]
type = "recovery_successful"

[[scenario.phases.assertions]]
type = "account_accessible"
```

#### 5.2 Quint-Generated Chaos Scenarios

The system automatically generates chaos scenarios from Quint specifications:

```toml
# Auto-generated from threshold_signatures.qnt
# crates/simulator/scenarios/chaos_generated/threshold_violations.toml

[[scenario]]
name = "Threshold Safety Violation Attempt"
description = "Auto-generated from Quint invariant ThresholdSafety"
expected_outcome = "safety_violation_prevented"

[scenario.quint_source]
specification = "threshold_signatures.qnt"
property = "ThresholdSafety"
violation_pattern = "insufficient_signers"

[scenario.setup]
participants = 5
threshold = 3

[scenario.byzantine]
count = 3  # Try to violate M-of-N requirement

[[scenario.byzantine.strategies]]
type = "signature_withholding"

[[scenario.byzantine.strategies]]
type = "invalid_signatures"

[[scenario.protocols]]
type = "dkd"

[[scenario.assertions]]
type = "property_violation_detected"
expected_property = "ThresholdSafety"

[[scenario.assertions]]
type = "protocol_aborts_safely"
```

#### 5.3 Scenario Execution Engine

```rust
// crates/simulator/src/scenarios/engine.rs
pub struct ScenarioEngine {
    quint_bridge: QuintBridge,
    scenario_loader: ScenarioLoader,
    execution_runtime: ScenarioRuntime,
}

// TOML-based scenario loading
pub struct ScenarioLoader {
    base_scenarios: HashMap<String, Scenario>,  // For inheritance
}

impl ScenarioLoader {
    /// Load all TOML scenario files from a directory
    pub fn load_directory(&mut self, dir: &Path) -> Result<Vec<Scenario>, ScenarioError> {
        let mut scenarios = Vec::new();
        
        // Find all .toml files in directory
        let toml_files = glob::glob(&format!("{}/**/*.toml", dir.display()))?;
        
        for toml_file in toml_files {
            let file_path = toml_file?;
            let file_scenarios = self.load_toml_file(&file_path)?;
            scenarios.extend(file_scenarios);
        }
        
        // Resolve inheritance after all files are loaded
        self.resolve_inheritance(&mut scenarios)?;
        
        Ok(scenarios)
    }
    
    /// Load scenarios from a single TOML file
    fn load_toml_file(&mut self, file_path: &Path) -> Result<Vec<Scenario>, ScenarioError> {
        let content = std::fs::read_to_string(file_path)?;
        let scenario_file: ScenarioFile = toml::from_str(&content)
            .map_err(|e| ScenarioError::TomlParse {
                file: file_path.to_string_lossy().to_string(),
                error: e.to_string(),
            })?;
        
        // Store scenarios that might be used for inheritance
        for scenario in &scenario_file.scenario {
            self.base_scenarios.insert(scenario.name.clone(), scenario.clone());
        }
        
        Ok(scenario_file.scenario)
    }
    
    /// Resolve "extends" relationships between scenarios
    fn resolve_inheritance(&self, scenarios: &mut Vec<Scenario>) -> Result<(), ScenarioError> {
        for scenario in scenarios.iter_mut() {
            if let Some(extends) = &scenario.extends {
                let base_scenario = self.base_scenarios.get(extends)
                    .ok_or_else(|| ScenarioError::InheritanceNotFound {
                        scenario: scenario.name.clone(),
                        extends: extends.clone(),
                    })?;
                
                // Inherit fields from base scenario if not overridden
                scenario.inherit_from(base_scenario);
            }
        }
        Ok(())
    }
}

impl Scenario {
    /// Inherit configuration from a base scenario
    fn inherit_from(&mut self, base: &Scenario) {
        // Only inherit if current value is None/empty
        if self.setup.participants == 0 {
            self.setup = base.setup.clone();
        }
        if self.network.is_none() {
            self.network = base.network.clone();
        }
        if self.byzantine.is_none() {
            self.byzantine = base.byzantine.clone();
        }
        if self.protocols.is_none() || self.protocols.as_ref().unwrap().is_empty() {
            self.protocols = base.protocols.clone();
        }
        // Assertions are additive - base + new assertions
        let mut inherited_assertions = base.assertions.clone();
        inherited_assertions.extend(self.assertions.clone());
        self.assertions = inherited_assertions;
    }
}

impl ScenarioEngine {
    /// Load and execute all scenarios in a directory
    pub async fn execute_scenario_suite(&mut self, scenario_dir: &Path) -> SuiteResult {
        let scenarios = self.scenario_loader.load_directory(scenario_dir)?;
        let mut results = Vec::new();
        
        for scenario in scenarios {
            let result = self.execute_scenario(&scenario).await?;
            results.push(result);
            
            // If this scenario was auto-generated from Quint and failed,
            // trigger time travel debugging
            if scenario.is_quint_generated() && result.has_violations() {
                self.trigger_time_travel_debug(&scenario, &result).await?;
            }
        }
        
        SuiteResult::new(results)
    }
    
    /// Execute a single declarative scenario
    pub async fn execute_scenario(&mut self, scenario: &Scenario) -> ScenarioResult {
        // Phase 1: Set up simulation from declarative configuration
        let mut sim = self.create_simulation_from_scenario(scenario)?;
        
        // Phase 2: Execute scenario phases sequentially
        let mut phase_results = Vec::new();
        
        for phase in &scenario.phases {
            let phase_result = self.execute_phase(&mut sim, phase).await?;
            phase_results.push(phase_result);
            
            // Early termination on critical failures
            if phase_result.should_abort_scenario() {
                break;
            }
        }
        
        // Phase 3: Verify assertions
        let assertion_results = self.verify_assertions(&sim, &scenario.assertions).await?;
        
        // Phase 4: Generate execution report
        ScenarioResult {
            scenario_name: scenario.name.clone(),
            phases: phase_results,
            assertions: assertion_results,
            execution_trace: sim.get_execution_trace(),
            final_state: sim.capture_final_state(),
        }
    }
    
    fn create_simulation_from_scenario(&self, scenario: &Scenario) -> Result<CheckpointSimulation> {
        let mut sim = CheckpointSimulation::new(scenario.setup.seed, self.quint_bridge.clone());
        
        // Configure participants
        sim.add_participants(scenario.setup.participants);
        sim.set_threshold(scenario.setup.threshold);
        
        // Configure network conditions
        if let Some(network) = &scenario.network {
            sim.set_latency_range(network.latency_range[0], network.latency_range[1]);
            sim.set_drop_rate(network.drop_rate);
            
            for partition in &network.partitions {
                sim.create_network_partition(partition);
            }
        }
        
        // Configure Byzantine participants
        if let Some(byzantine) = &scenario.byzantine {
            for (idx, strategy) in byzantine.strategies.iter().enumerate() {
                let participant_id = byzantine.participants[idx];
                sim.make_participant_byzantine(participant_id, strategy.clone());
            }
        }
        
        Ok(sim)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScenarioFile {
    pub scenario: Vec<Scenario>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Scenario {
    pub name: String,
    pub description: String,
    pub setup: ScenarioSetup,
    pub network: Option<NetworkConditions>,
    pub byzantine: Option<ByzantineConditions>,
    pub phases: Option<Vec<ScenarioPhase>>,
    pub protocols: Option<Vec<ProtocolExecution>>,
    pub assertions: Vec<ScenarioAssertion>,
    pub expected_outcome: ExpectedOutcome,
    pub extends: Option<String>,
    pub quint_source: Option<QuintMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScenarioSetup {
    pub participants: usize,
    pub threshold: usize,
    pub seed: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScenarioPhase {
    pub name: String,
    pub protocols: Vec<ProtocolExecution>,
    pub simulate: Option<Vec<SimulationEvent>>,
    pub checkpoints: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProtocolExecution {
    pub protocol_type: ProtocolType,
    pub timeout_epochs: Option<u64>,
    pub context: Option<String>,
    pub parameters: HashMap<String, serde_yaml::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub enum ProtocolType {
    Dkd,
    Resharing,
    Recovery,
    Locking,
    CounterCoordination,
}
```

#### 5.4 Integration with Quint-Driven Chaos Testing

The declarative scenarios system seamlessly integrates with Quint-driven chaos testing:

```rust
impl ScenarioEngine {
    /// Generate chaos scenarios from Quint specifications
    pub fn generate_quint_scenarios(&self, spec_name: &str) -> Result<Vec<Scenario>> {
        let chaos_scenarios = self.quint_bridge.generate_chaos_scenarios(spec_name)?;
        
        chaos_scenarios.into_iter()
            .map(|chaos_scenario| self.convert_chaos_to_declarative_scenario(chaos_scenario))
            .collect()
    }
    
    /// Convert ChaosScenario to declarative Scenario format
    fn convert_chaos_to_declarative_scenario(&self, chaos: ChaosScenario) -> Scenario {
        Scenario {
            name: format!("Chaos: {}", chaos.name),
            description: format!("Auto-generated chaos scenario targeting {}", chaos.target_property),
            
            setup: ScenarioSetup {
                participants: chaos.participant_count(),
                threshold: chaos.threshold_requirement(),
                seed: chaos.deterministic_seed(),
            },
            
            network: Some(NetworkConditions {
                latency_range: chaos.network_conditions.latency_range,
                drop_rate: chaos.network_conditions.drop_rate,
                partitions: chaos.network_conditions.partitions,
            }),
            
            byzantine: chaos.byzantine_devices.into(),
            
            phases: vec![ScenarioPhase {
                name: "chaos_execution".to_string(),
                protocols: vec![ProtocolExecution {
                    protocol_type: chaos.target_protocol,
                    timeout_epochs: Some(chaos.timeout_epochs),
                    context: Some(chaos.context),
                    parameters: chaos.parameters,
                }],
                simulate: chaos.simulation_events,
                checkpoints: vec!["pre_chaos".to_string(), "post_chaos".to_string()],
            }],
            
            assertions: chaos.expected_violations.into_iter()
                .map(|violation| ScenarioAssertion::PropertyViolation(violation))
                .collect(),
                
            expected_outcome: chaos.expected_outcome,
            
            quint_metadata: Some(QuintMetadata {
                specification: chaos.quint_spec_name,
                property: chaos.target_property,
                violation_type: chaos.violation_type,
            }),
        }
    }
}
```

#### 5.5 Benefits of Declarative TOML Scenarios

1. **Maintainability**: Scenarios are human-readable TOML instead of complex Rust test code
2. **Type Safety**: TOML's strong typing prevents common configuration errors
3. **Composability**: Scenarios can extend and inherit from base configurations
4. **Rust Ecosystem**: Native TOML support in Rust with excellent `serde` integration
5. **Discoverability**: All test scenarios are visible in the file system
6. **Quint Integration**: Seamless generation of chaos scenarios from formal specifications
7. **Non-Developer Friendly**: Protocol designers can write scenarios without deep Rust knowledge
8. **Version Control**: Scenario changes are clearly visible in diffs
9. **Comments**: TOML supports inline comments for documentation
10. **Automation**: CI/CD can easily discover and execute all scenarios
11. **Debugging**: Failed scenarios automatically trigger time travel debugging
12. **Performance**: Fast TOML parsing with minimal overhead

### 6. Integration Workflow

#### End-to-End Testing Workflow

```rust
// Example of complete workflow integration
#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_quint_driven_chaos_testing_workflow() {
        // Step 1: Load Quint specifications
        let quint_bridge = QuintBridge::load_specs("specs/quint").unwrap();

        // Step 2: Create enhanced simulation with monitoring
        let mut sim = CheckpointSimulation::new(DETERMINISTIC_SEED, quint_bridge.clone());

        // Step 3: Generate chaos scenarios from Quint specs
        let chaos_scenarios = quint_bridge.generate_chaos_scenarios("threshold_signatures").unwrap();

        for scenario in chaos_scenarios {
            println!("[target] Testing chaos scenario: {}", scenario.name);

            // Step 4: Set up simulation with scenario conditions
            sim.reset_to_initial_state();
            sim.add_participants(5); // 5 devices for threshold testing
            sim.apply_conditions(&scenario);

            // Step 5: Execute with property monitoring
            match sim.run_with_auto_checkpoints(1000, 100) {
                ExecutionResult::PropertyViolation { violation, checkpoint_id, .. } => {
                    println!("[ERROR] Property violation detected: {}", violation.property);

                    // Step 6: Start time travel debugging
                    let mut debugger = TimeTravelDebugger::new(sim.clone(), quint_bridge.clone());
                    let debug_session = debugger.debug_violation(violation);

                    // Step 7: Analyze debugging results
                    if let Some(minimal_repro) = debug_session.minimal_reproduction {
                        println!("[target] Found minimal reproduction with {} conditions",
                                minimal_repro.conditions.len());

                        // Step 8: Verify minimal reproduction
                        verify_minimal_reproduction(&minimal_repro, &mut sim);
                    }

                    // Step 9: Generate insights for developers
                    generate_developer_report(&debug_session);
                },
                ExecutionResult::Success { .. } => {
                    println!("[OK] Scenario passed: {}", scenario.name);
                },
                ExecutionResult::SimulationError { error, .. } => {
                    println!("[ERROR] Simulation error: {:?}", error);
                }
            }
        }
    }

    fn verify_minimal_reproduction(repro: &MinimalReproduction, sim: &mut CheckpointSimulation) {
        // Restore to checkpoint and verify reproduction
        sim.restore_checkpoint(repro.checkpoint_id).unwrap();
        sim.apply_conditions(&repro.conditions);

        let result = sim.run_with_monitoring(repro.steps_to_failure + 10);

        match result {
            ExecutionResult::PropertyViolation { violation, .. } => {
                assert_eq!(violation.property, repro.property);
                println!("[OK] Minimal reproduction verified");
            },
            _ => {
                panic!("[ERROR] Minimal reproduction failed to reproduce the violation");
            }
        }
    }

    fn generate_developer_report(session: &DebugSession) {
        println!("\n=== TIME TRAVEL DEBUG REPORT ===");
        println!("Property violated: {}", session.violation.property);
        println!("Debug checkpoints created: {}", session.checkpoints.len());
        println!("Focused tests executed: {}", session.focused_tests.len());
        println!("Insights discovered: {}", session.insights.len());

        if let Some(repro) = &session.minimal_reproduction {
            println!("\n--- MINIMAL REPRODUCTION ---");
            println!("Checkpoint: {:?}", repro.checkpoint_id);
            println!("Steps to failure: {}", repro.steps_to_failure);
            println!("Conditions required: {:#?}", repro.conditions);
        }

        println!("\n--- KEY INSIGHTS ---");
        for insight in &session.insights {
            if insight.significance > 0.8 {
                println!("• {}", insight.description);
            }
        }
    }
}
```

## Implementation Plan

### Phase 1: Foundation (Weeks 1-3)
1. **Declarative Scenarios Framework**: Implement YAML-based scenario system and execution engine
2. **Quint Integration Layer**: Implement `QuintBridge` and property extraction
3. **Enhanced Effects System**: Add property monitoring to `Effects`
4. **Basic Checkpointing**: Implement checkpoint creation and restoration

### Phase 2: Core Features (Weeks 4-6)
1. **Scenario-to-Simulation Bridge**: Connect declarative scenarios to simulation execution
2. **Chaos Test Generation**: Implement automated scenario generation from Quint specs
3. **Property Monitoring**: Real-time verification during simulation
4. **Time Travel Basics**: Basic checkpoint restoration and replay

### Phase 3: Advanced Features (Weeks 7-9)
1. **Quint-Generated Scenarios**: Auto-generation of chaos scenarios from formal specifications
2. **Failure Analysis**: Causal chain analysis and critical window identification
3. **Focused Testing**: Targeted test generation around failure points
4. **Root Cause Analysis**: Minimal reproduction discovery

### Phase 4: Integration & Polish (Weeks 10-12)
1. **Scenario Inheritance & Composition**: Advanced scenario features (extends, phases)
2. **CI/CD Integration**: Automated property verification in continuous integration
3. **Developer Tools**: CLI tools and debugging interfaces for scenarios
4. **Documentation**: Comprehensive guides and examples for declarative testing

## Success Metrics

### Quantitative Metrics
- **Property Coverage**: % of Quint properties verified during simulation
- **Failure Reproduction Rate**: % of property violations successfully reproduced
- **Minimal Reproduction Discovery**: % of failures reduced to minimal conditions
- **Debug Session Duration**: Time from violation detection to root cause identification
- **Scenario Coverage**: % of protocol behaviors covered by declarative scenarios
- **Scenario Execution Speed**: Time to execute full scenario suite
- **Auto-Generated Scenario Quality**: % of Quint-generated scenarios that find real issues

### Qualitative Metrics
- **Developer Experience**: Ease of debugging protocol failures and writing new scenarios
- **Specification Quality**: Completeness and accuracy of Quint specifications
- **Test Effectiveness**: Number of real bugs caught by generated chaos tests
- **Scenario Maintainability**: Ease of updating and extending declarative scenarios
- **Non-Developer Usability**: Ability for protocol designers to contribute scenarios

## Risk Mitigation

### Technical Risks
1. **Performance Overhead**: Checkpointing and monitoring could slow simulation
   - *Mitigation*: Lazy checkpointing, lightweight property evaluation
2. **Quint Integration Complexity**: Parsing and evaluating Quint specifications
   - *Mitigation*: Start with simple properties, gradually add complexity
3. **State Space Explosion**: Too many checkpoints and test variations
   - *Mitigation*: Smart pruning algorithms, focus on critical paths

### Project Risks
1. **Development Timeline**: Complex system with many integration points
   - *Mitigation*: Phased approach, early prototyping, incremental delivery
2. **Learning Curve**: Team needs to learn Quint specification language
   - *Mitigation*: Training sessions, documentation, gradual adoption

## Conclusion

This proposal outlines a comprehensive upgrade to Aura's simulation system that combines formal verification with practical chaos testing and declarative scenario management. By integrating Quint specifications with our existing deterministic simulation framework and replacing imperative tests with declarative scenarios, we can achieve:

1. **Systematic Property Verification**: Ensure protocol correctness through formal methods
2. **Comprehensive Failure Testing**: Generate targeted chaos scenarios automatically from Quint specifications
3. **Efficient Debugging**: Time travel capabilities for precise failure analysis
4. **Maintainable Testing**: Human-readable, declarative scenarios replace complex imperative test code
5. **Continuous Improvement**: Feedback loop between specifications, scenarios, and implementation

The proposed architecture leverages Aura's existing strengths in deterministic simulation while adding:
- The rigor of formal specification with Quint
- The precision of time travel debugging
- The maintainability of declarative scenario management
- Seamless integration between formal properties and practical testing

This combination will significantly improve our ability to develop robust, verifiable distributed protocols while making testing more accessible to protocol designers and easier to maintain over time.

## References

- [Quint Language Documentation](https://quint-lang.org/docs)
- [Choreographic Programming](https://quint-lang.org/choreo)
- [TLA+ Specification Language](https://lamport.azurewebsites.net/tla/tla.html)
- [Aura Simulation Engine RFC](006_simulation_engine_using_injected_effects.md)
- [P2P Threshold Protocols RFC](070_p2p_threshold_protocols.md)
