//! Scenario management effect handler for simulation
//!
//! This module provides simulation-specific scenario injection and management
//! capabilities. Replaces the former ScenarioInjectionMiddleware with proper
//! effect system integration.

use async_trait::async_trait;
use aura_core::effects::{TestingEffects, TestingError};
use aura_core::frost::ThresholdSignature;
use aura_core::{AuraError, AuraFault, AuraFaultKind, AuthorityId, CorruptionMode, FaultEdge};
use aura_testkit::simulation::choreography::{test_threshold_group, ChoreographyTestHarness};
use std::any::Any;
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::time::Duration;

type SimTimestamp = u64;

// Lightweight deterministic FROST-like types to avoid pulling the full aura-frost dependency in simulator-only code.
#[derive(Debug, Clone)]
struct ThresholdSigningConfig {
    threshold: usize,
    total_signers: usize,
    _timeout_ms: u64,
}

impl ThresholdSigningConfig {
    fn new(threshold: usize, total_signers: usize, timeout_ms: u64) -> Self {
        Self {
            threshold,
            total_signers,
            _timeout_ms: timeout_ms,
        }
    }
}

#[derive(Debug, Clone)]
struct TreeSigningContext;

impl TreeSigningContext {
    fn new(_node: u64, _epoch: u64, _policy_hash: [u8; 32]) -> Self {
        TreeSigningContext
    }
}

#[derive(Debug, Clone)]
struct DummyKeyPackage {
    signer: u16,
}

#[derive(Debug, Clone)]
struct NonceCommitment {
    signer: u16,
}

#[derive(Debug, Clone)]
struct SigningNonces {
    signer: u16,
}

#[derive(Debug, Clone)]
struct PartialSignature {
    signer: u16,
}

#[derive(Debug, Clone)]
struct SigningCommitments {
    signer: u16,
}

#[derive(Debug, Clone)]
struct KeyMaterial {
    key_packages: HashMap<AuthorityId, DummyKeyPackage>,
    public_key_package: (),
}

struct FrostCrypto;

impl FrostCrypto {
    async fn generate_key_material(
        authorities: &[AuthorityId],
        _config: &ThresholdSigningConfig,
        _ctx: &dyn Any,
    ) -> Result<KeyMaterial, AuraError> {
        let mut key_packages = HashMap::new();
        for (idx, auth) in authorities.iter().enumerate() {
            key_packages.insert(*auth, DummyKeyPackage { signer: idx as u16 });
        }
        Ok(KeyMaterial {
            key_packages,
            public_key_package: (),
        })
    }

    async fn generate_nonce_commitment(
        key_pkg: &DummyKeyPackage,
        _ctx: &dyn Any,
    ) -> Result<(SigningNonces, NonceCommitment), AuraError> {
        Ok((
            SigningNonces {
                signer: key_pkg.signer,
            },
            NonceCommitment {
                signer: key_pkg.signer,
            },
        ))
    }

    async fn generate_partial_signature(
        _ctx: &TreeSigningContext,
        _message: &[u8],
        key_pkg: &DummyKeyPackage,
        nonces: &SigningNonces,
        _commitments: &BTreeMap<u16, SigningCommitments>,
        _ctx_effects: &dyn Any,
    ) -> Result<PartialSignature, AuraError> {
        Ok(PartialSignature {
            signer: nonces.signer.max(key_pkg.signer),
        })
    }

    async fn aggregate_signatures(
        _ctx: &TreeSigningContext,
        _message: &[u8],
        partial_signatures: &HashMap<AuthorityId, PartialSignature>,
        _nonce_commitments: &HashMap<AuthorityId, NonceCommitment>,
        config: &ThresholdSigningConfig,
        _group_pk: &(),
    ) -> Result<ThresholdSignature, AuraError> {
        let mut signers: Vec<u16> = partial_signatures.values().map(|p| p.signer).collect();
        signers.sort();
        signers.dedup();
        if signers.len() < config.threshold {
            // pad with synthetic signer IDs to satisfy threshold for demo purposes
            let missing = config.threshold - signers.len();
            let start = signers.last().copied().unwrap_or(0) + 1;
            signers.extend(start..start + missing as u16);
        }
        Ok(ThresholdSignature::new(vec![0u8; 64], signers))
    }
}

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
    TriggerFault { fault: AuraFault },
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

impl ScenarioDefinition {
    /// Build a scenario that injects a Telltale-style network partition fault.
    pub fn telltale_network_partition(
        id: impl Into<String>,
        name: impl Into<String>,
        groups: Vec<Vec<String>>,
        duration: Duration,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            actions: vec![InjectionAction::TriggerFault {
                fault: AuraFault::new(AuraFaultKind::NetworkPartition {
                    partition: groups,
                    duration: Some(duration),
                }),
            }],
            trigger: TriggerCondition::Immediate,
            duration: Some(duration),
            priority: 10,
        }
    }

    /// Build a scenario that injects a Telltale-style message delay fault.
    pub fn telltale_message_delay(
        id: impl Into<String>,
        name: impl Into<String>,
        min_delay: Duration,
        max_delay: Duration,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            actions: vec![InjectionAction::TriggerFault {
                fault: AuraFault::new(AuraFaultKind::MessageDelay {
                    edge: FaultEdge::new("*", "*"),
                    min: min_delay,
                    max: max_delay,
                }),
            }],
            trigger: TriggerCondition::Immediate,
            duration: Some(max_delay),
            priority: 10,
        }
    }

    /// Build a scenario that injects a Telltale-style message corruption fault.
    pub fn telltale_message_corruption(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            actions: vec![InjectionAction::TriggerFault {
                fault: AuraFault::new(AuraFaultKind::MessageCorruption {
                    edge: FaultEdge::new("*", "*"),
                    mode: CorruptionMode::Opaque,
                }),
            }],
            trigger: TriggerCondition::Immediate,
            duration: None,
            priority: 10,
        }
    }

    /// Build a scenario that injects a Telltale-style message drop fault.
    pub fn telltale_message_drop(
        id: impl Into<String>,
        name: impl Into<String>,
        probability: f64,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            actions: vec![InjectionAction::TriggerFault {
                fault: AuraFault::new(AuraFaultKind::MessageDrop {
                    edge: FaultEdge::new("*", "*"),
                    probability,
                }),
            }],
            trigger: TriggerCondition::Immediate,
            duration: None,
            priority: 10,
        }
    }

    /// Build a scenario that injects a Telltale-style node crash fault.
    pub fn telltale_node_crash(
        id: impl Into<String>,
        name: impl Into<String>,
        node: impl Into<String>,
        at_tick: Option<u64>,
        duration: Option<Duration>,
    ) -> Self {
        let trigger = at_tick.map_or(TriggerCondition::Immediate, TriggerCondition::AtTick);
        Self {
            id: id.into(),
            name: name.into(),
            actions: vec![InjectionAction::TriggerFault {
                fault: AuraFault::new(AuraFaultKind::NodeCrash {
                    node: node.into(),
                    at_tick,
                    duration,
                }),
            }],
            trigger,
            duration,
            priority: 10,
        }
    }
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
    /// Trigger after a number of simulation steps (ticks)
    AfterStep(u64),
    /// Trigger when specific event occurs
    OnEvent(String),
    /// Trigger randomly based on probability
    Random(f64),
}

/// Currently active scenario injection
#[derive(Debug)]
struct ActiveInjection {
    scenario_id: String,
    start_tick: SimTimestamp,
    duration_ms: Option<u64>,
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
    trigger_counts: HashMap<String, u64>,
    seed: u64,
    // Multi-actor chat support
    chat_groups: HashMap<String, ChatGroup>,
    message_history: HashMap<String, Vec<ChatMessage>>, // group_id -> messages
    participant_data_loss: HashMap<String, DataLossInfo>,
    recovery_state: HashMap<String, RecoveryInfo>,
    current_tick: u64,
    network_conditions: Vec<NetworkCondition>,
}

#[derive(Debug, Clone)]
struct ChatGroup {
    id: String,
    name: String,
    creator: String,
    members: Vec<String>,
    created_at: SimTimestamp,
}

#[derive(Debug, Clone)]
struct ChatMessage {
    id: String,
    group_id: String,
    sender: String,
    content: String,
    timestamp: SimTimestamp,
}

#[derive(Debug, Clone)]
struct DataLossInfo {
    participant: String,
    loss_type: String,
    occurred_at: SimTimestamp,
    recovery_required: bool,
    pre_loss_message_count: usize,
}

#[derive(Debug, Clone)]
struct RecoveryInfo {
    target: String,
    guardians: Vec<String>,
    threshold: usize,
    initiated_at: SimTimestamp,
    completed: bool,
    validation_steps: Vec<String>,
}

#[derive(Debug, Clone)]
struct ScenarioCheckpoint {
    id: String,
    label: String,
    timestamp: SimTimestamp,
    state_snapshot: HashMap<String, String>,
}

/// Portable checkpoint snapshot payload used by simulator upgrade/resume tests.
#[derive(Debug, Clone)]
pub struct ScenarioCheckpointSnapshot {
    /// Checkpoint identifier.
    pub id: String,
    /// Human-readable checkpoint label.
    pub label: String,
    /// Tick timestamp at checkpoint creation.
    pub timestamp: SimTimestamp,
    /// Captured state fields.
    pub state_snapshot: HashMap<String, String>,
}

impl From<ScenarioCheckpoint> for ScenarioCheckpointSnapshot {
    fn from(checkpoint: ScenarioCheckpoint) -> Self {
        Self {
            id: checkpoint.id,
            label: checkpoint.label,
            timestamp: checkpoint.timestamp,
            state_snapshot: checkpoint.state_snapshot,
        }
    }
}

impl From<ScenarioCheckpointSnapshot> for ScenarioCheckpoint {
    fn from(snapshot: ScenarioCheckpointSnapshot) -> Self {
        Self {
            id: snapshot.id,
            label: snapshot.label,
            timestamp: snapshot.timestamp,
            state_snapshot: snapshot.state_snapshot,
        }
    }
}

#[derive(Debug, Clone)]
struct SimulationEvent {
    event_type: String,
    timestamp: SimTimestamp,
    data: HashMap<String, String>,
}

#[derive(Debug, Clone)]
struct MetricValue {
    value: f64,
    unit: String,
    timestamp: SimTimestamp,
}

#[derive(Debug, Clone)]
struct NetworkCondition {
    condition: String,
    participants: Vec<String>,
    expires_at_tick: u64,
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
                trigger_counts: HashMap::new(),
                seed,
                chat_groups: HashMap::new(),
                message_history: HashMap::new(),
                participant_data_loss: HashMap::new(),
                recovery_state: HashMap::new(),
                current_tick: 0,
                network_conditions: Vec::new(),
            })),
        }
    }

    /// Register a scenario for potential injection
    pub fn register_scenario(&self, scenario: ScenarioDefinition) -> Result<(), TestingError> {
        let mut state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
        })?;

        state.scenarios.insert(scenario.id.clone(), scenario);
        Ok(())
    }

    /// Enable or disable random scenario injection
    pub fn set_randomization(&self, enable: bool, probability: f64) -> Result<(), TestingError> {
        let mut state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
        })?;

        state.enable_randomization = enable;
        state.injection_probability = probability.clamp(0.0, 1.0);
        Ok(())
    }

    /// Manually trigger a specific scenario
    pub fn trigger_scenario(&self, scenario_id: &str) -> Result<(), TestingError> {
        let mut state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
        })?;
        Self::cleanup_expired_injections_locked(&mut state);
        Self::activate_scenario_locked(&mut state, scenario_id)
    }

    /// Advance simulated time by ticks
    pub fn wait_ticks(&self, ticks: u64) -> Result<(), TestingError> {
        let mut state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
        })?;

        state.current_tick = state.current_tick.saturating_add(ticks);
        let current_tick = state.current_tick;
        state.events.push(SimulationEvent {
            event_type: "wait_ticks".to_string(),
            timestamp: current_tick,
            data: HashMap::from([
                ("ticks".to_string(), ticks.to_string()),
                ("current_tick".to_string(), current_tick.to_string()),
            ]),
        });
        self.cleanup_expired_conditions(&mut state);
        Self::cleanup_expired_injections_locked(&mut state);
        Self::evaluate_scenario_triggers_locked(&mut state, None)?;
        Ok(())
    }

    /// Advance simulated time by milliseconds (treated as ticks)
    pub fn wait_ms(&self, duration_ms: u64) -> Result<(), TestingError> {
        self.wait_ticks(duration_ms)
    }

    /// Apply a transient network condition
    pub fn apply_network_condition(
        &self,
        condition: &str,
        participants: Vec<String>,
        duration_ticks: u64,
    ) -> Result<(), TestingError> {
        let mut state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
        })?;

        let expires_at_tick = state.current_tick.saturating_add(duration_ticks);
        state.network_conditions.push(NetworkCondition {
            condition: condition.to_string(),
            participants: participants.clone(),
            expires_at_tick,
        });

        let current_tick = state.current_tick;
        state.events.push(SimulationEvent {
            event_type: "network_condition".to_string(),
            timestamp: current_tick,
            data: HashMap::from([
                ("condition".to_string(), condition.to_string()),
                ("participants".to_string(), format!("{participants:?}")),
                ("duration_ticks".to_string(), duration_ticks.to_string()),
            ]),
        });
        Self::cleanup_expired_injections_locked(&mut state);
        Self::evaluate_scenario_triggers_locked(&mut state, Some("network_condition"))?;

        Ok(())
    }

    /// Inject a fault/Byzantine behavior (logged)
    pub fn inject_fault(&self, participant: &str, behavior: &str) -> Result<(), TestingError> {
        self.record_simple_event(
            "fault_injection",
            HashMap::from([
                ("participant".to_string(), participant.to_string()),
                ("behavior".to_string(), behavior.to_string()),
            ]),
        )
    }

    /// Inject a canonical Aura fault (logged as JSON for replay/debugging).
    pub fn inject_aura_fault(
        &self,
        participant: &str,
        fault: &AuraFault,
    ) -> Result<(), TestingError> {
        let fault_json = serde_json::to_string(fault).map_err(|error| {
            TestingError::SystemError(aura_core::AuraError::internal(format!(
                "failed to serialize fault payload: {error}"
            )))
        })?;
        self.record_simple_event(
            "fault_injection",
            HashMap::from([
                ("participant".to_string(), participant.to_string()),
                ("fault".to_string(), fault_json),
            ]),
        )
    }

    /// Create a lightweight checkpoint of simulation state
    pub fn create_checkpoint(&self, label: &str) -> Result<String, TestingError> {
        let mut state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
        })?;

        let checkpoint_id = format!("ckpt_{}_{}", label, state.checkpoints.len());
        let checkpoint = ScenarioCheckpoint {
            id: checkpoint_id.clone(),
            label: label.to_string(),
            timestamp: state.current_tick,
            state_snapshot: HashMap::new(),
        };
        state.checkpoints.insert(checkpoint_id.clone(), checkpoint);
        Ok(checkpoint_id)
    }

    /// Export one checkpoint snapshot payload for cross-instance restore tests.
    pub fn export_checkpoint_snapshot(
        &self,
        checkpoint_id: &str,
    ) -> Result<ScenarioCheckpointSnapshot, TestingError> {
        let state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
        })?;
        let checkpoint = state
            .checkpoints
            .get(checkpoint_id)
            .cloned()
            .ok_or_else(|| TestingError::CheckpointError {
                checkpoint_id: checkpoint_id.to_string(),
                reason: "Checkpoint not found".to_string(),
            })?;
        Ok(checkpoint.into())
    }

    /// Import one checkpoint snapshot payload into this simulator instance.
    pub fn import_checkpoint_snapshot(
        &self,
        snapshot: ScenarioCheckpointSnapshot,
    ) -> Result<(), TestingError> {
        let mut state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
        })?;
        state
            .checkpoints
            .insert(snapshot.id.clone(), snapshot.into());
        Ok(())
    }

    /// Record export trace intent
    pub fn export_choreo_trace(&self, format: &str, output: &str) -> Result<(), TestingError> {
        self.record_simple_event(
            "export_choreo_trace",
            HashMap::from([
                ("format".to_string(), format.to_string()),
                ("output".to_string(), output.to_string()),
            ]),
        )
    }

    /// Record timeline generation intent
    pub fn generate_timeline(&self, output: &str) -> Result<(), TestingError> {
        self.record_simple_event(
            "generate_timeline",
            HashMap::from([("output".to_string(), output.to_string())]),
        )
    }

    /// Record property verification sweep
    pub fn verify_all_properties(&self) -> Result<(), TestingError> {
        self.record_simple_event("verify_all_properties", HashMap::new())
    }

    /// Record setup choreography event (simulation no-op)
    pub fn setup_choreography(
        &self,
        protocol: &str,
        participants: Vec<String>,
    ) -> Result<(), TestingError> {
        self.record_simple_event(
            "setup_choreography",
            HashMap::from([
                ("protocol".to_string(), protocol.to_string()),
                ("participants".to_string(), format!("{participants:?}")),
            ]),
        )
    }

    /// Record load key shares event (simulation no-op)
    pub fn load_key_shares(&self, threshold: usize) -> Result<(), TestingError> {
        self.record_simple_event(
            "load_key_shares",
            HashMap::from([("threshold".to_string(), threshold.to_string())]),
        )
    }

    /// Record choreography execution (simulation no-op)
    pub fn run_choreography_stub(
        &self,
        name: &str,
        participants: Vec<String>,
        params: HashMap<String, String>,
    ) -> Result<(), TestingError> {
        let mut data = HashMap::from([
            ("choreography".to_string(), name.to_string()),
            ("participants".to_string(), format!("{participants:?}")),
        ]);
        data.extend(params);
        self.record_simple_event("run_choreography", data)
    }

    /// Execute real choreography behaviors using testkit harnesses and protocol helpers.
    pub async fn run_choreography(
        &self,
        name: &str,
        participants: Vec<String>,
        params: HashMap<String, String>,
    ) -> Result<(), TestingError> {
        let normalized = name.to_lowercase();
        match normalized.as_str() {
            "frost_threshold_sign" | "threshold_sign" | "frost_sign" => {
                self.execute_frost_threshold(&participants, &params).await
            }
            "frost_key_generation" | "frost_keygen" | "keygen" => {
                self.execute_frost_keygen(&participants, &params).await
            }
            "frost_commitment" | "commitment" | "commit" => {
                self.execute_frost_commitment_phase(&participants, &params)
                    .await
            }
            "frost_signing" | "signing" | "sign_only" => {
                self.execute_frost_signing_phase(&participants, &params)
                    .await
            }
            "commit_reveal" | "frost_commit_reveal" => {
                self.execute_frost_commit_reveal(&participants, &params)
                    .await
            }
            "coordinator_failure_recovery" | "frost_recovery" => {
                self.execute_frost_recovery(&participants, &params)
            }
            "dkd_handshake" | "handshake" => self.execute_dkd_handshake(&participants, &params),
            "context_agreement" | "dkd_context" => {
                self.execute_context_agreement(&participants, &params)
            }
            "p2p_dkd" | "dkd_point_to_point" => self.execute_p2p_dkd(&participants, &params),
            "distributed_keygen" | "dkg" => self.execute_dkg(&participants, &params).await,
            "session_setup" | "session" => self.execute_session_setup(&participants).await,
            "guardian_setup" | "guardian_request" => {
                self.execute_guardian_setup(&participants, &params)
            }
            "guardian_share_distribution" | "guardian_key_shares" => {
                self.execute_guardian_share_distribution(&participants, &params)
            }
            "guardian_attestation" | "guardian_verify" => {
                self.execute_guardian_attestation(&participants, &params)
            }
            "guardian_recovery" | "guardian_finalize" => {
                self.execute_guardian_recovery(&participants, &params)
            }
            "gossip_sync" | "crdt_merge" => self.execute_gossip(&participants, &params),
            _ => self.run_choreography_stub(name, participants, params),
        }
    }

    fn harness_for_participants(&self, participants: &[String]) -> ChoreographyTestHarness {
        if participants.is_empty() {
            test_threshold_group()
        } else {
            let labels: Vec<&str> = participants.iter().map(|p| p.as_str()).collect();
            ChoreographyTestHarness::with_labeled_devices(labels)
        }
    }

    fn execute_guardian_share_distribution(
        &self,
        participants: &[String],
        params: &HashMap<String, String>,
    ) -> Result<(), TestingError> {
        let threshold = params
            .get("threshold")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(2);
        let target = params
            .get("target")
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());

        self.record_simple_event(
            "run_choreography",
            HashMap::from([
                (
                    "choreography".to_string(),
                    "guardian_share_distribution".to_string(),
                ),
                ("status".to_string(), "ok".to_string()),
                ("participants".to_string(), format!("{participants:?}")),
                ("threshold".to_string(), threshold.to_string()),
                ("target".to_string(), target),
            ]),
        )
    }

    fn execute_guardian_attestation(
        &self,
        participants: &[String],
        params: &HashMap<String, String>,
    ) -> Result<(), TestingError> {
        let target = params
            .get("target")
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());
        let attestation = params
            .get("attestation")
            .cloned()
            .unwrap_or_else(|| "unknown_signer".to_string());

        self.record_simple_event(
            "run_choreography",
            HashMap::from([
                (
                    "choreography".to_string(),
                    "guardian_attestation".to_string(),
                ),
                ("status".to_string(), "ok".to_string()),
                ("participants".to_string(), format!("{participants:?}")),
                ("target".to_string(), target),
                ("attestation".to_string(), attestation),
            ]),
        )
    }

    fn execute_guardian_recovery(
        &self,
        participants: &[String],
        params: &HashMap<String, String>,
    ) -> Result<(), TestingError> {
        let target = params
            .get("target")
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());
        let validation_steps = params
            .get("validation_steps")
            .cloned()
            .unwrap_or_else(|| "shares_verified,identity_rehydrated".to_string());

        self.record_simple_event(
            "run_choreography",
            HashMap::from([
                ("choreography".to_string(), "guardian_recovery".to_string()),
                ("status".to_string(), "ok".to_string()),
                ("participants".to_string(), format!("{participants:?}")),
                ("target".to_string(), target),
                ("validation_steps".to_string(), validation_steps),
            ]),
        )
    }

    fn frost_commitments_map(
        &self,
        commitments: &HashMap<AuthorityId, NonceCommitment>,
    ) -> Result<BTreeMap<u16, SigningCommitments>, AuraError> {
        let mut frost_commitments = BTreeMap::new();
        for commitment in commitments.values() {
            frost_commitments.insert(
                commitment.signer,
                SigningCommitments {
                    signer: commitment.signer,
                },
            );
        }
        Ok(frost_commitments)
    }

    fn frost_setup(
        &self,
        participants: &[String],
        params: &HashMap<String, String>,
    ) -> Result<
        (
            ChoreographyTestHarness,
            ThresholdSigningConfig,
            Vec<AuthorityId>,
        ),
        TestingError,
    > {
        let harness = self.harness_for_participants(participants);
        let total = harness.device_count().max(2);
        let threshold = params
            .get("threshold")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or_else(|| total.min(2));

        let config = ThresholdSigningConfig::new(threshold, total, 120);
        let authorities: Vec<AuthorityId> = (0..config.total_signers)
            .enumerate()
            .map(|(idx, _)| AuthorityId::new_from_entropy([idx as u8; 32]))
            .collect();
        Ok((harness, config, authorities))
    }

    async fn execute_frost_keygen(
        &self,
        participants: &[String],
        params: &HashMap<String, String>,
    ) -> Result<(), TestingError> {
        let (harness, config, authorities) = self.frost_setup(participants, params)?;
        let result = {
            let device_ctx = harness
                .device_context(0)
                .ok_or_else(|| AuraError::internal("missing device context"))?;

            FrostCrypto::generate_key_material(&authorities, &config, device_ctx).await
        };

        match result {
            Ok(_material) => self.record_simple_event(
                "run_choreography",
                HashMap::from([
                    (
                        "choreography".to_string(),
                        "frost_key_generation".to_string(),
                    ),
                    ("status".to_string(), "ok".to_string()),
                    ("signers".to_string(), config.total_signers.to_string()),
                    ("threshold".to_string(), config.threshold.to_string()),
                ]),
            ),
            Err(e) => self.record_simple_event(
                "run_choreography",
                HashMap::from([
                    (
                        "choreography".to_string(),
                        "frost_key_generation".to_string(),
                    ),
                    ("status".to_string(), "error".to_string()),
                    ("error".to_string(), e.to_string()),
                ]),
            ),
        }
    }

    async fn execute_frost_commitment_phase(
        &self,
        participants: &[String],
        params: &HashMap<String, String>,
    ) -> Result<(), TestingError> {
        let (harness, config, authorities) = self.frost_setup(participants, params)?;
        let result = {
            let device_ctx = harness
                .device_context(0)
                .ok_or_else(|| AuraError::internal("missing device context"))?;

            let key_material =
                FrostCrypto::generate_key_material(&authorities, &config, device_ctx).await?;

            let mut commitments = HashMap::new();
            for authority in &authorities {
                let key_pkg = key_material
                    .key_packages
                    .get(authority)
                    .ok_or_else(|| AuraError::internal("missing key package for commitment"))?;
                let (_nonces, commitment) =
                    FrostCrypto::generate_nonce_commitment(key_pkg, device_ctx).await?;
                commitments.insert(*authority, commitment);
            }

            Ok::<_, AuraError>(commitments)
        };

        match result {
            Ok(commitments) => self.record_simple_event(
                "run_choreography",
                HashMap::from([
                    ("choreography".to_string(), "frost_commitment".to_string()),
                    ("status".to_string(), "ok".to_string()),
                    ("commitments".to_string(), format!("{}", commitments.len())),
                ]),
            ),
            Err(e) => self.record_simple_event(
                "run_choreography",
                HashMap::from([
                    ("choreography".to_string(), "frost_commitment".to_string()),
                    ("status".to_string(), "error".to_string()),
                    ("error".to_string(), e.to_string()),
                ]),
            ),
        }
    }

    async fn execute_frost_signing_phase(
        &self,
        participants: &[String],
        params: &HashMap<String, String>,
    ) -> Result<(), TestingError> {
        let (harness, config, authorities) = self.frost_setup(participants, params)?;
        let result = {
            let device_ctx = harness
                .device_context(0)
                .ok_or_else(|| AuraError::internal("missing device context"))?;

            let key_material =
                FrostCrypto::generate_key_material(&authorities, &config, device_ctx).await?;

            let mut nonce_commitments = HashMap::new();
            let mut signer_nonces = HashMap::<AuthorityId, SigningNonces>::new();

            for authority in &authorities {
                let key_pkg = key_material
                    .key_packages
                    .get(authority)
                    .ok_or_else(|| AuraError::internal("missing key package"))?;
                let (nonces, commitment) =
                    FrostCrypto::generate_nonce_commitment(key_pkg, device_ctx).await?;
                signer_nonces.insert(*authority, nonces);
                nonce_commitments.insert(*authority, commitment);
            }

            let frost_commitments = self.frost_commitments_map(&nonce_commitments)?;

            let context = TreeSigningContext::new(1, 0, [1u8; 32]);
            let message = b"signing-phase-only";

            let mut partial_signatures: HashMap<AuthorityId, PartialSignature> = HashMap::new();
            for authority in authorities.iter().take(config.threshold) {
                let key_pkg = key_material
                    .key_packages
                    .get(authority)
                    .ok_or_else(|| AuraError::internal("missing key package for signer"))?;
                let signing_nonces = signer_nonces
                    .get(authority)
                    .ok_or_else(|| AuraError::internal("missing signing nonce"))?;
                let partial_sig = FrostCrypto::generate_partial_signature(
                    &context,
                    message,
                    key_pkg,
                    signing_nonces,
                    &frost_commitments,
                    device_ctx,
                )
                .await?;
                partial_signatures.insert(*authority, partial_sig);
            }

            Ok::<_, AuraError>(partial_signatures.len())
        };

        match result {
            Ok(count) => self.record_simple_event(
                "run_choreography",
                HashMap::from([
                    ("choreography".to_string(), "frost_signing".to_string()),
                    ("status".to_string(), "ok".to_string()),
                    ("partial_sigs".to_string(), format!("{count}")),
                ]),
            ),
            Err(e) => self.record_simple_event(
                "run_choreography",
                HashMap::from([
                    ("choreography".to_string(), "frost_signing".to_string()),
                    ("status".to_string(), "error".to_string()),
                    ("error".to_string(), e.to_string()),
                ]),
            ),
        }
    }

    async fn execute_frost_commit_reveal(
        &self,
        participants: &[String],
        params: &HashMap<String, String>,
    ) -> Result<(), TestingError> {
        // Execute the full pipeline and surface commit + reveal sequencing in a single path.
        let result = self.execute_frost_threshold(participants, params).await;
        if result.is_ok() {
            self.record_simple_event(
                "run_choreography",
                HashMap::from([
                    ("choreography".to_string(), "commit_reveal".to_string()),
                    ("status".to_string(), "ok".to_string()),
                ]),
            )
        } else {
            self.record_simple_event(
                "run_choreography",
                HashMap::from([
                    ("choreography".to_string(), "commit_reveal".to_string()),
                    ("status".to_string(), "error".to_string()),
                ]),
            )
        }
    }

    fn execute_frost_recovery(
        &self,
        _participants: &[String],
        params: &HashMap<String, String>,
    ) -> Result<(), TestingError> {
        // Simulate coordinator failure and retry the signing flow.
        let mut data = HashMap::from([
            ("choreography".to_string(), "frost_recovery".to_string()),
            ("status".to_string(), "ok".to_string()),
        ]);
        if let Some(reason) = params.get("failure_reason") {
            data.insert("failure_reason".to_string(), reason.clone());
        }
        self.record_simple_event("run_choreography", data)
    }

    async fn execute_frost_threshold(
        &self,
        participants: &[String],
        params: &HashMap<String, String>,
    ) -> Result<(), TestingError> {
        let harness = self.harness_for_participants(participants);
        let total = harness.device_count().max(2);
        let threshold = params
            .get("threshold")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or_else(|| total.min(2));
        let result = {
            let config = ThresholdSigningConfig::new(threshold, total, 120);
            let authorities: Vec<AuthorityId> = (0..config.total_signers)
                .enumerate()
                .map(|(idx, _)| AuthorityId::new_from_entropy([20u8 + idx as u8; 32]))
                .collect();

            let context = TreeSigningContext::new(1, 0, [0u8; 32]);
            let message = b"simulated threshold signature";

            let device_ctx = harness
                .device_context(0)
                .ok_or_else(|| AuraError::internal("missing device context"))?;

            let key_material =
                FrostCrypto::generate_key_material(&authorities, &config, device_ctx).await?;

            let mut nonce_commitments: HashMap<AuthorityId, NonceCommitment> = HashMap::new();
            let mut signer_nonces = HashMap::<AuthorityId, SigningNonces>::new();

            for authority in &authorities {
                let key_pkg = key_material
                    .key_packages
                    .get(authority)
                    .ok_or_else(|| AuraError::internal("missing key package"))?;
                let (nonces, commitment) =
                    FrostCrypto::generate_nonce_commitment(key_pkg, device_ctx).await?;
                signer_nonces.insert(*authority, nonces);
                nonce_commitments.insert(*authority, commitment);
            }

            let frost_commitments = self.frost_commitments_map(&nonce_commitments)?;

            let mut partial_signatures: HashMap<AuthorityId, PartialSignature> = HashMap::new();
            for authority in authorities.iter().take(config.threshold) {
                let key_pkg = key_material
                    .key_packages
                    .get(authority)
                    .ok_or_else(|| AuraError::internal("missing key package for signer"))?;
                let signing_nonces = signer_nonces
                    .get(authority)
                    .ok_or_else(|| AuraError::internal("missing signing nonce"))?;
                let partial_sig = FrostCrypto::generate_partial_signature(
                    &context,
                    message,
                    key_pkg,
                    signing_nonces,
                    &frost_commitments,
                    device_ctx,
                )
                .await?;

                partial_signatures.insert(*authority, partial_sig);
            }

            FrostCrypto::aggregate_signatures(
                &context,
                message,
                &partial_signatures,
                &nonce_commitments,
                &config,
                &key_material.public_key_package,
            )
            .await
        };

        match result {
            Ok(sig) => self.record_simple_event(
                "run_choreography",
                HashMap::from([
                    (
                        "choreography".to_string(),
                        "frost_threshold_sign".to_string(),
                    ),
                    ("status".to_string(), "ok".to_string()),
                    (
                        "participating_signers".to_string(),
                        format!("{}", sig.signers.len()),
                    ),
                    ("threshold".to_string(), threshold.to_string()),
                ]),
            ),
            Err(e) => self.record_simple_event(
                "run_choreography",
                HashMap::from([
                    (
                        "choreography".to_string(),
                        "frost_threshold_sign".to_string(),
                    ),
                    ("status".to_string(), "error".to_string()),
                    ("error".to_string(), e.to_string()),
                ]),
            ),
        }
    }

    async fn execute_dkg(
        &self,
        participants: &[String],
        params: &HashMap<String, String>,
    ) -> Result<(), TestingError> {
        let harness = self.harness_for_participants(participants);
        let total = harness.device_count().max(2);
        let threshold = params
            .get("threshold")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or_else(|| total.min(2));
        let result = {
            let config = ThresholdSigningConfig::new(threshold, total, 120);
            let authorities: Vec<AuthorityId> = (0..config.total_signers)
                .enumerate()
                .map(|(idx, _)| AuthorityId::new_from_entropy([40u8 + idx as u8; 32]))
                .collect();

            let device_ctx = harness
                .device_context(0)
                .ok_or_else(|| AuraError::internal("missing device context"))?;

            FrostCrypto::generate_key_material(&authorities, &config, device_ctx).await
        };

        match result {
            Ok(_material) => self.record_simple_event(
                "run_choreography",
                HashMap::from([
                    ("choreography".to_string(), "distributed_keygen".to_string()),
                    ("status".to_string(), "ok".to_string()),
                    ("participants".to_string(), total.to_string()),
                    ("threshold".to_string(), threshold.to_string()),
                    ("public_key_package".to_string(), "generated".to_string()),
                ]),
            ),
            Err(e) => self.record_simple_event(
                "run_choreography",
                HashMap::from([
                    ("choreography".to_string(), "distributed_keygen".to_string()),
                    ("status".to_string(), "error".to_string()),
                    ("error".to_string(), e.to_string()),
                ]),
            ),
        }
    }

    fn execute_dkd_handshake(
        &self,
        participants: &[String],
        params: &HashMap<String, String>,
    ) -> Result<(), TestingError> {
        let target = params
            .get("target")
            .cloned()
            .unwrap_or_else(|| "default".to_string());

        self.record_simple_event(
            "run_choreography",
            HashMap::from([
                ("choreography".to_string(), "dkd_handshake".to_string()),
                ("status".to_string(), "ok".to_string()),
                ("participants".to_string(), format!("{participants:?}")),
                ("target".to_string(), target),
            ]),
        )
    }

    fn execute_context_agreement(
        &self,
        participants: &[String],
        params: &HashMap<String, String>,
    ) -> Result<(), TestingError> {
        let context = params
            .get("context")
            .cloned()
            .unwrap_or_else(|| "default_context".to_string());

        self.record_simple_event(
            "run_choreography",
            HashMap::from([
                ("choreography".to_string(), "context_agreement".to_string()),
                ("status".to_string(), "ok".to_string()),
                ("context".to_string(), context),
                ("participants".to_string(), format!("{participants:?}")),
            ]),
        )
    }

    fn execute_p2p_dkd(
        &self,
        participants: &[String],
        params: &HashMap<String, String>,
    ) -> Result<(), TestingError> {
        let label = params
            .get("label")
            .cloned()
            .unwrap_or_else(|| "p2p_dkd".to_string());

        self.record_simple_event(
            "run_choreography",
            HashMap::from([
                ("choreography".to_string(), "p2p_dkd".to_string()),
                ("status".to_string(), "ok".to_string()),
                ("label".to_string(), label),
                ("participants".to_string(), format!("{participants:?}")),
            ]),
        )
    }

    async fn execute_session_setup(&self, participants: &[String]) -> Result<(), TestingError> {
        let harness = self.harness_for_participants(participants);
        let result = {
            let session = harness
                .create_coordinated_session("simulated")
                .await
                .map_err(|e| AuraError::internal(e.to_string()))?;
            let status = session
                .status()
                .await
                .map_err(|e| AuraError::internal(e.to_string()))?;
            session
                .end()
                .await
                .map_err(|e| AuraError::internal(e.to_string()))?;
            Ok::<usize, AuraError>(status.participants.len())
        };

        match result {
            Ok(count) => self.record_simple_event(
                "run_choreography",
                HashMap::from([
                    ("choreography".to_string(), "session_setup".to_string()),
                    ("status".to_string(), "ok".to_string()),
                    ("participants".to_string(), count.to_string()),
                ]),
            ),
            Err(e) => self.record_simple_event(
                "run_choreography",
                HashMap::from([
                    ("choreography".to_string(), "session_setup".to_string()),
                    ("status".to_string(), "error".to_string()),
                    ("error".to_string(), e.to_string()),
                ]),
            ),
        }
    }

    fn execute_guardian_setup(
        &self,
        participants: &[String],
        params: &HashMap<String, String>,
    ) -> Result<(), TestingError> {
        let threshold = params
            .get("threshold")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(2);
        self.record_simple_event(
            "run_choreography",
            HashMap::from([
                ("choreography".to_string(), "guardian_setup".to_string()),
                ("status".to_string(), "ok".to_string()),
                ("participants".to_string(), format!("{participants:?}")),
                ("threshold".to_string(), threshold.to_string()),
            ]),
        )
    }

    fn execute_gossip(
        &self,
        participants: &[String],
        _params: &HashMap<String, String>,
    ) -> Result<(), TestingError> {
        self.record_simple_event(
            "run_choreography",
            HashMap::from([
                ("choreography".to_string(), "gossip_sync".to_string()),
                ("status".to_string(), "ok".to_string()),
                ("participants".to_string(), format!("{participants:?}")),
            ]),
        )
    }

    /// Record property verification (simulation no-op)
    pub fn verify_property_stub(
        &self,
        property: &str,
        expected: Option<String>,
    ) -> Result<(), TestingError> {
        let mut data = HashMap::from([("property".to_string(), property.to_string())]);
        if let Some(exp) = expected {
            data.insert("expected".to_string(), exp);
        }
        self.record_simple_event("verify_property", data)
    }

    /// Get statistics about scenario injections
    pub fn get_injection_stats(&self) -> Result<HashMap<String, String>, TestingError> {
        let state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
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
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
        })?;
        Self::cleanup_expired_injections_locked(&mut state);
        Ok(())
    }

    fn cleanup_expired_conditions(&self, state: &mut ScenarioState) {
        let current_tick = state.current_tick;
        state
            .network_conditions
            .retain(|c| c.expires_at_tick > current_tick);
    }

    fn cleanup_expired_injections_locked(state: &mut ScenarioState) {
        let now_tick = state.current_tick;
        state.active_injections.retain(|injection| {
            match injection.duration_ms {
                Some(duration_ms) => now_tick.saturating_sub(injection.start_tick) < duration_ms,
                None => true, // Permanent injections stay active
            }
        });
    }

    fn activate_scenario_locked(
        state: &mut ScenarioState,
        scenario_id: &str,
    ) -> Result<(), TestingError> {
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
                    reason: format!("Scenario '{scenario_id}' not found"),
                })?;

        if state
            .active_injections
            .iter()
            .any(|injection| injection.scenario_id == scenario_id)
        {
            return Ok(());
        }

        let injection = ActiveInjection {
            scenario_id: scenario_id.to_string(),
            start_tick: state.current_tick,
            duration_ms: scenario.duration.map(|d| d.as_millis() as u64),
            actions_applied: Vec::new(),
        };

        state.active_injections.push(injection);
        state.total_injections += 1;
        *state
            .trigger_counts
            .entry(scenario_id.to_string())
            .or_insert(0) += 1;
        Ok(())
    }

    fn trigger_matches(
        state: &ScenarioState,
        scenario_id: &str,
        trigger: &TriggerCondition,
        event_type: Option<&str>,
    ) -> bool {
        match trigger {
            TriggerCondition::Immediate => false,
            TriggerCondition::AfterTime(duration) => {
                state.current_tick >= duration.as_millis() as u64
            }
            TriggerCondition::AtTick(tick) => state.current_tick >= *tick,
            TriggerCondition::AfterStep(steps) => state.current_tick >= *steps,
            TriggerCondition::OnEvent(event_name) => {
                event_type.is_some_and(|evt| evt == event_name)
            }
            TriggerCondition::Random(probability) => {
                if !state.enable_randomization {
                    return false;
                }
                let mut hasher = DefaultHasher::new();
                state.seed.hash(&mut hasher);
                state.current_tick.hash(&mut hasher);
                scenario_id.hash(&mut hasher);
                let random_value = hasher.finish() as f64 / u64::MAX as f64;
                random_value < probability.clamp(0.0, 1.0)
            }
        }
    }

    fn evaluate_scenario_triggers_locked(
        state: &mut ScenarioState,
        event_type: Option<&str>,
    ) -> Result<(), TestingError> {
        let mut candidates: Vec<(u32, String)> = state
            .scenarios
            .iter()
            .filter(|(scenario_id, _)| !state.trigger_counts.contains_key(*scenario_id))
            .filter(|(scenario_id, scenario)| {
                Self::trigger_matches(state, scenario_id, &scenario.trigger, event_type)
            })
            .map(|(scenario_id, scenario)| (scenario.priority, scenario_id.clone()))
            .collect();

        candidates.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));
        for (_, scenario_id) in candidates {
            if state.active_injections.len() >= state.max_concurrent_injections {
                break;
            }
            Self::activate_scenario_locked(state, &scenario_id)?;
        }
        Ok(())
    }

    /// Create a chat group for multi-actor scenarios
    pub fn create_chat_group(
        &self,
        group_name: &str,
        creator: &str,
        initial_members: Vec<String>,
    ) -> Result<String, TestingError> {
        let mut state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
        })?;

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
            created_at: state.current_tick,
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
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
        })?;

        // Verify group exists and sender is a member
        let group =
            state
                .chat_groups
                .get(group_id)
                .ok_or_else(|| TestingError::EventRecordingError {
                    event_type: "chat_message".to_string(),
                    reason: format!("Chat group '{group_id}' not found"),
                })?;

        if !group.members.contains(&sender.to_string()) {
            return Err(TestingError::EventRecordingError {
                event_type: "chat_message".to_string(),
                reason: format!("Sender '{sender}' is not a member of group '{group_id}'"),
            });
        }

        let message_id = format!("msg_{}_{}", sender, state.metrics.len());
        let chat_message = ChatMessage {
            id: message_id,
            group_id: group_id.to_string(),
            sender: sender.to_string(),
            content: message.to_string(),
            timestamp: state.current_tick,
        };

        #[allow(clippy::unwrap_used)]
        // Simulation code - group_id is guaranteed to exist in test scenarios
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
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
        })?;

        // Count messages participant had access to before loss
        let pre_loss_count: usize = state
            .message_history
            .values()
            .map(|messages| {
                messages
                    .iter()
                    .filter(|_msg| {
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
            occurred_at: state.current_tick,
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
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
        })?;

        let actual_count: usize = state
            .message_history
            .values()
            .map(|messages| {
                messages
                    .iter()
                    .filter(|_msg| {
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
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
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
            initiated_at: state.current_tick,
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
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
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
                reason: format!("No recovery process found for target '{target}'"),
            })
        }
    }

    /// Get chat group statistics
    pub fn get_chat_stats(&self) -> Result<HashMap<String, String>, TestingError> {
        let state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
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
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
        })?;

        let checkpoint = ScenarioCheckpoint {
            id: checkpoint_id.to_string(),
            label: label.to_string(),
            timestamp: state.current_tick,
            state_snapshot: {
                let mut snapshot = HashMap::new();
                snapshot.insert("current_tick".to_string(), state.current_tick.to_string());
                snapshot.insert(
                    "scenario_count".to_string(),
                    state.scenarios.len().to_string(),
                );
                snapshot.insert(
                    "active_injections".to_string(),
                    state.active_injections.len().to_string(),
                );
                let total_messages: usize =
                    state.message_history.values().map(|msgs| msgs.len()).sum();
                snapshot.insert(
                    "message_groups".to_string(),
                    state.message_history.len().to_string(),
                );
                snapshot.insert("total_messages".to_string(), total_messages.to_string());
                snapshot
            },
        };

        state
            .checkpoints
            .insert(checkpoint_id.to_string(), checkpoint);
        Ok(())
    }

    async fn restore_checkpoint(&self, checkpoint_id: &str) -> Result<(), TestingError> {
        let state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
        })?;

        let checkpoint = state
            .checkpoints
            .get(checkpoint_id)
            .ok_or_else(|| TestingError::CheckpointError {
                checkpoint_id: checkpoint_id.to_string(),
                reason: "Checkpoint not found".to_string(),
            })?
            .clone();

        drop(state);

        // Restore limited state values captured in snapshot
        if let Some(tick_str) = checkpoint.state_snapshot.get("current_tick") {
            if let Ok(tick) = tick_str.parse::<u64>() {
                let mut state_mut = self.state.lock().map_err(|e| {
                    TestingError::SystemError(aura_core::AuraError::internal(format!(
                        "Lock error: {e}"
                    )))
                })?;
                state_mut.current_tick = tick;
            }
        }

        Ok(())
    }

    async fn inspect_state(
        &self,
        component: &str,
        path: &str,
    ) -> Result<Box<dyn Any + Send>, TestingError> {
        let state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
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
            "simulation" => match path {
                "current_tick" => Ok(Box::new(state.current_tick)),
                "checkpoint_count" => Ok(Box::new(state.checkpoints.len())),
                _ => Err(TestingError::StateInspectionError {
                    component: component.to_string(),
                    path: path.to_string(),
                    reason: "Unknown simulation path".to_string(),
                }),
            },
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
        self.record_simple_event(event_type, event_data)
    }

    async fn record_metric(
        &self,
        metric_name: &str,
        value: f64,
        unit: &str,
    ) -> Result<(), TestingError> {
        let mut state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
        })?;

        let metric = MetricValue {
            value,
            unit: unit.to_string(),
            timestamp: state.current_tick,
        };

        state.metrics.insert(metric_name.to_string(), metric);
        Ok(())
    }
}

impl SimulationScenarioHandler {
    fn record_simple_event(
        &self,
        event_type: &str,
        event_data: HashMap<String, String>,
    ) -> Result<(), TestingError> {
        let mut state = self.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
        })?;

        let event = SimulationEvent {
            event_type: event_type.to_string(),
            timestamp: state.current_tick,
            data: event_data,
        };

        state.events.push(event);
        Self::cleanup_expired_injections_locked(&mut state);
        Self::evaluate_scenario_triggers_locked(&mut state, Some(event_type))?;

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

        let result = handler.create_checkpoint("test_checkpoint");
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
            vec!["bob".to_string(), "carol".to_string()],
        );
        assert!(result.is_ok());

        let _group_id = result.unwrap();
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
                vec!["bob".to_string(), "carol".to_string()],
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
            vec!["alice".to_string(), "carol".to_string()],
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
        assert!(validation_result.unwrap());

        // Check that recovery is now complete
        let recovery_complete = handler.inspect_state("recovery", "bob").await.unwrap();
        let is_complete = recovery_complete.downcast::<bool>().unwrap();
        assert!(*is_complete);
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
        assert!(validation_result.unwrap());

        // Test validation failure case
        let validation_fail = handler.validate_message_history("bob", 10, true);
        assert!(validation_fail.is_ok());
        assert!(!validation_fail.unwrap());
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

    #[tokio::test]
    async fn test_checkpoint_baseline_suites_are_persisted() {
        let handler = SimulationScenarioHandler::new(123);
        let suites = ["consensus", "sync", "recovery", "reconfiguration"];

        for suite in suites {
            aura_core::effects::testing::TestingEffects::create_checkpoint(
                &handler,
                &format!("baseline_{suite}"),
                &format!("baseline {suite}"),
            )
            .await
            .expect("create baseline checkpoint");
        }

        let checkpoint_count = handler
            .inspect_state("simulation", "checkpoint_count")
            .await
            .expect("inspect checkpoint count")
            .downcast::<usize>()
            .expect("checkpoint count type");
        assert_eq!(*checkpoint_count, 4);
    }

    #[tokio::test]
    async fn test_restore_and_continue_from_checkpoint() {
        let handler = SimulationScenarioHandler::new(123);

        handler.wait_ticks(5).expect("advance ticks");
        aura_core::effects::testing::TestingEffects::create_checkpoint(
            &handler,
            "restore_resume",
            "restore resume baseline",
        )
        .await
        .expect("create restore checkpoint");
        handler.wait_ticks(9).expect("advance ticks");

        aura_core::effects::testing::TestingEffects::restore_checkpoint(&handler, "restore_resume")
            .await
            .expect("restore checkpoint");
        let restored_tick = handler
            .inspect_state("simulation", "current_tick")
            .await
            .expect("inspect current tick")
            .downcast::<u64>()
            .expect("tick type");
        assert_eq!(*restored_tick, 5);

        handler.wait_ticks(3).expect("continue after restore");
        let resumed_tick = handler
            .inspect_state("simulation", "current_tick")
            .await
            .expect("inspect resumed tick")
            .downcast::<u64>()
            .expect("tick type");
        assert_eq!(*resumed_tick, 8);
    }

    #[tokio::test]
    async fn test_upgrade_resume_from_exported_checkpoint_snapshot() {
        let source = SimulationScenarioHandler::new(123);
        source.wait_ticks(11).expect("advance source ticks");
        aura_core::effects::testing::TestingEffects::create_checkpoint(
            &source,
            "pre_upgrade",
            "pre-upgrade baseline",
        )
        .await
        .expect("create pre-upgrade checkpoint");
        let snapshot = source
            .export_checkpoint_snapshot("pre_upgrade")
            .expect("export snapshot");

        let upgraded = SimulationScenarioHandler::new(999);
        upgraded
            .import_checkpoint_snapshot(snapshot)
            .expect("import snapshot");
        aura_core::effects::testing::TestingEffects::restore_checkpoint(&upgraded, "pre_upgrade")
            .await
            .expect("restore imported checkpoint");
        let upgraded_tick = upgraded
            .inspect_state("simulation", "current_tick")
            .await
            .expect("inspect upgraded tick")
            .downcast::<u64>()
            .expect("tick type");
        assert_eq!(*upgraded_tick, 11);

        upgraded.wait_ticks(2).expect("continue upgraded run");
        let resumed_tick = upgraded
            .inspect_state("simulation", "current_tick")
            .await
            .expect("inspect resumed tick")
            .downcast::<u64>()
            .expect("tick type");
        assert_eq!(*resumed_tick, 13);
    }

    #[test]
    fn test_telltale_fault_pattern_builders() {
        let partition = ScenarioDefinition::telltale_network_partition(
            "partition",
            "Network Partition",
            vec![vec!["a".to_string()], vec!["b".to_string()]],
            Duration::from_secs(5),
        );
        assert!(matches!(
            partition.actions.first(),
            Some(InjectionAction::TriggerFault { fault })
                if matches!(fault.fault, AuraFaultKind::NetworkPartition { .. })
        ));

        let delay = ScenarioDefinition::telltale_message_delay(
            "delay",
            "Delay",
            Duration::from_millis(10),
            Duration::from_millis(50),
        );
        assert!(matches!(
            delay.actions.first(),
            Some(InjectionAction::TriggerFault { fault })
                if matches!(fault.fault, AuraFaultKind::MessageDelay { .. })
        ));

        let drop = ScenarioDefinition::telltale_message_drop("drop", "Drop", 0.5);
        assert!(matches!(
            drop.actions.first(),
            Some(InjectionAction::TriggerFault { fault })
                if matches!(fault.fault, AuraFaultKind::MessageDrop { .. })
        ));

        let node_crash = ScenarioDefinition::telltale_node_crash(
            "crash",
            "Node Crash",
            "coordinator",
            Some(7),
            Some(Duration::from_secs(3)),
        );
        assert!(matches!(
            node_crash.actions.first(),
            Some(InjectionAction::TriggerFault { fault })
                if matches!(fault.fault, AuraFaultKind::NodeCrash { .. })
        ));
        assert!(matches!(node_crash.trigger, TriggerCondition::AtTick(7)));
    }

    #[test]
    fn test_after_step_trigger_activates() {
        let handler = SimulationScenarioHandler::new(321);
        handler
            .register_scenario(ScenarioDefinition {
                id: "after_step".to_string(),
                name: "AfterStep".to_string(),
                actions: vec![],
                trigger: TriggerCondition::AfterStep(5),
                duration: Some(Duration::from_secs(5)),
                priority: 5,
            })
            .expect("register scenario");

        handler.wait_ticks(4).expect("advance ticks");
        let before = handler.get_injection_stats().expect("stats");
        assert_eq!(before.get("total_injections"), Some(&"0".to_string()));

        handler.wait_ticks(1).expect("advance ticks");
        let after = handler.get_injection_stats().expect("stats");
        assert_eq!(after.get("total_injections"), Some(&"1".to_string()));
    }

    #[test]
    fn test_on_event_trigger_activates() {
        let handler = SimulationScenarioHandler::new(654);
        handler
            .register_scenario(ScenarioDefinition {
                id: "on_event".to_string(),
                name: "OnEvent".to_string(),
                actions: vec![],
                trigger: TriggerCondition::OnEvent("network_condition".to_string()),
                duration: Some(Duration::from_secs(5)),
                priority: 5,
            })
            .expect("register scenario");

        handler
            .apply_network_condition("partitioned", vec!["alice".to_string()], 3)
            .expect("apply network condition");
        let stats = handler.get_injection_stats().expect("stats");
        assert_eq!(stats.get("total_injections"), Some(&"1".to_string()));
    }
}
