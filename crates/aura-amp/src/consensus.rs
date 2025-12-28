//! AMP channel epoch bump consensus adapter
//!
//! This module provides a thin wrapper that binds AMP channel-epoch bump
//! proposals to Aura Consensus and produces the corresponding committed bump
//! fact for insertion into the relational journal.
//!
//! Uses the new consensus protocol for executing channel epoch bumps.

use crate::{AmpEvidenceEffects, AmpJournalEffects};
use aura_consensus::protocol::run_consensus;
use aura_consensus::types::CommitFact;
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::effects::RandomEffects;
use aura_core::types::Epoch;
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
    let commit = run_consensus(prestate, proposal, params, random, time).await?;

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
            aura_journal::fact::RelationalFact::Protocol(
                aura_journal::ProtocolRelationalFact::AmpCommittedChannelEpochBump(
                    committed.clone(),
                ),
            ),
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
