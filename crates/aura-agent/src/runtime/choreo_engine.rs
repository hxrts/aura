//! Telltale protocol-machine choreography engine for Aura runtime integration.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use aura_core::conformance::{
    assert_effect_kinds_classified, AuraConformanceArtifactV1, AuraConformanceRunMetadataV1,
    AuraConformanceSurfaceV1, AuraVmDeterminismProfileV1, ConformanceSurfaceName,
    ConformanceValidationError,
};
use aura_core::effects::guard::{EffectInterpreter, EffectResult};
use aura_core::effects::{AdmissionError, CapabilityKey, RuntimeCapabilityEffects};
use aura_core::hash::hash;
use aura_effects::RuntimeCapabilityHandler;
use aura_mpst::termination::{compute_weighted_measure, SessionBufferSnapshot};
use aura_protocol::termination::{
    TerminationBudget, TerminationBudgetConfig, TerminationBudgetError, TerminationProtocolClass,
};
use telltale_machine::coroutine::Fault;
use telltale_machine::model::effects::EffectHandler as ProtocolMachineEffectHandler;
use telltale_machine::model::state::SessionStatus;
use telltale_machine::runtime::loader::CodeImage as ProtocolMachineCodeImage;
use telltale_machine::{
    canonical_effect_trace, enforce_protocol_machine_runtime_gates, normalize_trace, obs_session,
    EffectTraceEntry, EnvelopeDiffArtifactV1, ObsEvent, ProtocolMachine,
    ProtocolMachineConfig as VMConfig, ProtocolMachineError, RunStatus, RuntimeContracts,
    RuntimeGateResult, SessionId, StepResult, ThreadedProtocolMachine,
};
use tracing::warn;

use super::vm_effect_handler::AuraVmEffectHandler;
use super::vm_hardening::{
    build_vm_config, configured_guard_capacity, policy_requires_envelope_artifact,
    required_runtime_capabilities_for_policy, scheduler_control_input_for_protocol_machine_image,
    validate_determinism_profile, validate_envelope_artifact_for_policy,
    validate_protocol_execution_policy, validate_scheduler_execution_policy, vm_config_for_profile,
    AuraVmHardeningProfile, AuraVmParityProfile, AuraVmProtocolExecutionPolicy, AuraVmRuntimeMode,
    AuraVmRuntimeSelector, AuraVmSchedulerSignalsProvider,
};

/// Errors raised by [`AuraChoreoEngine`].
#[derive(Debug, thiserror::Error)]
pub enum AuraChoreoEngineError {
    /// VM execution/lifecycle error.
    #[error("vm error: {source}")]
    Vm {
        /// Wrapped VM error.
        source: ProtocolMachineError,
    },
    /// Session store lifecycle error.
    #[error("session lifecycle error: {message}")]
    SessionLifecycle {
        /// Session lifecycle failure reason.
        message: String,
    },
    /// Effect interpreter execution failure.
    #[error("effect interpreter error: {message}")]
    Interpreter {
        /// Interpreter failure reason.
        message: String,
    },
    /// VM runtime contracts are required but missing.
    #[error("missing runtime contracts for VM admission")]
    MissingRuntimeContracts,
    /// VM runtime profile not supported by provided contracts.
    #[error("unsupported VM determinism profile for provided runtime contracts")]
    UnsupportedDeterminismProfile,
    /// Required runtime capability is missing for bundle admission.
    #[error("missing runtime capability: {capability}")]
    MissingRuntimeCapability {
        /// Missing capability identifier.
        capability: String,
    },
    /// Effect trace contains unknown/unclassified effect kinds.
    #[error("unclassified effect envelope kinds in trace: {kinds:?}")]
    UnclassifiedEnvelopeKinds {
        /// Unknown kinds encountered while validating effect trace.
        kinds: Vec<String>,
    },
    /// Conformance artifact serialization/validation failure.
    #[error("conformance artifact error: {message}")]
    ConformanceArtifact {
        /// Conformance artifact failure reason.
        message: String,
    },
    /// Output-condition gate rejected an observable commit.
    #[error(
        "output-condition rejected (predicate={predicate_ref}, tick={tick:?}, witness={witness_ref:?}, digest={output_digest:?})"
    )]
    OutputConditionRejected {
        /// Predicate reference that failed.
        predicate_ref: String,
        /// Scheduler tick of the last failing check, if available.
        tick: Option<u64>,
        /// Optional witness reference captured in trace.
        witness_ref: Option<String>,
        /// Optional output digest captured in trace.
        output_digest: Option<String>,
    },
    /// Step budget derived from weighted termination bound was exceeded.
    #[error(
        "termination bound exceeded for {protocol}: steps_consumed={steps_consumed}, max_steps={max_steps}, initial_weight={initial_weight}"
    )]
    BoundExceeded {
        /// Protocol class identifier used for budget derivation.
        protocol: String,
        /// Initial weighted measure used for this run.
        initial_weight: u64,
        /// Maximum admissible steps.
        max_steps: u64,
        /// Steps consumed when violation was detected.
        steps_consumed: u64,
    },
}

impl From<ProtocolMachineError> for AuraChoreoEngineError {
    fn from(source: ProtocolMachineError) -> Self {
        Self::Vm { source }
    }
}

type VM = ProtocolMachine;
type VMError = ProtocolMachineError;

fn protocol_machine_config_from_vm_config(
    config: &VMConfig,
) -> Result<VMConfig, AuraChoreoEngineError> {
    config
        .clone()
        .validate_invariants()
        .map_err(|reason| AuraChoreoEngineError::Interpreter {
            message: format!("invalid protocol-machine config: {reason}"),
        })?;
    Ok(config.clone())
}

fn admit_protocol_machine_runtime(
    config: &VMConfig,
    runtime_contracts: Option<&RuntimeContracts>,
) -> Result<VMConfig, AuraChoreoEngineError> {
    validate_determinism_profile(config).map_err(|error| AuraChoreoEngineError::Interpreter {
        message: format!("invalid VM determinism profile: {error}"),
    })?;

    let protocol_machine_config = protocol_machine_config_from_vm_config(config)?;
    match enforce_protocol_machine_runtime_gates(&protocol_machine_config, runtime_contracts) {
        RuntimeGateResult::Admitted => Ok(protocol_machine_config),
        RuntimeGateResult::RejectedMissingContracts => {
            Err(AuraChoreoEngineError::MissingRuntimeContracts)
        }
        RuntimeGateResult::RejectedUnsupportedDeterminismProfile => {
            Err(AuraChoreoEngineError::UnsupportedDeterminismProfile)
        }
    }
}

enum AuraVmBackend {
    Cooperative(Box<ProtocolMachine>),
    Threaded(Box<ThreadedProtocolMachine>),
}

impl std::fmt::Debug for AuraVmBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cooperative(_) => f.write_str("AuraVmBackend::Cooperative"),
            Self::Threaded(_) => f.write_str("AuraVmBackend::Threaded"),
        }
    }
}

impl AuraVmBackend {
    fn new(config: &VMConfig, selector: AuraVmRuntimeSelector) -> Self {
        match selector.runtime_mode {
            AuraVmRuntimeMode::Cooperative => {
                Self::Cooperative(Box::new(ProtocolMachine::new(config.clone())))
            }
            AuraVmRuntimeMode::ThreadedReplayDeterministic
            | AuraVmRuntimeMode::ThreadedEnvelopeBounded => {
                Self::Threaded(Box::new(ThreadedProtocolMachine::with_workers(
                    config.clone(),
                    selector.threaded_workers.max(1),
                )))
            }
        }
    }

    fn load_choreography(
        &mut self,
        image: &ProtocolMachineCodeImage,
    ) -> Result<SessionId, ProtocolMachineError> {
        match self {
            Self::Cooperative(vm) => vm.load_choreography(image),
            Self::Threaded(vm) => vm.load_choreography(image),
        }
    }

    fn step_round(
        &mut self,
        handler: &dyn ProtocolMachineEffectHandler,
        concurrency: usize,
    ) -> Result<StepResult, ProtocolMachineError> {
        match self {
            Self::Cooperative(vm) => vm.step_round(handler, 1),
            Self::Threaded(vm) => vm.step_round(handler, concurrency.max(1)),
        }
    }

    fn run_replay_shared(
        &mut self,
        fallback: &dyn ProtocolMachineEffectHandler,
        replay_trace: Arc<[EffectTraceEntry]>,
        max_steps: usize,
        concurrency: usize,
    ) -> Result<RunStatus, ProtocolMachineError> {
        match self {
            Self::Cooperative(vm) => {
                vm.run_concurrent_replay_shared(fallback, replay_trace, max_steps, 1)
            }
            Self::Threaded(vm) => vm.run_concurrent_replay_shared(
                fallback,
                replay_trace,
                max_steps,
                concurrency.max(1),
            ),
        }
    }

    fn observable_trace(&self) -> Vec<ObsEvent> {
        match self {
            Self::Cooperative(vm) => vm.trace().to_vec(),
            Self::Threaded(vm) => vm.trace().to_vec(),
        }
    }

    fn effect_trace(&self) -> Vec<EffectTraceEntry> {
        match self {
            Self::Cooperative(vm) => vm.effect_trace().to_vec(),
            Self::Threaded(vm) => vm.effect_trace().to_vec(),
        }
    }

    fn reap_closed_sessions(&mut self) {
        if let Self::Cooperative(vm) = self {
            let _ = vm.reap_closed_sessions();
        }
    }

    fn as_cooperative(&self) -> Option<&ProtocolMachine> {
        match self {
            Self::Cooperative(vm) => Some(vm),
            Self::Threaded(_) => None,
        }
    }

    fn as_cooperative_mut(&mut self) -> Option<&mut ProtocolMachine> {
        match self {
            Self::Cooperative(vm) => Some(vm),
            Self::Threaded(_) => None,
        }
    }
}

/// VM-backed choreography engine with explicit session lifecycle hooks.
#[derive(Debug)]
pub struct AuraChoreoEngine<H: ProtocolMachineEffectHandler = AuraVmEffectHandler> {
    backend: AuraVmBackend,
    vm_config: VMConfig,
    runtime_selector: AuraVmRuntimeSelector,
    handler: Arc<H>,
    runtime_contracts: Option<RuntimeContracts>,
    runtime_capabilities: RuntimeCapabilityHandler,
    capability_snapshot: Vec<(String, bool)>,
    active_sessions: BTreeSet<SessionId>,
    session_protocol_classes: BTreeMap<SessionId, TerminationProtocolClass>,
    session_determinism_profiles: BTreeMap<SessionId, AuraVmProtocolExecutionPolicy>,
    session_runtime_selectors: BTreeMap<SessionId, AuraVmRuntimeSelector>,
    termination_budget_config: TerminationBudgetConfig,
}

impl Default for AuraChoreoEngine<AuraVmEffectHandler> {
    fn default() -> Self {
        Self::new(
            vm_config_for_profile(AuraVmHardeningProfile::Prod),
            Arc::new(AuraVmEffectHandler::default()),
        )
    }
}

impl<H: ProtocolMachineEffectHandler> AuraChoreoEngine<H> {
    /// Create an engine from explicit Aura hardening/parity profiles.
    pub fn new_with_profiles(
        hardening: AuraVmHardeningProfile,
        parity: AuraVmParityProfile,
        handler: Arc<H>,
    ) -> Self {
        Self::new(build_vm_config(hardening, parity), handler)
    }

    /// Create an engine with explicit VM configuration and host effect handler.
    pub fn new(config: VMConfig, handler: Arc<H>) -> Self {
        Self::new_with_protocol_machine_contracts(config, handler, None)
            .expect("default VM config should admit without runtime contracts")
    }

    /// Create an engine with current protocol-machine runtime contracts.
    pub fn new_with_protocol_machine_contracts(
        config: VMConfig,
        handler: Arc<H>,
        runtime_contracts: Option<RuntimeContracts>,
    ) -> Result<Self, AuraChoreoEngineError> {
        Self::new_with_protocol_machine_contracts_and_selector(
            config,
            handler,
            runtime_contracts,
            AuraVmRuntimeSelector::cooperative(),
        )
    }

    /// Create an engine with an explicit runtime selector and current protocol-machine contracts.
    pub fn new_with_protocol_machine_contracts_and_selector(
        config: VMConfig,
        handler: Arc<H>,
        runtime_contracts: Option<RuntimeContracts>,
        runtime_selector: AuraVmRuntimeSelector,
    ) -> Result<Self, AuraChoreoEngineError> {
        let protocol_machine_config =
            admit_protocol_machine_runtime(&config, runtime_contracts.as_ref())?;
        let capability_snapshot = runtime_contracts
            .as_ref()
            .map(telltale_machine::runtime_capability_snapshot)
            .unwrap_or_default();
        let runtime_capabilities = runtime_contracts
            .as_ref()
            .map(RuntimeCapabilityHandler::from_protocol_machine_runtime_contracts)
            .unwrap_or_else(|| {
                RuntimeCapabilityHandler::from_pairs(
                    capability_snapshot
                        .iter()
                        .map(|(name, admitted)| (name.as_str(), *admitted)),
                )
            });

        Ok(Self {
            backend: AuraVmBackend::new(&protocol_machine_config, runtime_selector),
            vm_config: config,
            runtime_selector,
            handler,
            runtime_contracts,
            runtime_capabilities,
            capability_snapshot,
            active_sessions: BTreeSet::new(),
            session_protocol_classes: BTreeMap::new(),
            session_determinism_profiles: BTreeMap::new(),
            session_runtime_selectors: BTreeMap::new(),
            termination_budget_config: TerminationBudgetConfig::default(),
        })
    }

    /// Admit a protocol bundle by checking required runtime capabilities.
    pub async fn admit_bundle(
        &self,
        required_capabilities: &[&str],
    ) -> Result<(), AuraChoreoEngineError> {
        let required = required_capabilities
            .iter()
            .map(|capability| CapabilityKey::new(*capability))
            .collect::<Vec<_>>();
        if let Err(error) = self
            .runtime_capabilities
            .require_capabilities(&required)
            .await
        {
            match error {
                AdmissionError::MissingCapability { capability } => {
                    return Err(AuraChoreoEngineError::MissingRuntimeCapability {
                        capability: capability_key_ref(capability.as_str()),
                    });
                }
                AdmissionError::MissingRuntimeContracts => {
                    return Err(AuraChoreoEngineError::MissingRuntimeContracts);
                }
                AdmissionError::InventoryUnavailable { .. } => {
                    return Err(AuraChoreoEngineError::Interpreter {
                        message: "runtime capability inventory unavailable".to_string(),
                    });
                }
                AdmissionError::Internal { .. } => {
                    return Err(AuraChoreoEngineError::Interpreter {
                        message: "runtime capability admission failed".to_string(),
                    });
                }
            }
        }
        Ok(())
    }

    /// Captured startup runtime capability snapshot.
    pub fn capability_snapshot(&self) -> &[(String, bool)] {
        &self.capability_snapshot
    }

    /// Runtime contracts admitted for this engine instance.
    pub fn runtime_contracts(&self) -> Option<&RuntimeContracts> {
        self.runtime_contracts.as_ref()
    }

    /// Selected runtime mode for this engine instance.
    pub fn runtime_selector(&self) -> AuraVmRuntimeSelector {
        self.runtime_selector
    }

    /// Borrow the underlying VM for advanced operations.
    pub fn vm(&self) -> &VM {
        self.backend
            .as_cooperative()
            .expect("cooperative VM access requested for threaded runtime")
    }

    /// Active VM configuration for this engine instance.
    pub fn vm_config(&self) -> &VMConfig {
        &self.vm_config
    }

    /// Mutably borrow the underlying VM for advanced operations.
    pub fn vm_mut(&mut self) -> &mut VM {
        self.backend
            .as_cooperative_mut()
            .expect("cooperative VM mutation requested for threaded runtime")
    }

    /// Borrow the host effect handler.
    pub fn handler(&self) -> &Arc<H> {
        &self.handler
    }

    /// Configure the multiplier for computed termination budgets.
    pub fn set_termination_budget_multiplier(
        &mut self,
        multiplier: f64,
    ) -> Result<(), AuraChoreoEngineError> {
        if !multiplier.is_finite() || multiplier <= 0.0 {
            return Err(AuraChoreoEngineError::Interpreter {
                message: format!("invalid termination budget multiplier: {multiplier}"),
            });
        }
        self.termination_budget_config.budget_multiplier = multiplier;
        Ok(())
    }

    /// Configure full termination budget settings.
    pub fn set_termination_budget_config(
        &mut self,
        config: TerminationBudgetConfig,
    ) -> Result<(), AuraChoreoEngineError> {
        if !config.budget_multiplier.is_finite() || config.budget_multiplier <= 0.0 {
            return Err(AuraChoreoEngineError::Interpreter {
                message: format!(
                    "invalid termination budget multiplier: {}",
                    config.budget_multiplier
                ),
            });
        }
        if !config.divergence_warn_ratio.is_finite() || config.divergence_warn_ratio <= 1.0 {
            return Err(AuraChoreoEngineError::Interpreter {
                message: format!(
                    "invalid termination divergence ratio: {}",
                    config.divergence_warn_ratio
                ),
            });
        }
        self.termination_budget_config = config;
        Ok(())
    }

    /// Current termination budget configuration.
    pub fn termination_budget_config(&self) -> TerminationBudgetConfig {
        self.termination_budget_config
    }

    /// Load a current protocol-machine code image into the runtime and open a tracked session.
    pub fn open_protocol_machine_session(
        &mut self,
        image: &ProtocolMachineCodeImage,
    ) -> Result<SessionId, AuraChoreoEngineError> {
        let sid = self.backend.load_choreography(image)?;
        self.active_sessions.insert(sid);
        self.session_protocol_classes
            .entry(sid)
            .or_insert(TerminationProtocolClass::RecoveryGrant);
        Ok(sid)
    }

    /// Execute one scheduler step using the configured effect handler.
    pub fn step(&mut self) -> Result<StepResult, AuraChoreoEngineError> {
        let result = self.backend.step_round(
            self.handler.as_ref(),
            self.runtime_selector.scheduler_concurrency,
        );
        let result = result.map_err(|error| self.map_vm_error(error))?;
        self.post_step_cleanup(&result);
        Ok(result)
    }

    /// Run until terminal/stuck/max-round status with a step budget.
    pub fn run(&mut self, max_steps: usize) -> Result<RunStatus, AuraChoreoEngineError> {
        let handler = Arc::clone(&self.handler);
        self.run_with_handler_budget(handler.as_ref(), max_steps)
    }

    /// Run while recording a deterministic VM effect trace.
    pub fn run_recording(
        &mut self,
        max_steps: usize,
    ) -> Result<(RunStatus, Vec<EffectTraceEntry>), AuraChoreoEngineError> {
        let (status, trace) = {
            let handler = Arc::clone(&self.handler);
            let status = self.run_with_handler_budget(handler.as_ref(), max_steps)?;
            let trace = self.backend.effect_trace();
            (status, trace)
        };
        assert_effect_kinds_classified(trace.iter().map(|entry| entry.effect_kind.as_str()))
            .map_err(|error| match error {
                ConformanceValidationError::UnclassifiedEnvelopeKinds { kinds } => {
                    AuraChoreoEngineError::UnclassifiedEnvelopeKinds { kinds }
                }
                ConformanceValidationError::MissingRequiredSurfaces { .. } => {
                    AuraChoreoEngineError::Interpreter {
                        message:
                            "unexpected surface validation error while classifying effect kinds"
                                .to_string(),
                    }
                }
            })?;
        self.refresh_active_sessions();
        Ok((status, trace))
    }

    /// Run while capturing a complete conformance artifact (`observable`, `scheduler_step`, `effect`).
    pub fn run_recording_conformance(
        &mut self,
        max_steps: usize,
        mut metadata: AuraConformanceRunMetadataV1,
    ) -> Result<(RunStatus, AuraConformanceArtifactV1), AuraChoreoEngineError> {
        metadata.vm_determinism_profile = self.active_determinism_profile_metadata();
        let (status, effect_trace) = self.run_recording(max_steps)?;
        let normalized_observable = normalize_trace(&self.backend.observable_trace());
        let canonical_effects = canonical_effect_trace(&effect_trace);

        let mut artifact = AuraConformanceArtifactV1::new(metadata);

        let observable_entries = normalized_observable
            .iter()
            .map(|event| {
                serde_json::to_value(event).map_err(|error| {
                    AuraChoreoEngineError::ConformanceArtifact {
                        message: format!("observable serialization failed: {error}"),
                    }
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        artifact.insert_surface(
            ConformanceSurfaceName::Observable,
            AuraConformanceSurfaceV1::new(observable_entries, None),
        );

        let scheduler_entries = normalized_observable
            .iter()
            .enumerate()
            .map(|(step_index, event)| {
                serde_json::to_value(serde_json::json!({
                    "step_index": step_index,
                    "session": obs_session(event),
                    "event": event,
                }))
                .map_err(|error| AuraChoreoEngineError::ConformanceArtifact {
                    message: format!("scheduler-step serialization failed: {error}"),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        artifact.insert_surface(
            ConformanceSurfaceName::SchedulerStep,
            AuraConformanceSurfaceV1::new(scheduler_entries, None),
        );

        let effect_entries = canonical_effects
            .iter()
            .map(|entry| {
                serde_json::to_value(entry).map_err(|error| {
                    AuraChoreoEngineError::ConformanceArtifact {
                        message: format!("effect-trace serialization failed: {error}"),
                    }
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        artifact.insert_surface(
            ConformanceSurfaceName::Effect,
            AuraConformanceSurfaceV1::new(effect_entries, None),
        );

        artifact.validate_required_surfaces().map_err(|error| {
            AuraChoreoEngineError::ConformanceArtifact {
                message: error.to_string(),
            }
        })?;
        artifact.recompute_digests().map_err(|error| {
            AuraChoreoEngineError::ConformanceArtifact {
                message: format!("digest recompute failed: {error}"),
            }
        })?;

        Ok((status, artifact))
    }

    /// Same as [`Self::run_recording_conformance`] but injects async-host transcript metadata.
    pub fn run_recording_conformance_with_host_transcript(
        &mut self,
        max_steps: usize,
        mut metadata: AuraConformanceRunMetadataV1,
        host_transcript_metadata: Option<(usize, String)>,
    ) -> Result<(RunStatus, AuraConformanceArtifactV1), AuraChoreoEngineError> {
        if let Some((entry_count, digest_hex)) = host_transcript_metadata {
            metadata.async_host_transcript_entries = Some(entry_count);
            metadata.async_host_transcript_digest_hex = Some(digest_hex);
        }
        self.run_recording_conformance(max_steps, metadata)
    }

    /// Replay a previously captured effect trace against the current VM state.
    pub fn run_replay(
        &mut self,
        replay_trace: &[EffectTraceEntry],
        max_steps: usize,
    ) -> Result<RunStatus, AuraChoreoEngineError> {
        let handler = Arc::clone(&self.handler);
        let status = self
            .backend
            .run_replay_shared(
                handler.as_ref(),
                Arc::<[EffectTraceEntry]>::from(replay_trace),
                max_steps,
                self.runtime_selector.scheduler_concurrency,
            )
            .map_err(|error| self.map_vm_error(error))?;
        self.post_run_status(status);
        Ok(status)
    }

    /// VM-maintained effect trace for the current execution.
    pub fn vm_effect_trace(&self) -> Vec<EffectTraceEntry> {
        self.backend.effect_trace()
    }

    /// Canonically normalized VM effect trace for deterministic diffing/replay artifacts.
    pub fn canonical_vm_effect_trace(&self) -> Vec<EffectTraceEntry> {
        canonical_effect_trace(&self.backend.effect_trace())
    }

    /// Explicitly close a tracked session.
    pub fn close_session(&mut self, sid: SessionId) -> Result<(), AuraChoreoEngineError> {
        let vm = self.backend.as_cooperative_mut().ok_or_else(|| {
            AuraChoreoEngineError::SessionLifecycle {
                message: "explicit close is not available for threaded runtime sessions yet"
                    .to_string(),
            }
        })?;
        vm.sessions_mut()
            .close(sid)
            .map_err(|message| AuraChoreoEngineError::SessionLifecycle { message })?;
        self.active_sessions.remove(&sid);
        self.session_protocol_classes.remove(&sid);
        self.session_determinism_profiles.remove(&sid);
        self.session_runtime_selectors.remove(&sid);
        Ok(())
    }

    /// Current set of tracked active sessions.
    pub fn active_sessions(&self) -> &BTreeSet<SessionId> {
        &self.active_sessions
    }

    /// Runtime selector captured at admission time for one tracked session.
    pub fn session_runtime_selector(&self, sid: SessionId) -> Option<AuraVmRuntimeSelector> {
        self.session_runtime_selectors.get(&sid).copied()
    }

    /// Determinism and envelope metadata surfaced at the session boundary.
    pub fn session_determinism_profile_metadata(
        &self,
        sid: SessionId,
    ) -> Option<AuraVmDeterminismProfileV1> {
        self.session_determinism_profiles
            .get(&sid)
            .copied()
            .map(AuraVmProtocolExecutionPolicy::artifact_metadata)
    }

    /// Whether a tracked session requires envelope-diff validation.
    pub fn session_requires_envelope_artifact(&self, sid: SessionId) -> bool {
        self.session_determinism_profiles
            .get(&sid)
            .copied()
            .map(policy_requires_envelope_artifact)
            .unwrap_or(false)
    }

    /// Validate an envelope artifact against the admitted session policy.
    pub fn validate_session_envelope_artifact(
        &self,
        sid: SessionId,
        artifact: Option<&EnvelopeDiffArtifactV1>,
    ) -> Result<(), AuraChoreoEngineError> {
        let policy = self
            .session_determinism_profiles
            .get(&sid)
            .copied()
            .ok_or_else(|| AuraChoreoEngineError::Interpreter {
                message: format!("missing admitted policy for session {sid}"),
            })?;
        validate_envelope_artifact_for_policy(policy, artifact).map_err(|error| {
            AuraChoreoEngineError::Interpreter {
                message: format!("envelope artifact validation failed: {error}"),
            }
        })
    }

    fn run_with_handler_budget(
        &mut self,
        handler: &dyn ProtocolMachineEffectHandler,
        max_steps: usize,
    ) -> Result<RunStatus, AuraChoreoEngineError> {
        let mut budget = self.build_termination_budget(max_steps)?;
        if budget.diverges_significantly(self.termination_budget_config) {
            warn!(
                protocol = %budget.protocol_class,
                multiplier = budget.budget_multiplier,
                divergence_ratio = self.termination_budget_config.divergence_warn_ratio,
                "configured termination budget multiplier diverges significantly from computed bound"
            );
        }

        let mut near_limit_logged = false;
        loop {
            let step = self
                .backend
                .step_round(handler, self.runtime_selector.scheduler_concurrency);
            let step = step.map_err(|error| self.map_vm_error(error))?;
            self.post_step_cleanup(&step);

            if let Err(error) = budget.check_progress() {
                if let TerminationBudgetError::BoundExceeded {
                    protocol_class,
                    initial_weight,
                    max_steps,
                    steps_consumed,
                    ..
                } = &error
                {
                    warn!(
                        protocol = %protocol_class,
                        initial_weight = *initial_weight,
                        max_steps = *max_steps,
                        steps_consumed = *steps_consumed,
                        "termination budget exceeded"
                    );
                }
                return Err(map_termination_error_to_engine(error));
            }

            if !near_limit_logged && budget.utilization() >= 0.8 {
                near_limit_logged = true;
                warn!(
                    protocol = %budget.protocol_class,
                    initial_weight = budget.initial_weight,
                    max_steps = budget.max_steps,
                    steps_consumed = budget.steps_consumed,
                    "termination budget nearing exhaustion (>80% consumed)"
                );
            }

            match step {
                StepResult::Continue => {}
                StepResult::Stuck => {
                    self.post_run_status(RunStatus::Stuck);
                    return Ok(RunStatus::Stuck);
                }
                StepResult::AllDone => {
                    self.post_run_status(RunStatus::AllDone);
                    return Ok(RunStatus::AllDone);
                }
            }
        }
    }

    fn build_termination_budget(
        &self,
        max_steps: usize,
    ) -> Result<TerminationBudget, AuraChoreoEngineError> {
        let initial_weight = self.collect_weighted_measure();
        let protocol_class = self.dominant_protocol_class();

        let mut config = self.termination_budget_config;
        let caller_cap = max_steps as u64;
        config.hard_step_cap = Some(match config.hard_step_cap {
            Some(configured_cap) => configured_cap.min(caller_cap),
            None => caller_cap,
        });
        TerminationBudget::from_weighted_measure(protocol_class, initial_weight, config)
            .map_err(map_termination_error_to_engine)
    }

    fn dominant_protocol_class(&self) -> TerminationProtocolClass {
        self.session_protocol_classes
            .values()
            .copied()
            .max_by(|left, right| {
                left.scheduler_factor()
                    .partial_cmp(&right.scheduler_factor())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or(TerminationProtocolClass::RecoveryGrant)
    }

    fn collect_weighted_measure(&self) -> u64 {
        let Some(vm) = self.backend.as_cooperative() else {
            let (lower_bound, _) = self.dominant_protocol_class().expected_weight_range();
            return lower_bound.max(1);
        };
        let mut local_types = Vec::new();
        let mut buffers = SessionBufferSnapshot::new();

        for session in vm.sessions().iter() {
            for entry in session.local_types.values() {
                local_types.push(entry.current.clone());
            }

            for (edge, buffer) in &session.buffers {
                buffers = buffers.with_buffer(&edge.sender, &edge.receiver, buffer.len() as u64);
            }
        }

        compute_weighted_measure(&local_types, &buffers)
    }

    fn refresh_active_sessions(&mut self) {
        if let Some(vm) = self.backend.as_cooperative() {
            self.active_sessions.retain(|sid| {
                vm.sessions()
                    .get(*sid)
                    .map(|session| session.status == SessionStatus::Active)
                    .unwrap_or(false)
            });
        }
        self.session_protocol_classes
            .retain(|sid, _| self.active_sessions.contains(sid));
        self.session_determinism_profiles
            .retain(|sid, _| self.active_sessions.contains(sid));
        self.session_runtime_selectors
            .retain(|sid, _| self.active_sessions.contains(sid));
    }

    fn cleanup_terminal_sessions(&mut self) {
        let Some(vm) = self.backend.as_cooperative() else {
            return;
        };
        let sessions_to_close = vm
            .sessions()
            .session_ids()
            .into_iter()
            .filter_map(|sid| {
                let session = vm.sessions().get(sid)?;
                if session.status != SessionStatus::Active {
                    return None;
                }
                let has_coroutines = vm.coroutines().iter().any(|coro| coro.session_id == sid);
                let all_terminal = vm
                    .coroutines()
                    .iter()
                    .filter(|coro| coro.session_id == sid)
                    .all(|coro| coro.is_terminal());
                (has_coroutines && all_terminal).then_some(sid)
            })
            .collect::<Vec<_>>();

        if let Some(vm) = self.backend.as_cooperative_mut() {
            for sid in sessions_to_close {
                let _ = vm.sessions_mut().close(sid);
            }
            let _ = vm.reap_closed_sessions();
        }
        self.refresh_active_sessions();
    }

    fn post_step_cleanup(&mut self, step: &StepResult) {
        match self.runtime_selector.runtime_mode {
            AuraVmRuntimeMode::Cooperative => self.cleanup_terminal_sessions(),
            AuraVmRuntimeMode::ThreadedReplayDeterministic
            | AuraVmRuntimeMode::ThreadedEnvelopeBounded => {
                if matches!(step, StepResult::AllDone) {
                    self.active_sessions.clear();
                    self.session_protocol_classes.clear();
                    self.session_determinism_profiles.clear();
                    self.session_runtime_selectors.clear();
                }
            }
        }
    }

    fn post_run_status(&mut self, status: RunStatus) {
        if matches!(status, RunStatus::AllDone) {
            self.active_sessions.clear();
            self.session_protocol_classes.clear();
            self.session_determinism_profiles.clear();
            self.session_runtime_selectors.clear();
            self.backend.reap_closed_sessions();
        }
    }

    fn map_vm_error(&self, error: VMError) -> AuraChoreoEngineError {
        match error {
            VMError::Fault {
                fault: Fault::OutputCondition { predicate_ref },
                ..
            } => {
                let trace = self.backend.observable_trace();
                let diagnostic = trace
                    .iter()
                    .rev()
                    .find_map(|event| match event {
                        ObsEvent::OutputConditionChecked {
                            tick,
                            predicate_ref: trace_predicate,
                            witness_ref,
                            output_digest,
                            passed,
                        } if !*passed && trace_predicate == &predicate_ref => Some((
                            Some(*tick),
                            witness_ref.clone(),
                            Some(output_digest.clone()),
                        )),
                        _ => None,
                    })
                    .unwrap_or((None, None, None));
                AuraChoreoEngineError::OutputConditionRejected {
                    predicate_ref,
                    tick: diagnostic.0,
                    witness_ref: diagnostic.1,
                    output_digest: diagnostic.2,
                }
            }
            other => AuraChoreoEngineError::Vm { source: other },
        }
    }
}

fn capability_key_ref(key: &str) -> String {
    let digest = hash(key.as_bytes());
    hex::encode(&digest[..8])
}

fn map_termination_error_to_engine(error: TerminationBudgetError) -> AuraChoreoEngineError {
    match error {
        TerminationBudgetError::BoundExceeded {
            protocol_class,
            initial_weight,
            max_steps,
            steps_consumed,
            ..
        } => AuraChoreoEngineError::BoundExceeded {
            protocol: protocol_class.to_string(),
            initial_weight,
            max_steps,
            steps_consumed,
        },
        TerminationBudgetError::InvalidMultiplier { multiplier } => {
            AuraChoreoEngineError::Interpreter {
                message: format!("invalid termination budget multiplier: {multiplier}"),
            }
        }
        TerminationBudgetError::InvalidDivergenceRatio { ratio } => {
            AuraChoreoEngineError::Interpreter {
                message: format!("invalid termination divergence ratio: {ratio}"),
            }
        }
    }
}

impl<H: ProtocolMachineEffectHandler> AuraChoreoEngine<H> {
    fn dominant_determinism_policy(&self) -> Option<AuraVmProtocolExecutionPolicy> {
        self.session_determinism_profiles
            .values()
            .copied()
            .max_by(|left, right| {
                left.protocol_class
                    .scheduler_factor()
                    .partial_cmp(&right.protocol_class.scheduler_factor())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    fn active_determinism_profile_metadata(&self) -> Option<AuraVmDeterminismProfileV1> {
        self.dominant_determinism_policy()
            .map(AuraVmProtocolExecutionPolicy::artifact_metadata)
    }
}

impl<H: ProtocolMachineEffectHandler + AuraVmSchedulerSignalsProvider> AuraChoreoEngine<H> {
    /// Admit one explicitly selected runtime policy, then open a current protocol-machine image.
    pub async fn open_protocol_machine_session_for_policy_admitted(
        &mut self,
        runtime_image: &ProtocolMachineCodeImage,
        policy: AuraVmProtocolExecutionPolicy,
        required_capabilities: &[&str],
    ) -> Result<SessionId, AuraChoreoEngineError> {
        let mut admission_capabilities = required_capabilities.to_vec();
        for capability in required_runtime_capabilities_for_policy(policy) {
            if !admission_capabilities
                .iter()
                .any(|existing| existing == capability)
            {
                admission_capabilities.push(capability);
            }
        }
        self.admit_bundle(admission_capabilities.as_slice()).await?;
        validate_protocol_execution_policy(&self.vm_config, policy).map_err(|error| {
            AuraChoreoEngineError::Interpreter {
                message: format!("unsupported VM runtime profile for protocol: {error}"),
            }
        })?;
        let scheduler_input = scheduler_control_input_for_protocol_machine_image(
            runtime_image,
            policy.protocol_class,
            configured_guard_capacity(&self.vm_config),
            self.handler.scheduler_signals(),
        );
        validate_scheduler_execution_policy(&self.vm_config, scheduler_input).map_err(|error| {
            AuraChoreoEngineError::Interpreter {
                message: format!("unsupported VM scheduler profile for protocol: {error}"),
            }
        })?;
        let sid = self.open_protocol_machine_session(runtime_image)?;
        self.session_runtime_selectors
            .insert(sid, AuraVmRuntimeSelector::for_policy(policy));
        self.session_protocol_classes
            .insert(sid, policy.protocol_class);
        self.session_determinism_profiles.insert(sid, policy);
        Ok(sid)
    }
}

impl AuraChoreoEngine<AuraVmEffectHandler> {
    /// Drain VM-emitted envelopes and execute their commands through an Aura interpreter.
    pub async fn flush_effect_envelopes<I: EffectInterpreter>(
        &self,
        interpreter: &I,
    ) -> Result<Vec<EffectResult>, AuraChoreoEngineError> {
        let envelopes = self.handler.drain_envelopes();
        let mut results = Vec::new();

        for envelope in envelopes {
            for command in envelope.commands {
                let result = interpreter.execute(command).await.map_err(|err| {
                    AuraChoreoEngineError::Interpreter {
                        message: err.to_string(),
                    }
                })?;
                results.push(result);
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_mpst::upstream::types::{GlobalType, LocalTypeR};
    use std::collections::BTreeMap;
    use telltale_machine::{
        runtime::loader::CodeImage as ProtocolMachineCodeImage, RuntimeContracts,
    };

    #[test]
    fn step_reaps_completed_sessions() {
        let runtime_image = ProtocolMachineCodeImage::from_local_types(
            &BTreeMap::from([("Solo".to_string(), LocalTypeR::End)]),
            &GlobalType::End,
        );
        let mut engine = AuraChoreoEngine::new(
            vm_config_for_profile(AuraVmHardeningProfile::Prod),
            Arc::new(AuraVmEffectHandler::default()),
        );

        let sid = engine
            .open_protocol_machine_session(&runtime_image)
            .expect("session opens");
        assert!(engine.active_sessions().contains(&sid));
        assert!(engine.vm().sessions().get(sid).is_some());

        let status = engine.run(8).expect("run succeeds");

        assert!(matches!(status, RunStatus::AllDone));
        assert!(!engine.active_sessions().contains(&sid));
        assert!(engine.vm().sessions().get(sid).is_none());
        assert_eq!(engine.vm().sessions().archived_closed().len(), 1);
    }

    #[test]
    fn explicit_threaded_selector_builds_threaded_backend() {
        let runtime_image = ProtocolMachineCodeImage::from_local_types(
            &BTreeMap::from([("Solo".to_string(), LocalTypeR::End)]),
            &GlobalType::End,
        );
        let policy =
            super::super::vm_hardening::policy_for_protocol("aura.sync.epoch_rotation", None)
                .expect("sync policy");
        let selector = AuraVmRuntimeSelector::for_policy(policy);
        let mut config = vm_config_for_profile(AuraVmHardeningProfile::Prod);
        super::super::vm_hardening::apply_protocol_execution_policy(&mut config, policy);
        let mut engine = AuraChoreoEngine::new_with_protocol_machine_contracts_and_selector(
            config,
            Arc::new(AuraVmEffectHandler::default()),
            Some(RuntimeContracts::full()),
            selector,
        )
        .expect("threaded engine");

        assert_eq!(
            engine.runtime_selector(),
            AuraVmRuntimeSelector {
                runtime_mode: AuraVmRuntimeMode::ThreadedEnvelopeBounded,
                threaded_workers: 4,
                scheduler_concurrency: 4,
            }
        );

        let sid = engine
            .open_protocol_machine_session(&runtime_image)
            .expect("session opens");
        assert!(engine.active_sessions().contains(&sid));
        let status = engine.run(8).expect("threaded run succeeds");
        assert!(matches!(status, RunStatus::AllDone));
        assert!(engine.active_sessions().is_empty());
    }

    #[tokio::test]
    async fn admission_surfaces_threaded_envelope_metadata() {
        let runtime_image = ProtocolMachineCodeImage::from_local_types(
            &BTreeMap::from([("Solo".to_string(), LocalTypeR::End)]),
            &GlobalType::End,
        );
        let policy =
            super::super::vm_hardening::policy_for_protocol("aura.sync.epoch_rotation", None)
                .expect("sync policy");
        let selector = AuraVmRuntimeSelector::for_policy(policy);
        let mut config = vm_config_for_profile(AuraVmHardeningProfile::Prod);
        super::super::vm_hardening::apply_protocol_execution_policy(&mut config, policy);
        let scheduler_input =
            super::super::vm_hardening::scheduler_control_input_for_protocol_machine_image(
                &runtime_image,
                policy.protocol_class,
                super::super::vm_hardening::configured_guard_capacity(&config),
                super::super::vm_hardening::AuraVmSchedulerSignals::default(),
            );
        let scheduler_policy =
            super::super::vm_hardening::scheduler_policy_for_input(scheduler_input);
        super::super::vm_hardening::apply_scheduler_execution_policy(
            &mut config,
            &scheduler_policy,
        );
        let mut engine = AuraChoreoEngine::new_with_protocol_machine_contracts_and_selector(
            config,
            Arc::new(AuraVmEffectHandler::default()),
            Some(RuntimeContracts::full()),
            selector,
        )
        .expect("engine");

        let sid = engine
            .open_protocol_machine_session_for_policy_admitted(&runtime_image, policy, &[])
            .await
            .expect("admission succeeds");

        assert_eq!(engine.session_runtime_selector(sid), Some(selector));
        assert!(engine.session_requires_envelope_artifact(sid));
        assert_eq!(
            engine
                .session_determinism_profile_metadata(sid)
                .map(|metadata| (
                    metadata.runtime_mode,
                    metadata.scheduler_envelope_class,
                    metadata.declared_wave_width_bound,
                )),
            Some((
                "threaded_envelope_bounded".to_string(),
                "envelope_bounded".to_string(),
                Some(4),
            ))
        );
        let err = engine
            .validate_session_envelope_artifact(sid, None)
            .expect_err("missing envelope artifact must fail closed");
        assert!(
            err.to_string().contains("missing envelope artifact"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn admission_surfaces_replay_threaded_metadata() {
        let runtime_image = ProtocolMachineCodeImage::from_local_types(
            &BTreeMap::from([("Solo".to_string(), LocalTypeR::End)]),
            &GlobalType::End,
        );
        let policy = super::super::vm_hardening::policy_for_protocol(
            "aura.dkg.ceremony",
            Some(super::super::vm_hardening::AURA_VM_POLICY_DKG_CEREMONY),
        )
        .expect("dkg policy");
        let selector = AuraVmRuntimeSelector::for_policy(policy);
        let mut config = vm_config_for_profile(AuraVmHardeningProfile::Prod);
        super::super::vm_hardening::apply_protocol_execution_policy(&mut config, policy);
        let scheduler_input =
            super::super::vm_hardening::scheduler_control_input_for_protocol_machine_image(
                &runtime_image,
                policy.protocol_class,
                super::super::vm_hardening::configured_guard_capacity(&config),
                super::super::vm_hardening::AuraVmSchedulerSignals::default(),
            );
        let scheduler_policy =
            super::super::vm_hardening::scheduler_policy_for_input(scheduler_input);
        super::super::vm_hardening::apply_scheduler_execution_policy(
            &mut config,
            &scheduler_policy,
        );
        let mut engine = AuraChoreoEngine::new_with_protocol_machine_contracts_and_selector(
            config,
            Arc::new(AuraVmEffectHandler::default()),
            Some(RuntimeContracts::full()),
            selector,
        )
        .expect("engine");

        let sid = engine
            .open_protocol_machine_session_for_policy_admitted(&runtime_image, policy, &[])
            .await
            .expect("admission succeeds");

        assert_eq!(engine.session_runtime_selector(sid), Some(selector));
        assert!(!engine.session_requires_envelope_artifact(sid));
        assert_eq!(
            engine
                .session_determinism_profile_metadata(sid)
                .map(|metadata| (
                    metadata.runtime_mode,
                    metadata.scheduler_envelope_class,
                    metadata.declared_wave_width_bound,
                )),
            Some((
                "threaded_replay_deterministic".to_string(),
                "session_normalized_permutation".to_string(),
                Some(2),
            ))
        );
    }

    #[tokio::test]
    async fn conformance_artifact_includes_selected_determinism_profile() {
        let runtime_image = ProtocolMachineCodeImage::from_local_types(
            &BTreeMap::from([("Solo".to_string(), LocalTypeR::End)]),
            &GlobalType::End,
        );
        let policy = super::super::vm_hardening::policy_for_protocol("aura.recovery.grant", None)
            .expect("policy");
        let mut config = vm_config_for_profile(AuraVmHardeningProfile::Prod);
        super::super::vm_hardening::apply_protocol_execution_policy(&mut config, policy);
        let scheduler_input =
            super::super::vm_hardening::scheduler_control_input_for_protocol_machine_image(
                &runtime_image,
                policy.protocol_class,
                super::super::vm_hardening::configured_guard_capacity(&config),
                super::super::vm_hardening::AuraVmSchedulerSignals::default(),
            );
        let scheduler_policy =
            super::super::vm_hardening::scheduler_policy_for_input(scheduler_input);
        super::super::vm_hardening::apply_scheduler_execution_policy(
            &mut config,
            &scheduler_policy,
        );
        let mut engine = AuraChoreoEngine::new_with_protocol_machine_contracts(
            config,
            Arc::new(AuraVmEffectHandler::default()),
            Some(RuntimeContracts::full()),
        )
        .expect("engine");

        engine
            .open_protocol_machine_session_for_policy_admitted(&runtime_image, policy, &[])
            .await
            .expect("session opens");
        let admitted_sid = *engine
            .active_sessions()
            .iter()
            .next()
            .expect("tracked session");
        assert_eq!(
            engine.session_runtime_selector(admitted_sid),
            Some(AuraVmRuntimeSelector::cooperative())
        );

        let (_status, artifact) = engine
            .run_recording_conformance(
                8,
                AuraConformanceRunMetadataV1 {
                    target: "native".to_string(),
                    profile: "test".to_string(),
                    scenario: "determinism_profile".to_string(),
                    seed: Some(1),
                    commit: None,
                    async_host_transcript_entries: None,
                    async_host_transcript_digest_hex: None,
                    vm_determinism_profile: None,
                },
            )
            .expect("conformance run succeeds");

        assert_eq!(
            artifact
                .metadata
                .vm_determinism_profile
                .as_ref()
                .map(|profile| profile.policy_ref.as_str()),
            Some("aura.vm.recovery_grant.prod")
        );
    }
}
