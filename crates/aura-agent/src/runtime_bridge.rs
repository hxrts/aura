//! RuntimeBridge implementation for AuraAgent
//!
//! This module implements the `RuntimeBridge` trait from `aura-app` for `AuraAgent`,
//! enabling the dependency inversion where `aura-app` defines the trait and
//! `aura-agent` provides the implementation.

use crate::core::AuraAgent;
use async_trait::async_trait;
use aura_app::runtime_bridge::{
    InvitationBridgeStatus, InvitationBridgeType, InvitationInfo, LanPeerInfo, RendezvousStatus,
    RuntimeBridge, SettingsBridgeState, SyncStatus,
};
use aura_app::IntentError;
use aura_core::effects::{StorageEffects, ThresholdSigningEffects, TransportEffects};
use aura_core::identifiers::AuthorityId;
use aura_core::threshold::{SigningContext, ThresholdConfig, ThresholdSignature};
use aura_core::tree::{AttestedOp, TreeOp};
use aura_core::DeviceId;
use aura_core::EffectContext;
use aura_effects::ReactiveHandler;
use aura_journal::fact::RelationalFact;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

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

const ACCOUNT_CONFIG_KEYS: [&str; 2] = ["account.json", "demo-account.json"];

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct StoredAccountConfig {
    #[serde(default)]
    authority_id: Option<String>,
    #[serde(default)]
    context_id: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    mfa_policy: Option<String>,
    #[serde(default)]
    created_at: Option<u64>,
}

impl AgentRuntimeBridge {
    async fn try_load_account_config(
        &self,
    ) -> Result<Option<(String, StoredAccountConfig)>, IntentError> {
        let effects = self.agent.runtime().effects();

        for key in ACCOUNT_CONFIG_KEYS {
            let bytes = effects
                .retrieve(key)
                .await
                .map_err(|e| IntentError::storage_error(format!("Failed to read {key}: {e}")))?;

            let Some(bytes) = bytes else {
                continue;
            };

            let config: StoredAccountConfig = serde_json::from_slice(&bytes)
                .map_err(|e| IntentError::internal_error(format!("Failed to parse {key}: {e}")))?;

            return Ok(Some((key.to_string(), config)));
        }

        Ok(None)
    }

    async fn load_account_config(&self) -> Result<(String, StoredAccountConfig), IntentError> {
        self.try_load_account_config().await?.ok_or_else(|| {
            IntentError::validation_failed("No account config found. Create an account first.")
        })
    }

    async fn store_account_config(
        &self,
        key: &str,
        config: &StoredAccountConfig,
    ) -> Result<(), IntentError> {
        let content = serde_json::to_vec_pretty(config)
            .map_err(|e| IntentError::internal_error(format!("Failed to serialize {key}: {e}")))?;

        let effects = self.agent.runtime().effects();
        effects
            .store(key, content)
            .await
            .map_err(|e| IntentError::storage_error(format!("Failed to write {key}: {e}")))?;

        Ok(())
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

        let is_running = if let Some(sync) = self.agent.runtime().sync() {
            sync.is_running().await
        } else {
            false
        };

        SyncStatus {
            is_running,
            connected_peers: transport_stats.active_channels as usize,
            last_sync_ms: None, // Would need to track this in SyncServiceManager
            pending_facts: 0,   // Would need to track this in SyncServiceManager
        }
    }

    async fn is_peer_online(&self, peer: AuthorityId) -> bool {
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
            Err(IntentError::no_agent("Sync service not available"))
        }
    }

    async fn sync_with_peer(&self, peer_id: &str) -> Result<(), IntentError> {
        if let Some(sync) = self.agent.runtime().sync() {
            // Parse peer_id into DeviceId
            let device_id: DeviceId = peer_id.into();

            // Create a single-element vector for the target peer
            let peers = vec![device_id];

            // Get the effects from agent runtime
            let effects = self.agent.runtime().effects();

            // Sync with the specific peer
            sync.sync_with_peers(&effects, peers)
                .await
                .map_err(|e| IntentError::internal_error(format!("Sync failed: {}", e)))
        } else {
            Err(IntentError::no_agent("Sync service not available"))
        }
    }

    // =========================================================================
    // Peer Discovery
    // =========================================================================

    async fn get_discovered_peers(&self) -> Vec<AuthorityId> {
        if let Some(rendezvous) = self.agent.runtime().rendezvous() {
            rendezvous.list_cached_peers().await
        } else {
            Vec::new()
        }
    }

    async fn get_rendezvous_status(&self) -> RendezvousStatus {
        if let Some(rendezvous) = self.agent.runtime().rendezvous() {
            RendezvousStatus {
                is_running: rendezvous.is_running().await,
                cached_peers: rendezvous.list_cached_peers().await.len(),
            }
        } else {
            RendezvousStatus::default()
        }
    }

    async fn trigger_discovery(&self) -> Result<(), IntentError> {
        if let Some(rendezvous) = self.agent.runtime().rendezvous() {
            // Trigger an on-demand discovery refresh
            rendezvous.trigger_discovery().await.map_err(|e| {
                IntentError::internal_error(format!("Failed to trigger discovery: {}", e))
            })
        } else {
            Err(IntentError::no_agent("Rendezvous service not available"))
        }
    }

    // =========================================================================
    // LAN Discovery
    // =========================================================================

    async fn get_lan_peers(&self) -> Vec<LanPeerInfo> {
        if let Some(rendezvous) = self.agent.runtime().rendezvous() {
            rendezvous
                .list_lan_discovered_peers()
                .await
                .into_iter()
                .map(|peer| LanPeerInfo {
                    authority_id: peer.authority_id,
                    address: peer.source_addr.to_string(),
                    discovered_at_ms: peer.discovered_at_ms,
                    display_name: peer.descriptor.display_name.clone(),
                })
                .collect()
        } else {
            Vec::new()
        }
    }

    async fn send_lan_invitation(
        &self,
        _peer: &LanPeerInfo,
        _invitation_code: &str,
    ) -> Result<(), IntentError> {
        // LAN invitation sending is not yet implemented in RendezvousManager
        // Future: Add direct peer-to-peer invitation exchange over LAN
        Err(IntentError::internal_error(
            "LAN invitation sending not yet implemented",
        ))
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
        threshold_k: u16,
        total_n: u16,
        guardian_ids: &[String],
    ) -> Result<(u64, Vec<Vec<u8>>, Vec<u8>), IntentError> {
        let authority = self.agent.authority_id();
        let signing_service = self.agent.threshold_signing();

        // Rotate keys to a new threshold configuration
        // The service returns (new_epoch, key_packages, public_key_bytes)
        // where public_key_bytes is already serialized
        signing_service
            .rotate_keys(&authority, threshold_k, total_n, guardian_ids)
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to rotate guardian keys: {}", e))
            })
    }

    async fn commit_guardian_key_rotation(&self, new_epoch: u64) -> Result<(), IntentError> {
        let authority = self.agent.authority_id();
        let signing_service = self.agent.threshold_signing();

        signing_service
            .commit_key_rotation(&authority, new_epoch)
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to commit key rotation: {}", e))
            })
    }

    async fn rollback_guardian_key_rotation(&self, failed_epoch: u64) -> Result<(), IntentError> {
        let authority = self.agent.authority_id();
        let signing_service = self.agent.threshold_signing();

        signing_service
            .rollback_key_rotation(&authority, failed_epoch)
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to rollback key rotation: {}", e))
            })
    }

    async fn initiate_guardian_ceremony(
        &self,
        threshold_k: u16,
        total_n: u16,
        guardian_ids: &[String],
    ) -> Result<String, IntentError> {
        // Step 1: Generate FROST keys at new epoch
        let (new_epoch, key_packages, _public_key) = self
            .rotate_guardian_keys(threshold_k, total_n, guardian_ids)
            .await?;

        // Step 2: Create ceremony ID (epoch provides uniqueness)
        // Using a monotonic counter for additional uniqueness within same process
        use std::sync::atomic::{AtomicU64, Ordering};
        static CEREMONY_COUNTER: AtomicU64 = AtomicU64::new(0);
        let counter = CEREMONY_COUNTER.fetch_add(1, Ordering::Relaxed);
        let ceremony_id = format!("ceremony-{}-{}", new_epoch, counter);

        tracing::info!(
            ceremony_id = %ceremony_id,
            new_epoch,
            threshold_k,
            total_n,
            "Guardian ceremony initiated, sending invitations to {} guardians",
            guardian_ids.len()
        );

        // Step 3: Register ceremony with tracker
        let tracker = self.agent.ceremony_tracker().await;
        tracker
            .register(
                ceremony_id.clone(),
                threshold_k,
                total_n,
                guardian_ids.to_vec(),
                new_epoch,
            )
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to register ceremony: {}", e))
            })?;

        // Step 4: Send guardian invitations with key packages
        // This routes through the proper aura-recovery protocol
        let recovery_service = self.agent.recovery().map_err(|e| {
            IntentError::service_error(format!("Recovery service unavailable: {}", e))
        })?;

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

        for (idx, guardian_id) in guardian_ids.iter().enumerate() {
            let key_package = &key_packages[idx];

            tracing::debug!(
                guardian_id = %guardian_id,
                key_package_size = key_package.len(),
                "Sending guardian invitation through protocol"
            );

            // Send through proper protocol (not mock!)
            // This should trigger the choreography-based guardian ceremony
            recovery_service
                .send_guardian_invitation(
                    guardian_id,
                    &ceremony_id,
                    threshold_k,
                    total_n,
                    &all_guardian_authority_ids,
                    new_epoch,
                    key_package,
                )
                .await
                .map_err(|e| {
                    IntentError::internal_error(format!(
                        "Failed to send guardian invitation to {}: {}",
                        guardian_id, e
                    ))
                })?;
        }

        tracing::info!(
            ceremony_id = %ceremony_id,
            "All guardian invitations sent successfully"
        );

        Ok(ceremony_id)
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

        let tracker = self.agent.ceremony_tracker().await;

        let state = tracker
            .get(ceremony_id)
            .await
            .map_err(|e| IntentError::validation_failed(format!("Ceremony not found: {}", e)))?;

        Ok(aura_app::runtime_bridge::CeremonyStatus {
            ceremony_id: ceremony_id.to_string(),
            accepted_count: state.accepted_guardians.len() as u16,
            total_count: state.total_n,
            threshold: state.threshold_k,
            is_complete: state.is_committed,
            has_failed: state.has_failed,
            accepted_guardians: state.accepted_guardians.clone(),
            error_message: state.error_message.clone(),
            pending_epoch: Some(state.new_epoch),
        })
    }

    // =========================================================================
    // Invitation Operations
    // =========================================================================

    async fn export_invitation(&self, invitation_id: &str) -> Result<String, IntentError> {
        // Get the invitation service from the agent
        let invitation_service = self.agent.invitations().map_err(|e| {
            IntentError::service_error(format!("Invitation service unavailable: {}", e))
        })?;

        // Export the invitation code
        invitation_service
            .export_code(invitation_id)
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
        let invitation_service = self.agent.invitations().map_err(|e| {
            IntentError::service_error(format!("Invitation service unavailable: {}", e))
        })?;

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
        let invitation_service = self.agent.invitations().map_err(|e| {
            IntentError::service_error(format!("Invitation service unavailable: {}", e))
        })?;

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
        block_id: String,
        message: Option<String>,
        ttl_ms: Option<u64>,
    ) -> Result<InvitationInfo, IntentError> {
        let invitation_service = self.agent.invitations().map_err(|e| {
            IntentError::service_error(format!("Invitation service unavailable: {}", e))
        })?;

        let invitation = invitation_service
            .invite_to_channel(receiver, block_id, message, ttl_ms)
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to create channel invitation: {}", e))
            })?;

        Ok(convert_invitation_to_bridge_info(&invitation))
    }

    async fn accept_invitation(&self, invitation_id: &str) -> Result<(), IntentError> {
        let invitation_service = self.agent.invitations().map_err(|e| {
            IntentError::service_error(format!("Invitation service unavailable: {}", e))
        })?;

        let result = invitation_service
            .accept(invitation_id)
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
        let invitation_service = self.agent.invitations().map_err(|e| {
            IntentError::service_error(format!("Invitation service unavailable: {}", e))
        })?;

        let result = invitation_service
            .decline(invitation_id)
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
        let invitation_service = self.agent.invitations().map_err(|e| {
            IntentError::service_error(format!("Invitation service unavailable: {}", e))
        })?;

        let result = invitation_service
            .cancel(invitation_id)
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
        let invitation_service = self.agent.invitations().map_err(|e| {
            IntentError::service_error(format!("Invitation service unavailable: {}", e))
        })?;

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
        // When implemented, would provide: display_name, mfa_policy from profile facts
        let (display_name, mfa_policy) = match self.try_load_account_config().await {
            Ok(Some((_key, config))) => (
                config.display_name.unwrap_or_default(),
                config.mfa_policy.unwrap_or_else(|| "disabled".to_string()),
            ),
            Ok(None) => (String::new(), "disabled".to_string()),
            Err(e) => {
                tracing::warn!("Failed to load account config for settings: {}", e);
                (String::new(), "disabled".to_string())
            }
        };

        SettingsBridgeState {
            display_name,
            mfa_policy,
            threshold_k,
            threshold_n,
            device_count: 1, // Requires device registry service
            contact_count,
        }
    }

    async fn set_display_name(&self, name: &str) -> Result<(), IntentError> {
        let (key, mut config) = self.load_account_config().await?;
        config.display_name = Some(name.to_string());
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
        // Verify the ceremony exists and get tracker
        let tracker = self.agent.ceremony_tracker().await;
        let _state = tracker
            .get(ceremony_id)
            .await
            .map_err(|e| IntentError::validation_failed(format!("Ceremony not found: {}", e)))?;

        if accept {
            // Record acceptance in ceremony tracker
            let guardian_id = self.agent.authority_id().to_string();
            tracker
                .mark_accepted(ceremony_id, guardian_id)
                .await
                .map_err(|e| {
                    IntentError::internal_error(format!(
                        "Failed to record guardian acceptance: {}",
                        e
                    ))
                })?;
            Ok(())
        } else {
            // Mark ceremony as failed due to decline
            tracker
                .mark_failed(
                    ceremony_id,
                    Some("Guardian declined invitation".to_string()),
                )
                .await
                .map_err(|e| {
                    IntentError::internal_error(format!("Failed to record guardian decline: {}", e))
                })?;
            Ok(())
        }
    }

    // =========================================================================
    // Time Operations
    // =========================================================================

    fn current_time_ms(&self) -> u64 {
        // RuntimeBridge currently exposes a synchronous time accessor.
        //
        // Aura's unified time system is effect-injected and async; until this bridge is
        // updated to support an async time call (or to return an explicitly provided
        // timestamp), return a deterministic value.
        0
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
// Helper functions
// ============================================================================

/// Convert domain Invitation to bridge InvitationInfo
fn convert_invitation_to_bridge_info(
    invitation: &crate::handlers::invitation::Invitation,
) -> InvitationInfo {
    InvitationInfo {
        invitation_id: invitation.invitation_id.clone(),
        sender_id: invitation.sender_id,
        receiver_id: invitation.receiver_id,
        invitation_type: convert_invitation_type_to_bridge(&invitation.invitation_type),
        status: convert_invitation_status_to_bridge(&invitation.status),
        created_at_ms: invitation.created_at,
        expires_at_ms: invitation.expires_at,
        message: invitation.message.clone(),
    }
}

/// Convert domain InvitationType to bridge InvitationBridgeType
fn convert_invitation_type_to_bridge(
    inv_type: &crate::handlers::invitation::InvitationType,
) -> InvitationBridgeType {
    match inv_type {
        crate::handlers::invitation::InvitationType::Contact { nickname } => {
            InvitationBridgeType::Contact {
                nickname: nickname.clone(),
            }
        }
        crate::handlers::invitation::InvitationType::Guardian { subject_authority } => {
            InvitationBridgeType::Guardian {
                subject_authority: *subject_authority,
            }
        }
        crate::handlers::invitation::InvitationType::Channel { block_id } => {
            InvitationBridgeType::Channel {
                block_id: block_id.clone(),
            }
        }
    }
}

/// Convert domain InvitationStatus to bridge InvitationBridgeStatus
fn convert_invitation_status_to_bridge(
    status: &crate::handlers::invitation::InvitationStatus,
) -> InvitationBridgeStatus {
    match status {
        crate::handlers::invitation::InvitationStatus::Pending => InvitationBridgeStatus::Pending,
        crate::handlers::invitation::InvitationStatus::Accepted => InvitationBridgeStatus::Accepted,
        crate::handlers::invitation::InvitationStatus::Declined => InvitationBridgeStatus::Declined,
        crate::handlers::invitation::InvitationStatus::Cancelled => {
            InvitationBridgeStatus::Cancelled
        }
        crate::handlers::invitation::InvitationStatus::Expired => InvitationBridgeStatus::Expired,
    }
}

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
