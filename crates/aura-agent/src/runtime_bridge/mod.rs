//! RuntimeBridge implementation for AuraAgent
//!
//! This module implements the `RuntimeBridge` trait from `aura-app` for `AuraAgent`,
//! enabling the dependency inversion where `aura-app` defines the trait and
//! `aura-agent` provides the implementation.

use crate::core::default_context_id_for_authority;
use crate::core::AuraAgent;
use crate::handlers::shared::context_commitment_from_journal;
use crate::runtime::consensus::build_consensus_params;
use crate::runtime::services::ceremony_runner::{CeremonyCommitMetadata, CeremonyInitRequest};
use crate::runtime::services::ServiceError;
use async_trait::async_trait;
use aura_app::runtime_bridge::{
    BridgeAuthorityInfo, BridgeDeviceInfo, InvitationInfo, LanPeerInfo, RendezvousStatus,
    RuntimeBridge, SettingsBridgeState, SyncStatus,
};
use aura_app::signal_defs::INVITATIONS_SIGNAL;
use aura_app::ui::workflows::authority::{authority_key_prefix, deserialize_authority};
use aura_app::views::invitations::InvitationStatus;
use aura_app::IntentError;
use aura_app::ReactiveHandler;
use aura_chat::{ChatFact, CHAT_FACT_TYPE_ID};
use aura_core::ceremony::SupersessionReason;
use aura_core::effects::{
    amp::{
        AmpChannelEffects, AmpCiphertext, ChannelBootstrapPackage, ChannelCloseParams,
        ChannelCreateParams, ChannelJoinParams, ChannelLeaveParams, ChannelSendParams,
    },
    random::RandomCoreEffects,
    reactive::ReactiveEffects,
    time::PhysicalTimeEffects,
    SecureStorageCapability, SecureStorageEffects, SecureStorageLocation, StorageCoreEffects,
    ThresholdSigningEffects, TransportEffects, TransportEnvelope,
};
use aura_core::hash::hash;
use aura_core::threshold::{AgreementMode, SigningContext, ThresholdConfig, ThresholdSignature};
use aura_core::tree::{AttestedOp, LeafRole, TreeOp};
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_core::types::{Epoch, FrostThreshold};
use aura_core::DeviceId;
use aura_core::EffectContext;
use aura_core::Hash32;
use aura_core::OwnedTaskSpawner;
use aura_core::Prestate;
use aura_core::{execute_with_timeout_budget, TimeoutBudget, TimeoutRunError};
use aura_journal::fact::{
    ChannelBootstrap, ChannelBumpReason, FactOptions, ProposedChannelEpochBump, RelationalFact,
};
use aura_journal::fact::{Fact as TypedFact, FactContent};
use aura_journal::DomainFact;
use aura_journal::ProtocolRelationalFact;
use aura_protocol::amp::{
    commit_bump_with_consensus, emit_proposed_bump, AmpJournalEffects, ChannelMembershipFact,
    ChannelParticipantEvent,
};
use aura_protocol::effects::TreeEffects;
use aura_social::moderation::facts::{HomePinFact, HomeUnpinFact};
use aura_social::moderation::{
    HomeBanFact, HomeKickFact, HomeMuteFact, HomeUnbanFact, HomeUnmuteFact,
};
use futures::{SinkExt, StreamExt};
#[cfg(target_arch = "wasm32")]
use gloo_net::websocket::{futures::WebSocket, Message};
use std::collections::{BTreeSet, HashSet};
use std::sync::Arc;
use std::time::Duration;
#[cfg(not(target_arch = "wasm32"))]
use tokio_tungstenite::connect_async;
#[cfg(not(target_arch = "wasm32"))]
use tokio_tungstenite::tungstenite::Message;

mod amp;
mod consensus;
mod invitation;
mod recovery;
mod rendezvous;
mod settings;

use amp::map_amp_error;
use consensus::{map_consensus_error, persist_consensus_dkg_transcript};
use invitation::convert_invitation_to_bridge_info;

const CHAT_FACT_CONTENT_TYPE: &str = "application/aura-chat-fact";
const FACT_SYNC_REQUEST_CONTENT_TYPE: &str = "application/aura-fact-sync-request";
const FACT_SYNC_RESPONSE_CONTENT_TYPE: &str = "application/aura-fact-sync-response";
const DEFAULT_HARNESS_SYNC_ROUNDS: usize = 3;
const DEFAULT_HARNESS_SYNC_BACKOFF_MS: u64 = 75;
const INVITATION_BRIDGE_STAGE_TIMEOUT_MS: u64 = 8_000;
const AMP_REPAIR_MEMBERSHIP_STAGE_TIMEOUT_MS: u64 = 1_000;

async fn execute_with_effect_timeout<TTime, T, E, Fut>(
    time: &TTime,
    timeout: Duration,
    operation: Fut,
) -> Result<T, TimeoutRunError<E>>
where
    TTime: PhysicalTimeEffects + Sync,
    Fut: std::future::Future<Output = Result<T, E>>,
{
    let started_at = time.physical_time().await.map_err(|error| {
        TimeoutRunError::Timeout(aura_core::TimeoutBudgetError::time_source_unavailable(
            error.to_string(),
        ))
    })?;
    let budget = TimeoutBudget::from_start_and_timeout(&started_at, timeout)
        .map_err(TimeoutRunError::Timeout)?;
    execute_with_timeout_budget(time, &budget, || operation).await
}

#[derive(Debug, Default)]
struct ChannelFactInspection {
    checkpoint_exists: bool,
    bootstrap: Option<ChannelBootstrap>,
}

async fn inspect_channel_context_facts(
    effects: &crate::runtime::AuraEffectSystem,
    context: ContextId,
    channel: ChannelId,
) -> Result<ChannelFactInspection, IntentError> {
    let journal = effects
        .fetch_context_journal(context)
        .await
        .map_err(|error| {
            IntentError::internal_error(format!("AMP context journal lookup failed: {error}"))
        })?;

    let mut inspection = ChannelFactInspection::default();
    for fact in journal.iter_facts() {
        let FactContent::Relational(RelationalFact::Protocol(protocol_fact)) = &fact.content else {
            continue;
        };
        match protocol_fact {
            ProtocolRelationalFact::AmpChannelCheckpoint(checkpoint)
                if checkpoint.context == context && checkpoint.channel == channel =>
            {
                inspection.checkpoint_exists = true;
            }
            ProtocolRelationalFact::AmpChannelBootstrap(bootstrap)
                if bootstrap.context == context && bootstrap.channel == channel =>
            {
                inspection.bootstrap = Some(bootstrap.clone());
            }
            _ => {}
        }
    }

    Ok(inspection)
}

async fn resolve_channel_context_from_local_chat_facts(
    effects: &crate::runtime::AuraEffectSystem,
    authority: AuthorityId,
    channel: ChannelId,
) -> Result<Option<ContextId>, IntentError> {
    let facts = effects
        .load_committed_facts(authority)
        .await
        .map_err(|error| {
            IntentError::internal_error(format!(
                "failed to load committed facts for channel resolution: {error}"
            ))
        })?;

    for fact in facts.into_iter().rev() {
        let FactContent::Relational(RelationalFact::Generic { envelope, .. }) = fact.content else {
            continue;
        };

        if envelope.type_id.as_str() != CHAT_FACT_TYPE_ID {
            continue;
        }

        let Some(ChatFact::ChannelCreated {
            context_id,
            channel_id,
            creator_id,
            ..
        }) = ChatFact::from_envelope(&envelope)
        else {
            continue;
        };

        if channel_id == channel && creator_id == authority {
            return Ok(Some(context_id));
        }
    }

    Ok(None)
}

fn service_error_to_intent(err: ServiceError) -> IntentError {
    IntentError::service_error(err.to_string())
}

fn service_unavailable(service: &'static str) -> IntentError {
    service_error_to_intent(ServiceError::unavailable(service, "service unavailable"))
}

fn service_unavailable_with_detail(
    service: &'static str,
    detail: impl std::fmt::Display,
) -> IntentError {
    service_error_to_intent(ServiceError::unavailable(service, format!("{detail}")))
}

fn harness_mode_enabled() -> bool {
    std::env::var_os("AURA_HARNESS_MODE").is_some()
}

fn harness_sync_rounds() -> usize {
    std::env::var("AURA_HARNESS_SYNC_ROUNDS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|rounds| *rounds > 0)
        .unwrap_or(DEFAULT_HARNESS_SYNC_ROUNDS)
}

fn harness_sync_backoff_ms() -> u64 {
    std::env::var("AURA_HARNESS_SYNC_BACKOFF_MS")
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

    async fn seed_sync_peers_from_rendezvous(&self) {
        if let (Some(sync), Some(rendezvous)) = (
            self.agent.runtime().sync(),
            self.agent.runtime().rendezvous(),
        ) {
            for peer_device in rendezvous.list_reachable_peer_devices().await {
                sync.add_peer(peer_device).await;
            }
        }
    }

    async fn sync_seeded_peers(&self) -> Result<(), IntentError> {
        let Some(sync) = self.agent.runtime().sync() else {
            return Ok(());
        };
        let peers = sync.peers().await;
        if peers.is_empty() {
            return Ok(());
        }
        let effects = self.agent.runtime().effects();
        sync.sync_with_peers(&effects, peers)
            .await
            .map_err(|e| IntentError::internal_error(format!("Sync failed: {e}")))
    }

    async fn refresh_reachability_after_ceremony_processing(&self) -> Result<(), IntentError> {
        let rounds = if harness_mode_enabled() {
            harness_sync_rounds()
        } else {
            1
        };
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
                self.sleep_ms(backoff_ms).await;
            }
        }

        match last_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    async fn pull_remote_relational_facts(&self, peer: AuthorityId) -> Result<usize, IntentError> {
        tracing::info!(peer = %peer, "pull_remote_relational_facts start");
        let Some(rendezvous) = self.agent.runtime().rendezvous() else {
            tracing::info!(peer = %peer, "pull_remote_relational_facts skipped: no rendezvous");
            return Ok(0);
        };

        let context = default_context_id_for_authority(peer);
        let mut descriptor = rendezvous.get_descriptor(context, peer).await;
        if descriptor.as_ref().map_or(true, |descriptor| {
            !descriptor
                .transport_hints
                .iter()
                .any(|hint| matches!(hint, aura_rendezvous::TransportHint::WebSocketDirect { .. }))
        }) {
            let lan_descriptor = rendezvous
                .list_lan_discovered_peers()
                .await
                .into_iter()
                .find(|candidate| candidate.authority_id == peer)
                .map(|candidate| candidate.descriptor);
            if lan_descriptor.is_some() {
                descriptor = lan_descriptor;
            }
        }
        let Some(descriptor) = descriptor else {
            tracing::info!(peer = %peer, context = %context, "pull_remote_relational_facts skipped: no descriptor");
            return Ok(0);
        };

        let addr: Option<String> = descriptor
            .transport_hints
            .iter()
            .find_map(|hint| match hint {
                aura_rendezvous::TransportHint::WebSocketDirect { addr, .. } => {
                    Some(addr.to_string())
                }
                _ => None,
            });
        let Some(addr) = addr else {
            tracing::info!(peer = %peer, "pull_remote_relational_facts skipped: no websocket direct hint");
            return Ok(0);
        };

        let url = if addr.starts_with("ws://") || addr.starts_with("wss://") {
            addr
        } else {
            format!("ws://{addr}")
        };

        let request = TransportEnvelope {
            destination: peer,
            source: self.agent.authority_id(),
            context,
            payload: Vec::new(),
            metadata: std::collections::HashMap::from([(
                "content-type".to_string(),
                FACT_SYNC_REQUEST_CONTENT_TYPE.to_string(),
            )]),
            receipt: None,
        };

        let bytes = aura_core::util::serialization::to_vec(&request).map_err(|e| {
            IntentError::internal_error(format!("Failed to encode fact sync request: {e}"))
        })?;

        #[cfg(target_arch = "wasm32")]
        let remote_facts: Vec<RelationalFact> = run_local_ws(move || async move {
            let mut socket = WebSocket::open(&url).map_err(|e| {
                IntentError::network_error(format!("Failed to open fact sync websocket {url}: {e}"))
            })?;
            socket.send(Message::Bytes(bytes)).await.map_err(|e| {
                IntentError::network_error(format!("Failed to send fact sync request: {e}"))
            })?;

            let response = socket.next().await.ok_or_else(|| {
                IntentError::network_error("Fact sync websocket closed before response".to_string())
            })?;
            let payload = match response.map_err(|e| {
                IntentError::network_error(format!("Fact sync websocket read failed: {e}"))
            })? {
                Message::Bytes(payload) => payload,
                _ => {
                    return Err(IntentError::network_error(
                        "Fact sync websocket returned non-binary payload".to_string(),
                    ));
                }
            };

            let envelope: TransportEnvelope = aura_core::util::serialization::from_slice(&payload)
                .map_err(|e| {
                    IntentError::internal_error(format!("Failed to decode fact sync response: {e}"))
                })?;

            if envelope
                .metadata
                .get("content-type")
                .map_or(true, |value| value != FACT_SYNC_RESPONSE_CONTENT_TYPE)
            {
                return Err(IntentError::network_error(
                    "Fact sync response had unexpected content type".to_string(),
                ));
            }

            aura_core::util::serialization::from_slice(&envelope.payload).map_err(|e| {
                IntentError::internal_error(format!("Failed to decode fact sync payload: {e}"))
            })
        })
        .await?;

        #[cfg(not(target_arch = "wasm32"))]
        let remote_facts: Vec<RelationalFact> = {
            let (mut socket, _) = connect_async(&url).await.map_err(|e| {
                IntentError::network_error(format!("Failed to open fact sync websocket {url}: {e}"))
            })?;
            socket.send(Message::Binary(bytes)).await.map_err(|e| {
                IntentError::network_error(format!("Failed to send fact sync request: {e}"))
            })?;

            let response = socket.next().await.ok_or_else(|| {
                IntentError::network_error("Fact sync websocket closed before response".to_string())
            })?;
            let payload = match response.map_err(|e| {
                IntentError::network_error(format!("Fact sync websocket read failed: {e}"))
            })? {
                Message::Binary(payload) => payload,
                _ => {
                    return Err(IntentError::network_error(
                        "Fact sync websocket returned non-binary payload".to_string(),
                    ));
                }
            };

            let envelope: TransportEnvelope = aura_core::util::serialization::from_slice(&payload)
                .map_err(|e| {
                    IntentError::internal_error(format!("Failed to decode fact sync response: {e}"))
                })?;

            if envelope
                .metadata
                .get("content-type")
                .map_or(true, |value| value != FACT_SYNC_RESPONSE_CONTENT_TYPE)
            {
                return Err(IntentError::network_error(
                    "Fact sync response had unexpected content type".to_string(),
                ));
            }

            aura_core::util::serialization::from_slice(&envelope.payload).map_err(|e| {
                IntentError::internal_error(format!("Failed to decode fact sync payload: {e}"))
            })?
        };

        tracing::info!(
            peer = %peer,
            fact_count = remote_facts.len(),
            "pull_remote_relational_facts response"
        );

        if remote_facts.is_empty() {
            return Ok(0);
        }

        let effects = self.agent.runtime().effects();
        let local = effects
            .load_committed_facts(self.agent.authority_id())
            .await
            .map_err(|e| IntentError::internal_error(format!("Failed to load local facts: {e}")))?;

        let mut known = HashSet::new();
        for fact in local {
            let TypedFact { content, .. } = fact;
            if let FactContent::Relational(rel) = content {
                if let Ok(bytes) = aura_core::util::serialization::to_vec(&rel) {
                    known.insert(bytes);
                }
            }
        }

        let mut new_facts = Vec::new();
        for fact in remote_facts {
            let bytes = aura_core::util::serialization::to_vec(&fact).map_err(|e| {
                IntentError::internal_error(format!("Failed to encode relational fact: {e}"))
            })?;
            if known.insert(bytes) {
                new_facts.push(fact);
            }
        }

        if new_facts.is_empty() {
            tracing::info!(peer = %peer, "pull_remote_relational_facts no new facts");
            return Ok(0);
        }

        effects
            .commit_relational_facts(new_facts.clone())
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to merge synced facts: {e}"))
            })?;
        effects.await_next_view_update().await;

        tracing::info!(
            peer = %peer,
            committed = new_facts.len(),
            "pull_remote_relational_facts committed"
        );

        Ok(new_facts.len())
    }
}

#[cfg(target_arch = "wasm32")]
async fn run_local_ws<Mk, Fut, T>(make_fut: Mk) -> Result<T, IntentError>
where
    Mk: FnOnce() -> Fut + 'static,
    Fut: core::future::Future<Output = Result<T, IntentError>> + 'static,
    T: 'static,
{
    make_fut().await
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

    fn task_spawner(&self) -> Option<OwnedTaskSpawner> {
        Some(self.agent.runtime().task_spawner())
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
            .map_err(|e| IntentError::internal_error(format!("Failed to commit facts: {e}")))?;

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
            .map_err(|e| IntentError::internal_error(format!("Failed to commit facts: {e}")))?;

        Ok(())
    }

    async fn send_chat_fact(
        &self,
        peer: AuthorityId,
        context: ContextId,
        fact: &RelationalFact,
    ) -> Result<(), IntentError> {
        let payload = aura_core::util::serialization::to_vec(fact).map_err(|e| {
            IntentError::internal_error(format!("Failed to serialize chat fact envelope: {e}"))
        })?;

        let mut metadata = std::collections::HashMap::new();
        metadata.insert(
            "content-type".to_string(),
            CHAT_FACT_CONTENT_TYPE.to_string(),
        );
        metadata.insert("target-authority-id".to_string(), peer.to_string());

        let effects = self.agent.runtime().effects();
        let reachable_devices = if let Some(rendezvous) = self.agent.runtime().rendezvous() {
            rendezvous
                .list_reachable_peer_devices_for_authority(peer)
                .await
        } else {
            Vec::new()
        };

        if reachable_devices.is_empty() {
            let envelope = TransportEnvelope {
                destination: peer,
                source: self.agent.authority_id(),
                context,
                payload,
                metadata,
                receipt: None,
            };

            tracing::debug!(
                source = %self.agent.authority_id(),
                destination = %peer,
                context = %context,
                mode = "authority_fallback",
                "send-chat-fact"
            );

            return effects
                .send_envelope(envelope)
                .await
                .map_err(|e| IntentError::network_error(format!("Failed to send chat fact: {e}")));
        }

        let mut last_error = None;
        let mut sent = 0usize;
        for device_id in reachable_devices {
            let mut device_metadata = metadata.clone();
            device_metadata.insert(
                "aura-destination-device-id".to_string(),
                device_id.to_string(),
            );
            let envelope = TransportEnvelope {
                destination: peer,
                source: self.agent.authority_id(),
                context,
                payload: payload.clone(),
                metadata: device_metadata,
                receipt: None,
            };

            tracing::debug!(
                source = %self.agent.authority_id(),
                destination = %envelope.destination,
                context = %context,
                target_authority = %peer,
                device_id = %device_id,
                mode = "device_route",
                "send-chat-fact"
            );

            match effects.send_envelope(envelope).await {
                Ok(()) => {
                    sent = sent.saturating_add(1);
                }
                Err(error) => {
                    last_error = Some(error.to_string());
                }
            }
        }

        if sent > 0 {
            Ok(())
        } else {
            Err(IntentError::network_error(format!(
                "Failed to send chat fact to any reachable device for {peer}: {}",
                last_error.unwrap_or_else(|| "no reachable device route succeeded".to_string())
            )))
        }
    }

    // =========================================================================
    // AMP Channel Operations
    // =========================================================================

    async fn amp_create_channel(
        &self,
        params: ChannelCreateParams,
    ) -> Result<ChannelId, IntentError> {
        let effects = self.agent.runtime().effects();
        effects.create_channel(params).await.map_err(map_amp_error)
    }

    async fn amp_create_channel_bootstrap(
        &self,
        context: ContextId,
        channel: ChannelId,
        recipients: Vec<AuthorityId>,
    ) -> Result<ChannelBootstrapPackage, IntentError> {
        if recipients.is_empty() {
            return Err(IntentError::internal_error(
                "bootstrap recipients cannot be empty".to_string(),
            ));
        }

        let effects = self.agent.runtime().effects();
        let inspection = inspect_channel_context_facts(&effects, context, channel).await?;

        let mut requested_recipients = BTreeSet::new();
        for recipient in recipients {
            requested_recipients.insert(recipient);
        }

        if let Some(existing) = inspection.bootstrap.clone() {
            if !requested_recipients.is_empty() {
                let existing_recipients: BTreeSet<_> =
                    existing.recipients.iter().copied().collect();
                if !requested_recipients.is_subset(&existing_recipients) {
                    return Err(IntentError::validation_failed(
                        "AMP bootstrap already exists; refusing to add new recipients (late joiners cannot receive bootstrap keys)",
                    ));
                }
            }

            let location = SecureStorageLocation::amp_bootstrap_key(
                &context,
                &channel,
                &existing.bootstrap_id,
            );
            let key = effects
                .secure_retrieve(&location, &[SecureStorageCapability::Read])
                .await
                .map_err(|e| {
                    IntentError::internal_error(format!("Failed to load AMP bootstrap key: {e}"))
                })?;
            if key.len() != 32 {
                return Err(IntentError::internal_error(format!(
                    "AMP bootstrap key has invalid length: {}",
                    key.len()
                )));
            }

            return Ok(ChannelBootstrapPackage {
                bootstrap_id: existing.bootstrap_id,
                key,
            });
        }

        if !inspection.checkpoint_exists {
            return Err(IntentError::internal_error(format!(
                "AMP channel checkpoint unavailable for bootstrap in context {context}"
            )));
        }

        let key_bytes = effects.random_bytes_32().await;
        let bootstrap_id = Hash32::from_bytes(&key_bytes);

        let location = SecureStorageLocation::amp_bootstrap_key(&context, &channel, &bootstrap_id);
        effects
            .secure_store(
                &location,
                &key_bytes,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to store AMP bootstrap key: {e}"))
            })?;

        let now = effects
            .physical_time()
            .await
            .map_err(|e| IntentError::internal_error(format!("Failed to read time: {e}")))?;

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
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to commit AMP bootstrap fact: {e}"))
            })?;

        Ok(ChannelBootstrapPackage {
            bootstrap_id,
            key: key_bytes.to_vec(),
        })
    }

    async fn amp_channel_state_exists(
        &self,
        context: ContextId,
        channel: ChannelId,
    ) -> Result<bool, IntentError> {
        let effects = self.agent.runtime().effects();
        Ok(inspect_channel_context_facts(&effects, context, channel)
            .await?
            .checkpoint_exists)
    }

    async fn resolve_amp_channel_context(
        &self,
        channel: ChannelId,
    ) -> Result<Option<ContextId>, IntentError> {
        let effects = self.agent.runtime().effects();
        let authority = self.agent.authority_id();

        if let Some(context) =
            resolve_channel_context_from_local_chat_facts(&effects, authority, channel).await?
        {
            if inspect_channel_context_facts(&effects, context, channel)
                .await?
                .checkpoint_exists
            {
                return Ok(Some(context));
            }
        }

        let contexts = self
            .agent
            .runtime()
            .contexts()
            .list_contexts_for_authority(authority)
            .await
            .map_err(|error| {
                IntentError::internal_error(format!(
                    "failed to list registered contexts for channel resolution: {error}"
                ))
            })?;

        for context in contexts {
            if inspect_channel_context_facts(&effects, context, channel)
                .await?
                .checkpoint_exists
            {
                return Ok(Some(context));
            }
        }

        Ok(None)
    }

    async fn amp_repair_local_channel_membership(
        &self,
        params: ChannelJoinParams,
    ) -> Result<(), IntentError> {
        let effects = self.agent.runtime().effects();
        let timestamp = execute_with_effect_timeout(
            &effects,
            Duration::from_millis(AMP_REPAIR_MEMBERSHIP_STAGE_TIMEOUT_MS),
            async { Ok::<_, IntentError>(ChannelMembershipFact::random_timestamp(&effects).await) },
        )
        .await
        .map_err(|error| match error {
            TimeoutRunError::Timeout(_) => IntentError::internal_error(format!(
                "amp_repair_local_channel_membership.random_timestamp timed out after {AMP_REPAIR_MEMBERSHIP_STAGE_TIMEOUT_MS}ms"
            )),
            TimeoutRunError::Operation(error) => error,
        })?;
        let membership = ChannelMembershipFact::new(
            params.context,
            params.channel,
            params.participant,
            ChannelParticipantEvent::Joined,
            timestamp,
        );
        execute_with_effect_timeout(
            &effects,
            Duration::from_millis(AMP_REPAIR_MEMBERSHIP_STAGE_TIMEOUT_MS),
            effects.insert_relational_fact(membership.to_generic()),
        )
        .await
        .map_err(|error| match error {
            TimeoutRunError::Timeout(_) => IntentError::internal_error(format!(
                "amp_repair_local_channel_membership.insert_relational_fact timed out after {AMP_REPAIR_MEMBERSHIP_STAGE_TIMEOUT_MS}ms"
            )),
            TimeoutRunError::Operation(error) => {
                IntentError::internal_error(format!(
                    "failed to repair local AMP membership: {error}"
                ))
            }
        })
    }

    async fn amp_close_channel(&self, params: ChannelCloseParams) -> Result<(), IntentError> {
        let effects = self.agent.runtime().effects();
        effects.close_channel(params).await.map_err(map_amp_error)
    }

    async fn amp_join_channel(&self, params: ChannelJoinParams) -> Result<(), IntentError> {
        let effects = self.agent.runtime().effects();
        effects.join_channel(params).await.map_err(map_amp_error)
    }

    async fn amp_leave_channel(&self, params: ChannelLeaveParams) -> Result<(), IntentError> {
        let effects = self.agent.runtime().effects();
        effects.leave_channel(params).await.map_err(map_amp_error)
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
            .map_err(|e| IntentError::internal_error(format!("AMP state lookup failed: {e}")))?;
        let bump_nonce = effects.random_bytes(32).await;
        let bump_id = Hash32(hash(&bump_nonce));
        let proposal = ProposedChannelEpochBump {
            context,
            channel,
            parent_epoch: state.chan_epoch,
            new_epoch: state.chan_epoch + 1,
            bump_id,
            reason: ChannelBumpReason::Routine,
        };

        emit_proposed_bump(effects.as_ref(), proposal.clone())
            .await
            .map_err(|e| IntentError::internal_error(format!("AMP proposal failed: {e}")))?;

        let policy =
            aura_core::threshold::policy_for(aura_core::threshold::CeremonyFlow::AmpEpochBump);
        if policy.allows_mode(AgreementMode::ConsensusFinalized) {
            let tree_state = effects.get_current_state().await.map_err(|e| {
                IntentError::internal_error(format!("Tree state lookup failed: {e}"))
            })?;
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
            .map_err(|e| IntentError::internal_error(format!("Invalid AMP prestate: {e}")))?;

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
            .map_err(|e| IntentError::internal_error(format!("AMP finalize failed: {e}")))?;
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
    ) -> Result<AmpCiphertext, IntentError> {
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
        let now = effects
            .physical_time()
            .await
            .map_err(|e| IntentError::internal_error(format!("Failed to read time: {e}")))?;

        let fact = HomeKickFact::new_ms(
            context_id,
            channel_id,
            target,
            self.agent.authority_id(),
            reason.unwrap_or_default(),
            now.ts_ms,
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
        let now = effects
            .physical_time()
            .await
            .map_err(|e| IntentError::internal_error(format!("Failed to read time: {e}")))?;

        let fact = HomeBanFact::new_ms(
            context_id,
            None,
            target,
            self.agent.authority_id(),
            reason.unwrap_or_default(),
            now.ts_ms,
            None,
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
        let now = effects
            .physical_time()
            .await
            .map_err(|e| IntentError::internal_error(format!("Failed to read time: {e}")))?;

        let fact = HomeUnbanFact::new_ms(
            context_id,
            None,
            target,
            self.agent.authority_id(),
            now.ts_ms,
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
        let now = effects
            .physical_time()
            .await
            .map_err(|e| IntentError::internal_error(format!("Failed to read time: {e}")))?;
        let expires_at = duration_secs.map(|s| now.ts_ms.saturating_add(s.saturating_mul(1000)));

        let fact = HomeMuteFact::new_ms(
            context_id,
            None,
            target,
            self.agent.authority_id(),
            duration_secs,
            now.ts_ms,
            expires_at,
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
        let now = effects
            .physical_time()
            .await
            .map_err(|e| IntentError::internal_error(format!("Failed to read time: {e}")))?;

        let fact = HomeUnmuteFact::new_ms(
            context_id,
            None,
            target,
            self.agent.authority_id(),
            now.ts_ms,
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
        let now = effects
            .physical_time()
            .await
            .map_err(|e| IntentError::internal_error(format!("Failed to read time: {e}")))?;

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
        let now = effects
            .physical_time()
            .await
            .map_err(|e| IntentError::internal_error(format!("Failed to read time: {e}")))?;

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
        // "Connected peers" is a UI-facing availability signal. It should reflect
        // currently reachable peers (e.g., contacts/devices online), not merely the
        // configured peer list.
        //
        // For now, we approximate this via TransportEffects active channel count, which
        // is supported in shared-transport simulation/demos and can be implemented by
        // production transports as they mature.
        let effects = self.agent.runtime().effects();
        let transport_stats = effects.get_transport_stats().await;

        let (is_running, active_sessions, last_sync_ms) =
            if let Some(sync) = self.agent.runtime().sync() {
                let health = sync.sync_service_health().await;
                (
                    sync.is_running().await,
                    health.as_ref().map(|h| h.active_sessions).unwrap_or(0),
                    health.and_then(|h| h.last_sync),
                )
            } else {
                let now_ms = self
                    .agent
                    .runtime()
                    .effects()
                    .physical_time()
                    .await
                    .map(|time| time.ts_ms)
                    .ok();
                (
                    false,
                    0,
                    now_ms.filter(|_| transport_stats.active_channels > 0),
                )
            };

        Ok(SyncStatus {
            is_running,
            connected_peers: (transport_stats.active_channels as usize)
                .max(active_sessions as usize),
            last_sync_ms,
            pending_facts: 0, // Would need to track this in SyncServiceManager
            active_sessions: active_sessions as usize,
        })
    }

    async fn is_peer_online(&self, peer: AuthorityId) -> bool {
        // Drive inbox processing opportunistically so background-less runtimes
        // still respond to key-rotation/device-enrollment messages.
        if let Err(e) = self.agent.process_ceremony_acceptances().await {
            tracing::debug!("Failed to process ceremony acceptances: {}", e);
        }

        let effects = self.agent.runtime().effects();
        let context = EffectContext::with_authority(self.agent.authority_id()).context_id();

        if effects.is_channel_established(context, peer).await {
            return true;
        }

        if let Some(rendezvous) = self.agent.runtime().rendezvous() {
            if rendezvous.get_descriptor(context, peer).await.is_some() {
                return true;
            }

            let fallback_context = default_context_id_for_authority(peer);
            if fallback_context != context
                && rendezvous
                    .get_descriptor(fallback_context, peer)
                    .await
                    .is_some()
            {
                return true;
            }

            // Fallback: check if peer is discovered on LAN (handles descriptor cache
            // eviction or timing races where the descriptor hasn't been dual-cached yet)
            if rendezvous.get_lan_discovered_peer(peer).await.is_some() {
                return true;
            }
        }

        false
    }
    async fn try_get_sync_peers(&self) -> Result<Vec<DeviceId>, IntentError> {
        Ok(if let Some(sync) = self.agent.runtime().sync() {
            sync.peers().await
        } else {
            Vec::new()
        })
    }

    async fn trigger_sync(&self) -> Result<(), IntentError> {
        if let Some(sync) = self.agent.runtime().sync() {
            let effects = self.agent.runtime().effects();
            let rounds = if harness_mode_enabled() {
                harness_sync_rounds()
            } else {
                1
            };
            let backoff_ms = harness_sync_backoff_ms();
            let mut last_sync_error: Option<IntentError> = None;

            for round in 0..rounds {
                if harness_mode_enabled() {
                    let _ = rendezvous::trigger_discovery(self).await;
                }

                self.seed_sync_peers_from_rendezvous().await;

                let authority_peers: Vec<AuthorityId> =
                    if let Some(rendezvous) = self.agent.runtime().rendezvous() {
                        let mut peers = rendezvous.list_cached_peers().await;
                        if peers.is_empty() {
                            peers = rendezvous
                                .list_lan_discovered_peers()
                                .await
                                .into_iter()
                                .map(|peer| peer.authority_id)
                                .collect();
                        }
                        peers.sort();
                        peers.dedup();
                        peers
                    } else {
                        Vec::new()
                    };
                let peers = sync.peers().await;

                let sync_result = if peers.is_empty() {
                    Ok(())
                } else {
                    sync.sync_with_peers(&effects, peers)
                        .await
                        .map_err(|e| IntentError::internal_error(format!("Sync failed: {e}")))
                };

                let mut pull_error: Option<IntentError> = None;
                for peer in authority_peers {
                    if let Err(error) = self.pull_remote_relational_facts(peer).await {
                        tracing::warn!(peer = %peer, error = %error, "fact sync pull failed");
                        if pull_error.is_none() {
                            pull_error = Some(error);
                        }
                    }
                }

                if let Some(error) = pull_error {
                    last_sync_error = Some(error);
                }

                match sync_result {
                    Ok(()) => {
                        if last_sync_error.is_none() {
                            return Ok(());
                        }
                    }
                    Err(error) => last_sync_error = Some(error),
                }

                if round + 1 < rounds {
                    self.sleep_ms(backoff_ms).await;
                }
            }

            match last_sync_error {
                Some(error) => Err(error),
                None => Ok(()),
            }
        } else {
            Err(service_unavailable("sync_service"))
        }
    }

    async fn process_ceremony_messages(&self) -> Result<(), IntentError> {
        let (processed_acceptances, processed_completions) = self
            .agent
            .process_ceremony_acceptances()
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to process ceremony messages: {e}"))
            })?;
        let invitation_handler = crate::handlers::invitation::InvitationHandler::new(
            crate::core::AuthorityContext::new_with_device(
                self.agent.authority_id(),
                self.agent.runtime().device_id(),
            ),
        )
        .map_err(|e| {
            IntentError::internal_error(format!(
                "Failed to create invitation handler for inbox processing: {e}"
            ))
        })?;
        let processed_contact_messages = invitation_handler
            .process_contact_invitation_acceptances(self.agent.runtime().effects())
            .await
            .map_err(|e| {
                IntentError::internal_error(format!(
                    "Failed to process contact/chat envelopes: {e}"
                ))
            })?;
        let processed_handshakes =
            if let Some(rendezvous_manager) = self.agent.runtime().rendezvous() {
                let authority = self.agent.context().clone();
                let handler = crate::handlers::rendezvous::RendezvousHandler::new(authority)
                    .map_err(|e| {
                        IntentError::internal_error(format!(
                            "Failed to create rendezvous handler for handshake processing: {e}"
                        ))
                    })?
                    .with_rendezvous_manager((*rendezvous_manager).clone());
                handler
                    .process_handshake_envelopes(self.agent.runtime().effects())
                    .await
                    .map_err(|e| {
                        IntentError::internal_error(format!(
                            "Failed to process rendezvous handshakes: {e}"
                        ))
                    })?
            } else {
                0
            };

        if processed_acceptances > 0
            || processed_completions > 0
            || processed_contact_messages > 0
            || processed_handshakes > 0
        {
            if let Err(error) = self.refresh_reachability_after_ceremony_processing().await {
                tracing::debug!(
                    acceptances = processed_acceptances,
                    completions = processed_completions,
                    contact_messages = processed_contact_messages,
                    handshakes = processed_handshakes,
                    error = %error,
                    "post-processing reachability refresh did not converge"
                );
            }
        }

        Ok(())
    }

    async fn sync_with_peer(&self, peer_id: &str) -> Result<(), IntentError> {
        if let Some(sync) = self.agent.runtime().sync() {
            // Parse peer_id into DeviceId
            let device_id: DeviceId = peer_id
                .parse()
                .map_err(|e| IntentError::validation_failed(format!("Invalid peer ID: {e}")))?;

            // Create a single-element vector for the target peer
            let peers = vec![device_id];

            // Get the effects from agent runtime
            let effects = self.agent.runtime().effects();

            // Sync with the specific peer
            sync.sync_with_peers(&effects, peers)
                .await
                .map_err(|e| IntentError::internal_error(format!("Sync failed: {}", e)))
        } else {
            Err(service_unavailable("sync_service"))
        }
    }

    async fn ensure_peer_channel(
        &self,
        context: ContextId,
        peer: AuthorityId,
    ) -> Result<(), IntentError> {
        let effects = self.agent.runtime().effects();
        let Some(rendezvous_manager) = self.agent.runtime().rendezvous() else {
            return Err(service_unavailable("rendezvous_manager"));
        };

        let authority = self.agent.context().clone();
        let handler = crate::handlers::rendezvous::RendezvousHandler::new(authority)
            .map_err(|e| {
                IntentError::internal_error(format!(
                    "Failed to create rendezvous handler for peer channel setup: {e}"
                ))
            })?
            .with_rendezvous_manager((*rendezvous_manager).clone());

        let fallback_context = default_context_id_for_authority(peer);
        let mut contexts = vec![context];
        if fallback_context != context {
            contexts.push(fallback_context);
        }

        let rounds = if harness_mode_enabled() {
            harness_sync_rounds()
        } else {
            1
        };
        let backoff_ms = if harness_mode_enabled() {
            harness_sync_backoff_ms()
        } else {
            0
        };

        let mut last_error: Option<String> = None;
        for channel_context in contexts {
            if effects.is_channel_established(channel_context, peer).await {
                self.seed_sync_peers_from_rendezvous().await;
                let _ = self.sync_seeded_peers().await;
                return Ok(());
            }

            let result = handler
                .initiate_channel(&effects, channel_context, peer)
                .await
                .map_err(|e| {
                    IntentError::network_error(format!(
                        "Failed to initiate peer channel for {peer} in {channel_context}: {e}"
                    ))
                });

            let result = match result {
                Ok(value) => value,
                Err(error) => {
                    last_error = Some(error.to_string());
                    continue;
                }
            };

            if !result.success {
                last_error = Some(
                    result
                        .error
                        .unwrap_or_else(|| "peer channel initiation was denied".to_string()),
                );
                continue;
            }

            for round in 0..rounds {
                if effects.is_channel_established(channel_context, peer).await {
                    self.seed_sync_peers_from_rendezvous().await;
                    let _ = self.sync_seeded_peers().await;
                    return Ok(());
                }

                if harness_mode_enabled() {
                    let _ = rendezvous::trigger_discovery(self).await;
                }
                self.seed_sync_peers_from_rendezvous().await;
                let _ = self.sync_seeded_peers().await;
                let _ = self.process_ceremony_messages().await;

                if round + 1 < rounds && backoff_ms > 0 {
                    self.sleep_ms(backoff_ms).await;
                }
            }

            last_error = Some(format!(
                "peer channel for {peer} in {channel_context} did not establish after bounded convergence"
            ));
        }

        Err(IntentError::network_error(last_error.unwrap_or_else(|| {
            format!("peer channel for {peer} in {context} did not establish after bounded convergence")
        })))
    }

    // =========================================================================
    // Peer Discovery
    // =========================================================================

    async fn try_get_discovered_peers(&self) -> Result<Vec<AuthorityId>, IntentError> {
        Ok(rendezvous::get_discovered_peers(self).await)
    }

    async fn try_get_rendezvous_status(&self) -> Result<RendezvousStatus, IntentError> {
        Ok(rendezvous::get_rendezvous_status(self).await)
    }

    async fn trigger_discovery(&self) -> Result<(), IntentError> {
        rendezvous::trigger_discovery(self).await
    }

    // =========================================================================
    // LAN Discovery
    // =========================================================================

    async fn try_get_lan_peers(&self) -> Result<Vec<LanPeerInfo>, IntentError> {
        Ok(rendezvous::get_lan_peers(self).await)
    }

    async fn send_lan_invitation(
        &self,
        _peer: &LanPeerInfo,
        _invitation_code: &str,
    ) -> Result<(), IntentError> {
        rendezvous::send_lan_invitation(self, _peer, _invitation_code).await
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

    async fn bootstrap_signing_keys(&self) -> Result<Vec<u8>, IntentError> {
        let authority = self.agent.authority_id();
        let signing_service = self.agent.threshold_signing();

        // Bootstrap 1-of-1 keys for single-device operation
        let public_key_package = signing_service
            .bootstrap_authority(&authority)
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to bootstrap signing keys: {}", e))
            })?;

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
        use aura_core::threshold::{
            policy_for, CeremonyFlow, KeyGenerationPolicy, ParticipantIdentity,
        };
        use aura_recovery::guardian_ceremony::GuardianState;
        use aura_recovery::{CeremonyId as GuardianCeremonyId, GuardianRotationOp};

        let participants = guardian_ids
            .iter()
            .copied()
            .map(aura_core::threshold::ParticipantIdentity::guardian)
            .collect::<Vec<_>>();

        let policy = policy_for(CeremonyFlow::GuardianSetupRotation);

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
            .map_err(|e| IntentError::internal_error(format!("Failed to read tree state: {e}")))?;
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
            .map_err(|e| IntentError::internal_error(format!("Failed to read time: {e}")))?
            .ts_ms;
        for old_id in runner
            .check_supersession_candidates(
                aura_app::runtime_bridge::CeremonyKind::GuardianRotation,
                Some(&prestate_hash),
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
                prestate_hash: Some(prestate_hash),
            })
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to register ceremony: {}", e))
            })?;

        // Step 4: Execute guardian ceremony choreography (send proposals + collect responses)
        let recovery_service = self
            .agent
            .recovery()
            .map_err(|e| service_unavailable_with_detail("recovery_service", e))?;

        let accepted_guardians = recovery_service
            .execute_guardian_ceremony_initiator(
                ceremony_id_hash,
                prestate_hash,
                operation.clone(),
                guardian_ids.to_vec(),
                key_packages.clone(),
            )
            .await
            .map_err(|e| {
                IntentError::internal_error(format!(
                    "Failed to execute guardian ceremony choreography: {e}"
                ))
            })?;

        // Step 5: Record accepted participants before committing
        for guardian_id in &accepted_guardians {
            runner
                .record_response(&ceremony_id, ParticipantIdentity::guardian(*guardian_id))
                .await
                .map_err(|e| {
                    IntentError::internal_error(format!(
                        "Failed to record guardian acceptance: {e}"
                    ))
                })?;
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
            .map_err(|e| IntentError::internal_error(format!("Failed to commit ceremony: {e}")))?;

        tracing::info!(
            ceremony_id = %ceremony_id,
            "Guardian ceremony completed successfully"
        );

        // Step 7: Commit GuardianBinding facts for each accepted guardian.
        // This enables the ContactsSignalView to reflect guardian status in the UI.
        for guardian_id in &accepted_guardians {
            let binding_fact = RelationalFact::Protocol(ProtocolRelationalFact::GuardianBinding {
                account_id: authority_id,
                guardian_id: *guardian_id,
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
    /// 3. Send envelope via TransportEffects
    /// 4. If send fails, return NetworkError indicating unreachable device
    ///
    /// # Error Cases
    ///
    /// - **No transport available**: Device has no running agent with SharedTransport
    /// - **Device unreachable**: Transport cannot deliver envelope to device's authority
    /// - **Validation failure**: Invalid threshold, missing current device, duplicate devices
    ///
    /// # See Also
    ///
    /// - `crates/aura-agent/src/core/ceremony_processor/threshold.rs` - Recipient handling
    /// - `docs/102_authority_and_identity.md` - Multi-authority device model
    async fn initiate_device_threshold_ceremony(
        &self,
        threshold_k: FrostThreshold,
        total_n: u16,
        device_ids: &[String],
    ) -> Result<aura_core::types::identifiers::CeremonyId, IntentError> {
        use aura_core::effects::{
            SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
            ThresholdSigningEffects,
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
            .map_err(|e| IntentError::internal_error(format!("Failed to read tree state: {e}")))?;

        let prestate_input = serde_json::to_vec(&(
            tree_state.epoch,
            tree_state.root_commitment,
            parsed_devices.clone(),
            threshold_value,
            total_n,
        ))
        .map_err(|e| IntentError::internal_error(format!("Serialize prestate: {e}")))?;
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
        .map_err(|e| IntentError::internal_error(format!("Serialize operation: {e}")))?;
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
            .map_err(|e| IntentError::internal_error(format!("Failed to read time: {e}")))?
            .ts_ms;
        for old_id in runner
            .check_supersession_candidates(
                aura_app::runtime_bridge::CeremonyKind::DeviceRotation,
                Some(&prestate_hash),
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
                prestate_hash: Some(prestate_hash),
            })
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to register ceremony: {e}"))
            })?;

        // Mark the initiator as accepted (their key package is already local).
        let _ = runner
            .record_response(&ceremony_id, ParticipantIdentity::device(current_device_id))
            .await;

        // Send key packages to other devices.
        let context_entropy = {
            let mut h = aura_core::hash::hasher();
            h.update(b"DEVICE_THRESHOLD_CONTEXT");
            h.update(&authority_id.to_bytes());
            h.update(ceremony_id.as_str().as_bytes());
            h.finalize()
        };
        let ceremony_context =
            aura_core::types::identifiers::ContextId::new_from_entropy(context_entropy);

        use base64::Engine;
        let config_b64 = if threshold_config.is_empty() {
            None
        } else {
            Some(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&threshold_config))
        };
        let pubkey_b64 = if public_key_package.is_empty() {
            None
        } else {
            Some(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&public_key_package))
        };

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

            let mut metadata = std::collections::HashMap::new();
            metadata.insert(
                "content-type".to_string(),
                "application/aura-device-threshold-key-package".to_string(),
            );
            metadata.insert("ceremony-id".to_string(), ceremony_id.to_string());
            metadata.insert(
                "pending-epoch".to_string(),
                pending_epoch.value().to_string(),
            );
            metadata.insert(
                "initiator-device-id".to_string(),
                current_device_id.to_string(),
            );
            metadata.insert("participant-device-id".to_string(), device_id.to_string());
            metadata.insert(
                "aura-destination-device-id".to_string(),
                device_id.to_string(),
            );
            // Include the target authority in metadata for ceremony coordination
            metadata.insert("target-authority-id".to_string(), authority_id.to_string());
            if let Some(config_b64) = config_b64.as_ref() {
                metadata.insert("threshold-config".to_string(), config_b64.clone());
            }
            if let Some(pubkey_b64) = pubkey_b64.as_ref() {
                metadata.insert("threshold-pubkey".to_string(), pubkey_b64.clone());
            }

            let envelope = aura_core::effects::TransportEnvelope {
                // Device-specific routing happens via aura-destination-device-id under the
                // shared target authority for the ceremony.
                destination: authority_id,
                source: authority_id,
                context: ceremony_context,
                payload: key_package,
                metadata,
                receipt: None,
            };

            effects.send_envelope(envelope).await.map_err(|e| {
                let error_msg = e.to_string();

                // Provide clearer error messages for common failure cases
                if error_msg.contains("not connected")
                    || error_msg.contains("unreachable")
                    || error_msg.contains("no route")
                    || error_msg.contains("offline mode") {
                    IntentError::network_error(format!(
                        "Device {} is not reachable. Ensure the device is online and connected to the network before starting the multifactor ceremony.",
                        device_id
                    ))
                } else {
                    IntentError::internal_error(format!(
                        "Failed to send device threshold key package to {}: {e}",
                        device_id
                    ))
                }
            })?;
        }

        Ok(ceremony_id)
    }

    async fn initiate_device_enrollment_ceremony(
        &self,
        nickname_suggestion: String,
        invitee_authority_id: Option<AuthorityId>,
    ) -> Result<aura_app::runtime_bridge::DeviceEnrollmentStart, IntentError> {
        use aura_core::effects::{
            SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
            ThresholdSigningEffects,
        };
        use aura_core::hash::hash;
        use aura_core::threshold::{
            policy_for, CeremonyFlow, KeyGenerationPolicy, ParticipantIdentity,
        };

        let authority_id = self.agent.authority_id();
        let effects = self.agent.runtime().effects();
        let current_device_id = self.agent.context().device_id();

        // Best-effort: derive current device participant set from the commitment tree.
        let tree_state = effects
            .get_current_state()
            .await
            .map_err(|e| IntentError::internal_error(format!("Failed to read tree state: {e}")))?;

        let mut device_ids: Vec<aura_core::DeviceId> = tree_state
            .leaves
            .values()
            .filter(|leaf| leaf.role == aura_core::tree::LeafRole::Device)
            .map(|leaf| leaf.device_id)
            .collect();

        if !device_ids.contains(&current_device_id) {
            device_ids.push(current_device_id);
        }

        // Generate a new device id to enroll (demo override supported via env).
        let entropy = effects.random_bytes(32).await;
        let mut entropy_bytes = [0u8; 32];
        entropy_bytes.copy_from_slice(&entropy[..32]);
        let new_device_id = match std::env::var("AURA_DEMO_DEVICE_ID") {
            Ok(override_id) => match override_id.parse::<aura_core::DeviceId>() {
                Ok(id) => id,
                Err(e) => {
                    tracing::warn!(
                        override_id = %override_id,
                        error = %e,
                        "Invalid AURA_DEMO_DEVICE_ID override; falling back to random device id"
                    );
                    aura_core::DeviceId::new_from_entropy(entropy_bytes)
                }
            },
            Err(_) => aura_core::DeviceId::new_from_entropy(entropy_bytes),
        };

        // Prepare new key material for the updated participant set.
        //
        // Threshold policy:
        // - Prefer existing device MFA threshold config, if present.
        // - Otherwise fall back to a simple default (1-of-1, 2-of-2, else 2-of-n).
        let mut other_device_ids: Vec<aura_core::DeviceId> = device_ids
            .into_iter()
            .filter(|id| *id != current_device_id)
            .collect();
        other_device_ids.sort_by_key(|a| a.to_string());

        let mut participant_device_ids: Vec<aura_core::DeviceId> =
            Vec::with_capacity(other_device_ids.len() + 2);
        participant_device_ids.push(current_device_id);
        participant_device_ids.extend(other_device_ids.iter().copied());
        participant_device_ids.push(new_device_id);

        let participants: Vec<ParticipantIdentity> = participant_device_ids
            .iter()
            .copied()
            .map(ParticipantIdentity::device)
            .collect();

        let policy = policy_for(CeremonyFlow::DeviceEnrollment);
        if policy.keygen != KeyGenerationPolicy::K2DealerBased {
            return Err(IntentError::internal_error(
                "Device enrollment requires dealer-based DKG (K2)".to_string(),
            ));
        }

        let total_n = participants.len() as u16;
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
                IntentError::internal_error(format!("Failed to prepare device rotation: {e}"))
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
                tracing::warn!(error = %e, "Missing device enrollment public key package");
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
                tracing::warn!(error = %e, "Missing device enrollment threshold config");
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

        let Some(invited_key_package) = key_package_by_device.get(&new_device_id).cloned() else {
            return Err(IntentError::internal_error(
                "Key rotation returned no key package for invited device".to_string(),
            ));
        };

        // Compute a best-effort prestate-bound ceremony id.
        let prestate_input = serde_json::to_vec(&(
            tree_state.epoch,
            tree_state.root_commitment,
            participant_device_ids.clone(),
        ))
        .map_err(|e| IntentError::internal_error(format!("Serialize prestate: {e}")))?;
        let context_commitment = aura_core::Hash32(hash(&prestate_input));
        let prestate = Prestate::new(
            vec![(authority_id, Hash32(tree_state.root_commitment))],
            context_commitment,
        )
        .map_err(|e| IntentError::internal_error(format!("Invalid enrollment prestate: {e}")))?;
        let prestate_hash = prestate.compute_hash();

        let op_input = serde_json::to_vec(&(
            new_device_id,
            pending_epoch.value(),
            threshold_k,
            total_n,
            current_device_id,
        ))
        .map_err(|e| IntentError::internal_error(format!("Serialize operation: {e}")))?;
        let op_hash = aura_core::Hash32(hash(&op_input));

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

        // Register ceremony (acceptance required from all non-initiator devices).
        let acceptor_device_ids: Vec<aura_core::DeviceId> = other_device_ids
            .iter()
            .copied()
            .chain(std::iter::once(new_device_id))
            .collect();
        let acceptors: Vec<ParticipantIdentity> = acceptor_device_ids
            .iter()
            .copied()
            .map(ParticipantIdentity::device)
            .collect();
        let acceptance_n = acceptors.len() as u16;
        let acceptance_threshold = threshold_k.min(acceptance_n);

        let runner = self.agent.ceremony_runner().await;
        let nickname_for_tracker = if nickname_suggestion.is_empty() {
            None
        } else {
            Some(nickname_suggestion.clone())
        };
        let now_ms = effects
            .physical_time()
            .await
            .map_err(|e| IntentError::internal_error(format!("Failed to read time: {e}")))?
            .ts_ms;
        for old_id in runner
            .check_supersession_candidates(
                aura_app::runtime_bridge::CeremonyKind::DeviceEnrollment,
                Some(&prestate_hash),
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
                kind: aura_app::runtime_bridge::CeremonyKind::DeviceEnrollment,
                initiator_id: authority_id,
                threshold_k: acceptance_threshold,
                total_n: acceptance_n,
                participants: acceptors,
                new_epoch: pending_epoch.value(),
                enrollment_device_id: Some(new_device_id),
                enrollment_nickname_suggestion: nickname_for_tracker,
                prestate_hash: Some(prestate_hash),
            })
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to register ceremony: {e}"))
            })?;

        // Distribute new-epoch key packages to existing devices (so they are not bricked).
        if !other_device_ids.is_empty() {
            use base64::Engine;
            let config_b64 = if threshold_config.is_empty() {
                None
            } else {
                Some(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&threshold_config))
            };
            let pubkey_b64 = if public_key_package.is_empty() {
                None
            } else {
                Some(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&public_key_package))
            };
            let context_entropy = {
                let mut h = aura_core::hash::hasher();
                h.update(b"DEVICE_ENROLLMENT_CONTEXT");
                h.update(&authority_id.to_bytes());
                h.update(ceremony_id.as_str().as_bytes());
                h.finalize()
            };
            let ceremony_context =
                aura_core::types::identifiers::ContextId::new_from_entropy(context_entropy);

            for device_id in &other_device_ids {
                let Some(key_package) = key_package_by_device.get(device_id).cloned() else {
                    return Err(IntentError::internal_error(format!(
                        "Missing key package for existing device {}",
                        device_id
                    )));
                };

                let mut metadata = std::collections::HashMap::new();
                metadata.insert(
                    "content-type".to_string(),
                    "application/aura-device-enrollment-key-package".to_string(),
                );
                metadata.insert("ceremony-id".to_string(), ceremony_id.to_string());
                metadata.insert(
                    "pending-epoch".to_string(),
                    pending_epoch.value().to_string(),
                );
                metadata.insert(
                    "initiator-device-id".to_string(),
                    current_device_id.to_string(),
                );
                metadata.insert("participant-device-id".to_string(), device_id.to_string());
                metadata.insert(
                    "aura-destination-device-id".to_string(),
                    device_id.to_string(),
                );
                if let Some(config_b64) = config_b64.as_ref() {
                    metadata.insert("threshold-config".to_string(), config_b64.clone());
                }
                if let Some(pubkey_b64) = pubkey_b64.as_ref() {
                    metadata.insert("threshold-pubkey".to_string(), pubkey_b64.clone());
                }

                let envelope = aura_core::effects::TransportEnvelope {
                    destination: authority_id,
                    source: authority_id,
                    context: ceremony_context,
                    payload: key_package,
                    metadata,
                    receipt: None,
                };

                effects.send_envelope(envelope).await.map_err(|e| {
                    IntentError::internal_error(format!(
                        "Failed to send device enrollment key package to {}: {e}",
                        device_id
                    ))
                })?;
            }
        }

        // Create a shareable device enrollment invitation.
        //
        // For the two-step exchange flow, the invitee's authority_id is provided
        // so the invitation is addressed to that specific authority. Otherwise,
        // fall back to self-addressed (legacy bearer token behavior).
        let invitation_service = self
            .agent
            .invitations()
            .map_err(|e| service_unavailable_with_detail("invitation_service", e))?;

        let receiver_id = invitee_authority_id.unwrap_or(authority_id);
        let baseline_tree_ops = effects
            .export_tree_ops()
            .await
            .map_err(|e| IntentError::internal_error(format!("Export baseline tree ops: {e}")))?
            .into_iter()
            .map(|op| {
                aura_core::util::serialization::to_vec(&op).map_err(|e| {
                    IntentError::internal_error(format!(
                        "Serialize baseline tree op for device enrollment: {e}"
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let invitation = invitation_service
            .invite_device_enrollment(
                receiver_id,
                authority_id,
                current_device_id,
                new_device_id,
                Some(nickname_suggestion),
                ceremony_id.clone(),
                pending_epoch.value(),
                invited_key_package,
                threshold_config.clone(),
                public_key_package.clone(),
                baseline_tree_ops,
                None,
            )
            .await
            .map_err(|e| IntentError::internal_error(format!("Create device invite: {e}")))?;

        tracing::info!(
            authority = %authority_id,
            websocket_addrs = ?effects
                .lan_transport()
                .map(|transport| transport.websocket_addrs().to_vec())
                .unwrap_or_default(),
            "device enrollment export transport state"
        );

        // Use compile-time safe export since we already have the invitation
        let enrollment_code = invitation_service.export_invitation_with_sender_hint(&invitation);

        Ok(aura_app::runtime_bridge::DeviceEnrollmentStart {
            ceremony_id: ceremony_id.clone(),
            enrollment_code,
            pending_epoch,
            device_id: new_device_id,
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
            .map_err(|e| IntentError::internal_error(format!("Failed to read tree state: {e}")))?;

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
        .map_err(|e| IntentError::internal_error(format!("Serialize prestate: {e}")))?;
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
        .map_err(|e| IntentError::internal_error(format!("Serialize operation: {e}")))?;
        let op_hash = aura_core::Hash32(hash(&op_input));

        let consensus_required = signing_service
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
                "Skipping consensus DKG transcript (single-signer authority)"
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
            .map_err(|e| IntentError::internal_error(format!("Failed to read time: {e}")))?
            .ts_ms;
        for old_id in runner
            .check_supersession_candidates(
                aura_app::runtime_bridge::CeremonyKind::DeviceRemoval,
                Some(&prestate_hash),
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
                prestate_hash: Some(prestate_hash),
            })
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to register ceremony: {e}"))
            })?;

        let _ = runner
            .record_response(&ceremony_id, ParticipantIdentity::device(current_device_id))
            .await;

        use base64::Engine;
        let config_b64 = if threshold_config.is_empty() {
            None
        } else {
            Some(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&threshold_config))
        };
        let pubkey_b64 = if public_key_package.is_empty() {
            None
        } else {
            Some(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&public_key_package))
        };

        let context_entropy = {
            let mut h = aura_core::hash::hasher();
            h.update(b"DEVICE_THRESHOLD_CONTEXT");
            h.update(&authority_id.to_bytes());
            h.update(ceremony_id.as_str().as_bytes());
            h.finalize()
        };
        let ceremony_context =
            aura_core::types::identifiers::ContextId::new_from_entropy(context_entropy);

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

            let mut metadata = std::collections::HashMap::new();
            metadata.insert(
                "content-type".to_string(),
                "application/aura-device-threshold-key-package".to_string(),
            );
            metadata.insert("ceremony-id".to_string(), ceremony_id.to_string());
            metadata.insert(
                "pending-epoch".to_string(),
                pending_epoch.value().to_string(),
            );
            metadata.insert(
                "initiator-device-id".to_string(),
                current_device_id.to_string(),
            );
            metadata.insert("participant-device-id".to_string(), device_id.to_string());
            metadata.insert(
                "aura-destination-device-id".to_string(),
                device_id.to_string(),
            );
            if let Some(config_b64) = config_b64.as_ref() {
                metadata.insert("threshold-config".to_string(), config_b64.clone());
            }
            if let Some(pubkey_b64) = pubkey_b64.as_ref() {
                metadata.insert("threshold-pubkey".to_string(), pubkey_b64.clone());
            }

            let envelope = aura_core::effects::TransportEnvelope {
                destination: authority_id,
                source: authority_id,
                context: ceremony_context,
                payload: key_package,
                metadata,
                receipt: None,
            };

            effects.send_envelope(envelope).await.map_err(|e| {
                IntentError::internal_error(format!(
                    "Failed to send device removal key package to {}: {e}",
                    device_id
                ))
            })?;
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
    ) -> Result<aura_app::runtime_bridge::CeremonyStatus, IntentError> {
        // Ensure ceremony progress is driven even when the caller only polls status.
        //
        // In demo mode, acceptances arrive via transport envelopes. If nothing processes
        // them, ceremonies will never complete and guardian bindings will never be committed.
        if let Err(e) = self.agent.process_ceremony_acceptances().await {
            tracing::debug!("Failed to process ceremony acceptances: {}", e);
        }

        let runner = self.agent.ceremony_runner().await;
        let tracker = self.agent.ceremony_tracker().await;
        let _status = runner
            .status(ceremony_id)
            .await
            .map_err(|e| IntentError::validation_failed(format!("Ceremony not found: {}", e)))?;
        let _timed_out = runner.is_timed_out(ceremony_id).await.unwrap_or(false);

        let state = tracker
            .get(ceremony_id)
            .await
            .map_err(|e| IntentError::validation_failed(format!("Ceremony not found: {}", e)))?;

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

    async fn get_key_rotation_ceremony_status(
        &self,
        ceremony_id: &aura_core::types::identifiers::CeremonyId,
    ) -> Result<aura_app::runtime_bridge::KeyRotationCeremonyStatus, IntentError> {
        // Ensure acceptances are processed so polling drives progress in demo/simulation mode.
        if let Err(e) = self.agent.process_ceremony_acceptances().await {
            tracing::debug!("Failed to process ceremony acceptances: {}", e);
        }

        let runner = self.agent.ceremony_runner().await;
        let tracker = self.agent.ceremony_tracker().await;
        let _status = runner
            .status(ceremony_id)
            .await
            .map_err(|e| IntentError::validation_failed(format!("Ceremony not found: {}", e)))?;
        let _timed_out = runner.is_timed_out(ceremony_id).await.unwrap_or(false);
        let state = tracker
            .get(ceremony_id)
            .await
            .map_err(|e| IntentError::validation_failed(format!("Ceremony not found: {}", e)))?;

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
    ) -> Result<(), IntentError> {
        // Ensure acceptances are processed so state is up-to-date.
        if let Err(e) = self.agent.process_ceremony_acceptances().await {
            tracing::debug!("Failed to process ceremony acceptances: {}", e);
        }

        let runner = self.agent.ceremony_runner().await;
        let tracker = self.agent.ceremony_tracker().await;
        let state = tracker.get(ceremony_id).await?;

        // Best-effort: rollback pending epoch if present and not committed.
        if !state.is_committed {
            self.rollback_guardian_key_rotation(Epoch::new(state.new_epoch))
                .await?;
        }

        runner
            .abort(ceremony_id, Some("Canceled".to_string()))
            .await?;

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
        message: Option<String>,
        ttl_ms: Option<u64>,
    ) -> Result<InvitationInfo, IntentError> {
        let invitation_service = self
            .agent
            .invitations()
            .map_err(|e| service_unavailable_with_detail("invitation_service", e))?;

        let invitation = invitation_service
            .invite_as_contact(receiver, nickname, message, ttl_ms)
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
                invitation_service.invite_to_channel(
                    receiver,
                    home_id,
                    context_id,
                    channel_name_hint,
                    bootstrap,
                    message,
                    ttl_ms,
                ),
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
            invitation_service
                .invite_to_channel(
                    receiver,
                    home_id,
                    context_id,
                    channel_name_hint,
                    bootstrap,
                    message,
                    ttl_ms,
                ),
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

    async fn accept_invitation(&self, invitation_id: &str) -> Result<(), IntentError> {
        let invitation_service = self
            .agent
            .invitations()
            .map_err(|e| service_unavailable_with_detail("invitation_service", e))?;

        let invitation_id =
            aura_core::types::identifiers::InvitationId::new(invitation_id.to_string());
        let result = execute_with_effect_timeout(
            &self.agent.runtime().effects(),
            Duration::from_millis(INVITATION_BRIDGE_STAGE_TIMEOUT_MS),
            invitation_service.accept(&invitation_id),
        )
        .await
        .map_err(|error| match error {
            TimeoutRunError::Timeout(_) => IntentError::internal_error(format!(
                "invitation_service.accept timed out after {INVITATION_BRIDGE_STAGE_TIMEOUT_MS}ms"
            )),
            TimeoutRunError::Operation(e) => {
                IntentError::internal_error(format!("Failed to accept invitation: {}", e))
            }
        })?;

        if result.success {
            Ok(())
        } else {
            Err(IntentError::internal_error(result.error.unwrap_or_else(
                || "Failed to accept invitation".to_string(),
            )))
        }
    }

    async fn decline_invitation(&self, invitation_id: &str) -> Result<(), IntentError> {
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

        if result.success {
            Ok(())
        } else {
            Err(IntentError::internal_error(result.error.unwrap_or_else(
                || "Failed to decline invitation".to_string(),
            )))
        }
    }

    async fn cancel_invitation(&self, invitation_id: &str) -> Result<(), IntentError> {
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

        if result.success {
            Ok(())
        } else {
            Err(IntentError::internal_error(result.error.unwrap_or_else(
                || "Failed to cancel invitation".to_string(),
            )))
        }
    }

    async fn try_list_pending_invitations(&self) -> Result<Vec<InvitationInfo>, IntentError> {
        Ok(if let Ok(invitation_service) = self.agent.invitations() {
            invitation_service
                .list_pending()
                .await
                .iter()
                .map(convert_invitation_to_bridge_info)
                .collect()
        } else {
            Vec::new()
        })
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

    async fn try_get_invited_peer_ids(&self) -> Result<Vec<AuthorityId>, IntentError> {
        // Get pending invitations where we are the sender
        Ok(if let Ok(invitation_service) = self.agent.invitations() {
            let our_authority = self.agent.authority_id();
            invitation_service
                .list_pending()
                .await
                .iter()
                .filter(|inv| inv.sender_id == our_authority)
                .map(|inv| inv.receiver_id)
                .collect()
        } else {
            Vec::new()
        })
    }

    // =========================================================================
    // Settings Operations
    // =========================================================================

    async fn try_get_settings(&self) -> Result<SettingsBridgeState, IntentError> {
        let device_count = self.try_list_devices().await?.len();

        // Get threshold config if available
        let (threshold_k, threshold_n) = if let Some(config) = self.get_threshold_config().await {
            (config.threshold, config.total_participants)
        } else {
            (0, 0)
        };

        // Get contact count from invitations (accepted contact invitations)
        let contact_count = if let Ok(service) = self.agent.invitations() {
            service
                .list_pending()
                .await
                .iter()
                .filter(|inv| {
                    matches!(
                        inv.invitation_type,
                        crate::handlers::invitation::InvitationType::Contact { .. }
                    ) && inv.status == crate::handlers::invitation::InvitationStatus::Accepted
                })
                .count()
        } else {
            0
        };

        // Settings service not yet implemented - return available data
        // When implemented, would provide: nickname_suggestion, mfa_policy from profile facts
        let (nickname_suggestion, mfa_policy) = match self.try_load_account_config().await {
            Ok(Some((_key, config))) => (
                config.nickname_suggestion.unwrap_or_default(),
                config.mfa_policy.unwrap_or_else(|| "disabled".to_string()),
            ),
            Ok(None) => (String::new(), "disabled".to_string()),
            Err(e) => {
                tracing::warn!("Failed to load account config for settings: {}", e);
                (String::new(), "disabled".to_string())
            }
        };

        Ok(SettingsBridgeState {
            nickname_suggestion,
            mfa_policy,
            threshold_k,
            threshold_n,
            device_count,
            contact_count,
        })
    }

    async fn try_list_devices(&self) -> Result<Vec<BridgeDeviceInfo>, IntentError> {
        use aura_app::views::naming::EffectiveName;
        use aura_core::tree::metadata::DeviceLeafMetadata;

        let effects = self.agent.runtime().effects();
        let current_device = self.agent.context().device_id();

        let state = match effects.get_current_state().await {
            Ok(state) => state,
            Err(e) => {
                tracing::warn!("Failed to read commitment tree state for devices: {e}");
                // Return at least the current device on error
                let id = current_device;
                let device = BridgeDeviceInfo {
                    id,
                    name: String::new(), // Will be computed from effective_name()
                    nickname: None,
                    nickname_suggestion: None,
                    is_current: true,
                    last_seen: None,
                };
                return Ok(vec![BridgeDeviceInfo {
                    name: device.effective_name(),
                    ..device
                }]);
            }
        };

        let mut devices: Vec<BridgeDeviceInfo> = state
            .leaves
            .values()
            .filter(|leaf| leaf.role == LeafRole::Device)
            .map(|leaf| {
                let id = leaf.device_id;

                // Try to decode nickname_suggestion from leaf metadata
                let nickname_suggestion = DeviceLeafMetadata::decode(&leaf.meta)
                    .ok()
                    .and_then(|meta| meta.nickname_suggestion);

                // Local nickname override (not yet wired to persistent storage)
                let nickname: Option<String> = None;

                let device = BridgeDeviceInfo {
                    id,
                    name: String::new(), // Will be computed from effective_name()
                    nickname,
                    nickname_suggestion,
                    is_current: leaf.device_id == current_device,
                    last_seen: None,
                };

                // Compute name using EffectiveName trait
                BridgeDeviceInfo {
                    name: device.effective_name(),
                    ..device
                }
            })
            .collect();

        // Ensure the current device is always included, even if not yet in the commitment tree.
        // This handles fresh accounts where no device enrollment ceremony has occurred yet.
        let current_in_tree = devices.iter().any(|d| d.is_current);
        if !current_in_tree {
            let id = current_device;
            let device = BridgeDeviceInfo {
                id,
                name: String::new(),
                nickname: None,
                nickname_suggestion: None,
                is_current: true,
                last_seen: None,
            };
            devices.insert(
                0,
                BridgeDeviceInfo {
                    name: device.effective_name(),
                    ..device
                },
            );
        }

        Ok(devices)
    }

    async fn try_list_authorities(&self) -> Result<Vec<BridgeAuthorityInfo>, IntentError> {
        let current_id = self.agent.authority_id();
        let current_nickname = match self.try_load_account_config().await {
            Ok(Some((_key, config))) => config
                .nickname_suggestion
                .filter(|value| !value.trim().is_empty()),
            Ok(None) => None,
            Err(error) => {
                tracing::warn!("Failed to load account config for authorities: {}", error);
                None
            }
        };

        let mut authorities = vec![BridgeAuthorityInfo {
            id: current_id,
            nickname_suggestion: current_nickname,
            is_current: true,
        }];
        let mut seen = HashSet::from([current_id]);

        let effects = self.agent.runtime().effects();
        let keys = match effects.list_keys(Some(authority_key_prefix())).await {
            Ok(keys) => keys,
            Err(error) => {
                tracing::warn!("Failed to list stored authorities: {}", error);
                return Ok(authorities);
            }
        };

        for key in keys {
            let Some(bytes) = (match effects.retrieve(&key).await {
                Ok(bytes) => bytes,
                Err(error) => {
                    tracing::warn!("Failed to read authority record {}: {}", key, error);
                    continue;
                }
            }) else {
                continue;
            };

            let record = match deserialize_authority(&bytes) {
                Ok(record) => record,
                Err(error) => {
                    tracing::warn!("Failed to decode authority record {}: {}", key, error);
                    continue;
                }
            };

            if !seen.insert(record.authority_id) {
                continue;
            }

            authorities.push(BridgeAuthorityInfo {
                id: record.authority_id,
                nickname_suggestion: None,
                is_current: record.authority_id == current_id,
            });
        }

        authorities.sort_by(|left, right| {
            right
                .is_current
                .cmp(&left.is_current)
                .then_with(|| left.id.to_string().cmp(&right.id.to_string()))
        });
        Ok(authorities)
    }

    async fn has_account_config(&self) -> Result<bool, IntentError> {
        AgentRuntimeBridge::has_account_config(self).await
    }

    async fn initialize_account(&self, nickname_suggestion: &str) -> Result<(), IntentError> {
        AgentRuntimeBridge::initialize_account(self, nickname_suggestion).await
    }

    async fn set_nickname_suggestion(&self, name: &str) -> Result<(), IntentError> {
        let (key, mut config) = self.load_account_config().await?;
        config.nickname_suggestion = Some(name.to_string());
        self.store_account_config(&key, &config).await
    }

    async fn set_mfa_policy(&self, policy: &str) -> Result<(), IntentError> {
        let (key, mut config) = self.load_account_config().await?;
        config.mfa_policy = Some(policy.to_string());
        self.store_account_config(&key, &config).await
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

    async fn current_time_ms(&self) -> Result<u64, IntentError> {
        let effects = self.agent.runtime().effects();
        let time = effects
            .physical_time()
            .await
            .map_err(|e| service_unavailable_with_detail("physical_time", e))?;
        Ok(time.ts_ms)
    }

    async fn sleep_ms(&self, ms: u64) {
        let effects = self.agent.runtime().effects();
        let _ = effects.sleep_ms(ms).await;
    }

    // =========================================================================
    // Authentication
    // =========================================================================

    async fn is_authenticated(&self) -> bool {
        if let Ok(auth_service) = self.agent.auth() {
            auth_service.is_authenticated().await
        } else {
            false
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
        Arc::new(AgentRuntimeBridge::new(self))
    }
}

// ============================================================================
#[allow(clippy::disallowed_types)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::AgentBuilder;
    use aura_chat::ChatFact;
    use aura_core::context::EffectContext;
    use aura_core::effects::ExecutionMode;
    use aura_core::hash::hash;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_LOCK.get_or_init(|| Mutex::new(()))
    }

    // Note: Full tests would require mock infrastructure which is in aura-testkit
    // These are placeholder tests showing the API usage

    #[test]
    fn test_sync_status_default() {
        let status = SyncStatus::default();
        assert!(!status.is_running);
        assert_eq!(status.connected_peers, 0);
    }

    #[test]
    fn test_rendezvous_status_default() {
        let status = RendezvousStatus::default();
        assert!(!status.is_running);
        assert_eq!(status.cached_peers, 0);
    }

    #[test]
    fn harness_sync_policy_defaults_when_env_missing() {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var("AURA_HARNESS_MODE");
        std::env::remove_var("AURA_HARNESS_SYNC_ROUNDS");
        std::env::remove_var("AURA_HARNESS_SYNC_BACKOFF_MS");

        assert!(!harness_mode_enabled());
        assert_eq!(harness_sync_rounds(), DEFAULT_HARNESS_SYNC_ROUNDS);
        assert_eq!(harness_sync_backoff_ms(), DEFAULT_HARNESS_SYNC_BACKOFF_MS);
    }

    #[test]
    fn harness_sync_policy_honors_explicit_env_values() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("AURA_HARNESS_MODE", "1");
        std::env::set_var("AURA_HARNESS_SYNC_ROUNDS", "5");
        std::env::set_var("AURA_HARNESS_SYNC_BACKOFF_MS", "125");

        assert!(harness_mode_enabled());
        assert_eq!(harness_sync_rounds(), 5);
        assert_eq!(harness_sync_backoff_ms(), 125);

        std::env::remove_var("AURA_HARNESS_MODE");
        std::env::remove_var("AURA_HARNESS_SYNC_ROUNDS");
        std::env::remove_var("AURA_HARNESS_SYNC_BACKOFF_MS");
    }

    #[tokio::test]
    async fn resolve_amp_channel_context_finds_committed_channel_created_fact() {
        let authority = AuthorityId::new_from_entropy([7u8; 32]);
        let build_context = EffectContext::new(
            authority,
            ContextId::new_from_entropy([9u8; 32]),
            ExecutionMode::Testing,
        );
        let agent = Arc::new(
            AgentBuilder::new()
                .with_authority(authority)
                .build_testing_async(&build_context)
                .await
                .expect("build testing agent"),
        );
        let bridge = AgentRuntimeBridge::new(agent);
        let context = ContextId::new_from_entropy([8u8; 32]);
        let channel = ChannelId::from_bytes(hash(b"resolve-amp-channel-context"));

        bridge
            .amp_create_channel(ChannelCreateParams {
                context,
                channel: Some(channel),
                skip_window: None,
                topic: None,
            })
            .await
            .expect("create channel");
        bridge
            .amp_join_channel(ChannelJoinParams {
                context,
                channel,
                participant: authority,
            })
            .await
            .expect("join channel");
        bridge
            .commit_relational_facts(&[ChatFact::channel_created_ms(
                context,
                channel,
                "shared-parity-lab".to_string(),
                None,
                false,
                42,
                authority,
            )
            .to_generic()])
            .await
            .expect("commit channel fact");

        let resolved = bridge
            .resolve_amp_channel_context(channel)
            .await
            .expect("resolve channel context");

        assert_eq!(resolved, Some(context));
    }
}
