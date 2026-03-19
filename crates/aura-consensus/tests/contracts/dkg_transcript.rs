//! DKG transcript integration tests.

use aura_consensus::dkg::transcript::{compute_transcript_hash, finalize_transcript};
use aura_consensus::dkg::types::{DealerPackage, DkgConfig};
use aura_core::{AuthorityId, Hash32};
use std::collections::BTreeMap;

fn test_package(dealer: AuthorityId, tag: u8) -> DealerPackage {
    let mut shares = BTreeMap::new();
    shares.insert(AuthorityId::new_from_entropy([tag; 32]), vec![tag; 8]);
    DealerPackage {
        dealer,
        commitment: vec![tag; 4],
        encrypted_shares: shares,
        proof: vec![tag; 2],
    }
}

#[test]
fn test_transcript_hash_deterministic() {
    let dealer = AuthorityId::new_from_entropy([1u8; 32]);
    let participants = vec![
        AuthorityId::new_from_entropy([2u8; 32]),
        AuthorityId::new_from_entropy([3u8; 32]),
    ];
    let config = DkgConfig {
        epoch: 1,
        threshold: 1,
        max_signers: 3,
        membership_hash: Hash32([0u8; 32]),
        cutoff: 10,
        prestate_hash: Hash32([4u8; 32]),
        operation_hash: Hash32([5u8; 32]),
        participants,
    };
    let packages = vec![test_package(dealer, 7), test_package(dealer, 9)];
    let hash1 = compute_transcript_hash(&config, &packages).unwrap();
    let hash2 = compute_transcript_hash(&config, &packages).unwrap();
    assert_eq!(hash1, hash2);
}

#[test]
fn test_finalize_transcript_populates_hash() {
    let dealer = AuthorityId::new_from_entropy([2u8; 32]);
    let participants = vec![
        AuthorityId::new_from_entropy([4u8; 32]),
        AuthorityId::new_from_entropy([5u8; 32]),
    ];
    let config = DkgConfig {
        epoch: 1,
        threshold: 1,
        max_signers: 3,
        membership_hash: Hash32([0u8; 32]),
        cutoff: 42,
        prestate_hash: Hash32([6u8; 32]),
        operation_hash: Hash32([7u8; 32]),
        participants,
    };
    let packages = vec![test_package(dealer, 3)];
    let transcript = finalize_transcript(&config, packages).unwrap();
    assert_eq!(transcript.epoch, config.epoch);
    assert_eq!(transcript.cutoff, config.cutoff);
    assert_eq!(transcript.packages.len(), 1);
    let expected_hash = compute_transcript_hash(&config, &transcript.packages).unwrap();
    assert_eq!(transcript.transcript_hash, expected_hash);
}
