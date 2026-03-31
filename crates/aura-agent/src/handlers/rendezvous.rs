//! Rendezvous Handlers
//!
//! Handlers for rendezvous operations including descriptor publication,
//! channel establishment, and relay coordination.

use super::shared::{
    build_transport_metadata, context_commitment_from_journal, resolve_charge_peer, HandlerContext,
    HandlerUtilities,
};
use crate::core::{AgentError, AgentResult, AuthorityContext};
use crate::runtime::consensus::build_consensus_params;
use crate::runtime::services::{RendezvousManager, ServiceRegistry};
use crate::runtime::AuraEffectSystem;
use aura_consensus::protocol::run_consensus;
use aura_core::crypto::single_signer::SingleSignerKeyPackage;
use aura_core::effects::secure::{
    SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
};
use aura_core::effects::{
    FlowBudgetEffects, TransportEffects, TransportEnvelope, TransportError, TransportReceipt,
};
use aura_core::hash::hash;
use aura_core::service::{EstablishPath, ServiceFamily};
use aura_core::threshold::{policy_for, AgreementMode, CeremonyFlow};
use aura_core::types::identifiers::{AuthorityId, ContextId};
use aura_core::{FlowCost, Hash32, Prestate, Receipt};
use aura_guards::chain::create_send_guard;
use aura_journal::DomainFact;
use aura_protocol::amp::AmpJournalEffects;
use aura_protocol::effects::EffectApiEffects;
use aura_protocol::effects::TreeEffects;
use aura_rendezvous::capabilities::{
    evaluation_candidates_for_rendezvous_guard, RendezvousCapability,
};
use aura_rendezvous::{
    EffectCommand, GuardOutcome, GuardSnapshot, RendezvousConfig, RendezvousDescriptor,
    RendezvousFact, RendezvousService, TransportHint, RENDEZVOUS_FACT_TYPE_ID,
};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Result of a rendezvous operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RendezvousResult {
    /// Whether the operation succeeded
    pub success: bool,
    /// Context ID affected
    pub context_id: ContextId,
    /// Peer involved (if applicable)
    pub peer: Option<AuthorityId>,
    /// Descriptor produced or updated by this operation
    pub descriptor: Option<RendezvousDescriptor>,
    /// Error message if operation failed
    pub error: Option<String>,
}

/// Channel establishment result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelResult {
    /// Whether establishment succeeded
    pub success: bool,
    /// Context the channel belongs to
    pub context_id: ContextId,
    /// Peer at other end of channel
    pub peer: AuthorityId,
    /// Channel identifier (if successful)
    pub channel_id: Option<[u8; 32]>,
    /// Selected transport method
    pub transport: Option<String>,
    /// Error message if failed
    pub error: Option<String>,
}

/// Rendezvous handler
#[derive(Clone)]
pub struct RendezvousHandler {
    context: HandlerContext,
    /// Inner rendezvous service for guard chain operations
    service: Arc<RendezvousService>,
    /// Canonical local runtime service registry surface.
    registry: Arc<ServiceRegistry>,
    /// Optional rendezvous manager for shared descriptor cache
    rendezvous_manager: Option<RendezvousManager>,
}

impl RendezvousHandler {
    fn decode_biscuit_frontier(
        &self,
        effects: &AuraEffectSystem,
    ) -> AgentResult<
        Option<(
            aura_authorization::Biscuit,
            aura_authorization::BiscuitAuthorizationBridge,
        )>,
    > {
        let Some(cache) = effects.biscuit_cache() else {
            return Ok(None);
        };

        let engine = base64::engine::general_purpose::STANDARD;
        let token_bytes = engine
            .decode(cache.token_b64)
            .map_err(|error| AgentError::effects(format!("decode biscuit token cache: {error}")))?;
        let root_pk_bytes = engine.decode(cache.root_pk_b64).map_err(|error| {
            AgentError::effects(format!("decode biscuit root public key cache: {error}"))
        })?;
        let root_public_key =
            aura_authorization::PublicKey::from_bytes(&root_pk_bytes).map_err(|error| {
                AgentError::effects(format!("parse biscuit root public key cache: {error}"))
            })?;
        let biscuit = aura_authorization::Biscuit::from(&token_bytes, root_public_key)
            .map_err(|error| AgentError::effects(format!("parse biscuit token cache: {error}")))?;
        let bridge = aura_authorization::BiscuitAuthorizationBridge::new(
            root_public_key,
            self.context.authority.authority_id(),
        );
        Ok(Some((biscuit, bridge)))
    }

    async fn build_rendezvous_capabilities(
        &self,
        effects: &AuraEffectSystem,
        now_ms: u64,
    ) -> AgentResult<Vec<aura_guards::types::CapabilityId>> {
        let Some((token, bridge)) = self.decode_biscuit_frontier(effects)? else {
            tracing::debug!(
                authority = %self.context.authority.authority_id(),
                "no Biscuit frontier available for rendezvous guard snapshot"
            );
            return Ok(Vec::new());
        };

        let current_time_seconds = (now_ms != 0).then_some(now_ms / 1_000);
        Ok(evaluation_candidates_for_rendezvous_guard()
            .iter()
            .filter_map(|capability| {
                let capability_name = capability.as_name();
                match bridge.has_capability_with_time(
                    &token,
                    capability_name.as_str(),
                    current_time_seconds,
                ) {
                    Ok(true) => Some(capability_name),
                    Ok(false) => None,
                    Err(error) => {
                        tracing::warn!(
                            authority = %self.context.authority.authority_id(),
                            capability = capability_name.as_str(),
                            error = %error,
                            "failed to evaluate rendezvous Biscuit capability"
                        );
                        None
                    }
                }
            })
            .collect())
    }

    /// Create a new rendezvous handler
    pub fn new(authority: AuthorityContext) -> AgentResult<Self> {
        HandlerUtilities::validate_authority_context(&authority)?;

        let config = RendezvousConfig::default();
        let service = Arc::new(RendezvousService::new(authority.authority_id(), config));

        Ok(Self {
            context: HandlerContext::new(authority),
            service,
            registry: Arc::new(ServiceRegistry::new()),
            rendezvous_manager: None,
        })
    }

    /// Create a rendezvous handler from a shared service instance.
    pub fn new_with_service(
        authority: AuthorityContext,
        service: Arc<RendezvousService>,
    ) -> AgentResult<Self> {
        HandlerUtilities::validate_authority_context(&authority)?;

        Ok(Self {
            context: HandlerContext::new(authority),
            service,
            registry: Arc::new(ServiceRegistry::new()),
            rendezvous_manager: None,
        })
    }

    /// Attach a rendezvous manager for shared descriptor cache access.
    pub fn with_rendezvous_manager(mut self, manager: RendezvousManager) -> Self {
        self.registry = manager.registry();
        self.rendezvous_manager = Some(manager);
        self
    }

    /// Get the authority context
    pub fn authority_context(&self) -> &AuthorityContext {
        &self.context.authority
    }

    // ========================================================================
    // Descriptor Operations
    // ========================================================================

    /// Publish a transport descriptor for a context
    pub async fn publish_descriptor(
        &self,
        effects: &AuraEffectSystem,
        context_id: ContextId,
        transport_hints: Vec<TransportHint>,
        _psk_commitment: [u8; 32],
        _validity_duration_ms: u64,
    ) -> AgentResult<RendezvousResult> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;

        // Enforce guard chain
        let guard = create_send_guard(
            RendezvousCapability::Publish.as_name(),
            context_id,
            self.context.authority.authority_id(),
            FlowCost::new(1), // Low cost for descriptor publication
        );
        let result = guard
            .evaluate(effects)
            .await
            .map_err(|e| AgentError::effects(format!("guard evaluation failed: {e}")))?;
        if !result.authorized {
            return Err(AgentError::effects(result.denial_reason.unwrap_or_else(
                || "descriptor publish not authorized".to_string(),
            )));
        }

        self.publish_descriptor_inner(effects, context_id, transport_hints)
            .await
    }

    /// Shared implementation for descriptor publication (after guard checks).
    async fn publish_descriptor_inner(
        &self,
        effects: &AuraEffectSystem,
        context_id: ContextId,
        transport_hints: Vec<TransportHint>,
    ) -> AgentResult<RendezvousResult> {
        let current_time = effects.current_timestamp().await.unwrap_or(0);

        // Retrieve identity keys to get public key
        let keys = retrieve_identity_keys(effects, &self.context.authority.authority_id()).await;
        let public_key = keys.map(|(_, pub_key)| pub_key).unwrap_or([0u8; 32]);

        // Create snapshot for guard evaluation
        let snapshot = self.create_snapshot(effects, context_id).await?;

        // Prepare the descriptor through the service
        let outcome = self.service.prepare_publish_descriptor(
            &snapshot,
            context_id,
            transport_hints,
            public_key,
            current_time,
        );

        // Check guard outcome and execute effects via the bridge
        if !outcome.decision.is_allowed() {
            return Ok(RendezvousResult {
                success: false,
                context_id,
                peer: None,
                descriptor: None,
                error: Some("Guard chain denied descriptor publication".to_string()),
            });
        }

        let mut published_descriptor: Option<RendezvousDescriptor> = None;
        // Cache descriptor before executing effects (for local access)
        for effect in &outcome.effects {
            if let EffectCommand::JournalAppend {
                fact: RendezvousFact::Descriptor(desc),
            } = effect
            {
                self.registry.cache_descriptor(desc.clone()).await;
                if let Some(manager) = self.rendezvous_manager.as_ref() {
                    if let Err(err) = manager.cache_descriptor(desc.clone()).await {
                        tracing::debug!(
                            error = %err,
                            "Failed to cache published descriptor in rendezvous manager"
                        );
                    }
                }
                published_descriptor = Some(desc.clone());
            }
        }

        // Execute all effect commands via the bridge
        execute_guard_outcome(outcome, &self.context.authority, context_id, effects).await?;

        Ok(RendezvousResult {
            success: true,
            context_id,
            peer: None,
            descriptor: published_descriptor,
            error: None,
        })
    }

    /// Cache a peer's descriptor received via journal sync
    pub async fn cache_peer_descriptor(&self, descriptor: RendezvousDescriptor) {
        if let Some(manager) = self.rendezvous_manager.as_ref() {
            if let Err(err) = manager.cache_descriptor(descriptor).await {
                tracing::debug!(error = %err, "Failed to cache descriptor in rendezvous manager");
            }
        } else {
            self.registry.cache_descriptor(descriptor).await;
        }
    }

    /// Get a peer's cached descriptor
    pub async fn get_peer_descriptor(
        &self,
        context_id: ContextId,
        peer: AuthorityId,
    ) -> Option<RendezvousDescriptor> {
        if let Some(manager) = self.rendezvous_manager.as_ref() {
            match manager.get_descriptor(context_id, peer).await {
                Some(descriptor) => Some(descriptor),
                None => manager.get_any_descriptor_for_authority(peer).await,
            }
        } else {
            match self.registry.get_descriptor(context_id, peer).await {
                Some(descriptor) => Some(descriptor),
                None => self.registry.get_any_descriptor_for_authority(peer).await,
            }
        }
    }

    /// Check if our descriptor needs refresh
    pub async fn needs_descriptor_refresh(
        &self,
        context_id: ContextId,
        now_ms: u64,
        refresh_window_ms: u64,
    ) -> bool {
        if let Some(manager) = self.rendezvous_manager.as_ref() {
            manager.needs_refresh(context_id, now_ms).await
        } else {
            self.registry
                .descriptor_needs_refresh(
                    context_id,
                    self.context.authority.authority_id(),
                    refresh_window_ms,
                    now_ms,
                )
                .await
        }
    }

    // ========================================================================
    // Channel Operations
    // ========================================================================

    /// Initiate channel establishment with a peer
    pub async fn initiate_channel(
        &self,
        effects: &AuraEffectSystem,
        context_id: ContextId,
        peer: AuthorityId,
    ) -> AgentResult<ChannelResult> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;

        // Enforce guard chain
        let guard = create_send_guard(
            RendezvousCapability::Connect.as_name(),
            context_id,
            self.context.authority.authority_id(),
            FlowCost::new(2), // Handshake cost
        );
        let result = guard
            .evaluate(effects)
            .await
            .map_err(|e| AgentError::effects(format!("guard evaluation failed: {e}")))?;
        if !result.authorized {
            return Err(AgentError::effects(result.denial_reason.unwrap_or_else(
                || "channel initiation not authorized".to_string(),
            )));
        }

        let current_time = effects.current_timestamp().await.unwrap_or(0);

        // Create snapshot for guard evaluation
        let snapshot = self.create_snapshot(effects, context_id).await?;

        // Generate PSK for the channel
        let psk = derive_channel_psk(context_id, self.context.authority.authority_id(), peer);

        // Prepare channel establishment
        let peer_descriptor = self
            .get_peer_descriptor(context_id, peer)
            .await
            .ok_or_else(|| AgentError::invalid("Peer descriptor not found in cache"))?;

        // Retrieve identity keys
        let keys = retrieve_identity_keys(effects, &self.context.authority.authority_id()).await;
        let (local_private_key, _) = keys.unwrap_or(([0u8; 32], [0u8; 32]));

        // Retrieve remote public key from descriptor
        let remote_public_key = peer_descriptor.public_key;

        let establish_path = peer_descriptor
            .advertised_establish_paths()
            .into_iter()
            .next()
            .ok_or_else(|| AgentError::invalid("Peer descriptor has no establish path"))?;

        let outcome = self
            .service
            .prepare_establish_channel(
                &snapshot,
                context_id,
                peer,
                &establish_path,
                &psk,
                &local_private_key,
                &remote_public_key,
                current_time,
                &peer_descriptor,
                effects,
            )
            .await
            .map_err(|e| AgentError::effects(format!("prepare channel failed: {e}")))?;

        // Check guard outcome
        if !outcome.decision.is_allowed() {
            return Ok(ChannelResult {
                success: false,
                context_id,
                peer,
                channel_id: None,
                transport: None,
                error: Some("Guard chain denied channel establishment".to_string()),
            });
        }

        // Track pending channel
        self.registry
            .track_pending_route(
                context_id,
                peer,
                ServiceFamily::Establish,
                Some(establish_path.route.clone()),
                current_time,
            )
            .await;

        // Execute all effect commands via the bridge (includes SendHandshake)
        execute_guard_outcome(outcome, &self.context.authority, context_id, effects).await?;

        Ok(ChannelResult {
            success: true,
            context_id,
            peer,
            channel_id: None, // Will be set after handshake completion
            transport: Some(describe_establish_path(&establish_path)),
            error: None,
        })
    }

    /// Complete channel establishment
    pub async fn complete_channel(
        &self,
        effects: &AuraEffectSystem,
        context_id: ContextId,
        peer: AuthorityId,
        channel_id: [u8; 32],
        epoch: u64,
    ) -> AgentResult<ChannelResult> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;

        // Remove from pending
        self.registry.remove_pending_route(context_id, peer).await;

        // Create channel established fact
        let fact = self
            .service
            .create_channel_established_fact(context_id, peer, channel_id, epoch);

        // Journal the fact
        HandlerUtilities::append_generic_fact(
            &self.context.authority,
            effects,
            context_id,
            RENDEZVOUS_FACT_TYPE_ID.into(),
            &fact.to_bytes(),
        )
        .await?;

        Ok(ChannelResult {
            success: true,
            context_id,
            peer,
            channel_id: Some(channel_id),
            transport: None,
            error: None,
        })
    }

    // ========================================================================
    // Relay Operations
    // ========================================================================

    /// Request relay assistance from another peer
    pub async fn request_relay(
        &self,
        effects: &AuraEffectSystem,
        context_id: ContextId,
        relay: AuthorityId,
        target: AuthorityId,
    ) -> AgentResult<RendezvousResult> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;

        // Enforce guard chain
        let guard = create_send_guard(
            RendezvousCapability::Relay.as_name(),
            context_id,
            self.context.authority.authority_id(),
            FlowCost::new(2), // Relay request cost
        );
        let result = guard
            .evaluate(effects)
            .await
            .map_err(|e| AgentError::effects(format!("guard evaluation failed: {e}")))?;
        if !result.authorized {
            return Err(AgentError::effects(
                result
                    .denial_reason
                    .unwrap_or_else(|| "relay request not authorized".to_string()),
            ));
        }

        // Create snapshot for guard evaluation
        let snapshot = self.create_snapshot(effects, context_id).await?;

        // Prepare relay request
        let outcome = self
            .service
            .prepare_relay_request(context_id, relay, target, &snapshot);

        if !outcome.decision.is_allowed() {
            return Ok(RendezvousResult {
                success: false,
                context_id,
                peer: Some(relay),
                descriptor: None,
                error: Some("Guard chain denied relay request".to_string()),
            });
        }

        Ok(RendezvousResult {
            success: true,
            context_id,
            peer: Some(relay),
            descriptor: None,
            error: None,
        })
    }

    // ========================================================================
    // Helper Methods
    // ========================================================================

    /// Create a guard snapshot from current state
    async fn create_snapshot(
        &self,
        effects: &AuraEffectSystem,
        context_id: ContextId,
    ) -> AgentResult<GuardSnapshot> {
        let now_ms = effects.current_timestamp().await.unwrap_or(0);
        Ok(GuardSnapshot {
            authority_id: self.context.authority.authority_id(),
            context_id,
            flow_budget_remaining: FlowCost::new(1000), // Default budget
            capabilities: self.build_rendezvous_capabilities(effects, now_ms).await?,
            epoch: now_ms / 1000, // Epoch in seconds
        })
    }

    async fn cleanup_registry_state(&self, now_ms: u64, pending_max_age_ms: u64) -> (usize, usize) {
        let removed_desc = self.registry.cleanup_expired_descriptors(now_ms).await;
        let removed_pending = self
            .registry
            .cleanup_stale_pending_routes(now_ms, pending_max_age_ms)
            .await;
        (removed_desc, removed_pending)
    }

    /// Cleanup expired descriptors and stale pending channels.
    ///
    /// Removes descriptors that are no longer valid and pending channels
    /// that have been waiting longer than the max age.
    pub async fn cleanup_expired(&self, now_ms: u64) {
        // Maximum age for pending channels before cleanup (5 minutes)
        const PENDING_CHANNEL_MAX_AGE_MS: u64 = 300_000;

        let (removed_desc, removed_pending) = self
            .cleanup_registry_state(now_ms, PENDING_CHANNEL_MAX_AGE_MS)
            .await;
        if removed_desc > 0 {
            tracing::debug!(removed = removed_desc, "Cleaned up expired descriptors");
        }
        if removed_pending > 0 {
            tracing::debug!(
                removed = removed_pending,
                "Cleaned up stale pending channels"
            );
        }
    }

    // ========================================================================
    // Handshake Processing
    // ========================================================================

    /// Process incoming rendezvous handshake envelopes.
    pub async fn process_handshake_envelopes(
        &self,
        effects: Arc<AuraEffectSystem>,
    ) -> AgentResult<usize> {
        let mut processed = 0usize;

        loop {
            let envelope = match effects.receive_envelope().await {
                Ok(env) => env,
                Err(TransportError::NoMessage) => break,
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to receive rendezvous handshake envelope");
                    break;
                }
            };

            let Some(content_type) = envelope.metadata.get("content-type") else {
                effects.requeue_envelope(envelope);
                break;
            };

            if content_type == HANDSHAKE_INIT_CONTENT_TYPE {
                if let Err(e) = self.handle_handshake_init(effects.clone(), envelope).await {
                    tracing::debug!(error = %e, "Failed to handle rendezvous handshake init");
                }
                processed += 1;
                continue;
            }

            if content_type == HANDSHAKE_COMPLETE_CONTENT_TYPE {
                if let Err(e) = self
                    .handle_handshake_complete(effects.clone(), envelope)
                    .await
                {
                    tracing::debug!(error = %e, "Failed to handle rendezvous handshake complete");
                }
                processed += 1;
                continue;
            }

            effects.requeue_envelope(envelope);
            break;
        }

        Ok(processed)
    }

    async fn handle_handshake_init(
        &self,
        effects: Arc<AuraEffectSystem>,
        envelope: TransportEnvelope,
    ) -> AgentResult<()> {
        if envelope.source == self.context.authority.authority_id() {
            return Ok(());
        }

        let init: aura_rendezvous::protocol::HandshakeInit =
            serde_json::from_slice(&envelope.payload).map_err(|e| {
                AgentError::internal(format!("Failed to decode rendezvous handshake init: {e}"))
            })?;

        let context_id = envelope.context;
        let initiator = envelope.source;

        let snapshot = self.create_snapshot(&effects, context_id).await?;

        let psk = derive_channel_psk(context_id, initiator, self.context.authority.authority_id());

        let keys = retrieve_identity_keys(&*effects, &self.context.authority.authority_id()).await;
        let (local_private_key, _) = keys.unwrap_or(([0u8; 32], [0u8; 32]));

        let (outcome, _channel) = self
            .service
            .prepare_handle_handshake(
                &snapshot,
                context_id,
                initiator,
                init.handshake,
                &psk,
                &local_private_key,
                &*effects,
            )
            .await
            .map_err(|e| AgentError::effects(format!("prepare handle handshake failed: {e}")))?;

        if !outcome.decision.is_allowed() {
            return Err(AgentError::effects(
                "Guard chain denied handshake init".to_string(),
            ));
        }

        execute_guard_outcome(outcome, &self.context.authority, context_id, &effects).await
    }

    async fn handle_handshake_complete(
        &self,
        effects: Arc<AuraEffectSystem>,
        envelope: TransportEnvelope,
    ) -> AgentResult<()> {
        if envelope.source == self.context.authority.authority_id() {
            return Ok(());
        }

        let completion: aura_rendezvous::protocol::HandshakeComplete =
            serde_json::from_slice(&envelope.payload).map_err(|e| {
                AgentError::internal(format!(
                    "Failed to decode rendezvous handshake completion: {e}"
                ))
            })?;

        let context_id = envelope.context;
        let peer = envelope.source;

        let snapshot = self.create_snapshot(&effects, context_id).await?;

        let _channel = self
            .service
            .prepare_handle_completion(&snapshot, context_id, peer, completion, &*effects)
            .await
            .map_err(|e| AgentError::effects(format!("handle completion failed: {e}")))?;

        self.registry.remove_pending_route(context_id, peer).await;

        Ok(())
    }
}

const HANDSHAKE_INIT_CONTENT_TYPE: &str = "application/aura-rendezvous-handshake-init";
const HANDSHAKE_COMPLETE_CONTENT_TYPE: &str = "application/aura-rendezvous-handshake-complete";

fn derive_channel_psk(
    context_id: ContextId,
    initiator: AuthorityId,
    responder: AuthorityId,
) -> [u8; 32] {
    let mut a = initiator.to_bytes();
    let mut b = responder.to_bytes();
    if a > b {
        std::mem::swap(&mut a, &mut b);
    }

    let mut material = Vec::with_capacity(32 + 16 + 16 + 24);
    material.extend_from_slice(b"AURA_RENDEZVOUS_PSK_V1");
    material.extend_from_slice(context_id.as_bytes());
    material.extend_from_slice(&a);
    material.extend_from_slice(&b);

    hash(&material)
}

fn describe_establish_path(path: &EstablishPath) -> String {
    if path.route.is_direct() {
        path.route
            .destination
            .address
            .clone()
            .unwrap_or_else(|| format!("{:?}", path.route.destination.protocol))
    } else {
        format!("{}-hop route", path.route.hops.len())
    }
}

// =============================================================================
// Guard Outcome Execution (effect commands)
// =============================================================================

/// Execute a guard outcome's effect commands.
pub async fn execute_guard_outcome(
    outcome: GuardOutcome,
    authority: &AuthorityContext,
    context_id: ContextId,
    effects: &AuraEffectSystem,
) -> AgentResult<()> {
    if outcome.decision.is_denied() {
        let reason = match &outcome.decision {
            aura_rendezvous::GuardDecision::Deny { reason } => reason.to_string(),
            _ => "Operation denied".to_string(),
        };
        return Err(AgentError::effects(format!(
            "Guard denied operation: {}",
            reason
        )));
    }

    let charge_peer =
        resolve_charge_peer(
            &outcome.effects,
            authority.authority_id(),
            |command| match command {
                EffectCommand::SendHandshake { peer, .. }
                | EffectCommand::SendHandshakeResponse { peer, .. }
                | EffectCommand::RecordReceipt { peer, .. } => Some(*peer),
                _ => None,
            },
        );
    let mut pending_receipt: Option<Receipt> = None;

    for command in outcome.effects {
        execute_effect_command(
            command,
            authority,
            context_id,
            effects,
            charge_peer,
            &mut pending_receipt,
        )
        .await?;
    }

    Ok(())
}

async fn execute_effect_command(
    command: EffectCommand,
    authority: &AuthorityContext,
    context_id: ContextId,
    effects: &AuraEffectSystem,
    charge_peer: AuthorityId,
    pending_receipt: &mut Option<Receipt>,
) -> AgentResult<()> {
    match command {
        EffectCommand::JournalAppend { fact } => {
            execute_journal_append(fact, authority, context_id, effects).await
        }
        EffectCommand::ChargeFlowBudget { cost } => {
            *pending_receipt =
                execute_charge_flow_budget(cost, context_id, charge_peer, effects).await?;
            Ok(())
        }
        EffectCommand::SendHandshake { peer, message } => {
            let receipt = pending_receipt.take();
            execute_send_handshake(peer, message, authority, context_id, receipt, effects).await
        }
        EffectCommand::SendHandshakeResponse { peer, message } => {
            let receipt = pending_receipt.take();
            execute_send_handshake_response(peer, message, authority, context_id, receipt, effects)
                .await
        }
        EffectCommand::RecordReceipt { operation, peer } => {
            execute_record_receipt(operation, peer, context_id, effects).await
        }
    }
}

async fn execute_journal_append(
    fact: RendezvousFact,
    authority: &AuthorityContext,
    context_id: ContextId,
    effects: &AuraEffectSystem,
) -> AgentResult<()> {
    let policy = policy_for(CeremonyFlow::RendezvousSecureChannel);

    if matches!(fact, RendezvousFact::ChannelEstablished { .. })
        && policy.allows_mode(AgreementMode::ConsensusFinalized)
        && !effects.is_testing()
    {
        let tree_state = effects.get_current_state().await.map_err(|e| {
            AgentError::effects(format!("Failed to read tree state for rendezvous: {e}"))
        })?;
        let journal = effects
            .fetch_context_journal(context_id)
            .await
            .map_err(|e| {
                AgentError::effects(format!("Failed to load rendezvous context journal: {e}"))
            })?;
        let context_commitment = context_commitment_from_journal(context_id, &journal)?;
        let prestate = Prestate::new(
            vec![(authority.authority_id(), Hash32(tree_state.root_commitment))],
            context_commitment,
        )
        .map_err(|e| AgentError::effects(format!("Invalid rendezvous prestate: {e}")))?;
        let params = build_consensus_params(context_id, effects, authority.authority_id(), effects)
            .await
            .map_err(|e| {
                AgentError::effects(format!("Failed to build rendezvous consensus params: {e}"))
            })?;
        let commit = run_consensus(&prestate, &fact, params, effects, effects)
            .await
            .map_err(|e| {
                AgentError::effects(format!("Rendezvous consensus finalization failed: {e}"))
            })?;

        effects
            .commit_relational_facts(vec![commit.to_relational_fact()])
            .await
            .map_err(|e| AgentError::effects(format!("Commit rendezvous consensus fact: {e}")))?;
    }

    HandlerUtilities::append_generic_fact(
        authority,
        effects,
        context_id,
        RENDEZVOUS_FACT_TYPE_ID.into(),
        &fact.to_bytes(),
    )
    .await
}

async fn execute_charge_flow_budget(
    cost: aura_core::FlowCost,
    context_id: ContextId,
    peer: AuthorityId,
    effects: &AuraEffectSystem,
) -> AgentResult<Option<Receipt>> {
    if effects.is_testing() {
        return Ok(None);
    }

    let receipt = effects
        .charge_flow(&context_id, &peer, cost)
        .await
        .map_err(|e| {
            AgentError::effects(format!("Failed to charge rendezvous flow budget: {e}"))
        })?;

    Ok(Some(receipt))
}

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

async fn execute_send_handshake(
    peer: AuthorityId,
    message: aura_rendezvous::protocol::HandshakeInit,
    authority: &AuthorityContext,
    context_id: ContextId,
    receipt: Option<Receipt>,
    effects: &AuraEffectSystem,
) -> AgentResult<()> {
    if effects.is_testing() {
        return Ok(());
    }

    let payload = serde_json::to_vec(&message).map_err(|e| {
        AgentError::internal(format!(
            "Failed to serialize rendezvous handshake init: {e}"
        ))
    })?;

    let metadata = build_transport_metadata(
        "application/aura-rendezvous-handshake-init",
        [
            ("protocol-version", "1".to_string()),
            ("rendezvous-epoch", message.handshake.epoch.to_string()),
        ],
    );

    let envelope = TransportEnvelope {
        destination: peer,
        source: authority.authority_id(),
        context: context_id,
        payload,
        metadata,
        receipt: receipt.map(transport_receipt_from_flow),
    };

    effects
        .send_envelope(envelope)
        .await
        .map_err(|e| AgentError::effects(format!("Rendezvous handshake send failed: {e}")))?;
    Ok(())
}

async fn execute_send_handshake_response(
    peer: AuthorityId,
    message: aura_rendezvous::protocol::HandshakeComplete,
    authority: &AuthorityContext,
    context_id: ContextId,
    receipt: Option<Receipt>,
    effects: &AuraEffectSystem,
) -> AgentResult<()> {
    if effects.is_testing() {
        return Ok(());
    }

    let payload = serde_json::to_vec(&message).map_err(|e| {
        AgentError::internal(format!(
            "Failed to serialize rendezvous handshake completion: {e}"
        ))
    })?;

    let metadata = build_transport_metadata(
        "application/aura-rendezvous-handshake-complete",
        [
            ("protocol-version", "1".to_string()),
            ("rendezvous-epoch", message.handshake.epoch.to_string()),
            ("rendezvous-channel-id", hex::encode(message.channel_id)),
        ],
    );

    let envelope = TransportEnvelope {
        destination: peer,
        source: authority.authority_id(),
        context: context_id,
        payload,
        metadata,
        receipt: receipt.map(transport_receipt_from_flow),
    };

    effects
        .send_envelope(envelope)
        .await
        .map_err(|e| AgentError::effects(format!("Rendezvous handshake response failed: {e}")))?;
    Ok(())
}

async fn execute_record_receipt(
    operation: String,
    peer: AuthorityId,
    context_id: ContextId,
    effects: &AuraEffectSystem,
) -> AgentResult<()> {
    if effects.is_testing() {
        return Ok(());
    }

    tracing::debug!(
        operation = %operation,
        peer = %peer,
        context = %context_id,
        "Rendezvous receipt recording requested"
    );
    Ok(())
}

async fn retrieve_identity_keys<E: SecureStorageEffects>(
    effects: &E,
    authority: &AuthorityId,
) -> Option<([u8; 32], [u8; 32])> {
    // Try to retrieve key from epoch 1 (bootstrap epoch)
    let location = SecureStorageLocation::new("signing_keys", format!("{}/1/1", authority));
    let caps = vec![SecureStorageCapability::Read];

    match effects.secure_retrieve(&location, &caps).await {
        Ok(bytes) => {
            if let Ok(pkg) = SingleSignerKeyPackage::from_bytes(&bytes) {
                let signing_key = pkg.signing_key().try_into().unwrap_or([0u8; 32]);
                let verifying_key = pkg.verifying_key().try_into().unwrap_or([0u8; 32]);
                Some((signing_key, verifying_key))
            } else {
                None
            }
        }
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::AgentConfig;
    use crate::runtime::services::{RendezvousManager, RendezvousManagerConfig};
    use aura_core::CapabilityName;
    use aura_effects::time::PhysicalTimeHandler;
    use aura_guards::GuardContextProvider;
    use aura_rendezvous::GuardDecision;
    use base64::Engine;

    fn create_test_authority(seed: u8) -> AuthorityContext {
        let authority_id = AuthorityId::new_from_entropy([seed; 32]);
        AuthorityContext::new(authority_id)
    }

    fn install_biscuit_cache(
        effects: &std::sync::Arc<AuraEffectSystem>,
        capabilities: Vec<CapabilityName>,
    ) {
        let authority = effects.authority_id();
        let issuer = aura_authorization::TokenAuthority::new(authority);
        let token = issuer
            .create_token(authority, capabilities)
            .expect("rendezvous test token should build");
        let engine = base64::engine::general_purpose::STANDARD;
        effects.set_biscuit_cache(crate::runtime::effects::BiscuitCache {
            token_b64: engine.encode(token.to_vec().expect("token should serialize")),
            root_pk_b64: engine.encode(issuer.root_public_key().to_bytes()),
        });
    }

    #[tokio::test]
    async fn test_handler_creation() {
        let authority_context = create_test_authority(50);
        let handler = RendezvousHandler::new(authority_context.clone());

        assert!(handler.is_ok());
    }

    #[tokio::test]
    async fn test_create_snapshot_uses_evaluated_rendezvous_capability_frontier() {
        let authority_context = create_test_authority(51);
        let handler = RendezvousHandler::new(authority_context.clone()).unwrap();
        let config = AgentConfig::default();
        let effects = crate::testing::simulation_effect_system_arc(&config);
        let context_id = ContextId::new_from_entropy([151u8; 32]);

        install_biscuit_cache(&effects, vec![RendezvousCapability::Publish.as_name()]);

        let snapshot = handler
            .create_snapshot(effects.as_ref(), context_id)
            .await
            .unwrap();

        assert_eq!(
            snapshot.capabilities,
            vec![RendezvousCapability::Publish.as_name()]
        );
    }

    #[tokio::test]
    async fn test_create_snapshot_without_biscuit_frontier_has_empty_capabilities() {
        let authority_context = create_test_authority(52);
        let handler = RendezvousHandler::new(authority_context.clone()).unwrap();
        let config = AgentConfig::default();
        let effects = crate::testing::simulation_effect_system_arc(&config);
        let context_id = ContextId::new_from_entropy([152u8; 32]);

        effects.clear_biscuit_cache();

        let snapshot = handler
            .create_snapshot(effects.as_ref(), context_id)
            .await
            .unwrap();

        assert!(snapshot.capabilities.is_empty());
    }

    #[tokio::test]
    async fn test_execute_allowed_outcome() {
        let authority = create_test_authority(80);
        let context_id = ContextId::new_from_entropy([180u8; 32]);
        let config = AgentConfig::default();
        let effects = crate::testing::simulation_effect_system_arc(&config);

        let outcome = GuardOutcome {
            decision: GuardDecision::Allow,
            effects: vec![EffectCommand::ChargeFlowBudget {
                cost: FlowCost::new(1),
            }],
        };

        let result = execute_guard_outcome(outcome, &authority, context_id, &effects).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_denied_outcome() {
        let authority = create_test_authority(81);
        let context_id = ContextId::new_from_entropy([181u8; 32]);
        let config = AgentConfig::default();
        let effects = crate::testing::simulation_effect_system_arc(&config);

        let outcome = GuardOutcome {
            decision: GuardDecision::Deny {
                reason: aura_guards::types::GuardViolation::other("Test denial"),
            },
            effects: vec![],
        };

        let result = execute_guard_outcome(outcome, &authority, context_id, &effects).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_journal_append() {
        let authority = create_test_authority(82);
        let context_id = ContextId::new_from_entropy([182u8; 32]);
        let config = AgentConfig::default();
        let effects = crate::testing::simulation_effect_system_arc(&config);

        let descriptor = RendezvousDescriptor {
            authority_id: authority.authority_id(),
            device_id: None,
            context_id,
            transport_hints: vec![TransportHint::quic_direct("127.0.0.1:8443").unwrap()],
            handshake_psk_commitment: [0u8; 32],
            public_key: [0u8; 32],
            valid_from: 0,
            valid_until: 10000,
            nonce: [0u8; 32],
            nickname_suggestion: None,
        };

        let outcome = GuardOutcome {
            decision: GuardDecision::Allow,
            effects: vec![EffectCommand::JournalAppend {
                fact: RendezvousFact::Descriptor(descriptor),
            }],
        };

        let result = execute_guard_outcome(outcome, &authority, context_id, &effects).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_record_receipt() {
        let authority = create_test_authority(83);
        let context_id = ContextId::new_from_entropy([183u8; 32]);
        let peer = AuthorityId::new_from_entropy([84u8; 32]);
        let config = AgentConfig::default();
        let effects = crate::testing::simulation_effect_system_arc(&config);

        let outcome = GuardOutcome {
            decision: GuardDecision::Allow,
            effects: vec![EffectCommand::RecordReceipt {
                operation: "test_operation".to_string(),
                peer,
            }],
        };

        let result = execute_guard_outcome(outcome, &authority, context_id, &effects).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_multiple_effects() {
        let authority = create_test_authority(85);
        let context_id = ContextId::new_from_entropy([185u8; 32]);
        let peer = AuthorityId::new_from_entropy([86u8; 32]);
        let config = AgentConfig::default();
        let effects = crate::testing::simulation_effect_system_arc(&config);

        let outcome = GuardOutcome {
            decision: GuardDecision::Allow,
            effects: vec![
                EffectCommand::ChargeFlowBudget {
                    cost: FlowCost::new(1),
                },
                EffectCommand::RecordReceipt {
                    operation: "multi_test".to_string(),
                    peer,
                },
            ],
        };

        let result = execute_guard_outcome(outcome, &authority, context_id, &effects).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_publish_descriptor() {
        let authority_context = create_test_authority(51);
        let handler = RendezvousHandler::new(authority_context.clone()).unwrap();

        let config = AgentConfig::default();
        let effects = crate::testing::simulation_effect_system_arc(&config);

        let context_id = ContextId::new_from_entropy([151u8; 32]);
        let result = handler
            .publish_descriptor(
                &effects,
                context_id,
                vec![TransportHint::quic_direct("127.0.0.1:8443").unwrap()],
                [0u8; 32],
                3600000, // 1 hour
            )
            .await
            .unwrap();

        assert!(result.success);
        assert_eq!(result.context_id, context_id);
    }

    #[tokio::test]
    async fn test_cache_peer_descriptor() {
        let authority_context = create_test_authority(52);
        let handler = RendezvousHandler::new(authority_context).unwrap();

        let context_id = ContextId::new_from_entropy([152u8; 32]);
        let peer = AuthorityId::new_from_entropy([53u8; 32]);

        let descriptor = RendezvousDescriptor {
            authority_id: peer,
            device_id: None,
            context_id,
            transport_hints: vec![TransportHint::quic_direct("192.168.1.1:8443").unwrap()],
            handshake_psk_commitment: [0u8; 32],
            public_key: [0u8; 32],
            valid_from: 0,
            valid_until: u64::MAX,
            nonce: [0u8; 32],
            nickname_suggestion: None,
        };

        handler.cache_peer_descriptor(descriptor.clone()).await;

        let cached = handler.get_peer_descriptor(context_id, peer).await;
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().authority_id, peer);
    }

    #[tokio::test]
    async fn test_handler_and_manager_share_descriptor_registry() {
        let authority_context = create_test_authority(53);
        let manager = RendezvousManager::new_with_default_udp(
            authority_context.authority_id(),
            RendezvousManagerConfig::for_testing(),
            Arc::new(PhysicalTimeHandler::new()),
        );
        let handler = RendezvousHandler::new(authority_context)
            .unwrap()
            .with_rendezvous_manager(manager.clone());

        let context_id = ContextId::new_from_entropy([153u8; 32]);
        let peer = AuthorityId::new_from_entropy([54u8; 32]);
        let descriptor = RendezvousDescriptor {
            authority_id: peer,
            device_id: None,
            context_id,
            transport_hints: vec![TransportHint::quic_direct("192.168.1.1:8443").unwrap()],
            handshake_psk_commitment: [0u8; 32],
            public_key: [0u8; 32],
            valid_from: 0,
            valid_until: u64::MAX,
            nonce: [0u8; 32],
            nickname_suggestion: None,
        };

        handler.cache_peer_descriptor(descriptor.clone()).await;

        let cached = manager
            .get_descriptor(context_id, peer)
            .await
            .expect("descriptor should be visible through manager registry");
        assert_eq!(cached.authority_id, peer);
        assert_eq!(
            manager
                .registry()
                .projection(Some(context_id), 1)
                .await
                .descriptors
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn test_initiate_channel() {
        let authority_context = create_test_authority(54);
        let handler = RendezvousHandler::new(authority_context.clone()).unwrap();

        let config = AgentConfig::default();
        let effects = crate::testing::simulation_effect_system_arc(&config);

        let context_id = ContextId::new_from_entropy([154u8; 32]);
        let peer = AuthorityId::new_from_entropy([55u8; 32]);

        // First cache the peer's descriptor
        let descriptor = RendezvousDescriptor {
            authority_id: peer,
            device_id: None,
            context_id,
            transport_hints: vec![TransportHint::quic_direct("192.168.1.1:8443").unwrap()],
            handshake_psk_commitment: [0u8; 32],
            public_key: [0u8; 32],
            valid_from: 0,
            valid_until: u64::MAX,
            nonce: [0u8; 32],
            nickname_suggestion: None,
        };
        handler.cache_peer_descriptor(descriptor).await;

        // Now initiate channel
        let result = handler
            .initiate_channel(&effects, context_id, peer)
            .await
            .unwrap();

        assert!(result.success);
        assert_eq!(result.peer, peer);
        assert!(result.transport.is_some());
    }

    #[tokio::test]
    async fn test_complete_channel() {
        let authority_context = create_test_authority(56);
        let handler = RendezvousHandler::new(authority_context.clone()).unwrap();

        let config = AgentConfig::default();
        let effects = crate::testing::simulation_effect_system_arc(&config);

        let context_id = ContextId::new_from_entropy([156u8; 32]);
        let peer = AuthorityId::new_from_entropy([57u8; 32]);
        let channel_id = [99u8; 32];

        let result = handler
            .complete_channel(&effects, context_id, peer, channel_id, 1)
            .await
            .unwrap();

        assert!(result.success);
        assert_eq!(result.channel_id, Some(channel_id));
    }
}
