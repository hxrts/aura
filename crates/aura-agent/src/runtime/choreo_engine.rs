//! Telltale VM-backed choreography engine for Aura runtime integration.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use aura_core::conformance::{
    assert_effect_kinds_classified, AuraConformanceArtifactV1, AuraConformanceRunMetadataV1,
    AuraConformanceSurfaceV1, ConformanceSurfaceName, ConformanceValidationError,
};
use aura_core::effects::guard::{EffectInterpreter, EffectResult};
use aura_core::effects::{AdmissionError, CapabilityKey, RuntimeCapabilityEffects};
use aura_core::hash::hash;
use aura_effects::RuntimeCapabilityHandler;
use aura_mpst::termination::{compute_weighted_measure, SessionBufferSnapshot};
use aura_protocol::admission::{CAPABILITY_BYZANTINE_ENVELOPE, CAPABILITY_TERMINATION_BOUNDED};
use aura_protocol::termination::{
    TerminationBudget, TerminationBudgetConfig, TerminationBudgetError, TerminationProtocolClass,
};
use telltale_vm::coroutine::Fault;
use telltale_vm::effect::EffectHandler as VmEffectHandler;
use telltale_vm::loader::CodeImage;
use telltale_vm::runtime_contracts::{
    enforce_vm_runtime_gates, runtime_capability_snapshot, RuntimeContracts, RuntimeGateResult,
};
use telltale_vm::session::SessionStatus;
use telltale_vm::trace::{normalize_trace, obs_session};
use telltale_vm::vm::{ObsEvent, RunStatus, StepResult, VMError};
use telltale_vm::{
    canonical_effect_trace, EffectTraceEntry, RecordingEffectHandler, ReplayEffectHandler,
};
use telltale_vm::{SessionId, VMConfig, VM};
use tracing::warn;

use super::vm_effect_handler::AuraVmEffectHandler;
use super::vm_hardening::{
    build_vm_config, validate_determinism_profile, vm_config_for_profile, AuraVmHardeningProfile,
    AuraVmParityProfile,
};

/// Errors raised by [`AuraChoreoEngine`].
#[derive(Debug, thiserror::Error)]
pub enum AuraChoreoEngineError {
    /// VM execution/lifecycle error.
    #[error("vm error: {source}")]
    Vm {
        /// Wrapped VM error.
        source: VMError,
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

impl From<VMError> for AuraChoreoEngineError {
    fn from(source: VMError) -> Self {
        Self::Vm { source }
    }
}

/// VM-backed choreography engine with explicit session lifecycle hooks.
#[derive(Debug)]
pub struct AuraChoreoEngine<H: VmEffectHandler = AuraVmEffectHandler> {
    vm: VM,
    handler: Arc<H>,
    runtime_contracts: Option<RuntimeContracts>,
    runtime_capabilities: RuntimeCapabilityHandler,
    capability_snapshot: Vec<(String, bool)>,
    active_sessions: BTreeSet<SessionId>,
    session_protocol_classes: BTreeMap<SessionId, TerminationProtocolClass>,
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

impl<H: VmEffectHandler> AuraChoreoEngine<H> {
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
        Self::new_with_contracts(config, handler, None)
            .expect("default VM config should admit without runtime contracts")
    }

    /// Create an engine with admission checks and capability snapshot capture.
    pub fn new_with_contracts(
        config: VMConfig,
        handler: Arc<H>,
        runtime_contracts: Option<RuntimeContracts>,
    ) -> Result<Self, AuraChoreoEngineError> {
        validate_determinism_profile(&config).map_err(|error| {
            AuraChoreoEngineError::Interpreter {
                message: format!("invalid VM determinism profile: {error}"),
            }
        })?;

        match enforce_vm_runtime_gates(&config, runtime_contracts.as_ref()) {
            RuntimeGateResult::Admitted => {}
            RuntimeGateResult::RejectedMissingContracts => {
                return Err(AuraChoreoEngineError::MissingRuntimeContracts);
            }
            RuntimeGateResult::RejectedUnsupportedDeterminismProfile => {
                return Err(AuraChoreoEngineError::UnsupportedDeterminismProfile);
            }
        }

        let capability_snapshot = runtime_contracts
            .as_ref()
            .map(runtime_capability_snapshot)
            .unwrap_or_default();
        let runtime_capabilities = RuntimeCapabilityHandler::from_pairs(
            capability_snapshot
                .iter()
                .map(|(name, admitted)| (name.as_str(), *admitted)),
        );

        Ok(Self {
            vm: VM::new(config),
            handler,
            runtime_contracts,
            runtime_capabilities,
            capability_snapshot,
            active_sessions: BTreeSet::new(),
            session_protocol_classes: BTreeMap::new(),
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

    /// Borrow the underlying VM for advanced operations.
    pub fn vm(&self) -> &VM {
        &self.vm
    }

    /// Mutably borrow the underlying VM for advanced operations.
    pub fn vm_mut(&mut self) -> &mut VM {
        &mut self.vm
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

    /// Load a choreography image into the VM and open a tracked session.
    pub fn open_session(&mut self, image: &CodeImage) -> Result<SessionId, AuraChoreoEngineError> {
        let sid = self.vm.load_choreography(image)?;
        self.active_sessions.insert(sid);
        self.session_protocol_classes
            .entry(sid)
            .or_insert(TerminationProtocolClass::RecoveryGrant);
        Ok(sid)
    }

    /// Admit required runtime capabilities, then open the choreography session.
    pub async fn open_session_admitted(
        &mut self,
        image: &CodeImage,
        required_capabilities: &[&str],
    ) -> Result<SessionId, AuraChoreoEngineError> {
        self.admit_bundle(required_capabilities).await?;
        let sid = self.open_session(image)?;
        self.session_protocol_classes.insert(
            sid,
            protocol_class_for_required_capabilities(required_capabilities),
        );
        Ok(sid)
    }

    /// Execute one scheduler step using the configured effect handler.
    pub fn step(&mut self) -> Result<StepResult, AuraChoreoEngineError> {
        let result = self.vm.step(self.handler.as_ref());
        self.cleanup_terminal_sessions();
        let result = result.map_err(|error| self.map_vm_error(error))?;
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
            let recorder = RecordingEffectHandler::new(handler.as_ref());
            let status = self.run_with_handler_budget(&recorder, max_steps)?;
            let trace = recorder.effect_trace();
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
        metadata: AuraConformanceRunMetadataV1,
    ) -> Result<(RunStatus, AuraConformanceArtifactV1), AuraChoreoEngineError> {
        let (status, effect_trace) = self.run_recording(max_steps)?;
        let normalized_observable = normalize_trace(self.vm.trace());
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
        let replay = ReplayEffectHandler::with_fallback(
            Arc::<[EffectTraceEntry]>::from(replay_trace),
            handler.as_ref(),
        );
        self.run_with_handler_budget(&replay, max_steps)
    }

    /// VM-maintained effect trace for the current execution.
    pub fn vm_effect_trace(&self) -> &[EffectTraceEntry] {
        self.vm.effect_trace()
    }

    /// Canonically normalized VM effect trace for deterministic diffing/replay artifacts.
    pub fn canonical_vm_effect_trace(&self) -> Vec<EffectTraceEntry> {
        canonical_effect_trace(self.vm.effect_trace())
    }

    /// Explicitly close a tracked session.
    pub fn close_session(&mut self, sid: SessionId) -> Result<(), AuraChoreoEngineError> {
        self.vm
            .sessions_mut()
            .close(sid)
            .map_err(|message| AuraChoreoEngineError::SessionLifecycle { message })?;
        self.active_sessions.remove(&sid);
        self.session_protocol_classes.remove(&sid);
        Ok(())
    }

    /// Current set of tracked active sessions.
    pub fn active_sessions(&self) -> &BTreeSet<SessionId> {
        &self.active_sessions
    }

    fn run_with_handler_budget(
        &mut self,
        handler: &dyn VmEffectHandler,
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
            let step = self.vm.step(handler);
            self.cleanup_terminal_sessions();
            let step = step.map_err(|error| self.map_vm_error(error))?;

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
                StepResult::Stuck => return Ok(RunStatus::Stuck),
                StepResult::AllDone => return Ok(RunStatus::AllDone),
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
        let mut local_types = Vec::new();
        let mut buffers = SessionBufferSnapshot::new();

        for session in self.vm.sessions().iter() {
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
        self.active_sessions.retain(|sid| {
            self.vm
                .sessions()
                .get(*sid)
                .map(|session| session.status == SessionStatus::Active)
                .unwrap_or(false)
        });
        self.session_protocol_classes
            .retain(|sid, _| self.active_sessions.contains(sid));
    }

    fn cleanup_terminal_sessions(&mut self) {
        let sessions_to_close = self
            .vm
            .sessions()
            .session_ids()
            .into_iter()
            .filter_map(|sid| {
                let session = self.vm.sessions().get(sid)?;
                if session.status != SessionStatus::Active {
                    return None;
                }
                let has_coroutines = self
                    .vm
                    .coroutines()
                    .iter()
                    .any(|coro| coro.session_id == sid);
                let all_terminal = self
                    .vm
                    .coroutines()
                    .iter()
                    .filter(|coro| coro.session_id == sid)
                    .all(|coro| coro.is_terminal());
                (has_coroutines && all_terminal).then_some(sid)
            })
            .collect::<Vec<_>>();

        for sid in sessions_to_close {
            let _ = self.vm.sessions_mut().close(sid);
        }
        self.refresh_active_sessions();
        let _ = self.vm.reap_closed_sessions();
        self.refresh_active_sessions();
    }

    fn map_vm_error(&self, error: VMError) -> AuraChoreoEngineError {
        match error {
            VMError::Fault {
                fault: Fault::OutputCondition { predicate_ref },
                ..
            } => {
                let diagnostic = self
                    .vm
                    .trace()
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

fn protocol_class_for_required_capabilities(
    required_capabilities: &[&str],
) -> TerminationProtocolClass {
    if required_capabilities
        .iter()
        .any(|capability| *capability == CAPABILITY_TERMINATION_BOUNDED)
    {
        return TerminationProtocolClass::SyncAntiEntropy;
    }
    if required_capabilities
        .iter()
        .any(|capability| *capability == CAPABILITY_BYZANTINE_ENVELOPE)
    {
        return TerminationProtocolClass::ConsensusFallback;
    }
    TerminationProtocolClass::RecoveryGrant
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
    use aura_mpst::telltale_types::{GlobalType, LocalTypeR};
    use std::collections::BTreeMap;

    #[test]
    fn step_reaps_completed_sessions() {
        let image = CodeImage::from_local_types(
            &BTreeMap::from([("Solo".to_string(), LocalTypeR::End)]),
            &GlobalType::End,
        );
        let mut engine = AuraChoreoEngine::new(
            vm_config_for_profile(AuraVmHardeningProfile::Prod),
            Arc::new(AuraVmEffectHandler::default()),
        );

        let sid = engine.open_session(&image).expect("session opens");
        assert!(engine.active_sessions().contains(&sid));
        assert!(engine.vm().sessions().get(sid).is_some());

        let status = engine.run(8).expect("run succeeds");

        assert!(matches!(status, RunStatus::AllDone));
        assert!(!engine.active_sessions().contains(&sid));
        assert!(engine.vm().sessions().get(sid).is_none());
        assert_eq!(engine.vm().sessions().archived_closed().len(), 1);
    }
}
