//! RuntimeBridge implementation for AuraAgent
//!
//! This module implements the `RuntimeBridge` trait from `aura-app` for `AuraAgent`,
//! enabling the dependency inversion where `aura-app` defines the trait and
//! `aura-agent` provides the implementation.

use crate::core::AuraAgent;
use crate::handlers::shared::context_commitment_from_journal;
use crate::handlers::InvitationServiceApi;
use crate::runtime::consensus::build_consensus_params;
use async_trait::async_trait;
use aura_app::runtime_bridge::{
    BridgeDeviceInfo, InvitationInfo, LanPeerInfo, RendezvousStatus, RuntimeBridge,
    SettingsBridgeState, SyncStatus,
};
use aura_app::signal_defs::INVITATIONS_SIGNAL;
use aura_app::views::invitations::InvitationStatus;
use aura_app::IntentError;
use aura_app::ReactiveHandler;
use aura_core::effects::{
    amp::{
        AmpChannelEffects, AmpCiphertext, ChannelBootstrapPackage, ChannelCloseParams,
        ChannelCreateParams, ChannelJoinParams, ChannelLeaveParams, ChannelSendParams,
    },
    random::RandomCoreEffects,
    reactive::ReactiveEffects,
    task::TaskSpawner,
    time::PhysicalTimeEffects,
    SecureStorageCapability, SecureStorageEffects, SecureStorageLocation, ThresholdSigningEffects,
    TransportEffects,
};
use aura_core::hash::hash;
use aura_core::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_core::threshold::{AgreementMode, SigningContext, ThresholdConfig, ThresholdSignature};
use aura_core::tree::{AttestedOp, LeafRole, TreeOp};
use aura_core::types::{Epoch, FrostThreshold};
use aura_core::DeviceId;
use aura_core::EffectContext;
use aura_core::Hash32;
use aura_core::Prestate;
use aura_journal::fact::{
    ChannelBootstrap, ChannelBumpReason, FactOptions, ProposedChannelEpochBump, RelationalFact,
};
use aura_journal::ProtocolRelationalFact;
use aura_journal::DomainFact;
use aura_protocol::amp::{commit_bump_with_consensus, emit_proposed_bump, AmpJournalEffects};
use aura_protocol::effects::TreeEffects;
use aura_social::moderation::facts::{HomePinFact, HomeUnpinFact};
use aura_social::moderation::{
    HomeBanFact, HomeKickFact, HomeMuteFact, HomeUnbanFact, HomeUnmuteFact,
};
use std::collections::BTreeSet;
use std::sync::Arc;

use crate::core::default_context_id_for_authority;
use crate::runtime::services::ceremony_runner::{CeremonyCommitMetadata, CeremonyInitRequest};
use crate::runtime::services::ServiceError;
use aura_core::ceremony::SupersessionReason;

mod amp;
mod consensus;
mod invitation;
mod recovery;
mod rendezvous;
mod settings;

use amp::map_amp_error;
use consensus::{map_consensus_error, persist_consensus_dkg_transcript};
use invitation::convert_invitation_to_bridge_info;

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
}

#[async_trait]
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

    fn task_spawner(&self) -> Option<Arc<dyn TaskSpawner>> {
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
        let state = aura_protocol::amp::get_channel_state(&effects, context, channel)
            .await
            .map_err(|e| IntentError::internal_error(format!("AMP state lookup failed: {e}")))?;

        let mut requested_recipients = BTreeSet::new();
        for recipient in recipients {
            requested_recipients.insert(recipient);
        }

        if let Some(existing) = state.bootstrap.clone() {
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

            let params = build_consensus_params(effects.as_ref(), authority_id, effects.as_ref())
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

        tasks.spawn_interval_until(
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

    async fn get_sync_status(&self) -> SyncStatus {
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
                let health = sync.health().await;
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

        SyncStatus {
            is_running,
            connected_peers: transport_stats.active_channels as usize,
            last_sync_ms,
            pending_facts: 0, // Would need to track this in SyncServiceManager
            active_sessions: active_sessions as usize,
        }
    }

    async fn is_peer_online(&self, peer: AuthorityId) -> bool {
        // Drive inbox processing opportunistically so background-less runtimes
        // still respond to key-rotation/device-enrollment messages.
        if let Err(e) = self.agent.process_ceremony_acceptances().await {
            tracing::debug!("Failed to process ceremony acceptances: {}", e);
        }

        let effects = self.agent.runtime().effects();
        let context = EffectContext::with_authority(self.agent.authority_id()).context_id();
        effects.is_channel_established(context, peer).await
    }
    async fn get_sync_peers(&self) -> Vec<DeviceId> {
        if let Some(sync) = self.agent.runtime().sync() {
            sync.peers().await
        } else {
            Vec::new()
        }
    }

    async fn trigger_sync(&self) -> Result<(), IntentError> {
        if let Some(_sync) = self.agent.runtime().sync() {
            // The sync service runs continuously in the background
            // Triggering a manual sync would be a new feature
            Ok(())
        } else {
            Err(service_unavailable("sync_service"))
        }
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

    // =========================================================================
    // Peer Discovery
    // =========================================================================

    async fn get_discovered_peers(&self) -> Vec<AuthorityId> {
        rendezvous::get_discovered_peers(self).await
    }

    async fn get_rendezvous_status(&self) -> RendezvousStatus {
        rendezvous::get_rendezvous_status(self).await
    }

    async fn trigger_discovery(&self) -> Result<(), IntentError> {
        rendezvous::trigger_discovery(self).await
    }

    // =========================================================================
    // LAN Discovery
    // =========================================================================

    async fn get_lan_peers(&self) -> Vec<LanPeerInfo> {
        rendezvous::get_lan_peers(self).await
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
        guardian_ids: &[String],
    ) -> Result<(Epoch, Vec<Vec<u8>>, Vec<u8>), IntentError> {
        let authority = self.agent.authority_id();
        let signing_service = self.agent.threshold_signing();

        let participants = guardian_ids
            .iter()
            .map(|id_str| {
                id_str
                    .parse::<AuthorityId>()
                    .map(aura_core::threshold::ParticipantIdentity::guardian)
                    .map_err(|_| {
                        IntentError::validation_failed(format!(
                            "Failed to parse guardian id as AuthorityId: {}",
                            id_str
                        ))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;

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
        guardian_ids: &[String],
    ) -> Result<String, IntentError> {
        use aura_core::hash::hash;
        use aura_core::threshold::{policy_for, CeremonyFlow, KeyGenerationPolicy, ParticipantIdentity};
        use aura_recovery::guardian_ceremony::GuardianState;
        use aura_recovery::{CeremonyId as GuardianCeremonyId, GuardianRotationOp};

        // Convert String guardian IDs to AuthorityIds for the ceremony protocol
        let all_guardian_authority_ids: Vec<AuthorityId> = guardian_ids
            .iter()
            .filter_map(|id_str| id_str.parse().ok())
            .collect();

        if all_guardian_authority_ids.len() != guardian_ids.len() {
            return Err(IntentError::validation_failed(
                "Failed to parse one or more guardian IDs as AuthorityIds".to_string(),
            ));
        }

        let participants = all_guardian_authority_ids
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
            guardian_ids: all_guardian_authority_ids.clone(),
            new_epoch: new_epoch.value(),
        };
        let operation_hash = operation.compute_hash();

        let consensus_required = signing_service
            .threshold_state(&authority_id)
            .await
            .map(|state| state.threshold > 1 || state.total_participants > 1)
            .unwrap_or(true);

        if policy.keygen == KeyGenerationPolicy::K3ConsensusDkg && consensus_required {
            let params = build_consensus_params(effects.as_ref(), authority_id, &signing_service)
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
            aura_core::identifiers::CeremonyId::new(hex::encode(ceremony_id_hash.0 .0));

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
                all_guardian_authority_ids.clone(),
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
                .record_response(
                    &ceremony_id,
                    ParticipantIdentity::guardian(*guardian_id),
                )
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
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to commit ceremony: {e}"))
            })?;

        tracing::info!(
            ceremony_id = %ceremony_id,
            "Guardian ceremony completed successfully"
        );

        // Step 7: Commit GuardianBinding facts for each accepted guardian.
        // This enables the ContactsSignalView to reflect guardian status in the UI.
        for guardian_id in &accepted_guardians {
            let binding_fact =
                RelationalFact::Protocol(ProtocolRelationalFact::GuardianBinding {
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

        Ok(ceremony_id.to_string())
    }

    async fn initiate_device_threshold_ceremony(
        &self,
        threshold_k: FrostThreshold,
        total_n: u16,
        device_ids: &[String],
    ) -> Result<String, IntentError> {
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
        let signing_service = self.agent.threshold_signing();
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

        let policy = policy_for(CeremonyFlow::DeviceMfaRotation);

        let participants: Vec<ParticipantIdentity> = parsed_devices
            .iter()
            .copied()
            .map(ParticipantIdentity::device)
            .collect();

        let (pending_epoch, key_packages, _public_key) = effects
            .rotate_keys(&authority_id, threshold_value, total_n, &participants)
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
                tracing::warn!(error = %e, "Missing MFA public key package");
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
                tracing::warn!(error = %e, "Missing MFA threshold config");
                Vec::new()
            }
        };

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

        if policy.keygen == KeyGenerationPolicy::K3ConsensusDkg {
            let params = build_consensus_params(effects.as_ref(), authority_id, &signing_service)
                .await
                .map_err(map_consensus_error)?;
            let _ = persist_consensus_dkg_transcript(
                effects.clone(),
                prestate,
                params,
                authority_id,
                pending_epoch.value(),
                threshold_value,
                total_n,
                &participants,
                op_hash,
            )
            .await?;
        }

        let nonce_bytes = effects.random_bytes(8).await;
        let nonce = u64::from_le_bytes(nonce_bytes[..8].try_into().unwrap_or_default());
        let mut ceremony_seed = Vec::with_capacity(32 + 32 + 8);
        ceremony_seed.extend_from_slice(prestate_hash.as_bytes());
        ceremony_seed.extend_from_slice(op_hash.as_bytes());
        ceremony_seed.extend_from_slice(&nonce.to_le_bytes());
        let ceremony_hash = aura_core::Hash32(hash(&ceremony_seed));
        let ceremony_id = aura_core::identifiers::CeremonyId::new(format!(
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
        let ceremony_context = aura_core::identifiers::ContextId::new_from_entropy(context_entropy);

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
                    "Failed to send device threshold key package to {}: {e}",
                    device_id
                ))
            })?;
        }

        Ok(ceremony_id.to_string())
    }

    async fn initiate_device_enrollment_ceremony(
        &self,
        nickname_suggestion: String,
        invitee_authority_id: Option<String>,
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
        let ceremony_id = aura_core::identifiers::CeremonyId::new(format!(
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
                aura_core::identifiers::ContextId::new_from_entropy(context_entropy);

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

        let receiver_id = match invitee_authority_id {
            Some(invitee_auth_str) => invitee_auth_str
                .parse::<aura_core::AuthorityId>()
                .map_err(|e| {
                    IntentError::validation_failed(format!(
                        "Invalid invitee authority ID '{}': {e}",
                        invitee_auth_str
                    ))
                })?,
            None => {
                // Legacy behavior: self-addressed invitation (bearer token)
                authority_id
            }
        };

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
                None,
            )
            .await
            .map_err(|e| IntentError::internal_error(format!("Create device invite: {e}")))?;

        // Use compile-time safe export since we already have the invitation
        let enrollment_code = InvitationServiceApi::export_invitation(&invitation);

        Ok(aura_app::runtime_bridge::DeviceEnrollmentStart {
            ceremony_id: ceremony_id.to_string(),
            enrollment_code,
            pending_epoch,
            device_id: new_device_id,
        })
    }

    async fn initiate_device_removal_ceremony(
        &self,
        device_id: String,
    ) -> Result<String, IntentError> {
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

        if policy.keygen == aura_core::threshold::KeyGenerationPolicy::K3ConsensusDkg {
            let params = build_consensus_params(effects.as_ref(), authority_id, &signing_service)
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
        }

        let nonce_bytes = effects.random_bytes(8).await;
        let nonce = u64::from_le_bytes(nonce_bytes[..8].try_into().unwrap_or_default());
        let mut ceremony_seed = Vec::with_capacity(32 + 32 + 8);
        ceremony_seed.extend_from_slice(prestate_hash.as_bytes());
        ceremony_seed.extend_from_slice(op_hash.as_bytes());
        ceremony_seed.extend_from_slice(&nonce.to_le_bytes());
        let ceremony_hash = aura_core::Hash32(hash(&ceremony_seed));
        let ceremony_id = aura_core::identifiers::CeremonyId::new(format!(
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
        let ceremony_context = aura_core::identifiers::ContextId::new_from_entropy(context_entropy);

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

        if policy.keygen == aura_core::threshold::KeyGenerationPolicy::K3ConsensusDkg {
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

        Ok(ceremony_id.to_string())
    }
    async fn get_ceremony_status(
        &self,
        ceremony_id: &str,
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
        let ceremony_id = aura_core::identifiers::CeremonyId::new(ceremony_id.to_string());

        let _status = runner
            .status(&ceremony_id)
            .await
            .map_err(|e| IntentError::validation_failed(format!("Ceremony not found: {}", e)))?;
        let _timed_out = runner.is_timed_out(&ceremony_id).await.unwrap_or(false);

        let state = tracker
            .get(&ceremony_id)
            .await
            .map_err(|e| IntentError::validation_failed(format!("Ceremony not found: {}", e)))?;

        let accepted_guardians: Vec<String> = state
            .accepted_participants
            .iter()
            .filter_map(|p| match p {
                aura_core::threshold::ParticipantIdentity::Guardian(id) => Some(id.to_string()),
                _ => None,
            })
            .collect();

        Ok(aura_app::runtime_bridge::CeremonyStatus {
            ceremony_id: ceremony_id.to_string(),
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
        ceremony_id: &str,
    ) -> Result<aura_app::runtime_bridge::KeyRotationCeremonyStatus, IntentError> {
        // Ensure acceptances are processed so polling drives progress in demo/simulation mode.
        if let Err(e) = self.agent.process_ceremony_acceptances().await {
            tracing::debug!("Failed to process ceremony acceptances: {}", e);
        }

        let runner = self.agent.ceremony_runner().await;
        let tracker = self.agent.ceremony_tracker().await;
        let ceremony_id = aura_core::identifiers::CeremonyId::new(ceremony_id.to_string());
        let _status = runner
            .status(&ceremony_id)
            .await
            .map_err(|e| IntentError::validation_failed(format!("Ceremony not found: {}", e)))?;
        let _timed_out = runner.is_timed_out(&ceremony_id).await.unwrap_or(false);
        let state = tracker
            .get(&ceremony_id)
            .await
            .map_err(|e| IntentError::validation_failed(format!("Ceremony not found: {}", e)))?;

        Ok(aura_app::runtime_bridge::KeyRotationCeremonyStatus {
            ceremony_id: ceremony_id.to_string(),
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

    async fn cancel_key_rotation_ceremony(&self, ceremony_id: &str) -> Result<(), IntentError> {
        // Ensure acceptances are processed so state is up-to-date.
        if let Err(e) = self.agent.process_ceremony_acceptances().await {
            tracing::debug!("Failed to process ceremony acceptances: {}", e);
        }

        let runner = self.agent.ceremony_runner().await;
        let tracker = self.agent.ceremony_tracker().await;
        let ceremony_id = aura_core::identifiers::CeremonyId::new(ceremony_id.to_string());
        let state = tracker.get(&ceremony_id).await?;

        // Best-effort: rollback pending epoch if present and not committed.
        if !state.is_committed {
            self.rollback_guardian_key_rotation(Epoch::new(state.new_epoch))
                .await?;
        }

        runner
            .abort(&ceremony_id, Some("Canceled".to_string()))
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

        // Export the invitation code
        let invitation_id = aura_core::identifiers::InvitationId::new(invitation_id.to_string());
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
        bootstrap: Option<ChannelBootstrapPackage>,
        message: Option<String>,
        ttl_ms: Option<u64>,
    ) -> Result<InvitationInfo, IntentError> {
        let invitation_service = self
            .agent
            .invitations()
            .map_err(|e| service_unavailable_with_detail("invitation_service", e))?;

        let invitation = invitation_service
            .invite_to_channel(receiver, home_id, bootstrap, message, ttl_ms)
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to create channel invitation: {}", e))
            })?;

        Ok(convert_invitation_to_bridge_info(&invitation))
    }

    async fn accept_invitation(&self, invitation_id: &str) -> Result<(), IntentError> {
        let invitation_service = self
            .agent
            .invitations()
            .map_err(|e| service_unavailable_with_detail("invitation_service", e))?;

        let invitation_id = aura_core::identifiers::InvitationId::new(invitation_id.to_string());
        let result = invitation_service
            .accept(&invitation_id)
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to accept invitation: {}", e))
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

        let invitation_id = aura_core::identifiers::InvitationId::new(invitation_id.to_string());
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

        let invitation_id = aura_core::identifiers::InvitationId::new(invitation_id.to_string());
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

    async fn list_pending_invitations(&self) -> Vec<InvitationInfo> {
        if let Ok(invitation_service) = self.agent.invitations() {
            invitation_service
                .list_pending()
                .await
                .iter()
                .map(convert_invitation_to_bridge_info)
                .collect()
        } else {
            Vec::new()
        }
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
            .map_err(|e| {
                IntentError::validation_failed(format!("Invalid invitation code: {}", e))
            })?;

        Ok(convert_invitation_to_bridge_info(&invitation))
    }

    async fn get_invited_peer_ids(&self) -> Vec<String> {
        // Get pending invitations where we are the sender
        if let Ok(invitation_service) = self.agent.invitations() {
            let our_authority = self.agent.authority_id();
            invitation_service
                .list_pending()
                .await
                .iter()
                .filter(|inv| inv.sender_id == our_authority)
                .map(|inv| inv.receiver_id.to_string())
                .collect()
        } else {
            Vec::new()
        }
    }

    // =========================================================================
    // Settings Operations
    // =========================================================================

    async fn get_settings(&self) -> SettingsBridgeState {
        let device_count = self.list_devices().await.len();

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

        SettingsBridgeState {
            nickname_suggestion,
            mfa_policy,
            threshold_k,
            threshold_n,
            device_count,
            contact_count,
        }
    }

    async fn list_devices(&self) -> Vec<BridgeDeviceInfo> {
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
                return vec![BridgeDeviceInfo {
                    name: device.effective_name(),
                    ..device
                }];
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

        devices
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
        ceremony_id: &str,
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
#[cfg(test)]
mod tests {
    use super::*;

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
}
