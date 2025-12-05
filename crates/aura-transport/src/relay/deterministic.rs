//! Deterministic Random Relay Selector
//!
//! Implements relay selection using deterministic randomness derived from
//! the relay context. This ensures reproducible behavior in tests and simulation.

use super::helpers::{hash_relay_seed, select_by_tiers};
use aura_core::{
    effects::relay::{RelayCandidate, RelayContext, RelaySelector},
    identifiers::AuthorityId,
};

/// Deterministic random relay selector.
///
/// Selects relays using hash-based deterministic randomness. The selection
/// is reproducible given the same context and candidates.
///
/// # Selection Strategy
///
/// When `prefer_proximity` is true (default):
/// 1. Block peers are preferred (highest trust, lowest latency)
/// 2. Neighborhood peers are next (established traversal rights)
/// 3. Guardians are fallback (explicit relay capability)
///
/// When `prefer_proximity` is false:
/// - All reachable candidates are treated equally (flat selection)
///
/// # Example
///
/// ```ignore
/// use aura_transport::relay::DeterministicRandomSelector;
/// use aura_core::effects::relay::RelaySelector;
///
/// // Prefer closer relationships
/// let selector = DeterministicRandomSelector::new(true);
///
/// // Flat selection across all candidates
/// let flat_selector = DeterministicRandomSelector::flat();
/// ```
#[derive(Debug, Clone)]
pub struct DeterministicRandomSelector {
    /// Whether to prefer closer relationships (tiered selection)
    prefer_proximity: bool,
    /// Maximum candidates to select from each tier
    max_per_tier: usize,
}

impl DeterministicRandomSelector {
    /// Create a new selector with proximity preference.
    ///
    /// # Arguments
    /// * `prefer_proximity` - If true, prefer block peers over neighborhood
    ///   peers over guardians. If false, treat all candidates equally.
    pub fn new(prefer_proximity: bool) -> Self {
        Self {
            prefer_proximity,
            max_per_tier: 2,
        }
    }

    /// Create a selector that prefers closer relationships.
    ///
    /// This is the default and recommended configuration.
    pub fn proximity() -> Self {
        Self::new(true)
    }

    /// Create a selector that treats all candidates equally.
    ///
    /// Useful for testing or when tier-based selection is not desired.
    pub fn flat() -> Self {
        Self::new(false)
    }

    /// Set the maximum number of candidates to select from each tier.
    ///
    /// Default is 2. Higher values provide more fallback options.
    pub fn with_max_per_tier(mut self, max: usize) -> Self {
        self.max_per_tier = max;
        self
    }

    /// Select relays using flat (non-tiered) selection.
    fn select_flat(
        &self,
        context: &RelayContext,
        candidates: &[RelayCandidate],
    ) -> Vec<AuthorityId> {
        let seed = hash_relay_seed(context);
        let reachable: Vec<_> = candidates.iter().filter(|c| c.reachable).collect();

        if reachable.is_empty() {
            return Vec::new();
        }

        // For flat selection, we create a single tier with all reachable candidates
        let count = reachable.len().min(self.max_per_tier * 3); // 3 tiers worth
        let mut result = Vec::with_capacity(count);
        let mut remaining = reachable;

        for round in 0..count {
            if remaining.is_empty() {
                break;
            }

            // Derive round-specific seed
            let mut hasher = aura_core::hash::hasher();
            hasher.update(&seed);
            hasher.update(&[round as u8]);
            let round_seed = hasher.finalize();

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
}

impl Default for DeterministicRandomSelector {
    fn default() -> Self {
        Self::proximity()
    }
}

impl RelaySelector for DeterministicRandomSelector {
    fn select(&self, context: &RelayContext, candidates: &[RelayCandidate]) -> Vec<AuthorityId> {
        if candidates.is_empty() {
            return Vec::new();
        }

        if self.prefer_proximity {
            let seed = hash_relay_seed(context);
            select_by_tiers(candidates, &seed, self.max_per_tier)
        } else {
            self.select_flat(context, candidates)
        }
    }
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
    fn test_deterministic_selection() {
        let selector = DeterministicRandomSelector::proximity();
        let context = test_context();
        let candidates = vec![
            block_candidate(1),
            block_candidate(2),
            neighborhood_candidate(3),
        ];

        let result1 = selector.select(&context, &candidates);
        let result2 = selector.select(&context, &candidates);

        assert_eq!(result1, result2);
    }

    #[test]
    fn test_empty_candidates() {
        let selector = DeterministicRandomSelector::proximity();
        let context = test_context();
        let candidates: Vec<RelayCandidate> = vec![];

        let result = selector.select(&context, &candidates);
        assert!(result.is_empty());
    }

    #[test]
    fn test_single_candidate() {
        let selector = DeterministicRandomSelector::proximity();
        let context = test_context();
        let candidates = vec![block_candidate(1)];

        let result = selector.select(&context, &candidates);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], test_authority(1));
    }

    #[test]
    fn test_proximity_prefers_block_peers() {
        let selector = DeterministicRandomSelector::proximity().with_max_per_tier(1);
        let context = test_context();
        let candidates = vec![
            neighborhood_candidate(1),
            guardian_candidate(2),
            block_candidate(3),
        ];

        let result = selector.select(&context, &candidates);

        // Block peer should be first even though listed last
        assert!(!result.is_empty());
        assert_eq!(result[0], test_authority(3));
    }

    #[test]
    fn test_flat_selection_no_preference() {
        let selector = DeterministicRandomSelector::flat();
        // Create candidates of different types
        let candidates = vec![
            block_candidate(1),
            neighborhood_candidate(2),
            guardian_candidate(3),
        ];

        // Run selection many times to verify distribution isn't biased
        let mut block_first = 0;
        let mut neighborhood_first = 0;
        let mut guardian_first = 0;

        for i in 0..300u64 {
            let mut ctx = test_context();
            ctx.epoch = i;

            let result = selector.select(&ctx, &candidates);
            if let Some(first) = result.first() {
                if *first == test_authority(1) {
                    block_first += 1;
                } else if *first == test_authority(2) {
                    neighborhood_first += 1;
                } else if *first == test_authority(3) {
                    guardian_first += 1;
                }
            }
        }

        // Each should be selected roughly 100 times (33%)
        // Allow wide variance since sample size is small
        assert!(block_first > 50, "block_first {} too low", block_first);
        assert!(
            neighborhood_first > 50,
            "neighborhood_first {} too low",
            neighborhood_first
        );
        assert!(
            guardian_first > 50,
            "guardian_first {} too low",
            guardian_first
        );
    }

    #[test]
    fn test_unreachable_excluded() {
        let selector = DeterministicRandomSelector::proximity();
        let context = test_context();

        let mut unreachable = block_candidate(1);
        unreachable.reachable = false;

        let candidates = vec![unreachable, neighborhood_candidate(2)];

        let result = selector.select(&context, &candidates);

        // Only neighborhood peer should be selected
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], test_authority(2));
    }

    #[test]
    fn test_max_per_tier() {
        let selector = DeterministicRandomSelector::proximity().with_max_per_tier(1);
        let context = test_context();

        let candidates = vec![
            block_candidate(1),
            block_candidate(2),
            block_candidate(3),
            neighborhood_candidate(4),
        ];

        let result = selector.select(&context, &candidates);

        // Should select 1 block peer + 1 neighborhood peer
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_different_contexts_different_results() {
        let selector = DeterministicRandomSelector::proximity();
        let candidates = vec![
            block_candidate(1),
            block_candidate(2),
            block_candidate(3),
            block_candidate(4),
        ];

        // With enough different contexts, we should see different selections
        let mut seen_different = false;
        let mut last_result = None;

        for i in 0..100u64 {
            let mut ctx = test_context();
            ctx.epoch = i;

            let result = selector.select(&ctx, &candidates);

            if let Some(last) = &last_result {
                if result != *last {
                    seen_different = true;
                    break;
                }
            }
            last_result = Some(result);
        }

        assert!(
            seen_different,
            "Expected different selections for different contexts"
        );
    }
}
