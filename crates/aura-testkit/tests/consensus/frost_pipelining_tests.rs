//! FROST Pipelining Tests
//!
//! Tests for FROST threshold signature pipelining optimization.
//! Uses trusted dealer model for key generation.
//!
//! These tests verify:
//! - Fast path consensus (1 RTT) with cached commitments
//! - Epoch rotation invalidation behavior
//! - Duplicate commitment rejection
//! - WitnessState nonce lifecycle

use aura_consensus::witness::{WitnessSet, WitnessState, WitnessTracker};
use aura_core::{
    crypto::tree_signing::FrostNonces, frost::NonceCommitment, types::Epoch, AuthorityId,
};
use aura_testkit::builders::keys::helpers::test_frost_key_shares;
use rand::SeedableRng;

/// Helper to create test authority IDs
fn authority(seed: u8) -> AuthorityId {
    AuthorityId::new_from_entropy([seed; 32])
}

/// Deterministic single-use nonces for signer `seed` over a fixed share.
fn test_nonces(seed: u8) -> FrostNonces {
    let signing_share = frost_ed25519::keys::SigningShare::deserialize([seed; 32])
        .unwrap_or_else(|err| panic!("signing share deserialization failed: {err:?}"));
    let mut rng = rand_chacha::ChaCha20Rng::from_seed([seed; 32]);
    FrostNonces::generate_for_share(
        frost_ed25519::Identifier::try_from(u16::from(seed))
            .unwrap_or_else(|err| panic!("identifier: {err:?}")),
        &signing_share,
        &mut rng,
    )
    .unwrap_or_else(|err| panic!("nonce generation failed: {err:?}"))
}

/// Test fast path (1 RTT) consensus with cached commitments
///
/// Verifies that when witnesses have pre-cached nonces from previous rounds,
/// consensus can complete in a single round-trip.
#[tokio::test]
async fn test_steady_state_single_rtt() {
    // Generate 2-of-3 FROST keys using trusted dealer
    let (key_packages, _pubkey_package) = test_frost_key_shares(2, 3, 12345);

    // Create witness set
    let witnesses = vec![authority(1), authority(2), authority(3)];
    let witness_set = WitnessSet::new(2, witnesses.clone()).unwrap();
    let epoch = Epoch::from(1);

    // Initially no cached commitments - should not have fast path quorum
    assert!(
        !witness_set.has_fast_path_quorum(epoch).await,
        "Should not have fast path quorum initially"
    );

    // Simulate caching nonces for each witness (as would happen after first consensus round)
    for (i, (_frost_id, key_package)) in key_packages.iter().enumerate() {
        let witness_id = witnesses[i];

        // Generate nonces using the key package
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([i as u8 + 10; 32]);
        let nonces = FrostNonces::generate(key_package, &mut rng)
            .unwrap_or_else(|err| panic!("nonce generation failed: {err:?}"));

        witness_set
            .update_witness_nonce(witness_id, nonces, epoch)
            .await
            .unwrap_or_else(|err| panic!("update nonce failed: {err:?}"));
    }

    // Now should have fast path quorum
    assert!(
        witness_set.has_fast_path_quorum(epoch).await,
        "Should have fast path quorum after caching nonces"
    );

    // Collect cached commitments
    let cached = witness_set.collect_cached_commitments(epoch).await;
    assert_eq!(cached.len(), 3, "Should have 3 cached commitments");

    // Verify we can use the cached commitments (threshold = 2, we have 3)
    assert!(
        cached.len() >= 2,
        "Should have enough commitments for threshold"
    );

    // Cached nonces move out of the shared witness state exactly once.
    assert!(witness_set
        .take_witness_nonce(witnesses[0], epoch)
        .await
        .is_some());
    assert!(
        witness_set
            .take_witness_nonce(witnesses[0], epoch)
            .await
            .is_none(),
        "a cached nonce must not be handed out for a second round"
    );
    assert_eq!(witness_set.collect_cached_commitments(epoch).await.len(), 2);
}

/// Test that epoch rotation invalidates cached nonces
///
/// Verifies that when the epoch changes, all cached nonces are invalidated
/// to prevent cross-epoch reuse.
#[tokio::test]
async fn test_epoch_rotation_invalidation() {
    let witnesses = vec![authority(10), authority(11), authority(12)];
    let witness_set = WitnessSet::new(2, witnesses.clone()).unwrap();
    let epoch1 = Epoch::from(1);
    let epoch2 = Epoch::from(2);

    // Cache nonces in epoch 1 using deterministic test key shares
    let (key_packages, _pubkey_package) = test_frost_key_shares(2, 3, 4242);
    let key_package = key_packages
        .values()
        .next()
        .unwrap_or_else(|| panic!("test key share"));
    let mut rng = rand_chacha::ChaCha20Rng::from_seed([7u8; 32]);
    let nonces = FrostNonces::generate(key_package, &mut rng)
        .unwrap_or_else(|err| panic!("nonce generation failed: {err:?}"));

    witness_set
        .update_witness_nonce(witnesses[0], nonces, epoch1)
        .await
        .unwrap_or_else(|err| panic!("update nonce failed: {err:?}"));

    // Verify nonce is cached in epoch 1
    let cached_epoch1 = witness_set.collect_cached_commitments(epoch1).await;
    assert_eq!(
        cached_epoch1.len(),
        1,
        "Should have cached commitment in epoch 1"
    );

    // Epoch change - cached commitments should not be available in new epoch
    let cached_epoch2 = witness_set.collect_cached_commitments(epoch2).await;
    assert!(
        cached_epoch2.is_empty(),
        "Cached commitments should not be available in new epoch"
    );

    // Explicit invalidation
    witness_set.invalidate_all_caches().await;

    // Even epoch 1 should now show empty
    let cached_after_invalidate = witness_set.collect_cached_commitments(epoch1).await;
    assert!(
        cached_after_invalidate.is_empty(),
        "All caches should be cleared after invalidation"
    );
}

/// Test that duplicate commitments are handled correctly
///
/// Verifies that the tracker properly deduplicates commitments from the same witness.
#[tokio::test]
async fn test_adversarial_duplicate_commitments() {
    let mut tracker = WitnessTracker::new();
    let witness = authority(1);

    let commitment1 = NonceCommitment {
        signer: 1,
        commitment: vec![1u8; 32],
    };

    let commitment2 = NonceCommitment {
        signer: 1,
        commitment: vec![2u8; 32],
    };

    // Add first commitment
    tracker.add_nonce(witness, commitment1);
    assert_eq!(tracker.nonce_commitments.len(), 1);

    // Add second commitment from same witness (should replace, not add)
    tracker.add_nonce(witness, commitment2.clone());
    assert_eq!(
        tracker.nonce_commitments.len(),
        1,
        "Duplicate from same witness should replace, not add"
    );

    // Verify the second commitment replaced the first
    let stored = tracker.nonce_commitments.get(&witness).unwrap();
    assert_eq!(
        stored.commitment, commitment2.commitment,
        "Later commitment should replace earlier one"
    );
}

/// Test WitnessState nonce lifecycle
///
/// Verifies the lifecycle of nonces through caching, usage, and invalidation.
#[tokio::test]
async fn test_witness_state_lifecycle() {
    let witness_id = authority(5);
    let epoch1 = Epoch::from(1);
    let epoch2 = Epoch::from(2);

    let mut state = WitnessState::new(witness_id, epoch1);

    // Initially no cached nonce
    assert!(
        !state.has_cached_nonce(epoch1),
        "Should not have cached nonce initially"
    );
    assert!(
        state.get_next_commitment(epoch1).is_none(),
        "Should not have commitment initially"
    );

    // Cache a nonce
    state.set_next_nonce(test_nonces(5), epoch1);

    // Now should have cached nonce
    assert!(
        state.has_cached_nonce(epoch1),
        "Should have cached nonce after set"
    );
    assert!(
        state.get_next_commitment(epoch1).is_some(),
        "Should have commitment after set"
    );

    // Epoch mismatch - should not return cached nonce
    assert!(
        !state.has_cached_nonce(epoch2),
        "Should not return nonce for wrong epoch"
    );
    assert!(
        state.get_next_commitment(epoch2).is_none(),
        "Should not return commitment for wrong epoch"
    );

    // Take the nonce (consuming it)
    let taken = state.take_nonce(epoch1);
    assert!(taken.is_some(), "Should be able to take cached nonce");

    // After taking, should be empty
    assert!(
        !state.has_cached_nonce(epoch1),
        "Should not have nonce after taking"
    );
    assert!(
        state.take_nonce(epoch1).is_none(),
        "Second take should return None"
    );

    // Invalidation
    state.set_next_nonce(test_nonces(6), epoch1);
    assert!(state.has_cached_nonce(epoch1));

    state.invalidate();
    assert!(
        !state.has_cached_nonce(epoch1),
        "Should not have nonce after invalidation"
    );
}

/// Test WitnessTracker threshold checking
#[tokio::test]
async fn test_witness_tracker_threshold() {
    use aura_core::Hash32;

    let mut tracker = WitnessTracker::with_threshold(2);
    let threshold = 2u16;
    let result_id = Hash32::new([0u8; 32]);

    // Initially below threshold
    assert!(!tracker.has_nonce_threshold(threshold));
    assert!(!tracker.has_signature_threshold(threshold));

    // Add first witness
    tracker.add_nonce(
        authority(1),
        NonceCommitment {
            signer: 1,
            commitment: vec![1u8; 32],
        },
    );
    assert!(!tracker.has_nonce_threshold(threshold));

    // Add second witness - now at threshold
    tracker.add_nonce(
        authority(2),
        NonceCommitment {
            signer: 2,
            commitment: vec![2u8; 32],
        },
    );
    assert!(tracker.has_nonce_threshold(threshold));

    // Add signatures
    let _ = tracker.add_signature(
        authority(1),
        aura_core::frost::PartialSignature {
            signer: 1,
            signature: vec![1u8; 64],
        },
        result_id,
    );
    assert!(!tracker.has_signature_threshold(threshold));

    let _ = tracker.add_signature(
        authority(2),
        aura_core::frost::PartialSignature {
            signer: 2,
            signature: vec![2u8; 64],
        },
        result_id,
    );
    assert!(tracker.has_signature_threshold(threshold));

    // Verify participants
    let participants = tracker.get_participants();
    assert_eq!(participants.len(), 2);
}

/// Test WitnessSet with invalid configurations
#[tokio::test]
async fn test_witness_set_validation() {
    // Empty witness set should fail
    let result = WitnessSet::new(1, vec![]);
    assert!(result.is_err(), "Empty witness set should fail");

    // Zero threshold should fail
    let result = WitnessSet::new(0, vec![authority(1)]);
    assert!(result.is_err(), "Zero threshold should fail");

    // Threshold > witnesses should fail
    let result = WitnessSet::new(3, vec![authority(1), authority(2)]);
    assert!(
        result.is_err(),
        "Threshold exceeding witness count should fail"
    );

    // Valid configuration should succeed
    let result = WitnessSet::new(2, vec![authority(1), authority(2), authority(3)]);
    assert!(result.is_ok(), "Valid configuration should succeed");
}
