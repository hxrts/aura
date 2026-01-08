//! OTA Activation Service - Public API for OTA activation ceremonies
//!
//! Provides a runtime-facing API for OTA hard-fork activation ceremonies,
//! wiring the shared ceremony runner to the OTA ceremony executor.

use crate::core::{AgentError, AgentResult, AuthorityContext};
use crate::runtime::services::ceremony_runner::{
    CeremonyCommitMetadata, CeremonyInitRequest, CeremonyRunner,
};
use crate::runtime::AuraEffectSystem;
use aura_core::effects::{JournalEffects, PhysicalTimeEffects};
use aura_core::identifiers::CeremonyId;
use aura_core::threshold::ParticipantIdentity;
use aura_core::types::Epoch;
use aura_core::{DeviceId, Hash32};
use aura_sync::protocols::ota_ceremony::{
    OTACeremonyConfig, OTACeremonyExecutor, OTACeremonyId, ReadinessCommitment, UpgradeProposal,
};
use parking_lot::RwLock;
use std::sync::Arc;

/// OTA activation ceremony service API.
#[derive(Clone)]
pub struct OtaActivationServiceApi {
    effects: Arc<AuraEffectSystem>,
    ceremony_runner: CeremonyRunner,
    authority_context: AuthorityContext,
    executor: Arc<RwLock<OTACeremonyExecutor<Arc<AuraEffectSystem>>>>,
    config: OTACeremonyConfig,
}

impl OtaActivationServiceApi {
    /// Create a new OTA activation service with default configuration.
    pub fn new(
        effects: Arc<AuraEffectSystem>,
        authority_context: AuthorityContext,
    ) -> AgentResult<Self> {
        let ceremony_runner =
            CeremonyRunner::new(crate::runtime::services::CeremonyTracker::new());
        Self::new_with_runner_and_config(
            effects,
            authority_context,
            ceremony_runner,
            OTACeremonyConfig::default(),
        )
    }

    /// Create a new OTA activation service with a shared ceremony runner.
    pub fn new_with_runner(
        effects: Arc<AuraEffectSystem>,
        authority_context: AuthorityContext,
        ceremony_runner: CeremonyRunner,
    ) -> AgentResult<Self> {
        Self::new_with_runner_and_config(
            effects,
            authority_context,
            ceremony_runner,
            OTACeremonyConfig::default(),
        )
    }

    /// Create a new OTA activation service with a shared runner and explicit config.
    pub fn new_with_runner_and_config(
        effects: Arc<AuraEffectSystem>,
        authority_context: AuthorityContext,
        ceremony_runner: CeremonyRunner,
        config: OTACeremonyConfig,
    ) -> AgentResult<Self> {
        let executor = OTACeremonyExecutor::new(effects.clone(), config.clone());
        Ok(Self {
            effects,
            ceremony_runner,
            authority_context,
            executor: Arc::new(RwLock::new(executor)),
            config,
        })
    }

    fn runner_ceremony_id(ceremony_id: OTACeremonyId) -> CeremonyId {
        CeremonyId::new(hex::encode(ceremony_id.0.as_bytes()))
    }

    async fn compute_prestate_hash(&self) -> AgentResult<Hash32> {
        let journal = self
            .effects
            .get_journal()
            .await
            .map_err(|e| AgentError::effects(format!("Failed to load journal: {e}")))?;
        let journal_bytes = serde_json::to_vec(&journal.facts)
            .map_err(|e| AgentError::internal(format!("Failed to serialize journal: {e}")))?;
        Ok(Hash32::from_bytes(&journal_bytes))
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

        let prestate_hash = self.compute_prestate_hash().await?;
        let ceremony_id = {
            let mut executor = self.executor.write();
            executor
                .initiate_ceremony_with_prestate(proposal.clone(), current_epoch, prestate_hash)
                .await
                .map_err(|e| AgentError::runtime(format!("Failed to initiate OTA ceremony: {e}")))?
        };

        let runner_id = Self::runner_ceremony_id(ceremony_id);
        let participants = participants
            .into_iter()
            .map(ParticipantIdentity::device)
            .collect::<Vec<_>>();

        if let Err(err) = self
            .ceremony_runner
            .start(CeremonyInitRequest {
                ceremony_id: runner_id.clone(),
                kind: aura_app::runtime_bridge::CeremonyKind::OtaActivation,
                initiator_id: self.authority_context.authority_id(),
                threshold_k,
                total_n,
                participants,
                new_epoch: proposal.activation_epoch.value(),
                enrollment_device_id: None,
                enrollment_nickname_suggestion: None,
                prestate_hash: Some(prestate_hash),
            })
            .await
        {
            let _ = self.abort_activation(ceremony_id, "Failed to register ceremony").await;
            return Err(AgentError::internal(format!(
                "Failed to register OTA ceremony: {err}"
            )));
        }

        Ok(ceremony_id)
    }

    /// Record a device readiness commitment and mirror acceptances in the ceremony runner.
    pub async fn record_commitment(
        &self,
        ceremony_id: OTACeremonyId,
        commitment: ReadinessCommitment,
    ) -> AgentResult<bool> {
        let threshold_reached = {
            let mut executor = self.executor.write();
            executor
                .process_commitment(ceremony_id, commitment.clone())
                .await
                .map_err(|e| {
                    AgentError::runtime(format!("Failed to process OTA commitment: {e}"))
                })?
        };

        if commitment.ready {
            let runner_id = Self::runner_ceremony_id(ceremony_id);
            self.ceremony_runner
                .record_response(&runner_id, ParticipantIdentity::device(commitment.device))
                .await
                .map_err(|e| {
                    AgentError::internal(format!("Failed to record OTA response: {e}"))
                })?;
        }

        Ok(threshold_reached)
    }

    /// Commit the OTA activation ceremony and update the shared runner status.
    pub async fn commit_activation(&self, ceremony_id: OTACeremonyId) -> AgentResult<Epoch> {
        let activation_epoch = {
            let mut executor = self.executor.write();
            executor.commit_ceremony(ceremony_id).await.map_err(|e| {
                AgentError::runtime(format!("Failed to commit OTA ceremony: {e}"))
            })?
        };

        let committed_at = self
            .effects
            .physical_time()
            .await
            .map_err(|e| AgentError::effects(format!("Failed to read time: {e}")))?;

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

        Ok(activation_epoch)
    }

    /// Abort the OTA activation ceremony and update the shared runner status.
    pub async fn abort_activation(
        &self,
        ceremony_id: OTACeremonyId,
        reason: &str,
    ) -> AgentResult<()> {
        {
            let mut executor = self.executor.write();
            executor
                .abort_ceremony(ceremony_id, reason)
                .await
                .map_err(|e| AgentError::runtime(format!("Failed to abort OTA ceremony: {e}")))?;
        }

        let runner_id = Self::runner_ceremony_id(ceremony_id);
        self.ceremony_runner
            .abort(&runner_id, Some(reason.to_string()))
            .await
            .map_err(|e| AgentError::internal(format!("Failed to record OTA abort: {e}")))?;

        Ok(())
    }
}
