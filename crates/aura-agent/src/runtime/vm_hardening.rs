//! Telltale VM hardening profiles and parity-lane configuration for Aura.

use aura_core::effects::VmBridgeSchedulerSignals;
use aura_core::AuraVmDeterminismProfileV1;
use aura_mpst::termination::{compute_weighted_measure, SessionBufferSnapshot};
use aura_protocol::termination::TerminationProtocolClass;
use telltale_vm::envelope_diff::EnvelopeDiffArtifactV1;
use telltale_vm::loader::CodeImage;
use telltale_vm::vm::{FlowPolicy, FlowPredicate, GuardLayerConfig};
use telltale_vm::{
    CanonicalReplayFragmentV1, CommunicationReplayMode, DeterminismMode, EffectDeterminismTier,
    EffectOrderingClass, EffectTraceCaptureMode, FailureVisibleDiffClass, MonitorMode,
    OutputConditionPolicy, PayloadValidationMode, PriorityPolicy, SchedPolicy,
    SchedulerPermutationClass, ThreadedRoundSemantics, VMConfig,
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
    /// Envelope artifact required by runtime policy is missing.
    #[error("missing envelope artifact for policy {policy_ref}")]
    MissingEnvelopeArtifact {
        /// Policy requiring the artifact.
        policy_ref: String,
    },
    /// Envelope diff exceeded the declared wave-width bound.
    #[error(
        "envelope artifact wave width exceeded for policy {policy_ref}: baseline={baseline}, candidate={candidate}, declared={declared}"
    )]
    EnvelopeWaveWidthExceeded {
        /// Policy requiring the artifact.
        policy_ref: String,
        /// Observed baseline max wave width.
        baseline: usize,
        /// Observed candidate max wave width.
        candidate: usize,
        /// Declared admissible upper bound.
        declared: usize,
    },
    /// Envelope diff scheduler class exceeded the policy envelope.
    #[error(
        "scheduler envelope class {actual:?} exceeds policy {policy_ref} allowance {expected:?}"
    )]
    EnvelopeSchedulerClassRejected {
        /// Policy reference under validation.
        policy_ref: String,
        /// Minimum required/allowed class derived from policy.
        expected: AuraVmSchedulerEnvelopeClass,
        /// Actual class from the artifact.
        actual: SchedulerPermutationClass,
    },
    /// Envelope diff effect ordering class exceeded the policy envelope.
    #[error(
        "effect ordering class {actual:?} exceeds policy {policy_ref} allowance for tier {expected:?}"
    )]
    EnvelopeEffectOrderingRejected {
        /// Policy reference under validation.
        policy_ref: String,
        /// Expected determinism tier.
        expected: EffectDeterminismTier,
        /// Actual effect ordering class from the artifact.
        actual: EffectOrderingClass,
    },
    /// Envelope diff failure-visible class exceeded the policy envelope.
    #[error(
        "failure-visible class {actual:?} exceeds policy {policy_ref} allowance for runtime mode {expected:?}"
    )]
    EnvelopeFailureVisibleRejected {
        /// Policy reference under validation.
        policy_ref: String,
        /// Expected runtime mode.
        expected: AuraVmRuntimeMode,
        /// Actual failure-visible class from the artifact.
        actual: FailureVisibleDiffClass,
    },
    /// Envelope diff effect determinism tier drifted from the policy tier.
    #[error(
        "envelope effect determinism tier {actual:?} does not match policy {policy_ref} tier {expected:?}"
    )]
    EnvelopeEffectTierMismatch {
        /// Policy reference under validation.
        policy_ref: String,
        /// Expected policy tier.
        expected: EffectDeterminismTier,
        /// Actual artifact tier.
        actual: EffectDeterminismTier,
    },
}

/// Canonical VM execution policy bound to one Aura protocol class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuraVmProtocolExecutionPolicy {
    /// Stable policy selector reference.
    pub policy_ref: &'static str,
    /// Aura protocol class.
    pub protocol_class: TerminationProtocolClass,
    /// Aura-selected runtime mode for this protocol.
    pub runtime_mode: AuraVmRuntimeMode,
    /// Declared scheduler envelope class for this protocol.
    pub scheduler_envelope_class: AuraVmSchedulerEnvelopeClass,
    /// Declared upper bound for threaded wave width when runtime mode is threaded.
    pub declared_wave_width_bound: Option<usize>,
    /// Telltale determinism mode.
    pub determinism_mode: DeterminismMode,
    /// Telltale effect determinism tier.
    pub effect_determinism_tier: EffectDeterminismTier,
    /// Telltale communication replay mode.
    pub communication_replay_mode: CommunicationReplayMode,
}

impl AuraVmProtocolExecutionPolicy {
    /// Concurrency profile admitted by this policy.
    #[must_use]
    pub const fn concurrency_profile(self) -> AuraVmConcurrencyProfile {
        match self.runtime_mode {
            AuraVmRuntimeMode::Cooperative => AuraVmConcurrencyProfile::Canonical,
            AuraVmRuntimeMode::ThreadedReplayDeterministic
            | AuraVmRuntimeMode::ThreadedEnvelopeBounded => {
                AuraVmConcurrencyProfile::EnvelopeAdmitted
            }
        }
    }

    /// Whether this policy is canonical-only in the runtime.
    #[must_use]
    pub const fn is_canonical_only(self) -> bool {
        matches!(
            self.concurrency_profile(),
            AuraVmConcurrencyProfile::Canonical
        )
    }

    /// Whether this policy admits bounded concurrency beyond canonical execution.
    #[must_use]
    pub const fn is_envelope_admitted(self) -> bool {
        matches!(
            self.concurrency_profile(),
            AuraVmConcurrencyProfile::EnvelopeAdmitted
        )
    }

    /// Conservative cooperative fallback preserving the protocol's other determinism dimensions.
    #[must_use]
    pub const fn canonical_fallback_policy(self) -> Self {
        Self {
            runtime_mode: AuraVmRuntimeMode::Cooperative,
            scheduler_envelope_class: AuraVmSchedulerEnvelopeClass::Exact,
            declared_wave_width_bound: Some(1),
            ..self
        }
    }
}

/// Concrete runtime-selection input for one admitted VM fragment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuraVmRuntimeSelector {
    /// Selected runtime mode for this fragment.
    pub runtime_mode: AuraVmRuntimeMode,
    /// Worker count used when the threaded runtime is selected.
    pub threaded_workers: usize,
    /// Scheduler concurrency width used for one runtime round.
    pub scheduler_concurrency: usize,
}

impl AuraVmRuntimeSelector {
    /// Canonical selector for one protocol execution policy.
    #[must_use]
    pub fn for_policy(policy: AuraVmProtocolExecutionPolicy) -> Self {
        let width = policy.declared_wave_width_bound.unwrap_or(1).max(1);
        match policy.runtime_mode {
            AuraVmRuntimeMode::Cooperative => Self::cooperative(),
            AuraVmRuntimeMode::ThreadedReplayDeterministic
            | AuraVmRuntimeMode::ThreadedEnvelopeBounded => Self {
                runtime_mode: policy.runtime_mode,
                threaded_workers: width,
                scheduler_concurrency: width,
            },
        }
    }

    /// Canonical cooperative selector used when runtime work stays on the reference path.
    #[must_use]
    pub const fn cooperative() -> Self {
        Self {
            runtime_mode: AuraVmRuntimeMode::Cooperative,
            threaded_workers: 1,
            scheduler_concurrency: 1,
        }
    }

    /// Whether the selector requires a threaded runtime.
    #[must_use]
    pub const fn is_threaded(self) -> bool {
        self.runtime_mode.is_threaded()
    }
}

/// Derived runtime capabilities required by the selected runtime mode.
#[must_use]
pub fn required_runtime_capabilities_for_policy(
    policy: AuraVmProtocolExecutionPolicy,
) -> &'static [&'static str] {
    match policy.runtime_mode {
        AuraVmRuntimeMode::Cooperative => &[],
        AuraVmRuntimeMode::ThreadedReplayDeterministic => &["mixed_determinism"],
        AuraVmRuntimeMode::ThreadedEnvelopeBounded => &["mixed_determinism", "vmEnvelopeAdherence"],
    }
}

/// Whether the selected policy requires an envelope-diff artifact.
#[must_use]
pub const fn policy_requires_envelope_artifact(policy: AuraVmProtocolExecutionPolicy) -> bool {
    matches!(
        policy.runtime_mode,
        AuraVmRuntimeMode::ThreadedEnvelopeBounded
    )
}

/// Build an envelope diff artifact from cooperative and threaded replay fragments.
#[must_use]
pub fn build_envelope_diff_artifact_for_policy(
    policy: AuraVmProtocolExecutionPolicy,
    baseline_engine: impl Into<String>,
    candidate_engine: impl Into<String>,
    baseline: &CanonicalReplayFragmentV1,
    candidate: &CanonicalReplayFragmentV1,
    baseline_max_wave_width: usize,
    candidate_max_wave_width: usize,
) -> EnvelopeDiffArtifactV1 {
    EnvelopeDiffArtifactV1::from_replay_fragments(
        baseline_engine,
        candidate_engine,
        baseline,
        candidate,
        baseline_max_wave_width,
        candidate_max_wave_width,
        policy.declared_wave_width_bound.unwrap_or(1).max(1),
        policy.effect_determinism_tier,
    )
}

/// Validate that an envelope artifact stays within the policy-defined runtime envelope.
///
/// # Errors
///
/// Returns [`AuraVmDeterminismProfileError`] when the artifact is missing or exceeds policy.
pub fn validate_envelope_artifact_for_policy(
    policy: AuraVmProtocolExecutionPolicy,
    artifact: Option<&EnvelopeDiffArtifactV1>,
) -> Result<(), AuraVmDeterminismProfileError> {
    let Some(artifact) = artifact else {
        if policy_requires_envelope_artifact(policy) {
            return Err(AuraVmDeterminismProfileError::MissingEnvelopeArtifact {
                policy_ref: policy.policy_ref.to_string(),
            });
        }
        return Ok(());
    };

    let diff = &artifact.envelope_diff;
    let wave = &diff.wave_width_bound;
    if !wave.within_declared_bound() {
        return Err(AuraVmDeterminismProfileError::EnvelopeWaveWidthExceeded {
            policy_ref: policy.policy_ref.to_string(),
            baseline: wave.baseline_max_wave_width,
            candidate: wave.candidate_max_wave_width,
            declared: wave.declared_upper_bound,
        });
    }

    if !scheduler_class_within_policy(
        diff.scheduler_permutation_class,
        policy.scheduler_envelope_class,
    ) {
        return Err(
            AuraVmDeterminismProfileError::EnvelopeSchedulerClassRejected {
                policy_ref: policy.policy_ref.to_string(),
                expected: policy.scheduler_envelope_class,
                actual: diff.scheduler_permutation_class,
            },
        );
    }

    if !effect_ordering_within_policy(diff.effect_ordering_class, policy.effect_determinism_tier) {
        return Err(
            AuraVmDeterminismProfileError::EnvelopeEffectOrderingRejected {
                policy_ref: policy.policy_ref.to_string(),
                expected: policy.effect_determinism_tier,
                actual: diff.effect_ordering_class,
            },
        );
    }

    if !failure_visible_within_policy(diff.failure_visible_diff_class, policy.runtime_mode) {
        return Err(
            AuraVmDeterminismProfileError::EnvelopeFailureVisibleRejected {
                policy_ref: policy.policy_ref.to_string(),
                expected: policy.runtime_mode,
                actual: diff.failure_visible_diff_class,
            },
        );
    }

    if diff.effect_determinism_tier != policy.effect_determinism_tier {
        return Err(AuraVmDeterminismProfileError::EnvelopeEffectTierMismatch {
            policy_ref: policy.policy_ref.to_string(),
            expected: policy.effect_determinism_tier,
            actual: diff.effect_determinism_tier,
        });
    }

    Ok(())
}

fn scheduler_class_within_policy(
    actual: SchedulerPermutationClass,
    expected: AuraVmSchedulerEnvelopeClass,
) -> bool {
    match expected {
        AuraVmSchedulerEnvelopeClass::Exact => {
            matches!(actual, SchedulerPermutationClass::Exact)
        }
        AuraVmSchedulerEnvelopeClass::SessionNormalizedPermutation => matches!(
            actual,
            SchedulerPermutationClass::Exact
                | SchedulerPermutationClass::SessionNormalizedPermutation
        ),
        AuraVmSchedulerEnvelopeClass::EnvelopeBounded => true,
    }
}

fn effect_ordering_within_policy(actual: EffectOrderingClass, tier: EffectDeterminismTier) -> bool {
    match tier {
        EffectDeterminismTier::StrictDeterministic => {
            matches!(actual, EffectOrderingClass::Exact)
        }
        EffectDeterminismTier::ReplayDeterministic => matches!(
            actual,
            EffectOrderingClass::Exact | EffectOrderingClass::ReplayDeterministic
        ),
        EffectDeterminismTier::EnvelopeBoundedNondeterministic => true,
    }
}

fn failure_visible_within_policy(
    actual: FailureVisibleDiffClass,
    runtime_mode: AuraVmRuntimeMode,
) -> bool {
    match runtime_mode {
        AuraVmRuntimeMode::Cooperative | AuraVmRuntimeMode::ThreadedReplayDeterministic => {
            matches!(actual, FailureVisibleDiffClass::Exact)
        }
        AuraVmRuntimeMode::ThreadedEnvelopeBounded => true,
    }
}

/// Host-selected runtime execution mode for one admitted protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuraVmRuntimeMode {
    /// Canonical cooperative VM execution with exact scheduler semantics.
    Cooperative,
    /// Threaded execution constrained to replay-deterministic/session-normalized behavior.
    ThreadedReplayDeterministic,
    /// Threaded execution admitted under an explicit operational envelope.
    ThreadedEnvelopeBounded,
}

impl AuraVmRuntimeMode {
    /// Stable runtime-mode identifier.
    #[must_use]
    pub const fn as_ref(self) -> &'static str {
        match self {
            Self::Cooperative => "cooperative",
            Self::ThreadedReplayDeterministic => "threaded_replay_deterministic",
            Self::ThreadedEnvelopeBounded => "threaded_envelope_bounded",
        }
    }

    /// Whether this mode requires threaded VM execution semantics.
    #[must_use]
    pub const fn is_threaded(self) -> bool {
        !matches!(self, Self::Cooperative)
    }

    /// Canonical Telltale round semantics for this mode.
    #[must_use]
    pub const fn threaded_round_semantics(self) -> ThreadedRoundSemantics {
        match self {
            Self::Cooperative => ThreadedRoundSemantics::CanonicalOneStep,
            Self::ThreadedReplayDeterministic | Self::ThreadedEnvelopeBounded => {
                ThreadedRoundSemantics::WaveParallelExtension
            }
        }
    }
}

/// Runtime concurrency profile admitted by Aura for one protocol path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuraVmConcurrencyProfile {
    /// Exact single-owner reference execution.
    Canonical,
    /// Bounded runtime concurrency admitted by policy.
    EnvelopeAdmitted,
}

impl AuraVmConcurrencyProfile {
    /// Stable concurrency-profile identifier.
    #[must_use]
    pub const fn as_ref(self) -> &'static str {
        match self {
            Self::Canonical => "canonical",
            Self::EnvelopeAdmitted => "envelope_admitted",
        }
    }
}

/// Scheduler envelope class declared by Aura for one protocol runtime mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuraVmSchedulerEnvelopeClass {
    /// No scheduler permutation beyond canonical order.
    Exact,
    /// Session-normalized permutation only.
    SessionNormalizedPermutation,
    /// Envelope-bounded scheduler divergence is admitted.
    EnvelopeBounded,
}

impl AuraVmSchedulerEnvelopeClass {
    /// Stable scheduler-envelope identifier.
    #[must_use]
    pub const fn as_ref(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::SessionNormalizedPermutation => "session_normalized_permutation",
            Self::EnvelopeBounded => "envelope_bounded",
        }
    }
}

/// Runtime signals that influence scheduler selection without bypassing the VM scheduler.
pub type AuraVmSchedulerSignals = VmBridgeSchedulerSignals;

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
            runtime_mode: self.runtime_mode.as_ref().to_string(),
            scheduler_envelope_class: self.scheduler_envelope_class.as_ref().to_string(),
            declared_wave_width_bound: self.declared_wave_width_bound,
            determinism_mode: determinism_mode_ref(self.determinism_mode).to_string(),
            effect_determinism_tier: effect_determinism_tier_ref(self.effect_determinism_tier)
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
            runtime_mode: AuraVmRuntimeMode::Cooperative,
            scheduler_envelope_class: AuraVmSchedulerEnvelopeClass::Exact,
            declared_wave_width_bound: Some(1),
            determinism_mode: DeterminismMode::Full,
            effect_determinism_tier: EffectDeterminismTier::StrictDeterministic,
            communication_replay_mode: CommunicationReplayMode::Off,
        }),
        AURA_VM_POLICY_CONSENSUS_FALLBACK => Ok(AuraVmProtocolExecutionPolicy {
            policy_ref: AURA_VM_POLICY_CONSENSUS_FALLBACK,
            protocol_class: TerminationProtocolClass::ConsensusFallback,
            runtime_mode: AuraVmRuntimeMode::ThreadedReplayDeterministic,
            scheduler_envelope_class: AuraVmSchedulerEnvelopeClass::SessionNormalizedPermutation,
            declared_wave_width_bound: Some(2),
            determinism_mode: DeterminismMode::ModuloCommutativity,
            effect_determinism_tier: EffectDeterminismTier::ReplayDeterministic,
            communication_replay_mode: CommunicationReplayMode::Sequence,
        }),
        AURA_VM_POLICY_CONSENSUS_FAST_PATH => Ok(AuraVmProtocolExecutionPolicy {
            policy_ref: AURA_VM_POLICY_CONSENSUS_FAST_PATH,
            protocol_class: TerminationProtocolClass::ConsensusFastPath,
            runtime_mode: AuraVmRuntimeMode::Cooperative,
            scheduler_envelope_class: AuraVmSchedulerEnvelopeClass::Exact,
            declared_wave_width_bound: Some(1),
            determinism_mode: DeterminismMode::Full,
            effect_determinism_tier: EffectDeterminismTier::StrictDeterministic,
            communication_replay_mode: CommunicationReplayMode::Sequence,
        }),
        AURA_VM_POLICY_DKG_CEREMONY => Ok(AuraVmProtocolExecutionPolicy {
            policy_ref: AURA_VM_POLICY_DKG_CEREMONY,
            protocol_class: TerminationProtocolClass::DkgCeremony,
            runtime_mode: AuraVmRuntimeMode::ThreadedReplayDeterministic,
            scheduler_envelope_class: AuraVmSchedulerEnvelopeClass::SessionNormalizedPermutation,
            declared_wave_width_bound: Some(2),
            determinism_mode: DeterminismMode::Replay,
            effect_determinism_tier: EffectDeterminismTier::ReplayDeterministic,
            communication_replay_mode: CommunicationReplayMode::Sequence,
        }),
        AURA_VM_POLICY_RECOVERY_GRANT => Ok(AuraVmProtocolExecutionPolicy {
            policy_ref: AURA_VM_POLICY_RECOVERY_GRANT,
            protocol_class: TerminationProtocolClass::RecoveryGrant,
            runtime_mode: AuraVmRuntimeMode::Cooperative,
            scheduler_envelope_class: AuraVmSchedulerEnvelopeClass::Exact,
            declared_wave_width_bound: Some(1),
            determinism_mode: DeterminismMode::Full,
            effect_determinism_tier: EffectDeterminismTier::StrictDeterministic,
            communication_replay_mode: CommunicationReplayMode::Off,
        }),
        AURA_VM_POLICY_SYNC_ANTI_ENTROPY => Ok(AuraVmProtocolExecutionPolicy {
            policy_ref: AURA_VM_POLICY_SYNC_ANTI_ENTROPY,
            protocol_class: TerminationProtocolClass::SyncAntiEntropy,
            runtime_mode: AuraVmRuntimeMode::ThreadedEnvelopeBounded,
            scheduler_envelope_class: AuraVmSchedulerEnvelopeClass::EnvelopeBounded,
            declared_wave_width_bound: Some(4),
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
    let protocol_class =
        TerminationProtocolClass::from_protocol_id(protocol_id).ok_or_else(|| {
            AuraVmDeterminismProfileError::PolicyClassMismatch {
                policy_ref: policy_ref
                    .unwrap_or(AURA_VM_POLICY_PROD_DEFAULT)
                    .to_string(),
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
    config.threaded_round_semantics = policy.runtime_mode.threaded_round_semantics();
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
    if config.threaded_round_semantics != policy.runtime_mode.threaded_round_semantics() {
        return Err(AuraVmDeterminismProfileError::InvalidCombination {
            mode: config.determinism_mode,
            tier: config.effect_determinism_tier,
            replay: config.communication_replay_mode,
            reason: "vm threaded round semantics do not match runtime mode policy",
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
    use telltale_vm::threaded::ThreadedVM;
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

    fn capture_envelope_artifact(
        policy: AuraVmProtocolExecutionPolicy,
        candidate_max_wave_width: usize,
    ) -> EnvelopeDiffArtifactV1 {
        let image = flow_compatibility_image();
        let mut config = build_vm_config(
            AuraVmHardeningProfile::Prod,
            AuraVmParityProfile::RuntimeDefault,
        );
        apply_protocol_execution_policy(&mut config, policy);
        let handler = UnitHandler;

        let mut baseline = VM::new(config.clone());
        baseline.load_choreography(&image).expect("baseline load");
        baseline
            .run_concurrent(&handler, 128, 1)
            .expect("baseline run");

        let mut candidate = ThreadedVM::with_workers(config, candidate_max_wave_width.max(1));
        candidate.load_choreography(&image).expect("candidate load");
        candidate
            .run_concurrent(&handler, 128, candidate_max_wave_width.max(1))
            .expect("candidate run");

        build_envelope_diff_artifact_for_policy(
            policy,
            "native_cooperative",
            "native_threaded",
            &baseline.canonical_replay_fragment(),
            &candidate.canonical_replay_fragment(),
            1,
            candidate_max_wave_width.max(1),
        )
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
        assert_eq!(
            dkg.runtime_mode,
            AuraVmRuntimeMode::ThreadedReplayDeterministic
        );
        assert_eq!(
            dkg.concurrency_profile(),
            AuraVmConcurrencyProfile::EnvelopeAdmitted
        );
        assert!(dkg.is_envelope_admitted());
        assert!(!dkg.is_canonical_only());
        assert_eq!(
            dkg.scheduler_envelope_class,
            AuraVmSchedulerEnvelopeClass::SessionNormalizedPermutation
        );
        assert_eq!(dkg.declared_wave_width_bound, Some(2));
        assert_eq!(dkg.determinism_mode, DeterminismMode::Replay);
        assert_eq!(
            dkg.effect_determinism_tier,
            EffectDeterminismTier::ReplayDeterministic
        );
        assert_eq!(
            dkg.communication_replay_mode,
            CommunicationReplayMode::Sequence
        );
        assert_eq!(
            AuraVmRuntimeSelector::for_policy(dkg),
            AuraVmRuntimeSelector {
                runtime_mode: AuraVmRuntimeMode::ThreadedReplayDeterministic,
                threaded_workers: 2,
                scheduler_concurrency: 2,
            }
        );

        let invitation = policy_for_protocol(
            "aura.invitation.exchange",
            Some(AURA_VM_POLICY_PROD_DEFAULT),
        )
        .expect("invitation policy must resolve");
        assert_eq!(
            invitation.protocol_class,
            TerminationProtocolClass::RecoveryGrant
        );
        assert_eq!(invitation.runtime_mode, AuraVmRuntimeMode::Cooperative);
        assert_eq!(
            invitation.concurrency_profile(),
            AuraVmConcurrencyProfile::Canonical
        );
        assert!(invitation.is_canonical_only());
        assert!(!invitation.is_envelope_admitted());
        assert_eq!(invitation.determinism_mode, DeterminismMode::Full);
        assert_eq!(
            AuraVmRuntimeSelector::for_policy(invitation),
            AuraVmRuntimeSelector::cooperative()
        );
        assert_eq!(
            required_runtime_capabilities_for_policy(invitation),
            &[] as &[&str]
        );
        assert!(!policy_requires_envelope_artifact(invitation));

        let sync = policy_for_protocol("aura.sync.epoch_rotation", None)
            .expect("sync policy must resolve");
        assert_eq!(
            sync.runtime_mode,
            AuraVmRuntimeMode::ThreadedEnvelopeBounded
        );
        assert_eq!(
            sync.concurrency_profile(),
            AuraVmConcurrencyProfile::EnvelopeAdmitted
        );
        assert!(sync.is_envelope_admitted());
        assert_eq!(
            sync.scheduler_envelope_class,
            AuraVmSchedulerEnvelopeClass::EnvelopeBounded
        );
        assert_eq!(sync.declared_wave_width_bound, Some(4));
        assert_eq!(
            AuraVmRuntimeSelector::for_policy(sync),
            AuraVmRuntimeSelector {
                runtime_mode: AuraVmRuntimeMode::ThreadedEnvelopeBounded,
                threaded_workers: 4,
                scheduler_concurrency: 4,
            }
        );
        assert_eq!(
            required_runtime_capabilities_for_policy(sync),
            &["mixed_determinism", "vmEnvelopeAdherence"]
        );
        assert!(policy_requires_envelope_artifact(sync));
        assert_eq!(
            sync.canonical_fallback_policy(),
            AuraVmProtocolExecutionPolicy {
                runtime_mode: AuraVmRuntimeMode::Cooperative,
                scheduler_envelope_class: AuraVmSchedulerEnvelopeClass::Exact,
                declared_wave_width_bound: Some(1),
                ..sync
            }
        );
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
    fn envelope_validation_rejects_missing_required_artifact() {
        let policy = policy_for_protocol("aura.sync.epoch_rotation", None)
            .expect("sync policy must resolve");
        let err = validate_envelope_artifact_for_policy(policy, None)
            .expect_err("envelope-bounded policy must fail without artifact");
        assert!(matches!(
            err,
            AuraVmDeterminismProfileError::MissingEnvelopeArtifact { .. }
        ));
    }

    #[test]
    fn envelope_validation_accepts_artifact_within_policy() {
        let policy = policy_for_protocol("aura.sync.epoch_rotation", None)
            .expect("sync policy must resolve");
        let artifact = capture_envelope_artifact(policy, 2);
        validate_envelope_artifact_for_policy(policy, Some(&artifact))
            .expect("artifact should satisfy sync envelope policy");
    }

    #[test]
    fn envelope_validation_rejects_wave_width_overflow() {
        let policy = policy_for_protocol("aura.sync.epoch_rotation", None)
            .expect("sync policy must resolve");
        let artifact = capture_envelope_artifact(policy, 5);
        let err = validate_envelope_artifact_for_policy(policy, Some(&artifact))
            .expect_err("artifact should fail when observed width exceeds declared bound");
        assert!(matches!(
            err,
            AuraVmDeterminismProfileError::EnvelopeWaveWidthExceeded { .. }
        ));
    }

    #[test]
    fn envelope_validation_rejects_scheduler_class_drift() {
        let policy = policy_for_protocol("aura.dkg.ceremony", Some(AURA_VM_POLICY_DKG_CEREMONY))
            .expect("dkg policy must resolve");
        let mut artifact = capture_envelope_artifact(policy, 2);
        artifact.envelope_diff.scheduler_permutation_class =
            SchedulerPermutationClass::EnvelopeBounded;

        let err = validate_envelope_artifact_for_policy(policy, Some(&artifact))
            .expect_err("scheduler envelope drift must be rejected");
        assert!(matches!(
            err,
            AuraVmDeterminismProfileError::EnvelopeSchedulerClassRejected { .. }
        ));
    }

    #[test]
    fn envelope_validation_rejects_failure_visible_drift() {
        let policy = policy_for_protocol("aura.dkg.ceremony", Some(AURA_VM_POLICY_DKG_CEREMONY))
            .expect("dkg policy must resolve");
        let mut artifact = capture_envelope_artifact(policy, 2);
        artifact.envelope_diff.failure_visible_diff_class =
            FailureVisibleDiffClass::EnvelopeBounded;

        let err = validate_envelope_artifact_for_policy(policy, Some(&artifact))
            .expect_err("failure-visible drift must be rejected");
        assert!(matches!(
            err,
            AuraVmDeterminismProfileError::EnvelopeFailureVisibleRejected { .. }
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
