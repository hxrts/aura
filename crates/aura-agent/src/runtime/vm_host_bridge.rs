#![allow(clippy::disallowed_types)] // Synchronous VM host callbacks require short, non-async critical sections.

use crate::runtime::vm_hardening::{
    apply_protocol_execution_policy, apply_scheduler_execution_policy, configured_guard_capacity,
    policy_for_protocol, scheduler_control_input_for_protocol_machine_image, scheduler_policy_for_input,
    AuraVmProtocolExecutionPolicy, AuraVmRuntimeSelector, AuraVmSchedulerSignals,
    AuraVmSchedulerSignalsProvider,
};
use crate::runtime::{
    build_vm_config, AuraChoreoEngine, AuraEffectSystem, AuraRuntimeEnvelopeAdmission,
    AuraVmHardeningProfile, AuraVmParityProfile, RuntimeVmEvent,
};
use aura_core::effects::{VmBridgeEffects, VmBridgePendingSend};
use aura_core::AuraVmDeterminismProfileV1;
use aura_mpst::upstream::types::{GlobalType, LocalTypeR};
use aura_mpst::{CompositionManifest, GuardCapabilityAdmission};
use aura_protocol::effects::{ChoreographicEffects, ChoreographicRole, ChoreographyError};
use serde::{de::DeserializeOwned, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use telltale_machine::model::effects::{EffectFailure, EffectHandler, EffectResult};
use telltale_machine::coroutine::{
    BlockReason as ProtocolMachineBlockReason, CoroStatus as ProtocolMachineCoroStatus,
};
use telltale_machine::{
    runtime::loader::CodeImage as ProtocolMachineCodeImage, EffectTraceEntry, ProtocolMachine,
    RuntimeContracts, SessionId, StepResult as ProtocolMachineStepResult,
    Value,
};
use telltale_vm::loader::CodeImage;

use super::subsystems::VmBridgeState;

#[derive(Debug, thiserror::Error)]
pub enum AuraVmConcurrencyEnvelopeError {
    #[error(
        "protocol {protocol_id} denied requested concurrency profile {requested_profile} for policy {requested_policy_ref}: {reason}"
    )]
    AdmissionDenied {
        protocol_id: String,
        requested_policy_ref: String,
        requested_profile: String,
        reason: &'static str,
    },
    #[error(
        "protocol {protocol_id} activated canonical fallback from policy {requested_policy_ref} to {effective_policy_ref}: {reason}"
    )]
    CanonicalFallbackActivated {
        protocol_id: String,
        requested_policy_ref: String,
        effective_policy_ref: String,
        reason: &'static str,
    },
}

impl AuraVmConcurrencyEnvelopeError {
    fn error_kind(&self) -> &'static str {
        match self {
            Self::AdmissionDenied { .. } => "admission_denied",
            Self::CanonicalFallbackActivated { .. } => "canonical_fallback_activated",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AuraVmSessionOpenError {
    #[error("failed to build role-scoped VM image for protocol {protocol_id}: {message}")]
    RoleScopedImage {
        protocol_id: String,
        message: String,
    },
    #[error("failed to resolve VM protocol policy for protocol {protocol_id}: {source}")]
    PolicyResolution {
        protocol_id: String,
        #[source]
        source: crate::runtime::AuraVmDeterminismProfileError,
    },
    #[error("failed to claim VM fragments for protocol {protocol_id}: {message}")]
    FragmentClaim {
        protocol_id: String,
        message: String,
    },
    #[error("failed to admit declared guard capabilities for protocol {protocol_id}: {message}")]
    ManifestGuardCapability {
        protocol_id: String,
        message: String,
    },
    #[error(
        "failed to create VM engine for protocol {protocol_id} under policy {policy_ref}: {source}"
    )]
    EngineCreation {
        protocol_id: String,
        policy_ref: String,
        #[source]
        source: crate::runtime::AuraChoreoEngineError,
    },
    #[error(
        "failed to open VM session for protocol {protocol_id} under policy {policy_ref}: {source}"
    )]
    SessionOpen {
        protocol_id: String,
        policy_ref: String,
        #[source]
        source: crate::runtime::AuraChoreoEngineError,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedVmReceive {
    pub from_role: String,
    pub to_role: String,
    pub peer_role: ChoreographicRole,
    pub payload: Vec<u8>,
}

#[derive(Debug)]
pub struct AuraVmBridgeRound {
    pub step: ProtocolMachineStepResult,
    pub blocked_receive: Option<BlockedVmReceive>,
    pub host_wait_status: AuraVmHostWaitStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuraVmRoundDisposition {
    Continue,
    Complete,
}

#[allow(dead_code)] // Task 5 exposes reusable session snapshots before all services consume them.
#[derive(Debug, Clone)]
pub struct AuraVmSessionArtifactSnapshot {
    pub session_id: SessionId,
    pub determinism_profile: Option<AuraVmDeterminismProfileV1>,
    pub requires_envelope_artifact: bool,
    pub effect_trace: Vec<EffectTraceEntry>,
    pub canonical_effect_trace: Vec<EffectTraceEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuraVmHostWaitStatus {
    Idle,
    Delivered,
    TimedOut,
    Cancelled,
    Deferred,
}

pub struct AuraQueuedVmBridgeHandler {
    bridge_effects: Arc<dyn VmBridgeEffects>,
}

impl std::fmt::Debug for AuraQueuedVmBridgeHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuraQueuedVmBridgeHandler")
            .field("bridge_effects", &"session-local")
            .field(
                "scheduler_signals",
                &self.bridge_effects.scheduler_signals(),
            )
            .finish()
    }
}

impl Default for AuraQueuedVmBridgeHandler {
    fn default() -> Self {
        Self::with_bridge_effects(Arc::new(VmBridgeState::new()))
    }
}

impl AuraQueuedVmBridgeHandler {
    pub fn with_bridge_effects(bridge_effects: Arc<dyn VmBridgeEffects>) -> Self {
        Self { bridge_effects }
    }

    pub fn push_send_bytes(&self, payload: Vec<u8>) {
        self.bridge_effects.enqueue_outbound_payload(payload);
    }

    pub fn drain_pending_sends(&self) -> Vec<VmBridgePendingSend> {
        self.bridge_effects.drain_pending_sends()
    }

    #[allow(dead_code)]
    pub fn push_choice_label(&self, label: impl Into<String>) {
        self.bridge_effects.enqueue_branch_choice(label.into());
    }

    pub fn bytes_to_value(payload: &[u8]) -> Value {
        Value::Str(hex::encode(payload))
    }

    #[allow(dead_code)]
    pub fn set_scheduler_signals(&self, signals: AuraVmSchedulerSignals) {
        self.bridge_effects.set_scheduler_signals(signals);
    }
}

impl AuraVmSchedulerSignalsProvider for AuraQueuedVmBridgeHandler {
    fn scheduler_signals(&self) -> AuraVmSchedulerSignals {
        self.bridge_effects.scheduler_signals()
    }
}

impl EffectHandler for AuraQueuedVmBridgeHandler {
    fn handler_identity(&self) -> String {
        "aura-vm-host-bridge".to_string()
    }

    fn handle_send(
        &self,
        role: &str,
        partner: &str,
        label: &str,
        _state: &[Value],
    ) -> EffectResult<Value> {
        let payload_bytes = self
            .bridge_effects
            .dequeue_outbound_payload()
            .ok_or_else(|| {
                format!("missing queued outbound payload for VM send {role}->{partner}:{label}")
            });
        let payload_bytes = match payload_bytes {
            Ok(payload_bytes) => payload_bytes,
            Err(error) => {
                return EffectResult::failure(EffectFailure::contract_violation(error));
            }
        };
        self.bridge_effects
            .record_pending_send(VmBridgePendingSend {
                from_role: role.to_string(),
                to_role: partner.to_string(),
                label: label.to_string(),
                payload: payload_bytes.clone(),
            });
        EffectResult::success(Self::bytes_to_value(&payload_bytes))
    }

    fn handle_recv(
        &self,
        _role: &str,
        _partner: &str,
        _label: &str,
        state: &mut Vec<Value>,
        payload: &Value,
    ) -> EffectResult<()> {
        if let Some(last) = state.last_mut() {
            *last = payload.clone();
        } else {
            state.push(payload.clone());
        }
        EffectResult::success(())
    }

    fn handle_choose(
        &self,
        _role: &str,
        _partner: &str,
        labels: &[String],
        _state: &[Value],
    ) -> EffectResult<String> {
        if let Some(choice) = self.bridge_effects.dequeue_branch_choice() {
            if labels.iter().any(|label| label == &choice) {
                return EffectResult::success(choice);
            }
            return EffectResult::failure(EffectFailure::contract_violation(format!(
                "queued VM choice {choice} not present in offered labels {labels:?}"
            )));
        }
        labels
            .first()
            .cloned()
            .map(EffectResult::success)
            .unwrap_or_else(|| {
                EffectResult::failure(EffectFailure::contract_violation(
                    "no labels available for VM bridge choice",
                ))
            })
    }

    fn step(&self, _role: &str, _state: &mut Vec<Value>) -> EffectResult<()> {
        EffectResult::success(())
    }
}

fn transcode_protocol_machine<T, U>(value: &T, context: &str) -> Result<U, String>
where
    T: Serialize + ?Sized,
    U: DeserializeOwned,
{
    let payload = serde_json::to_value(value)
        .map_err(|error| format!("{context} serialization failed: {error}"))?;
    serde_json::from_value(payload)
        .map_err(|error| format!("{context} deserialization failed: {error}"))
}

fn effective_host_bridge_policy(
    protocol_id: &str,
    policy: AuraVmProtocolExecutionPolicy,
) -> (
    AuraVmProtocolExecutionPolicy,
    AuraRuntimeEnvelopeAdmission,
    Option<AuraVmConcurrencyEnvelopeError>,
    Option<AuraVmConcurrencyEnvelopeError>,
) {
    if policy.is_canonical_only() {
        let admission = AuraRuntimeEnvelopeAdmission::from_policy(protocol_id, policy, policy);
        (policy, admission, None, None)
    } else {
        let effective_policy = policy.canonical_fallback_policy();
        let admission =
            AuraRuntimeEnvelopeAdmission::from_policy(protocol_id, policy, effective_policy);
        (
            effective_policy,
            admission,
            Some(AuraVmConcurrencyEnvelopeError::AdmissionDenied {
                protocol_id: protocol_id.to_string(),
                requested_policy_ref: policy.policy_ref.to_string(),
                requested_profile: policy.concurrency_profile().as_ref().to_string(),
                reason: "host_bridge_canonical_only",
            }),
            Some(AuraVmConcurrencyEnvelopeError::CanonicalFallbackActivated {
                protocol_id: protocol_id.to_string(),
                requested_policy_ref: policy.policy_ref.to_string(),
                effective_policy_ref: effective_policy.policy_ref.to_string(),
                reason: "host_bridge_canonical_only",
            }),
        )
    }
}

fn log_concurrency_profile_selection(admission: &AuraRuntimeEnvelopeAdmission) {
    tracing::debug!(
        event = RuntimeVmEvent::ConcurrencyProfileSelected.as_event_name(),
        protocol_id = admission.protocol_id,
        requested_policy_ref = admission.requested_policy_ref,
        requested_profile = admission.requested_profile.as_ref(),
        requested_runtime_mode = admission.requested_runtime_mode.as_ref(),
        effective_policy_ref = admission.effective_policy_ref,
        effective_profile = admission.effective_profile.as_ref(),
        effective_runtime_mode = admission.effective_runtime_mode.as_ref(),
        evidence = ?admission.evidence,
        canonical_fallback = admission.activated_fallback(),
        "Selected runtime concurrency profile for VM session"
    );
}

fn log_concurrency_profile_fallback(
    admission: &AuraRuntimeEnvelopeAdmission,
    error: &AuraVmConcurrencyEnvelopeError,
) {
    tracing::warn!(
        event = RuntimeVmEvent::ConcurrencyProfileFallback.as_event_name(),
        protocol_id = admission.protocol_id,
        requested_policy_ref = admission.requested_policy_ref,
        requested_profile = admission.requested_profile.as_ref(),
        requested_runtime_mode = admission.requested_runtime_mode.as_ref(),
        effective_policy_ref = admission.effective_policy_ref,
        effective_profile = admission.effective_profile.as_ref(),
        effective_runtime_mode = admission.effective_runtime_mode.as_ref(),
        evidence = ?admission.evidence,
        error_kind = error.error_kind(),
        error = %error,
        "Fell back to canonical VM runtime profile"
    );
}

fn log_concurrency_profile_admission_failed(error: &AuraVmConcurrencyEnvelopeError) {
    tracing::warn!(
        event = RuntimeVmEvent::ConcurrencyProfileAdmissionFailed.as_event_name(),
        error_kind = error.error_kind(),
        error = %error,
        "Denied requested VM concurrency profile"
    );
}

pub fn build_role_scoped_code_image(
    roles: &[&str],
    active_role: &str,
    global_type: &GlobalType,
    local_types: &BTreeMap<String, LocalTypeR>,
) -> Result<CodeImage, String> {
    let mut scoped = BTreeMap::new();
    for role in roles {
        if *role == active_role {
            let local_type = local_types
                .get(*role)
                .cloned()
                .ok_or_else(|| format!("missing VM local type for active role {active_role}"))?;
            scoped.insert((*role).to_string(), local_type);
        } else {
            scoped.insert((*role).to_string(), LocalTypeR::End);
        }
    }
    Ok(CodeImage::from_local_types(&scoped, global_type))
}

fn build_role_scoped_protocol_machine_code_image(
    roles: &[&str],
    active_role: &str,
    global_type: &GlobalType,
    local_types: &BTreeMap<String, LocalTypeR>,
) -> Result<ProtocolMachineCodeImage, String> {
    let legacy_image = build_role_scoped_code_image(roles, active_role, global_type, local_types)?;
    let protocol_machine_global: telltale_types_v9::GlobalType =
        transcode_protocol_machine(&legacy_image.global_type, "role-scoped global type")?;
    let protocol_machine_locals: BTreeMap<String, telltale_types_v9::LocalTypeR> =
        transcode_protocol_machine(&legacy_image.local_types, "role-scoped local types")?;
    let image = ProtocolMachineCodeImage::from_local_types(
        &protocol_machine_locals,
        &protocol_machine_global,
    );
    image
        .validate_runtime_shape()
        .map_err(|reason| format!("invalid protocol-machine role-scoped image: {reason}"))?;
    Ok(image)
}

#[cfg(test)]
fn open_role_scoped_vm_session(
    role_names: &[&str],
    active_role: &str,
    global_type: &GlobalType,
    local_types: &BTreeMap<String, LocalTypeR>,
) -> Result<
    (
        AuraChoreoEngine<AuraQueuedVmBridgeHandler>,
        Arc<AuraQueuedVmBridgeHandler>,
        SessionId,
    ),
    String,
> {
    let image =
        build_role_scoped_protocol_machine_code_image(role_names, active_role, global_type, local_types)?;
    let handler = Arc::new(AuraQueuedVmBridgeHandler::default());
    let config = build_vm_config(
        AuraVmHardeningProfile::Prod,
        AuraVmParityProfile::RuntimeDefault,
    );
    let mut engine = AuraChoreoEngine::new_with_protocol_machine_contracts(
        config,
        Arc::clone(&handler),
        Some(RuntimeContracts::full()),
    )
    .map_err(|error| format!("failed to create VM engine: {error}"))?;
    let sid = engine
        .open_protocol_machine_session(&image)
        .map_err(|error| format!("failed to open VM session: {error}"))?;
    Ok((engine, handler, sid))
}

pub(in crate::runtime) async fn open_role_scoped_vm_session_admitted(
    role_names: &[&str],
    active_role: &str,
    global_type: &GlobalType,
    local_types: &BTreeMap<String, LocalTypeR>,
    protocol_id: &str,
    determinism_policy_ref: Option<&str>,
    scheduler_signals: AuraVmSchedulerSignals,
    required_capabilities: &[&str],
) -> Result<
    (
        AuraChoreoEngine<AuraQueuedVmBridgeHandler>,
        Arc<AuraQueuedVmBridgeHandler>,
        SessionId,
    ),
    AuraVmSessionOpenError,
> {
    let runtime_image = build_role_scoped_protocol_machine_code_image(
        role_names,
        active_role,
        global_type,
        local_types,
    )
        .map_err(|message| AuraVmSessionOpenError::RoleScopedImage {
            protocol_id: protocol_id.to_string(),
            message,
        })?;
    let handler = Arc::new(AuraQueuedVmBridgeHandler::default());
    handler.set_scheduler_signals(scheduler_signals);
    let mut config = build_vm_config(
        AuraVmHardeningProfile::Prod,
        AuraVmParityProfile::RuntimeDefault,
    );
    let requested_policy =
        policy_for_protocol(protocol_id, determinism_policy_ref).map_err(|source| {
            AuraVmSessionOpenError::PolicyResolution {
                protocol_id: protocol_id.to_string(),
                source,
            }
        })?;
    let (effective_policy, admission, admission_denied, fallback_activated) =
        effective_host_bridge_policy(protocol_id, requested_policy);
    if let Some(error) = admission_denied.as_ref() {
        log_concurrency_profile_admission_failed(error);
    }
    if let Some(error) = fallback_activated.as_ref() {
        log_concurrency_profile_fallback(&admission, error);
    }
    log_concurrency_profile_selection(&admission);
    apply_protocol_execution_policy(&mut config, effective_policy);
    let scheduler_input = scheduler_control_input_for_protocol_machine_image(
        &runtime_image,
        effective_policy.protocol_class,
        configured_guard_capacity(&config),
        handler.scheduler_signals(),
    );
    let scheduler_policy = scheduler_policy_for_input(scheduler_input);
    apply_scheduler_execution_policy(&mut config, &scheduler_policy);
    let mut engine = AuraChoreoEngine::new_with_protocol_machine_contracts_and_selector(
        config,
        Arc::clone(&handler),
        Some(RuntimeContracts::full()),
        AuraVmRuntimeSelector::for_policy(effective_policy),
    )
    .map_err(|source| AuraVmSessionOpenError::EngineCreation {
        protocol_id: protocol_id.to_string(),
        policy_ref: effective_policy.policy_ref.to_string(),
        source,
    })?;
    let sid = engine
        .open_protocol_machine_session_for_policy_admitted(
            &runtime_image,
            effective_policy,
            required_capabilities,
        )
        .await
        .map_err(|source| AuraVmSessionOpenError::SessionOpen {
            protocol_id: protocol_id.to_string(),
            policy_ref: admission.effective_policy_ref().to_string(),
            source,
        })?;
    Ok((engine, handler, sid))
}

pub(in crate::runtime) async fn open_manifest_vm_session_admitted(
    effects: &AuraEffectSystem,
    manifest: &CompositionManifest,
    active_role: &str,
    global_type: &GlobalType,
    local_types: &BTreeMap<String, LocalTypeR>,
    scheduler_signals: AuraVmSchedulerSignals,
) -> Result<
    (
        AuraChoreoEngine<AuraQueuedVmBridgeHandler>,
        Arc<AuraQueuedVmBridgeHandler>,
        SessionId,
    ),
    AuraVmSessionOpenError,
> {
    manifest
        .validate_guard_capabilities(GuardCapabilityAdmission::first_party_only())
        .map_err(|error| AuraVmSessionOpenError::ManifestGuardCapability {
            protocol_id: manifest.protocol_id.clone(),
            message: error.to_string(),
        })?;
    let role_names = manifest
        .role_names
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let required_capabilities = manifest
        .required_capabilities
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let claimed_session_id = effects.current_runtime_choreography_session_id();
    let current_owner_label = effects
        .current_runtime_choreography_session_owner_label()
        .map_err(|message| AuraVmSessionOpenError::FragmentClaim {
            protocol_id: manifest.protocol_id.clone(),
            message,
        })?;
    let claimed_fragments = effects
        .claim_vm_fragments_for_manifest(current_owner_label, manifest)
        .map_err(|message| AuraVmSessionOpenError::FragmentClaim {
            protocol_id: manifest.protocol_id.clone(),
            message,
        })?;
    let open_result = open_role_scoped_vm_session_admitted(
        role_names.as_slice(),
        active_role,
        global_type,
        local_types,
        manifest.protocol_id.as_str(),
        manifest.determinism_policy_ref.as_deref(),
        scheduler_signals,
        required_capabilities.as_slice(),
    )
    .await;
    if open_result.is_err() && claimed_session_id.is_some() {
        let _ = effects.release_vm_fragments(&claimed_fragments);
    }
    open_result
}

pub async fn flush_pending_vm_sends(
    effects: &AuraEffectSystem,
    handler: &AuraQueuedVmBridgeHandler,
    peer_roles: &BTreeMap<String, ChoreographicRole>,
) -> Result<(), String> {
    for pending in handler.drain_pending_sends() {
        let peer_role = peer_roles.get(&pending.to_role).copied().ok_or_else(|| {
            format!(
                "missing peer mapping for VM send target role {}",
                pending.to_role
            )
        })?;
        effects
            .send_to_role_bytes(peer_role, pending.payload)
            .await
            .map_err(|error| {
                format!(
                    "failed to bridge VM send {}->{}:{}: {error}",
                    pending.from_role, pending.to_role, pending.label
                )
            })?;
    }
    Ok(())
}

pub async fn advance_host_bridged_vm_round(
    effects: &AuraEffectSystem,
    engine: &mut AuraChoreoEngine<AuraQueuedVmBridgeHandler>,
    handler: &AuraQueuedVmBridgeHandler,
    sid: SessionId,
    active_role: &str,
    peer_roles: &BTreeMap<String, ChoreographicRole>,
) -> Result<AuraVmBridgeRound, String> {
    let step = engine
        .step()
        .map_err(|error| format!("{active_role} VM step failed: {error}"))?;
    flush_pending_vm_sends(effects, handler, peer_roles).await?;
    let (blocked_receive, host_wait_status) =
        classify_blocked_receive(effects, engine.vm(), sid, active_role, peer_roles)
            .await
            .map_err(|error| format!("{active_role} VM receive failed: {error}"))?;
    Ok(AuraVmBridgeRound {
        step,
        blocked_receive,
        host_wait_status,
    })
}

pub async fn advance_host_bridged_vm_round_until_receive<F>(
    effects: &AuraEffectSystem,
    engine: &mut AuraChoreoEngine<AuraQueuedVmBridgeHandler>,
    handler: &AuraQueuedVmBridgeHandler,
    sid: SessionId,
    active_role: &str,
    peer_roles: &BTreeMap<String, ChoreographicRole>,
    stop_on_receive_error: F,
) -> Result<AuraVmBridgeRound, String>
where
    F: Fn(&ChoreographyError) -> bool,
{
    let step = engine
        .step()
        .map_err(|error| format!("{active_role} VM step failed: {error}"))?;
    flush_pending_vm_sends(effects, handler, peer_roles).await?;
    let (blocked_receive, host_wait_status) = match receive_blocked_vm_message(
        effects,
        engine.vm(),
        sid,
        active_role,
        peer_roles,
    )
    .await
    {
        Ok(blocked_receive) => (
            blocked_receive.clone(),
            if blocked_receive.is_some() {
                AuraVmHostWaitStatus::Delivered
            } else {
                AuraVmHostWaitStatus::Idle
            },
        ),
        Err(error) if stop_on_receive_error(&error) => (None, AuraVmHostWaitStatus::Deferred),
        Err(error) if is_receive_cancelled(&error) => (None, AuraVmHostWaitStatus::Cancelled),
        Err(error) if is_receive_timed_out(&error) => (None, AuraVmHostWaitStatus::TimedOut),
        Err(error) => return Err(format!("{active_role} VM receive failed: {error}")),
    };
    Ok(AuraVmBridgeRound {
        step,
        blocked_receive,
        host_wait_status,
    })
}

pub fn handle_standard_vm_round(
    engine: &mut AuraChoreoEngine<AuraQueuedVmBridgeHandler>,
    sid: SessionId,
    round: AuraVmBridgeRound,
    context_label: &str,
) -> Result<AuraVmRoundDisposition, String> {
    if let Some(blocked) = round.blocked_receive {
        inject_vm_receive(engine, sid, &blocked)?;
        return Ok(AuraVmRoundDisposition::Continue);
    }

    match round.host_wait_status {
        AuraVmHostWaitStatus::Idle | AuraVmHostWaitStatus::Delivered => {}
        AuraVmHostWaitStatus::TimedOut => {
            return Err(format!(
                "{context_label} timed out while waiting for receive"
            ));
        }
        AuraVmHostWaitStatus::Cancelled => {
            return Err(format!(
                "{context_label} cancelled while waiting for receive"
            ));
        }
        AuraVmHostWaitStatus::Deferred => {}
    }

    match round.step {
        ProtocolMachineStepResult::AllDone => Ok(AuraVmRoundDisposition::Complete),
        ProtocolMachineStepResult::Continue => Ok(AuraVmRoundDisposition::Continue),
        ProtocolMachineStepResult::Stuck => Err(format!(
            "{context_label} became stuck without a pending receive"
        )),
    }
}

async fn classify_blocked_receive(
    effects: &AuraEffectSystem,
    vm: &ProtocolMachine,
    sid: SessionId,
    active_role: &str,
    peer_roles: &BTreeMap<String, ChoreographicRole>,
) -> Result<(Option<BlockedVmReceive>, AuraVmHostWaitStatus), ChoreographyError> {
    match receive_blocked_vm_message(effects, vm, sid, active_role, peer_roles).await {
        Ok(blocked_receive) => Ok((
            blocked_receive.clone(),
            if blocked_receive.is_some() {
                AuraVmHostWaitStatus::Delivered
            } else {
                AuraVmHostWaitStatus::Idle
            },
        )),
        Err(error) if is_receive_timed_out(&error) => Ok((None, AuraVmHostWaitStatus::TimedOut)),
        Err(error) if is_receive_cancelled(&error) => Ok((None, AuraVmHostWaitStatus::Cancelled)),
        Err(error) => Err(error),
    }
}

fn is_receive_timed_out(error: &ChoreographyError) -> bool {
    matches!(
        error,
        ChoreographyError::Transport { source }
            if source
                .downcast_ref::<aura_core::effects::TransportError>()
                .is_some_and(|inner| matches!(inner, aura_core::effects::TransportError::NoMessage))
    )
}

fn is_receive_cancelled(error: &ChoreographyError) -> bool {
    matches!(error, ChoreographyError::SessionNotStarted)
        || matches!(
            error,
            ChoreographyError::InternalError { message }
                if message.contains("binding changed while waiting for receive")
        )
}

pub fn blocked_recv_edge(
    vm: &ProtocolMachine,
    sid: SessionId,
    role: &str,
) -> Option<(String, String)> {
    vm.coroutines().iter().find_map(|coro| {
        if coro.session_id != sid || coro.role != role {
            return None;
        }
        match &coro.status {
            ProtocolMachineCoroStatus::Blocked(ProtocolMachineBlockReason::Recv { edge, .. }) => {
                Some((edge.sender.clone(), edge.receiver.clone()))
            }
            _ => None,
        }
    })
}

pub async fn receive_blocked_vm_message(
    effects: &AuraEffectSystem,
    vm: &ProtocolMachine,
    sid: SessionId,
    active_role: &str,
    peer_roles: &BTreeMap<String, ChoreographicRole>,
) -> Result<Option<BlockedVmReceive>, ChoreographyError> {
    let Some((from_role, to_role)) = blocked_recv_edge(vm, sid, active_role) else {
        return Ok(None);
    };
    let peer_role = peer_roles
        .get(&from_role)
        .copied()
        .or_else(|| peer_roles.get(&to_role).copied())
        .ok_or_else(|| ChoreographyError::InternalError {
            message: format!("missing peer mapping for blocked VM edge {from_role}->{to_role}"),
        })?;
    let payload = effects.receive_from_role_bytes(peer_role).await?;
    Ok(Some(BlockedVmReceive {
        from_role,
        to_role,
        peer_role,
        payload,
    }))
}

pub fn inject_vm_receive(
    engine: &mut AuraChoreoEngine<AuraQueuedVmBridgeHandler>,
    sid: SessionId,
    receive: &BlockedVmReceive,
) -> Result<(), String> {
    engine
        .vm_mut()
        .inject_message(
            sid,
            &receive.from_role,
            &receive.to_role,
            AuraQueuedVmBridgeHandler::bytes_to_value(&receive.payload),
        )
        .map(|_| ())
        .map_err(|error| format!("failed to inject VM message: {error}"))
}

pub fn close_and_reap_vm_session(
    engine: &mut AuraChoreoEngine<AuraQueuedVmBridgeHandler>,
    sid: SessionId,
) -> Result<(), String> {
    let _ = collect_vm_session_artifacts(engine, sid);
    engine
        .close_session(sid)
        .map_err(|error| format!("failed to close VM session: {error}"))?;
    let _ = engine.vm_mut().reap_closed_sessions();
    Ok(())
}

pub fn collect_vm_session_artifacts(
    engine: &AuraChoreoEngine<AuraQueuedVmBridgeHandler>,
    sid: SessionId,
) -> Result<AuraVmSessionArtifactSnapshot, String> {
    if !engine.active_sessions().contains(&sid) {
        return Err(format!(
            "cannot collect artifacts for inactive VM session {sid}"
        ));
    }
    Ok(AuraVmSessionArtifactSnapshot {
        session_id: sid,
        determinism_profile: engine.session_determinism_profile_metadata(sid),
        requires_envelope_artifact: engine.session_requires_envelope_artifact(sid),
        effect_trace: engine.vm_effect_trace(),
        canonical_effect_trace: engine.canonical_vm_effect_trace(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::AgentConfig;
    use aura_core::AuthorityId;
    use aura_mpst::upstream::types::Label;
    use aura_mpst::CompositionLinkSpec;
    use aura_protocol::effects::{ChoreographicEffects, RoleIndex};
    use aura_testkit::stateful_effects::MockVmBridgeEffects;
    use std::sync::Arc;
    use telltale_machine::model::effects::EffectFailure;
    use telltale_machine::model::state::SessionStatus;
    use uuid::Uuid;

    fn authority_device_role(authority_id: AuthorityId, role_index: u16) -> ChoreographicRole {
        ChoreographicRole::for_authority(
            authority_id,
            RoleIndex::new(role_index.into()).expect("role index"),
        )
    }

    fn test_effects(authority_id: AuthorityId) -> Arc<AuraEffectSystem> {
        let authority_bytes = authority_id.to_bytes();
        let seed_salt = u64::from_le_bytes(authority_bytes[..8].try_into().expect("salt bytes"));
        Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority_with_salt(
                &AgentConfig::default(),
                authority_id,
                seed_salt,
            )
            .expect("testing effect system"),
        )
    }

    fn manifest_with_bundles(protocol_id: &str, bundle_ids: &[&str]) -> CompositionManifest {
        CompositionManifest {
            protocol_name: protocol_id.to_string(),
            protocol_namespace: None,
            protocol_qualified_name: protocol_id.to_string(),
            protocol_id: protocol_id.to_string(),
            role_names: vec!["Role".to_string()],
            required_capabilities: Vec::new(),
            guard_capabilities: Vec::new(),
            determinism_policy_ref: None,
            delegation_constraints: Vec::new(),
            link_specs: bundle_ids
                .iter()
                .map(|bundle_id| CompositionLinkSpec {
                    role: "Role".to_string(),
                    bundle_id: (*bundle_id).to_string(),
                    imports: Vec::new(),
                    exports: Vec::new(),
                })
                .collect(),
        }
    }

    #[test]
    fn role_scoped_image_keeps_stubbed_peer_edges() {
        let global = GlobalType::send("Sender", "Receiver", Label::new("msg"), GlobalType::End);
        let locals = BTreeMap::from([
            (
                "Sender".to_string(),
                LocalTypeR::send("Receiver", Label::new("msg"), LocalTypeR::End),
            ),
            (
                "Receiver".to_string(),
                LocalTypeR::recv("Sender", Label::new("msg"), LocalTypeR::End),
            ),
        ]);

        let image =
            build_role_scoped_code_image(&["Sender", "Receiver"], "Sender", &global, &locals)
                .expect("image");

        assert_eq!(
            image.roles(),
            vec!["Receiver".to_string(), "Sender".to_string()]
        );
        assert_eq!(image.local_types["Receiver"], LocalTypeR::End);
        assert!(matches!(
            image.local_types["Sender"],
            LocalTypeR::Send { .. }
        ));
    }

    #[test]
    fn queued_bridge_handler_surfaces_pending_send_payloads() {
        let handler = AuraQueuedVmBridgeHandler::default();
        handler.push_send_bytes(vec![0xaa, 0xbb]);

        let payload = handler
            .handle_send("Sender", "Receiver", "InvitationOffer", &[])
            .expect_success(|| EffectFailure::contract_violation("queued send should not block"))
            .expect("queued send");
        assert_eq!(
            payload,
            AuraQueuedVmBridgeHandler::bytes_to_value(&[0xaa, 0xbb])
        );

        let sends = handler.drain_pending_sends();
        assert_eq!(sends.len(), 1);
        assert_eq!(sends[0].label, "InvitationOffer");
        assert_eq!(sends[0].payload, vec![0xaa, 0xbb]);
    }

    #[test]
    fn queued_bridge_handler_respects_queued_choice() {
        let handler =
            AuraQueuedVmBridgeHandler::with_bridge_effects(Arc::new(MockVmBridgeEffects::new()));
        handler.push_choice_label("cancel");

        let choice = handler
            .handle_choose(
                "Initiator",
                "Guardian1",
                &["finalize".to_string(), "cancel".to_string()],
                &[],
            )
            .expect_success(|| EffectFailure::contract_violation("choice should not block"))
            .expect("choice");

        assert_eq!(choice, "cancel");
    }

    #[tokio::test]
    async fn flush_pending_vm_sends_reports_missing_peer_mapping_explicitly() {
        let authority_id = AuthorityId::from_uuid(Uuid::from_bytes([0x31; 16]));
        let effects = test_effects(authority_id);
        let session_id = Uuid::from_u128(0x3131);
        let roles = vec![
            authority_device_role(authority_id, 0),
            authority_device_role(AuthorityId::from_uuid(Uuid::from_bytes([0x32; 16])), 0),
        ];
        let handler = AuraQueuedVmBridgeHandler::default();

        effects
            .start_session(session_id, roles)
            .await
            .expect("session starts");
        handler.push_send_bytes(vec![0xAB]);
        handler
            .handle_send("Sender", "Receiver", "msg", &[])
            .expect_success(|| {
                EffectFailure::contract_violation("pending send should not block")
            })
            .expect("pending send recorded");

        let err = flush_pending_vm_sends(effects.as_ref(), &handler, &BTreeMap::new())
            .await
            .expect_err("missing peer mapping must fail flush");
        assert!(err.contains("missing peer mapping for VM send target role Receiver"));

        effects.end_session().await.expect("session ends");
    }

    #[tokio::test]
    async fn flush_pending_vm_sends_reports_teardown_during_pending_async_work() {
        let authority_id = AuthorityId::from_uuid(Uuid::from_bytes([0x33; 16]));
        let peer_authority = AuthorityId::from_uuid(Uuid::from_bytes([0x34; 16]));
        let effects = test_effects(authority_id);
        let session_id = Uuid::from_u128(0x3434);
        let peer_role = authority_device_role(peer_authority, 0);
        let roles = vec![authority_device_role(authority_id, 0), peer_role];
        let handler = AuraQueuedVmBridgeHandler::default();
        let peer_roles = BTreeMap::from([("Receiver".to_string(), peer_role)]);

        effects
            .start_session(session_id, roles)
            .await
            .expect("session starts");
        handler.push_send_bytes(vec![0xCD]);
        handler
            .handle_send("Sender", "Receiver", "msg", &[])
            .expect_success(|| {
                EffectFailure::contract_violation("pending send should not block")
            })
            .expect("pending send recorded");
        effects.end_session().await.expect("session ends");

        let err = flush_pending_vm_sends(effects.as_ref(), &handler, &peer_roles)
            .await
            .expect_err("teardown before flush must fail explicitly");
        assert!(err.contains("failed to bridge VM send Sender->Receiver:msg"));
    }

    #[test]
    fn close_and_reap_vm_session_removes_closed_session() {
        let global = GlobalType::End;
        let locals = BTreeMap::from([("Sender".to_string(), LocalTypeR::End)]);
        let (mut engine, _handler, sid) =
            open_role_scoped_vm_session(&["Sender"], "Sender", &global, &locals)
                .expect("session opens");

        assert!(engine.active_sessions().contains(&sid));
        close_and_reap_vm_session(&mut engine, sid).expect("session closes");
        assert!(!engine.active_sessions().contains(&sid));
        assert!(matches!(
            engine
                .vm()
                .sessions()
                .get(sid)
                .map(|session| &session.status),
            Some(SessionStatus::Closed)
        ));
    }

    #[test]
    fn collect_vm_session_artifacts_snapshots_traces_and_policy_metadata() {
        let global = GlobalType::send("Sender", "Receiver", Label::new("msg"), GlobalType::End);
        let locals = BTreeMap::from([
            (
                "Sender".to_string(),
                LocalTypeR::send("Receiver", Label::new("msg"), LocalTypeR::End),
            ),
            (
                "Receiver".to_string(),
                LocalTypeR::recv("Sender", Label::new("msg"), LocalTypeR::End),
            ),
        ]);
        let (mut engine, handler, sid) =
            open_role_scoped_vm_session(&["Sender", "Receiver"], "Sender", &global, &locals)
                .expect("session opens");
        handler.push_send_bytes(vec![0xAB]);
        engine.step().expect("vm step succeeds");

        let snapshot = collect_vm_session_artifacts(&engine, sid).expect("snapshot collects");
        assert_eq!(snapshot.session_id, sid);
        assert!(!snapshot.requires_envelope_artifact);
        assert_eq!(
            snapshot.effect_trace.len(),
            snapshot.canonical_effect_trace.len()
        );
    }

    #[tokio::test]
    async fn envelope_admitted_host_bridge_path_matches_canonical_cooperative_trace() {
        let global = GlobalType::send("Sender", "Receiver", Label::new("msg"), GlobalType::End);
        let locals = BTreeMap::from([
            (
                "Sender".to_string(),
                LocalTypeR::send("Receiver", Label::new("msg"), LocalTypeR::End),
            ),
            (
                "Receiver".to_string(),
                LocalTypeR::recv("Sender", Label::new("msg"), LocalTypeR::End),
            ),
        ]);

        let (mut admitted_engine, admitted_handler, admitted_sid) =
            open_role_scoped_vm_session_admitted(
                &["Sender", "Receiver"],
                "Sender",
                &global,
                &locals,
                "aura.sync.epoch_rotation",
                None,
                AuraVmSchedulerSignals::default(),
                &[],
            )
            .await
            .expect("envelope-admitted host bridge session opens");
        admitted_handler.push_send_bytes(vec![0xAB]);
        admitted_engine.step().expect("admitted step succeeds");
        let admitted_snapshot = collect_vm_session_artifacts(&admitted_engine, admitted_sid)
            .expect("admitted snapshot");

        let (mut canonical_engine, canonical_handler, canonical_sid) =
            open_role_scoped_vm_session(&["Sender", "Receiver"], "Sender", &global, &locals)
                .expect("canonical session opens");
        canonical_handler.push_send_bytes(vec![0xAB]);
        canonical_engine.step().expect("canonical step succeeds");
        let canonical_snapshot = collect_vm_session_artifacts(&canonical_engine, canonical_sid)
            .expect("canonical snapshot");

        assert_eq!(
            admitted_snapshot.effect_trace,
            canonical_snapshot.effect_trace
        );
        assert_eq!(
            admitted_snapshot.canonical_effect_trace,
            canonical_snapshot.canonical_effect_trace
        );
    }

    #[tokio::test]
    async fn linked_manifest_claims_fragment_ownership_and_releases_on_session_end() {
        let authority_id = AuthorityId::from_uuid(Uuid::from_bytes([0x41; 16]));
        let effects = test_effects(authority_id);
        let session_id = Uuid::from_u128(0xABCD);
        let roles = vec![authority_device_role(authority_id, 0)];
        let manifest = manifest_with_bundles("aura.invitation.exchange", &["bundle-a", "bundle-b"]);
        let global_type = GlobalType::End;
        let local_types = BTreeMap::from([("Role".to_string(), LocalTypeR::End)]);

        let owner = effects
            .start_owned_choreography_session("vm_host_bridge_test_owner", session_id, roles)
            .await
            .expect("owned session starts");

        let (mut engine, _handler, sid) = open_manifest_vm_session_admitted(
            effects.as_ref(),
            &manifest,
            "Role",
            &global_type,
            &local_types,
            AuraVmSchedulerSignals::default(),
        )
        .await
        .expect("vm session opens");

        let snapshot = effects.vm_fragment_snapshot();
        assert_eq!(snapshot.len(), 2);
        assert!(snapshot
            .iter()
            .any(|(fragment, _)| fragment.fragment_key == "bundle:bundle-a"));
        assert!(snapshot
            .iter()
            .any(|(fragment, _)| fragment.fragment_key == "bundle:bundle-b"));

        close_and_reap_vm_session(&mut engine, sid).expect("session closes");
        effects
            .end_owned_choreography_session(&owner)
            .await
            .expect("owned session ends");
        assert!(effects.vm_fragment_snapshot().is_empty());
    }

    #[tokio::test]
    async fn failed_manifest_open_releases_only_newly_claimed_fragments() {
        let authority_id = AuthorityId::from_uuid(Uuid::from_bytes([0x42; 16]));
        let effects = test_effects(authority_id);
        let session_id = Uuid::from_u128(0xABCE);
        let roles = vec![authority_device_role(authority_id, 0)];
        let existing_manifest = manifest_with_bundles("aura.existing.protocol", &["bundle-a"]);
        let failing_manifest = manifest_with_bundles("aura.failing.protocol", &["bundle-b"]);
        let global_type = GlobalType::End;

        let owner = effects
            .start_owned_choreography_session("vm_host_bridge_test_owner", session_id, roles)
            .await
            .expect("owned session starts");

        effects
            .claim_vm_fragments_for_manifest(owner.owner_label.clone(), &existing_manifest)
            .expect("preexisting fragment claim succeeds");

        let local_types = BTreeMap::new();
        let error = open_manifest_vm_session_admitted(
            effects.as_ref(),
            &failing_manifest,
            "Role",
            &global_type,
            &local_types,
            AuraVmSchedulerSignals::default(),
        )
        .await
        .expect_err("open should fail after claiming bundle-b");
        assert!(matches!(
            error,
            AuraVmSessionOpenError::RoleScopedImage { .. }
        ));

        let snapshot = effects.vm_fragment_snapshot();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].0.fragment_key, "bundle:bundle-a");

        effects
            .end_owned_choreography_session(&owner)
            .await
            .expect("owned session ends");
    }

    #[tokio::test]
    async fn host_bridge_falls_back_to_canonical_runtime_for_envelope_admitted_policy() {
        let authority_id = AuthorityId::from_uuid(Uuid::from_bytes([0x51; 16]));
        let effects = test_effects(authority_id);
        let session_id = Uuid::from_u128(0xBCDE);
        let roles = vec![authority_device_role(authority_id, 0)];
        let manifest = CompositionManifest {
            protocol_name: "aura-sync anti entropy".to_string(),
            protocol_namespace: None,
            protocol_qualified_name: "aura.sync.anti_entropy".to_string(),
            protocol_id: "aura.sync.anti_entropy".to_string(),
            role_names: vec!["Role".to_string()],
            required_capabilities: Vec::new(),
            guard_capabilities: Vec::new(),
            determinism_policy_ref: Some(
                crate::runtime::AURA_VM_POLICY_SYNC_ANTI_ENTROPY.to_string(),
            ),
            delegation_constraints: Vec::new(),
            link_specs: Vec::new(),
        };
        let global_type = GlobalType::End;
        let local_types = BTreeMap::from([("Role".to_string(), LocalTypeR::End)]);

        let owner = effects
            .start_owned_choreography_session("vm_host_bridge_test_owner", session_id, roles)
            .await
            .expect("owned session starts");

        let (mut engine, _handler, sid) = open_manifest_vm_session_admitted(
            effects.as_ref(),
            &manifest,
            "Role",
            &global_type,
            &local_types,
            AuraVmSchedulerSignals::default(),
        )
        .await
        .expect("vm session opens");

        let metadata = engine
            .session_determinism_profile_metadata(sid)
            .expect("determinism metadata recorded");
        assert_eq!(metadata.runtime_mode, "cooperative");
        assert_eq!(metadata.scheduler_envelope_class, "exact");
        assert!(!engine.session_requires_envelope_artifact(sid));
        assert_eq!(
            engine.session_runtime_selector(sid),
            Some(AuraVmRuntimeSelector::cooperative())
        );

        close_and_reap_vm_session(&mut engine, sid).expect("session closes");
        assert!(engine.session_determinism_profile_metadata(sid).is_none());
        assert!(!engine.session_requires_envelope_artifact(sid));
        assert!(engine.session_runtime_selector(sid).is_none());

        effects
            .end_owned_choreography_session(&owner)
            .await
            .expect("owned session ends");
    }
}
