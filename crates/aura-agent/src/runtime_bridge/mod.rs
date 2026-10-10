//! RuntimeBridge implementation for AuraAgent
//!
//! This module implements the `RuntimeBridge` trait from `aura-app` for `AuraAgent`,
//! enabling the dependency inversion where `aura-app` defines the trait and
//! `aura-agent` provides the implementation.

use crate::core::default_context_id_for_authority;
mod enrollment_issuance;
pub(crate) mod enrollment_quorum;
use crate::core::AuraAgent;
use crate::handlers::shared::context_commitment_from_journal;
use crate::runtime::consensus::build_consensus_params;
use crate::runtime::services::ceremony_runner::{CeremonyCommitMetadata, CeremonyInitRequest};
use crate::runtime::services::{RendezvousManager, SyncServiceManager};
use crate::runtime::transport_boundary::send_guarded_transport_envelope;
use async_trait::async_trait;
use aura_app::runtime_bridge::{
    AuthenticationStatus, AuthoritativeChannelBinding, AuthoritativeModerationStatus,
    BootstrapCandidateInfo, BridgeAuthorityInfo, BridgeDeviceInfo, CausalStampKey,
    CeremonyProcessingOutcome, DiscoveryTriggerOutcome, InvitationBridgeStatus, InvitationInfo,
    InvitationMutationOutcome, RendezvousStatus, RuntimeBridge, RuntimeBridgeError,
    SettingsBridgeState, SyncStatus,
};
use aura_app::signal_defs::{HOMES_SIGNAL, INVITATIONS_SIGNAL};
use aura_app::ui_contract::{
    AmpAccusationDiagnostic, AmpChannelTransitionSnapshot, AmpTransitionPolicySnapshot,
    AmpTransitionState, ChannelFactKey, MessageDropSnapshot, SupervisedTaskFailureSnapshot,
};
use aura_app::views::home::{HomeState, HomesState};
use aura_app::views::invitations::InvitationStatus;
use aura_app::IntentError;
use aura_app::ReactiveHandler;
use aura_chat::view::CanonicalChannelCreation;
use aura_chat::{ChatDelta, ChatFact, ChatViewReducer, CHAT_FACT_TYPE_ID};
use aura_composition::{downcast_delta_owned, ViewDeltaReducer};
use aura_core::ceremony::SupersessionReason;
use aura_core::effects::{
    amp::{
        AmpChannelEffects, AmpCiphertext, ChannelBootstrapPackage, ChannelCloseParams,
        ChannelCreateParams, ChannelJoinParams, ChannelLeaveParams, ChannelSendParams,
    },
    random::RandomCoreEffects,
    reactive::ReactiveEffects,
    time::PhysicalTimeEffects,
    transport::TransportReceipt,
    SecureStorageCapability, SecureStorageEffects, SecureStorageLocation, ThresholdSigningEffects,
    TransportEnvelope,
};
use aura_core::hash::hash;
use aura_core::threshold::ParticipantIdentity;
use aura_core::threshold::{AgreementMode, SigningContext, ThresholdConfig, ThresholdSignature};
use aura_core::tree::{AttestedOp, TreeOp};
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_core::types::{Epoch, FrostThreshold};
use aura_core::DeviceId;
use aura_core::FlowBudgetEffects;
use aura_core::FlowCost;
use aura_core::Hash32;
use aura_core::OwnedTaskSpawner;
use aura_core::Prestate;
use aura_core::Receipt;
use aura_core::{execute_with_timeout_budget, TimeoutBudget, TimeoutRunError};
use aura_journal::fact::FactContent;
use aura_journal::fact::{
    ChannelBootstrap, ChannelBumpReason, FactOptions, ProposedChannelEpochBump, RelationalFact,
};
use aura_journal::DomainFact;
use aura_journal::ProtocolRelationalFact;
use aura_protocol::amp::{
    commit_bump_with_consensus, emit_proposed_bump, AmpJournalEffects, ChannelParticipantEvent,
};
use aura_protocol::effects::TreeEffects;
use aura_social::moderation::facts::{HomePinFact, HomeUnpinFact};
use aura_social::moderation::{
    home_governance_causal, HomeBanFact, HomeGovernanceKey, HomeKickFact, HomeMuteFact,
    HomeUnbanFact, HomeUnmuteFact, TaggedHomeGovernanceEvent,
};

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::Duration;

mod amp;
mod consensus;
mod error_boundary;
mod identity;
mod invitation;
mod recovery;
mod rendezvous;
mod settings;
mod sibling_facts;
mod sync;

use amp::map_amp_error;
use consensus::{map_consensus_error, persist_consensus_dkg_transcript};
use error_boundary::{
    bridge_internal, bridge_network, bridge_runtime_internal, bridge_service_unavailable,
    bridge_service_unavailable_with_detail, bridge_validation_message,
};
use invitation::convert_invitation_to_bridge_info;

const CHAT_FACT_CONTENT_TYPE: &str = "application/aura-chat-fact";
// Fixed local bridge deadlines. These bound one bridge-owned stage or wire
// exchange and should stay code-defined unless they become true runtime policy
// knobs with external operators.
const INVITATION_BRIDGE_STAGE_TIMEOUT_MS: u64 = 8_000;
const AMP_REPAIR_MEMBERSHIP_STAGE_TIMEOUT_MS: u64 = 1_000;
// Harness-only convergence tuning. These affect local retry pacing for tests
// and debug sessions, not protocol meaning, so env overrides are acceptable.
const HARNESS_MODE_ENV_VAR: &str = "AURA_HARNESS_MODE";
const HARNESS_SYNC_ROUNDS_ENV_VAR: &str = "AURA_HARNESS_SYNC_ROUNDS";
const HARNESS_SYNC_BACKOFF_MS_ENV_VAR: &str = "AURA_HARNESS_SYNC_BACKOFF_MS";
const DEFAULT_HARNESS_SYNC_ROUNDS: usize = 3;
const DEFAULT_HARNESS_SYNC_BACKOFF_MS: u64 = 75;

fn transport_receipt_from_flow(receipt: Receipt) -> TransportReceipt {
    TransportReceipt {
        context: receipt.ctx,
        src: receipt.src,
        dst: receipt.dst,
        epoch: receipt.epoch.value(),
        cost: receipt.cost.value(),
        nonce: receipt.nonce.value(),
        prev: receipt.prev.0,
        sig: receipt.sig.into_bytes(),
    }
}

fn attach_chat_fact_test_receipt_if_needed(
    effects: &crate::runtime::AuraEffectSystem,
    envelope: &mut TransportEnvelope,
) {
    crate::runtime::receipt_model::attach_test_transport_receipt_if_needed(
        effects.is_testing(),
        envelope,
    );
}

fn descriptor_has_placeholder_crypto(descriptor: &aura_rendezvous::RendezvousDescriptor) -> bool {
    descriptor.public_key == [0u8; 32] || descriptor.handshake_psk_commitment == [0u8; 32]
}

async fn seed_authority_route_descriptor_if_needed(
    effects: &crate::runtime::AuraEffectSystem,
    local_authority: AuthorityId,
    peer: AuthorityId,
) {
    let Some(rendezvous) = effects.rendezvous_manager() else {
        return;
    };

    let authority_context = default_context_id_for_authority(peer);
    if rendezvous
        .get_descriptor(authority_context, peer)
        .await
        .is_some_and(|descriptor| !descriptor_has_placeholder_crypto(&descriptor))
    {
        return;
    }

    let local_context = default_context_id_for_authority(local_authority);
    let source_descriptor = if let Some(descriptor) = rendezvous
        .get_descriptor(local_context, peer)
        .await
        .filter(|descriptor| !descriptor_has_placeholder_crypto(descriptor))
    {
        Some(descriptor)
    } else if let Some(descriptor) = rendezvous
        .list_cached_descriptors_for_authority(peer)
        .await
        .into_iter()
        .find(|descriptor| !descriptor_has_placeholder_crypto(descriptor))
    {
        Some(descriptor)
    } else {
        rendezvous
            .get_lan_discovered_peer(peer)
            .await
            .map(|peer| peer.descriptor)
            .filter(|descriptor| !descriptor_has_placeholder_crypto(descriptor))
    };

    let Some(mut descriptor) = source_descriptor else {
        return;
    };

    descriptor.context_id = authority_context;
    let _ = rendezvous.cache_descriptor(descriptor).await;
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "secure_storage_bootstrap_read_write",
    capability_type = SecureStorageCapability,
    family = "runtime_helper"
)]
fn secure_storage_bootstrap_store_capabilities() -> [SecureStorageCapability; 2] {
    [
        SecureStorageCapability::Read,
        SecureStorageCapability::Write,
    ]
}

fn map_time_read_error(error: impl std::fmt::Display) -> IntentError {
    bridge_internal("Failed to read time", error)
}

fn map_tree_read_error(error: impl std::fmt::Display) -> IntentError {
    bridge_internal("Failed to read tree state", error)
}

fn map_serialization_error(label: &'static str, error: impl std::fmt::Display) -> IntentError {
    bridge_internal(label, error)
}

fn map_amp_state_error(error: impl std::fmt::Display) -> IntentError {
    bridge_internal("AMP state lookup failed", error)
}

fn map_amp_prestate_error(error: impl std::fmt::Display) -> IntentError {
    bridge_internal("Invalid AMP prestate", error)
}

fn map_amp_proposal_error(error: impl std::fmt::Display) -> IntentError {
    bridge_internal("AMP proposal failed", error)
}

fn map_amp_finalize_error(error: impl std::fmt::Display) -> IntentError {
    bridge_internal("AMP finalize failed", error)
}

fn is_generic_contact_invitation(invitation: &crate::handlers::invitation::Invitation) -> bool {
    invitation.sender_id == invitation.receiver_id
        && matches!(
            invitation.invitation_type,
            aura_invitation::InvitationType::Contact { .. }
        )
}

fn collect_authoritative_moderation_homes(
    homes: &HomesState,
    context_id: ContextId,
    channel_id: ChannelId,
) -> Vec<HomeState> {
    let mut candidates = Vec::new();

    if let Some(home) = homes.home_state(&channel_id) {
        if home.context_id == Some(context_id) {
            candidates.push(home.clone());
        }
    }

    for (_, home) in homes.iter() {
        if home.context_id == Some(context_id)
            && !candidates
                .iter()
                .any(|candidate: &HomeState| candidate.id == home.id)
        {
            candidates.push(home.clone());
        }
    }

    if let Some(home) = homes.current_home() {
        if !candidates
            .iter()
            .any(|candidate: &HomeState| candidate.id == home.id)
        {
            candidates.push(home.clone());
        }
    }

    candidates
}

async fn execute_with_effect_timeout<TTime, T, E, F, Fut>(
    time: &TTime,
    timeout: Duration,
    operation: F,
) -> Result<T, TimeoutRunError<E>>
where
    TTime: PhysicalTimeEffects + Sync,
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
{
    let started_at = time.physical_time().await.map_err(|error| {
        TimeoutRunError::Timeout(aura_core::TimeoutBudgetError::time_source_failure(error))
    })?;
    let budget = TimeoutBudget::from_start_and_timeout(&started_at, timeout)
        .map_err(TimeoutRunError::Timeout)?;
    execute_with_timeout_budget(time, &budget, operation).await
}

fn amp_transition_snapshot(
    channel: ChannelId,
    stable_epoch: u64,
    transition: &aura_journal::reduction::AmpTransitionReduction,
) -> AmpChannelTransitionSnapshot {
    let conflict_evidence = transition
        .conflict_evidence_ids
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let cooldown_until_generation = if transition.conflict_evidence_ids.is_empty() {
        None
    } else {
        Some(stable_epoch.saturating_add(1))
    };
    let accusation_history = transition
        .conflict_evidence_ids
        .iter()
        .map(|evidence_id| AmpAccusationDiagnostic {
            evidence_id: evidence_id.to_string(),
            witness: None,
            cooldown_until_generation,
        })
        .collect::<Vec<_>>();
    let emergency_policy = transition
        .quarantine_epochs
        .iter()
        .next()
        .map(|_| AmpTransitionPolicySnapshot::EmergencyQuarantine)
        .or_else(|| {
            (!transition.prune_before_epochs.is_empty())
                .then_some(AmpTransitionPolicySnapshot::EmergencyCryptoshred)
        });

    AmpChannelTransitionSnapshot {
        channel: ChannelFactKey::identified(channel.to_string()),
        stable_epoch,
        state: amp_transition_state(transition.status),
        live_transition_id: transition.live_transition_id.map(|id| id.to_string()),
        finalized_transition_id: transition.finalized_transition_id.map(|id| id.to_string()),
        conflict_evidence,
        emergency_policy,
        suspect_authorities: transition
            .emergency_suspects
            .iter()
            .map(ToString::to_string)
            .collect(),
        quarantine_epochs: transition.quarantine_epochs.iter().copied().collect(),
        prune_before_epochs: transition.prune_before_epochs.iter().copied().collect(),
        cryptoshred_active: !transition.prune_before_epochs.is_empty(),
        accusation_history,
    }
}

fn amp_transition_state(
    status: aura_journal::reduction::AmpTransitionReductionStatus,
) -> AmpTransitionState {
    match status {
        aura_journal::reduction::AmpTransitionReductionStatus::Observed => {
            AmpTransitionState::Observed
        }
        aura_journal::reduction::AmpTransitionReductionStatus::A2Live => AmpTransitionState::A2Live,
        aura_journal::reduction::AmpTransitionReductionStatus::A2Conflict => {
            AmpTransitionState::A2Conflict
        }
        aura_journal::reduction::AmpTransitionReductionStatus::A3Finalized => {
            AmpTransitionState::A3Finalized
        }
        aura_journal::reduction::AmpTransitionReductionStatus::A3Conflict => {
            AmpTransitionState::A3Conflict
        }
        aura_journal::reduction::AmpTransitionReductionStatus::Aborted => {
            AmpTransitionState::Aborted
        }
        aura_journal::reduction::AmpTransitionReductionStatus::Superseded => {
            AmpTransitionState::Superseded
        }
    }
}

async fn resolve_channel_ids_from_local_chat_facts(
    effects: &crate::runtime::AuraEffectSystem,
    authority: AuthorityId,
    channel_name: &str,
) -> Result<Vec<ChannelId>, RuntimeBridgeError> {
    let normalized = channel_name.trim().to_ascii_lowercase();
    let facts = effects
        .load_committed_facts(authority)
        .await
        .map_err(|error| map_amp_error(aura_core::effects::amp::AmpChannelError::Effect(error)))?;

    let mut chat_facts = Vec::new();
    for fact in facts.into_iter().rev() {
        let FactContent::Relational(RelationalFact::Generic {
            context_id,
            envelope,
        }) = fact.content
        else {
            continue;
        };
        if envelope.type_id.as_str() != CHAT_FACT_TYPE_ID {
            continue;
        }
        let decoded = decode_required_name_chat_fact(context_id, &envelope)?;
        chat_facts.push(decoded);
    }
    Ok(resolve_created_channel_ids_by_name(chat_facts, &normalized))
}

fn decode_required_name_chat_fact(
    context: ContextId,
    envelope: &aura_core::types::facts::FactEnvelope,
) -> Result<ChatFact, RuntimeBridgeError> {
    let decoded = ChatFact::try_from_envelope(envelope)
        .map_err(|error| map_amp_error(aura_core::effects::amp::AmpChannelError::Effect(error)))?;
    if decoded.context_id() != context {
        let error = aura_core::types::facts::FactError::InvalidEnvelope(
            "committed chat envelope and payload contexts disagree".to_string(),
        );
        return Err(map_amp_error(
            aura_core::effects::amp::AmpChannelError::Effect(aura_core::AuraError::Invalid {
                message: error.to_string(),
                source: Some(Arc::new(error)),
            }),
        ));
    }
    Ok(decoded)
}

async fn committed_channel_creation_contexts(
    effects: &crate::runtime::AuraEffectSystem,
    authority: AuthorityId,
    channel: ChannelId,
) -> Result<Vec<ContextId>, RuntimeBridgeError> {
    let facts = effects
        .load_committed_facts(authority)
        .await
        .map_err(|error| map_amp_error(aura_core::effects::amp::AmpChannelError::Effect(error)))?;
    let mut contexts = Vec::new();
    for fact in facts {
        let FactContent::Relational(RelationalFact::Generic {
            context_id,
            envelope,
        }) = fact.content
        else {
            continue;
        };
        if envelope.type_id.as_str() != CHAT_FACT_TYPE_ID {
            continue;
        }
        if let ChatFact::ChannelCreated { channel_id, .. } =
            decode_required_name_chat_fact(context_id, &envelope)?
        {
            if channel_id == channel && !contexts.contains(&context_id) {
                contexts.push(context_id);
            }
        }
    }
    Ok(contexts)
}

fn resolve_created_channel_ids_by_name(
    facts_newest_first: impl IntoIterator<Item = ChatFact>,
    normalized: &str,
) -> Vec<ChannelId> {
    let mut latest_names = HashMap::new();
    let mut creations = HashMap::new();
    for fact in facts_newest_first {
        match fact {
            ChatFact::ChannelUpdated {
                context_id,
                channel_id,
                name: Some(name),
                ..
            } => {
                latest_names.entry(channel_id).or_insert((context_id, name));
            }
            ChatFact::ChannelCreated {
                context_id,
                channel_id,
                name,
                ..
            } => {
                creations.entry(channel_id).or_insert((context_id, name));
            }
            _ => {}
        }
    }

    let mut resolved = Vec::new();
    for (channel_id, (context_id, created_name)) in creations {
        let name = latest_names
            .get(&channel_id)
            .filter(|(seen_context, _)| *seen_context == context_id)
            .map(|(_, name)| name)
            .unwrap_or(&created_name);
        if name.trim().eq_ignore_ascii_case(normalized) {
            resolved.push(channel_id);
        }
    }
    resolved.sort();
    resolved
}

fn service_unavailable(service: &'static str) -> IntentError {
    bridge_service_unavailable(service)
}

fn service_unavailable_with_detail(
    service: &'static str,
    detail: impl std::fmt::Display,
) -> IntentError {
    bridge_service_unavailable_with_detail(service, detail)
}

fn require_sync_service(bridge: &AgentRuntimeBridge) -> Result<&SyncServiceManager, IntentError> {
    bridge
        .agent
        .runtime()
        .sync()
        .ok_or_else(|| service_unavailable("sync_service"))
}

fn require_rendezvous_service(
    bridge: &AgentRuntimeBridge,
) -> Result<&RendezvousManager, IntentError> {
    bridge
        .agent
        .runtime()
        .rendezvous()
        .ok_or_else(|| service_unavailable("rendezvous_service"))
}

fn harness_mode_enabled() -> bool {
    std::env::var_os(HARNESS_MODE_ENV_VAR).is_some()
}

#[cfg(test)]
pub(crate) fn harness_mode_env_key_for_tests() -> &'static str {
    HARNESS_MODE_ENV_VAR
}

/// Sync rounds after ceremony processing: harness runs retry to settle
/// reachability quickly; normal runs make one pass.
fn reachability_refresh_rounds() -> usize {
    if harness_mode_enabled() {
        harness_sync_rounds()
    } else {
        1
    }
}

fn harness_sync_rounds() -> usize {
    std::env::var(HARNESS_SYNC_ROUNDS_ENV_VAR)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|rounds| *rounds > 0)
        .unwrap_or(DEFAULT_HARNESS_SYNC_ROUNDS)
}

fn harness_sync_backoff_ms() -> u64 {
    std::env::var(HARNESS_SYNC_BACKOFF_MS_ENV_VAR)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(DEFAULT_HARNESS_SYNC_BACKOFF_MS)
}

/// Wrapper to implement RuntimeBridge for AuraAgent
///
/// This struct wraps an Arc<AuraAgent> to provide the RuntimeBridge implementation.
/// It handles the translation between the abstract RuntimeBridge interface and
/// the concrete AuraAgent services.
pub struct AgentRuntimeBridge {
    agent: Arc<AuraAgent>,
}

impl AgentRuntimeBridge {
    /// Create a new runtime bridge from an AuraAgent
    pub fn new(agent: Arc<AuraAgent>) -> Self {
        Self { agent }
    }

    pub(super) async fn seed_sync_peers_from_rendezvous(&self) {
        if let (Some(sync), Some(rendezvous)) = (
            self.agent.runtime().sync(),
            self.agent.runtime().rendezvous(),
        ) {
            let local_device = self.agent.runtime().effects().device_id();
            for peer_device in rendezvous
                .list_reachable_sibling_devices(local_device)
                .await
            {
                sync.add_peer(peer_device).await;
            }
        }
    }

    pub(super) async fn sync_seeded_peers(&self) -> Result<(), IntentError> {
        let Some(sync) = self.agent.runtime().sync() else {
            return Err(service_unavailable("sync_service"));
        };
        let peers = sync.peers().await;
        if peers.is_empty() {
            return Err(bridge_validation_message(
                "No sync peers are available for synchronization",
            ));
        }
        let effects = self.agent.runtime().effects();
        sync::sync_with_peer_list(sync, &effects, peers)
            .await
            .map_err(|e| bridge_internal("Sync failed", e))
    }

    pub(super) async fn refresh_reachability_after_ceremony_processing(
        &self,
    ) -> Result<(), RuntimeBridgeError> {
        let rounds = reachability_refresh_rounds();
        let backoff_ms = harness_sync_backoff_ms();
        let mut last_error = None;

        for round in 0..rounds {
            if harness_mode_enabled() {
                let _ = rendezvous::trigger_discovery(self).await;
            }
            self.seed_sync_peers_from_rendezvous().await;
            match self.sync_seeded_peers().await {
                Ok(()) => return Ok(()),
                Err(error) => last_error = Some(error),
            }
            if round + 1 < rounds && harness_mode_enabled() && backoff_ms > 0 {
                self.sleep_ms(backoff_ms).await?;
            }
        }

        match last_error {
            Some(error) => Err(error.into()),
            None => Ok(()),
        }
    }

    #[allow(dead_code)] // Kept only for explicit fail-closed regression tests.
    pub(super) async fn pull_remote_relational_facts(
        &self,
        peer: AuthorityId,
    ) -> Result<usize, IntentError> {
        tracing::info!(peer = %peer, "pull_remote_relational_facts start");
        let _ = peer;
        Err(bridge_validation_message(
            "Direct LAN relational fact pull has been removed; use authenticated sync services instead",
        ))
    }
}

#[cfg(target_arch = "wasm32")]
#[allow(dead_code)] // Retained as the wasm-local bridge helper for future websocket callsites.
async fn run_local_ws<Mk, Fut, T>(make_fut: Mk) -> Result<T, IntentError>
where
    Mk: FnOnce() -> Fut + 'static,
    Fut: core::future::Future<Output = Result<T, IntentError>> + 'static,
    T: 'static,
{
    make_fut().await
}

/// Diagnostic projection of one supervised task failure (group, task, cause).
fn supervised_task_failure_snapshot(
    failure: &crate::runtime::TaskSupervisionError,
) -> SupervisedTaskFailureSnapshot {
    use crate::runtime::TaskSupervisionError as E;
    let (group, task, cause) = match failure {
        E::TaskFailed {
            group,
            task,
            source,
        } => (group, task.clone(), source.to_string()),
        E::Panicked { group, task } => (group, task.clone(), "panicked".to_string()),
        E::Cancelled { group, task } => (group, task.clone(), "cancelled".to_string()),
        E::AdmissionClosed { group, task } => (group, task.clone(), failure.to_string()),
        E::AdmissionLimit { group, .. }
        | E::Budget { group, .. }
        | E::Timeout { group, .. }
        | E::ForcedAbort { group, .. } => (group, String::new(), failure.to_string()),
    };
    SupervisedTaskFailureSnapshot {
        group: group.clone(),
        task,
        cause,
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl RuntimeBridge for AgentRuntimeBridge {
    // =========================================================================
    // Identity & Authority
    // =========================================================================

    fn authority_id(&self) -> AuthorityId {
        self.agent.authority_id()
    }

    fn reactive_handler(&self) -> ReactiveHandler {
        self.agent.runtime().effects().reactive_handler()
    }

    fn task_spawner(&self) -> OwnedTaskSpawner {
        self.agent.runtime().task_spawner()
    }

    fn supervised_task_failures(&self) -> Vec<SupervisedTaskFailureSnapshot> {
        self.agent
            .supervised_task_failures()
            .iter()
            .map(supervised_task_failure_snapshot)
            .collect()
    }

    fn message_drops(&self) -> Vec<MessageDropSnapshot> {
        let (drops, _) = self.agent.runtime().effects().message_drops();
        drops
            .into_iter()
            .map(|drop| MessageDropSnapshot {
                direction: drop.reason.direction(),
                context_id: drop.context_id.map(|id| id.to_string()),
                channel_id: drop.channel_id.map(|id| id.to_string()),
                peer_id: drop.peer_id.map(|id| id.to_string()),
                message_id: drop.message_id,
                reason: drop.reason.to_string(),
            })
            .collect()
    }

    fn record_outbound_message_delivery_failure(
        &self,
        failure: aura_app::runtime_bridge::OutboundMessageDeliveryFailure,
    ) {
        self.agent
            .runtime()
            .effects()
            .record_message_drop(crate::reactive::MessageDrop::outbound(failure));
    }

    // =========================================================================
    // Fact Persistence
    // =========================================================================

    async fn commit_relational_facts(&self, facts: &[RelationalFact]) -> Result<(), IntentError> {
        if facts.is_empty() {
            return Ok(());
        }

        let effects = self.agent.runtime().effects();
        effects
            .commit_relational_facts(facts.to_vec())
            .await
            .map_err(|e| bridge_internal("Commit facts failed", e))?;

        Ok(())
    }

    async fn commit_relational_facts_with_options(
        &self,
        facts: &[RelationalFact],
        options: FactOptions,
    ) -> Result<(), IntentError> {
        if facts.is_empty() {
            return Ok(());
        }

        let effects = self.agent.runtime().effects();
        effects
            .commit_relational_facts_with_options(facts.to_vec(), options)
            .await
            .map_err(|e| bridge_internal("Commit facts failed", e))?;

        Ok(())
    }

    async fn send_chat_fact(
        &self,
        peer: AuthorityId,
        context: ContextId,
        fact: &RelationalFact,
    ) -> Result<(), IntentError> {
        let payload = aura_core::util::serialization::to_vec(fact)
            .map_err(|e| bridge_internal("Serialize chat fact envelope failed", e))?;

        let mut metadata = std::collections::HashMap::new();
        metadata.insert(
            "content-type".to_string(),
            CHAT_FACT_CONTENT_TYPE.to_string(),
        );
        metadata.insert("target-authority-id".to_string(), peer.to_string());

        let effects = self.agent.runtime().effects();
        seed_authority_route_descriptor_if_needed(
            effects.as_ref(),
            self.agent.authority_id(),
            peer,
        )
        .await;
        let reachable_device_count = if let Some(rendezvous) = self.agent.runtime().rendezvous() {
            rendezvous
                .list_reachable_peer_devices_for_authority(peer)
                .await
                .len()
        } else {
            0
        };

        // Charged in every mode so deterministic runs exercise production
        // receipt issuance and envelope-route validation.
        let flow_receipt = Some(
            effects
                .charge_flow(
                    &default_context_id_for_authority(peer),
                    &peer,
                    FlowCost::new(1),
                )
                .await
                .map_err(|e| bridge_network("Charge chat fact flow failed", e))?,
        );

        let mut envelope = TransportEnvelope {
            destination: peer,
            source: self.agent.authority_id(),
            // Chat-fact payloads carry the authoritative channel context already.
            // Transport delivery should ride the established authority route so
            // remote fanout does not depend on a context-specific descriptor.
            context: default_context_id_for_authority(peer),
            payload,
            metadata,
            receipt: flow_receipt.map(transport_receipt_from_flow),
        };
        attach_chat_fact_test_receipt_if_needed(effects.as_ref(), &mut envelope);

        tracing::debug!(
            source = %self.agent.authority_id(),
            destination = %peer,
            context = %context,
            transport_context = %envelope.context,
            reachable_device_count,
            mode = "authority_route",
            "send-chat-fact"
        );

        send_guarded_transport_envelope(effects.as_ref(), envelope)
            .await
            .map_err(|e| bridge_network("Send chat fact failed", e))
    }

    // =========================================================================
    // AMP Channel Operations
    // =========================================================================

    async fn amp_create_channel(
        &self,
        params: ChannelCreateParams,
    ) -> Result<ChannelId, RuntimeBridgeError> {
        let effects = self.agent.runtime().effects();
        effects.create_channel(params).await.map_err(map_amp_error)
    }

    async fn amp_create_channel_bootstrap(
        &self,
        context: ContextId,
        channel: ChannelId,
        recipients: Vec<AuthorityId>,
    ) -> Result<Option<ChannelBootstrapPackage>, RuntimeBridgeError> {
        if recipients.is_empty() {
            return Err(bridge_validation_message("bootstrap recipients cannot be empty").into());
        }

        let effects = self.agent.runtime().effects();
        let _canonical = aura_protocol::amp::get_channel_state(&effects, context, channel)
            .await
            .map_err(|error| {
                map_amp_error(aura_core::effects::amp::AmpChannelError::Effect(error))
            })?;
        let journal = effects
            .fetch_context_journal(context)
            .await
            .map_err(|error| {
                map_amp_error(aura_core::effects::amp::AmpChannelError::Effect(error))
            })?;
        let mut existing_bootstrap = None;
        for fact in journal.iter_facts() {
            if let FactContent::Relational(RelationalFact::Protocol(
                ProtocolRelationalFact::AmpChannelBootstrap(bootstrap),
            )) = &fact.content
            {
                if bootstrap.context == context && bootstrap.channel == channel {
                    existing_bootstrap = Some(bootstrap.clone());
                }
            }
        }

        let mut requested_recipients = BTreeSet::new();
        for recipient in recipients {
            requested_recipients.insert(recipient);
        }

        // A later member never receives the existing epoch-0 key: it reads
        // only messages sent after it joins, under the epoch its join's key
        // ceremony starts (docs/112 §1.2.1).
        if existing_bootstrap.is_some() {
            return Ok(None);
        }

        let key_bytes = effects.random_bytes_32().await;
        let bootstrap_id = Hash32::from_bytes(&key_bytes);

        let location = SecureStorageLocation::amp_bootstrap_key(&context, &channel, &bootstrap_id);
        let store_capabilities = secure_storage_bootstrap_store_capabilities();
        effects
            .secure_store(&location, &key_bytes, &store_capabilities)
            .await
            .map_err(|e| {
                RuntimeBridgeError::with_source(
                    IntentError::storage_error("Store AMP bootstrap key failed"),
                    e,
                )
            })?;

        let now = effects.physical_time().await.map_err(|error| {
            RuntimeBridgeError::with_source(
                IntentError::service_error("Physical clock read failed"),
                error,
            )
        })?;

        let bootstrap_fact = ChannelBootstrap {
            context,
            channel,
            bootstrap_id,
            dealer: self.agent.authority_id(),
            recipients: requested_recipients.into_iter().collect(),
            created_at: now,
            expires_at: None,
        };

        effects
            .insert_relational_fact(RelationalFact::Protocol(
                aura_journal::ProtocolRelationalFact::AmpChannelBootstrap(bootstrap_fact),
            ))
            .await
            .map_err(|e| map_amp_error(aura_core::effects::amp::AmpChannelError::Effect(e)))?;

        Ok(Some(ChannelBootstrapPackage {
            bootstrap_id,
            key: key_bytes.to_vec(),
        }))
    }

    async fn amp_channel_state_exists(
        &self,
        context: ContextId,
        channel: ChannelId,
    ) -> Result<bool, RuntimeBridgeError> {
        let effects = self.agent.runtime().effects();
        match aura_protocol::amp::get_channel_state(&effects, context, channel).await {
            Ok(_) => Ok(true),
            Err(error) => {
                if aura_protocol::amp::ChannelStateUnavailable::find(&error).is_some_and(
                    |absence| absence.context() == context && absence.channel() == channel,
                ) {
                    return Ok(false);
                }
                Err(map_amp_error(
                    aura_core::effects::amp::AmpChannelError::Effect(error),
                ))
            }
        }
    }

    async fn amp_list_channel_participants(
        &self,
        context: ContextId,
        channel: ChannelId,
    ) -> Result<Vec<AuthorityId>, RuntimeBridgeError> {
        let invitation_service = self.agent.invitations().map_err(|e| {
            RuntimeBridgeError::with_source(
                IntentError::service_error("Invitation service unavailable"),
                e,
            )
        })?;
        invitation_service
            .channel_participants(context, channel)
            .await
            .map(|participants| participants.into_iter().collect())
            .map_err(|error| map_amp_error(aura_core::effects::amp::AmpChannelError::Effect(error)))
    }

    async fn amp_channel_transition_diagnostics(
        &self,
        context: ContextId,
        channel: ChannelId,
    ) -> Result<Option<AmpChannelTransitionSnapshot>, RuntimeBridgeError> {
        let effects = self.agent.runtime().effects();
        let state = aura_protocol::amp::get_reduced_channel_state(&effects, context, channel)
            .await
            .map_err(|error| {
                map_amp_error(aura_core::effects::amp::AmpChannelError::Effect(error))
            })?;
        Ok(state
            .transition
            .as_ref()
            .map(|transition| amp_transition_snapshot(channel, state.chan_epoch, transition)))
    }

    async fn resend_channel_invitation_acceptance_notifications(
        &self,
        _context: ContextId,
        channel: ChannelId,
    ) -> Result<(), IntentError> {
        let invitation_service = self.agent.invitations().map_err(|error| {
            IntentError::internal_error(format!(
                "failed to access invitation service for channel acceptance resend: {error}"
            ))
        })?;
        let effects = self.agent.runtime().effects();
        let local_authority = self.agent.authority_id();
        let handler = crate::handlers::invitation::InvitationHandler::new(
            crate::core::AuthorityContext::new_with_device(
                local_authority,
                self.agent.runtime().device_id(),
            ),
        )
        .map_err(|error| {
            IntentError::internal_error(format!(
                "failed to create invitation handler for acceptance resend: {error}"
            ))
        })?;

        for invitation in invitation_service.list_with_storage().await {
            if invitation.status != aura_invitation::InvitationStatus::Accepted {
                continue;
            }
            if invitation.sender_id == local_authority || invitation.receiver_id != local_authority
            {
                continue;
            }
            let aura_invitation::InvitationType::Channel { home_id, .. } =
                invitation.invitation_type
            else {
                continue;
            };
            if home_id != channel {
                continue;
            }
            let resend_result: Result<(), crate::core::AgentError> = handler
                .notify_channel_invitation_acceptance(effects.as_ref(), &invitation.invitation_id)
                .await;
            resend_result.map_err(|error| {
                IntentError::internal_error(format!(
                    "failed to resend channel invitation acceptance for {}: {error}",
                    invitation.invitation_id
                ))
            })?;
        }

        Ok(())
    }

    async fn moderation_status(
        &self,
        context_id: ContextId,
        channel_id: ChannelId,
        authority_id: AuthorityId,
        current_time_ms: u64,
    ) -> Result<AuthoritativeModerationStatus, aura_app::runtime_bridge::RuntimeBridgeError> {
        let committed_facts = self
            .agent
            .runtime()
            .effects()
            .load_committed_facts(self.agent.authority_id())
            .await
            .map_err(|error| {
                bridge_runtime_internal("Load committed moderation facts failed", error)
            })?;
        let (is_banned, is_muted) = aura_social::try_is_user_banned_and_muted(
            &committed_facts,
            &context_id,
            &authority_id,
            current_time_ms,
            Some(&channel_id),
        )
        .map_err(|error| {
            bridge_runtime_internal(
                "Decode required moderation facts failed",
                aura_core::AuraError::from(error),
            )
        })?;
        let homes: HomesState =
            self.reactive_handler()
                .read(&*HOMES_SIGNAL)
                .await
                .map_err(|error| {
                    bridge_runtime_internal(
                        "Read authoritative homes signal for moderation status failed",
                        error,
                    )
                    .with_kind(aura_app::runtime_bridge::RuntimeBridgeErrorKind::Reactive)
                })?;
        let candidates = collect_authoritative_moderation_homes(&homes, context_id, channel_id);
        let roster_known = candidates.iter().any(|home| !home.members.is_empty());
        let is_member = candidates
            .iter()
            .any(|home| home.member(&authority_id).is_some());

        Ok(AuthoritativeModerationStatus {
            is_banned,
            is_muted,
            roster_known,
            is_member,
        })
    }

    async fn causal_stamp(
        &self,
        key: CausalStampKey,
    ) -> Result<aura_core::time::CausalMetadata, IntentError> {
        let effects = self.agent.runtime().effects();
        let (context_id, key) = match key {
            CausalStampKey::HomeGovernance { context_id, key } => (context_id, key),
            CausalStampKey::Contact(key) => {
                return crate::handlers::shared::stamp_contact_causal(
                    &effects,
                    self.agent.authority_id(),
                    key,
                )
                .await
                .map_err(|error| bridge_internal("Stamp contact fact failed", error));
            }
            CausalStampKey::Friendship(key) => {
                return crate::handlers::shared::stamp_friendship_causal(
                    &effects,
                    self.agent.authority_id(),
                    key,
                )
                .await
                .map_err(|error| bridge_internal("Stamp friendship fact failed", error));
            }
            CausalStampKey::Chat(key) => {
                return crate::handlers::shared::stamp_message_revision_causal(
                    &effects,
                    self.agent.authority_id(),
                    key,
                )
                .await
                .map_err(|error| bridge_internal("Stamp chat revision failed", error));
            }
            CausalStampKey::InvitationOutcome(invitation_id) => {
                return crate::handlers::shared::stamp_invitation_outcome_causal(
                    &effects,
                    self.agent.authority_id(),
                    &invitation_id,
                )
                .await
                .map_err(|error| bridge_internal("Stamp invitation outcome failed", error));
            }
        };
        let committed = effects
            .load_committed_facts(self.agent.authority_id())
            .await
            .map_err(|error| bridge_internal("Load committed governance facts failed", error))?;
        let mut observed = Vec::new();
        for fact in &committed {
            let FactContent::Relational(RelationalFact::Generic {
                context_id: fact_context,
                envelope,
            }) = &fact.content
            else {
                continue;
            };
            if *fact_context != context_id {
                continue;
            }
            if let Some(event) = TaggedHomeGovernanceEvent::try_decode(*fact_context, envelope)
                .map_err(|error| {
                    bridge_internal(
                        "Decode committed governance fact failed",
                        aura_core::AuraError::from(error),
                    )
                })?
            {
                observed.push(event);
            }
        }
        crate::handlers::shared::stamp_after(&effects, &observed, |clock| {
            home_governance_causal(key, &observed, clock)
        })
        .await
        .map_err(|error| bridge_internal("Advance logical clock failed", error))
    }

    async fn canonical_channel_creation(
        &self,
        binding: AuthoritativeChannelBinding,
    ) -> Result<Option<CanonicalChannelCreation>, IntentError> {
        let facts = self
            .agent
            .runtime()
            .effects()
            .load_committed_facts(self.agent.authority_id())
            .await
            .map_err(|error| {
                bridge_internal("Load committed channel creation facts failed", error)
            })?;
        Ok(facts.into_iter().rev().find_map(|fact| {
            let FactContent::Relational(RelationalFact::Generic { envelope, .. }) = fact.content
            else {
                return None;
            };
            if envelope.type_id.as_str() != CHAT_FACT_TYPE_ID {
                return None;
            }
            ChatViewReducer
                .reduce_fact(CHAT_FACT_TYPE_ID, &envelope.payload, None)
                .into_iter()
                .filter_map(downcast_delta_owned::<ChatDelta>)
                .find_map(|delta| match delta {
                    ChatDelta::ChannelAdded(creation)
                        if creation.channel_id() == binding.channel_id
                            && creation.context_id() == binding.context_id =>
                    {
                        Some(creation)
                    }
                    _ => None,
                })
        }))
    }

    async fn resolve_amp_channel_context(
        &self,
        channel: ChannelId,
    ) -> Result<Option<ContextId>, RuntimeBridgeError> {
        let effects = self.agent.runtime().effects();
        let authority = self.agent.authority_id();

        let mut contexts = self
            .agent
            .runtime()
            .contexts()
            .list_contexts_for_authority(authority)
            .await
            .map_err(|error| {
                RuntimeBridgeError::with_source(
                    IntentError::service_error("Registered channel contexts unavailable"),
                    error,
                )
            })?;
        // A joined channel (e.g. an accepted home) may live in a context that
        // is not registered for this authority; its committed creation fact
        // names the context. Each candidate is still verified by AMP state.
        for context in committed_channel_creation_contexts(&effects, authority, channel).await? {
            if !contexts.contains(&context) {
                contexts.push(context);
            }
        }

        for context in contexts {
            match aura_protocol::amp::get_channel_state(&effects, context, channel).await {
                Ok(_) => return Ok(Some(context)),
                Err(error)
                    if aura_protocol::amp::ChannelStateUnavailable::find(&error).is_some_and(
                        |absence| absence.context() == context && absence.channel() == channel,
                    ) => {}
                Err(error) => {
                    return Err(map_amp_error(
                        aura_core::effects::amp::AmpChannelError::Effect(error),
                    ))
                }
            }
        }

        Ok(None)
    }

    async fn identify_materialized_channel_ids_by_name(
        &self,
        channel_name: &str,
    ) -> Result<Vec<ChannelId>, RuntimeBridgeError> {
        let effects = self.agent.runtime().effects();
        let authority = self.agent.authority_id();
        let mut resolved = BTreeSet::new();

        for channel_id in
            resolve_channel_ids_from_local_chat_facts(&effects, authority, channel_name).await?
        {
            if self
                .resolve_amp_channel_context(channel_id)
                .await?
                .is_some()
            {
                resolved.insert(channel_id);
            }
        }

        Ok(resolved.into_iter().collect())
    }

    async fn identify_materialized_channel_bindings_by_name(
        &self,
        channel_name: &str,
    ) -> Result<Vec<AuthoritativeChannelBinding>, RuntimeBridgeError> {
        let mut bindings = Vec::new();

        for channel_id in self
            .identify_materialized_channel_ids_by_name(channel_name)
            .await?
        {
            if let Some(context_id) = self.resolve_amp_channel_context(channel_id).await? {
                let binding = AuthoritativeChannelBinding {
                    channel_id,
                    context_id,
                };
                if !bindings.contains(&binding) {
                    bindings.push(binding);
                }
            }
        }

        Ok(bindings)
    }

    async fn amp_repair_local_channel_membership(
        &self,
        params: ChannelJoinParams,
    ) -> Result<(), RuntimeBridgeError> {
        let effects = self.agent.runtime().effects();
        let membership = execute_with_effect_timeout(
            &effects,
            Duration::from_millis(AMP_REPAIR_MEMBERSHIP_STAGE_TIMEOUT_MS),
            || async {
                aura_protocol::amp::journal::channel_membership_event(
                    &effects,
                    params.context,
                    params.channel,
                    params.participant,
                    ChannelParticipantEvent::Joined,
                    None,
                )
                .await
            },
        )
        .await
        .map_err(|error| match error {
            TimeoutRunError::Timeout(error) => amp::map_amp_budget_error(error),
            TimeoutRunError::Operation(error) => {
                map_amp_error(crate::runtime::effects::amp_membership_error(error))
            }
        })?;
        execute_with_effect_timeout(
            &effects,
            Duration::from_millis(AMP_REPAIR_MEMBERSHIP_STAGE_TIMEOUT_MS),
            || effects.insert_relational_fact(membership.to_generic()),
        )
        .await
        .map_err(|error| match error {
            TimeoutRunError::Timeout(error) => amp::map_amp_budget_error(error),
            TimeoutRunError::Operation(error) => {
                map_amp_error(aura_core::effects::amp::AmpChannelError::Effect(error))
            }
        })
    }

    async fn amp_close_channel(
        &self,
        params: ChannelCloseParams,
    ) -> Result<(), RuntimeBridgeError> {
        let effects = self.agent.runtime().effects();
        effects.close_channel(params).await.map_err(map_amp_error)
    }

    async fn amp_join_channel(&self, params: ChannelJoinParams) -> Result<(), RuntimeBridgeError> {
        let effects = self.agent.runtime().effects();
        effects.join_channel(params).await.map_err(map_amp_error)
    }

    async fn amp_leave_channel(
        &self,
        params: ChannelLeaveParams,
    ) -> Result<(), RuntimeBridgeError> {
        let effects = self.agent.runtime().effects();
        // Members to tell, read before the local leave removes the channel.
        let members = effects
            .reactive_handler()
            .read(&*aura_app::signal_defs::CHAT_SIGNAL)
            .await
            .ok()
            .and_then(|chat| chat.channel(&params.channel).map(|c| c.member_ids.clone()))
            .unwrap_or_default();
        let ChannelLeaveParams {
            context,
            channel,
            participant,
        } = params;
        let membership = effects
            .commit_channel_membership(
                context,
                channel,
                participant,
                ChannelParticipantEvent::Left,
                None,
            )
            .await
            .map_err(map_amp_error)?;

        // Best effort: other members drop us from their member lists.
        for member in members.into_iter().filter(|member| *member != participant) {
            if let Err(error) = self.send_chat_fact(member, context, &membership).await {
                tracing::debug!(%member, %error, "channel leave notification not sent");
            }
        }
        Ok(())
    }

    async fn bump_channel_epoch(
        &self,
        context: ContextId,
        channel: ChannelId,
        reason: String,
    ) -> Result<(), IntentError> {
        let effects = self.agent.runtime().effects();
        let authority_id = self.agent.authority_id();
        let state = aura_protocol::amp::get_channel_state(&effects, context, channel)
            .await
            .map_err(map_amp_state_error)?;
        let bump_nonce = effects.random_bytes(32).await;
        let bump_id = Hash32(hash(&bump_nonce));
        let proposal = ProposedChannelEpochBump::new(
            context,
            channel,
            state.chan_epoch,
            state.chan_epoch + 1,
            bump_id,
            ChannelBumpReason::Routine,
        );

        emit_proposed_bump(effects.as_ref(), proposal.clone())
            .await
            .map_err(map_amp_proposal_error)?;

        let policy =
            aura_core::threshold::policy_for(aura_core::threshold::CeremonyFlow::AmpEpochBump);
        let consensus_required = crate::runtime::consensus::consensus_required_for_authority(
            effects.as_ref(),
            authority_id,
        )
        .await;
        if policy.allows_mode(AgreementMode::ConsensusFinalized) && consensus_required {
            let tree_state = effects
                .get_current_state()
                .await
                .map_err(map_tree_read_error)?;
            let journal = effects.fetch_context_journal(context).await.map_err(|e| {
                IntentError::internal_error(format!("Context journal lookup failed: {e}"))
            })?;
            let context_commitment =
                context_commitment_from_journal(context, &journal).map_err(|e| {
                    IntentError::internal_error(format!("Context commitment failed: {e}"))
                })?;
            let prestate = Prestate::new(
                vec![(authority_id, Hash32(tree_state.root_commitment))],
                context_commitment,
            )
            .map_err(map_amp_prestate_error)?;

            let params =
                build_consensus_params(context, effects.as_ref(), authority_id, effects.as_ref())
                    .await
                    .map_err(map_consensus_error)?;

            let transcript_ref = effects
                .latest_dkg_transcript_commit(authority_id, context)
                .await
                .map_err(|e| {
                    IntentError::internal_error(format!("AMP transcript lookup failed: {e}"))
                })?
                .and_then(|commit| commit.blob_ref.or(Some(commit.transcript_hash)));

            commit_bump_with_consensus(
                effects.as_ref(),
                &prestate,
                &proposal,
                params.key_packages,
                params.group_public_key,
                transcript_ref,
            )
            .await
            .map_err(map_amp_finalize_error)?;
        }

        tracing::info!(
            context = %context,
            channel = %channel,
            new_epoch = state.chan_epoch + 1,
            reason = %reason,
            "Channel epoch bumped"
        );

        Ok(())
    }

    async fn start_channel_invitation_monitor(
        &self,
        invitation_ids: Vec<String>,
        context: ContextId,
        channel: ChannelId,
    ) -> Result<(), IntentError> {
        if invitation_ids.is_empty() {
            return Ok(());
        }

        let effects = self.agent.runtime().effects();
        let reactive = effects.reactive_handler();
        let agent = self.agent.clone();
        let tasks = self.agent.runtime().tasks();
        let time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync> =
            Arc::new(effects.time_effects().clone());
        let remaining = Arc::new(std::sync::atomic::AtomicUsize::new(120));

        #[cfg(not(target_arch = "wasm32"))]
        let _monitor_task_handle = tasks.spawn_interval_until_named(
            "runtime_bridge.channel_invitation_monitor",
            time_effects.clone(),
            std::time::Duration::from_millis(1000),
            move || {
                let _effects = effects.clone();
                let reactive = reactive.clone();
                let agent = agent.clone();
                let invitation_ids = invitation_ids.clone();
                let remaining = remaining.clone();

                async move {
                    let remaining_now = remaining.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                    if remaining_now == 0 {
                        return false;
                    }

                    let invitations = match reactive.read(&INVITATIONS_SIGNAL).await {
                        Ok(state) => state,
                        Err(_) => return true,
                    };

                    let mut all_accepted = true;
                    let mut has_failure = false;

                    for id in &invitation_ids {
                        match invitations.invitation(id).map(|inv| inv.status) {
                            Some(InvitationStatus::Accepted) => {}
                            Some(InvitationStatus::Rejected)
                            | Some(InvitationStatus::Expired)
                            | Some(InvitationStatus::Revoked) => {
                                has_failure = true;
                                break;
                            }
                            _ => {
                                all_accepted = false;
                            }
                        }
                    }

                    if has_failure {
                        return false;
                    }

                    if all_accepted {
                        let bridge = AgentRuntimeBridge::new(agent.clone());
                        let _ = bridge
                            .bump_channel_epoch(
                                context,
                                channel,
                                "All invitations accepted".to_string(),
                            )
                            .await;
                        return false;
                    }

                    true
                }
            },
        );

        #[cfg(target_arch = "wasm32")]
        let _monitor_task_handle = tasks.spawn_local_interval_until_named(
            "runtime_bridge.channel_invitation_monitor",
            time_effects,
            std::time::Duration::from_millis(1000),
            move || {
                let _effects = effects.clone();
                let reactive = reactive.clone();
                let agent = agent.clone();
                let invitation_ids = invitation_ids.clone();
                let remaining = remaining.clone();

                async move {
                    let remaining_now = remaining.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                    if remaining_now == 0 {
                        return false;
                    }

                    let invitations = match reactive.read(&INVITATIONS_SIGNAL).await {
                        Ok(state) => state,
                        Err(_) => return true,
                    };

                    let mut all_accepted = true;
                    let mut has_failure = false;

                    for id in &invitation_ids {
                        match invitations.invitation(id).map(|inv| inv.status) {
                            Some(InvitationStatus::Accepted) => {}
                            Some(InvitationStatus::Rejected)
                            | Some(InvitationStatus::Expired)
                            | Some(InvitationStatus::Revoked) => {
                                has_failure = true;
                                break;
                            }
                            _ => {
                                all_accepted = false;
                            }
                        }
                    }

                    if has_failure {
                        return false;
                    }

                    if all_accepted {
                        let bridge = AgentRuntimeBridge::new(agent.clone());
                        let _ = bridge
                            .bump_channel_epoch(
                                context,
                                channel,
                                "All invitations accepted".to_string(),
                            )
                            .await;
                        return false;
                    }

                    true
                }
            },
        );

        Ok(())
    }

    async fn amp_send_message(
        &self,
        params: ChannelSendParams,
    ) -> Result<AmpCiphertext, RuntimeBridgeError> {
        let effects = self.agent.runtime().effects();
        effects.send_message(params).await.map_err(map_amp_error)
    }

    // =========================================================================
    // Moderation Operations
    // =========================================================================

    async fn moderation_kick(
        &self,
        context_id: ContextId,
        channel_id: ChannelId,
        target: AuthorityId,
        reason: Option<String>,
    ) -> Result<(), IntentError> {
        let effects = self.agent.runtime().effects();
        let now = effects.physical_time().await.map_err(map_time_read_error)?;

        let causal = self
            .causal_stamp(CausalStampKey::HomeGovernance {
                context_id,
                key: HomeGovernanceKey::Kick {
                    target,
                    channel: channel_id,
                },
            })
            .await?;
        let fact = HomeKickFact::new_ms(
            context_id,
            channel_id,
            target,
            self.agent.authority_id(),
            reason.unwrap_or_default(),
            now.ts_ms,
            causal,
        )
        .to_generic();

        self.commit_relational_facts(&[fact]).await
    }

    async fn moderation_ban(
        &self,
        context_id: ContextId,
        _channel_id: ChannelId,
        target: AuthorityId,
        reason: Option<String>,
    ) -> Result<(), IntentError> {
        let effects = self.agent.runtime().effects();
        let now = effects.physical_time().await.map_err(map_time_read_error)?;

        let causal = self
            .causal_stamp(CausalStampKey::HomeGovernance {
                context_id,
                key: HomeGovernanceKey::Ban {
                    target,
                    channel: None,
                },
            })
            .await?;
        let fact = HomeBanFact::new_ms(
            context_id,
            None,
            target,
            self.agent.authority_id(),
            reason.unwrap_or_default(),
            now.ts_ms,
            None,
            causal,
        )
        .to_generic();

        self.commit_relational_facts(&[fact]).await
    }

    async fn moderation_unban(
        &self,
        context_id: ContextId,
        _channel_id: ChannelId,
        target: AuthorityId,
    ) -> Result<(), IntentError> {
        let effects = self.agent.runtime().effects();
        let now = effects.physical_time().await.map_err(map_time_read_error)?;

        let causal = self
            .causal_stamp(CausalStampKey::HomeGovernance {
                context_id,
                key: HomeGovernanceKey::Unban {
                    target,
                    channel: None,
                },
            })
            .await?;
        let fact = HomeUnbanFact::new_ms(
            context_id,
            None,
            target,
            self.agent.authority_id(),
            now.ts_ms,
            causal,
        )
        .to_generic();

        self.commit_relational_facts(&[fact]).await
    }

    async fn moderation_mute(
        &self,
        context_id: ContextId,
        _channel_id: ChannelId,
        target: AuthorityId,
        duration_secs: Option<u64>,
    ) -> Result<(), IntentError> {
        let effects = self.agent.runtime().effects();
        let now = effects.physical_time().await.map_err(map_time_read_error)?;
        let expires_at = duration_secs.map(|s| now.ts_ms.saturating_add(s.saturating_mul(1000)));

        let causal = self
            .causal_stamp(CausalStampKey::HomeGovernance {
                context_id,
                key: HomeGovernanceKey::Mute {
                    target,
                    channel: None,
                },
            })
            .await?;
        let fact = HomeMuteFact::new_ms(
            context_id,
            None,
            target,
            self.agent.authority_id(),
            duration_secs,
            now.ts_ms,
            expires_at,
            causal,
        )
        .to_generic();

        self.commit_relational_facts(&[fact]).await
    }

    async fn moderation_unmute(
        &self,
        context_id: ContextId,
        _channel_id: ChannelId,
        target: AuthorityId,
    ) -> Result<(), IntentError> {
        let effects = self.agent.runtime().effects();
        let now = effects.physical_time().await.map_err(map_time_read_error)?;

        let causal = self
            .causal_stamp(CausalStampKey::HomeGovernance {
                context_id,
                key: HomeGovernanceKey::Unmute {
                    target,
                    channel: None,
                },
            })
            .await?;
        let fact = HomeUnmuteFact::new_ms(
            context_id,
            None,
            target,
            self.agent.authority_id(),
            now.ts_ms,
            causal,
        )
        .to_generic();

        self.commit_relational_facts(&[fact]).await
    }

    async fn moderation_pin(
        &self,
        context_id: ContextId,
        channel_id: ChannelId,
        message_id: String,
    ) -> Result<(), IntentError> {
        let effects = self.agent.runtime().effects();
        let now = effects.physical_time().await.map_err(map_time_read_error)?;

        let fact = HomePinFact::new_ms(
            context_id,
            channel_id,
            message_id,
            self.agent.authority_id(),
            now.ts_ms,
        )
        .to_generic();

        self.commit_relational_facts(&[fact]).await
    }

    async fn moderation_unpin(
        &self,
        context_id: ContextId,
        channel_id: ChannelId,
        message_id: String,
    ) -> Result<(), IntentError> {
        let effects = self.agent.runtime().effects();
        let now = effects.physical_time().await.map_err(map_time_read_error)?;

        let fact = HomeUnpinFact::new_ms(
            context_id,
            channel_id,
            message_id,
            self.agent.authority_id(),
            now.ts_ms,
        )
        .to_generic();

        self.commit_relational_facts(&[fact]).await
    }

    async fn channel_set_topic(
        &self,
        context_id: ContextId,
        channel_id: ChannelId,
        topic: String,
        timestamp_ms: u64,
    ) -> Result<(), IntentError> {
        let fact = aura_chat::ChatFact::channel_updated_ms(
            context_id,
            channel_id,
            None,
            Some(topic),
            None,
            None,
            timestamp_ms,
            self.agent.authority_id(),
        )
        .to_generic();

        self.commit_relational_facts(&[fact]).await
    }

    // =========================================================================
    // Sync Operations
    // =========================================================================

    async fn try_get_sync_status(&self) -> Result<SyncStatus, IntentError> {
        sync::get_sync_status(self).await
    }

    async fn is_peer_online(&self, peer: AuthorityId) -> bool {
        sync::is_peer_online(self, peer).await
    }
    async fn try_get_sync_peers(&self) -> Result<Vec<DeviceId>, IntentError> {
        sync::get_sync_peers(self).await
    }

    async fn trigger_sync(&self) -> Result<(), IntentError> {
        sync::trigger_sync(self).await
    }

    async fn process_ceremony_messages(&self) -> Result<CeremonyProcessingOutcome, IntentError> {
        sync::process_ceremony_messages(self).await
    }

    async fn sync_with_peer(&self, peer_id: &str) -> Result<(), IntentError> {
        sync::sync_with_peer(self, peer_id).await
    }

    async fn ensure_peer_channel(
        &self,
        context: ContextId,
        peer: AuthorityId,
    ) -> Result<(), aura_app::runtime_bridge::RuntimeBridgeError> {
        sync::ensure_peer_channel(self, context, peer).await
    }

    // =========================================================================
    // Peer Discovery
    // =========================================================================

    async fn try_get_discovered_peers(&self) -> Result<Vec<AuthorityId>, IntentError> {
        rendezvous::get_discovered_peers(self).await
    }

    async fn try_get_rendezvous_status(&self) -> Result<RendezvousStatus, IntentError> {
        rendezvous::get_rendezvous_status(self).await
    }

    async fn trigger_discovery(&self) -> Result<DiscoveryTriggerOutcome, IntentError> {
        rendezvous::trigger_discovery(self).await
    }

    // =========================================================================
    // Bootstrap Discovery
    // =========================================================================

    async fn try_get_bootstrap_candidates(
        &self,
    ) -> Result<Vec<BootstrapCandidateInfo>, IntentError> {
        rendezvous::get_bootstrap_candidates(self).await
    }

    async fn replay_committed_facts(
        &self,
    ) -> Result<(), aura_app::runtime_bridge::RuntimeBridgeError> {
        self.agent
            .runtime()
            .replay_committed_facts()
            .await
            .map_err(|source| {
                error_boundary::bridge_runtime_internal("required reactive replay", source)
            })
    }

    async fn try_get_lan_discovery_stats(
        &self,
    ) -> Result<Option<aura_app::signal_defs::LanDiscoveryStats>, IntentError> {
        let rendezvous = require_rendezvous_service(self)?;
        Ok(rendezvous
            .lan_metrics()
            .await
            .map(|metrics| aura_app::signal_defs::LanDiscoveryStats {
                announcements_sent: metrics.announcements_sent,
                packets_received: metrics.packets_received,
                packets_invalid: metrics.packets_invalid,
                peers_discovered: metrics.peers_discovered,
            }))
    }

    async fn refresh_bootstrap_candidate_registration(&self) -> Result<(), IntentError> {
        rendezvous::refresh_bootstrap_candidate_registration(self).await
    }

    async fn send_bootstrap_invitation(
        &self,
        _peer: &BootstrapCandidateInfo,
        _invitation_code: &str,
    ) -> Result<(), IntentError> {
        rendezvous::send_bootstrap_invitation(self, _peer, _invitation_code).await
    }

    // =========================================================================
    // Threshold Signing
    // =========================================================================

    async fn sign_tree_op(&self, op: &TreeOp) -> Result<AttestedOp, IntentError> {
        let authority = self.agent.authority_id();
        let signing_service = self.agent.threshold_signing();

        // Create signing context for self-operation
        let context = SigningContext::self_tree_op(authority, op.clone());

        // Sign using the unified threshold signing service
        let signature = signing_service
            .sign(context)
            .await
            .map_err(|e| IntentError::internal_error(format!("Threshold signing failed: {}", e)))?;

        // Create attested operation
        Ok(AttestedOp {
            op: op.clone(),
            agg_sig: signature.signature,
            signer_count: signature.signer_count,
        })
    }

    async fn bootstrap_signing_keys(
        &self,
    ) -> Result<Vec<u8>, aura_app::runtime_bridge::RuntimeBridgeError> {
        let authority = self.agent.authority_id();
        let signing_service = self.agent.threshold_signing();

        // Bootstrap 1-of-1 keys for single-device operation
        let public_key_package = signing_service
            .bootstrap_authority(&authority)
            .await
            .map_err(|e| {
                error_boundary::bridge_runtime_internal("Failed to bootstrap signing keys", e)
            })?;

        self.restore_owned_device_enrollment_ceremonies()
            .await
            .map_err(|error| {
                error_boundary::bridge_runtime_internal("Restore enrollment ceremonies", error)
            })?;

        // The LAN identity key now exists; announce this account right away.
        if let Err(error) = self.agent.runtime().publish_lan_descriptor().await {
            tracing::debug!(
                error = %error,
                "LAN descriptor publish after signing-key bootstrap failed; periodic refresh will retry"
            );
        }

        Ok(public_key_package)
    }

    async fn get_threshold_config(&self) -> Option<ThresholdConfig> {
        let authority = self.agent.authority_id();
        let signing_service = self.agent.threshold_signing();
        signing_service.threshold_config(&authority).await
    }

    async fn has_signing_capability(&self) -> bool {
        let authority = self.agent.authority_id();
        let signing_service = self.agent.threshold_signing();
        signing_service.has_signing_capability(&authority).await
    }

    async fn get_public_key_package(&self) -> Option<Vec<u8>> {
        let authority = self.agent.authority_id();
        let signing_service = self.agent.threshold_signing();
        signing_service.public_key_package(&authority).await
    }

    async fn sign_with_context(
        &self,
        context: SigningContext,
    ) -> Result<ThresholdSignature, IntentError> {
        let signing_service = self.agent.threshold_signing();
        signing_service
            .sign(context)
            .await
            .map_err(|e| IntentError::internal_error(format!("Threshold signing failed: {}", e)))
    }

    async fn export_device_enrollment_setup_request(
        &self,
    ) -> Result<String, aura_invitation::enrollment_setup::EnrollmentSetupExportError> {
        self.agent
            .threshold_signing()
            .export_device_enrollment_setup_request(self.agent.authority_id())
            .await
    }

    async fn verify_device_enrollment_setup_possession(
        &self,
        code: String,
    ) -> Result<
        aura_invitation::enrollment_setup::VerifiedEnrollmentSetupPossession,
        aura_invitation::enrollment_setup::EnrollmentSetupVerificationError,
    > {
        let effects = self.agent.runtime().effects();
        let now = effects.physical_time().await?.ts_ms;
        Ok(
            aura_invitation::enrollment_setup::DeviceEnrollmentSetupRequest::decode(&code)?
                .verify_possession(effects.as_ref(), now)
                .await?,
        )
    }

    async fn verify_enrollment_manifest_transfer(
        &self,
        manifest_code: String,
        initiator_verifier_code: String,
    ) -> Result<
        aura_invitation::enrollment_manifest::VerifiedEnrollmentManifestSignature,
        aura_invitation::enrollment_manifest::EnrollmentManifestError,
    > {
        use aura_invitation::enrollment_manifest::{
            decode_initiator_verifier_transfer, EnrollmentManifestError,
            SignedEnrollmentTrustManifest,
        };
        if initiator_verifier_code.trim().is_empty() {
            return Err(EnrollmentManifestError::MissingPin);
        }
        let selected = decode_initiator_verifier_transfer(&initiator_verifier_code)?;
        let signed = SignedEnrollmentTrustManifest::decode(&manifest_code)?;
        if signed.manifest.subject != selected.subject
            || signed.manifest.initiator_device != selected.initiator_device
        {
            return Err(EnrollmentManifestError::Pin);
        }
        let verifier = selected.verifying_key;
        let effects = self.agent.runtime().effects();
        if effects.physical_time().await?.ts_ms >= signed.manifest.expires_at_ms {
            return Err(EnrollmentManifestError::Expired);
        }
        if signed.manifest.invitee_device != effects.device_id() {
            return Err(EnrollmentManifestError::Pin);
        }
        signed
            .manifest
            .verify_signature(effects.as_ref(), &verifier, &signed.signature)
            .await
    }

    async fn rotate_guardian_keys(
        &self,
        threshold_k: FrostThreshold,
        total_n: u16,
        guardian_ids: &[AuthorityId],
    ) -> Result<(Epoch, Vec<Vec<u8>>, Vec<u8>), IntentError> {
        let authority = self.agent.authority_id();
        let signing_service = self.agent.threshold_signing();

        let participants = guardian_ids
            .iter()
            .copied()
            .map(aura_core::threshold::ParticipantIdentity::guardian)
            .collect::<Vec<_>>();

        // Rotate keys to a new threshold configuration
        // The service returns (new_epoch, key_packages, public_key_bytes)
        // where public_key_bytes is already serialized
        signing_service
            .rotate_keys(&authority, threshold_k.value(), total_n, &participants)
            .await
            .map(|(epoch, key_packages, public_key)| (Epoch::new(epoch), key_packages, public_key))
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to rotate guardian keys: {}", e))
            })
    }

    async fn commit_guardian_key_rotation(&self, new_epoch: Epoch) -> Result<(), IntentError> {
        let authority = self.agent.authority_id();
        let signing_service = self.agent.threshold_signing();
        let policy = aura_core::threshold::policy_for(
            aura_core::threshold::CeremonyFlow::GuardianSetupRotation,
        );

        let consensus_required = signing_service
            .threshold_state(&authority)
            .await
            .map(|state| state.threshold > 1 || state.total_participants > 1)
            .unwrap_or(true);

        if policy.keygen == aura_core::threshold::KeyGenerationPolicy::K3ConsensusDkg
            && consensus_required
        {
            let effects = self.agent.runtime().effects();
            let context_id = default_context_id_for_authority(authority);
            let has_commit = effects
                .has_dkg_transcript_commit(authority, context_id, new_epoch.value())
                .await
                .map_err(|e| {
                    IntentError::internal_error(format!(
                        "Failed to verify DKG transcript commit: {e}"
                    ))
                })?;
            if !has_commit {
                return Err(IntentError::validation_failed(
                    "Missing consensus DKG transcript".to_string(),
                ));
            }
        } else if policy.keygen == aura_core::threshold::KeyGenerationPolicy::K3ConsensusDkg
            && !consensus_required
        {
            tracing::info!(
                ceremony = "guardian_rotation",
                "Skipping consensus transcript check (single-signer authority)"
            );
        }

        signing_service
            .commit_key_rotation(&authority, new_epoch.value())
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to commit key rotation: {}", e))
            })
    }

    async fn rollback_guardian_key_rotation(&self, failed_epoch: Epoch) -> Result<(), IntentError> {
        let authority = self.agent.authority_id();
        let signing_service = self.agent.threshold_signing();

        signing_service
            .rollback_key_rotation(&authority, failed_epoch.value())
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to rollback key rotation: {}", e))
            })
    }

    async fn initiate_guardian_ceremony(
        &self,
        threshold_k: FrostThreshold,
        total_n: u16,
        guardian_ids: &[AuthorityId],
    ) -> Result<aura_core::types::identifiers::CeremonyId, IntentError> {
        use aura_core::hash::hash;
        use aura_core::threshold::{policy_for, CeremonyFlow, KeyGenerationPolicy};
        use aura_recovery::guardian_ceremony::GuardianState;
        use aura_recovery::{CeremonyId as GuardianCeremonyId, GuardianRotationOp};

        let participants = guardian_ids
            .iter()
            .copied()
            .map(aura_core::threshold::ParticipantIdentity::guardian)
            .collect::<Vec<_>>();

        let policy = policy_for(CeremonyFlow::GuardianSetupRotation);

        // Step 0: every guardian must have accepted a guardian invitation, which
        // records the verified key the ceremony needs. Fail before rotating
        // keys instead of reporting a started ceremony that fails at once.
        {
            use aura_core::effects::StorageCoreEffects;
            let effects = self.agent.runtime().effects();
            for guardian in guardian_ids {
                let key = effects
                    .retrieve(
                        &crate::handlers::recovery::recovery_guardian_public_key_storage_key(
                            *guardian,
                        ),
                    )
                    .await
                    .map_err(|error| {
                        IntentError::internal_error(format!(
                            "Failed to read guardian key for {guardian}: {error}"
                        ))
                    })?;
                if key.is_none() {
                    return Err(IntentError::validation_failed(format!(
                        "{guardian} has not accepted a guardian invitation yet; invite them as a guardian first"
                    )));
                }
            }
        }

        // Step 1: Generate FROST keys at new epoch
        let (new_epoch, key_packages, _public_key) = self
            .rotate_guardian_keys(threshold_k, total_n, guardian_ids)
            .await?;

        // Step 2: Compute prestate + operation hashes and derive a ceremony id.
        let authority_id = self.agent.authority_id();
        let signing_service = self.agent.threshold_signing();
        let effects = self.agent.runtime().effects();

        let current_state = match signing_service.threshold_state(&authority_id).await {
            Some(state) => {
                let public_key = signing_service
                    .public_key_package(&authority_id)
                    .await
                    .unwrap_or_default();

                let public_key_hash = aura_core::Hash32(hash(&public_key));
                let current_guardian_ids: Vec<AuthorityId> = state
                    .participants
                    .iter()
                    .filter_map(|p| match p {
                        aura_core::threshold::ParticipantIdentity::Guardian(id) => Some(*id),
                        _ => None,
                    })
                    .collect();

                GuardianState {
                    epoch: state.epoch,
                    threshold_k: state.threshold,
                    guardian_ids: current_guardian_ids,
                    public_key_hash,
                }
            }
            None => GuardianState::empty(),
        };

        let tree_state = effects
            .get_current_state()
            .await
            .map_err(map_tree_read_error)?;
        let context_commitment = current_state.compute_prestate_hash(&authority_id);
        let prestate = Prestate::new(
            vec![(authority_id, Hash32(tree_state.root_commitment))],
            context_commitment,
        )
        .map_err(|e| IntentError::internal_error(format!("Invalid guardian prestate: {e}")))?;
        let prestate_hash = prestate.compute_hash();
        let threshold_k_value = threshold_k.value();
        let operation = GuardianRotationOp {
            threshold_k: threshold_k_value,
            total_n,
            guardian_ids: guardian_ids.to_vec(),
            new_epoch: new_epoch.value(),
        };
        let operation_hash = operation.compute_hash();

        let consensus_required = signing_service
            .threshold_state(&authority_id)
            .await
            .map(|state| state.threshold > 1 || state.total_participants > 1)
            .unwrap_or(true);

        if policy.keygen == KeyGenerationPolicy::K3ConsensusDkg && consensus_required {
            // For guardian rotation, use authority's own context
            let guardian_context =
                aura_core::ContextId::new_from_entropy(hash(&authority_id.to_bytes()));
            let params = build_consensus_params(
                guardian_context,
                effects.as_ref(),
                authority_id,
                &signing_service,
            )
            .await
            .map_err(map_consensus_error)?;
            let _ = persist_consensus_dkg_transcript(
                effects.clone(),
                prestate,
                params,
                authority_id,
                new_epoch.value(),
                threshold_k_value,
                total_n,
                &participants,
                operation_hash,
            )
            .await?;
        } else if policy.keygen == KeyGenerationPolicy::K3ConsensusDkg && !consensus_required {
            tracing::info!(
                ceremony = "guardian_rotation",
                "Skipping consensus DKG transcript (single-signer authority)"
            );
        }

        // Use a monotonic nonce for uniqueness within this process.
        use std::sync::atomic::{AtomicU64, Ordering};
        static CEREMONY_NONCE: AtomicU64 = AtomicU64::new(0);
        let nonce = CEREMONY_NONCE.fetch_add(1, Ordering::Relaxed);
        let ceremony_id_hash = GuardianCeremonyId::new(prestate_hash, operation_hash, nonce);
        let ceremony_id =
            aura_core::types::identifiers::CeremonyId::new(hex::encode(ceremony_id_hash.0 .0));

        tracing::info!(
            ceremony_id = %ceremony_id,
            new_epoch = new_epoch.value(),
            threshold_k = threshold_k_value,
            total_n,
            "Guardian ceremony initiated, sending invitations to {} guardians",
            guardian_ids.len()
        );

        // Step 3: Register ceremony with runner (and supersede stale candidates)
        let runner = self.agent.ceremony_runner().await;
        let now_ms = effects
            .physical_time()
            .await
            .map_err(map_time_read_error)?
            .ts_ms;
        for old_id in runner
            .check_supersession_candidates(
                aura_app::runtime_bridge::CeremonyKind::GuardianRotation,
                &prestate_hash,
            )
            .await
        {
            let _ = runner
                .supersede(
                    &old_id,
                    &ceremony_id,
                    SupersessionReason::NewerRequest,
                    now_ms,
                )
                .await;
        }
        runner
            .start(CeremonyInitRequest {
                ceremony_id: ceremony_id.clone(),
                kind: aura_app::runtime_bridge::CeremonyKind::GuardianRotation,
                initiator_id: authority_id,
                threshold_k: threshold_k_value,
                total_n,
                participants,
                new_epoch: new_epoch.value(),
                enrollment_device_id: None,
                enrollment_nickname_suggestion: None,
                prestate_hash,
            })
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to register ceremony: {}", e))
            })?;

        // Steps 4-7 wait on guardians, who approve on their own devices and may
        // take minutes. Run them as an owned background task and return the
        // ceremony id now; failures mark the ceremony failed in the tracker.
        let recovery_service = self
            .agent
            .recovery()
            .map_err(|e| service_unavailable_with_detail("recovery_service", e))?;
        let task_name = format!("guardian_ceremony_initiator.{ceremony_id}");
        let task_ceremony_id = ceremony_id.clone();
        let task_signing_service = signing_service.clone();
        let rotation_epoch = new_epoch.value();
        let fut = async move {
            let result = run_guardian_ceremony_initiator(
                recovery_service,
                runner.clone(),
                effects,
                authority_id,
                task_ceremony_id.clone(),
                ceremony_id_hash,
                prestate_hash,
                operation,
                key_packages,
            )
            .await;
            // The prepared key rotation becomes active only once the ceremony
            // commits; a failed ceremony restores the previous epoch.
            let result = match result {
                Ok(()) => task_signing_service
                    .commit_key_rotation(&authority_id, rotation_epoch)
                    .await
                    .map_err(|e| format!("Failed to commit guardian key rotation: {e}")),
                Err(error) => {
                    if let Err(rollback) = task_signing_service
                        .rollback_key_rotation(&authority_id, rotation_epoch)
                        .await
                    {
                        tracing::warn!(
                            ceremony_id = %task_ceremony_id,
                            error = %rollback,
                            "Failed to roll back guardian key rotation"
                        );
                    }
                    Err(error)
                }
            };
            if let Err(error) = result {
                tracing::warn!(
                    ceremony_id = %task_ceremony_id,
                    error = %error,
                    "Guardian ceremony failed"
                );
                let _ = runner.abort(&task_ceremony_id, Some(error)).await;
            }
        };
        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                let _task_handle = self.agent.runtime().tasks().spawn_local_named(task_name, fut);
            } else {
                let _task_handle = self.agent.runtime().tasks().spawn_named(task_name, fut);
            }
        }

        Ok(ceremony_id)
    }

    /// Initiate a device threshold (multifactor) ceremony with cross-authority envelope routing.
    ///
    /// This implementation handles the technical details of distributing FROST key packages
    /// to devices that may have different authorities than the target authority being configured.
    ///
    /// # Device-Targeted Envelope Routing
    ///
    /// Enrollment stays within the existing authority. Device-specific routing is expressed with
    /// `metadata["aura-destination-device-id"]`, while the envelope destination remains the
    /// authority being configured.
    ///
    /// Key package envelopes are routed as follows:
    /// - **destination**: Authority being configured for threshold signing
    /// - **source**: Initiator's authority (current authority_id)
    /// - **metadata["aura-destination-device-id"]**: Specific destination device within that authority
    ///
    /// This keeps authority identity explicit and avoids modeling device enrollment as a
    /// cross-authority handoff.
    ///
    /// # Fresh DKG vs Existing State
    ///
    /// This ceremony performs fresh distributed key generation (DKG):
    /// - Calls `rotate_keys()` to generate new FROST key material at pending epoch
    /// - Does NOT load existing threshold state (which may not exist yet)
    /// - Does NOT call `build_consensus_params()` (consensus happens after distribution)
    ///
    /// The threshold state is only established in storage AFTER devices respond with acceptances.
    ///
    /// # Envelope Distribution
    ///
    /// For each device in `device_ids`:
    /// 1. Compute device authority from device_id
    /// 2. Create TransportEnvelope with:
    ///    - destination = device_authority
    ///    - metadata["target-authority-id"] = initiator's authority_id
    ///    - metadata["participant-device-id"] = device_id (for recipient validation)
    ///    - payload = FROST key package for this participant
    /// 3. Launch a device-scoped `aura.sync.device_epoch_rotation` session for each
    ///    participant device
    /// 4. Let the protocol session carry proposal, acceptance, and commit ordering
    ///
    /// # Error Cases
    ///
    /// - **No transport available**: Device has no running agent with SharedTransport
    /// - **Device unreachable**: Transport cannot deliver envelope to device's authority
    /// - **Validation failure**: Invalid threshold, missing current device, duplicate devices
    ///
    /// # See Also
    ///
    /// - `crates/aura-agent/src/handlers/device_epoch_rotation.rs` - Recipient handling
    /// - `docs/102_authority_and_identity.md` - Multi-authority device model
    async fn initiate_device_threshold_ceremony(
        &self,
        threshold_k: FrostThreshold,
        total_n: u16,
        device_ids: &[String],
    ) -> Result<aura_core::types::identifiers::CeremonyId, IntentError> {
        use aura_core::effects::{
            SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
        };
        use aura_core::hash::hash;
        use aura_core::threshold::{policy_for, CeremonyFlow, ParticipantIdentity};

        let authority_id = self.agent.authority_id();
        let effects = self.agent.runtime().effects();
        let current_device_id = self.agent.context().device_id();

        let mut parsed_devices: Vec<aura_core::DeviceId> = Vec::with_capacity(device_ids.len());
        for id_str in device_ids {
            let device_id: aura_core::DeviceId = id_str.parse().map_err(|_| {
                IntentError::validation_failed(format!("Failed to parse device id: {}", id_str))
            })?;
            if parsed_devices.contains(&device_id) {
                return Err(IntentError::validation_failed(format!(
                    "Duplicate device id provided: {}",
                    id_str
                )));
            }
            parsed_devices.push(device_id);
        }

        if parsed_devices.len() != total_n as usize {
            return Err(IntentError::validation_failed(format!(
                "Device count ({}) must match total_n ({})",
                parsed_devices.len(),
                total_n
            )));
        }

        if !parsed_devices.contains(&current_device_id) {
            return Err(IntentError::validation_failed(
                "Current device must participate in MFA ceremony".to_string(),
            ));
        }

        // A multifactor ceremony re-keys this account's own enrolled devices
        // (their tree leaves authenticate the rotation). A device of another
        // authority, such as a contact, is added through device enrollment
        // first; refuse it here with a clear reason (Task 200).
        let enrolled_tree = effects
            .get_current_state()
            .await
            .map_err(map_tree_read_error)?;
        if let Some(unenrolled) = parsed_devices.iter().find(|device| {
            !enrolled_tree.leaves.values().any(|leaf| {
                leaf.role == aura_core::tree::LeafRole::Device && leaf.device_id == **device
            })
        }) {
            return Err(IntentError::validation_failed(format!(
                "Device {unenrolled} is not enrolled in this account. Enroll the device \
                 before including it in a multifactor ceremony."
            )));
        }

        let threshold_value = threshold_k.value();
        if threshold_value < 2 || threshold_value > total_n {
            return Err(IntentError::validation_failed(format!(
                "Invalid threshold {} for {} devices",
                threshold_value, total_n
            )));
        }

        let _policy = policy_for(CeremonyFlow::DeviceMfaRotation);

        let participants: Vec<ParticipantIdentity> = parsed_devices
            .iter()
            .copied()
            .map(ParticipantIdentity::device)
            .collect();

        let (pending_epoch, key_packages, public_key_package) = effects
            .rotate_keys(&authority_id, threshold_value, total_n, &participants)
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to prepare device rotation: {e}"))
            })?;
        let pending_epoch = Epoch::new(pending_epoch);

        let config_location = SecureStorageLocation::with_sub_key(
            "threshold_config",
            format!("{}", authority_id),
            format!("{}", pending_epoch.value()),
        );

        let threshold_config = match effects
            .secure_retrieve(
                &config_location,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await
        {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::warn!(error = %e, "Missing MFA threshold config");
                Vec::new()
            }
        };

        // Use the freshly generated public_key_package from rotate_keys
        // Map key packages to devices for ceremony distribution
        let mut key_package_by_device: std::collections::HashMap<aura_core::DeviceId, Vec<u8>> =
            std::collections::HashMap::new();
        for (device_id, key_package) in parsed_devices.iter().copied().zip(key_packages.iter()) {
            key_package_by_device.insert(device_id, key_package.clone());
        }

        let tree_state = effects
            .get_current_state()
            .await
            .map_err(map_tree_read_error)?;

        let prestate_input = serde_json::to_vec(&(
            tree_state.epoch,
            tree_state.root_commitment,
            parsed_devices.clone(),
            threshold_value,
            total_n,
        ))
        .map_err(|e| map_serialization_error("Serialize prestate", e))?;
        let context_commitment = aura_core::Hash32(hash(&prestate_input));
        let prestate = Prestate::new(
            vec![(authority_id, Hash32(tree_state.root_commitment))],
            context_commitment,
        )
        .map_err(|e| IntentError::internal_error(format!("Invalid MFA prestate: {e}")))?;
        let prestate_hash = prestate.compute_hash();

        let op_input = serde_json::to_vec(&(
            pending_epoch.value(),
            threshold_value,
            total_n,
            &parsed_devices,
        ))
        .map_err(|e| map_serialization_error("Serialize operation", e))?;
        let op_hash = aura_core::Hash32(hash(&op_input));

        // For K3ConsensusDkg ceremonies, we would normally run consensus to finalize the DKG.
        // However, for device threshold ceremonies, we're doing FRESH DKG (we just called rotate_keys),
        // so we don't have threshold state in storage yet. We skip the consensus step here and just
        // distribute key packages. The consensus will happen later when devices respond.
        //
        // Note: For guardian ceremonies or subsequent rotations, this path would need to be updated
        // to handle consensus properly. For now, device threshold ceremonies are key package distribution only.

        let nonce_bytes = effects.random_bytes(8).await;
        let nonce = u64::from_le_bytes(nonce_bytes[..8].try_into().unwrap_or_default());
        let mut ceremony_seed = Vec::with_capacity(32 + 32 + 8);
        ceremony_seed.extend_from_slice(prestate_hash.as_bytes());
        ceremony_seed.extend_from_slice(op_hash.as_bytes());
        ceremony_seed.extend_from_slice(&nonce.to_le_bytes());
        let ceremony_hash = aura_core::Hash32(hash(&ceremony_seed));
        let ceremony_id = aura_core::types::identifiers::CeremonyId::new(format!(
            "ceremony:{}",
            hex::encode(ceremony_hash.as_bytes())
        ));

        let runner = self.agent.ceremony_runner().await;
        let now_ms = effects
            .physical_time()
            .await
            .map_err(map_time_read_error)?
            .ts_ms;
        for old_id in runner
            .check_supersession_candidates(
                aura_app::runtime_bridge::CeremonyKind::DeviceRotation,
                &prestate_hash,
            )
            .await
        {
            let _ = runner
                .supersede(
                    &old_id,
                    &ceremony_id,
                    SupersessionReason::NewerRequest,
                    now_ms,
                )
                .await;
        }
        runner
            .start(CeremonyInitRequest {
                ceremony_id: ceremony_id.clone(),
                kind: aura_app::runtime_bridge::CeremonyKind::DeviceRotation,
                initiator_id: authority_id,
                threshold_k: threshold_value,
                total_n,
                participants,
                new_epoch: pending_epoch.value(),
                enrollment_device_id: None,
                enrollment_nickname_suggestion: None,
                prestate_hash,
            })
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to register ceremony: {e}"))
            })?;

        // Mark the initiator as accepted (their key package is already local).
        let _ = runner
            .record_local_response(&ceremony_id, ParticipantIdentity::device(current_device_id))
            .await;

        // Launch one protocol-native device epoch rotation session per peer device.
        for device_id in parsed_devices.iter().copied() {
            if device_id == current_device_id {
                continue;
            }

            let Some(key_package) = key_package_by_device.get(&device_id).cloned() else {
                return Err(IntentError::internal_error(format!(
                    "Missing key package for device {}",
                    device_id
                )));
            };
            self.spawn_device_epoch_rotation(
                crate::handlers::device_epoch_rotation::DeviceEpochRotationInitRequest {
                    ceremony_id: ceremony_id.clone(),
                    kind: aura_sync::protocols::DeviceEpochRotationKind::Rotation,
                    pending_epoch: pending_epoch.value(),
                    participant_device_id: device_id,
                    key_package,
                    threshold_config: threshold_config.clone(),
                    public_key_package: public_key_package.clone(),
                },
            );
        }

        Ok(ceremony_id)
    }

    async fn initiate_device_enrollment_ceremony(
        &self,
        nickname_suggestion: String,
        setup: aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup,
    ) -> Result<
        aura_app::runtime_bridge::DeviceEnrollmentStart,
        aura_invitation::enrollment_setup::EnrollmentIssuanceError,
    > {
        self.issue_original_device_enrollment(nickname_suggestion, setup, None)
            .await
    }

    async fn prepare_device_enrollment_ceremony(
        &self,
        nickname_suggestion: String,
        setup: aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup,
    ) -> Result<
        aura_app::runtime_bridge::PreparedDeviceEnrollmentSigning,
        aura_invitation::enrollment_setup::EnrollmentIssuanceError,
    > {
        self.agent
            .threshold_signing()
            .prepare_original_quorum_issuer(self.agent.clone(), nickname_suggestion, setup)
            .await
            .map_err(|source| {
                aura_invitation::enrollment_setup::EnrollmentIssuanceError::at(
                    aura_invitation::enrollment_setup::EnrollmentIssuanceStage::InvitationExport,
                    source,
                )
            })
    }

    async fn approve_device_enrollment_signing(
        &self,
        approval: aura_app::ui::workflows::ceremonies::UserApprovedEnrollmentSigningIntent,
    ) -> Result<(), aura_core::AuraError> {
        let approved = enrollment_quorum::admit_original_runtime_approval(self, approval)?;
        let group = self
            .agent
            .runtime()
            .tasks()
            .group("enrollment.explicit-approved-participants");
        self.agent
            .threshold_signing()
            .approve_original_quorum_participant(approved, &group)
            .await
    }

    async fn resume_device_enrollment_signing(
        &self,
        approval: aura_app::ui::workflows::ceremonies::UserApprovedEnrollmentSigningIntent,
    ) -> Result<
        aura_app::runtime_bridge::DeviceEnrollmentStart,
        aura_invitation::enrollment_setup::EnrollmentIssuanceError,
    > {
        let approved = enrollment_quorum::admit_original_runtime_approval(self, approval).map_err(
            |source| {
                aura_invitation::enrollment_setup::EnrollmentIssuanceError::at(
                    aura_invitation::enrollment_setup::EnrollmentIssuanceStage::InvitationExport,
                    source,
                )
            },
        )?;
        self.agent
            .threshold_signing()
            .resume_original_quorum_issuer(approved)
            .await
            .map_err(|source| {
                aura_invitation::enrollment_setup::EnrollmentIssuanceError::at(
                    aura_invitation::enrollment_setup::EnrollmentIssuanceStage::InvitationExport,
                    source,
                )
            })
    }

    async fn initiate_device_removal_ceremony(
        &self,
        device_id: String,
    ) -> Result<aura_core::types::identifiers::CeremonyId, IntentError> {
        use aura_core::effects::ThresholdSigningEffects;
        use aura_core::hash::hash;
        use aura_core::threshold::ParticipantIdentity;

        let authority_id = self.agent.authority_id();
        let effects = self.agent.runtime().effects();
        let signing_service = self.agent.threshold_signing();
        let current_device_id = self.agent.context().device_id();

        let target_device_id: aura_core::DeviceId = device_id.parse().map_err(|e| {
            IntentError::validation_failed(format!("Invalid device id '{device_id}': {e}"))
        })?;

        if target_device_id == current_device_id {
            return Err(IntentError::validation_failed(
                "Cannot remove the current device".to_string(),
            ));
        }

        let tree_state = effects
            .get_current_state()
            .await
            .map_err(map_tree_read_error)?;

        let leaf_to_remove = tree_state
            .leaves
            .iter()
            .find_map(|(leaf_id, leaf)| {
                if leaf.role == aura_core::tree::LeafRole::Device
                    && leaf.device_id == target_device_id
                {
                    Some(*leaf_id)
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                IntentError::validation_failed(format!(
                    "Device is not present in the commitment tree: {target_device_id}"
                ))
            })?;

        // Determine remaining device participants.
        let mut remaining_devices: Vec<aura_core::DeviceId> = tree_state
            .leaves
            .values()
            .filter(|leaf| {
                leaf.role == aura_core::tree::LeafRole::Device && leaf.device_id != target_device_id
            })
            .map(|leaf| leaf.device_id)
            .collect();

        if !remaining_devices.contains(&current_device_id) {
            remaining_devices.push(current_device_id);
        }

        let policy =
            aura_core::threshold::policy_for(aura_core::threshold::CeremonyFlow::DeviceRemoval);

        let mut other_device_ids: Vec<aura_core::DeviceId> = remaining_devices
            .iter()
            .copied()
            .filter(|id| *id != current_device_id)
            .collect();
        other_device_ids.sort_by_key(|a| a.to_string());

        let mut participant_device_ids: Vec<aura_core::DeviceId> =
            Vec::with_capacity(other_device_ids.len() + 1);
        participant_device_ids.push(current_device_id);
        participant_device_ids.extend(other_device_ids.iter().copied());

        let participants: Vec<ParticipantIdentity> = participant_device_ids
            .iter()
            .copied()
            .map(ParticipantIdentity::device)
            .collect();

        let total_n: u16 = participants.len().try_into().unwrap_or(u16::MAX);
        let mut threshold_k = if let Some(config) = self.get_threshold_config().await {
            config.threshold
        } else if total_n <= 2 {
            total_n
        } else {
            2
        };
        if threshold_k == 0 || threshold_k > total_n {
            threshold_k = total_n;
        }
        if total_n > 1 && threshold_k < 2 {
            threshold_k = 2.min(total_n);
        }

        let (pending_epoch, key_packages, _public_key) = effects
            .rotate_keys(&authority_id, threshold_k, total_n, &participants)
            .await
            .map_err(|e| {
                IntentError::internal_error(format!(
                    "Failed to prepare device removal rotation: {e}"
                ))
            })?;
        let pending_epoch = Epoch::new(pending_epoch);

        let pubkey_location = SecureStorageLocation::with_sub_key(
            "threshold_pubkey",
            format!("{}", authority_id),
            format!("{}", pending_epoch.value()),
        );
        let config_location = SecureStorageLocation::with_sub_key(
            "threshold_config",
            format!("{}", authority_id),
            format!("{}", pending_epoch.value()),
        );

        let public_key_package = match effects
            .secure_retrieve(
                &pubkey_location,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await
        {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::warn!(error = %e, "Missing device removal public key package");
                Vec::new()
            }
        };

        let threshold_config = match effects
            .secure_retrieve(
                &config_location,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await
        {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::warn!(error = %e, "Missing device removal threshold config");
                Vec::new()
            }
        };

        let mut key_package_by_device: std::collections::HashMap<aura_core::DeviceId, Vec<u8>> =
            std::collections::HashMap::new();
        for (device_id, key_package) in participant_device_ids
            .iter()
            .copied()
            .zip(key_packages.iter())
        {
            key_package_by_device.insert(device_id, key_package.clone());
        }

        // Compute a best-effort prestate-bound ceremony id.
        let prestate_input = serde_json::to_vec(&(
            tree_state.epoch,
            tree_state.root_commitment,
            target_device_id,
        ))
        .map_err(|e| map_serialization_error("Serialize prestate", e))?;
        let context_commitment = aura_core::Hash32(hash(&prestate_input));
        let prestate = Prestate::new(
            vec![(authority_id, Hash32(tree_state.root_commitment))],
            context_commitment,
        )
        .map_err(|e| IntentError::internal_error(format!("Invalid removal prestate: {e}")))?;
        let prestate_hash = prestate.compute_hash();

        let op_input = serde_json::to_vec(&(
            target_device_id,
            pending_epoch.value(),
            threshold_k,
            total_n,
        ))
        .map_err(|e| map_serialization_error("Serialize operation", e))?;
        let op_hash = aura_core::Hash32(hash(&op_input));

        // Aura Consensus agrees between authorities; the remaining participants
        // here are devices of this one authority, which agree through the
        // device-epoch rotation sessions below instead.
        let intra_authority = participants
            .iter()
            .all(|participant| matches!(participant, ParticipantIdentity::Device(_)));
        let consensus_required = !intra_authority
            && signing_service
                .threshold_state(&authority_id)
                .await
                .map(|state| state.threshold > 1 || state.total_participants > 1)
                .unwrap_or(true);

        if policy.keygen == aura_core::threshold::KeyGenerationPolicy::K3ConsensusDkg
            && consensus_required
        {
            // For guardian addition, use authority's own context
            let guardian_add_context =
                aura_core::ContextId::new_from_entropy(hash(&authority_id.to_bytes()));
            let params = build_consensus_params(
                guardian_add_context,
                effects.as_ref(),
                authority_id,
                &signing_service,
            )
            .await
            .map_err(map_consensus_error)?;
            let _ = persist_consensus_dkg_transcript(
                effects.clone(),
                prestate,
                params,
                authority_id,
                pending_epoch.value(),
                threshold_k,
                total_n,
                &participants,
                op_hash,
            )
            .await?;
        } else if policy.keygen == aura_core::threshold::KeyGenerationPolicy::K3ConsensusDkg
            && !consensus_required
        {
            tracing::info!(
                ceremony = "device_removal",
                "Skipping consensus DKG transcript (single-signer or device-only participants)"
            );
        }

        let nonce_bytes = effects.random_bytes(8).await;
        let nonce = u64::from_le_bytes(nonce_bytes[..8].try_into().unwrap_or_default());
        let mut ceremony_seed = Vec::with_capacity(32 + 32 + 8);
        ceremony_seed.extend_from_slice(prestate_hash.as_bytes());
        ceremony_seed.extend_from_slice(op_hash.as_bytes());
        ceremony_seed.extend_from_slice(&nonce.to_le_bytes());
        let ceremony_hash = aura_core::Hash32(hash(&ceremony_seed));
        let ceremony_id = aura_core::types::identifiers::CeremonyId::new(format!(
            "ceremony:{}",
            hex::encode(ceremony_hash.as_bytes())
        ));

        let runner = self.agent.ceremony_runner().await;
        let now_ms = effects
            .physical_time()
            .await
            .map_err(map_time_read_error)?
            .ts_ms;
        for old_id in runner
            .check_supersession_candidates(
                aura_app::runtime_bridge::CeremonyKind::DeviceRemoval,
                &prestate_hash,
            )
            .await
        {
            let _ = runner
                .supersede(
                    &old_id,
                    &ceremony_id,
                    SupersessionReason::NewerRequest,
                    now_ms,
                )
                .await;
        }
        runner
            .start(CeremonyInitRequest {
                ceremony_id: ceremony_id.clone(),
                kind: aura_app::runtime_bridge::CeremonyKind::DeviceRemoval,
                initiator_id: authority_id,
                threshold_k,
                total_n,
                participants: participants.clone(),
                new_epoch: pending_epoch.value(),
                enrollment_device_id: Some(target_device_id),
                enrollment_nickname_suggestion: None,
                prestate_hash,
            })
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to register ceremony: {e}"))
            })?;

        let _ = runner
            .record_local_response(&ceremony_id, ParticipantIdentity::device(current_device_id))
            .await;

        for device_id in participant_device_ids.iter().copied() {
            if device_id == current_device_id {
                continue;
            }

            let Some(key_package) = key_package_by_device.get(&device_id).cloned() else {
                return Err(IntentError::internal_error(format!(
                    "Missing key package for device {}",
                    device_id
                )));
            };
            self.spawn_device_epoch_rotation(
                crate::handlers::device_epoch_rotation::DeviceEpochRotationInitRequest {
                    ceremony_id: ceremony_id.clone(),
                    kind: aura_sync::protocols::DeviceEpochRotationKind::Removal,
                    pending_epoch: pending_epoch.value(),
                    participant_device_id: device_id,
                    key_package,
                    threshold_config: threshold_config.clone(),
                    public_key_package: public_key_package.clone(),
                },
            );
        }

        if policy.keygen == aura_core::threshold::KeyGenerationPolicy::K3ConsensusDkg
            && consensus_required
        {
            let context_id = default_context_id_for_authority(authority_id);
            let has_commit = effects
                .has_dkg_transcript_commit(authority_id, context_id, pending_epoch.value())
                .await
                .map_err(|e| {
                    IntentError::internal_error(format!(
                        "Failed to verify DKG transcript commit: {e}"
                    ))
                })?;
            if !has_commit {
                let _ = runner
                    .abort(
                        &ceremony_id,
                        Some("Missing consensus DKG transcript".to_string()),
                    )
                    .await;
                return Err(IntentError::validation_failed(
                    "Missing consensus DKG transcript".to_string(),
                ));
            }
        }

        if total_n == 1 && threshold_k == 1 {
            let op = aura_core::tree::TreeOp {
                parent_epoch: tree_state.epoch,
                parent_commitment: tree_state.root_commitment,
                op: aura_core::tree::TreeOpKind::RemoveLeaf {
                    leaf: leaf_to_remove,
                    reason: 0,
                },
                version: 1,
            };

            let attested = match self.sign_tree_op(&op).await {
                Ok(attested) => attested,
                Err(e) => {
                    let _ = runner
                        .abort(&ceremony_id, Some(format!("Failed to sign tree op: {e}")))
                        .await;
                    return Err(IntentError::internal_error(format!(
                        "Failed to sign tree op: {e}"
                    )));
                }
            };

            if let Err(e) = effects.apply_attested_op(attested).await {
                let _ = runner
                    .abort(&ceremony_id, Some(format!("Failed to apply tree op: {e}")))
                    .await;
                return Err(IntentError::internal_error(format!(
                    "Failed to apply tree op for device removal: {e}"
                )));
            }

            if let Err(e) = effects
                .commit_key_rotation(&authority_id, pending_epoch.value())
                .await
            {
                let _ = runner
                    .abort(&ceremony_id, Some(format!("Commit failed: {e}")))
                    .await;
                return Err(IntentError::internal_error(format!(
                    "Failed to commit key rotation: {e}"
                )));
            }

            let _ = runner
                .commit(&ceremony_id, CeremonyCommitMetadata::default())
                .await;
        }

        Ok(ceremony_id)
    }
    async fn get_ceremony_status(
        &self,
        ceremony_id: &aura_core::types::identifiers::CeremonyId,
    ) -> Result<
        aura_app::runtime_bridge::CeremonyStatus,
        aura_app::runtime_bridge::RuntimeBridgeError,
    > {
        let tracker = self.agent.ceremony_tracker().await;

        let state = tracker
            .get(ceremony_id)
            .await
            .map_err(|error| error_boundary::bridge_runtime_internal("Read ceremony", error))?;

        let accepted_guardians: Vec<AuthorityId> = state
            .accepted_participants
            .iter()
            .filter_map(|p| match p {
                aura_core::threshold::ParticipantIdentity::Guardian(id) => Some(*id),
                _ => None,
            })
            .collect();

        Ok(aura_app::runtime_bridge::CeremonyStatus {
            ceremony_id: ceremony_id.clone(),
            accepted_count: accepted_guardians.len() as u16,
            total_count: state.total_n,
            threshold: state.threshold_k,
            is_complete: state.is_committed,
            has_failed: state.has_failed,
            accepted_guardians,
            error_message: state.error_message.clone(),
            pending_epoch: Some(Epoch::new(state.new_epoch)),
            agreement_mode: state.agreement_mode,
            reversion_risk: state.agreement_mode != AgreementMode::ConsensusFinalized,
        })
    }

    async fn get_ceremony_terminal_outcome(
        &self,
        ceremony_id: &aura_core::types::identifiers::CeremonyId,
    ) -> Result<
        Option<aura_app::runtime_bridge::CeremonyTerminalOutcome>,
        aura_app::runtime_bridge::RuntimeBridgeError,
    > {
        let effects = self.agent.runtime().effects();
        if let Some(failure) = crate::handlers::invitation::enrollment_manifest_admission::load_failed_enrollment_for_ceremony(
    effects.as_ref(), self.agent.authority_id(), ceremony_id,
).await.map_err(|error| error_boundary::bridge_runtime_internal("Read authenticated failed enrollment", error))? {
    return Ok(Some(aura_app::runtime_bridge::CeremonyTerminalOutcome::Failed(failure.evidence().reason())));
}
        self.agent
            .ceremony_runner()
            .await
            .terminal_outcome(ceremony_id)
            .await
            .map_err(|error| {
                error_boundary::bridge_runtime_internal("Read durable ceremony", error)
            })
    }

    async fn list_device_enrollment_ceremonies(
        &self,
    ) -> Result<
        Vec<aura_core::types::identifiers::CeremonyId>,
        aura_app::runtime_bridge::RuntimeBridgeError,
    > {
        self.agent
            .ceremony_tracker()
            .await
            .list_device_enrollment_ceremonies()
            .await
            .map_err(|error| {
                error_boundary::bridge_runtime_internal("Read durable ceremony", error)
            })
    }

    async fn get_guardian_invitation_terminal_outcome(
        &self,
        invitation_id: &aura_core::types::identifiers::InvitationId,
    ) -> Result<Option<aura_app::runtime_bridge::CeremonyTerminalOutcome>, IntentError> {
        let key = crate::handlers::invitation::guardian_confirmation_storage_key(invitation_id);
        let effects = self.agent.runtime().effects();
        let evidence =
            aura_core::effects::storage::StorageCoreEffects::retrieve(effects.as_ref(), &key)
                .await
                .map_err(|error| {
                    IntentError::internal_error(format!(
                        "load verified guardian confirmation: {error}"
                    ))
                })?;
        if let Some(bytes) = evidence {
            let confirm: aura_invitation::protocol::GuardianConfirm =
                aura_core::util::serialization::from_slice(&bytes).map_err(|error| {
                    IntentError::internal_error(format!(
                        "decode verified guardian confirmation: {error}"
                    ))
                })?;
            if confirm.invitation_id != *invitation_id
                || !confirm.established
                || confirm.signature.is_empty()
            {
                return Err(IntentError::validation_failed(
                    "stored guardian confirmation evidence is invalid",
                ));
            }
            return Ok(Some(
                aura_app::runtime_bridge::CeremonyTerminalOutcome::Committed,
            ));
        }
        let ceremony_id = aura_core::types::identifiers::CeremonyId::new(invitation_id.to_string());
        let runner = self.agent.ceremony_runner().await;
        match runner.terminal_outcome(&ceremony_id).await {
            Ok(Some(aura_app::runtime_bridge::CeremonyTerminalOutcome::Failed(reason))) => {
                Ok(Some(
                    aura_app::runtime_bridge::CeremonyTerminalOutcome::Failed(reason),
                ))
            }
            Ok(_) | Err(_) => Ok(None),
        }
    }

    async fn get_key_rotation_ceremony_status(
        &self,
        ceremony_id: &aura_core::types::identifiers::CeremonyId,
    ) -> Result<
        aura_app::runtime_bridge::KeyRotationCeremonyStatus,
        aura_app::runtime_bridge::RuntimeBridgeError,
    > {
        let tracker = self.agent.ceremony_tracker().await;
        let state = tracker
            .get(ceremony_id)
            .await
            .map_err(|error| error_boundary::bridge_runtime_internal("Read ceremony", error))?;

        Ok(aura_app::runtime_bridge::KeyRotationCeremonyStatus {
            ceremony_id: ceremony_id.clone(),
            kind: state.kind,
            accepted_count: state.accepted_participants.len() as u16,
            total_count: state.total_n,
            threshold: state.threshold_k,
            is_complete: state.is_committed,
            has_failed: state.has_failed,
            accepted_participants: state.accepted_participants.iter().cloned().collect(),
            error_message: state.error_message,
            pending_epoch: Some(Epoch::new(state.new_epoch)),
            agreement_mode: state.agreement_mode,
            reversion_risk: state.agreement_mode != AgreementMode::ConsensusFinalized,
        })
    }

    async fn cancel_key_rotation_ceremony(
        &self,
        ceremony_id: &aura_core::types::identifiers::CeremonyId,
    ) -> Result<(), aura_app::runtime_bridge::RuntimeBridgeError> {
        let runner = self.agent.ceremony_runner().await;
        let tracker = self.agent.ceremony_tracker().await;
        let state = tracker.get(ceremony_id).await.map_err(|error| {
            error_boundary::bridge_runtime_internal("Read cancellation ceremony", error)
        })?;

        if state.kind == aura_app::runtime_bridge::CeremonyKind::DeviceEnrollment {
            // The ID selects original owned issuance, never terminal authority.
            // Its retained control gates the original-window Cancelled CAS and
            // wakes the existing signed notification owner before retirement.
            let invitations = self.agent.invitations().map_err(|error| {
                error_boundary::bridge_runtime_internal(
                    "Access enrollment cancellation owner",
                    error,
                )
            })?;
            invitations
                .cancel_original_device_enrollment_ceremony(ceremony_id)
                .await
                .map_err(|error| {
                    error_boundary::bridge_runtime_internal(
                        "Cancel original device enrollment",
                        error,
                    )
                })?;
            return Ok(());
        }

        // Guardian cancellation remains under its runtime decision gate.
        runner
            .abort(ceremony_id, Some("Canceled".to_string()))
            .await
            .map_err(|error| error_boundary::bridge_runtime_internal("Cancel ceremony", error))?;
        if !state.is_committed {
            self.rollback_guardian_key_rotation(Epoch::new(state.new_epoch))
                .await?;
        }

        Ok(())
    }

    // =========================================================================
    // Invitation Operations
    // =========================================================================

    async fn export_invitation(&self, invitation_id: &str) -> Result<String, IntentError> {
        // Get the invitation service from the agent
        let invitation_service = self
            .agent
            .invitations()
            .map_err(|e| service_unavailable_with_detail("invitation_service", e))?;

        // Export the invite code
        let invitation_id =
            aura_core::types::identifiers::InvitationId::new(invitation_id.to_string());
        invitation_service
            .export_code(&invitation_id)
            .await
            .map_err(|e| IntentError::internal_error(format!("Failed to export invitation: {}", e)))
    }

    async fn create_contact_invitation(
        &self,
        receiver: AuthorityId,
        nickname: Option<String>,
        receiver_nickname: Option<String>,
        message: Option<String>,
        ttl_ms: Option<u64>,
    ) -> Result<InvitationInfo, IntentError> {
        let invitation_service = self
            .agent
            .invitations()
            .map_err(|e| service_unavailable_with_detail("invitation_service", e))?;

        let invitation = invitation_service
            .invite_as_contact(receiver, nickname, receiver_nickname, message, ttl_ms)
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to create contact invitation: {}", e))
            })?;

        Ok(convert_invitation_to_bridge_info(&invitation))
    }

    async fn create_guardian_invitation(
        &self,
        receiver: AuthorityId,
        subject: AuthorityId,
        message: Option<String>,
        ttl_ms: Option<u64>,
    ) -> Result<InvitationInfo, IntentError> {
        let invitation_service = self
            .agent
            .invitations()
            .map_err(|e| service_unavailable_with_detail("invitation_service", e))?;

        let invitation = invitation_service
            .invite_as_guardian(receiver, subject, message, ttl_ms)
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to create guardian invitation: {}", e))
            })?;

        Ok(convert_invitation_to_bridge_info(&invitation))
    }

    async fn create_channel_invitation(
        &self,
        receiver: AuthorityId,
        home_id: String,
        context_id: Option<ContextId>,
        channel_name_hint: Option<String>,
        bootstrap: Option<ChannelBootstrapPackage>,
        message: Option<String>,
        ttl_ms: Option<u64>,
    ) -> Result<InvitationInfo, IntentError> {
        let invitation_service = self
            .agent
            .invitations()
            .map_err(|e| service_unavailable_with_detail("invitation_service", e))?;

        #[cfg(not(target_arch = "wasm32"))]
        let invitation = {
            match execute_with_effect_timeout(
                &self.agent.runtime().effects(),
                Duration::from_millis(INVITATION_BRIDGE_STAGE_TIMEOUT_MS),
                || {
                    invitation_service.invite_to_channel(
                        receiver,
                        home_id,
                        context_id,
                        channel_name_hint,
                        bootstrap,
                        message,
                        ttl_ms,
                    )
                },
            )
            .await
            {
                Err(TimeoutRunError::Timeout(_)) => {
                    return Err(IntentError::internal_error(format!(
                        "invitation_service.invite_to_channel timed out after {INVITATION_BRIDGE_STAGE_TIMEOUT_MS}ms"
                    )));
                }
                Err(TimeoutRunError::Operation(e)) => {
                    return Err(IntentError::internal_error(format!(
                        "Failed to create channel invitation: {}",
                        e
                    )));
                }
                Ok(result) => result,
            }
        };

        #[cfg(target_arch = "wasm32")]
        let invitation = execute_with_effect_timeout(
            &self.agent.runtime().effects(),
            Duration::from_millis(INVITATION_BRIDGE_STAGE_TIMEOUT_MS),
            || {
                invitation_service
                    .invite_to_channel(
                        receiver,
                        home_id,
                        context_id,
                        channel_name_hint,
                        bootstrap,
                        message,
                        ttl_ms,
                    )
            },
        )
        .await
        .map_err(|error| match error {
            TimeoutRunError::Timeout(_) => IntentError::internal_error(format!(
                "invitation_service.invite_to_channel timed out after {INVITATION_BRIDGE_STAGE_TIMEOUT_MS}ms"
            )),
            TimeoutRunError::Operation(e) => {
                IntentError::internal_error(format!("Failed to create channel invitation: {}", e))
            }
        })?
        ;

        Ok(convert_invitation_to_bridge_info(&invitation))
    }

    async fn accept_invitation(
        &self,
        invitation_id: &str,
    ) -> Result<InvitationMutationOutcome, aura_app::runtime_bridge::RuntimeBridgeError> {
        let invitation_service = self.agent.invitations().map_err(|e| {
            error_boundary::bridge_runtime_service_unavailable_with_cause("invitation_service", e)
        })?;

        let invitation_id =
            aura_core::types::identifiers::InvitationId::new(invitation_id.to_string());
        // Invitation acceptance already owns its deadline budgeting inside the
        // workflow and handler layers. Adding a second fixed bridge timeout
        // creates competing timeout policies and can fail a valid acceptance
        // path before the canonical owner budget expires.
        let result = invitation_service
            .accept(&invitation_id)
            .await
            .map_err(error_boundary::bridge_runtime_invitation_accept)?;
        self.adopt_enrolled_signing_epoch(&invitation_service, &invitation_id)
            .await?;

        Ok(InvitationMutationOutcome {
            invitation_id,
            new_status: match result.new_status {
                crate::handlers::InvitationStatus::Pending => InvitationBridgeStatus::Pending,
                crate::handlers::InvitationStatus::Accepted => InvitationBridgeStatus::Accepted,
                crate::handlers::InvitationStatus::Declined => InvitationBridgeStatus::Declined,
                crate::handlers::InvitationStatus::Expired => InvitationBridgeStatus::Expired,
                crate::handlers::InvitationStatus::Cancelled => InvitationBridgeStatus::Cancelled,
            },
        })
    }

    async fn decline_invitation(
        &self,
        invitation_id: &str,
    ) -> Result<InvitationMutationOutcome, IntentError> {
        let invitation_service = self
            .agent
            .invitations()
            .map_err(|e| service_unavailable_with_detail("invitation_service", e))?;

        let invitation_id =
            aura_core::types::identifiers::InvitationId::new(invitation_id.to_string());
        let result = invitation_service
            .decline(&invitation_id)
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to decline invitation: {}", e))
            })?;

        Ok(InvitationMutationOutcome {
            invitation_id,
            new_status: match result.new_status {
                crate::handlers::InvitationStatus::Pending => InvitationBridgeStatus::Pending,
                crate::handlers::InvitationStatus::Accepted => InvitationBridgeStatus::Accepted,
                crate::handlers::InvitationStatus::Declined => InvitationBridgeStatus::Declined,
                crate::handlers::InvitationStatus::Expired => InvitationBridgeStatus::Expired,
                crate::handlers::InvitationStatus::Cancelled => InvitationBridgeStatus::Cancelled,
            },
        })
    }

    async fn cancel_invitation(
        &self,
        invitation_id: &str,
    ) -> Result<InvitationMutationOutcome, IntentError> {
        let invitation_service = self
            .agent
            .invitations()
            .map_err(|e| service_unavailable_with_detail("invitation_service", e))?;

        let invitation_id =
            aura_core::types::identifiers::InvitationId::new(invitation_id.to_string());
        let result = invitation_service
            .cancel(&invitation_id)
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to cancel invitation: {}", e))
            })?;

        Ok(InvitationMutationOutcome {
            invitation_id,
            new_status: match result.new_status {
                crate::handlers::InvitationStatus::Pending => InvitationBridgeStatus::Pending,
                crate::handlers::InvitationStatus::Accepted => InvitationBridgeStatus::Accepted,
                crate::handlers::InvitationStatus::Declined => InvitationBridgeStatus::Declined,
                crate::handlers::InvitationStatus::Expired => InvitationBridgeStatus::Expired,
                crate::handlers::InvitationStatus::Cancelled => InvitationBridgeStatus::Cancelled,
            },
        })
    }

    async fn try_list_pending_invitations(&self) -> Result<Vec<InvitationInfo>, IntentError> {
        let invitation_service = self
            .agent
            .invitations()
            .map_err(|e| service_unavailable_with_detail("invitation_service", e))?;
        Ok(invitation_service
            .list_with_storage()
            .await
            .iter()
            .filter(|inv| inv.status == crate::handlers::InvitationStatus::Pending)
            .map(convert_invitation_to_bridge_info)
            .collect())
    }

    async fn import_invitation(&self, code: &str) -> Result<InvitationInfo, IntentError> {
        let invitation_service = self
            .agent
            .invitations()
            .map_err(|e| service_unavailable_with_detail("invitation_service", e))?;

        // Import into the agent cache so later operations (accept/decline) can resolve
        // the invitation details by ID even when the original `Sent` fact isn't present.
        let invitation = invitation_service
            .import_and_cache(code)
            .await
            .map_err(|e| IntentError::validation_failed(format!("Invalid invite code: {}", e)))?;

        Ok(convert_invitation_to_bridge_info(&invitation))
    }

    async fn import_enrollment_invitation(
        &self,
        code: &str,
        pin: aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentManifest,
    ) -> Result<InvitationInfo, aura_invitation::enrollment_manifest::EnrollmentManifestError> {
        crate::handlers::invitation::enrollment_manifest_admission::admit_user_transfer(
            self.agent.runtime().effects().as_ref(),
            self.agent.authority_id(),
            code,
            &pin,
        )
        .await?;
        self.import_invitation(code).await.map_err(|e| {
            aura_invitation::enrollment_manifest::EnrollmentManifestError::Runtime(Box::new(e))
        })
    }

    async fn try_get_invited_peer_ids(&self) -> Result<Vec<AuthorityId>, IntentError> {
        let invitation_service = self
            .agent
            .invitations()
            .map_err(|e| service_unavailable_with_detail("invitation_service", e))?;
        let our_authority = self.agent.authority_id();
        Ok(invitation_service
            .list_with_storage()
            .await
            .iter()
            .filter(|inv| {
                inv.status == crate::handlers::InvitationStatus::Pending
                    && inv.sender_id == our_authority
                    && !is_generic_contact_invitation(inv)
            })
            .map(|inv| inv.receiver_id)
            .collect())
    }

    // =========================================================================
    // Settings Operations
    // =========================================================================

    async fn try_get_settings(
        &self,
    ) -> Result<SettingsBridgeState, aura_app::runtime_bridge::RuntimeBridgeError> {
        identity::get_settings(self).await
    }

    async fn try_list_devices(
        &self,
    ) -> Result<Vec<BridgeDeviceInfo>, aura_app::runtime_bridge::RuntimeBridgeError> {
        identity::list_devices(self).await
    }

    async fn try_list_authorities(
        &self,
    ) -> Result<Vec<BridgeAuthorityInfo>, aura_app::runtime_bridge::RuntimeBridgeError> {
        identity::list_authorities(self).await
    }

    async fn has_account_config(
        &self,
    ) -> Result<bool, aura_app::runtime_bridge::RuntimeBridgeError> {
        AgentRuntimeBridge::has_account_config(self).await
    }

    async fn initialize_account(
        &self,
        nickname_suggestion: &str,
    ) -> Result<(), aura_app::runtime_bridge::RuntimeBridgeError> {
        AgentRuntimeBridge::initialize_account(self, nickname_suggestion).await
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "runtime_bridge_nickname_submission",
        receiver_type = AgentRuntimeBridge,
        family = "runtime_helper"
    )]
    async fn set_nickname_suggestion(
        &self,
        name: &str,
    ) -> Result<(), aura_app::runtime_bridge::RuntimeBridgeError> {
        let _ = identity::RUNTIME_BRIDGE_IDENTITY_NICKNAME_MUTATION_CAPABILITY;
        identity::set_nickname_suggestion(self, name).await
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "runtime_bridge_mfa_policy_submission",
        receiver_type = AgentRuntimeBridge,
        family = "runtime_helper"
    )]
    async fn set_mfa_policy(
        &self,
        policy: &str,
    ) -> Result<(), aura_app::runtime_bridge::RuntimeBridgeError> {
        let _ = identity::RUNTIME_BRIDGE_IDENTITY_MFA_POLICY_MUTATION_CAPABILITY;
        identity::set_mfa_policy(self, policy).await
    }

    async fn set_device_signing_consent(
        &self,
        consent: aura_app::runtime_bridge::DeviceSigningConsent,
    ) -> Result<(), aura_app::runtime_bridge::RuntimeBridgeError> {
        self.agent
            .threshold_signing()
            .set_device_signing_consent(consent)
            .await
            .map_err(|error| {
                error_boundary::bridge_runtime_internal(
                    "Store device signing consent failed",
                    error,
                )
            })
    }

    async fn try_list_pending_signing_requests(
        &self,
    ) -> Result<
        Vec<aura_app::runtime_bridge::PendingSigningRequest>,
        aura_app::runtime_bridge::RuntimeBridgeError,
    > {
        Ok(self
            .agent
            .threshold_signing()
            .pending_signing_requests()
            .await)
    }

    async fn decide_pending_signing_request(
        &self,
        request_id: &str,
        approve: bool,
    ) -> Result<(), aura_app::runtime_bridge::RuntimeBridgeError> {
        self.agent
            .threshold_signing()
            .decide_pending_signing_request(request_id, approve)
            .await
            .map_err(|error| {
                error_boundary::bridge_runtime_internal(
                    "Decide pending signing request failed",
                    error,
                )
            })
    }

    async fn set_peer_flow_allowance(
        &self,
        context: ContextId,
        peer: AuthorityId,
        window: u64,
    ) -> Result<(), aura_app::runtime_bridge::RuntimeBridgeError> {
        self.agent
            .runtime()
            .effects()
            .set_flow_allowance(context, peer, window)
            .await
            .map_err(|error| {
                bridge_runtime_internal("Commit flow allowance override failed", error)
            })
    }

    // =========================================================================
    // Recovery Operations
    // =========================================================================

    async fn respond_to_guardian_ceremony(
        &self,
        ceremony_id: &aura_core::types::identifiers::CeremonyId,
        accept: bool,
        _reason: Option<String>,
    ) -> Result<(), IntentError> {
        recovery::respond_to_guardian_ceremony(self, ceremony_id, accept, _reason).await
    }

    // =========================================================================
    // Time Operations
    // =========================================================================

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "runtime_bridge_physical_time_query",
        receiver_type = AgentRuntimeBridge,
        family = "runtime_helper"
    )]
    async fn current_time_ms(&self) -> Result<u64, aura_app::runtime_bridge::RuntimeBridgeError> {
        let _ = identity::RUNTIME_BRIDGE_IDENTITY_TIME_QUERY_CAPABILITY;
        identity::current_time_ms(self).await
    }

    fn physical_time_provider(&self) -> Arc<dyn aura_core::effects::PhysicalTimeEffects> {
        self.agent.runtime().effects()
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "runtime_bridge_required_sleep",
        receiver_type = AgentRuntimeBridge,
        family = "runtime_helper"
    )]
    async fn sleep_ms(&self, ms: u64) -> Result<(), RuntimeBridgeError> {
        let _ = identity::RUNTIME_BRIDGE_IDENTITY_SLEEP_CAPABILITY;
        identity::sleep_ms(self, ms).await
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "runtime_bridge_required_absolute_wait",
        receiver_type = AgentRuntimeBridge,
        family = "runtime_helper"
    )]
    async fn wait_until_physical_deadline(
        &self,
        deadline: aura_core::types::window::WindowPosition<
            aura_core::types::window::PhysicalMillis,
        >,
    ) -> Result<aura_core::time::PhysicalTime, RuntimeBridgeError> {
        identity::wait_until_physical_deadline(self, deadline).await
    }

    // =========================================================================
    // Authentication
    // =========================================================================

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "runtime_bridge_authentication_query",
        receiver_type = AgentRuntimeBridge,
        family = "runtime_helper"
    )]
    async fn authentication_status(
        &self,
    ) -> Result<AuthenticationStatus, aura_app::runtime_bridge::RuntimeBridgeError> {
        let _ = identity::RUNTIME_BRIDGE_IDENTITY_AUTHENTICATION_QUERY_CAPABILITY;
        identity::authentication_status(self).await
    }
}

// ============================================================================
// AgentRuntimeBridge helpers
// ============================================================================

/// Guardian ceremony steps 4-7 for the initiator: run the choreography, record
/// verified acceptances, commit the ceremony, and publish GuardianBinding facts.
#[allow(clippy::too_many_arguments)]
async fn run_guardian_ceremony_initiator(
    recovery_service: crate::handlers::RecoveryServiceApi,
    runner: crate::runtime::services::ceremony_runner::CeremonyRunner,
    effects: Arc<crate::AuraEffectSystem>,
    authority_id: AuthorityId,
    ceremony_id: aura_core::types::identifiers::CeremonyId,
    ceremony_id_hash: aura_recovery::CeremonyId,
    prestate_hash: Hash32,
    operation: aura_recovery::GuardianRotationOp,
    key_packages: Vec<Vec<u8>>,
) -> Result<(), String> {
    use aura_core::threshold::ParticipantIdentity;

    // Step 4: Execute guardian ceremony choreography (send proposals + collect responses)
    let guardian_ids = operation.guardian_ids.clone();
    let threshold = operation.threshold_k;
    let accepted_guardian_responses = recovery_service
        .execute_guardian_ceremony_initiator(
            ceremony_id_hash,
            prestate_hash,
            operation,
            guardian_ids,
            key_packages,
        )
        .await
        .map_err(|e| format!("Failed to execute guardian ceremony choreography: {e}"))?;

    // Step 5: Record accepted participants before committing
    for response in &accepted_guardian_responses {
        runner
            .record_verified_response(
                &ceremony_id,
                ParticipantIdentity::guardian(response.payload().guardian_id),
                response,
            )
            .await
            .map_err(|e| format!("Failed to record guardian acceptance: {e}"))?;
    }

    // Step 6: Mark ceremony as committed after successful choreography completion
    runner
        .commit(
            &ceremony_id,
            CeremonyCommitMetadata {
                committed_at: None,
                consensus_id: None,
            },
        )
        .await
        .map_err(|e| format!("Failed to commit ceremony: {e}"))?;

    tracing::info!(
        ceremony_id = %ceremony_id,
        "Guardian ceremony completed successfully"
    );

    // Step 7: Commit GuardianBinding facts for each accepted guardian.
    // This enables the ContactsSignalView to reflect guardian status in the UI.
    for response in &accepted_guardian_responses {
        let guardian_id = response.payload().guardian_id;
        let binding_fact = RelationalFact::Protocol(ProtocolRelationalFact::GuardianBinding {
            account_id: authority_id,
            guardian_id,
            binding_hash: Hash32::default(),
        });
        if let Err(e) = effects.commit_relational_facts(vec![binding_fact]).await {
            tracing::warn!(
                guardian_id = %guardian_id,
                error = %e,
                "Failed to commit GuardianBinding fact (UI may not reflect guardian status)"
            );
        } else {
            tracing::info!(
                guardian_id = %guardian_id,
                "Committed GuardianBinding fact"
            );
        }
    }

    // Step 8: Record setup completion so the recovery view shows the new
    // guardian set and threshold.
    {
        use aura_journal::DomainFact as _;
        let completed_at = effects
            .physical_time()
            .await
            .map_err(|e| format!("Failed to read time for guardian setup completion: {e}"))?;
        let completed = aura_recovery::RecoveryFact::GuardianSetupCompleted {
            context_id: crate::core::default_context_id_for_authority(authority_id),
            guardian_ids: accepted_guardian_responses
                .iter()
                .map(|response| response.payload().guardian_id)
                .collect(),
            trace_id: Some(ceremony_id.to_string()),
            threshold,
            completed_at,
        }
        .to_generic();
        if let Err(e) = effects.commit_relational_facts(vec![completed]).await {
            tracing::warn!(
                ceremony_id = %ceremony_id,
                error = %e,
                "Failed to commit GuardianSetupCompleted fact"
            );
        }
    }

    Ok(())
}

impl AgentRuntimeBridge {
    /// After a device enrollment is accepted, switch this device's signing state
    /// to the enrollment's epoch for the account it joined.
    ///
    /// The enrollment stores this device's share, threshold config and public
    /// key package for the pending epoch; until they are committed the device
    /// keeps the provisional keys it started with, so it cannot sign or open
    /// payloads sealed to its tree leaf.
    async fn adopt_enrolled_signing_epoch(
        &self,
        invitation_service: &crate::handlers::invitation_service::InvitationServiceApi,
        invitation_id: &aura_core::types::identifiers::InvitationId,
    ) -> Result<(), aura_app::runtime_bridge::RuntimeBridgeError> {
        let Some(invitation) = invitation_service.get(invitation_id).await else {
            return Err(error_boundary::bridge_runtime_internal(
                "Missing accepted invitation",
                aura_core::AuraError::not_found("accepted invitation unavailable"),
            ));
        };
        if !matches!(
            invitation.invitation_type,
            crate::handlers::invitation::InvitationType::DeviceEnrollment { .. }
        ) {
            return Ok(());
        }
        // The ID locates immutable secure evidence; cached status/received IDs
        // cannot authorize activation. The loader revalidates actual signed
        // committed confirmation under the original independent transfer pin.
        let confirmed =
            crate::handlers::invitation::enrollment_manifest_admission::load_confirmed_enrollment(
                self.agent.runtime().effects().as_ref(),
                invitation.receiver_id,
                invitation_id,
            )
            .await
            .map_err(|source| {
                error_boundary::bridge_runtime_internal(
                    "Load confirmed enrolled generation",
                    source,
                )
            })?;
        crate::runtime::services::enrollment_profile::complete_confirmed_handoff(
            self.agent.runtime().effects().as_ref(),
            &self.agent.runtime().threshold_signing(),
            confirmed,
        )
        .await
        .map_err(|source| {
            error_boundary::bridge_runtime_internal(
                "Activate confirmed enrolled signing generation",
                source,
            )
        })
    }

    /// Called after signing bootstrap restores the active context. No service-start
    /// readiness shortcut is used; pending generations are verified before tasks start.
    async fn restore_owned_device_enrollment_ceremonies(&self) -> Result<(), aura_core::AuraError> {
        let runtime = self.agent.runtime();
        let tracker = runtime.ceremony_tracker();
        let effects = runtime.effects();
        let signing = runtime.threshold_signing();
        effects.retire_unissued_enrollment_allocation().await?;
        for ceremony_id in tracker.list_device_enrollment_ceremonies().await? {
            if effects
                .secure_exists(&SecureStorageLocation::new(
                    "device_enrollment_orphan_retirement_v1",
                    ceremony_id.to_string(),
                ))
                .await?
            {
                tracker
                    .restore_retired_orphan_registration(&ceremony_id)
                    .await?;
                continue;
            }
            // Legacy outcome observations cannot construct a new active registration.
            if !effects
                .secure_exists(&SecureStorageLocation::new(
                    "device_enrollment_registration_v1",
                    ceremony_id.to_string(),
                ))
                .await?
            {
                continue;
            }
            if !tracker
                .restore_verified_enrollment_registration(&ceremony_id, &signing)
                .await?
            {
                continue;
            }
            let state = tracker.get(&ceremony_id).await?;
            if matches!(
                state.terminal_outcome,
                Some(aura_app::runtime_bridge::CeremonyTerminalOutcome::Failed(_))
            ) {
                tracker
                    .retire_failed_enrollment_generation(&ceremony_id)
                    .await?;
                if state.terminal_outcome
                    == Some(aura_app::runtime_bridge::CeremonyTerminalOutcome::Failed(
                        aura_app::runtime_bridge::CeremonyFailureReason::Cancelled,
                    ))
                {
                    let invitations = self.agent.invitations().map_err(|source| {
                        aura_core::AuraError::Internal {
                            message: "restore cancelled enrollment notice owner".into(),
                            source: Some(std::sync::Arc::new(source)),
                        }
                    })?;
                    let capability = invitations
                        .prepare_cancelled_enrollment_notice_recovery(&ceremony_id)
                        .await
                        .map_err(|source| match source {
                            crate::core::AgentError::Aura(cause) => cause,
                            source => aura_core::AuraError::Internal {
                                message: "restore cancelled enrollment notice owner".into(),
                                source: Some(std::sync::Arc::new(source)),
                            },
                        })?;
                    if let Some(capability) = capability {
                        invitations
                            .start_cancelled_enrollment_notice_recovery(capability)
                            .map_err(|source| match source {
                                crate::core::AgentError::Aura(cause) => cause,
                                source => aura_core::AuraError::Internal {
                                    message: "restore cancelled enrollment notice owner".into(),
                                    source: Some(std::sync::Arc::new(source)),
                                },
                            })?;
                    }
                }
                continue;
            }
            if state.terminal_outcome.is_some() {
                continue;
            }
            let registered_generation = effects
                .resume_owned_enrollment_registration(
                    tracker,
                    state.initiator_id,
                    state.new_epoch,
                    &ceremony_id,
                    state.prestate_hash,
                )
                .await?;
            let admission = self
                .agent
                .invitations()
                .map_err(|error| aura_core::AuraError::Internal {
                    message: "restore enrollment invitation owner".into(),
                    source: Some(std::sync::Arc::new(error)),
                })?
                .start_registered_device_enrollment(&registered_generation)
                .await
                .map_err(|error| aura_core::AuraError::Internal {
                    message: "resume registered enrollment initiator".into(),
                    source: Some(std::sync::Arc::new(error)),
                })?;
            if admission == crate::handlers::invitation_service::DeviceEnrollmentInitiatorStart::AlreadyRunning { continue; }
            let invitee = state
                .enrollment_device_id
                .ok_or_else(|| aura_core::AuraError::invalid("missing restored invitee"))?;
            let peers = state
                .participants
                .iter()
                .filter_map(|participant| match participant {
                    ParticipantIdentity::Device(device) if *device != invitee => Some(*device),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if peers.is_empty() {
                self.spawn_sole_device_enrollment_finalizer(ceremony_id);
            } else {
                let public_key_package = effects
                    .secure_retrieve(
                        &SecureStorageLocation::with_sub_key(
                            "threshold_pubkey",
                            state.initiator_id.to_string(),
                            state.new_epoch.to_string(),
                        ),
                        &[SecureStorageCapability::Read],
                    )
                    .await?;
                let threshold_config = effects
                    .secure_retrieve(
                        &SecureStorageLocation::with_sub_key(
                            "threshold_config",
                            state.initiator_id.to_string(),
                            state.new_epoch.to_string(),
                        ),
                        &[SecureStorageCapability::Read],
                    )
                    .await?;
                for device in peers {
                    let key_package = signing
                        .participant_key_package(
                            &state.initiator_id,
                            state.new_epoch,
                            &ParticipantIdentity::device(device),
                        )
                        .await?;
                    self.spawn_device_epoch_rotation(
                        crate::handlers::device_epoch_rotation::DeviceEpochRotationInitRequest {
                            ceremony_id: ceremony_id.clone(),
                            kind: aura_sync::protocols::DeviceEpochRotationKind::Enrollment,
                            pending_epoch: state.new_epoch,
                            participant_device_id: device,
                            key_package,
                            public_key_package: public_key_package.clone(),
                            threshold_config: threshold_config.clone(),
                        },
                    );
                }
            }
        }
        Ok(())
    }

    fn spawn_sole_device_enrollment_finalizer(
        &self,
        ceremony_id: aura_core::types::identifiers::CeremonyId,
    ) {
        let ceremony_runner = self.agent.runtime().ceremony_runner().clone();
        let service = crate::handlers::device_epoch_rotation::DeviceEpochRotationService::new(
            self.agent.authority_id(),
            self.agent.runtime().effects(),
            self.agent.runtime().ceremony_tracker().clone(),
            self.agent.runtime().ceremony_runner().clone(),
            self.agent.runtime().threshold_signing(),
            self.agent.runtime().reconfiguration().clone(),
        );
        let task_name = format!("device_enrollment_finalize.{ceremony_id}");
        let fut = async move {
            if let Err(error) = service.finalize_sole_device_enrollment(&ceremony_id).await {
                let reason = if error.is_timeout() {
                    aura_app::runtime_bridge::CeremonyFailureReason::TimedOut
                } else {
                    aura_app::runtime_bridge::CeremonyFailureReason::RuntimeFailed
                };
                if let Err(settle_error) = ceremony_runner
                    .fail_with_reason(&ceremony_id, reason, Some(error.to_string()))
                    .await
                {
                    tracing::warn!(
                        error = %settle_error,
                        ceremony_id = %ceremony_id,
                        "sole-device enrollment terminal outcome publication failed"
                    );
                }
                tracing::warn!(
                    error = %error,
                    ceremony_id = %ceremony_id,
                    "sole-device enrollment finalization failed"
                );
            }
        };
        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                let _task_handle = self.agent.runtime().tasks().spawn_local_named(task_name, fut);
            } else {
                let _task_handle = self.agent.runtime().tasks().spawn_named(task_name, fut);
            }
        }
    }

    fn spawn_device_epoch_rotation(
        &self,
        request: crate::handlers::device_epoch_rotation::DeviceEpochRotationInitRequest,
    ) {
        let service = crate::handlers::device_epoch_rotation::DeviceEpochRotationService::new(
            self.agent.authority_id(),
            self.agent.runtime().effects(),
            self.agent.runtime().ceremony_tracker().clone(),
            self.agent.runtime().ceremony_runner().clone(),
            self.agent.runtime().threshold_signing(),
            self.agent.runtime().reconfiguration().clone(),
        );
        let task_name = format!(
            "device_epoch_rotation.{}.{}",
            request.ceremony_id, request.participant_device_id
        );
        let ceremony_runner = self.agent.runtime().ceremony_runner().clone();
        // A failed initiator session ends the tracked ceremony with a typed
        // terminal instead of leaving it pending until its deadline.
        let fut = async move {
            let ceremony_id = request.ceremony_id.clone();
            if let Err(error) = service.execute_initiator(request).await {
                let reason = if error.is_timeout() {
                    aura_app::runtime_bridge::CeremonyFailureReason::TimedOut
                } else {
                    aura_app::runtime_bridge::CeremonyFailureReason::RuntimeFailed
                };
                tracing::warn!(error = ?error, ceremony_id = %ceremony_id, "device epoch rotation initiator failed");
                if let Err(settle_error) = ceremony_runner
                    .fail_with_reason(&ceremony_id, reason, Some(error.to_string()))
                    .await
                {
                    tracing::warn!(
                        error = %settle_error,
                        ceremony_id = %ceremony_id,
                        "device epoch rotation terminal outcome publication failed"
                    );
                }
            }
        };
        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                let _task_handle = self.agent.runtime().tasks().spawn_local_named(task_name, fut);
            } else {
                let _task_handle = self.agent.runtime().tasks().spawn_named(task_name, fut);
            }
        }
    }
}

// ============================================================================
// AuraAgent extension
// ============================================================================

impl AuraAgent {
    /// Get this agent as a RuntimeBridge
    ///
    /// This enables the dependency inversion pattern where `aura-app` defines
    /// the `RuntimeBridge` trait and `aura-agent` implements it.
    ///
    /// ## Example
    ///
    /// ```rust,ignore
    /// let agent = AgentBuilder::new()
    ///     .with_authority(authority_id)
    ///     .build_production(&ctx)
    ///     .await?;
    ///
    /// let app = AppCore::with_runtime(config, agent.as_runtime_bridge())?;
    /// ```
    pub fn as_runtime_bridge(self: Arc<Self>) -> Arc<dyn RuntimeBridge> {
        sync::start_periodic_sync(&self);
        Arc::new(AgentRuntimeBridge::new(self))
    }
}

// ============================================================================
#[allow(clippy::disallowed_types)]
#[cfg(test)]
pub(crate) mod tests {
    include!("tests.rs");
}

#[cfg(test)]
mod required_name_read_tests {
    use super::*;
    use std::error::Error as _;
    #[test]
    fn corrupt_and_wrong_context_committed_chat_records_fail_native_name_read() {
        let context = ContextId::new_from_entropy([0x54; 32]);
        let fact = ChatFact::channel_created_ms(
            context,
            ChannelId::from_bytes([0x55; 32]),
            "required-name".to_string(),
            None,
            false,
            1,
            AuthorityId::new_from_entropy([0x56; 32]),
        );
        let envelope = fact.to_envelope();
        assert_eq!(
            decode_required_name_chat_fact(context, &envelope).expect("matching committed context"),
            fact
        );
        let mut corrupt = envelope.clone();
        corrupt.payload = vec![0xff];
        let error = decode_required_name_chat_fact(context, &corrupt)
            .expect_err("corrupt required record cannot mean no name match");
        assert_eq!(
            error.kind(),
            aura_app::runtime_bridge::RuntimeBridgeErrorKind::Serialization
        );
        let mut current = error.source();
        let mut original = false;
        while let Some(source) = current {
            original |= source.is::<aura_core::util::serialization::SerializationError>();
            current = source.source();
        }
        assert!(original, "actual required codec failure retained");
        let error =
            decode_required_name_chat_fact(ContextId::new_from_entropy([0x57; 32]), &envelope)
                .expect_err("outer and payload context disagreement cannot identify a channel");
        assert_eq!(
            error.kind(),
            aura_app::runtime_bridge::RuntimeBridgeErrorKind::Validation
        );
        let mut current = error.source();
        let mut mismatch = false;
        while let Some(source) = current {
            mismatch |= matches!(
                source.downcast_ref::<aura_core::types::facts::FactError>(),
                Some(aura_core::types::facts::FactError::InvalidEnvelope(_))
            );
            current = source.source();
        }
        assert!(mismatch);
    }
}
