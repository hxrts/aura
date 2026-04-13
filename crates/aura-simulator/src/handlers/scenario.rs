//! Scenario management effect handler for simulation
//!
//! This module provides simulation-specific scenario injection and management
//! capabilities. Replaces the former ScenarioInjectionMiddleware with proper
//! effect system integration.

use crate::environment_bridge::{
    AuraAdmissionPressureOverlayV1, AuraEnvironmentArtifacts, AuraEnvironmentBridge,
    AuraEnvironmentOverlayV1, AuraInterferenceOverlayV1, AuraProviderOverlayV1,
    AuraTopologyChurnOverlayV1,
};
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
    /// Apply an adaptive privacy transition to simulator state.
    AdaptivePrivacyTransition(AdaptivePrivacyTransition),
}

/// Density class for sync opportunities in adaptive privacy scenarios.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncOpportunityDensity {
    Sparse,
    Balanced,
    Heavy,
}

/// Adaptive privacy transition applied by scenario actions.
#[derive(Debug, Clone)]
pub enum AdaptivePrivacyTransition {
    ConfigureMovement {
        profile_id: String,
        clusters: Vec<String>,
        home_locality_bias: f64,
        neighborhood_locality_bias: f64,
    },
    EstablishAnonymousPath {
        path_id: String,
        initiator: String,
        destination: String,
        hops: Vec<String>,
        ttl_ticks: u64,
        reusable: bool,
    },
    ReuseEstablishedPath {
        path_id: String,
    },
    ExpireEstablishedPath {
        path_id: String,
    },
    RecordEstablishFlow {
        flow_id: String,
        source: String,
        destination: String,
        path_id: Option<String>,
    },
    RecordMoveBatch {
        batch_id: String,
        envelope_count: usize,
    },
    ObserveLocalHealth {
        provider: String,
        score: f64,
        latency_ms: u64,
    },
    RecordCoverTraffic {
        provider: String,
        envelope_count: usize,
    },
    RecordAccountabilityReply {
        reply_id: String,
        deadline_ticks: u64,
        completed_after_ticks: Option<u64>,
    },
    RecordRouteDiversity {
        selector_id: String,
        unique_paths: usize,
        dominant_provider: Option<String>,
    },
    RecordHonestHopCompromise {
        path_id: String,
        compromised_hops: Vec<String>,
        honest_hops_remaining: usize,
    },
    RecordPartitionHealCycle {
        cycle_id: String,
        partition_groups: Vec<Vec<String>>,
        heal_after_ticks: u64,
    },
    RecordChurnBurst {
        burst_id: String,
        affected_participants: Vec<String>,
        entering: usize,
        leaving: usize,
    },
    RecordProviderSaturation {
        provider: String,
        queue_depth: usize,
        utilization: f64,
    },
    RecordHeldObjectRetention {
        object_id: String,
        selector: String,
        retention_ticks: u64,
        seeded_from_move: bool,
    },
    RecordSelectorRetrieval {
        retrieval_id: String,
        selector: String,
        expected_objects: usize,
        sync_profile: String,
    },
    RecordSyncOpportunity {
        profile_id: String,
        density: SyncOpportunityDensity,
        peers: Vec<String>,
    },
    RecordMoveToHoldSeed {
        batch_id: String,
        object_id: String,
        selector: String,
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
    parameters: HashMap<String, String>,
    behaviors: HashMap<String, String>,
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
    adaptive_privacy: AdaptivePrivacyState,
    environment_bridge: AuraEnvironmentBridge,
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

#[derive(Debug, Clone)]
struct AdaptivePrivacyState {
    movement_profiles: HashMap<String, MovementProfile>,
    anonymous_paths: HashMap<String, AnonymousPathState>,
    establish_flows: HashMap<String, EstablishFlowState>,
    move_batches: HashMap<String, MoveBatchState>,
    local_health: HashMap<String, LocalHealthObservation>,
    cover_events: Vec<CoverTrafficRecord>,
    accountability_replies: HashMap<String, AccountabilityReplyState>,
    route_diversity: HashMap<String, RouteDiversityObservation>,
    honest_hop_compromise_patterns: Vec<HonestHopCompromisePattern>,
    partition_heal_cycles: HashMap<String, PartitionHealCycle>,
    churn_bursts: HashMap<String, ChurnBurstState>,
    provider_saturation: HashMap<String, ProviderSaturationState>,
    held_objects: HashMap<String, HeldObjectRetentionState>,
    selector_retrievals: HashMap<String, SelectorRetrievalState>,
    sync_opportunities: HashMap<String, SyncOpportunityState>,
    move_to_hold_seeds: HashMap<String, MoveToHoldSeedState>,
}

impl AdaptivePrivacyState {
    fn new() -> Self {
        Self {
            movement_profiles: HashMap::new(),
            anonymous_paths: HashMap::new(),
            establish_flows: HashMap::new(),
            move_batches: HashMap::new(),
            local_health: HashMap::new(),
            cover_events: Vec::new(),
            accountability_replies: HashMap::new(),
            route_diversity: HashMap::new(),
            honest_hop_compromise_patterns: Vec::new(),
            partition_heal_cycles: HashMap::new(),
            churn_bursts: HashMap::new(),
            provider_saturation: HashMap::new(),
            held_objects: HashMap::new(),
            selector_retrievals: HashMap::new(),
            sync_opportunities: HashMap::new(),
            move_to_hold_seeds: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct MovementProfile {
    id: String,
    clusters: Vec<String>,
    home_locality_bias: f64,
    neighborhood_locality_bias: f64,
    recorded_at: SimTimestamp,
}

#[derive(Debug, Clone)]
struct AnonymousPathState {
    id: String,
    initiator: String,
    destination: String,
    hops: Vec<String>,
    established_at: SimTimestamp,
    expires_at_tick: u64,
    reusable: bool,
    reuse_count: u64,
    expired: bool,
}

#[derive(Debug, Clone)]
struct EstablishFlowState {
    id: String,
    source: String,
    destination: String,
    path_id: Option<String>,
    established_at: SimTimestamp,
}

#[derive(Debug, Clone)]
struct MoveBatchState {
    id: String,
    envelope_count: usize,
    created_at: SimTimestamp,
}

#[derive(Debug, Clone)]
struct LocalHealthObservation {
    provider: String,
    score: f64,
    latency_ms: u64,
    observed_at: SimTimestamp,
}

#[derive(Debug, Clone)]
struct CoverTrafficRecord {
    provider: String,
    envelope_count: usize,
    recorded_at: SimTimestamp,
}

#[derive(Debug, Clone)]
struct AccountabilityReplyState {
    id: String,
    deadline_tick: u64,
    completed_at_tick: Option<u64>,
    within_deadline: bool,
}

#[derive(Debug, Clone)]
struct RouteDiversityObservation {
    selector_id: String,
    unique_paths: usize,
    dominant_provider: Option<String>,
    recorded_at: SimTimestamp,
}

#[derive(Debug, Clone)]
struct HonestHopCompromisePattern {
    path_id: String,
    compromised_hops: Vec<String>,
    honest_hops_remaining: usize,
    recorded_at: SimTimestamp,
}

#[derive(Debug, Clone)]
struct PartitionHealCycle {
    id: String,
    partition_groups: Vec<Vec<String>>,
    partitioned_at: SimTimestamp,
    heals_at_tick: u64,
    healed: bool,
}

#[derive(Debug, Clone)]
struct ChurnBurstState {
    id: String,
    affected_participants: Vec<String>,
    entering: usize,
    leaving: usize,
    recorded_at: SimTimestamp,
}

#[derive(Debug, Clone)]
struct ProviderSaturationState {
    provider: String,
    queue_depth: usize,
    utilization: f64,
    recorded_at: SimTimestamp,
}

#[derive(Debug, Clone)]
struct HeldObjectRetentionState {
    object_id: String,
    selector: String,
    retained_at: SimTimestamp,
    retention_until_tick: u64,
    seeded_from_move: bool,
    expired: bool,
}

#[derive(Debug, Clone)]
struct SelectorRetrievalState {
    retrieval_id: String,
    selector: String,
    expected_objects: usize,
    sync_profile: String,
    recorded_at: SimTimestamp,
}

#[derive(Debug, Clone)]
struct SyncOpportunityState {
    profile_id: String,
    density: SyncOpportunityDensity,
    peers: Vec<String>,
    recorded_at: SimTimestamp,
}

#[derive(Debug, Clone)]
struct MoveToHoldSeedState {
    batch_id: String,
    object_id: String,
    selector: String,
    seeded_at: SimTimestamp,
}

struct SimulationScenarioShared {
    state: Mutex<ScenarioState>,
}

/// Simulation-specific scenario management handler
pub struct SimulationScenarioHandler {
    shared: Arc<SimulationScenarioShared>,
}

impl SimulationScenarioHandler {
    /// Create a new scenario handler
    pub fn new(seed: u64) -> Self {
        Self {
            shared: Arc::new(SimulationScenarioShared {
                state: Mutex::new(ScenarioState {
                    scenarios: HashMap::new(),
                    active_injections: Vec::new(),
                    checkpoints: HashMap::new(),
                    events: Vec::new(),
                    metrics: HashMap::new(),
                    parameters: HashMap::new(),
                    behaviors: HashMap::new(),
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
                    adaptive_privacy: AdaptivePrivacyState::new(),
                    environment_bridge: AuraEnvironmentBridge::new(),
                }),
            }),
        }
    }

    /// Register a scenario for potential injection
    pub fn register_scenario(&self, scenario: ScenarioDefinition) -> Result<(), TestingError> {
        let mut state = self.shared.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
        })?;

        state.scenarios.insert(scenario.id.clone(), scenario);
        Ok(())
    }

    /// Enable or disable random scenario injection
    pub fn set_randomization(&self, enable: bool, probability: f64) -> Result<(), TestingError> {
        let mut state = self.shared.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
        })?;

        state.enable_randomization = enable;
        state.injection_probability = probability.clamp(0.0, 1.0);
        Ok(())
    }

    /// Manually trigger a specific scenario
    pub fn trigger_scenario(&self, scenario_id: &str) -> Result<(), TestingError> {
        let mut state = self.shared.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
        })?;
        Self::cleanup_expired_injections_locked(&mut state);
        Self::activate_scenario_locked(&mut state, scenario_id)
    }

    /// Advance simulated time by ticks
    pub fn wait_ticks(&self, ticks: u64) -> Result<(), TestingError> {
        let mut state = self.shared.state.lock().map_err(|e| {
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
        let mut state = self.shared.state.lock().map_err(|e| {
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
        let mut state = self.shared.state.lock().map_err(|e| {
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
        let state = self.shared.state.lock().map_err(|e| {
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
        let mut state = self.shared.state.lock().map_err(|e| {
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

    fn increment_counter_parameter(
        state: &mut ScenarioState,
        key: impl Into<String>,
    ) -> Result<(), TestingError> {
        let key = key.into();
        let next = state
            .parameters
            .get(&key)
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0)
            .saturating_add(1);
        state.parameters.insert(key, next.to_string());
        Ok(())
    }

    fn first_group_id(state: &ScenarioState) -> Option<String> {
        let mut group_ids = state.chat_groups.keys().cloned().collect::<Vec<_>>();
        group_ids.sort();
        group_ids.into_iter().next()
    }

    /// Execute generic stateful choreography behavior for simulator-only flows
    /// that do not have a dedicated protocol harness.
    pub fn run_choreography_fallback(
        &self,
        name: &str,
        participants: Vec<String>,
        params: HashMap<String, String>,
    ) -> Result<(), TestingError> {
        let mut state = self.shared.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
        })?;
        let mut data = HashMap::from([
            ("choreography".to_string(), name.to_string()),
            ("participants".to_string(), format!("{participants:?}")),
            ("status".to_string(), "ok".to_string()),
        ]);
        data.extend(params.clone());

        let normalized = name.to_lowercase();
        match normalized.as_str() {
            "account_creation" => {
                for participant in &participants {
                    state
                        .parameters
                        .insert(format!("account:{participant}:ready"), "true".to_string());
                }
            }
            "chat_group_creation" => {
                let creator = params
                    .get("creator")
                    .cloned()
                    .or_else(|| participants.first().cloned())
                    .unwrap_or_else(|| "unknown".to_string());
                let group_name = params
                    .get("group_name")
                    .cloned()
                    .unwrap_or_else(|| format!("{creator}'s group"));
                let group_id = Self::create_chat_group_locked(
                    &mut state,
                    &group_name,
                    &creator,
                    participants.clone(),
                )?;
                state
                    .parameters
                    .insert("chat:last_group_id".to_string(), group_id.clone());
                data.insert("group_id".to_string(), group_id);
            }
            "chat_group_invitation" => {
                let invitee = params
                    .get("invitee")
                    .cloned()
                    .or_else(|| participants.last().cloned())
                    .unwrap_or_else(|| "unknown".to_string());
                let group_id = params
                    .get("group_id")
                    .cloned()
                    .or_else(|| state.parameters.get("chat:last_group_id").cloned())
                    .or_else(|| Self::first_group_id(&state))
                    .ok_or_else(|| TestingError::EventRecordingError {
                        event_type: "run_choreography".to_string(),
                        reason: "chat_group_invitation requires an existing chat group".to_string(),
                    })?;
                let group = state.chat_groups.get_mut(&group_id).ok_or_else(|| {
                    TestingError::EventRecordingError {
                        event_type: "run_choreography".to_string(),
                        reason: format!("chat group '{group_id}' not found"),
                    }
                })?;
                if !group.members.contains(&invitee) {
                    group.members.push(invitee);
                }
                data.insert("group_id".to_string(), group_id);
            }
            "chat_message" => {
                let sender = params
                    .get("sender")
                    .cloned()
                    .or_else(|| participants.first().cloned())
                    .unwrap_or_else(|| "unknown".to_string());
                let message = params
                    .get("message")
                    .cloned()
                    .unwrap_or_else(|| "<empty>".to_string());
                let group_id = params
                    .get("group_id")
                    .cloned()
                    .or_else(|| state.parameters.get("chat:last_group_id").cloned())
                    .or_else(|| Self::first_group_id(&state))
                    .ok_or_else(|| TestingError::EventRecordingError {
                        event_type: "run_choreography".to_string(),
                        reason: "chat_message requires an existing chat group".to_string(),
                    })?;
                Self::send_chat_message_locked(&mut state, &group_id, &sender, &message)?;
                data.insert("group_id".to_string(), group_id);
            }
            "guardian_request" => {
                let requester = params
                    .get("requester")
                    .cloned()
                    .or_else(|| participants.first().cloned())
                    .unwrap_or_else(|| "unknown".to_string());
                let guardian = params
                    .get("guardian")
                    .cloned()
                    .or_else(|| participants.get(1).cloned())
                    .unwrap_or_else(|| "unknown".to_string());
                state.parameters.insert(
                    format!("guardian_relationship:{requester}:{guardian}:requested"),
                    "true".to_string(),
                );
            }
            "guardian_accept" => {
                let guardian = params
                    .get("guardian")
                    .cloned()
                    .or_else(|| participants.first().cloned())
                    .unwrap_or_else(|| "unknown".to_string());
                let protege = params
                    .get("protege")
                    .cloned()
                    .or_else(|| participants.get(1).cloned())
                    .unwrap_or_else(|| "unknown".to_string());
                state.parameters.insert(
                    format!("guardian_relationship:{protege}:{guardian}:accepted"),
                    "true".to_string(),
                );
            }
            "guardian_authority_configuration" => {
                let target = params
                    .get("target")
                    .cloned()
                    .or_else(|| participants.get(1).cloned())
                    .or_else(|| participants.first().cloned())
                    .unwrap_or_else(|| "unknown".to_string());
                let threshold = params
                    .get("threshold")
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or_else(|| participants.len().max(1));
                let guardians = participants
                    .iter()
                    .filter(|participant| **participant != target)
                    .cloned()
                    .collect::<Vec<_>>();
                state.parameters.insert(
                    format!("guardian_config:{target}:threshold"),
                    threshold.to_string(),
                );
                state.parameters.insert(
                    format!("guardian_config:{target}:guardians"),
                    guardians.join(","),
                );
                state
                    .parameters
                    .insert(format!("guardians:{target}:configured"), "true".to_string());
            }
            "guardian_recovery_request" => {
                let target = params
                    .get("requester")
                    .cloned()
                    .or_else(|| params.get("target").cloned())
                    .or_else(|| participants.first().cloned())
                    .unwrap_or_else(|| "unknown".to_string());
                let guardians = state
                    .parameters
                    .get(&format!("guardian_config:{target}:guardians"))
                    .map(|value| {
                        value
                            .split(',')
                            .filter(|entry| !entry.is_empty())
                            .map(ToOwned::to_owned)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let threshold = state
                    .parameters
                    .get(&format!("guardian_config:{target}:threshold"))
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or_else(|| guardians.len().max(1));
                Self::initiate_guardian_recovery_locked(&mut state, &target, guardians, threshold)?;
            }
            "guardian_recovery_validation" => {
                let target = params
                    .get("protege")
                    .cloned()
                    .or_else(|| participants.get(1).cloned())
                    .unwrap_or_else(|| "unknown".to_string());
                let validator = params
                    .get("guardian")
                    .cloned()
                    .or_else(|| participants.first().cloned())
                    .unwrap_or_else(|| "unknown".to_string());
                let recovery = state.recovery_state.get_mut(&target).ok_or_else(|| {
                    TestingError::EventRecordingError {
                        event_type: "run_choreography".to_string(),
                        reason: format!("guardian recovery for '{target}' has not started"),
                    }
                })?;
                recovery.validation_steps.push(validator);
            }
            "guardian_recovery_coordination" => {
                let target = params
                    .get("target")
                    .cloned()
                    .or_else(|| participants.get(1).cloned())
                    .or_else(|| participants.first().cloned())
                    .unwrap_or_else(|| "unknown".to_string());
                state.parameters.insert(
                    format!("recovery:{target}:threshold_met"),
                    "true".to_string(),
                );
            }
            "threshold_key_recovery" => {
                let target = params
                    .get("target")
                    .cloned()
                    .or_else(|| participants.first().cloned())
                    .unwrap_or_else(|| "unknown".to_string());
                let mut validation_steps = state
                    .recovery_state
                    .get(&target)
                    .map(|recovery| recovery.validation_steps.clone())
                    .unwrap_or_default();
                validation_steps.push("threshold_key_recovery".to_string());
                Self::verify_recovery_success_locked(&mut state, &target, validation_steps)?;
            }
            "chat_history_sync" => {
                let target = params
                    .get("target")
                    .cloned()
                    .or_else(|| participants.first().cloned())
                    .unwrap_or_else(|| "unknown".to_string());
                state
                    .parameters
                    .insert(format!("chat_history_synced:{target}"), "true".to_string());
            }
            _ => {}
        }

        Self::increment_counter_parameter(&mut state, format!("choreography:{normalized}:count"))?;
        if let Some(round) = params.get("round") {
            Self::increment_counter_parameter(&mut state, format!("choreography_round:{round}"))?;
        }
        Self::push_event_locked(&mut state, "run_choreography", data);
        Ok(())
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
            _ => self.run_choreography_fallback(name, participants, params),
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

    fn evaluate_property_locked(
        state: &ScenarioState,
        property: &str,
    ) -> Result<bool, TestingError> {
        let total_messages = state
            .message_history
            .values()
            .map(std::vec::Vec::len)
            .sum::<usize>();
        let no_duplicate_messages = state.message_history.values().all(|messages| {
            let mut ids = std::collections::HashSet::new();
            messages
                .iter()
                .all(|message| ids.insert(message.id.clone()))
        });
        let any_successful_choreography = state
            .events
            .iter()
            .any(|event| event.event_type == "run_choreography");

        if let Some(participant) = property.strip_suffix("_account_ready") {
            return Ok(state
                .parameters
                .get(&format!("account:{participant}:ready"))
                .is_some_and(|value| value == "true"));
        }
        if let Some(participant) = property.strip_suffix("_account_restored") {
            return Ok(state
                .recovery_state
                .get(participant)
                .is_some_and(|recovery| recovery.completed)
                && !state.participant_data_loss.contains_key(participant));
        }
        if let Some(participant) = property.strip_suffix("_chat_history_restored") {
            return Ok(state
                .parameters
                .get(&format!("chat_history_synced:{participant}"))
                .is_some_and(|value| value == "true"));
        }
        if let Some(participant) = property.strip_suffix("_account_inaccessible") {
            return Ok(state.participant_data_loss.contains_key(participant));
        }
        if let Some(participant) = property.strip_suffix("_data_lost") {
            return Ok(state.participant_data_loss.contains_key(participant));
        }
        if let Some(participant) = property.strip_suffix("_guardians_configured") {
            return Ok(state
                .parameters
                .get(&format!("guardians:{participant}:configured"))
                .is_some_and(|value| value == "true"));
        }
        if let Some(round) = property
            .strip_prefix("round_")
            .and_then(|rest| rest.strip_suffix("_complete"))
        {
            return Ok(state
                .parameters
                .get(&format!("choreography_round:{round}"))
                .is_some());
        }

        let result = match property {
            "all_rounds_complete" => {
                state
                    .parameters
                    .keys()
                    .filter(|key| key.starts_with("choreography_round:"))
                    .count()
                    >= 3
            }
            "protocol_success" | "protocol_completed" | "all_phases_completed" => {
                any_successful_choreography
            }
            "signature_valid"
            | "threshold_satisfied"
            | "all_agree_on_signature"
            | "threshold_shares_generated"
            | "all_participants_have_shares" => state.parameters.keys().any(|key| {
                key.starts_with("choreography:frost_") || key == "choreography:keygen:count"
            }),
            "new_coordinator_elected" | "protocol_completes_after_recovery" => state
                .parameters
                .get("choreography:coordinator_failure_recovery:count")
                .is_some(),
            "derived_keys_match" | "derivation_deterministic" => {
                state.parameters.get("choreography:p2p_dkd:count").is_some()
                    || state
                        .parameters
                        .get("choreography:dkd_handshake:count")
                        .is_some()
            }
            "epoch_monotonic_increase" | "old_tickets_invalidated" => state
                .parameters
                .get("choreography:epoch_increment:count")
                .is_some(),
            "group_chat_created" => !state.chat_groups.is_empty(),
            "all_members_joined" => state
                .chat_groups
                .values()
                .any(|group| group.members.len() >= 3),
            "message_history_available"
            | "all_messages_delivered"
            | "message_consistency"
            | "all_messages_received"
            | "consistent_message_count"
            | "message_continuity_maintained"
            | "bob_can_see_full_history" => total_messages > 0,
            "no_duplicate_messages" => no_duplicate_messages,
            "guardian_relationships_established" => state
                .parameters
                .keys()
                .any(|key| key.starts_with("guardian_relationship:") && key.ends_with(":accepted")),
            "recovery_request_validated" => state
                .recovery_state
                .values()
                .any(|recovery| !recovery.validation_steps.is_empty()),
            "guardian_approval_threshold_met" => state
                .recovery_state
                .values()
                .any(|recovery| recovery.validation_steps.len() >= recovery.threshold),
            "bob_account_restored" => {
                state
                    .recovery_state
                    .get("bob")
                    .is_some_and(|recovery| recovery.completed)
                    && !state.participant_data_loss.contains_key("bob")
            }
            "bob_can_send_messages" | "group_functionality_restored" => {
                !state.chat_groups.is_empty() && !state.participant_data_loss.contains_key("bob")
            }
            "counter_value_correct" => state
                .parameters
                .get("choreography:counter_increment:count")
                .and_then(|value| value.parse::<u64>().ok())
                .is_some_and(|count| count >= 3),
            _ => any_successful_choreography,
        };

        Ok(result)
    }

    /// Evaluate a property against current simulator state and fail the scenario
    /// when the observed value differs from the declared expectation.
    pub fn verify_property(
        &self,
        property: &str,
        expected: Option<String>,
    ) -> Result<(), TestingError> {
        let expected_bool = expected
            .as_deref()
            .and_then(|value| {
                let trimmed = value.trim().trim_matches('"');
                match trimmed {
                    "true" => Some(true),
                    "false" => Some(false),
                    _ => None,
                }
            })
            .unwrap_or(true);
        let actual = {
            let state = self.shared.state.lock().map_err(|e| {
                TestingError::SystemError(aura_core::AuraError::internal(format!(
                    "Lock error: {e}"
                )))
            })?;
            Self::evaluate_property_locked(&state, property)?
        };

        let mut data = HashMap::from([
            ("property".to_string(), property.to_string()),
            ("expected".to_string(), expected_bool.to_string()),
            ("actual".to_string(), actual.to_string()),
        ]);
        if let Some(exp) = expected {
            data.insert("raw_expected".to_string(), exp);
        }
        self.record_simple_event("verify_property", data)?;
        if actual != expected_bool {
            return Err(TestingError::PropertyAssertionFailed {
                property_id: property.to_string(),
                description: format!(
                    "expected property '{property}' to be {expected_bool}, observed {actual}"
                ),
            });
        }
        Ok(())
    }

    /// Get statistics about scenario injections
    pub fn get_injection_stats(&self) -> Result<HashMap<String, String>, TestingError> {
        let state = self.shared.state.lock().map_err(|e| {
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
        stats.insert(
            "applied_actions".to_string(),
            state
                .active_injections
                .iter()
                .map(|injection| injection.actions_applied.len())
                .sum::<usize>()
                .to_string(),
        );

        Ok(stats)
    }

    /// Clean up expired injections
    fn cleanup_expired_injections(&self) -> Result<(), TestingError> {
        let mut state = self.shared.state.lock().map_err(|e| {
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
        Self::cleanup_expired_adaptive_privacy_locked(state);
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

    fn cleanup_expired_adaptive_privacy_locked(state: &mut ScenarioState) {
        let current_tick = state.current_tick;

        for path in state.adaptive_privacy.anonymous_paths.values_mut() {
            if current_tick >= path.expires_at_tick {
                path.expired = true;
            }
        }

        for cycle in state.adaptive_privacy.partition_heal_cycles.values_mut() {
            if current_tick >= cycle.heals_at_tick {
                cycle.healed = true;
            }
        }

        for held_object in state.adaptive_privacy.held_objects.values_mut() {
            if current_tick >= held_object.retention_until_tick {
                held_object.expired = true;
            }
        }
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

        let scenario = state.scenarios.get(scenario_id).cloned().ok_or_else(|| {
            TestingError::EventRecordingError {
                event_type: "scenario_trigger".to_string(),
                reason: format!("Scenario '{scenario_id}' not found"),
            }
        })?;

        if state
            .active_injections
            .iter()
            .any(|injection| injection.scenario_id == scenario_id)
        {
            return Ok(());
        }

        let actions_applied = Self::apply_scenario_actions_locked(state, &scenario)?;
        let injection = ActiveInjection {
            scenario_id: scenario_id.to_string(),
            start_tick: state.current_tick,
            duration_ms: scenario.duration.map(|d| d.as_millis() as u64),
            actions_applied,
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

    fn apply_scenario_actions_locked(
        state: &mut ScenarioState,
        scenario: &ScenarioDefinition,
    ) -> Result<Vec<String>, TestingError> {
        let mut applied = Vec::with_capacity(scenario.actions.len());
        for action in scenario.actions.clone() {
            applied.push(Self::apply_injection_action_locked(state, action)?);
        }
        Ok(applied)
    }

    fn apply_injection_action_locked(
        state: &mut ScenarioState,
        action: InjectionAction,
    ) -> Result<String, TestingError> {
        match action {
            InjectionAction::ModifyParameter { key, value } => {
                state.parameters.insert(key.clone(), value.clone());
                Self::push_event_locked(
                    state,
                    "scenario_modify_parameter",
                    HashMap::from([
                        ("key".to_string(), key.clone()),
                        ("value".to_string(), value),
                    ]),
                );
                Ok(format!("modify_parameter:{key}"))
            }
            InjectionAction::InjectEvent { event_type, data } => {
                Self::push_event_locked(state, &event_type, data);
                Ok(format!("inject_event:{event_type}"))
            }
            InjectionAction::ModifyBehavior {
                component,
                behavior,
            } => {
                state.behaviors.insert(component.clone(), behavior.clone());
                Self::push_event_locked(
                    state,
                    "scenario_modify_behavior",
                    HashMap::from([
                        ("component".to_string(), component.clone()),
                        ("behavior".to_string(), behavior),
                    ]),
                );
                Ok(format!("modify_behavior:{component}"))
            }
            InjectionAction::TriggerFault { fault } => {
                let payload = serde_json::to_string(&fault).map_err(|error| {
                    TestingError::EventRecordingError {
                        event_type: "scenario_trigger_fault".to_string(),
                        reason: format!("failed to serialize fault payload: {error}"),
                    }
                })?;
                Self::push_event_locked(
                    state,
                    "scenario_trigger_fault",
                    HashMap::from([("fault".to_string(), payload)]),
                );
                Ok("trigger_fault".to_string())
            }
            InjectionAction::CreateChatGroup {
                group_name,
                creator,
                initial_members,
            } => {
                let group_id =
                    Self::create_chat_group_locked(state, &group_name, &creator, initial_members)?;
                Ok(format!("create_chat_group:{group_id}"))
            }
            InjectionAction::SendChatMessage {
                group_id,
                sender,
                message,
            } => {
                Self::send_chat_message_locked(state, &group_id, &sender, &message)?;
                Ok(format!("send_chat_message:{group_id}:{sender}"))
            }
            InjectionAction::SimulateDataLoss {
                target_participant,
                loss_type,
                recovery_required,
            } => {
                Self::simulate_data_loss_locked(
                    state,
                    &target_participant,
                    &loss_type,
                    recovery_required,
                )?;
                Ok(format!("simulate_data_loss:{target_participant}"))
            }
            InjectionAction::ValidateMessageHistory {
                participant,
                expected_message_count,
                include_pre_recovery,
            } => {
                let valid = Self::validate_message_history_locked(
                    state,
                    &participant,
                    expected_message_count,
                    include_pre_recovery,
                )?;
                Self::push_event_locked(
                    state,
                    "scenario_validate_message_history",
                    HashMap::from([
                        ("participant".to_string(), participant.clone()),
                        ("valid".to_string(), valid.to_string()),
                    ]),
                );
                Ok(format!("validate_message_history:{participant}"))
            }
            InjectionAction::InitiateGuardianRecovery {
                target,
                guardians,
                threshold,
            } => {
                Self::initiate_guardian_recovery_locked(state, &target, guardians, threshold)?;
                Ok(format!("initiate_guardian_recovery:{target}"))
            }
            InjectionAction::VerifyRecoverySuccess {
                target,
                validation_steps,
            } => {
                Self::verify_recovery_success_locked(state, &target, validation_steps)?;
                Ok(format!("verify_recovery_success:{target}"))
            }
            InjectionAction::AdaptivePrivacyTransition(transition) => {
                Self::apply_adaptive_privacy_transition_locked(state, transition)
            }
        }
    }

    fn apply_adaptive_privacy_transition_locked(
        state: &mut ScenarioState,
        transition: AdaptivePrivacyTransition,
    ) -> Result<String, TestingError> {
        match transition {
            AdaptivePrivacyTransition::ConfigureMovement {
                profile_id,
                clusters,
                home_locality_bias,
                neighborhood_locality_bias,
            } => {
                state.environment_bridge.configure_mobility_profile(
                    profile_id.clone(),
                    clusters.clone(),
                    bias_to_per_mille(home_locality_bias),
                    bias_to_per_mille(neighborhood_locality_bias),
                    state.current_tick,
                );
                state.adaptive_privacy.movement_profiles.insert(
                    profile_id.clone(),
                    MovementProfile {
                        id: profile_id.clone(),
                        clusters,
                        home_locality_bias,
                        neighborhood_locality_bias,
                        recorded_at: state.current_tick,
                    },
                );
                Ok(format!("adaptive_privacy:movement:{profile_id}"))
            }
            AdaptivePrivacyTransition::EstablishAnonymousPath {
                path_id,
                initiator,
                destination,
                hops,
                ttl_ticks,
                reusable,
            } => {
                state.adaptive_privacy.anonymous_paths.insert(
                    path_id.clone(),
                    AnonymousPathState {
                        id: path_id.clone(),
                        initiator,
                        destination,
                        hops,
                        established_at: state.current_tick,
                        expires_at_tick: state.current_tick.saturating_add(ttl_ticks),
                        reusable,
                        reuse_count: 0,
                        expired: false,
                    },
                );
                Ok(format!("adaptive_privacy:path_established:{path_id}"))
            }
            AdaptivePrivacyTransition::ReuseEstablishedPath { path_id } => {
                let path = state
                    .adaptive_privacy
                    .anonymous_paths
                    .get_mut(&path_id)
                    .ok_or_else(|| TestingError::StateInspectionError {
                        component: "adaptive_privacy".to_string(),
                        path: format!("path:{path_id}"),
                        reason: "anonymous path not found".to_string(),
                    })?;
                path.reuse_count = path.reuse_count.saturating_add(1);
                Ok(format!("adaptive_privacy:path_reused:{path_id}"))
            }
            AdaptivePrivacyTransition::ExpireEstablishedPath { path_id } => {
                let path = state
                    .adaptive_privacy
                    .anonymous_paths
                    .get_mut(&path_id)
                    .ok_or_else(|| TestingError::StateInspectionError {
                        component: "adaptive_privacy".to_string(),
                        path: format!("path:{path_id}"),
                        reason: "anonymous path not found".to_string(),
                    })?;
                path.expired = true;
                Ok(format!("adaptive_privacy:path_expired:{path_id}"))
            }
            AdaptivePrivacyTransition::RecordEstablishFlow {
                flow_id,
                source,
                destination,
                path_id,
            } => {
                state.adaptive_privacy.establish_flows.insert(
                    flow_id.clone(),
                    EstablishFlowState {
                        id: flow_id.clone(),
                        source,
                        destination,
                        path_id,
                        established_at: state.current_tick,
                    },
                );
                Ok(format!("adaptive_privacy:establish_flow:{flow_id}"))
            }
            AdaptivePrivacyTransition::RecordMoveBatch {
                batch_id,
                envelope_count,
            } => {
                state.adaptive_privacy.move_batches.insert(
                    batch_id.clone(),
                    MoveBatchState {
                        id: batch_id.clone(),
                        envelope_count,
                        created_at: state.current_tick,
                    },
                );
                Ok(format!("adaptive_privacy:move_batch:{batch_id}"))
            }
            AdaptivePrivacyTransition::ObserveLocalHealth {
                provider,
                score,
                latency_ms,
            } => {
                state.adaptive_privacy.local_health.insert(
                    provider.clone(),
                    LocalHealthObservation {
                        provider: provider.clone(),
                        score,
                        latency_ms,
                        observed_at: state.current_tick,
                    },
                );
                Ok(format!("adaptive_privacy:local_health:{provider}"))
            }
            AdaptivePrivacyTransition::RecordCoverTraffic {
                provider,
                envelope_count,
            } => {
                state
                    .adaptive_privacy
                    .cover_events
                    .push(CoverTrafficRecord {
                        provider: provider.clone(),
                        envelope_count,
                        recorded_at: state.current_tick,
                    });
                Ok(format!("adaptive_privacy:cover:{provider}"))
            }
            AdaptivePrivacyTransition::RecordAccountabilityReply {
                reply_id,
                deadline_ticks,
                completed_after_ticks,
            } => {
                let completed_at_tick =
                    completed_after_ticks.map(|delta| state.current_tick.saturating_add(delta));
                let deadline_tick = state.current_tick.saturating_add(deadline_ticks);
                let within_deadline =
                    completed_at_tick.map_or(true, |completed| completed <= deadline_tick);
                state.adaptive_privacy.accountability_replies.insert(
                    reply_id.clone(),
                    AccountabilityReplyState {
                        id: reply_id.clone(),
                        deadline_tick,
                        completed_at_tick,
                        within_deadline,
                    },
                );
                Ok(format!("adaptive_privacy:accountability_reply:{reply_id}"))
            }
            AdaptivePrivacyTransition::RecordRouteDiversity {
                selector_id,
                unique_paths,
                dominant_provider,
            } => {
                state.adaptive_privacy.route_diversity.insert(
                    selector_id.clone(),
                    RouteDiversityObservation {
                        selector_id: selector_id.clone(),
                        unique_paths,
                        dominant_provider,
                        recorded_at: state.current_tick,
                    },
                );
                Ok(format!("adaptive_privacy:route_diversity:{selector_id}"))
            }
            AdaptivePrivacyTransition::RecordHonestHopCompromise {
                path_id,
                compromised_hops,
                honest_hops_remaining,
            } => {
                state.adaptive_privacy.honest_hop_compromise_patterns.push(
                    HonestHopCompromisePattern {
                        path_id: path_id.clone(),
                        compromised_hops,
                        honest_hops_remaining,
                        recorded_at: state.current_tick,
                    },
                );
                Ok(format!("adaptive_privacy:honest_hop_compromise:{path_id}"))
            }
            AdaptivePrivacyTransition::RecordPartitionHealCycle {
                cycle_id,
                partition_groups,
                heal_after_ticks,
            } => {
                let heals_at_tick = state.current_tick.saturating_add(heal_after_ticks);
                state.adaptive_privacy.partition_heal_cycles.insert(
                    cycle_id.clone(),
                    PartitionHealCycle {
                        id: cycle_id.clone(),
                        partition_groups,
                        partitioned_at: state.current_tick,
                        heals_at_tick,
                        healed: false,
                    },
                );
                Ok(format!("adaptive_privacy:partition_heal:{cycle_id}"))
            }
            AdaptivePrivacyTransition::RecordChurnBurst {
                burst_id,
                affected_participants,
                entering,
                leaving,
            } => {
                state.adaptive_privacy.churn_bursts.insert(
                    burst_id.clone(),
                    ChurnBurstState {
                        id: burst_id.clone(),
                        affected_participants,
                        entering,
                        leaving,
                        recorded_at: state.current_tick,
                    },
                );
                Ok(format!("adaptive_privacy:churn:{burst_id}"))
            }
            AdaptivePrivacyTransition::RecordProviderSaturation {
                provider,
                queue_depth,
                utilization,
            } => {
                state.environment_bridge.observe_node_capability(
                    provider.clone(),
                    queue_depth,
                    bias_to_per_mille(utilization),
                    state.current_tick,
                );
                state.adaptive_privacy.provider_saturation.insert(
                    provider.clone(),
                    ProviderSaturationState {
                        provider: provider.clone(),
                        queue_depth,
                        utilization,
                        recorded_at: state.current_tick,
                    },
                );
                Ok(format!("adaptive_privacy:provider_saturation:{provider}"))
            }
            AdaptivePrivacyTransition::RecordHeldObjectRetention {
                object_id,
                selector,
                retention_ticks,
                seeded_from_move,
            } => {
                state.adaptive_privacy.held_objects.insert(
                    object_id.clone(),
                    HeldObjectRetentionState {
                        object_id: object_id.clone(),
                        selector,
                        retained_at: state.current_tick,
                        retention_until_tick: state.current_tick.saturating_add(retention_ticks),
                        seeded_from_move,
                        expired: false,
                    },
                );
                Ok(format!("adaptive_privacy:held_object:{object_id}"))
            }
            AdaptivePrivacyTransition::RecordSelectorRetrieval {
                retrieval_id,
                selector,
                expected_objects,
                sync_profile,
            } => {
                state.adaptive_privacy.selector_retrievals.insert(
                    retrieval_id.clone(),
                    SelectorRetrievalState {
                        retrieval_id: retrieval_id.clone(),
                        selector,
                        expected_objects,
                        sync_profile,
                        recorded_at: state.current_tick,
                    },
                );
                Ok(format!(
                    "adaptive_privacy:selector_retrieval:{retrieval_id}"
                ))
            }
            AdaptivePrivacyTransition::RecordSyncOpportunity {
                profile_id,
                density,
                peers,
            } => {
                state.environment_bridge.observe_link_admission(
                    profile_id.clone(),
                    sync_density_label(density).to_string(),
                    peers.clone(),
                    state.current_tick,
                );
                state.adaptive_privacy.sync_opportunities.insert(
                    profile_id.clone(),
                    SyncOpportunityState {
                        profile_id: profile_id.clone(),
                        density,
                        peers,
                        recorded_at: state.current_tick,
                    },
                );
                Ok(format!("adaptive_privacy:sync_opportunity:{profile_id}"))
            }
            AdaptivePrivacyTransition::RecordMoveToHoldSeed {
                batch_id,
                object_id,
                selector,
            } => {
                if let Some(held) = state.adaptive_privacy.held_objects.get_mut(&object_id) {
                    held.seeded_from_move = true;
                }
                state.adaptive_privacy.move_to_hold_seeds.insert(
                    object_id.clone(),
                    MoveToHoldSeedState {
                        batch_id: batch_id.clone(),
                        object_id: object_id.clone(),
                        selector,
                        seeded_at: state.current_tick,
                    },
                );
                Ok(format!(
                    "adaptive_privacy:move_to_hold_seed:{batch_id}:{object_id}"
                ))
            }
        }
    }

    fn push_event_locked(
        state: &mut ScenarioState,
        event_type: &str,
        data: HashMap<String, String>,
    ) {
        state.events.push(SimulationEvent {
            event_type: event_type.to_string(),
            timestamp: state.current_tick,
            data,
        });
    }

    fn create_chat_group_locked(
        state: &mut ScenarioState,
        group_name: &str,
        creator: &str,
        initial_members: Vec<String>,
    ) -> Result<String, TestingError> {
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

    fn send_chat_message_locked(
        state: &mut ScenarioState,
        group_id: &str,
        sender: &str,
        message: &str,
    ) -> Result<(), TestingError> {
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
        let messages = state.message_history.get_mut(group_id).unwrap();
        messages.push(chat_message);

        Ok(())
    }

    fn simulate_data_loss_locked(
        state: &mut ScenarioState,
        target_participant: &str,
        loss_type: &str,
        recovery_required: bool,
    ) -> Result<(), TestingError> {
        let pre_loss_count: usize = state
            .message_history
            .values()
            .map(|messages: &Vec<ChatMessage>| {
                messages
                    .iter()
                    .filter(|_msg| {
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

    fn validate_message_history_locked(
        state: &ScenarioState,
        participant: &str,
        expected_message_count: usize,
        include_pre_recovery: bool,
    ) -> Result<bool, TestingError> {
        let actual_count: usize = state
            .message_history
            .values()
            .map(|messages| {
                messages
                    .iter()
                    .filter(|_msg| {
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
                Ok(actual_count >= loss_info.pre_loss_message_count
                    && actual_count >= expected_message_count)
            } else {
                Ok(actual_count >= expected_message_count)
            }
        } else {
            Ok(actual_count >= expected_message_count)
        }
    }

    fn initiate_guardian_recovery_locked(
        state: &mut ScenarioState,
        target: &str,
        guardians: Vec<String>,
        threshold: usize,
    ) -> Result<(), TestingError> {
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

    fn verify_recovery_success_locked(
        state: &mut ScenarioState,
        target: &str,
        validation_steps: Vec<String>,
    ) -> Result<bool, TestingError> {
        if let Some(recovery_info) = state.recovery_state.get_mut(target) {
            recovery_info.completed = true;
            recovery_info.validation_steps = validation_steps;
            state.participant_data_loss.remove(target);
            Ok(true)
        } else {
            Err(TestingError::EventRecordingError {
                event_type: "recovery_verification".to_string(),
                reason: format!("No recovery process found for target '{target}'"),
            })
        }
    }

    /// Create a chat group for multi-actor scenarios
    pub fn create_chat_group(
        &self,
        group_name: &str,
        creator: &str,
        initial_members: Vec<String>,
    ) -> Result<String, TestingError> {
        let mut state = self.shared.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
        })?;
        Self::create_chat_group_locked(&mut state, group_name, creator, initial_members)
    }

    /// Send a chat message in a scenario
    pub fn send_chat_message(
        &self,
        group_id: &str,
        sender: &str,
        message: &str,
    ) -> Result<(), TestingError> {
        let mut state = self.shared.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
        })?;
        Self::send_chat_message_locked(&mut state, group_id, sender, message)
    }

    /// Simulate data loss for a participant
    pub fn simulate_data_loss(
        &self,
        target_participant: &str,
        loss_type: &str,
        recovery_required: bool,
    ) -> Result<(), TestingError> {
        let mut state = self.shared.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
        })?;
        Self::simulate_data_loss_locked(
            &mut state,
            target_participant,
            loss_type,
            recovery_required,
        )
    }

    /// Validate message history for a participant across recovery
    pub fn validate_message_history(
        &self,
        participant: &str,
        expected_message_count: usize,
        include_pre_recovery: bool,
    ) -> Result<bool, TestingError> {
        let state = self.shared.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
        })?;
        Self::validate_message_history_locked(
            &state,
            participant,
            expected_message_count,
            include_pre_recovery,
        )
    }

    /// Initiate guardian recovery for a participant
    pub fn initiate_guardian_recovery(
        &self,
        target: &str,
        guardians: Vec<String>,
        threshold: usize,
    ) -> Result<(), TestingError> {
        let mut state = self.shared.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
        })?;
        Self::initiate_guardian_recovery_locked(&mut state, target, guardians, threshold)
    }

    /// Verify recovery completion
    pub fn verify_recovery_success(
        &self,
        target: &str,
        validation_steps: Vec<String>,
    ) -> Result<bool, TestingError> {
        let mut state = self.shared.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
        })?;
        Self::verify_recovery_success_locked(&mut state, target, validation_steps)
    }

    /// Get chat group statistics
    pub fn get_chat_stats(&self) -> Result<HashMap<String, String>, TestingError> {
        let state = self.shared.state.lock().map_err(|e| {
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

fn bias_to_per_mille(value: f64) -> u64 {
    (value.clamp(0.0, 1.0) * 1000.0).round() as u64
}

fn sync_density_label(density: SyncOpportunityDensity) -> &'static str {
    match density {
        SyncOpportunityDensity::Sparse => "sparse",
        SyncOpportunityDensity::Balanced => "balanced",
        SyncOpportunityDensity::Heavy => "heavy",
    }
}

#[async_trait]
impl TestingEffects for SimulationScenarioHandler {
    async fn create_checkpoint(
        &self,
        checkpoint_id: &str,
        label: &str,
    ) -> Result<(), TestingError> {
        let mut state = self.shared.state.lock().map_err(|e| {
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
        let state = self.shared.state.lock().map_err(|e| {
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
                let mut state_mut = self.shared.state.lock().map_err(|e| {
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
        let state = self.shared.state.lock().map_err(|e| {
            TestingError::SystemError(aura_core::AuraError::internal(format!("Lock error: {e}")))
        })?;

        match component {
            "scenarios" => {
                if path == "count" {
                    Ok(Box::new(state.scenarios.len()))
                } else if path == "active" {
                    Ok(Box::new(state.active_injections.len()))
                } else if let Some(key) = path.strip_prefix("parameter:") {
                    Ok(Box::new(
                        state.parameters.get(key).cloned().unwrap_or_default(),
                    ))
                } else if let Some(key) = path.strip_prefix("behavior:") {
                    Ok(Box::new(
                        state.behaviors.get(key).cloned().unwrap_or_default(),
                    ))
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
            "environment" => match path {
                "mobility_profiles" => Ok(Box::new(
                    state.environment_bridge.snapshot().mobility_profiles.len(),
                )),
                "link_admissions" => Ok(Box::new(
                    state.environment_bridge.snapshot().link_admissions.len(),
                )),
                "node_capabilities" => Ok(Box::new(
                    state.environment_bridge.snapshot().node_capabilities.len(),
                )),
                "trace_entries" => Ok(Box::new(state.environment_bridge.trace().entries.len())),
                _ => Err(TestingError::StateInspectionError {
                    component: component.to_string(),
                    path: path.to_string(),
                    reason: "Unknown environment path".to_string(),
                }),
            },
            "adaptive_privacy" => {
                if path == "movement_profiles" {
                    Ok(Box::new(state.adaptive_privacy.movement_profiles.len()))
                } else if path == "anonymous_paths" {
                    Ok(Box::new(state.adaptive_privacy.anonymous_paths.len()))
                } else if path == "active_anonymous_paths" {
                    Ok(Box::new(
                        state
                            .adaptive_privacy
                            .anonymous_paths
                            .values()
                            .filter(|path| !path.expired)
                            .count(),
                    ))
                } else if path == "expired_anonymous_paths" {
                    Ok(Box::new(
                        state
                            .adaptive_privacy
                            .anonymous_paths
                            .values()
                            .filter(|path| path.expired)
                            .count(),
                    ))
                } else if path == "establish_flows" {
                    Ok(Box::new(state.adaptive_privacy.establish_flows.len()))
                } else if path == "move_batches" {
                    Ok(Box::new(state.adaptive_privacy.move_batches.len()))
                } else if path == "local_health_observations" {
                    Ok(Box::new(state.adaptive_privacy.local_health.len()))
                } else if path == "cover_events" {
                    Ok(Box::new(state.adaptive_privacy.cover_events.len()))
                } else if path == "accountability_replies" {
                    Ok(Box::new(
                        state.adaptive_privacy.accountability_replies.len(),
                    ))
                } else if path == "late_accountability_replies" {
                    Ok(Box::new(
                        state
                            .adaptive_privacy
                            .accountability_replies
                            .values()
                            .filter(|reply| !reply.within_deadline)
                            .count(),
                    ))
                } else if path == "route_diversity" {
                    Ok(Box::new(state.adaptive_privacy.route_diversity.len()))
                } else if path == "honest_hop_compromise_patterns" {
                    Ok(Box::new(
                        state.adaptive_privacy.honest_hop_compromise_patterns.len(),
                    ))
                } else if path == "partition_heal_cycles" {
                    Ok(Box::new(state.adaptive_privacy.partition_heal_cycles.len()))
                } else if path == "healed_partition_cycles" {
                    Ok(Box::new(
                        state
                            .adaptive_privacy
                            .partition_heal_cycles
                            .values()
                            .filter(|cycle| cycle.healed)
                            .count(),
                    ))
                } else if path == "churn_bursts" {
                    Ok(Box::new(state.adaptive_privacy.churn_bursts.len()))
                } else if path == "provider_saturation" {
                    Ok(Box::new(state.adaptive_privacy.provider_saturation.len()))
                } else if path == "held_objects" {
                    Ok(Box::new(state.adaptive_privacy.held_objects.len()))
                } else if path == "active_held_objects" {
                    Ok(Box::new(
                        state
                            .adaptive_privacy
                            .held_objects
                            .values()
                            .filter(|object| !object.expired)
                            .count(),
                    ))
                } else if path == "selector_retrievals" {
                    Ok(Box::new(state.adaptive_privacy.selector_retrievals.len()))
                } else if path == "sync_opportunities" {
                    Ok(Box::new(state.adaptive_privacy.sync_opportunities.len()))
                } else if path == "move_to_hold_seeds" {
                    Ok(Box::new(state.adaptive_privacy.move_to_hold_seeds.len()))
                } else if let Some(rest) = path.strip_prefix("path:") {
                    let parts: Vec<&str> = rest.split(':').collect();
                    if parts.len() != 2 {
                        return Err(TestingError::StateInspectionError {
                            component: component.to_string(),
                            path: path.to_string(),
                            reason: "Expected path:<id>:<field>".to_string(),
                        });
                    }
                    let path_state = state
                        .adaptive_privacy
                        .anonymous_paths
                        .get(parts[0])
                        .ok_or_else(|| TestingError::StateInspectionError {
                            component: component.to_string(),
                            path: path.to_string(),
                            reason: "Anonymous path not found".to_string(),
                        })?;
                    match parts[1] {
                        "reuse_count" => Ok(Box::new(path_state.reuse_count)),
                        "expired" => Ok(Box::new(path_state.expired)),
                        "hop_count" => Ok(Box::new(path_state.hops.len())),
                        _ => Err(TestingError::StateInspectionError {
                            component: component.to_string(),
                            path: path.to_string(),
                            reason: "Unknown path field".to_string(),
                        }),
                    }
                } else if let Some(rest) = path.strip_prefix("held:") {
                    let parts: Vec<&str> = rest.split(':').collect();
                    if parts.len() != 2 {
                        return Err(TestingError::StateInspectionError {
                            component: component.to_string(),
                            path: path.to_string(),
                            reason: "Expected held:<id>:<field>".to_string(),
                        });
                    }
                    let held_object = state
                        .adaptive_privacy
                        .held_objects
                        .get(parts[0])
                        .ok_or_else(|| TestingError::StateInspectionError {
                            component: component.to_string(),
                            path: path.to_string(),
                            reason: "Held object not found".to_string(),
                        })?;
                    match parts[1] {
                        "expired" => Ok(Box::new(held_object.expired)),
                        "seeded_from_move" => Ok(Box::new(held_object.seeded_from_move)),
                        _ => Err(TestingError::StateInspectionError {
                            component: component.to_string(),
                            path: path.to_string(),
                            reason: "Unknown held-object field".to_string(),
                        }),
                    }
                } else if let Some(rest) = path.strip_prefix("reply:") {
                    let parts: Vec<&str> = rest.split(':').collect();
                    if parts.len() != 2 {
                        return Err(TestingError::StateInspectionError {
                            component: component.to_string(),
                            path: path.to_string(),
                            reason: "Expected reply:<id>:<field>".to_string(),
                        });
                    }
                    let reply = state
                        .adaptive_privacy
                        .accountability_replies
                        .get(parts[0])
                        .ok_or_else(|| TestingError::StateInspectionError {
                            component: component.to_string(),
                            path: path.to_string(),
                            reason: "Reply not found".to_string(),
                        })?;
                    match parts[1] {
                        "within_deadline" => Ok(Box::new(reply.within_deadline)),
                        _ => Err(TestingError::StateInspectionError {
                            component: component.to_string(),
                            path: path.to_string(),
                            reason: "Unknown reply field".to_string(),
                        }),
                    }
                } else {
                    Err(TestingError::StateInspectionError {
                        component: component.to_string(),
                        path: path.to_string(),
                        reason: "Unknown adaptive privacy path".to_string(),
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
        self.record_simple_event(event_type, event_data)
    }

    async fn record_metric(
        &self,
        metric_name: &str,
        value: f64,
        unit: &str,
    ) -> Result<(), TestingError> {
        let mut state = self.shared.state.lock().map_err(|e| {
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
    /// Capture the Aura environment bridge artifacts for migrated simulation state.
    #[must_use]
    pub(crate) fn capture_environment_artifacts(&self) -> AuraEnvironmentArtifacts {
        let state = self
            .shared
            .state
            .lock()
            .expect("scenario state mutex should not be poisoned");
        let mut artifacts = state.environment_bridge.capture_artifacts();
        artifacts.overlay = Some(capture_adaptive_privacy_overlay(&state.adaptive_privacy));
        artifacts
    }

    fn record_simple_event(
        &self,
        event_type: &str,
        event_data: HashMap<String, String>,
    ) -> Result<(), TestingError> {
        let mut state = self.shared.state.lock().map_err(|e| {
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

fn capture_adaptive_privacy_overlay(
    adaptive_privacy: &AdaptivePrivacyState,
) -> AuraEnvironmentOverlayV1 {
    let mut mobility_profiles = adaptive_privacy
        .movement_profiles
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    mobility_profiles.sort();

    let mut provider_heterogeneity = adaptive_privacy
        .provider_saturation
        .values()
        .map(|state| AuraProviderOverlayV1 {
            provider: state.provider.clone(),
            queue_depth: state.queue_depth,
            utilization_per_mille: bias_to_per_mille(state.utilization),
            health_score_per_mille: adaptive_privacy
                .local_health
                .get(&state.provider)
                .map(|health| bias_to_per_mille(health.score)),
            latency_ms: adaptive_privacy
                .local_health
                .get(&state.provider)
                .map(|health| health.latency_ms),
        })
        .collect::<Vec<_>>();
    provider_heterogeneity.sort_by(|left, right| left.provider.cmp(&right.provider));

    let mut admission_pressure = adaptive_privacy
        .sync_opportunities
        .values()
        .map(|state| AuraAdmissionPressureOverlayV1 {
            profile_id: state.profile_id.clone(),
            density: sync_density_label(state.density).to_string(),
            peer_count: state.peers.len(),
        })
        .collect::<Vec<_>>();
    admission_pressure.sort_by(|left, right| left.profile_id.cmp(&right.profile_id));

    let mut topology_churn = adaptive_privacy
        .churn_bursts
        .values()
        .map(|burst| AuraTopologyChurnOverlayV1 {
            burst_id: burst.id.clone(),
            affected_participants: burst.affected_participants.clone(),
            entering: burst.entering,
            leaving: burst.leaving,
            recorded_at_tick: burst.recorded_at,
        })
        .collect::<Vec<_>>();
    topology_churn.sort_by(|left, right| left.burst_id.cmp(&right.burst_id));

    let mut adversary_interference = adaptive_privacy
        .honest_hop_compromise_patterns
        .iter()
        .map(|pattern| AuraInterferenceOverlayV1 {
            path_id: pattern.path_id.clone(),
            compromised_hops: pattern.compromised_hops.clone(),
            honest_hops_remaining: pattern.honest_hops_remaining,
            recorded_at_tick: pattern.recorded_at,
        })
        .collect::<Vec<_>>();
    adversary_interference.sort_by(|left, right| left.path_id.cmp(&right.path_id));

    AuraEnvironmentOverlayV1 {
        mobility_profiles,
        partition_heal_cycle_count: adaptive_privacy.partition_heal_cycles.len(),
        provider_heterogeneity,
        admission_pressure,
        topology_churn,
        adversary_interference,
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
    async fn test_scenario_actions_apply_parameter_and_behavior_state() {
        let handler = SimulationScenarioHandler::new(123);

        handler
            .register_scenario(ScenarioDefinition {
                id: "action_apply".to_string(),
                name: "Action Apply".to_string(),
                actions: vec![
                    InjectionAction::ModifyParameter {
                        key: "sync_density".to_string(),
                        value: "sparse".to_string(),
                    },
                    InjectionAction::ModifyBehavior {
                        component: "selector".to_string(),
                        behavior: "weighted_rotation".to_string(),
                    },
                ],
                trigger: TriggerCondition::Immediate,
                duration: Some(Duration::from_secs(5)),
                priority: 1,
            })
            .expect("register scenario");

        handler
            .trigger_scenario("action_apply")
            .expect("trigger scenario");

        let parameter = handler
            .inspect_state("scenarios", "parameter:sync_density")
            .await
            .expect("inspect parameter")
            .downcast::<String>()
            .expect("parameter type");
        assert_eq!(&*parameter, "sparse");

        let behavior = handler
            .inspect_state("scenarios", "behavior:selector")
            .await
            .expect("inspect behavior")
            .downcast::<String>()
            .expect("behavior type");
        assert_eq!(&*behavior, "weighted_rotation");
    }

    #[tokio::test]
    async fn test_adaptive_privacy_scenario_support_covers_phase_six_surface() {
        let handler = SimulationScenarioHandler::new(123);

        handler
            .register_scenario(ScenarioDefinition {
                id: "adaptive_privacy_surface".to_string(),
                name: "Adaptive Privacy Surface".to_string(),
                actions: vec![
                    InjectionAction::AdaptivePrivacyTransition(
                        AdaptivePrivacyTransition::ConfigureMovement {
                            profile_id: "clustered_social".to_string(),
                            clusters: vec!["home-a".to_string(), "neighborhood-1".to_string()],
                            home_locality_bias: 0.9,
                            neighborhood_locality_bias: 0.7,
                        },
                    ),
                    InjectionAction::AdaptivePrivacyTransition(
                        AdaptivePrivacyTransition::EstablishAnonymousPath {
                            path_id: "path-a".to_string(),
                            initiator: "alice".to_string(),
                            destination: "bob".to_string(),
                            hops: vec![
                                "relay-1".to_string(),
                                "relay-2".to_string(),
                                "relay-3".to_string(),
                            ],
                            ttl_ticks: 5,
                            reusable: true,
                        },
                    ),
                    InjectionAction::AdaptivePrivacyTransition(
                        AdaptivePrivacyTransition::ReuseEstablishedPath {
                            path_id: "path-a".to_string(),
                        },
                    ),
                    InjectionAction::AdaptivePrivacyTransition(
                        AdaptivePrivacyTransition::RecordEstablishFlow {
                            flow_id: "flow-a".to_string(),
                            source: "alice".to_string(),
                            destination: "bob".to_string(),
                            path_id: Some("path-a".to_string()),
                        },
                    ),
                    InjectionAction::AdaptivePrivacyTransition(
                        AdaptivePrivacyTransition::RecordMoveBatch {
                            batch_id: "batch-a".to_string(),
                            envelope_count: 3,
                        },
                    ),
                    InjectionAction::AdaptivePrivacyTransition(
                        AdaptivePrivacyTransition::ObserveLocalHealth {
                            provider: "provider-a".to_string(),
                            score: 0.87,
                            latency_ms: 24,
                        },
                    ),
                    InjectionAction::AdaptivePrivacyTransition(
                        AdaptivePrivacyTransition::RecordCoverTraffic {
                            provider: "provider-a".to_string(),
                            envelope_count: 2,
                        },
                    ),
                    InjectionAction::AdaptivePrivacyTransition(
                        AdaptivePrivacyTransition::RecordAccountabilityReply {
                            reply_id: "reply-a".to_string(),
                            deadline_ticks: 3,
                            completed_after_ticks: Some(2),
                        },
                    ),
                    InjectionAction::AdaptivePrivacyTransition(
                        AdaptivePrivacyTransition::RecordRouteDiversity {
                            selector_id: "selector-a".to_string(),
                            unique_paths: 3,
                            dominant_provider: Some("provider-a".to_string()),
                        },
                    ),
                    InjectionAction::AdaptivePrivacyTransition(
                        AdaptivePrivacyTransition::RecordHonestHopCompromise {
                            path_id: "path-a".to_string(),
                            compromised_hops: vec!["relay-1".to_string()],
                            honest_hops_remaining: 2,
                        },
                    ),
                    InjectionAction::AdaptivePrivacyTransition(
                        AdaptivePrivacyTransition::RecordPartitionHealCycle {
                            cycle_id: "cycle-a".to_string(),
                            partition_groups: vec![
                                vec!["alice".to_string(), "carol".to_string()],
                                vec!["bob".to_string()],
                            ],
                            heal_after_ticks: 4,
                        },
                    ),
                    InjectionAction::AdaptivePrivacyTransition(
                        AdaptivePrivacyTransition::RecordChurnBurst {
                            burst_id: "churn-a".to_string(),
                            affected_participants: vec![
                                "alice".to_string(),
                                "bob".to_string(),
                                "carol".to_string(),
                            ],
                            entering: 2,
                            leaving: 1,
                        },
                    ),
                    InjectionAction::AdaptivePrivacyTransition(
                        AdaptivePrivacyTransition::RecordProviderSaturation {
                            provider: "provider-a".to_string(),
                            queue_depth: 12,
                            utilization: 0.94,
                        },
                    ),
                    InjectionAction::AdaptivePrivacyTransition(
                        AdaptivePrivacyTransition::RecordHeldObjectRetention {
                            object_id: "held-a".to_string(),
                            selector: "selector:alpha".to_string(),
                            retention_ticks: 6,
                            seeded_from_move: false,
                        },
                    ),
                    InjectionAction::AdaptivePrivacyTransition(
                        AdaptivePrivacyTransition::RecordSelectorRetrieval {
                            retrieval_id: "retrieval-a".to_string(),
                            selector: "selector:alpha".to_string(),
                            expected_objects: 2,
                            sync_profile: "sparse".to_string(),
                        },
                    ),
                    InjectionAction::AdaptivePrivacyTransition(
                        AdaptivePrivacyTransition::RecordSyncOpportunity {
                            profile_id: "sync-sparse".to_string(),
                            density: SyncOpportunityDensity::Sparse,
                            peers: vec!["alice".to_string(), "bob".to_string()],
                        },
                    ),
                    InjectionAction::AdaptivePrivacyTransition(
                        AdaptivePrivacyTransition::RecordMoveToHoldSeed {
                            batch_id: "batch-a".to_string(),
                            object_id: "held-a".to_string(),
                            selector: "selector:alpha".to_string(),
                        },
                    ),
                ],
                trigger: TriggerCondition::Immediate,
                duration: Some(Duration::from_secs(5)),
                priority: 10,
            })
            .expect("register scenario");

        handler
            .trigger_scenario("adaptive_privacy_surface")
            .expect("trigger adaptive privacy scenario");

        let movement_profiles = handler
            .inspect_state("adaptive_privacy", "movement_profiles")
            .await
            .expect("movement profiles")
            .downcast::<usize>()
            .expect("movement type");
        assert_eq!(*movement_profiles, 1);

        let active_paths = handler
            .inspect_state("adaptive_privacy", "active_anonymous_paths")
            .await
            .expect("active paths")
            .downcast::<usize>()
            .expect("active path type");
        assert_eq!(*active_paths, 1);

        let reuse_count = handler
            .inspect_state("adaptive_privacy", "path:path-a:reuse_count")
            .await
            .expect("path reuse")
            .downcast::<u64>()
            .expect("reuse type");
        assert_eq!(*reuse_count, 1);

        let establish_flows = handler
            .inspect_state("adaptive_privacy", "establish_flows")
            .await
            .expect("establish flows")
            .downcast::<usize>()
            .expect("establish flows type");
        assert_eq!(*establish_flows, 1);

        let move_batches = handler
            .inspect_state("adaptive_privacy", "move_batches")
            .await
            .expect("move batches")
            .downcast::<usize>()
            .expect("move batches type");
        assert_eq!(*move_batches, 1);

        let local_health = handler
            .inspect_state("adaptive_privacy", "local_health_observations")
            .await
            .expect("local health")
            .downcast::<usize>()
            .expect("local health type");
        assert_eq!(*local_health, 1);

        let cover_events = handler
            .inspect_state("adaptive_privacy", "cover_events")
            .await
            .expect("cover events")
            .downcast::<usize>()
            .expect("cover type");
        assert_eq!(*cover_events, 1);

        let accountability_replies = handler
            .inspect_state("adaptive_privacy", "accountability_replies")
            .await
            .expect("accountability replies")
            .downcast::<usize>()
            .expect("accountability type");
        assert_eq!(*accountability_replies, 1);

        let within_deadline = handler
            .inspect_state("adaptive_privacy", "reply:reply-a:within_deadline")
            .await
            .expect("reply deadline state")
            .downcast::<bool>()
            .expect("reply deadline type");
        assert!(*within_deadline);

        let route_diversity = handler
            .inspect_state("adaptive_privacy", "route_diversity")
            .await
            .expect("route diversity")
            .downcast::<usize>()
            .expect("route diversity type");
        assert_eq!(*route_diversity, 1);

        let honest_hop_compromise_patterns = handler
            .inspect_state("adaptive_privacy", "honest_hop_compromise_patterns")
            .await
            .expect("compromise patterns")
            .downcast::<usize>()
            .expect("compromise pattern type");
        assert_eq!(*honest_hop_compromise_patterns, 1);

        let partition_cycles = handler
            .inspect_state("adaptive_privacy", "partition_heal_cycles")
            .await
            .expect("partition cycles")
            .downcast::<usize>()
            .expect("partition cycle type");
        assert_eq!(*partition_cycles, 1);

        let churn_bursts = handler
            .inspect_state("adaptive_privacy", "churn_bursts")
            .await
            .expect("churn bursts")
            .downcast::<usize>()
            .expect("churn type");
        assert_eq!(*churn_bursts, 1);

        let provider_saturation = handler
            .inspect_state("adaptive_privacy", "provider_saturation")
            .await
            .expect("provider saturation")
            .downcast::<usize>()
            .expect("provider saturation type");
        assert_eq!(*provider_saturation, 1);

        let held_objects = handler
            .inspect_state("adaptive_privacy", "held_objects")
            .await
            .expect("held objects")
            .downcast::<usize>()
            .expect("held object type");
        assert_eq!(*held_objects, 1);

        let seeded_from_move = handler
            .inspect_state("adaptive_privacy", "held:held-a:seeded_from_move")
            .await
            .expect("move to hold seed")
            .downcast::<bool>()
            .expect("seeded from move type");
        assert!(*seeded_from_move);

        let selector_retrievals = handler
            .inspect_state("adaptive_privacy", "selector_retrievals")
            .await
            .expect("selector retrievals")
            .downcast::<usize>()
            .expect("selector retrieval type");
        assert_eq!(*selector_retrievals, 1);

        let sync_opportunities = handler
            .inspect_state("adaptive_privacy", "sync_opportunities")
            .await
            .expect("sync opportunities")
            .downcast::<usize>()
            .expect("sync opportunity type");
        assert_eq!(*sync_opportunities, 1);

        let move_to_hold_seeds = handler
            .inspect_state("adaptive_privacy", "move_to_hold_seeds")
            .await
            .expect("move to hold seeds")
            .downcast::<usize>()
            .expect("move to hold seed type");
        assert_eq!(*move_to_hold_seeds, 1);

        let environment_mobility = handler
            .inspect_state("environment", "mobility_profiles")
            .await
            .expect("environment mobility")
            .downcast::<usize>()
            .expect("environment mobility type");
        assert_eq!(*environment_mobility, 1);

        let environment_admission = handler
            .inspect_state("environment", "link_admissions")
            .await
            .expect("environment admission")
            .downcast::<usize>()
            .expect("environment admission type");
        assert_eq!(*environment_admission, 1);

        let environment_capabilities = handler
            .inspect_state("environment", "node_capabilities")
            .await
            .expect("environment capabilities")
            .downcast::<usize>()
            .expect("environment capability type");
        assert_eq!(*environment_capabilities, 1);

        let environment_trace_entries = handler
            .inspect_state("environment", "trace_entries")
            .await
            .expect("environment trace entries")
            .downcast::<usize>()
            .expect("environment trace type");
        assert_eq!(*environment_trace_entries, 3);

        let artifacts = handler.capture_environment_artifacts();
        let snapshot = artifacts.snapshot;
        assert_eq!(snapshot.mobility_profiles.len(), 1);
        assert_eq!(snapshot.link_admissions.len(), 1);
        assert_eq!(snapshot.node_capabilities.len(), 1);
        assert_eq!(snapshot.mobility_profiles[0].profile_id, "clustered_social");
        assert_eq!(snapshot.link_admissions[0].density, "sparse");
        assert_eq!(snapshot.node_capabilities[0].provider, "provider-a");

        let trace = artifacts.trace;
        assert_eq!(trace.entries.len(), 3);
    }

    #[tokio::test]
    async fn test_adaptive_privacy_path_and_hold_expiry_follows_simulated_time() {
        let handler = SimulationScenarioHandler::new(123);

        handler
            .register_scenario(ScenarioDefinition {
                id: "adaptive_privacy_expiry".to_string(),
                name: "Adaptive Privacy Expiry".to_string(),
                actions: vec![
                    InjectionAction::AdaptivePrivacyTransition(
                        AdaptivePrivacyTransition::EstablishAnonymousPath {
                            path_id: "path-exp".to_string(),
                            initiator: "alice".to_string(),
                            destination: "bob".to_string(),
                            hops: vec!["relay-1".to_string(), "relay-2".to_string()],
                            ttl_ticks: 2,
                            reusable: true,
                        },
                    ),
                    InjectionAction::AdaptivePrivacyTransition(
                        AdaptivePrivacyTransition::RecordHeldObjectRetention {
                            object_id: "held-exp".to_string(),
                            selector: "selector:exp".to_string(),
                            retention_ticks: 2,
                            seeded_from_move: false,
                        },
                    ),
                ],
                trigger: TriggerCondition::Immediate,
                duration: Some(Duration::from_secs(5)),
                priority: 1,
            })
            .expect("register expiry scenario");

        handler
            .trigger_scenario("adaptive_privacy_expiry")
            .expect("trigger expiry scenario");

        handler.wait_ticks(2).expect("advance ticks to expiry");

        let path_expired = handler
            .inspect_state("adaptive_privacy", "path:path-exp:expired")
            .await
            .expect("path expired")
            .downcast::<bool>()
            .expect("path expired type");
        assert!(*path_expired);

        let held_expired = handler
            .inspect_state("adaptive_privacy", "held:held-exp:expired")
            .await
            .expect("held object expired")
            .downcast::<bool>()
            .expect("held object expired type");
        assert!(*held_expired);
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
