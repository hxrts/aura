//! Helper functions for relay selection
//!
//! Provides utility functions for deterministic relay selection including
//! seed hashing, tier partitioning, and candidate selection.

use aura_core::{
    effects::relay::{RelayCandidate, RelayContext, RelayRelationship},
    hash::hasher,
    identifiers::AuthorityId,
};

/// Domain separator for relay selection hashing.
const RELAY_DOMAIN: &[u8] = b"AURA_RELAY_SELECT_v1";

/// Hash inputs to produce a deterministic seed for relay selection.
///
/// Uses BLAKE3 to combine domain separator, context_id, epoch, and nonce
/// into a 32-byte seed that can be used for deterministic random selection.
///
/// # Arguments
/// * `context` - The relay context containing selection parameters
///
/// # Returns
/// A 32-byte seed suitable for deterministic selection
pub fn hash_relay_seed(context: &RelayContext) -> [u8; 32] {
    let mut hasher = hasher();
    hasher.update(RELAY_DOMAIN);
    hasher.update(context.context_id.as_bytes());
    hasher.update(&context.epoch.to_le_bytes());
    hasher.update(&context.nonce);
    hasher.update(&context.source.to_bytes());
    hasher.update(&context.destination.to_bytes());

    let digest = hasher.finalize();
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&digest);
    seed
}

/// Partition candidates by their relationship type.
///
/// Returns three vectors: block peers, neighborhood peers, and guardians.
/// Only reachable candidates are included in the result.
///
/// # Arguments
/// * `candidates` - All relay candidates to partition
///
/// # Returns
/// Tuple of (block_peers, neighborhood_peers, guardians)
pub fn partition_by_relationship(
    candidates: &[RelayCandidate],
) -> (
    Vec<&RelayCandidate>,
    Vec<&RelayCandidate>,
    Vec<&RelayCandidate>,
) {
    let mut block_peers = Vec::new();
    let mut neighborhood_peers = Vec::new();
    let mut guardians = Vec::new();

    for candidate in candidates {
        if !candidate.reachable {
            continue;
        }

        match &candidate.relationship {
            RelayRelationship::BlockPeer { .. } => block_peers.push(candidate),
            RelayRelationship::NeighborhoodPeer { .. } => neighborhood_peers.push(candidate),
            RelayRelationship::Guardian => guardians.push(candidate),
        }
    }

    (block_peers, neighborhood_peers, guardians)
}

/// Select one candidate from a tier using deterministic randomness.
///
/// Uses the seed to select a candidate from the given tier. Selection is
/// deterministic: the same seed and tier always produce the same result.
///
/// # Arguments
/// * `tier` - Candidates in this tier (should all have same relationship type)
/// * `seed` - 32-byte seed for deterministic selection
/// * `tier_index` - Which tier this is (0=block, 1=neighborhood, 2=guardian)
///
/// # Returns
/// The selected candidate's authority ID, or None if tier is empty
pub fn select_one_from_tier(
    tier: &[&RelayCandidate],
    seed: &[u8; 32],
    tier_index: u8,
) -> Option<AuthorityId> {
    if tier.is_empty() {
        return None;
    }

    // Derive a tier-specific seed to avoid correlation between tiers
    let mut hasher = hasher();
    hasher.update(seed);
    hasher.update(&[tier_index]);
    let tier_seed = hasher.finalize();

    // Use first 8 bytes as index source
    // Hash output is always 32 bytes, so we can safely extract first 8 bytes
    let index_bytes: [u8; 8] = [
        tier_seed[0],
        tier_seed[1],
        tier_seed[2],
        tier_seed[3],
        tier_seed[4],
        tier_seed[5],
        tier_seed[6],
        tier_seed[7],
    ];
    let index = u64::from_le_bytes(index_bytes) as usize % tier.len();

    Some(tier[index].authority_id)
}

/// Select relays from candidates using tier-based priority.
///
/// Selects up to `max_per_tier` candidates from each tier, starting with
/// block peers (highest priority), then neighborhood peers, then guardians.
///
/// # Arguments
/// * `candidates` - All relay candidates
/// * `seed` - 32-byte seed for deterministic selection
/// * `max_per_tier` - Maximum candidates to select from each tier
///
/// # Returns
/// Ordered list of authority IDs to use as relays (first is primary)
pub fn select_by_tiers(
    candidates: &[RelayCandidate],
    seed: &[u8; 32],
    max_per_tier: usize,
) -> Vec<AuthorityId> {
    let (block_peers, neighborhood_peers, guardians) = partition_by_relationship(candidates);

    let mut result = Vec::new();

    // Select from block peers first
    result.extend(select_multiple_from_tier(
        &block_peers,
        seed,
        0,
        max_per_tier,
    ));

    // Then neighborhood peers
    result.extend(select_multiple_from_tier(
        &neighborhood_peers,
        seed,
        1,
        max_per_tier,
    ));

    // Finally guardians
    result.extend(select_multiple_from_tier(&guardians, seed, 2, max_per_tier));

    result
}

/// Select multiple candidates from a tier using deterministic randomness.
fn select_multiple_from_tier(
    tier: &[&RelayCandidate],
    seed: &[u8; 32],
    tier_index: u8,
    max_count: usize,
) -> Vec<AuthorityId> {
    if tier.is_empty() {
        return Vec::new();
    }

    let count = tier.len().min(max_count);
    let mut result = Vec::with_capacity(count);
    let mut remaining: Vec<_> = tier.to_vec();

    for selection_round in 0..count {
        if remaining.is_empty() {
            break;
        }

        // Derive a round-specific seed
        let mut hasher = hasher();
        hasher.update(seed);
        hasher.update(&[tier_index, selection_round as u8]);
        let round_seed = hasher.finalize();

        // Use first 8 bytes as index
        // Hash output is always 32 bytes, so we can safely extract first 8 bytes
        let index_bytes: [u8; 8] = [
            round_seed[0],
            round_seed[1],
            round_seed[2],
            round_seed[3],
            round_seed[4],
            round_seed[5],
            round_seed[6],
            round_seed[7],
        ];
        let index = u64::from_le_bytes(index_bytes) as usize % remaining.len();

        result.push(remaining[index].authority_id);
        remaining.remove(index);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::identifiers::ContextId;

    fn test_authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn test_context() -> RelayContext {
        RelayContext::new(
            ContextId::new_from_entropy([1u8; 32]),
            test_authority(1),
            test_authority(2),
            1,
            [0u8; 32],
        )
    }

    fn block_candidate(seed: u8) -> RelayCandidate {
        RelayCandidate::block_peer(test_authority(seed), [seed; 32])
    }

    fn neighborhood_candidate(seed: u8) -> RelayCandidate {
        RelayCandidate::neighborhood_peer(test_authority(seed), [seed; 32])
    }

    fn guardian_candidate(seed: u8) -> RelayCandidate {
        RelayCandidate::guardian(test_authority(seed))
    }

    #[test]
    fn test_hash_relay_seed_deterministic() {
        let context = test_context();

        let seed1 = hash_relay_seed(&context);
        let seed2 = hash_relay_seed(&context);

        assert_eq!(seed1, seed2);
    }

    #[test]
    fn test_hash_relay_seed_different_contexts() {
        let context1 = test_context();
        let mut context2 = test_context();
        context2.epoch = 2;

        let seed1 = hash_relay_seed(&context1);
        let seed2 = hash_relay_seed(&context2);

        assert_ne!(seed1, seed2);
    }

    #[test]
    fn test_partition_by_relationship() {
        let candidates = vec![
            block_candidate(1),
            block_candidate(2),
            neighborhood_candidate(3),
            guardian_candidate(4),
        ];

        let (blocks, neighborhoods, guardians) = partition_by_relationship(&candidates);

        assert_eq!(blocks.len(), 2);
        assert_eq!(neighborhoods.len(), 1);
        assert_eq!(guardians.len(), 1);
    }

    #[test]
    fn test_partition_excludes_unreachable() {
        let mut candidate = block_candidate(1);
        candidate.reachable = false;

        let candidates = vec![candidate, block_candidate(2)];
        let (blocks, _, _) = partition_by_relationship(&candidates);

        assert_eq!(blocks.len(), 1);
    }

    #[test]
    fn test_select_one_from_tier_deterministic() {
        let candidates = [block_candidate(1), block_candidate(2), block_candidate(3)];
        let refs: Vec<_> = candidates.iter().collect();
        let seed = [42u8; 32];

        let selected1 = select_one_from_tier(&refs, &seed, 0);
        let selected2 = select_one_from_tier(&refs, &seed, 0);

        assert_eq!(selected1, selected2);
    }

    #[test]
    fn test_select_one_from_tier_empty() {
        let refs: Vec<&RelayCandidate> = vec![];
        let seed = [42u8; 32];

        let selected = select_one_from_tier(&refs, &seed, 0);
        assert!(selected.is_none());
    }

    #[test]
    fn test_select_by_tiers_priority() {
        let candidates = vec![
            block_candidate(1),
            neighborhood_candidate(2),
            guardian_candidate(3),
        ];
        let seed = [42u8; 32];

        let selected = select_by_tiers(&candidates, &seed, 1);

        // Should select one from each tier, block first
        assert_eq!(selected.len(), 3);
        assert_eq!(selected[0], test_authority(1)); // Block peer
        assert_eq!(selected[1], test_authority(2)); // Neighborhood peer
        assert_eq!(selected[2], test_authority(3)); // Guardian
    }

    #[test]
    fn test_select_by_tiers_max_per_tier() {
        let candidates = vec![
            block_candidate(1),
            block_candidate(2),
            block_candidate(3),
            neighborhood_candidate(4),
        ];
        let seed = [42u8; 32];

        let selected = select_by_tiers(&candidates, &seed, 2);

        // Should select 2 block peers + 1 neighborhood peer
        assert_eq!(selected.len(), 3);
    }

    #[test]
    fn test_distribution_roughly_even() {
        // Test that selection is roughly evenly distributed
        let candidates = vec![
            block_candidate(1),
            block_candidate(2),
            block_candidate(3),
            block_candidate(4),
        ];

        let mut counts = [0u32; 4];

        // Run many selections with different seeds
        for i in 0..1000u32 {
            let context = RelayContext::new(
                ContextId::new_from_entropy([1u8; 32]),
                test_authority(1),
                test_authority(2),
                i as u64,
                [0u8; 32],
            );
            let seed = hash_relay_seed(&context);
            let selected = select_by_tiers(&candidates, &seed, 1);

            if let Some(auth) = selected.first() {
                for (j, candidate) in candidates.iter().enumerate() {
                    if *auth == candidate.authority_id {
                        counts[j] += 1;
                        break;
                    }
                }
            }
        }

        // Each candidate should be selected roughly 250 times (25%)
        // Allow +/- 10% variance
        for count in counts {
            assert!(count > 150, "Count {} too low", count);
            assert!(count < 350, "Count {} too high", count);
        }
    }
}
