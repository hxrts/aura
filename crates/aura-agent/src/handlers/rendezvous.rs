//! Rendezvous Handlers
//!
//! Handlers for rendezvous operations including descriptor publication,
//! channel establishment, and relay coordination.

use super::shared::{HandlerContext, HandlerUtilities};
use crate::core::{AgentError, AgentResult, AuthorityContext};
use crate::runtime::AuraEffectSystem;
use aura_core::effects::{GuardSnapshot, RandomEffects};
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_journal::DomainFact;
use aura_protocol::effects::EffectApiEffects;
use aura_protocol::guards::pure::GuardChain;
use aura_protocol::guards::send_guard::create_send_guard;
use aura_rendezvous::{
    RendezvousDescriptor, RendezvousFact, RendezvousService, SelectedTransport, TransportHint,
    RENDEZVOUS_FACT_TYPE_ID,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Result of a rendezvous operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RendezvousResult {
    /// Whether the operation succeeded
    pub success: bool,
    /// Context ID affected
    pub context_id: ContextId,
    /// Peer involved (if applicable)
    pub peer: Option<AuthorityId>,
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
pub struct RendezvousHandler {
    context: HandlerContext,
    /// Inner rendezvous service for guard chain operations
    service: Arc<RwLock<RendezvousService>>,
    /// Pending channel establishments
    pending_channels: Arc<RwLock<HashMap<(ContextId, AuthorityId), PendingChannel>>>,
}

/// Pending channel state
#[derive(Debug, Clone)]
struct PendingChannel {
    context_id: ContextId,
    peer: AuthorityId,
    initiated_at: u64,
    transport: SelectedTransport,
}

impl RendezvousHandler {
    /// Create a new rendezvous handler
    pub fn new(authority: AuthorityContext, guard_chain: Arc<GuardChain>) -> AgentResult<Self> {
        HandlerUtilities::validate_authority_context(&authority)?;

        let service = RendezvousService::new(authority.authority_id, guard_chain);

        Ok(Self {
            context: HandlerContext::new(authority),
            service: Arc::new(RwLock::new(service)),
            pending_channels: Arc::new(RwLock::new(HashMap::new())),
        })
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
        psk_commitment: [u8; 32],
        validity_duration_ms: u64,
    ) -> AgentResult<RendezvousResult> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;

        // Enforce guard (unless testing)
        if !effects.is_testing() {
            let guard = create_send_guard(
                "rendezvous:publish_descriptor".to_string(),
                context_id,
                self.context.authority.authority_id,
                1, // Low cost for descriptor publication
            );
            let result = guard
                .evaluate(effects)
                .await
                .map_err(|e| AgentError::effects(format!("guard evaluation failed: {e}")))?;
            if !result.authorized {
                return Err(AgentError::effects(
                    result
                        .denial_reason
                        .unwrap_or_else(|| "descriptor publish not authorized".to_string()),
                ));
            }
        }

        let current_time = effects.current_timestamp().await.unwrap_or(0);

        // Generate nonce
        let nonce_uuid = effects.random_uuid().await;
        let mut nonce = [0u8; 32];
        nonce[..16].copy_from_slice(nonce_uuid.as_bytes());

        // Create snapshot for guard evaluation
        let snapshot = self.create_snapshot(effects, context_id).await?;

        // Prepare the descriptor through the service
        let (fact, outcome) = {
            let service = self.service.read().await;
            service
                .prepare_publish_descriptor(
                    context_id,
                    transport_hints,
                    psk_commitment,
                    current_time,
                    current_time.saturating_add(validity_duration_ms),
                    nonce,
                    &snapshot,
                )
                .map_err(|e| AgentError::effects(format!("prepare descriptor failed: {e}")))?
        };

        // Check guard outcome
        if !outcome.decision.is_authorized() {
            return Ok(RendezvousResult {
                success: false,
                context_id,
                peer: None,
                error: Some("Guard chain denied descriptor publication".to_string()),
            });
        }

        // Journal the descriptor fact
        HandlerUtilities::append_generic_fact(
            &self.context.authority,
            effects,
            context_id,
            RENDEZVOUS_FACT_TYPE_ID,
            &fact.to_bytes(),
        )
        .await?;

        // Cache our own descriptor
        if let RendezvousFact::Descriptor(desc) = &fact {
            let mut service = self.service.write().await;
            service.cache_descriptor(desc.clone());
        }

        Ok(RendezvousResult {
            success: true,
            context_id,
            peer: None,
            error: None,
        })
    }

    /// Cache a peer's descriptor received via journal sync
    pub async fn cache_peer_descriptor(&self, descriptor: RendezvousDescriptor) {
        let mut service = self.service.write().await;
        service.cache_descriptor(descriptor);
    }

    /// Get a peer's cached descriptor
    pub async fn get_peer_descriptor(
        &self,
        context_id: ContextId,
        peer: AuthorityId,
    ) -> Option<RendezvousDescriptor> {
        let service = self.service.read().await;
        service.get_cached_descriptor(context_id, peer).cloned()
    }

    /// Check if our descriptor needs refresh
    pub async fn needs_descriptor_refresh(
        &self,
        context_id: ContextId,
        now_ms: u64,
        refresh_window_ms: u64,
    ) -> bool {
        let service = self.service.read().await;
        service.needs_refresh(context_id, now_ms, refresh_window_ms)
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

        // Enforce guard (unless testing)
        if !effects.is_testing() {
            let guard = create_send_guard(
                "rendezvous:initiate_channel".to_string(),
                context_id,
                self.context.authority.authority_id,
                2, // Handshake cost
            );
            let result = guard
                .evaluate(effects)
                .await
                .map_err(|e| AgentError::effects(format!("guard evaluation failed: {e}")))?;
            if !result.authorized {
                return Err(AgentError::effects(
                    result
                        .denial_reason
                        .unwrap_or_else(|| "channel initiation not authorized".to_string()),
                ));
            }
        }

        let current_time = effects.current_timestamp().await.unwrap_or(0);

        // Create snapshot for guard evaluation
        let snapshot = self.create_snapshot(effects, context_id).await?;

        // Prepare channel establishment
        let (transport, outcome) = {
            let service = self.service.read().await;
            service
                .prepare_establish_channel(context_id, peer, &snapshot)
                .map_err(|e| AgentError::effects(format!("prepare channel failed: {e}")))?
        };

        // Check guard outcome
        if !outcome.decision.is_authorized() {
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
        {
            let mut pending = self.pending_channels.write().await;
            pending.insert(
                (context_id, peer),
                PendingChannel {
                    context_id,
                    peer,
                    initiated_at: current_time,
                    transport: transport.clone(),
                },
            );
        }

        let transport_str = match &transport {
            SelectedTransport::Direct { addr } => format!("direct:{}", addr),
            SelectedTransport::Relayed { relay } => format!("relay:{}", relay),
            SelectedTransport::Tor { onion_addr } => format!("tor:{}", onion_addr),
        };

        Ok(ChannelResult {
            success: true,
            context_id,
            peer,
            channel_id: None, // Will be set after handshake completion
            transport: Some(transport_str),
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
        {
            let mut pending = self.pending_channels.write().await;
            pending.remove(&(context_id, peer));
        }

        // Create channel established fact
        let fact = {
            let service = self.service.read().await;
            service.create_channel_established_fact(context_id, peer, channel_id, epoch)
        };

        // Journal the fact
        HandlerUtilities::append_generic_fact(
            &self.context.authority,
            effects,
            context_id,
            RENDEZVOUS_FACT_TYPE_ID,
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

        // Enforce guard (unless testing)
        if !effects.is_testing() {
            let guard = create_send_guard(
                "rendezvous:relay_request".to_string(),
                context_id,
                self.context.authority.authority_id,
                2, // Relay request cost
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
        }

        // Create snapshot for guard evaluation
        let snapshot = self.create_snapshot(effects, context_id).await?;

        // Prepare relay request
        let outcome = {
            let service = self.service.read().await;
            service
                .prepare_relay_request(context_id, relay, target, &snapshot)
                .map_err(|e| AgentError::effects(format!("prepare relay failed: {e}")))?
        };

        if !outcome.decision.is_authorized() {
            return Ok(RendezvousResult {
                success: false,
                context_id,
                peer: Some(relay),
                error: Some("Guard chain denied relay request".to_string()),
            });
        }

        Ok(RendezvousResult {
            success: true,
            context_id,
            peer: Some(relay),
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
        use aura_core::effects::{FlowBudgetView, MetadataView};
        use aura_core::time::{PhysicalTime, TimeStamp};
        use aura_core::Cap;

        let current_time = effects.current_timestamp().await.unwrap_or(0);

        // Build budgets from effects
        let mut budgets = HashMap::new();
        budgets.insert((context_id, self.context.authority.authority_id), 1000u32);

        // Generate random seed
        let seed_uuid = effects.random_uuid().await;
        let mut rng_seed = [0u8; 32];
        rng_seed[..16].copy_from_slice(seed_uuid.as_bytes());

        Ok(GuardSnapshot {
            now: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: current_time,
                uncertainty: None,
            }),
            caps: Cap::default(),
            budgets: FlowBudgetView::new(budgets),
            metadata: MetadataView::default(),
            rng_seed,
        })
    }

    /// Cleanup expired descriptors
    pub async fn cleanup_expired(&self, now_ms: u64) {
        let mut service = self.service.write().await;
        service.cleanup_expired_descriptors(now_ms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::context::RelationalContext;
    use crate::core::AgentConfig;
    use crate::runtime::effects::AuraEffectSystem;
    use aura_protocol::guards::pure::GuardChain;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    fn create_test_authority(seed: u8) -> AuthorityContext {
        let authority_id = AuthorityId::new_from_entropy([seed; 32]);
        let mut authority_context = AuthorityContext::new(authority_id);
        authority_context.add_context(RelationalContext {
            context_id: ContextId::new_from_entropy([seed + 100; 32]),
            participants: vec![],
            metadata: Default::default(),
        });
        authority_context
    }

    #[tokio::test]
    async fn test_handler_creation() {
        let authority_context = create_test_authority(50);
        let guard_chain = Arc::new(GuardChain::standard());
        let handler = RendezvousHandler::new(authority_context.clone(), guard_chain);

        assert!(handler.is_ok());
    }

    #[tokio::test]
    async fn test_publish_descriptor() {
        let authority_context = create_test_authority(51);
        let guard_chain = Arc::new(GuardChain::standard());
        let handler = RendezvousHandler::new(authority_context.clone(), guard_chain).unwrap();

        let config = AgentConfig::default();
        let effects = Arc::new(RwLock::new(AuraEffectSystem::testing(&config).unwrap()));
        let effects_guard = effects.read().await;

        let context_id = ContextId::new_from_entropy([151u8; 32]);
        let result = handler
            .publish_descriptor(
                &effects_guard,
                context_id,
                vec![TransportHint::QuicDirect {
                    addr: "127.0.0.1:8443".to_string(),
                }],
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
        let guard_chain = Arc::new(GuardChain::standard());
        let handler = RendezvousHandler::new(authority_context, guard_chain).unwrap();

        let context_id = ContextId::new_from_entropy([152u8; 32]);
        let peer = AuthorityId::new_from_entropy([53u8; 32]);

        let descriptor = RendezvousDescriptor {
            authority_id: peer,
            context_id,
            transport_hints: vec![TransportHint::QuicDirect {
                addr: "192.168.1.1:8443".to_string(),
            }],
            handshake_psk_commitment: [0u8; 32],
            valid_from: 0,
            valid_until: u64::MAX,
            nonce: [0u8; 32],
        };

        handler.cache_peer_descriptor(descriptor.clone()).await;

        let cached = handler.get_peer_descriptor(context_id, peer).await;
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().authority_id, peer);
    }

    #[tokio::test]
    async fn test_initiate_channel() {
        let authority_context = create_test_authority(54);
        let guard_chain = Arc::new(GuardChain::standard());
        let handler = RendezvousHandler::new(authority_context.clone(), guard_chain).unwrap();

        let config = AgentConfig::default();
        let effects = Arc::new(RwLock::new(AuraEffectSystem::testing(&config).unwrap()));
        let effects_guard = effects.read().await;

        let context_id = ContextId::new_from_entropy([154u8; 32]);
        let peer = AuthorityId::new_from_entropy([55u8; 32]);

        // First cache the peer's descriptor
        let descriptor = RendezvousDescriptor {
            authority_id: peer,
            context_id,
            transport_hints: vec![TransportHint::QuicDirect {
                addr: "192.168.1.1:8443".to_string(),
            }],
            handshake_psk_commitment: [0u8; 32],
            valid_from: 0,
            valid_until: u64::MAX,
            nonce: [0u8; 32],
        };
        handler.cache_peer_descriptor(descriptor).await;

        // Now initiate channel
        let result = handler
            .initiate_channel(&effects_guard, context_id, peer)
            .await
            .unwrap();

        assert!(result.success);
        assert_eq!(result.peer, peer);
        assert!(result.transport.is_some());
    }

    #[tokio::test]
    async fn test_complete_channel() {
        let authority_context = create_test_authority(56);
        let guard_chain = Arc::new(GuardChain::standard());
        let handler = RendezvousHandler::new(authority_context.clone(), guard_chain).unwrap();

        let config = AgentConfig::default();
        let effects = Arc::new(RwLock::new(AuraEffectSystem::testing(&config).unwrap()));
        let effects_guard = effects.read().await;

        let context_id = ContextId::new_from_entropy([156u8; 32]);
        let peer = AuthorityId::new_from_entropy([57u8; 32]);
        let channel_id = [99u8; 32];

        let result = handler
            .complete_channel(&effects_guard, context_id, peer, channel_id, 1)
            .await
            .unwrap();

        assert!(result.success);
        assert_eq!(result.channel_id, Some(channel_id));
    }
}
