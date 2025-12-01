//! Consensus integration smoke test for AMP bumps.

use aura_core::epochs::Epoch;
use aura_core::frost::Share;
use aura_core::AuthorityId;
use aura_journal::fact::{ChannelBumpReason, ProposedChannelEpochBump};
use aura_protocol::consensus::run_amp_channel_epoch_bump;
use std::collections::HashMap;

#[tokio::test]
async fn amp_consensus_smoke() {
    // Minimal proposal and witness set.
    let ctx = aura_core::identifiers::ContextId::new_from_entropy([1u8; 32]);
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
    let witnesses = vec![AuthorityId::new_from_entropy([11u8; 32])];
    let key_packages: HashMap<AuthorityId, Share> = HashMap::new();

    // Create test FROST keys using testkit
    let (_, group_public_key) = aura_testkit::builders::keys::helpers::test_frost_key_shares(
        2,     // threshold
        3,     // total
        12345, // deterministic seed
    );

    // This should currently fail because key_packages are empty; ensures error path is exercised.
    let result = run_amp_channel_epoch_bump(
        &prestate,
        &proposal,
        witnesses,
        1,
        key_packages,
        group_public_key.into(),
        Epoch::from(1),
    )
    .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn amp_consensus_success_path() {
    let ctx = aura_core::identifiers::ContextId::new_from_entropy([2u8; 32]);
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
    let witnesses = vec![
        AuthorityId::new_from_entropy([21u8; 32]),
        AuthorityId::new_from_entropy([22u8; 32]),
    ];
    let mut key_packages: HashMap<AuthorityId, Share> = HashMap::new();

    // Create test FROST keys using testkit
    let (frost_key_packages, gp) = aura_testkit::builders::keys::helpers::test_frost_key_shares(
        2,     // threshold
        3,     // total
        54321, // different deterministic seed
    );

    // Insert the first two key packages for the witnesses
    for (witness, (_, key_pkg)) in witnesses.iter().zip(frost_key_packages.into_iter().take(2)) {
        key_packages.insert(*witness, key_pkg.into());
    }

    let result = run_amp_channel_epoch_bump(
        &prestate,
        &proposal,
        witnesses,
        2,
        key_packages,
        gp.into(),
        Epoch::from(1),
    )
    .await;
    assert!(
        result.is_ok(),
        "consensus should succeed with key material: {:?}",
        result.as_ref().err()
    );
}
