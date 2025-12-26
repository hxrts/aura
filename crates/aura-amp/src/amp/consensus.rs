//! AMP channel epoch bump consensus adapter
//!
//! This module provides a thin wrapper that binds AMP channel-epoch bump
//! proposals to Aura Consensus and produces the corresponding committed bump
//! fact for insertion into the relational journal.
//!
//! Uses the new consensus protocol for executing channel epoch bumps.

use aura_consensus::protocol::run_consensus;
use aura_consensus::types::CommitFact;
use super::{AmpEvidenceEffects, AmpJournalEffects};
use aura_core::effects::RandomEffects;
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::epochs::Epoch;
use aura_core::frost::{PublicKeyPackage, Share};
use aura_core::{AuthorityId, Prestate, Result};
use aura_journal::fact::{CommittedChannelEpochBump, ProposedChannelEpochBump};
use std::collections::HashMap;

/// Derive a witness set and threshold from a prestate.
///
/// Default policy: all authorities in the prestate, threshold = f+1 (majority)
fn default_witness_policy(prestate: &Prestate) -> (Vec<AuthorityId>, u16) {
    let witnesses: Vec<AuthorityId> = prestate
        .authority_commitments
        .iter()
        .map(|(id, _)| *id)
        .collect();

    let threshold = ((witnesses.len() as u16) / 2).saturating_add(1).max(1);
    (witnesses, threshold)
}

/// Run consensus for a channel epoch bump proposal and materialize the committed fact.
///
/// Returns the committed bump fact alongside the raw commit fact so callers can
/// insert both the AMP fact and consensus evidence into journals if desired.
pub async fn run_amp_channel_epoch_bump(
    prestate: &Prestate,
    proposal: &ProposedChannelEpochBump,
    witnesses: Vec<AuthorityId>,
    threshold: u16,
    key_packages: HashMap<AuthorityId, Share>,
    group_public_key: PublicKeyPackage,
    epoch: Epoch,
    random: &(impl RandomEffects + ?Sized),
    time: &(impl PhysicalTimeEffects + ?Sized),
) -> Result<(CommittedChannelEpochBump, CommitFact)> {
    // Consensus over the proposal itself; serialization is handled by `run_consensus`.
    let params = aura_consensus::protocol::ConsensusParams {
        witnesses,
        threshold,
        key_packages,
        group_public_key,
        epoch,
    };
    let commit = run_consensus(prestate, proposal, params, &random, &time).await?;

    let committed = CommittedChannelEpochBump {
        context: proposal.context,
        channel: proposal.channel,
        parent_epoch: proposal.parent_epoch,
        new_epoch: proposal.new_epoch,
        chosen_bump_id: proposal.bump_id,
        consensus_id: commit.consensus_id.0,
    };

    Ok((committed, commit))
}

/// Run consensus with default witness policy (all authorities, majority threshold).
pub async fn run_amp_channel_epoch_bump_default(
    prestate: &Prestate,
    proposal: &ProposedChannelEpochBump,
    key_packages: HashMap<AuthorityId, Share>,
    group_public_key: PublicKeyPackage,
    epoch: Epoch,
    random: &(impl RandomEffects + ?Sized),
    time: &(impl PhysicalTimeEffects + ?Sized),
) -> Result<(CommittedChannelEpochBump, CommitFact)> {
    let (witnesses, threshold) = default_witness_policy(prestate);
    run_amp_channel_epoch_bump(
        prestate,
        proposal,
        witnesses,
        threshold,
        key_packages,
        group_public_key,
        epoch,
        random,
        time,
    )
    .await
}

/// Run consensus for a bump and persist the committed fact into the journal.
///
/// Evidence plumbing: Inserts committed bump + consensus commit fact + evidence deltas.
/// Tracks message provenance per AMP specification requirements.
pub async fn finalize_amp_bump_with_journal<J: AmpJournalEffects + AmpEvidenceEffects>(
    journal: &J,
    prestate: &Prestate,
    proposal: &ProposedChannelEpochBump,
    witnesses: Vec<AuthorityId>,
    threshold: u16,
    key_packages: HashMap<AuthorityId, Share>,
    group_public_key: PublicKeyPackage,
    epoch: Epoch,
    random: &(impl RandomEffects + ?Sized),
    time: &(impl PhysicalTimeEffects + ?Sized),
) -> Result<CommittedChannelEpochBump> {
    let (committed, commit) = run_amp_channel_epoch_bump(
        prestate,
        proposal,
        witnesses.clone(),
        threshold,
        key_packages,
        group_public_key,
        epoch,
        random,
        time,
    )
    .await?;

    // Insert AMP committed bump fact
    journal
        .insert_relational_fact(
            aura_journal::fact::RelationalFact::AmpCommittedChannelEpochBump(committed.clone()),
        )
        .await?;

    // Insert consensus evidence for observability/audit
    journal
        .insert_relational_fact(commit.to_relational_fact())
        .await?;

    // Evidence deltas: Track message provenance for each witness participation
    // This creates an audit trail of which authorities contributed to consensus
    for witness in &witnesses {
        journal
            .insert_evidence_delta(*witness, commit.consensus_id, committed.context)
            .await?;
    }

    Ok(committed)
}

/// Run consensus with default witness policy and persist committed fact.
pub async fn finalize_amp_bump_with_journal_default<J: AmpJournalEffects + AmpEvidenceEffects>(
    journal: &J,
    prestate: &Prestate,
    proposal: &ProposedChannelEpochBump,
    key_packages: HashMap<AuthorityId, Share>,
    group_public_key: PublicKeyPackage,
    epoch: Epoch,
    random: &(impl RandomEffects + ?Sized),
    time: &(impl PhysicalTimeEffects + ?Sized),
) -> Result<CommittedChannelEpochBump> {
    let (witnesses, threshold) = default_witness_policy(prestate);
    finalize_amp_bump_with_journal(
        journal,
        prestate,
        proposal,
        witnesses,
        threshold,
        key_packages,
        group_public_key,
        epoch,
        random,
        time,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{frost::Share, AuthorityId, ContextId};
    use aura_testkit::stateful_effects::MockRandomHandler;
    use aura_testkit::time::ControllableTimeSource;
    use std::collections::HashMap;

    fn authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn context(seed: u8) -> ContextId {
        ContextId::new_from_entropy([seed; 32])
    }

    #[tokio::test]
    async fn amp_consensus_missing_keys_fails() {
        let prestate = Prestate::new(vec![], aura_core::Hash32::default());
        let proposal = ProposedChannelEpochBump {
            context: context(1),
            channel: aura_core::identifiers::ChannelId::from_bytes([1u8; 32]),
            parent_epoch: 0,
            new_epoch: 1,
            bump_id: aura_core::Hash32::new([2u8; 32]),
            reason: aura_journal::fact::ChannelBumpReason::Routine,
        };

        let witnesses = vec![authority(10), authority(11), authority(12)];
        let key_packages: HashMap<AuthorityId, Share> = HashMap::new();

        // Create test FROST keys using testkit (minimum valid parameters)
        let (_, group_public_key) = aura_testkit::builders::keys::helpers::test_frost_key_shares(
            2,     // threshold
            3,     // total
            12345, // deterministic seed
        );

        let random = MockRandomHandler::new_with_seed(99);
        let time = ControllableTimeSource::new(1_700_000_000_000);

        let result = run_amp_channel_epoch_bump(
            &prestate,
            &proposal,
            witnesses,
            2,
            key_packages,
            group_public_key.into(),
            Epoch::from(1),
            &random,
            &time,
        )
        .await;

        assert!(result.is_err(), "missing key packages should error");
    }
}
