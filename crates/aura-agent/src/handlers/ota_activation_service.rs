//! OTA Activation Service - Public API for OTA activation ceremonies
//!
//! Provides a runtime-facing API for OTA hard-fork activation ceremonies,
//! driving readiness collection through the generated Telltale OTA protocol
//! rather than a service-owned mutable executor state machine.

use crate::core::{AgentError, AgentResult, AuthorityContext};
use crate::handlers::shared::map_handler_time_read_error;
use crate::runtime::services::ceremony_runner::{
    CeremonyCommitMetadata, CeremonyInitRequest, CeremonyRunner,
};
use crate::runtime::{AuraEffectSystem, TaskSupervisor};
use async_trait::async_trait;
use aura_core::effects::PhysicalTimeEffects;
use aura_core::threshold::{AgreementMode, ParticipantIdentity};
use aura_core::types::identifiers::CeremonyId;
use aura_core::types::Epoch;
use aura_core::{AuraError, DeviceId, Hash32};
use aura_mpst::upstream::runtime::{
    ChoreoHandler, ChoreoHandlerExt, ChoreoResult, ChoreographyError as TelltaleChoreographyError,
    LabelId, RoleId,
};
use aura_mpst::GeneratedChoreographyRuntime;
use aura_sync::protocols::ota_ceremony::telltale_session_types_ota_activation::ota_activation::{
    runners::run_coordinator, OTAActivationProtocolRole as OtaActivationRole,
};
use aura_sync::protocols::ota_ceremony::{
    compute_ota_ceremony_prestate_hash, create_ota_activation_signature,
    emit_ota_ceremony_aborted_fact, emit_ota_ceremony_committed_fact,
    emit_ota_ceremony_initiated_fact, emit_ota_commitment_received_fact,
    emit_ota_threshold_reached_fact,
    telltale_session_types_ota_activation::BranchLabel as OtaBranchLabel, OTACeremonyAbort,
    OTACeremonyAbortReason, OTACeremonyCommit, OTACeremonyConfig, OTACeremonyId, OTACeremonyState,
    OTACeremonyStatus, OTAReadinessOutcome, OTAReadinessWitness, ReadinessCommitment,
    UpgradeProposal,
};
use serde::{de::DeserializeOwned, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, RwLock};

struct OtaActivationShared {
    ceremonies: RwLock<HashMap<OTACeremonyId, OtaProtocolSession>>,
}

struct OtaProtocolSession {
    command_tx: mpsc::UnboundedSender<OtaProtocolCommand>,
}

enum OtaProtocolCommand {
    Commitment {
        commitment: ReadinessCommitment,
        respond_to: oneshot::Sender<AgentResult<OTAReadinessOutcome>>,
    },
    Finalize {
        decision: OtaProtocolDecision,
        respond_to: OtaFinalizeResponder,
    },
}

#[derive(Clone)]
enum OtaProtocolDecision {
    Commit,
    Abort(OTACeremonyAbortReason),
}

enum OtaFinalizeResponder {
    Commit(oneshot::Sender<AgentResult<Epoch>>),
    Abort(oneshot::Sender<AgentResult<()>>),
}

enum OtaPendingOutbound {
    Proposal,
    FinalDecision,
}

struct OtaProtocolEndpoint;

struct OtaCoordinatorProtocolRuntime {
    effects: Arc<AuraEffectSystem>,
    ceremony_id: OTACeremonyId,
    state: Arc<RwLock<OTACeremonyState>>,
    devices: Vec<OtaActivationRole>,
    command_rx: mpsc::UnboundedReceiver<OtaProtocolCommand>,
    pending_decision: Option<OtaProtocolDecision>,
    finalize_responder: Option<OtaFinalizeResponder>,
    next_outbound: OtaPendingOutbound,
}

impl OtaCoordinatorProtocolRuntime {
    fn new(
        effects: Arc<AuraEffectSystem>,
        ceremony_id: OTACeremonyId,
        state: Arc<RwLock<OTACeremonyState>>,
        devices: Vec<OtaActivationRole>,
        command_rx: mpsc::UnboundedReceiver<OtaProtocolCommand>,
    ) -> Self {
        Self {
            effects,
            ceremony_id,
            state,
            devices,
            command_rx,
            pending_decision: None,
            finalize_responder: None,
            next_outbound: OtaPendingOutbound::Proposal,
        }
    }

    async fn run_until_terminal(mut self) -> AgentResult<()> {
        let run_result = run_coordinator(&mut self)
            .await
            .map_err(|error| AgentError::runtime(error.to_string()));
        match run_result {
            Ok(_) => self.finish_selected_decision().await,
            Err(error) => {
                self.fail_pending_finalization(&error);
                Err(error)
            }
        }
    }

    async fn finish_selected_decision(&mut self) -> AgentResult<()> {
        match self.pending_decision.clone() {
            Some(OtaProtocolDecision::Commit) => {
                let (activation_epoch, ready_devices, commitments) = {
                    let mut state = self.state.write().await;
                    state.status = OTACeremonyStatus::Committed;
                    state.agreement_mode = AgreementMode::ConsensusFinalized;
                    (
                        state.proposal.activation_epoch,
                        state.ready_devices(),
                        state.commitments.values().cloned().collect::<Vec<_>>(),
                    )
                };
                let threshold_signature =
                    create_ota_activation_signature(self.ceremony_id, &commitments)
                        .map_err(map_ota_error)?;
                emit_ota_ceremony_committed_fact(
                    &*self.effects,
                    self.ceremony_id,
                    activation_epoch,
                    &ready_devices,
                    &threshold_signature,
                )
                .await
                .map_err(map_ota_error)?;
                if let Some(OtaFinalizeResponder::Commit(responder)) =
                    self.finalize_responder.take()
                {
                    let _ = responder.send(Ok(activation_epoch));
                }
                Ok(())
            }
            Some(OtaProtocolDecision::Abort(reason)) => {
                let reason_string = reason.to_string();
                {
                    let mut state = self.state.write().await;
                    state.status = OTACeremonyStatus::Aborted {
                        reason: reason.clone(),
                    };
                }
                emit_ota_ceremony_aborted_fact(&*self.effects, self.ceremony_id, &reason_string)
                    .await
                    .map_err(map_ota_error)?;
                if let Some(OtaFinalizeResponder::Abort(responder)) = self.finalize_responder.take()
                {
                    let _ = responder.send(Ok(()));
                }
                Ok(())
            }
            None => Err(AgentError::runtime(
                "OTA protocol completed without a final decision".to_string(),
            )),
        }
    }

    fn fail_pending_finalization(&mut self, error: &AgentError) {
        if let Some(responder) = self.finalize_responder.take() {
            match responder {
                OtaFinalizeResponder::Commit(responder) => {
                    let _ = responder.send(Err(AgentError::runtime(error.to_string())));
                }
                OtaFinalizeResponder::Abort(responder) => {
                    let _ = responder.send(Err(AgentError::runtime(error.to_string())));
                }
            }
        }
    }

    async fn handle_commitment(
        &mut self,
        commitment: ReadinessCommitment,
    ) -> AgentResult<OTAReadinessOutcome> {
        let current_prestate = compute_ota_ceremony_prestate_hash(&*self.effects)
            .await
            .map_err(map_ota_error)?;
        let now = self
            .effects
            .physical_time()
            .await
            .map_err(map_handler_time_read_error)?
            .ts_ms;

        let (threshold_reached, ready_count, ready_devices) = {
            let mut state = self.state.write().await;

            if state.status != OTACeremonyStatus::CollectingCommitments {
                return Err(AgentError::runtime(format!(
                    "Ceremony not collecting commitments: {:?}",
                    state.status
                )));
            }

            if commitment.prestate_hash != current_prestate {
                return Err(AgentError::invalid(
                    "Prestate hash mismatch - state has changed since commitment was created",
                ));
            }

            if commitment.ceremony_id != self.ceremony_id {
                return Err(AgentError::invalid("Commitment is for different ceremony"));
            }

            if now > state.started_at_ms.saturating_add(state.timeout_ms) {
                let reason = OTACeremonyAbortReason::TimedOut;
                state.status = OTACeremonyStatus::Aborted {
                    reason: reason.clone(),
                };
                self.pending_decision = Some(OtaProtocolDecision::Abort(reason));
                return Ok(OTAReadinessOutcome::Collecting);
            }

            if state.commitments.contains_key(&commitment.device) {
                return Err(AgentError::invalid("Device has already committed"));
            }

            state
                .commitments
                .insert(commitment.device, commitment.clone());
            let threshold_reached = state.threshold_met();
            if threshold_reached {
                state.status = OTACeremonyStatus::AwaitingConsensus;
                state.agreement_mode = AgreementMode::CoordinatorSoftSafe;
            }

            (
                threshold_reached,
                state.ready_count(),
                state.ready_devices(),
            )
        };

        emit_ota_commitment_received_fact(&*self.effects, self.ceremony_id, &commitment)
            .await
            .map_err(map_ota_error)?;
        if threshold_reached {
            emit_ota_threshold_reached_fact(
                &*self.effects,
                self.ceremony_id,
                ready_count,
                &ready_devices,
            )
            .await
            .map_err(map_ota_error)?;
        }

        let threshold = self.state.read().await.threshold;
        Ok(if threshold_reached {
            OTAReadinessOutcome::ThresholdReached(OTAReadinessWitness {
                ceremony_id: self.ceremony_id,
                ready_devices,
                ready_count,
                threshold,
            })
        } else {
            OTAReadinessOutcome::Collecting
        })
    }

    async fn remaining_timeout_ms(&self) -> AgentResult<u64> {
        let now = self
            .effects
            .physical_time()
            .await
            .map_err(map_handler_time_read_error)?
            .ts_ms;
        let state = self.state.read().await;
        let deadline = state.started_at_ms.saturating_add(state.timeout_ms);
        Ok(deadline.saturating_sub(now))
    }

    async fn mark_timed_out(&mut self) {
        let mut state = self.state.write().await;
        let reason = OTACeremonyAbortReason::TimedOut;
        state.status = OTACeremonyStatus::Aborted {
            reason: reason.clone(),
        };
        self.pending_decision = Some(OtaProtocolDecision::Abort(reason));
    }

    async fn recv_command_until_timeout(&mut self) -> AgentResult<Option<OtaProtocolCommand>> {
        let remaining_ms = self.remaining_timeout_ms().await?;
        if remaining_ms == 0 {
            self.mark_timed_out().await;
            return Ok(None);
        }

        tokio::select! {
            command = self.command_rx.recv() => Ok(command),
            _ = self.effects.sleep_ms(remaining_ms) => {
                self.mark_timed_out().await;
                Ok(None)
            }
        }
    }

    async fn collect_commitments_for_threshold(&mut self) -> AgentResult<Vec<ReadinessCommitment>> {
        let mut collected = Vec::new();
        loop {
            if matches!(self.pending_decision, Some(OtaProtocolDecision::Abort(_))) {
                break;
            }

            let command = self.recv_command_until_timeout().await?;
            let Some(command) = command else {
                break;
            };
            match command {
                OtaProtocolCommand::Commitment {
                    commitment,
                    respond_to,
                } => {
                    let result = self.handle_commitment(commitment.clone()).await;
                    let threshold_reached =
                        matches!(result, Ok(OTAReadinessOutcome::ThresholdReached(_)));
                    if result.is_ok() {
                        collected.push(commitment);
                    }
                    let _ = respond_to.send(result);
                    if threshold_reached {
                        break;
                    }
                }
                OtaProtocolCommand::Finalize {
                    decision,
                    respond_to,
                } => match decision {
                    OtaProtocolDecision::Commit => {
                        let _ = respond_to.into_commit().send(Err(AgentError::invalid(
                            "Ceremony is not yet awaiting consensus",
                        )));
                    }
                    OtaProtocolDecision::Abort(reason) => {
                        self.pending_decision = Some(OtaProtocolDecision::Abort(reason));
                        self.finalize_responder = Some(respond_to);
                        break;
                    }
                },
            }
        }
        Ok(collected)
    }

    async fn choose_final_branch(&mut self) -> Result<OtaBranchLabel, TelltaleChoreographyError> {
        if let Some(label) = self.selected_branch_label() {
            return Ok(label);
        }

        loop {
            let command = self
                .recv_command_until_timeout()
                .await
                .map_err(map_runtime_error)?;
            let Some(command) = command else {
                return self.selected_branch_label().ok_or_else(|| {
                    TelltaleChoreographyError::ExecutionError(
                        "OTA ceremony timed out before final decision".to_string(),
                    )
                });
            };
            match command {
                OtaProtocolCommand::Commitment {
                    commitment,
                    respond_to,
                } => {
                    let result = self.handle_commitment(commitment).await;
                    let _ = respond_to.send(result);
                }
                OtaProtocolCommand::Finalize {
                    decision,
                    respond_to,
                } => match decision {
                    OtaProtocolDecision::Commit => {
                        let state = self.state.read().await;
                        if state.status != OTACeremonyStatus::AwaitingConsensus
                            || !state.threshold_met()
                        {
                            let _ = respond_to
                                .into_commit()
                                .send(Err(AgentError::invalid("Ceremony not awaiting consensus")));
                            continue;
                        }
                        drop(state);
                        self.pending_decision = Some(OtaProtocolDecision::Commit);
                        self.finalize_responder = Some(respond_to);
                        return Ok(OtaBranchLabel::Commit);
                    }
                    OtaProtocolDecision::Abort(reason) => {
                        self.pending_decision = Some(OtaProtocolDecision::Abort(reason));
                        self.finalize_responder = Some(respond_to);
                        return Ok(OtaBranchLabel::Abort);
                    }
                },
            }
        }
    }

    fn selected_branch_label(&self) -> Option<OtaBranchLabel> {
        match self.pending_decision {
            Some(OtaProtocolDecision::Commit) => Some(OtaBranchLabel::Commit),
            Some(OtaProtocolDecision::Abort(_)) => Some(OtaBranchLabel::Abort),
            None => None,
        }
    }

    async fn final_commit_payload(&self) -> AgentResult<OTACeremonyCommit> {
        let state = self.state.read().await;
        Ok(OTACeremonyCommit {
            ceremony_id: self.ceremony_id,
            activation_epoch: state.proposal.activation_epoch,
            ready_devices: state.ready_devices(),
        })
    }

    async fn final_abort_payload(&self) -> AgentResult<OTACeremonyAbort> {
        let reason = match self.pending_decision.clone() {
            Some(OtaProtocolDecision::Abort(reason)) => reason,
            _ => {
                return Err(AgentError::runtime(
                    "Abort payload requested without abort decision".to_string(),
                ));
            }
        };
        Ok(OTACeremonyAbort {
            ceremony_id: self.ceremony_id,
            reason,
        })
    }
}

#[async_trait]
impl ChoreoHandler for OtaCoordinatorProtocolRuntime {
    type Role = OtaActivationRole;
    type Endpoint = OtaProtocolEndpoint;

    async fn send<M: Serialize + Send + Sync>(
        &mut self,
        _ep: &mut Self::Endpoint,
        _to: Self::Role,
        _msg: &M,
    ) -> ChoreoResult<()> {
        Ok(())
    }

    async fn recv<M: DeserializeOwned + Send>(
        &mut self,
        _ep: &mut Self::Endpoint,
        _from: Self::Role,
    ) -> ChoreoResult<M> {
        Err(TelltaleChoreographyError::ExecutionError(
            "OTA coordinator runtime only supports threshold collection".to_string(),
        ))
    }

    async fn choose(
        &mut self,
        _ep: &mut Self::Endpoint,
        _who: Self::Role,
        _label: <Self::Role as RoleId>::Label,
    ) -> ChoreoResult<()> {
        Ok(())
    }

    async fn offer(
        &mut self,
        _ep: &mut Self::Endpoint,
        _from: Self::Role,
    ) -> ChoreoResult<<Self::Role as RoleId>::Label> {
        Err(TelltaleChoreographyError::ExecutionError(
            "OTA coordinator runtime does not receive branch selections".to_string(),
        ))
    }

    async fn with_timeout<F, T>(
        &mut self,
        _ep: &mut Self::Endpoint,
        _at: Self::Role,
        dur: Duration,
        body: F,
    ) -> ChoreoResult<T>
    where
        F: std::future::Future<Output = ChoreoResult<T>> + Send,
    {
        let remaining_ms = self
            .remaining_timeout_ms()
            .await
            .map_err(map_runtime_error)?;
        let enforced = dur.min(Duration::from_millis(remaining_ms.max(1)));
        tokio::select! {
            result = body => result,
            _ = self.effects.sleep_ms(enforced.as_millis().min(u128::from(u64::MAX)) as u64) => {
                Err(TelltaleChoreographyError::Timeout(enforced))
            }
        }
    }
}

#[async_trait]
impl ChoreoHandlerExt for OtaCoordinatorProtocolRuntime {
    async fn setup(&mut self, _role: Self::Role) -> ChoreoResult<Self::Endpoint> {
        Ok(OtaProtocolEndpoint)
    }

    async fn teardown(&mut self, _ep: Self::Endpoint) -> ChoreoResult<()> {
        Ok(())
    }
}

#[async_trait]
impl GeneratedChoreographyRuntime for OtaCoordinatorProtocolRuntime {
    async fn provide_message<M: Send + 'static>(&mut self, _to: Self::Role) -> ChoreoResult<M> {
        match self.next_outbound {
            OtaPendingOutbound::Proposal => {
                self.next_outbound = OtaPendingOutbound::FinalDecision;
                let proposal = self.state.read().await.proposal.clone();
                downcast_payload(proposal)
            }
            OtaPendingOutbound::FinalDecision => match self.pending_decision.clone() {
                Some(OtaProtocolDecision::Commit) => {
                    let payload = self
                        .final_commit_payload()
                        .await
                        .map_err(map_runtime_error)?;
                    downcast_payload(payload)
                }
                Some(OtaProtocolDecision::Abort(_)) => {
                    let payload = self
                        .final_abort_payload()
                        .await
                        .map_err(map_runtime_error)?;
                    downcast_payload(payload)
                }
                None => Err(TelltaleChoreographyError::ExecutionError(
                    "OTA final payload requested before a decision was selected".to_string(),
                )),
            },
        }
    }

    async fn select_branch<L: LabelId>(&mut self, choices: &[L]) -> ChoreoResult<L> {
        let label: OtaBranchLabel = self.choose_final_branch().await?;
        choices
            .iter()
            .copied()
            .find(|choice| choice.as_str() == label.as_str())
            .ok_or_else(|| {
                TelltaleChoreographyError::ExecutionError(
                    "Generated OTA branch set does not include the selected label".to_string(),
                )
            })
    }

    fn resolve_family(&self, family: &str) -> ChoreoResult<Vec<Self::Role>> {
        if family == "Devices" {
            return Ok(self.devices.clone());
        }
        Err(TelltaleChoreographyError::ExecutionError(format!(
            "Unknown OTA role family: {family}"
        )))
    }

    async fn collect<M: DeserializeOwned + Send>(
        &mut self,
        _ep: &mut Self::Endpoint,
        _from: &[Self::Role],
    ) -> ChoreoResult<Vec<M>> {
        let commitments = self
            .collect_commitments_for_threshold()
            .await
            .map_err(map_runtime_error)?;
        commitments
            .into_iter()
            .map(|commitment| {
                serde_json::to_vec(&commitment)
                    .map_err(|error| TelltaleChoreographyError::Serialization(error.to_string()))
                    .and_then(|bytes| {
                        serde_json::from_slice(&bytes).map_err(|error| {
                            TelltaleChoreographyError::Serialization(error.to_string())
                        })
                    })
            })
            .collect()
    }
}

impl OtaFinalizeResponder {
    fn into_commit(self) -> oneshot::Sender<AgentResult<Epoch>> {
        match self {
            OtaFinalizeResponder::Commit(responder) => responder,
            OtaFinalizeResponder::Abort(_) => {
                panic!("internal OTA finalization responder mismatch: expected commit");
            }
        }
    }
}

fn downcast_payload<M: Send + 'static, T: Send + 'static>(payload: T) -> ChoreoResult<M> {
    let boxed: Box<dyn std::any::Any + Send> = Box::new(payload);
    boxed.downcast::<M>().map(|payload| *payload).map_err(|_| {
        TelltaleChoreographyError::ExecutionError(format!(
            "OTA generated runner requested unexpected payload type {}",
            std::any::type_name::<M>()
        ))
    })
}

fn map_ota_error(error: AuraError) -> AgentError {
    AgentError::runtime(error.to_string())
}

fn map_runtime_error(error: AgentError) -> TelltaleChoreographyError {
    TelltaleChoreographyError::ExecutionError(error.to_string())
}

fn build_ota_ceremony_state(
    ceremony_id: OTACeremonyId,
    proposal: UpgradeProposal,
    config: &OTACeremonyConfig,
    started_at_ms: u64,
) -> OTACeremonyState {
    OTACeremonyState {
        ceremony_id,
        proposal,
        status: OTACeremonyStatus::CollectingCommitments,
        agreement_mode: AgreementMode::CoordinatorSoftSafe,
        commitments: HashMap::new(),
        threshold: config.threshold,
        quorum_size: config.quorum_size,
        started_at_ms,
        timeout_ms: config.timeout_ms,
    }
}

fn validate_ota_proposal(
    proposal: &UpgradeProposal,
    current_epoch: Epoch,
    config: &OTACeremonyConfig,
) -> AgentResult<()> {
    if proposal.kind != aura_sync::protocols::ota::UpgradeKind::HardFork {
        return Err(AgentError::invalid(
            "OTA ceremony only required for hard forks",
        ));
    }

    if proposal.activation_epoch.value()
        < current_epoch.value() + config.min_activation_notice_epochs
    {
        return Err(AgentError::invalid(format!(
            "Activation epoch {} too soon. Current: {}, minimum notice: {} epochs",
            proposal.activation_epoch, current_epoch, config.min_activation_notice_epochs
        )));
    }

    Ok(())
}

/// OTA activation ceremony service API.
#[derive(Clone)]
pub struct OtaActivationServiceApi {
    effects: Arc<AuraEffectSystem>,
    ceremony_runner: CeremonyRunner,
    authority_context: AuthorityContext,
    shared: Arc<OtaActivationShared>,
    config: OTACeremonyConfig,
    tasks: Arc<TaskSupervisor>,
}

impl OtaActivationServiceApi {
    /// Create a new OTA activation service with shared runtime-owned supervisors.
    pub fn new_with_runner(
        effects: Arc<AuraEffectSystem>,
        authority_context: AuthorityContext,
        ceremony_runner: CeremonyRunner,
        tasks: Arc<TaskSupervisor>,
    ) -> AgentResult<Self> {
        Self::new_with_runner_and_config(
            effects,
            authority_context,
            ceremony_runner,
            OTACeremonyConfig::default(),
            tasks,
        )
    }

    /// Create a new OTA activation service with a shared runner and explicit config.
    pub(crate) fn new_with_runner_and_config(
        effects: Arc<AuraEffectSystem>,
        authority_context: AuthorityContext,
        ceremony_runner: CeremonyRunner,
        config: OTACeremonyConfig,
        tasks: Arc<TaskSupervisor>,
    ) -> AgentResult<Self> {
        Ok(Self {
            effects,
            ceremony_runner,
            authority_context,
            shared: Arc::new(OtaActivationShared {
                ceremonies: RwLock::new(HashMap::new()),
            }),
            config,
            tasks,
        })
    }

    fn runner_ceremony_id(ceremony_id: OTACeremonyId) -> CeremonyId {
        CeremonyId::new(hex::encode(ceremony_id.0.as_bytes()))
    }

    async fn compute_prestate_hash(&self) -> AgentResult<Hash32> {
        compute_ota_ceremony_prestate_hash(&*self.effects)
            .await
            .map_err(map_ota_error)
    }

    fn spawn_coordinator_protocol(
        &self,
        ceremony_id: OTACeremonyId,
        state: Arc<RwLock<OTACeremonyState>>,
        command_rx: mpsc::UnboundedReceiver<OtaProtocolCommand>,
        participant_count: usize,
    ) {
        let effects = self.effects.clone();
        let tasks = self.tasks.group(format!(
            "ota_activation_service.{}",
            hex::encode(ceremony_id.0.as_bytes())
        ));
        let devices = (0..participant_count)
            .map(|index| OtaActivationRole::Devices(index as u32))
            .collect::<Vec<_>>();
        let fut = async move {
            let runtime = OtaCoordinatorProtocolRuntime::new(
                effects,
                ceremony_id,
                state,
                devices,
                command_rx,
            );
            if let Err(error) = runtime.run_until_terminal().await {
                tracing::error!(
                    ceremony_id = %hex::encode(ceremony_id.0.as_bytes()),
                    error = %error,
                    "OTA coordinator protocol session failed"
                );
            }
        };

        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                let _task_handle = tasks.spawn_local_named("coordinator", fut);
            } else {
                let _task_handle = tasks.spawn_named("coordinator", fut);
            }
        }
    }

    async fn session_command_tx(
        &self,
        ceremony_id: OTACeremonyId,
    ) -> AgentResult<mpsc::UnboundedSender<OtaProtocolCommand>> {
        self.shared
            .ceremonies
            .read()
            .await
            .get(&ceremony_id)
            .map(|session| session.command_tx.clone())
            .ok_or_else(|| AgentError::context("OTA ceremony not found"))
    }

    async fn remove_session(&self, ceremony_id: OTACeremonyId) {
        self.shared.ceremonies.write().await.remove(&ceremony_id);
    }

    /// Initiate a new OTA activation ceremony and register with the shared runner.
    pub async fn initiate_activation(
        &self,
        proposal: UpgradeProposal,
        current_epoch: Epoch,
        participants: Vec<DeviceId>,
        threshold_k: u16,
    ) -> AgentResult<OTACeremonyId> {
        let total_n = u16::try_from(participants.len()).map_err(|_| {
            AgentError::config("OTA ceremony participants exceed supported size".to_string())
        })?;
        if u32::from(threshold_k) != self.config.threshold
            || u32::from(total_n) != self.config.quorum_size
        {
            return Err(AgentError::config(format!(
                "OTA ceremony config mismatch: threshold {} of {} (configured {} of {})",
                threshold_k, total_n, self.config.threshold, self.config.quorum_size
            )));
        }

        validate_ota_proposal(&proposal, current_epoch, &self.config)?;

        let prestate_hash = self.compute_prestate_hash().await?;
        let started_at_ms = self
            .effects
            .physical_time()
            .await
            .map_err(map_handler_time_read_error)?
            .ts_ms;
        let ceremony_id =
            OTACeremonyId::new(&prestate_hash, &proposal.compute_hash(), started_at_ms);
        let state = Arc::new(RwLock::new(build_ota_ceremony_state(
            ceremony_id,
            proposal.clone(),
            &self.config,
            started_at_ms,
        )));

        emit_ota_ceremony_initiated_fact(&*self.effects, &self.config, ceremony_id, &proposal)
            .await
            .map_err(map_ota_error)?;

        let runner_id = Self::runner_ceremony_id(ceremony_id);
        let runner_participants = participants
            .iter()
            .copied()
            .map(ParticipantIdentity::device)
            .collect::<Vec<_>>();
        self.ceremony_runner
            .start(CeremonyInitRequest {
                ceremony_id: runner_id.clone(),
                kind: aura_app::runtime_bridge::CeremonyKind::OtaActivation,
                initiator_id: self.authority_context.authority_id(),
                threshold_k,
                total_n,
                participants: runner_participants,
                new_epoch: proposal.activation_epoch.value(),
                enrollment_device_id: None,
                enrollment_nickname_suggestion: None,
                prestate_hash,
            })
            .await
            .map_err(|error| {
                AgentError::internal(format!("Failed to register OTA ceremony: {error}"))
            })?;

        let (command_tx, command_rx) = mpsc::unbounded_channel();
        self.shared
            .ceremonies
            .write()
            .await
            .insert(ceremony_id, OtaProtocolSession { command_tx });
        self.spawn_coordinator_protocol(ceremony_id, state, command_rx, participants.len());
        Ok(ceremony_id)
    }

    /// Record a device readiness commitment and mirror acceptances in the ceremony runner.
    pub async fn record_commitment(
        &self,
        ceremony_id: OTACeremonyId,
        commitment: ReadinessCommitment,
    ) -> AgentResult<OTAReadinessOutcome> {
        let command_tx = self.session_command_tx(ceremony_id).await?;
        let (respond_to, recv) = oneshot::channel();
        command_tx
            .send(OtaProtocolCommand::Commitment {
                commitment: commitment.clone(),
                respond_to,
            })
            .map_err(|_| {
                AgentError::runtime("OTA ceremony session is no longer active".to_string())
            })?;
        let readiness = recv.await.map_err(|_| {
            AgentError::runtime("OTA ceremony session dropped response".to_string())
        })??;

        if commitment.ready {
            let runner_id = Self::runner_ceremony_id(ceremony_id);
            self.ceremony_runner
                .record_response(&runner_id, ParticipantIdentity::device(commitment.device))
                .await
                .map_err(|e| AgentError::internal(format!("Failed to record OTA response: {e}")))?;
        }

        Ok(readiness)
    }

    /// Commit the OTA activation ceremony and update the shared runner status.
    pub async fn commit_activation(&self, ceremony_id: OTACeremonyId) -> AgentResult<Epoch> {
        let command_tx = self.session_command_tx(ceremony_id).await?;
        let (respond_to, recv) = oneshot::channel();
        command_tx
            .send(OtaProtocolCommand::Finalize {
                decision: OtaProtocolDecision::Commit,
                respond_to: OtaFinalizeResponder::Commit(respond_to),
            })
            .map_err(|_| {
                AgentError::runtime("OTA ceremony session is no longer active".to_string())
            })?;
        let activation_epoch = recv.await.map_err(|_| {
            AgentError::runtime("OTA ceremony session dropped commit response".to_string())
        })??;

        let committed_at = self
            .effects
            .physical_time()
            .await
            .map_err(map_handler_time_read_error)?;

        let runner_id = Self::runner_ceremony_id(ceremony_id);
        self.ceremony_runner
            .commit(
                &runner_id,
                CeremonyCommitMetadata {
                    committed_at: Some(committed_at),
                    consensus_id: None,
                },
            )
            .await
            .map_err(|e| AgentError::internal(format!("Failed to record OTA commit: {e}")))?;

        self.remove_session(ceremony_id).await;
        Ok(activation_epoch)
    }

    /// Abort the OTA activation ceremony and update the shared runner status.
    pub async fn abort_activation(
        &self,
        ceremony_id: OTACeremonyId,
        reason: &str,
    ) -> AgentResult<()> {
        let command_tx = self.session_command_tx(ceremony_id).await?;
        let (respond_to, recv) = oneshot::channel();
        command_tx
            .send(OtaProtocolCommand::Finalize {
                decision: OtaProtocolDecision::Abort(OTACeremonyAbortReason::Manual {
                    reason: reason.to_string(),
                }),
                respond_to: OtaFinalizeResponder::Abort(respond_to),
            })
            .map_err(|_| {
                AgentError::runtime("OTA ceremony session is no longer active".to_string())
            })?;
        recv.await.map_err(|_| {
            AgentError::runtime("OTA ceremony session dropped abort response".to_string())
        })??;

        let runner_id = Self::runner_ceremony_id(ceremony_id);
        self.ceremony_runner
            .abort(&runner_id, Some(reason.to_string()))
            .await
            .map_err(|e| AgentError::internal(format!("Failed to record OTA abort: {e}")))?;

        self.remove_session(ceremony_id).await;
        Ok(())
    }
}
