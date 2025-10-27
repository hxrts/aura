# RFC: Quint-Simulation Integration for Formal Property Verification

**Status**: Draft
**Authors**: Aura Development Team
**Related Documents**:
- [006_simulation_engine_using_injected_effects.md](006_simulation_engine_using_injected_effects.md)
- [080_quint_driven_chaos_testing.md](080_quint_driven_chaos_testing.md)
- [110_dev_console_architecture.md](110_dev_console_architecture.md)

## Executive Summary

This RFC proposes a comprehensive integration between Quint formal specifications and our existing simulation infrastructure to achieve the best of both worlds: mathematical rigor from formal verification and practical validation from chaos engineering. The integration introduces a **hybrid verification architecture** that leverages Quint as a property oracle while maintaining our powerful simulation capabilities for real-world distributed systems testing.

## Motivation

### Current Architecture Strengths

Our existing simulation system provides:
- **Real implementation testing** with actual Rust protocol code
- **Deterministic chaos engineering** with Byzantine fault injection
- **Production-scale testing** with network partitions and realistic failures
- **Rich visualization** through the Dev Console for debugging and understanding

### Current Architecture Gaps

However, we're missing:
- **Formal property verification** during simulation execution
- **Systematic edge case discovery** through model checking
- **Mathematical correctness guarantees** for protocol properties
- **Automatic counterexample generation** from property violations

### Quint's Capabilities We Can Leverage

Quint provides:
- **Formal specification language** with TLA+-based semantics
- **Model checking** with exhaustive state exploration
- **Property verification** (invariants + temporal logic)
- **Counterexample generation** for property violations
- **Trace analysis** in ITF (Informal Trace Format)

## Goals

### Primary Goals
1. **Hybrid Verification**: Combine formal verification with practical testing
2. **Property Oracle**: Use Quint specifications to define what properties to verify
3. **Automatic Chaos Generation**: Generate edge case scenarios from Quint counterexamples
4. **Real-time Property Monitoring**: Check formal properties during simulation execution
5. **Enhanced Visualization**: Integrate property violations into Dev Console debugging workflow

### Secondary Goals
1. **Developer Experience**: Seamless workflow between specifications and testing
2. **CI/CD Integration**: Automatic property verification in continuous integration
3. **Educational Value**: Bridge formal methods and practical distributed systems

## Architecture Overview

### Hybrid Architecture: Quint as Property Oracle + Aura as Implementation Validator

```
┌─────────────────────────────────────────────────────────────────┐
│ Quint Formal Verification Layer                                 │
│                                                                 │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐           │
│  │ Quint Specs  │  │ Model Check  │  │ Counter-     │           │
│  │ (.qnt files) │─▶│ Properties   │─▶│ example Gen  │           │
│  │              │  │              │  │              │           │
│  │ • Invariants │  │ • Safety     │  │ • Edge Cases │           │
│  │ • Temporal   │  │ • Liveness   │  │ • Violation  │           │
│  │ • Properties │  │ • Fairness   │  │   Scenarios  │           │
│  └──────────────┘  └──────────────┘  └──────────────┘           │
│                                             │                   │
└─────────────────────────────────────────────┼───────────────────┘
                                              │
                                              ▼
┌─────────────────────────────────────────────┼──────────────────┐
│ Integration Layer: QuintBridge              │                  │
│                                             │                  │
│  ┌──────────────┐  ┌──────────────┐  ┌─────▼──────┐            │
│  │ Property     │  │ Trace        │  │ Scenario   │            │
│  │ Extractor    │  │ Converter    │  │ Generator  │            │
│  │              │  │              │  │            │            │
│  │ • Parse .qnt │  │ • Aura ↔ ITF │  │ • Counter- │.           │
│  │ • Extract    │  │ • Bi-direct  │  │   examples │            │
│  │   invariants │  │   conversion │  │ • TOML     │            │
│  └──────────────┘  └──────────────┘  └────────────┘            │
│                                             │                  │
└─────────────────────────────────────────────┼──────────────────┘
                                              │
                                              ▼
┌─────────────────────────────────────────────┼──────────────────┐
│ Aura Simulation + Visualization             │                  │
│                                             │                  │
│  ┌──────────────┐  ┌──────────────┐  ┌─────▼──────┐            │
│  │ Enhanced     │  │ Property     │  │ Generated  │            │
│  │ Simulation   │  │ Monitor      │  │ Scenarios  │            │
│  │              │  │              │  │            │            │
│  │ • Real Rust  │  │ • Real-time  │  │ • Chaos    │            │
│  │   protocols  │  │   checking   │  │   testing  │            │
│  │ • Chaos      │  │ • Violation  │  │ • Edge     │            │
│  │   injection  │  │   detection  │  │   cases    │            │
│  └──────────────┘  └──────────────┘  └────────────┘            │
│                                             │                  │
│  ┌──────────────────────────────────────────┼──────────────┐   │
│  │ Dev Console Integration                  │              │   │
│  │                                          ▼              │   │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐      │   │
│  │  │ Property    │  │ Violation   │  │ Trace       │      │   │
│  │  │ Dashboard   │  │ Inspector   │  │ Verifier    │      │   │
│  │  └─────────────┘  └─────────────┘  └─────────────┘      │   │
│  └─────────────────────────────────────────────────────────┘   │
└────────────────────────────────────────────────────────────────┘
```

## Detailed Design

### 1. QuintBridge: Integration Layer

The `QuintBridge` serves as the crucial integration layer between Quint's formal verification and our simulation system.

```rust
// crates/quint-bridge/src/lib.rs
pub struct QuintBridge {
    quint_runner: QuintRunner,
    property_extractor: PropertyExtractor,
    trace_converter: TraceConverter,
    scenario_generator: ScenarioGenerator,
}

#[derive(Debug, Clone)]
pub struct QuintSpec {
    name: String,
    file_path: PathBuf,
    invariants: Vec<QuintInvariant>,
    temporal_properties: Vec<QuintTemporalProperty>,
    state_variables: Vec<QuintStateVar>,
    actions: Vec<QuintAction>,
}

impl QuintBridge {
    /// Load and parse all Quint specifications from directory
    pub fn load_specs(spec_dir: &Path) -> Result<Self, QuintError> {
        let quint_files = glob::glob(&format!("{}/*.qnt", spec_dir.display()))?;
        let mut specs = HashMap::new();

        for qnt_file in quint_files {
            // Use Quint CLI to parse and validate
            let output = Command::new("quint")
                .args(["parse", &qnt_file?.to_string_lossy()])
                .output()?;

            if !output.status.success() {
                return Err(QuintError::ParseFailed(String::from_utf8_lossy(&output.stderr).to_string()));
            }

            let spec = Self::parse_quint_ast(&output.stdout)?;
            specs.insert(spec.name.clone(), spec);
        }

        Ok(QuintBridge {
            quint_runner: QuintRunner::new(),
            property_extractor: PropertyExtractor::new(specs),
            trace_converter: TraceConverter::new(),
            scenario_generator: ScenarioGenerator::new(),
        })
    }

    /// Extract all verifiable properties from loaded specifications
    pub fn extract_properties(&self, protocol: &str) -> Vec<VerifiableProperty> {
        self.property_extractor.get_properties_for_protocol(protocol)
    }

    /// Run Quint model checking to find counterexamples
    pub fn find_counterexamples(&self, spec_name: &str, property: &str) -> Result<Vec<QuintCounterexample>, QuintError> {
        // Use Quint CLI to run model checking
        let output = Command::new("quint")
            .args([
                "verify",
                "--invariant", property,
                &format!("specs/quint/{}.qnt", spec_name)
            ])
            .output()?;

        if output.status.success() {
            // Property holds - no counterexamples
            Ok(vec![])
        } else {
            // Parse counterexamples from Quint output
            self.parse_counterexamples(&output.stdout)
        }
    }

    /// Convert Aura simulation trace to Quint ITF format
    pub fn convert_trace_to_itf(&self, trace: &SimulationTrace) -> Result<ItfTrace, ConversionError> {
        self.trace_converter.aura_to_itf(trace)
    }

    /// Verify Aura simulation trace against Quint properties
    pub fn verify_trace(&self, trace: &SimulationTrace, properties: &[VerifiableProperty]) -> VerificationResult {
        // Convert trace to ITF format
        let itf_trace = match self.convert_trace_to_itf(trace) {
            Ok(itf) => itf,
            Err(e) => return VerificationResult::ConversionError(e),
        };

        let mut violations = Vec::new();

        for property in properties {
            // Write ITF trace to temp file
            let trace_file = self.write_temp_itf_trace(&itf_trace)?;

            // Use Quint to check property against trace
            let verification = self.quint_runner.verify_trace_property(&trace_file, property)?;

            if let Some(violation) = verification.violation {
                violations.push(PropertyViolation {
                    property: property.name.clone(),
                    step: violation.step,
                    trace_fragment: violation.trace_fragment,
                    quint_counterexample: Some(violation.counterexample),
                });
            }

            // Clean up temp file
            std::fs::remove_file(trace_file)?;
        }

        if violations.is_empty() {
            VerificationResult::Success
        } else {
            VerificationResult::Violations(violations)
        }
    }

    /// Generate chaos test scenarios from Quint counterexamples
    pub fn generate_scenarios_from_counterexamples(
        &self,
        spec_name: &str,
        property: &str
    ) -> Result<Vec<Scenario>, QuintError> {
        let counterexamples = self.find_counterexamples(spec_name, property)?;

        counterexamples.into_iter()
            .map(|ce| self.scenario_generator.convert_counterexample_to_scenario(ce))
            .collect()
    }
}
```

### 2. Enhanced Simulation with Property Monitoring

Extend our existing simulation to include real-time property monitoring:

```rust
// crates/sim/src/monitored_simulation.rs
pub struct MonitoredSimulation {
    base: Simulation,
    quint_bridge: QuintBridge,
    property_monitor: PropertyMonitor,
    violation_handler: ViolationHandler,
}

pub struct PropertyMonitor {
    active_properties: Vec<VerifiableProperty>,
    trace_buffer: VecDeque<TraceEvent>,
    check_interval: u64,  // Check properties every N ticks
}

impl MonitoredSimulation {
    pub fn new(seed: u64, quint_bridge: QuintBridge) -> Self {
        let base = Simulation::new(seed);
        let properties = quint_bridge.extract_properties("all");

        MonitoredSimulation {
            base,
            quint_bridge,
            property_monitor: PropertyMonitor::new(properties),
            violation_handler: ViolationHandler::new(),
        }
    }

    /// Step simulation with property monitoring
    pub fn step_monitored(&mut self, steps: u64) -> StepResult {
        for step in 0..steps {
            // Execute normal simulation step
            let events = self.base.step()?;

            // Add events to trace buffer for property checking
            self.property_monitor.trace_buffer.extend(events);

            // Check properties at regular intervals
            if step % self.property_monitor.check_interval == 0 {
                if let Some(violation) = self.check_properties_incremental()? {
                    return StepResult::PropertyViolation {
                        violation,
                        step_number: step,
                        trace: self.get_trace_snapshot(),
                    };
                }
            }
        }

        StepResult::Success { steps_executed: steps }
    }

    /// Check properties against current trace buffer
    fn check_properties_incremental(&mut self) -> Result<Option<PropertyViolation>, SimError> {
        // Convert recent trace buffer to ITF format
        let recent_trace = SimulationTrace::from_events(&self.property_monitor.trace_buffer);

        // Verify against all active properties
        let verification_result = self.quint_bridge.verify_trace(
            &recent_trace,
            &self.property_monitor.active_properties
        );

        match verification_result {
            VerificationResult::Violations(violations) => {
                let violation = violations.into_iter().next().unwrap();

                // Handle violation (checkpoint, debug info, etc.)
                self.violation_handler.handle_violation(&violation, &self.base);

                Ok(Some(violation))
            },
            VerificationResult::Success => Ok(None),
            VerificationResult::ConversionError(e) => Err(SimError::QuintConversion(e)),
        }
    }

    /// Run until idle with comprehensive property verification
    pub fn run_until_idle_verified(&mut self) -> VerificationResult {
        // Run simulation to completion
        let result = self.base.run_until_idle();

        // Get full execution trace
        let full_trace = self.base.get_execution_trace();

        // Verify against all properties
        self.quint_bridge.verify_trace(&full_trace, &self.property_monitor.active_properties)
    }
}
```

### 3. Scenario Generation from Counterexamples

Convert Quint counterexamples into executable TOML scenarios:

```rust
// crates/quint-bridge/src/scenario_generator.rs
pub struct ScenarioGenerator {
    template_loader: TemplateLoader,
    action_converter: ActionConverter,
}

impl ScenarioGenerator {
    /// Convert Quint counterexample to Aura scenario
    pub fn convert_counterexample_to_scenario(&self, ce: QuintCounterexample) -> Scenario {
        // Extract setup from counterexample initial state
        let setup = self.extract_setup_from_initial_state(&ce.initial_state);

        // Convert Quint actions to Aura protocol steps
        let protocol_steps = ce.trace.steps.iter()
            .filter_map(|step| self.action_converter.convert_quint_action(&step.action))
            .collect();

        // Generate Byzantine behaviors from counterexample pattern
        let byzantine_config = self.infer_byzantine_behavior(&ce);

        // Create scenario targeting the violated property
        Scenario {
            name: format!("Generated: {} Violation", ce.violated_property),
            description: format!(
                "Auto-generated scenario from Quint counterexample for property: {}",
                ce.violated_property
            ),

            setup: ScenarioSetup {
                participants: setup.participant_count,
                threshold: setup.threshold,
                seed: self.generate_deterministic_seed(&ce),
            },

            network: Some(self.extract_network_conditions(&ce)),
            byzantine: byzantine_config,

            phases: vec![ScenarioPhase {
                name: "counterexample_execution".to_string(),
                protocols: protocol_steps,
                simulate: Some(self.extract_simulation_events(&ce)),
                checkpoints: vec!["pre_violation".to_string()],
            }],

            assertions: vec![
                ScenarioAssertion::PropertyViolation {
                    property: ce.violated_property.clone(),
                    expected_step: ce.violation_step,
                }
            ],

            expected_outcome: ExpectedOutcome::PropertyViolation(ce.violated_property),

            quint_metadata: Some(QuintMetadata {
                specification: ce.source_spec.clone(),
                property: ce.violated_property,
                counterexample_id: ce.id,
                generation_timestamp: SystemTime::now(),
            }),
        }
    }

    /// Infer Byzantine behavior patterns from counterexample
    fn infer_byzantine_behavior(&self, ce: &QuintCounterexample) -> Option<ByzantineConditions> {
        // Analyze counterexample trace for suspicious patterns
        let mut suspicious_participants = HashSet::new();
        let mut behavior_patterns = Vec::new();

        for step in &ce.trace.steps {
            match &step.action {
                QuintAction::SendMessage { from, to, content } => {
                    // Look for message tampering, equivocation, etc.
                    if self.is_malicious_message(content) {
                        suspicious_participants.insert(from.clone());
                        behavior_patterns.push(ByzantineStrategy {
                            strategy_type: "message_tampering".to_string(),
                            description: format!("Tampered message in step {}", step.step_number),
                            parameters: self.extract_tampering_params(content),
                        });
                    }
                },
                QuintAction::StateTransition { participant, from_state, to_state } => {
                    // Look for invalid state transitions
                    if !self.is_valid_transition(from_state, to_state) {
                        suspicious_participants.insert(participant.clone());
                        behavior_patterns.push(ByzantineStrategy {
                            strategy_type: "invalid_state_transition".to_string(),
                            description: format!("Invalid transition: {} -> {}", from_state, to_state),
                            parameters: HashMap::new(),
                        });
                    }
                },
                _ => {}
            }
        }

        if !suspicious_participants.is_empty() {
            Some(ByzantineConditions {
                count: suspicious_participants.len(),
                participants: suspicious_participants.into_iter().collect(),
                strategies: behavior_patterns,
            })
        } else {
            None
        }
    }
}
```

### 4. Dev Console Integration

Enhance the Dev Console to display formal verification results:

```rust
// console/src/components/property_dashboard.rs
use leptos::*;
use stylance::import_style;

import_style!(style, "property_dashboard.module.css");

#[component]
pub fn PropertyDashboard() -> impl IntoView {
    let (properties, set_properties) = create_signal(Vec::<VerifiableProperty>::new());
    let (violations, set_violations) = create_signal(Vec::<PropertyViolation>::new());

    view! {
        <div class=style::dashboard>
            <div class=style::header>
                <h2>"Formal Properties"</h2>
                <button class=style::refresh_btn on:click=move |_| {
                    // Refresh properties from QuintBridge
                }>"Refresh"</button>
            </div>

            <div class=style::content>
                <div class=style::properties_panel>
                    <h3>"Active Properties"</h3>
                    <For
                        each=move || properties.get()
                        key=|prop| prop.name.clone()
                        children=move |prop| {
                            view! {
                                <PropertyCard property=prop />
                            }
                        }
                    />
                </div>

                <div class=style::violations_panel>
                    <h3>"Property Violations"</h3>
                    <For
                        each=move || violations.get()
                        key=|violation| violation.id
                        children=move |violation| {
                            view! {
                                <ViolationCard violation=violation />
                            }
                        }
                    />
                </div>
            </div>
        </div>
    }
}

#[component]
fn PropertyCard(property: VerifiableProperty) -> impl IntoView {
    let status_class = match property.status {
        PropertyStatus::Verified => style::status_verified,
        PropertyStatus::Violated => style::status_violated,
        PropertyStatus::Unknown => style::status_unknown,
    };

    view! {
        <div class=format!("{} {}", style::property_card, status_class)>
            <div class=style::property_header>
                <span class=style::property_name>{property.name}</span>
                <span class=style::property_type>{property.property_type}</span>
            </div>
            <div class=style::property_description>
                {property.description}
            </div>
            <div class=style::property_actions>
                <button class=style::verify_btn on:click=move |_| {
                    // Trigger verification for this property
                }>"Verify"</button>
                <button class=style::generate_tests_btn on:click=move |_| {
                    // Generate test scenarios for this property
                }>"Generate Tests"</button>
            </div>
        </div>
    }
}
```

```css
/* console/src/components/property_dashboard.module.css */
.dashboard {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
}

.header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: var(--spacing-md);
    border-bottom: 1px solid var(--border);
}

.content {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--spacing-md);
    padding: var(--spacing-md);
    overflow: auto;
}

.property_card {
    background: var(--bg-tertiary);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: var(--spacing-md);
    margin-bottom: var(--spacing-sm);
}

.status_verified {
    border-left: 4px solid var(--success);
}

.status_violated {
    border-left: 4px solid var(--error);
}

.status_unknown {
    border-left: 4px solid var(--warning);
}

.property_header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: var(--spacing-sm);
}

.property_name {
    font-weight: bold;
    color: var(--text-primary);
}

.property_type {
    font-size: 0.85em;
    color: var(--text-secondary);
    background: var(--bg-primary);
    padding: 2px 6px;
    border-radius: 3px;
}

.property_actions {
    display: flex;
    gap: var(--spacing-sm);
    margin-top: var(--spacing-sm);
}

.verify_btn, .generate_tests_btn {
    padding: var(--spacing-xs) var(--spacing-sm);
    background: var(--accent);
    color: white;
    border: none;
    border-radius: 3px;
    cursor: pointer;
    font-size: 0.85em;
}

.verify_btn:hover, .generate_tests_btn:hover {
    background: var(--accent-hover);
}
```

### 5. Enhanced Visualization Components

Add property-aware visualization to existing Dev Console components:

```rust
// console/src/components/enhanced_timeline.rs
#[component]
pub fn EnhancedTimeline() -> impl IntoView {
    let (trace_events, set_trace_events) = create_signal(Vec::<TraceEvent>::new());
    let (property_violations, set_violations) = create_signal(Vec::<PropertyViolation>::new());

    view! {
        <div class=style::timeline_container>
            <div class=style::timeline_header>
                <div class=style::controls>
                    <PlaybackControls />
                </div>
                <div class=style::property_filter>
                    <PropertyFilter />
                </div>
            </div>

            <div class=style::timeline_content>
                // Main timeline with property violation markers
                <svg class=style::timeline_svg>
                    // Render timeline ticks
                    <TimelineAxis />

                    // Render participant swimlanes
                    <For
                        each=move || get_participants()
                        key=|p| p.id.clone()
                        children=move |participant| {
                            view! {
                                <ParticipantLane
                                    participant=participant
                                    events=trace_events
                                    violations=property_violations
                                />
                            }
                        }
                    />

                    // Overlay property violation markers
                    <For
                        each=move || property_violations.get()
                        key=|v| v.id
                        children=move |violation| {
                            view! {
                                <ViolationMarker violation=violation />
                            }
                        }
                    />
                </svg>
            </div>
        </div>
    }
}

#[component]
fn ViolationMarker(violation: PropertyViolation) -> impl IntoView {
    let x = violation.step as f64 * TICK_WIDTH;
    let onclick = move |_| {
        // Jump to violation and open property inspector
        navigate_to_violation(&violation);
    };

    view! {
        <g class=style::violation_marker on:click=onclick>
            <line
                x1=x y1="0"
                x2=x y2="100%"
                stroke="var(--error)"
                stroke-width="2"
                stroke-dasharray="5,5"
            />
            <circle
                cx=x cy="10"
                r="8"
                fill="var(--error)"
                class=style::violation_indicator
            />
            <text
                x=x y="25"
                text-anchor="middle"
                fill="var(--error)"
                font-size="10"
            >
                {violation.property.chars().take(8).collect::<String>()}
            </text>
        </g>
    }
}
```

## Implementation Plan

### Phase 1: Foundation (Weeks 1-3)
1. **QuintBridge Core**
   - Implement basic Quint CLI integration
   - Property extraction from .qnt files
   - Basic trace conversion (Aura → ITF)
   - Simple counterexample parsing

2. **Enhanced Simulation**
   - Add property monitoring to existing simulation
   - Implement incremental property checking
   - Basic violation detection and reporting

3. **Dev Console Integration**
   - Add Property Dashboard component
   - Enhance Timeline with violation markers
   - Basic property verification UI

### Phase 2: Automation (Weeks 4-6)
1. **Scenario Generation**
   - Implement counterexample → scenario conversion
   - Automatic chaos test generation
   - TOML scenario export with Quint metadata

2. **Real-time Verification**
   - Streaming property verification during simulation
   - Violation detection with immediate debugging
   - Checkpoint creation at violation points

3. **CI/CD Integration**
   - Automatic property extraction for all specs
   - Generated scenario execution in CI
   - Property verification reports

### Phase 3: Advanced Features (Weeks 7-9)
1. **Bidirectional Trace Conversion**
   - Full ITF → Aura trace conversion
   - Load Quint-generated traces in simulation
   - Cross-verification between Quint and Aura

2. **Advanced Property Types**
   - Temporal logic properties (LTL/CTL)
   - Fairness and liveness properties
   - Custom property definitions

3. **Enhanced Visualization**
   - Property-aware choreography diagrams
   - Causality visualization with property context
   - Interactive property exploration

### Phase 4: Optimization (Weeks 10-12)
1. **Performance Optimization**
   - Efficient incremental property checking
   - Lazy evaluation of complex properties
   - Parallel verification for independent properties

2. **Developer Experience**
   - Property specification assistant
   - Violation explanation and debugging hints
   - Automatic minimal reproduction generation

3. **Advanced Integration**
   - Quint REPL integration in Dev Console
   - Interactive property exploration
   - Specification-driven test generation

## Use Cases and Workflows

### Use Case 1: Property-Driven Development

```bash
# 1. Define formal properties in Quint
cat specs/quint/dkd_protocol.qnt
# Contains: invariant KeyConsistency = ...

# 2. Extract properties and generate chaos tests
cargo run --bin quint-chaos-gen -- \
  --spec dkd_protocol \
  --property KeyConsistency \
  --output scenarios/generated/

# 3. Run generated scenarios in CI
cargo test scenario_dkd_key_consistency_violation

# 4. If violation found, debug in Dev Console
aura-dev-console --trace ci-artifacts/violation-trace.bin
```

### Use Case 2: Real-time Verification During Development

```rust
#[test]
fn test_dkd_with_property_monitoring() {
    let quint_bridge = QuintBridge::load_specs("specs/quint")?;
    let mut sim = MonitoredSimulation::new(SEED, quint_bridge);

    // Add participants
    let alice = sim.add_participant("alice");
    let bob = sim.add_participant("bob");
    let carol = sim.add_participant("carol");

    // Initiate DKD protocol
    sim.initiate_dkd(vec![alice, bob, carol], "test_context")?;

    // Run with real-time property monitoring
    match sim.run_until_idle_verified() {
        VerificationResult::Success => {
            println!("[OK] All properties verified");
        },
        VerificationResult::Violations(violations) => {
            for violation in violations {
                println!("[ERROR] Property violation: {}", violation.property);
                // Automatic debugging workflow triggered
                let debug_session = sim.debug_violation(violation);
                debug_session.export_minimal_reproduction("minimal-repro.toml");
            }
        }
    }
}
```

### Use Case 3: Interactive Property Exploration in Dev Console

```bash
# Dev Console REPL with Quint integration
>> load scenario dkd-basic.toml
>> load quint-spec dkd_protocol.qnt

>> properties
Available properties:
- KeyConsistency (invariant): All honest participants derive same key
- ProtocolTermination (temporal): DKD eventually completes or aborts
- ThresholdSafety (invariant): No valid key with < threshold participants

>> verify KeyConsistency
[OK] Property holds for current scenario

>> generate-counterexample ProtocolTermination
Found counterexample: Live-lock due to message reordering
Generated scenario: scenarios/generated/livelock-dkd.toml

>> load scenario scenarios/generated/livelock-dkd.toml
>> run
[ERROR] Property violation at step 1500: ProtocolTermination
[Automatic time travel debugging initiated]

>> goto 1450
>> step 10
[Observe the conditions leading to livelock]
```

## Benefits

### For Developers
1. **Confidence**: Formal guarantees that protocols are mathematically correct
2. **Efficiency**: Automatic discovery of edge cases without manual scenario design
3. **Understanding**: Rich visualization linking formal properties to concrete execution
4. **Debugging**: Systematic root cause analysis guided by property violations

### For Protocol Designers
1. **Rigor**: Bridge between mathematical specification and practical implementation
2. **Validation**: Verify that implementations correctly realize formal specifications
3. **Communication**: Shared language between formal methods and systems engineering

### For Testing
1. **Coverage**: Systematic exploration of protocol behaviors
2. **Automation**: Continuous verification integrated with CI/CD
3. **Regression**: Prevent introduction of bugs that violate known properties
4. **Reproducibility**: Deterministic reproduction of complex distributed system failures

## Success Metrics

### Quantitative Metrics
- **Property Coverage**: % of Quint properties verified during simulation
- **Bug Detection Rate**: Number of real bugs found through generated scenarios
- **Counterexample Quality**: % of Quint counterexamples that translate to meaningful scenarios
- **Performance Impact**: Overhead of real-time property monitoring (target: <10%)
- **CI Integration**: Time to detect property violations in continuous integration

### Qualitative Metrics
- **Developer Adoption**: Usage of property-driven development workflow
- **Specification Quality**: Completeness and accuracy of Quint specifications
- **Educational Impact**: Team understanding of formal methods concepts
- **Debugging Effectiveness**: Time from violation detection to root cause identification

## Conclusion

This proposal outlines a comprehensive integration between Quint formal verification and our existing simulation infrastructure. By using Quint as a **property oracle** while maintaining our **implementation-focused testing capabilities**, we achieve the best of both worlds:

- **Mathematical rigor** from formal specification and verification
- **Practical validation** from chaos engineering and real-world testing
- **Rich visualization** linking formal properties to concrete execution
- **Automatic test generation** from counterexamples and property violations

The hybrid architecture leverages each system's strengths while addressing their individual limitations, creating a uniquely powerful approach to distributed systems verification that combines formal methods with practical systems engineering.

The integration preserves our existing simulation strengths while adding formal verification capabilities that will significantly improve our confidence in protocol correctness and our ability to systematically discover and fix edge cases.
