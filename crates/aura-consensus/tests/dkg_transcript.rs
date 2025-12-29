use aura_consensus::dkg::transcript::{compute_transcript_hash, finalize_transcript};
use aura_consensus::dkg::types::DealerPackage;
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
    let packages = vec![test_package(dealer, 7), test_package(dealer, 9)];
    let hash1 = compute_transcript_hash(&packages).unwrap();
    let hash2 = compute_transcript_hash(&packages).unwrap();
    assert_eq!(hash1, hash2);
}

#[test]
fn test_finalize_transcript_populates_hash() {
    let dealer = AuthorityId::new_from_entropy([2u8; 32]);
    let packages = vec![test_package(dealer, 3)];
    let transcript = finalize_transcript(1, Hash32([0u8; 32]), 42, packages).unwrap();
    assert_eq!(transcript.epoch, 1);
    assert_eq!(transcript.cutoff, 42);
    assert_eq!(transcript.packages.len(), 1);
    let expected_hash = compute_transcript_hash(&transcript.packages).unwrap();
    assert_eq!(transcript.transcript_hash, expected_hash);
}
