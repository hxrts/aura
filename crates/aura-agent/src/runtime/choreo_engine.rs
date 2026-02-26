//! Telltale VM-backed choreography engine for Aura runtime integration.

use std::collections::BTreeSet;
use std::sync::Arc;

use aura_core::effects::guard::{EffectInterpreter, EffectResult};
use telltale_vm::effect::EffectHandler as VmEffectHandler;
use telltale_vm::loader::CodeImage;
use telltale_vm::runtime_contracts::{
    enforce_vm_runtime_gates, runtime_capability_snapshot, RuntimeContracts, RuntimeGateResult,
};
use telltale_vm::session::SessionStatus;
use telltale_vm::vm::{RunStatus, StepResult, VMError};
use telltale_vm::{canonical_effect_trace, EffectTraceEntry, RecordingEffectHandler};
use telltale_vm::{SessionId, VMConfig, VM};

use super::vm_effect_handler::AuraVmEffectHandler;

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
    capability_snapshot: Vec<(String, bool)>,
    active_sessions: BTreeSet<SessionId>,
}

impl Default for AuraChoreoEngine<AuraVmEffectHandler> {
    fn default() -> Self {
        Self::new(
            VMConfig::default(),
            Arc::new(AuraVmEffectHandler::default()),
        )
    }
}

impl<H: VmEffectHandler> AuraChoreoEngine<H> {
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

        Ok(Self {
            vm: VM::new(config),
            handler,
            runtime_contracts,
            capability_snapshot,
            active_sessions: BTreeSet::new(),
        })
    }

    /// Admit a protocol bundle by checking required runtime capabilities.
    pub fn admit_bundle(
        &self,
        required_capabilities: &[&str],
    ) -> Result<(), AuraChoreoEngineError> {
        for required in required_capabilities {
            let has_capability = self
                .capability_snapshot
                .iter()
                .find(|(name, _)| name == required)
                .map(|(_, value)| *value)
                .unwrap_or(false);
            if !has_capability {
                return Err(AuraChoreoEngineError::MissingRuntimeCapability {
                    capability: (*required).to_string(),
                });
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

    /// Load a choreography image into the VM and open a tracked session.
    pub fn open_session(&mut self, image: &CodeImage) -> Result<SessionId, AuraChoreoEngineError> {
        let sid = self.vm.load_choreography(image)?;
        self.active_sessions.insert(sid);
        Ok(sid)
    }

    /// Execute one scheduler step using the configured effect handler.
    pub fn step(&mut self) -> Result<StepResult, AuraChoreoEngineError> {
        let result = self.vm.step(self.handler.as_ref())?;
        self.refresh_active_sessions();
        Ok(result)
    }

    /// Run until terminal/stuck/max-round status with a step budget.
    pub fn run(&mut self, max_steps: usize) -> Result<RunStatus, AuraChoreoEngineError> {
        let status = self.vm.run(self.handler.as_ref(), max_steps)?;
        self.refresh_active_sessions();
        Ok(status)
    }

    /// Run while recording a deterministic VM effect trace.
    pub fn run_recording(
        &mut self,
        max_steps: usize,
    ) -> Result<(RunStatus, Vec<EffectTraceEntry>), AuraChoreoEngineError> {
        let (status, trace) = {
            let recorder = RecordingEffectHandler::new(self.handler.as_ref());
            let status = self.vm.run(&recorder, max_steps)?;
            let trace = recorder.effect_trace();
            (status, trace)
        };
        self.refresh_active_sessions();
        Ok((status, trace))
    }

    /// Replay a previously captured effect trace against the current VM state.
    pub fn run_replay(
        &mut self,
        replay_trace: &[EffectTraceEntry],
        max_steps: usize,
    ) -> Result<RunStatus, AuraChoreoEngineError> {
        let status = self
            .vm
            .run_replay(self.handler.as_ref(), replay_trace, max_steps)?;
        self.refresh_active_sessions();
        Ok(status)
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
        Ok(())
    }

    /// Current set of tracked active sessions.
    pub fn active_sessions(&self) -> &BTreeSet<SessionId> {
        &self.active_sessions
    }

    fn refresh_active_sessions(&mut self) {
        self.active_sessions.retain(|sid| {
            self.vm
                .sessions()
                .get(*sid)
                .map(|session| session.status == SessionStatus::Active)
                .unwrap_or(false)
        });
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
