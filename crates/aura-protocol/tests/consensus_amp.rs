//! Consensus integration smoke test for AMP bumps.

use aura_core::AuthorityId;
use aura_journal::fact::{ChannelBumpReason, ProposedChannelEpochBump};
use aura_protocol::consensus::run_amp_channel_epoch_bump;
use frost_ed25519::keys::{KeyPackage, PublicKeyPackage};
use std::collections::HashMap;

#[tokio::test]
async fn amp_consensus_smoke() {
    // Minimal proposal and witness set.
    let ctx = aura_core::identifiers::ContextId::new();
    let channel = aura_core::identifiers::ChannelId::from_bytes([1u8; 32]);
    let proposal = ProposedChannelEpochBump {
        context: ctx,
        channel,
        parent_epoch: 0,
        new_epoch: 1,
        bump_id: aura_core::Hash32::new([9u8; 32]),
        reason: ChannelBumpReason::Routine,
    };

    let prestate = aura_core::Prestate::new(vec![], aura_core::Hash32::default());
    let witnesses = vec![AuthorityId::new()];
    let key_packages: HashMap<AuthorityId, KeyPackage> = HashMap::new();

    // Create test FROST keys using testkit
    let (_, group_public_key) = aura_testkit::builders::keys::helpers::test_frost_key_shares(
        1,     // threshold
        1,     // total
        12345, // deterministic seed
    );

    // This should currently fail because key_packages are empty; ensures error path is exercised.
    let result = run_amp_channel_epoch_bump(
        &prestate,
        &proposal,
        witnesses,
        1,
        key_packages,
        group_public_key,
    )
    .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn amp_consensus_success_path() {
    let ctx = aura_core::identifiers::ContextId::new();
    let channel = aura_core::identifiers::ChannelId::from_bytes([2u8; 32]);
    let proposal = ProposedChannelEpochBump {
        context: ctx,
        channel,
        parent_epoch: 0,
        new_epoch: 1,
        bump_id: aura_core::Hash32::new([3u8; 32]),
        reason: ChannelBumpReason::Routine,
    };

    let prestate = aura_core::Prestate::new(vec![], aura_core::Hash32::default());
    let witnesses = vec![AuthorityId::new()];
    let mut key_packages: HashMap<AuthorityId, KeyPackage> = HashMap::new();

    // Create test FROST keys using testkit
    let (frost_key_packages, gp) = aura_testkit::builders::keys::helpers::test_frost_key_shares(
        1,     // threshold
        1,     // total
        54321, // different deterministic seed
    );

    // Insert the first key package for the witness
    if let Some((_, key_pkg)) = frost_key_packages.into_iter().next() {
        key_packages.insert(witnesses[0], key_pkg);
    }

    let result =
        run_amp_channel_epoch_bump(&prestate, &proposal, witnesses, 1, key_packages, gp).await;
    assert!(result.is_ok(), "consensus should succeed with key material");
}
