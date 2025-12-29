//! Threshold lifecycle traits (fast-path + finalization taxonomy).

use crate::{AuraError, AuthorityId, Hash32};
use async_trait::async_trait;

/// Base trait for lifecycle operations.
#[async_trait]
pub trait ThresholdLifecycle: Send + Sync {
    type KeyMaterial;
    type SignatureOutput;

    fn threshold(&self) -> u16;
    fn total_participants(&self) -> u16;
    fn epoch(&self) -> u64;

    async fn generate_keys(&self) -> Result<Self::KeyMaterial, AuraError>;
    async fn sign(&self, message: &[u8]) -> Result<Self::SignatureOutput, AuraError>;
    async fn verify(
        &self,
        message: &[u8],
        sig: &Self::SignatureOutput,
    ) -> Result<bool, AuraError>;
}

/// Provisional fast-path (A1).
#[async_trait]
pub trait ProvisionalLifecycle: ThresholdLifecycle {
    async fn mark_provisional(&self, op_id: Hash32, prestate: Hash32) -> Result<(), AuraError>;
}

/// Coordinator soft-safe fast-path (A2).
#[async_trait]
pub trait CoordinatorLifecycle: ThresholdLifecycle {
    type ConvergenceCert;
    type ReversionFact;

    fn coordinator_epoch(&self) -> u64;

    async fn issue_convergence_cert(
        &self,
        op_id: Hash32,
        prestate: Hash32,
    ) -> Result<Self::ConvergenceCert, AuraError>;

    async fn mark_reversion(
        &self,
        op_id: Hash32,
        winner: Hash32,
    ) -> Result<Self::ReversionFact, AuraError>;
}

/// Consensus-finalized lifecycle (A3).
#[async_trait]
pub trait ConsensusLifecycle: ThresholdLifecycle {
    type DkgTranscript;
    type Share;

    async fn run_bft_dkg(
        &self,
        witnesses: &[AuthorityId],
    ) -> Result<Self::DkgTranscript, AuraError>;

    async fn recover_share_from_transcript(
        &self,
        transcript: &Self::DkgTranscript,
    ) -> Result<Self::Share, AuraError>;
}

/// Rotation / escalation between states.
#[async_trait]
pub trait RotationLifecycle: ThresholdLifecycle {
    async fn rotate(
        &self,
        from_state: Hash32,
        to_state: Hash32,
        reason: String,
    ) -> Result<(), AuraError>;

    async fn abort(&self, op_id: Hash32, reason: String) -> Result<(), AuraError>;
}
