//! Core consensus protocol logic.

use super::instance::ProtocolInstance;
use crate::{
    dkg::{self, DealerPackage, DkgConfig, DkgTranscriptStore},
    frost::FrostConsensusOrchestrator,
    messages::{ConsensusRequest, ConsensusResponse},
    types::{ConsensusConfig, ConsensusId},
};
use async_lock::RwLock;
use aura_core::{
    effects::{PhysicalTimeEffects, RandomEffects},
    epochs::Epoch,
    frost::{PublicKeyPackage, Share},
    AuraError, AuthorityId, ContextId, Prestate, Result,
};
use std::collections::HashMap;
use std::sync::Arc;

/// Protocol coordinator that manages consensus execution
pub struct ConsensusProtocol {
    /// Our authority ID
    pub(crate) authority_id: AuthorityId,

    /// Consensus configuration
    pub(crate) config: ConsensusConfig,

    /// FROST orchestrator for crypto operations
    pub(crate) frost_orchestrator: FrostConsensusOrchestrator,

    /// Group public key package for verification/aggregation
    pub(crate) group_public_key: PublicKeyPackage,

    /// Active protocol instances
    pub(crate) instances: Arc<RwLock<HashMap<ConsensusId, ProtocolInstance>>>,
}

impl ConsensusProtocol {
    /// Evict stale protocol instances that have exceeded the configured timeout.
    pub async fn cleanup_stale_instances(&self, now_ms: u64) -> usize {
        let timeout_ms = self.config.timeout_ms.get();
        let mut removed = 0usize;
        let mut instances = self.instances.write().await;
        instances.retain(|_, instance| {
            let stale = now_ms.saturating_sub(instance.start_time_ms) > timeout_ms;
            if stale {
                removed += 1;
            }
            !stale
        });
        removed
    }

    /// Create a new consensus protocol instance
    pub fn new(
        authority_id: AuthorityId,
        config: ConsensusConfig,
        key_packages: HashMap<AuthorityId, Share>,
        group_public_key: PublicKeyPackage,
    ) -> Result<Self> {
        let frost_orchestrator = FrostConsensusOrchestrator::new(
            config.clone(),
            key_packages,
            group_public_key.clone(),
        )?;

        Ok(Self {
            authority_id,
            config,
            frost_orchestrator,
            group_public_key,
            instances: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Run consensus as coordinator
    pub async fn run_consensus<T: serde::Serialize>(
        &self,
        prestate: &Prestate,
        operation: &T,
        random: &(impl RandomEffects + ?Sized),
        time: &(impl PhysicalTimeEffects + ?Sized),
    ) -> Result<ConsensusResponse> {
        // Best-effort cleanup of stale instances before starting a new run.
        if let Ok(now) = time.physical_time().await {
            let _ = self.cleanup_stale_instances(now.ts_ms).await;
        }
        // Serialize operation
        let operation_bytes =
            serde_json::to_vec(operation).map_err(|e| AuraError::serialization(e.to_string()))?;

        // Compute hashes
        let prestate_hash = prestate.compute_hash();
        let operation_hash = crate::hash_operation(&operation_bytes)?;

        let request = ConsensusRequest {
            prestate_hash,
            operation_bytes,
            operation_hash,
            timeout_ms: Some(self.config.timeout_ms),
        };

        // Use FROST orchestrator for the actual consensus
        self.frost_orchestrator
            .run_consensus(request, random, time)
            .await
    }

    /// Finalize a DKG transcript and persist its commit reference.
    pub async fn finalize_dkg_transcript<S: DkgTranscriptStore + ?Sized>(
        &self,
        context: ContextId,
        config: &DkgConfig,
        packages: Vec<DealerPackage>,
        store: &S,
    ) -> Result<aura_journal::fact::DkgTranscriptCommit> {
        let transcript = dkg::ceremony::run_dkg_ceremony(config, packages)?;
        dkg::ceremony::persist_transcript(store, context, &transcript).await
    }

    /// Handle epoch change
    pub async fn handle_epoch_change(&self, new_epoch: Epoch) {
        self.frost_orchestrator.handle_epoch_change(new_epoch).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_creation() {
        let witnesses = vec![
            AuthorityId::new_from_entropy([1u8; 32]),
            AuthorityId::new_from_entropy([2u8; 32]),
        ];
        let config = ConsensusConfig::new(2, witnesses, Epoch::from(1)).unwrap();
        let authority_id = AuthorityId::new_from_entropy([3u8; 32]);

        let protocol = ConsensusProtocol::new(
            authority_id,
            config,
            HashMap::new(),
            PublicKeyPackage::new(vec![0u8; 32], std::collections::BTreeMap::new(), 1, 1),
        )
        .unwrap();

        // Protocol should be created successfully
        let _ = protocol;
    }
}
