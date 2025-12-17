//! RuntimeBridge implementation for AuraAgent
//!
//! This module implements the `RuntimeBridge` trait from `aura-app` for `AuraAgent`,
//! enabling the dependency inversion where `aura-app` defines the trait and
//! `aura-agent` provides the implementation.

use crate::core::AuraAgent;
use async_trait::async_trait;
use aura_app::runtime_bridge::{LanPeerInfo, RendezvousStatus, RuntimeBridge, SyncStatus};
use aura_app::IntentError;
use aura_core::domain::FactValue;
use aura_core::effects::{JournalEffects, ThresholdSigningEffects};
use aura_core::identifiers::AuthorityId;
use aura_core::threshold::{SigningContext, ThresholdConfig, ThresholdSignature};
use aura_core::time::TimeStamp;
use aura_core::tree::{AttestedOp, TreeOp};
use aura_core::DeviceId;
use aura_journal::JournalFact;
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

#[async_trait]
impl RuntimeBridge for AgentRuntimeBridge {
    // =========================================================================
    // Identity & Authority
    // =========================================================================

    fn authority_id(&self) -> AuthorityId {
        self.agent.authority_id()
    }

    // =========================================================================
    // Fact Persistence
    // =========================================================================

    async fn persist_facts(&self, facts: &[JournalFact]) -> Result<(), IntentError> {
        if facts.is_empty() {
            return Ok(());
        }

        // Get the effect system
        let effects = self.agent.runtime().effects();
        let effects_guard = effects.read().await;

        // Get the current journal
        let mut journal = effects_guard
            .get_journal()
            .await
            .map_err(|e| IntentError::internal_error(format!("Failed to get journal: {}", e)))?;

        // Add each fact to the journal
        for fact in facts {
            let timestamp_ms = extract_timestamp_ms(&fact.timestamp);
            let actor_id = fact.source_authority.to_string();
            let key = format!("fact:{}:{}", actor_id, timestamp_ms);

            journal.facts.insert_with_context(
                key,
                FactValue::String(fact.content.clone()),
                actor_id,
                timestamp_ms,
                None,
            );
        }

        // Persist the updated journal
        effects_guard.persist_journal(&journal).await.map_err(|e| {
            IntentError::internal_error(format!("Failed to persist journal: {}", e))
        })?;

        Ok(())
    }

    // =========================================================================
    // Sync Operations
    // =========================================================================

    async fn get_sync_status(&self) -> SyncStatus {
        if let Some(sync) = self.agent.runtime().sync() {
            SyncStatus {
                is_running: sync.is_running().await,
                connected_peers: sync.peers().await.len(),
                last_sync_ms: None, // Would need to track this in SyncServiceManager
                pending_facts: 0,   // Would need to track this in SyncServiceManager
            }
        } else {
            SyncStatus::default()
        }
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
                    // RendezvousDescriptor doesn't have display_name; use None for now
                    display_name: None,
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
        let signing_service = self.agent.threshold_signing().await;

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
        let signing_service = self.agent.threshold_signing().await;

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
        let signing_service = self.agent.threshold_signing().await;
        signing_service.threshold_config(&authority).await
    }

    async fn has_signing_capability(&self) -> bool {
        let authority = self.agent.authority_id();
        let signing_service = self.agent.threshold_signing().await;
        signing_service.has_signing_capability(&authority).await
    }

    async fn get_public_key_package(&self) -> Option<Vec<u8>> {
        let authority = self.agent.authority_id();
        let signing_service = self.agent.threshold_signing().await;
        signing_service.public_key_package(&authority).await
    }

    async fn sign_with_context(
        &self,
        context: SigningContext,
    ) -> Result<ThresholdSignature, IntentError> {
        let signing_service = self.agent.threshold_signing().await;
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
        let signing_service = self.agent.threshold_signing().await;

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
        let signing_service = self.agent.threshold_signing().await;

        signing_service
            .commit_key_rotation(&authority, new_epoch)
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to commit key rotation: {}", e))
            })
    }

    async fn rollback_guardian_key_rotation(&self, failed_epoch: u64) -> Result<(), IntentError> {
        let authority = self.agent.authority_id();
        let signing_service = self.agent.threshold_signing().await;

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

        // Step 2: Create ceremony ID
        let ceremony_id = format!("ceremony-{}-{}", new_epoch, uuid::Uuid::new_v4());

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
        let recovery_service = self.agent.recovery().await.map_err(|e| {
            IntentError::service_error(format!("Recovery service unavailable: {}", e))
        })?;

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
            is_complete: state.accepted_guardians.len() >= state.threshold_k as usize,
            has_failed: state.has_failed,
            accepted_guardians: state.accepted_guardians.clone(),
            error_message: state.error_message.clone(),
        })
    }

    // =========================================================================
    // Invitation Operations
    // =========================================================================

    async fn export_invitation(&self, invitation_id: &str) -> Result<String, IntentError> {
        // Get the invitation service from the agent
        let invitation_service = self.agent.invitations().await.map_err(|e| {
            IntentError::service_error(format!("Invitation service unavailable: {}", e))
        })?;

        // Export the invitation code
        invitation_service
            .export_code(invitation_id)
            .await
            .map_err(|e| IntentError::internal_error(format!("Failed to export invitation: {}", e)))
    }

    // =========================================================================
    // Authentication
    // =========================================================================

    async fn is_authenticated(&self) -> bool {
        if let Ok(auth_service) = self.agent.auth().await {
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

/// Extract a millisecond timestamp from a TimeStamp enum.
///
/// Different TimeStamp variants are handled as follows:
/// - PhysicalClock: Uses the ts_ms field directly
/// - LogicalClock: Uses the lamport clock value
/// - OrderClock: Uses 0 (opaque ordering, no temporal meaning)
/// - Range: Uses the earliest bound if available
fn extract_timestamp_ms(ts: &TimeStamp) -> u64 {
    match ts {
        TimeStamp::PhysicalClock(physical) => physical.ts_ms,
        TimeStamp::LogicalClock(logical) => logical.lamport,
        TimeStamp::OrderClock(_) => 0, // OrderClock is opaque, no timestamp extraction
        TimeStamp::Range(range) => {
            // Use earliest bound from range
            range.earliest_ms
        }
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
