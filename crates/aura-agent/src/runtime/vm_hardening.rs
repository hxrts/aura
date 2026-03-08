//! Telltale VM hardening profiles and parity-lane configuration for Aura.

use aura_core::AuraVmDeterminismProfileV1;
use aura_mpst::termination::{compute_weighted_measure, SessionBufferSnapshot};
use aura_protocol::termination::TerminationProtocolClass;
use telltale_vm::loader::CodeImage;
use telltale_vm::vm::{FlowPolicy, FlowPredicate, GuardLayerConfig};
use telltale_vm::{
    CommunicationReplayMode, DeterminismMode, EffectDeterminismTier, EffectTraceCaptureMode,
    MonitorMode, OutputConditionPolicy, PayloadValidationMode, PriorityPolicy, SchedPolicy,
    VMConfig,
};

/// Default output predicate when no operation-specific hint is provided.
pub const AURA_OUTPUT_PREDICATE_OBSERVABLE: &str = "aura.observable_output";
/// Legacy VM default output predicate retained for compatibility.
pub const AURA_OUTPUT_PREDICATE_VM_OBSERVABLE: &str = "vm.observable_output";
/// Output predicate used for transport send visibility.
pub const AURA_OUTPUT_PREDICATE_TRANSPORT_SEND: &str = "aura.transport.send";
/// Output predicate used for transport receive visibility.
pub const AURA_OUTPUT_PREDICATE_TRANSPORT_RECV: &str = "aura.transport.recv";
/// Output predicate used for choice/branch visibility.
pub const AURA_OUTPUT_PREDICATE_CHOICE: &str = "aura.protocol.choice";
/// Output predicate used for invoke/step visibility.
pub const AURA_OUTPUT_PREDICATE_STEP: &str = "aura.protocol.step";
/// Output predicate used for guard acquire visibility.
pub const AURA_OUTPUT_PREDICATE_GUARD_ACQUIRE: &str = "aura.guard.acquire";
/// Output predicate used for guard release visibility.
pub const AURA_OUTPUT_PREDICATE_GUARD_RELEASE: &str = "aura.guard.release";
/// Production determinism policy reference for generic short-running protocols.
pub const AURA_VM_POLICY_PROD_DEFAULT: &str = "aura.vm.prod.default";
/// Production determinism policy reference for consensus fallback protocols.
pub const AURA_VM_POLICY_CONSENSUS_FALLBACK: &str = "aura.vm.consensus_fallback.prod";
/// Production determinism policy reference for consensus fast-path protocols.
pub const AURA_VM_POLICY_CONSENSUS_FAST_PATH: &str = "aura.vm.consensus_fast_path.prod";
/// Production determinism policy reference for DKG protocols.
pub const AURA_VM_POLICY_DKG_CEREMONY: &str = "aura.vm.dkg_ceremony.prod";
/// Production determinism policy reference for recovery-grant style protocols.
pub const AURA_VM_POLICY_RECOVERY_GRANT: &str = "aura.vm.recovery_grant.prod";
/// Production determinism policy reference for sync/anti-entropy protocols.
pub const AURA_VM_POLICY_SYNC_ANTI_ENTROPY: &str = "aura.vm.sync_anti_entropy.prod";
/// Scheduler policy reference for fair low-pressure workloads.
pub const AURA_VM_SCHED_ROUND_ROBIN: &str = "aura.vm.scheduler.round_robin";
/// Scheduler policy reference for token-biased heavy workloads.
pub const AURA_VM_SCHED_PROGRESS_AWARE: &str = "aura.vm.scheduler.progress_aware";
/// Scheduler policy reference for contention/budget-constrained workloads.
pub const AURA_VM_SCHED_PRIORITY_AGING: &str = "aura.vm.scheduler.priority_aging";

/// Typed guard-layer identifiers reserved by Aura runtime profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuraVmGuardLayer {
    /// Serializes migration/reconfiguration critical sections.
    MigrationLock,
    /// Reserves scarce slots for high-value protocol operations.
    HighValueProtocolSlot,
}

impl AuraVmGuardLayer {
    /// Stable VM layer identifier.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::MigrationLock => "aura.guard.migration_lock",
            Self::HighValueProtocolSlot => "aura.guard.high_value_protocol_slot",
        }
    }

    /// All reserved guard layers configured by Aura VM profiles.
    #[must_use]
    pub fn all() -> [Self; 2] {
        [Self::MigrationLock, Self::HighValueProtocolSlot]
    }
}

/// Profile-level VM hardening in Aura runtime contexts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuraVmHardeningProfile {
    /// Local developer profile: strict assertions with verbose diagnostics.
    Dev,
    /// CI profile: strictest commit/flow gates and replay-consumption checks.
    Ci,
    /// Production profile: safety-preserving defaults with bounded overhead.
    #[default]
    Prod,
}

impl AuraVmHardeningProfile {
    /// Parse profile from a stable textual identifier.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "dev" => Some(Self::Dev),
            "ci" => Some(Self::Ci),
            "prod" | "production" => Some(Self::Prod),
            _ => None,
        }
    }
}

/// Cross-target parity lane profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuraVmParityProfile {
    /// Native cooperative scheduler lane.
    NativeCooperative,
    /// Native threaded lane.
    NativeThreaded,
    /// WASM cooperative lane.
    WasmCooperative,
    /// Runtime default lane for non-parity production execution.
    #[default]
    RuntimeDefault,
}

/// Determinism profile parse/validation error.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AuraVmDeterminismProfileError {
    /// Unknown determinism mode string.
    #[error("unknown determinism mode: {raw}")]
    UnknownDeterminismMode {
        /// Invalid input value.
        raw: String,
    },
    /// Unknown effect determinism tier string.
    #[error("unknown effect determinism tier: {raw}")]
    UnknownEffectDeterminismTier {
        /// Invalid input value.
        raw: String,
    },
    /// Unknown communication replay mode string.
    #[error("unknown communication replay mode: {raw}")]
    UnknownCommunicationReplayMode {
        /// Invalid input value.
        raw: String,
    },
    /// Unknown VM policy selector.
    #[error("unknown VM determinism policy reference: {raw}")]
    UnknownPolicyRef {
        /// Invalid input value.
        raw: String,
    },
    /// Invalid profile combination in VM config.
    #[error(
        "invalid determinism profile combination: mode={mode:?}, tier={tier:?}, replay={replay:?} ({reason})"
    )]
    InvalidCombination {
        /// Determinism mode.
        mode: DeterminismMode,
        /// Effect determinism tier.
        tier: EffectDeterminismTier,
        /// Communication replay mode.
        replay: CommunicationReplayMode,
        /// Human-readable reason.
        reason: &'static str,
    },
    /// Selected policy reference does not match protocol class.
    #[error(
        "determinism policy {policy_ref} is incompatible with protocol class {protocol_class} for protocol {protocol_id}"
    )]
    PolicyClassMismatch {
        /// Selected policy reference.
        policy_ref: String,
        /// Protocol identifier being admitted.
        protocol_id: String,
        /// Resolved Aura protocol class.
        protocol_class: String,
    },
    /// Selected scheduler policy does not match the runtime recommendation.
    #[error(
        "scheduler policy {actual_policy_ref} does not match recommended policy {expected_policy_ref} for protocol class {protocol_class} (initial_weight={initial_weight}, guard_capacity_slots={guard_capacity_slots})"
    )]
    SchedulerPolicyMismatch {
        /// Recommended stable scheduler policy reference.
        expected_policy_ref: String,
        /// Actual configured scheduler policy reference.
        actual_policy_ref: String,
        /// Resolved Aura protocol class.
        protocol_class: String,
        /// Initial weighted measure for the admitted image.
        initial_weight: u64,
        /// Effective guard capacity used for the decision.
        guard_capacity_slots: usize,
    },
}

/// Canonical VM execution policy bound to one Aura protocol class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuraVmProtocolExecutionPolicy {
    /// Stable policy selector reference.
    pub policy_ref: &'static str,
    /// Aura protocol class.
    pub protocol_class: TerminationProtocolClass,
    /// Telltale determinism mode.
    pub determinism_mode: DeterminismMode,
    /// Telltale effect determinism tier.
    pub effect_determinism_tier: EffectDeterminismTier,
    /// Telltale communication replay mode.
    pub communication_replay_mode: CommunicationReplayMode,
}

/// Runtime signals that influence scheduler selection without bypassing the VM scheduler.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AuraVmSchedulerSignals {
    /// Recent guard contention events observed by the host bridge.
    pub guard_contention_events: u64,
    /// Flow-budget pressure in basis points (`0..=10_000`).
    pub flow_budget_pressure_bps: u16,
    /// Leakage-budget pressure in basis points (`0..=10_000`).
    pub leakage_budget_pressure_bps: u16,
}

impl AuraVmSchedulerSignals {
    /// Clamp pressure signals to the representable basis-point range.
    #[must_use]
    pub fn normalized(self) -> Self {
        Self {
            guard_contention_events: self.guard_contention_events,
            flow_budget_pressure_bps: self.flow_budget_pressure_bps.min(10_000),
            leakage_budget_pressure_bps: self.leakage_budget_pressure_bps.min(10_000),
        }
    }
}

/// Host-side scheduler selection input for one admitted VM session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuraVmSchedulerControlInput {
    /// Aura protocol class for the admitted session.
    pub protocol_class: TerminationProtocolClass,
    /// Initial Telltale weighted progress measure.
    pub initial_weight: u64,
    /// Effective active guard capacity available to the VM.
    pub guard_capacity_slots: usize,
    /// Host bridge pressure/contention signals.
    pub signals: AuraVmSchedulerSignals,
}

/// Canonical scheduler decision selected by the host for one VM session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuraVmSchedulerExecutionPolicy {
    /// Stable scheduler policy selector reference.
    pub policy_ref: &'static str,
    /// Concrete Telltale scheduler policy to configure.
    pub sched_policy: SchedPolicy,
}

/// Host handlers that can surface scheduler-control signals at admission time.
pub trait AuraVmSchedulerSignalsProvider {
    /// Snapshot scheduler signals visible to the host bridge.
    fn scheduler_signals(&self) -> AuraVmSchedulerSignals;
}

impl AuraVmProtocolExecutionPolicy {
    /// Lossless metadata snapshot suitable for conformance/trace artifacts.
    #[must_use]
    pub fn artifact_metadata(self) -> AuraVmDeterminismProfileV1 {
        AuraVmDeterminismProfileV1 {
            policy_ref: self.policy_ref.to_string(),
            protocol_class: self.protocol_class.to_string(),
            determinism_mode: determinism_mode_ref(self.determinism_mode).to_string(),
            effect_determinism_tier: effect_determinism_tier_ref(
                self.effect_determinism_tier,
            )
            .to_string(),
            communication_replay_mode: communication_replay_mode_ref(
                self.communication_replay_mode,
            )
            .to_string(),
        }
    }
}

/// Stable textual identifier for one determinism mode.
#[must_use]
pub const fn determinism_mode_ref(mode: DeterminismMode) -> &'static str {
    match mode {
        DeterminismMode::Full => "full",
        DeterminismMode::ModuloEffects => "modulo_effects",
        DeterminismMode::ModuloCommutativity => "modulo_commutativity",
        DeterminismMode::Replay => "replay",
    }
}

/// Stable textual identifier for one effect determinism tier.
#[must_use]
pub const fn effect_determinism_tier_ref(tier: EffectDeterminismTier) -> &'static str {
    match tier {
        EffectDeterminismTier::StrictDeterministic => "strict_deterministic",
        EffectDeterminismTier::ReplayDeterministic => "replay_deterministic",
        EffectDeterminismTier::EnvelopeBoundedNondeterministic => {
            "envelope_bounded_nondeterministic"
        }
    }
}

/// Stable textual identifier for one communication replay mode.
#[must_use]
pub const fn communication_replay_mode_ref(mode: CommunicationReplayMode) -> &'static str {
    match mode {
        CommunicationReplayMode::Off => "off",
        CommunicationReplayMode::Sequence => "sequence",
        CommunicationReplayMode::Nullifier => "nullifier",
    }
}

/// Canonical execution policy for one stable policy selector.
///
/// # Errors
///
/// Returns [`AuraVmDeterminismProfileError::UnknownPolicyRef`] for unsupported selectors.
pub fn policy_for_ref(
    policy_ref: &str,
) -> Result<AuraVmProtocolExecutionPolicy, AuraVmDeterminismProfileError> {
    match policy_ref {
        AURA_VM_POLICY_PROD_DEFAULT => Ok(AuraVmProtocolExecutionPolicy {
            policy_ref: AURA_VM_POLICY_PROD_DEFAULT,
            protocol_class: TerminationProtocolClass::RecoveryGrant,
            determinism_mode: DeterminismMode::Full,
            effect_determinism_tier: EffectDeterminismTier::StrictDeterministic,
            communication_replay_mode: CommunicationReplayMode::Off,
        }),
        AURA_VM_POLICY_CONSENSUS_FALLBACK => Ok(AuraVmProtocolExecutionPolicy {
            policy_ref: AURA_VM_POLICY_CONSENSUS_FALLBACK,
            protocol_class: TerminationProtocolClass::ConsensusFallback,
            determinism_mode: DeterminismMode::ModuloCommutativity,
            effect_determinism_tier: EffectDeterminismTier::ReplayDeterministic,
            communication_replay_mode: CommunicationReplayMode::Sequence,
        }),
        AURA_VM_POLICY_CONSENSUS_FAST_PATH => Ok(AuraVmProtocolExecutionPolicy {
            policy_ref: AURA_VM_POLICY_CONSENSUS_FAST_PATH,
            protocol_class: TerminationProtocolClass::ConsensusFastPath,
            determinism_mode: DeterminismMode::Full,
            effect_determinism_tier: EffectDeterminismTier::StrictDeterministic,
            communication_replay_mode: CommunicationReplayMode::Sequence,
        }),
        AURA_VM_POLICY_DKG_CEREMONY => Ok(AuraVmProtocolExecutionPolicy {
            policy_ref: AURA_VM_POLICY_DKG_CEREMONY,
            protocol_class: TerminationProtocolClass::DkgCeremony,
            determinism_mode: DeterminismMode::Replay,
            effect_determinism_tier: EffectDeterminismTier::ReplayDeterministic,
            communication_replay_mode: CommunicationReplayMode::Sequence,
        }),
        AURA_VM_POLICY_RECOVERY_GRANT => Ok(AuraVmProtocolExecutionPolicy {
            policy_ref: AURA_VM_POLICY_RECOVERY_GRANT,
            protocol_class: TerminationProtocolClass::RecoveryGrant,
            determinism_mode: DeterminismMode::Full,
            effect_determinism_tier: EffectDeterminismTier::StrictDeterministic,
            communication_replay_mode: CommunicationReplayMode::Off,
        }),
        AURA_VM_POLICY_SYNC_ANTI_ENTROPY => Ok(AuraVmProtocolExecutionPolicy {
            policy_ref: AURA_VM_POLICY_SYNC_ANTI_ENTROPY,
            protocol_class: TerminationProtocolClass::SyncAntiEntropy,
            determinism_mode: DeterminismMode::ModuloEffects,
            effect_determinism_tier: EffectDeterminismTier::EnvelopeBoundedNondeterministic,
            communication_replay_mode: CommunicationReplayMode::Sequence,
        }),
        _ => Err(AuraVmDeterminismProfileError::UnknownPolicyRef {
            raw: policy_ref.to_string(),
        }),
    }
}

/// Canonical execution policy for one protocol id plus optional manifest selector.
///
/// # Errors
///
/// Returns an error when the protocol id is unknown or the selected policy ref is incompatible
/// with the resolved protocol class.
pub fn policy_for_protocol(
    protocol_id: &str,
    policy_ref: Option<&str>,
) -> Result<AuraVmProtocolExecutionPolicy, AuraVmDeterminismProfileError> {
    let protocol_class = TerminationProtocolClass::from_protocol_id(protocol_id).ok_or_else(|| {
        AuraVmDeterminismProfileError::PolicyClassMismatch {
            policy_ref: policy_ref.unwrap_or(AURA_VM_POLICY_PROD_DEFAULT).to_string(),
            protocol_id: protocol_id.to_string(),
            protocol_class: "unknown".to_string(),
        }
    })?;
    let selected_policy_ref = policy_ref.unwrap_or(match protocol_class {
        TerminationProtocolClass::ConsensusFastPath => AURA_VM_POLICY_CONSENSUS_FAST_PATH,
        TerminationProtocolClass::ConsensusFallback => AURA_VM_POLICY_CONSENSUS_FALLBACK,
        TerminationProtocolClass::SyncAntiEntropy => AURA_VM_POLICY_SYNC_ANTI_ENTROPY,
        TerminationProtocolClass::DkgCeremony => AURA_VM_POLICY_DKG_CEREMONY,
        TerminationProtocolClass::RecoveryGrant => AURA_VM_POLICY_RECOVERY_GRANT,
    });
    let policy = policy_for_ref(selected_policy_ref)?;
    if policy.protocol_class != protocol_class {
        return Err(AuraVmDeterminismProfileError::PolicyClassMismatch {
            policy_ref: selected_policy_ref.to_string(),
            protocol_id: protocol_id.to_string(),
            protocol_class: protocol_class.to_string(),
        });
    }
    Ok(policy)
}

/// Apply the selected protocol execution policy onto a VM config.
pub fn apply_protocol_execution_policy(
    config: &mut VMConfig,
    policy: AuraVmProtocolExecutionPolicy,
) {
    config.determinism_mode = policy.determinism_mode;
    config.effect_determinism_tier = policy.effect_determinism_tier;
    config.communication_replay_mode = policy.communication_replay_mode;
}

/// Stable textual identifier for one scheduler policy.
#[must_use]
pub fn scheduler_policy_ref(policy: &SchedPolicy) -> &'static str {
    match policy {
        SchedPolicy::Cooperative => "cooperative",
        SchedPolicy::RoundRobin => AURA_VM_SCHED_ROUND_ROBIN,
        SchedPolicy::Priority(PriorityPolicy::Aging) => AURA_VM_SCHED_PRIORITY_AGING,
        SchedPolicy::Priority(PriorityPolicy::TokenWeighted) => {
            "aura.vm.scheduler.priority_token_weighted"
        }
        SchedPolicy::Priority(PriorityPolicy::FixedMap(_)) => "aura.vm.scheduler.priority_fixed",
        SchedPolicy::ProgressAware => AURA_VM_SCHED_PROGRESS_AWARE,
    }
}

/// Effective guard capacity available in this VM config.
#[must_use]
pub fn configured_guard_capacity(config: &VMConfig) -> usize {
    config
        .guard_layers
        .iter()
        .filter(|layer| layer.active)
        .count()
        .max(1)
}

/// Compute scheduler-selection input for one admitted code image.
#[must_use]
pub fn scheduler_control_input_for_image(
    image: &CodeImage,
    protocol_class: TerminationProtocolClass,
    guard_capacity_slots: usize,
    signals: AuraVmSchedulerSignals,
) -> AuraVmSchedulerControlInput {
    let local_types = image.local_types.values().cloned().collect::<Vec<_>>();
    let initial_weight = compute_weighted_measure(&local_types, &SessionBufferSnapshot::new());
    AuraVmSchedulerControlInput {
        protocol_class,
        initial_weight,
        guard_capacity_slots: guard_capacity_slots.max(1),
        signals: signals.normalized(),
    }
}

/// Canonical scheduler policy for one admitted session.
#[must_use]
pub fn scheduler_policy_for_input(
    input: AuraVmSchedulerControlInput,
) -> AuraVmSchedulerExecutionPolicy {
    let high_budget_pressure = input.signals.flow_budget_pressure_bps >= 7_500
        || input.signals.leakage_budget_pressure_bps >= 7_500;
    let guard_constrained =
        input.guard_capacity_slots <= 1 || input.signals.guard_contention_events > 0;
    let heavy_protocol = matches!(
        input.protocol_class,
        TerminationProtocolClass::ConsensusFallback
            | TerminationProtocolClass::SyncAntiEntropy
            | TerminationProtocolClass::DkgCeremony
    );
    let (_, high_weight) = input.protocol_class.expected_weight_range();
    let heavy_weight = input.initial_weight >= high_weight;

    if guard_constrained || high_budget_pressure {
        AuraVmSchedulerExecutionPolicy {
            policy_ref: AURA_VM_SCHED_PRIORITY_AGING,
            sched_policy: SchedPolicy::Priority(PriorityPolicy::Aging),
        }
    } else if heavy_protocol || heavy_weight {
        AuraVmSchedulerExecutionPolicy {
            policy_ref: AURA_VM_SCHED_PROGRESS_AWARE,
            sched_policy: SchedPolicy::ProgressAware,
        }
    } else {
        AuraVmSchedulerExecutionPolicy {
            policy_ref: AURA_VM_SCHED_ROUND_ROBIN,
            sched_policy: SchedPolicy::RoundRobin,
        }
    }
}

/// Apply the selected scheduler execution policy onto a VM config.
pub fn apply_scheduler_execution_policy(
    config: &mut VMConfig,
    policy: &AuraVmSchedulerExecutionPolicy,
) {
    config.sched_policy = policy.sched_policy.clone();
}

/// Validate that a VM config matches the selected scheduler execution policy.
///
/// # Errors
///
/// Returns [`AuraVmDeterminismProfileError::SchedulerPolicyMismatch`] when the config drifts
/// from the recommended scheduler policy.
pub fn validate_scheduler_execution_policy(
    config: &VMConfig,
    input: AuraVmSchedulerControlInput,
) -> Result<AuraVmSchedulerExecutionPolicy, AuraVmDeterminismProfileError> {
    let expected = scheduler_policy_for_input(input);
    if config.sched_policy != expected.sched_policy {
        return Err(AuraVmDeterminismProfileError::SchedulerPolicyMismatch {
            expected_policy_ref: expected.policy_ref.to_string(),
            actual_policy_ref: scheduler_policy_ref(&config.sched_policy).to_string(),
            protocol_class: input.protocol_class.to_string(),
            initial_weight: input.initial_weight,
            guard_capacity_slots: input.guard_capacity_slots,
        });
    }
    Ok(expected)
}

/// Validate that a VM config matches the selected protocol execution policy.
///
/// # Errors
///
/// Returns [`AuraVmDeterminismProfileError::InvalidCombination`] when the config drifts from the
/// selected policy dimensions.
pub fn validate_protocol_execution_policy(
    config: &VMConfig,
    policy: AuraVmProtocolExecutionPolicy,
) -> Result<(), AuraVmDeterminismProfileError> {
    validate_determinism_profile(config)?;

    if config.determinism_mode != policy.determinism_mode {
        return Err(AuraVmDeterminismProfileError::InvalidCombination {
            mode: config.determinism_mode,
            tier: config.effect_determinism_tier,
            replay: config.communication_replay_mode,
            reason: "vm determinism mode does not match protocol policy",
        });
    }
    if config.effect_determinism_tier != policy.effect_determinism_tier {
        return Err(AuraVmDeterminismProfileError::InvalidCombination {
            mode: config.determinism_mode,
            tier: config.effect_determinism_tier,
            replay: config.communication_replay_mode,
            reason: "vm effect determinism tier does not match protocol policy",
        });
    }
    if config.communication_replay_mode != policy.communication_replay_mode {
        return Err(AuraVmDeterminismProfileError::InvalidCombination {
            mode: config.determinism_mode,
            tier: config.effect_determinism_tier,
            replay: config.communication_replay_mode,
            reason: "vm communication replay mode does not match protocol policy",
        });
    }

    Ok(())
}

/// Parse `DeterminismMode` from a stable textual identifier.
///
/// # Errors
///
/// Returns [`AuraVmDeterminismProfileError::UnknownDeterminismMode`] for unknown values.
pub fn parse_determinism_mode(raw: &str) -> Result<DeterminismMode, AuraVmDeterminismProfileError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "full" => Ok(DeterminismMode::Full),
        "modulo_effects" | "modulo-effects" => Ok(DeterminismMode::ModuloEffects),
        "modulo_commutativity" | "modulo-commutativity" => Ok(DeterminismMode::ModuloCommutativity),
        "replay" => Ok(DeterminismMode::Replay),
        _ => Err(AuraVmDeterminismProfileError::UnknownDeterminismMode {
            raw: raw.to_string(),
        }),
    }
}

/// Parse `EffectDeterminismTier` from a stable textual identifier.
///
/// # Errors
///
/// Returns [`AuraVmDeterminismProfileError::UnknownEffectDeterminismTier`] for unknown values.
pub fn parse_effect_determinism_tier(
    raw: &str,
) -> Result<EffectDeterminismTier, AuraVmDeterminismProfileError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "strict_deterministic" | "strict-deterministic" => {
            Ok(EffectDeterminismTier::StrictDeterministic)
        }
        "replay_deterministic" | "replay-deterministic" => {
            Ok(EffectDeterminismTier::ReplayDeterministic)
        }
        "envelope_bounded_nondeterministic" | "envelope-bounded-nondeterministic" => {
            Ok(EffectDeterminismTier::EnvelopeBoundedNondeterministic)
        }
        _ => Err(
            AuraVmDeterminismProfileError::UnknownEffectDeterminismTier {
                raw: raw.to_string(),
            },
        ),
    }
}

/// Parse `CommunicationReplayMode` from a stable textual identifier.
///
/// # Errors
///
/// Returns [`AuraVmDeterminismProfileError::UnknownCommunicationReplayMode`] for unknown values.
pub fn parse_communication_replay_mode(
    raw: &str,
) -> Result<CommunicationReplayMode, AuraVmDeterminismProfileError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "off" => Ok(CommunicationReplayMode::Off),
        "sequence" => Ok(CommunicationReplayMode::Sequence),
        "nullifier" => Ok(CommunicationReplayMode::Nullifier),
        _ => Err(
            AuraVmDeterminismProfileError::UnknownCommunicationReplayMode {
                raw: raw.to_string(),
            },
        ),
    }
}

/// Validate determinism/profile requirements encoded in VM config.
///
/// # Errors
///
/// Returns [`AuraVmDeterminismProfileError::InvalidCombination`] when the configured profile is
/// inconsistent with Aura admission constraints.
pub fn validate_determinism_profile(
    config: &VMConfig,
) -> Result<(), AuraVmDeterminismProfileError> {
    let mode = config.determinism_mode;
    let tier = config.effect_determinism_tier;
    let replay = config.communication_replay_mode;

    if mode == DeterminismMode::Full && tier != EffectDeterminismTier::StrictDeterministic {
        return Err(AuraVmDeterminismProfileError::InvalidCombination {
            mode,
            tier,
            replay,
            reason: "full determinism requires strict effect determinism",
        });
    }

    if tier == EffectDeterminismTier::EnvelopeBoundedNondeterministic
        && !matches!(
            mode,
            DeterminismMode::ModuloEffects | DeterminismMode::ModuloCommutativity
        )
    {
        return Err(AuraVmDeterminismProfileError::InvalidCombination {
            mode,
            tier,
            replay,
            reason: "envelope-bounded effect tier requires a modulo determinism mode",
        });
    }

    if replay == CommunicationReplayMode::Nullifier && mode == DeterminismMode::Full {
        return Err(AuraVmDeterminismProfileError::InvalidCombination {
            mode,
            tier,
            replay,
            reason: "nullifier replay mode is incompatible with full determinism mode",
        });
    }

    Ok(())
}

/// Output predicates accepted by Aura hardening policies.
#[must_use]
pub fn aura_output_predicate_allow_list() -> Vec<String> {
    [
        AURA_OUTPUT_PREDICATE_OBSERVABLE,
        AURA_OUTPUT_PREDICATE_VM_OBSERVABLE,
        AURA_OUTPUT_PREDICATE_TRANSPORT_SEND,
        AURA_OUTPUT_PREDICATE_TRANSPORT_RECV,
        AURA_OUTPUT_PREDICATE_CHOICE,
        AURA_OUTPUT_PREDICATE_STEP,
        AURA_OUTPUT_PREDICATE_GUARD_ACQUIRE,
        AURA_OUTPUT_PREDICATE_GUARD_RELEASE,
    ]
    .iter()
    .map(ToString::to_string)
    .collect()
}

fn aura_guard_layers() -> Vec<GuardLayerConfig> {
    AuraVmGuardLayer::all()
        .into_iter()
        .map(|layer| GuardLayerConfig {
            id: layer.id().to_string(),
            active: true,
        })
        .collect()
}

/// Serializable flow-policy predicate for Aura role/category constraints.
#[must_use]
pub fn aura_flow_policy_predicate() -> FlowPredicate {
    FlowPredicate::Any(vec![
        FlowPredicate::TargetRolePrefix("Proposer".to_string()),
        FlowPredicate::TargetRolePrefix("Witness".to_string()),
        FlowPredicate::TargetRolePrefix("Observer".to_string()),
        FlowPredicate::TargetRolePrefix("Committer".to_string()),
        FlowPredicate::TargetRolePrefix("Issuer".to_string()),
        FlowPredicate::TargetRolePrefix("Invitee".to_string()),
        FlowPredicate::TargetRolePrefix("Inviter".to_string()),
        FlowPredicate::TargetRolePrefix("Requester".to_string()),
        FlowPredicate::TargetRolePrefix("Guardian".to_string()),
        FlowPredicate::TargetRolePrefix("Primary".to_string()),
        FlowPredicate::TargetRolePrefix("Replica".to_string()),
        FlowPredicate::TargetRolePrefix("Sync".to_string()),
        FlowPredicate::TargetRolePrefix("Relay".to_string()),
        FlowPredicate::TargetRolePrefix("Context".to_string()),
    ])
}

fn apply_hardening_profile(config: &mut VMConfig, profile: AuraVmHardeningProfile) {
    config.monitor_mode = MonitorMode::SessionTypePrecheck;
    config.host_contract_assertions = true;
    config.output_condition_policy =
        OutputConditionPolicy::PredicateAllowList(aura_output_predicate_allow_list());
    config.flow_policy = FlowPolicy::PredicateExpr(aura_flow_policy_predicate());
    config.guard_layers = aura_guard_layers();
    config.payload_validation_mode = PayloadValidationMode::Structural;
    config.effect_trace_capture_mode = EffectTraceCaptureMode::Full;
    config.max_payload_bytes = 256 * 1024;

    match profile {
        AuraVmHardeningProfile::Dev => {
            config.communication_replay_mode = CommunicationReplayMode::Off;
            config.sched_policy = SchedPolicy::Cooperative;
        }
        AuraVmHardeningProfile::Ci => {
            config.communication_replay_mode = CommunicationReplayMode::Sequence;
            // Keep CI strict without requiring schema annotations on every test choreography.
            config.payload_validation_mode = PayloadValidationMode::Structural;
        }
        AuraVmHardeningProfile::Prod => {
            config.communication_replay_mode = CommunicationReplayMode::Off;
            config.effect_trace_capture_mode = EffectTraceCaptureMode::TopologyOnly;
        }
    }
}

fn apply_parity_profile(config: &mut VMConfig, parity: AuraVmParityProfile) {
    match parity {
        AuraVmParityProfile::NativeCooperative | AuraVmParityProfile::WasmCooperative => {
            config.sched_policy = SchedPolicy::Cooperative;
            config.determinism_mode = DeterminismMode::Full;
            config.effect_determinism_tier = EffectDeterminismTier::StrictDeterministic;
        }
        AuraVmParityProfile::NativeThreaded => {
            config.sched_policy = SchedPolicy::RoundRobin;
            config.determinism_mode = DeterminismMode::ModuloCommutativity;
            config.effect_determinism_tier = EffectDeterminismTier::ReplayDeterministic;
        }
        AuraVmParityProfile::RuntimeDefault => {}
    }
}

/// Build a VM config with explicit hardening and parity profiles.
#[must_use]
pub fn build_vm_config(hardening: AuraVmHardeningProfile, parity: AuraVmParityProfile) -> VMConfig {
    let mut config = VMConfig::default();
    apply_hardening_profile(&mut config, hardening);
    apply_parity_profile(&mut config, parity);
    config
}

/// Build a VM config from hardening profile only.
#[must_use]
pub fn vm_config_for_profile(hardening: AuraVmHardeningProfile) -> VMConfig {
    build_vm_config(hardening, AuraVmParityProfile::RuntimeDefault)
}

#[cfg(test)]
mod tests {
    use super::*;
    use telltale_types::{GlobalType, Label};
    use telltale_vm::coroutine::KnowledgeFact;
    use telltale_vm::effect::EffectHandler;
    use telltale_vm::instr::Endpoint;
    use telltale_vm::loader::CodeImage;
    use telltale_vm::vm::{RunStatus, VM};
    use telltale_vm::{verify_output_condition, OutputConditionMeta, Value};

    #[derive(Default)]
    struct UnitHandler;

    impl EffectHandler for UnitHandler {
        fn handle_send(
            &self,
            _role: &str,
            _partner: &str,
            _label: &str,
            _state: &[Value],
        ) -> Result<Value, String> {
            Ok(Value::Unit)
        }

        fn handle_recv(
            &self,
            _role: &str,
            _partner: &str,
            _label: &str,
            _state: &mut Vec<Value>,
            _payload: &Value,
        ) -> Result<(), String> {
            Ok(())
        }

        fn handle_choose(
            &self,
            _role: &str,
            _partner: &str,
            labels: &[String],
            _state: &[Value],
        ) -> Result<String, String> {
            labels
                .first()
                .cloned()
                .ok_or_else(|| "no labels available".to_string())
        }

        fn step(&self, _role: &str, _state: &mut Vec<Value>) -> Result<(), String> {
            Ok(())
        }
    }

    fn flow_compatibility_image() -> CodeImage {
        let global = GlobalType::send(
            "Primary",
            "Replica",
            Label::new("delta"),
            GlobalType::send("Replica", "Primary", Label::new("receipt"), GlobalType::End),
        );
        let locals = telltale_theory::projection::project_all(&global)
            .expect("projection must succeed")
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>();
        CodeImage::from_local_types(&locals, &global)
    }

    #[test]
    fn profile_parser_handles_known_values() {
        assert_eq!(
            AuraVmHardeningProfile::parse("dev"),
            Some(AuraVmHardeningProfile::Dev)
        );
        assert_eq!(
            AuraVmHardeningProfile::parse("CI"),
            Some(AuraVmHardeningProfile::Ci)
        );
        assert_eq!(
            AuraVmHardeningProfile::parse("production"),
            Some(AuraVmHardeningProfile::Prod)
        );
        assert_eq!(AuraVmHardeningProfile::parse("unknown"), None);
    }

    #[test]
    fn ci_profile_denies_unknown_output_predicates() {
        let config = build_vm_config(
            AuraVmHardeningProfile::Ci,
            AuraVmParityProfile::NativeCooperative,
        );
        let unknown = OutputConditionMeta {
            predicate_ref: "aura.unknown".to_string(),
            witness_ref: Some("witness-x".to_string()),
            output_digest: "digest".to_string(),
        };
        assert!(
            !verify_output_condition(&config.output_condition_policy, &unknown),
            "ci profile must reject unknown output predicates"
        );

        let known = OutputConditionMeta {
            predicate_ref: AURA_OUTPUT_PREDICATE_TRANSPORT_SEND.to_string(),
            witness_ref: None,
            output_digest: "digest".to_string(),
        };
        assert!(
            verify_output_condition(&config.output_condition_policy, &known),
            "ci profile must admit known output predicates"
        );
    }

    #[test]
    fn flow_policy_predicate_blocks_unclassified_targets() {
        let config = build_vm_config(
            AuraVmHardeningProfile::Ci,
            AuraVmParityProfile::NativeCooperative,
        );
        let fact = KnowledgeFact {
            endpoint: Endpoint {
                sid: 1,
                role: "Proposer".to_string(),
            },
            fact: "commit-ready".to_string(),
        };
        assert!(config.flow_policy.allows_knowledge(&fact, "GuardianAlpha"));
        assert!(!config.flow_policy.allows_knowledge(&fact, "UnknownRole"));
    }

    #[test]
    fn prod_profile_keeps_bounded_overhead_defaults() {
        let config = build_vm_config(
            AuraVmHardeningProfile::Prod,
            AuraVmParityProfile::RuntimeDefault,
        );
        assert_eq!(
            config.effect_trace_capture_mode,
            EffectTraceCaptureMode::TopologyOnly
        );
        assert_eq!(
            config.communication_replay_mode,
            CommunicationReplayMode::Off
        );
        assert_eq!(config.monitor_mode, MonitorMode::SessionTypePrecheck);
        let guard_layer_ids = config
            .guard_layers
            .iter()
            .map(|layer| layer.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert!(guard_layer_ids.contains(AuraVmGuardLayer::MigrationLock.id()));
        assert!(guard_layer_ids.contains(AuraVmGuardLayer::HighValueProtocolSlot.id()));
    }

    #[test]
    fn flow_policy_preserves_send_recv_progression_for_allowed_roles() {
        let image = flow_compatibility_image();
        let handler = UnitHandler;

        let ci_config = build_vm_config(
            AuraVmHardeningProfile::Ci,
            AuraVmParityProfile::NativeCooperative,
        );
        let mut allow_all_config = ci_config.clone();
        allow_all_config.flow_policy = FlowPolicy::AllowAll;

        let mut ci_vm = VM::new(ci_config);
        ci_vm.load_choreography(&image).expect("load choreography");
        let ci_status = ci_vm.run(&handler, 64).expect("ci run");
        assert_eq!(ci_status, RunStatus::AllDone);

        let mut allow_all_vm = VM::new(allow_all_config);
        allow_all_vm
            .load_choreography(&image)
            .expect("load choreography");
        let allow_status = allow_all_vm.run(&handler, 64).expect("allow-all run");
        assert_eq!(allow_status, RunStatus::AllDone);

        let ci_effect_kinds = ci_vm
            .effect_trace()
            .iter()
            .map(|entry| entry.effect_kind.clone())
            .collect::<Vec<_>>();
        let allow_effect_kinds = allow_all_vm
            .effect_trace()
            .iter()
            .map(|entry| entry.effect_kind.clone())
            .collect::<Vec<_>>();

        assert_eq!(
            ci_effect_kinds, allow_effect_kinds,
            "Aura flow policy must preserve send/recv progression for allowed roles"
        );
    }

    #[test]
    fn determinism_profile_parsers_handle_known_values() {
        assert_eq!(
            parse_determinism_mode("modulo-commutativity"),
            Ok(DeterminismMode::ModuloCommutativity)
        );
        assert_eq!(
            parse_effect_determinism_tier("replay_deterministic"),
            Ok(EffectDeterminismTier::ReplayDeterministic)
        );
        assert_eq!(
            parse_communication_replay_mode("sequence"),
            Ok(CommunicationReplayMode::Sequence)
        );
        assert!(matches!(
            parse_determinism_mode("unknown"),
            Err(AuraVmDeterminismProfileError::UnknownDeterminismMode { .. })
        ));
    }

    #[test]
    fn determinism_profile_validation_rejects_incompatible_combinations() {
        let mut config = VMConfig {
            determinism_mode: DeterminismMode::Full,
            effect_determinism_tier: EffectDeterminismTier::ReplayDeterministic,
            communication_replay_mode: CommunicationReplayMode::Off,
            ..VMConfig::default()
        };
        assert!(matches!(
            validate_determinism_profile(&config),
            Err(AuraVmDeterminismProfileError::InvalidCombination { .. })
        ));

        config.effect_determinism_tier = EffectDeterminismTier::StrictDeterministic;
        assert!(validate_determinism_profile(&config).is_ok());
    }

    #[test]
    fn protocol_policy_mapping_is_class_driven() {
        let dkg = policy_for_protocol("aura.dkg.ceremony", Some(AURA_VM_POLICY_DKG_CEREMONY))
            .expect("dkg policy must resolve");
        assert_eq!(dkg.protocol_class, TerminationProtocolClass::DkgCeremony);
        assert_eq!(dkg.determinism_mode, DeterminismMode::Replay);
        assert_eq!(
            dkg.effect_determinism_tier,
            EffectDeterminismTier::ReplayDeterministic
        );
        assert_eq!(
            dkg.communication_replay_mode,
            CommunicationReplayMode::Sequence
        );

        let invitation = policy_for_protocol(
            "aura.invitation.exchange",
            Some(AURA_VM_POLICY_PROD_DEFAULT),
        )
        .expect("invitation policy must resolve");
        assert_eq!(invitation.protocol_class, TerminationProtocolClass::RecoveryGrant);
        assert_eq!(invitation.determinism_mode, DeterminismMode::Full);
    }

    #[test]
    fn protocol_policy_validation_rejects_class_mismatches() {
        let err = policy_for_protocol(
            "aura.sync.epoch_rotation",
            Some(AURA_VM_POLICY_RECOVERY_GRANT),
        )
        .expect_err("mismatched policy ref must fail");
        assert!(matches!(
            err,
            AuraVmDeterminismProfileError::PolicyClassMismatch { .. }
        ));
    }

    #[test]
    fn protocol_policy_validation_rejects_vm_config_drift() {
        let policy = policy_for_protocol("aura.sync.epoch_rotation", None)
            .expect("sync policy must resolve");
        let config = build_vm_config(
            AuraVmHardeningProfile::Prod,
            AuraVmParityProfile::RuntimeDefault,
        );
        assert!(matches!(
            validate_protocol_execution_policy(&config, policy),
            Err(AuraVmDeterminismProfileError::InvalidCombination { .. })
        ));

        let mut aligned = config;
        apply_protocol_execution_policy(&mut aligned, policy);
        assert!(validate_protocol_execution_policy(&aligned, policy).is_ok());
    }

    #[test]
    fn scheduler_policy_uses_progress_aware_for_heavy_protocol_classes() {
        let image = flow_compatibility_image();
        let input = scheduler_control_input_for_image(
            &image,
            TerminationProtocolClass::SyncAntiEntropy,
            2,
            AuraVmSchedulerSignals::default(),
        );
        let selected = scheduler_policy_for_input(input);
        assert_eq!(selected.policy_ref, AURA_VM_SCHED_PROGRESS_AWARE);
        assert_eq!(selected.sched_policy, SchedPolicy::ProgressAware);
    }

    #[test]
    fn scheduler_policy_uses_aging_when_capacity_or_budget_pressure_is_low() {
        let image = flow_compatibility_image();
        let input = scheduler_control_input_for_image(
            &image,
            TerminationProtocolClass::RecoveryGrant,
            1,
            AuraVmSchedulerSignals {
                guard_contention_events: 0,
                flow_budget_pressure_bps: 8_000,
                leakage_budget_pressure_bps: 0,
            },
        );
        let selected = scheduler_policy_for_input(input);
        assert_eq!(selected.policy_ref, AURA_VM_SCHED_PRIORITY_AGING);
        assert_eq!(
            selected.sched_policy,
            SchedPolicy::Priority(PriorityPolicy::Aging)
        );
    }

    #[test]
    fn scheduler_policy_validation_rejects_config_drift() {
        let image = flow_compatibility_image();
        let input = scheduler_control_input_for_image(
            &image,
            TerminationProtocolClass::RecoveryGrant,
            1,
            AuraVmSchedulerSignals {
                guard_contention_events: 2,
                flow_budget_pressure_bps: 0,
                leakage_budget_pressure_bps: 7_900,
            },
        );
        let config = build_vm_config(
            AuraVmHardeningProfile::Prod,
            AuraVmParityProfile::RuntimeDefault,
        );
        assert!(matches!(
            validate_scheduler_execution_policy(&config, input),
            Err(AuraVmDeterminismProfileError::SchedulerPolicyMismatch { .. })
        ));

        let mut aligned = config;
        let selected = scheduler_policy_for_input(input);
        apply_scheduler_execution_policy(&mut aligned, &selected);
        assert!(validate_scheduler_execution_policy(&aligned, input).is_ok());
    }
}
