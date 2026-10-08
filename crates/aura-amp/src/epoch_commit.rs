//! Consensus-committed channel epoch, as a fact members sync (Task 164).
//!
//! A membership change moves a channel to a new epoch agreed by Aura
//! Consensus among the new epoch's members (A3). [`ChannelEpochCommitFact`]
//! carries that agreement to every member: the committed bump and the
//! consensus commit whose aggregate signature covers the proposed bump. It
//! is self-verifying against the epoch's group key, which every member of
//! the new epoch holds from its key ceremony, so any member may relay it;
//! a receiver admits it only when the commit verifies against the group key
//! it holds itself, never the key the commit carries.

use aura_consensus::CommitFact;
use aura_core::crypto::tree_signing::PublicKeyPackage;
use aura_core::hash::hash;
use aura_core::types::identifiers::{ChannelId, ContextId};
use aura_core::{AuraError, Hash32};
use aura_journal::fact::{CommittedChannelEpochBump, ProposedChannelEpochBump, RelationalFact};
use aura_journal::reduction::{RelationalBinding, RelationalBindingType};
use aura_journal::{DomainFact, FactReducer};
use aura_macros::DomainFact;
use serde::{Deserialize, Serialize};

/// Type identifier of [`ChannelEpochCommitFact`].
pub const CHANNEL_EPOCH_COMMIT_FACT_TYPE_ID: &str = "amp-channel-epoch-commit";

/// A channel epoch bump committed by consensus among the new epoch's members.
#[derive(Debug, Clone, Serialize, Deserialize, DomainFact)]
#[domain_fact(
    type_id = "amp-channel-epoch-commit",
    schema_version = 1,
    context = "context"
)]
pub struct ChannelEpochCommitFact {
    context: ContextId,
    channel: ChannelId,
    committed: CommittedChannelEpochBump,
    commit: CommitFact,
}

impl ChannelEpochCommitFact {
    /// Bind a consensus commit to the bump it agreed on: the commit's
    /// operation must be exactly `proposal`.
    pub fn new(
        proposal: &ProposedChannelEpochBump,
        commit: CommitFact,
        transcript_ref: Option<Hash32>,
    ) -> Result<Self, AuraError> {
        let agreed: ProposedChannelEpochBump = serde_json::from_slice(&commit.operation_bytes)
            .map_err(|error| AuraError::serialization(error.to_string()))?;
        if agreed != *proposal {
            return Err(AuraError::invalid(
                "consensus commit agreed on a different bump",
            ));
        }
        Ok(Self {
            context: proposal.context,
            channel: proposal.channel,
            committed: CommittedChannelEpochBump::from_proposal(
                proposal,
                commit.consensus_id.0,
                transcript_ref,
            ),
            commit,
        })
    }

    pub fn context(&self) -> ContextId {
        self.context
    }

    pub fn channel(&self) -> ChannelId {
        self.channel
    }

    pub fn committed(&self) -> &CommittedChannelEpochBump {
        &self.committed
    }

    /// Check the fact against the new epoch's group key the receiver holds:
    /// the commit carries that key, its signature verifies, it agreed on the
    /// bump this fact commits, and the bump is for this fact's channel.
    pub fn verify_with(&self, trusted_group: &PublicKeyPackage) -> Result<(), AuraError> {
        let carried = self.commit.group_public_key.as_ref().map(|key| {
            (
                &key.group_public_key,
                &key.signer_public_keys,
                key.threshold,
            )
        });
        let trusted = (
            &trusted_group.group_public_key,
            &trusted_group.signer_public_keys,
            trusted_group.threshold,
        );
        if carried != Some(trusted) {
            return Err(AuraError::permission_denied(
                "epoch commit is not signed by this epoch's group key",
            ));
        }
        self.commit.verify()?;
        let agreed: ProposedChannelEpochBump = serde_json::from_slice(&self.commit.operation_bytes)
            .map_err(|error| AuraError::serialization(error.to_string()))?;
        let expected = CommittedChannelEpochBump::from_proposal(
            &agreed,
            self.commit.consensus_id.0,
            self.committed.transcript_ref,
        );
        if expected != self.committed
            || agreed.context != self.context
            || agreed.channel != self.channel
        {
            return Err(AuraError::permission_denied(
                "epoch commit does not match the bump it signed",
            ));
        }
        Ok(())
    }

    /// The AMP context journal fact the epoch reduction reads: the committed
    /// bump (this fact itself keeps the consensus evidence).
    pub fn committed_bump_fact(&self) -> RelationalFact {
        RelationalFact::Protocol(
            aura_journal::ProtocolRelationalFact::AmpCommittedChannelEpochBump(
                self.committed.clone(),
            ),
        )
    }
}

/// Registry reducer for [`ChannelEpochCommitFact`]: a content-addressed
/// binding; the epoch itself is reduced from the relational facts.
#[derive(Debug, Clone, Copy, Default)]
pub struct ChannelEpochCommitFactReducer;

impl FactReducer for ChannelEpochCommitFactReducer {
    fn handles_type(&self) -> &'static str {
        CHANNEL_EPOCH_COMMIT_FACT_TYPE_ID
    }

    fn reduce_envelope(
        &self,
        context_id: ContextId,
        envelope: &aura_core::types::facts::FactEnvelope,
    ) -> Option<RelationalBinding> {
        let fact = ChannelEpochCommitFact::from_envelope(envelope)?;
        if fact.context != context_id {
            return None;
        }
        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic("amp-channel-epoch-commit".to_string()),
            context_id,
            data: hash(&envelope.payload).to_vec(),
        })
    }
}
