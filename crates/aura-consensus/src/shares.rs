//! Type-safe linear share collection with threshold proofs
//!
//! This module provides compile-time guarantees that threshold signatures can
//! only be combined after collecting sufficient shares. Uses a sealed/unsealed
//! type pattern to prevent calling combine() before threshold is met.
//!
//! ## Status
//!
//! **Implemented but not yet integrated into the consensus protocol.**
//!
//! This module was designed for a more complex consensus model where witnesses
//! might vote for different result IDs (requiring conflict resolution). The
//! current consensus protocol uses a simpler model where all witnesses vote on
//! the same operation hash.
//!
//! ## Integration Path
//!
//! Two options for future integration:
//!
//! 1. **Adapt to single-result-ID model**: Simplify ShareCollector to work with
//!    the current protocol's assumption of a single agreed-upon result ID.
//!
//! 2. **Refactor protocol for multi-result-ID handling**: Extend the protocol to
//!    explicitly handle cases where witnesses vote for different results, using
//!    ShareCollector's multi-result tracking.
//!
//! Until integration, the current HashMap-based signature collection in
//! WitnessTracker works correctly but lacks compile-time threshold guarantees.
//!
//! ## Design
//!
//! The sealed/unsealed type pattern provides type-level proof of threshold:
//!
//! ```text
//! LinearShareSet (unsealed)
//!   └─ accepts shares via try_insert()
//!   └─ when threshold reached → seals into ThresholdShareSet
//!
//! ThresholdShareSet (sealed)
//!   └─ ONLY this type has combine() method
//!   └─ type system proves >= threshold shares exist
//! ```
//!
//! See `EQUIVOCATION_ARCHITECTURE.md` for full architectural context.

use aura_core::{
    frost::{PartialSignature, ThresholdSignature},
    AuthorityId, Hash32, Result,
};
use std::collections::BTreeMap;

/// Type alias for ResultId (the hash of a consensus result)
pub type ResultId = Hash32;

/// Share collector manages multiple result IDs, each with its own share set
///
/// This is the main entry point for collecting shares during consensus.
/// It tracks shares by ResultId and returns a ThresholdShareSet when
/// the threshold is reached for a particular result.
#[derive(Debug, Clone)]
pub struct ShareCollector {
    threshold: usize,
    shares_by_rid: BTreeMap<ResultId, LinearShareSet>,
}

impl ShareCollector {
    /// Create a new share collector with the given threshold
    pub fn new(threshold: usize) -> Self {
        Self {
            threshold,
            shares_by_rid: BTreeMap::new(),
        }
    }

    /// Try to insert a share. Returns ThresholdShareSet if threshold reached.
    ///
    /// # Errors
    /// - Returns error if the witness has already submitted a share for this result ID
    /// - Returns error if trying to insert into a sealed share set
    pub fn try_insert(
        &mut self,
        rid: ResultId,
        witness: AuthorityId,
        share: PartialSignature,
    ) -> Result<InsertResult> {
        let share_set = self
            .shares_by_rid
            .entry(rid)
            .or_insert_with(LinearShareSet::new);

        share_set.try_insert(witness, share)?;

        // Check if we reached threshold
        if let Some(threshold_set) = share_set.seal_if_threshold_reached(self.threshold) {
            Ok(InsertResult::ThresholdReached(threshold_set))
        } else {
            Ok(InsertResult::Inserted {
                count: share_set.count(),
            })
        }
    }

    /// Get the current share count for a result ID
    pub fn share_count(&self, rid: &ResultId) -> usize {
        self.shares_by_rid.get(rid).map(|s| s.count()).unwrap_or(0)
    }

    /// Check if a particular result ID has reached threshold
    pub fn has_threshold(&self, rid: &ResultId) -> bool {
        self.shares_by_rid
            .get(rid)
            .map(|s| s.is_sealed())
            .unwrap_or(false)
    }

    /// Get all result IDs being tracked
    pub fn result_ids(&self) -> Vec<ResultId> {
        self.shares_by_rid.keys().copied().collect()
    }

    /// Get all signatures for a specific result_id
    pub fn get_signatures_for_result(&self, rid: &ResultId) -> Vec<PartialSignature> {
        self.shares_by_rid
            .get(rid)
            .map(|set| set.shares.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Get all witnesses (participants) for a specific result_id
    pub fn get_participants_for_result(&self, rid: &ResultId) -> Vec<AuthorityId> {
        self.shares_by_rid
            .get(rid)
            .map(|set| set.shares.keys().cloned().collect())
            .unwrap_or_default()
    }
}

/// Unsealed share set - can accept new shares until sealed
///
/// This type represents a share set that has not yet reached threshold.
/// It can accept new shares via `try_insert()`.
#[derive(Debug, Clone)]
pub struct LinearShareSet {
    shares: BTreeMap<AuthorityId, PartialSignature>,
    sealed: bool,
}

impl LinearShareSet {
    fn new() -> Self {
        Self {
            shares: BTreeMap::new(),
            sealed: false,
        }
    }

    fn try_insert(&mut self, witness: AuthorityId, share: PartialSignature) -> Result<()> {
        if self.sealed {
            return Err(aura_core::AuraError::invalid(
                "Cannot insert into sealed share set",
            ));
        }

        if self.shares.contains_key(&witness) {
            return Err(aura_core::AuraError::invalid(
                "Duplicate share from witness",
            ));
        }

        self.shares.insert(witness, share);
        Ok(())
    }

    fn seal_if_threshold_reached(&mut self, threshold: usize) -> Option<ThresholdShareSet> {
        if self.shares.len() >= threshold && !self.sealed {
            self.sealed = true;
            Some(ThresholdShareSet {
                shares: self.shares.clone(),
            })
        } else {
            None
        }
    }

    fn count(&self) -> usize {
        self.shares.len()
    }

    fn is_sealed(&self) -> bool {
        self.sealed
    }
}

/// Sealed share set - threshold proven at type level
///
/// This is the ONLY type that can call combine() to create a threshold signature.
/// The type system guarantees that we have >= threshold shares.
#[derive(Debug, Clone)]
pub struct ThresholdShareSet {
    shares: BTreeMap<AuthorityId, PartialSignature>,
}

impl ThresholdShareSet {
    /// Combine shares into threshold signature.
    ///
    /// Type-level guarantee: we have >= threshold shares.
    ///
    /// # Errors
    /// Returns error if FROST signature combination fails (cryptographic error).
    pub fn combine(self) -> Result<ThresholdSignature> {
        // Extract signatures in deterministic order (BTreeMap is sorted)
        let signatures: Vec<_> = self.shares.values().cloned().collect();
        let signers: Vec<_> = signatures.iter().map(|s| s.signer).collect();

        // Use FROST to combine signatures
        // Note: This is a simplified version. In production, this would call
        // the actual FROST combine function from aura-core::crypto::tree_signing
        if signatures.is_empty() {
            return Err(aura_core::AuraError::invalid(
                "Cannot combine empty signature set",
            ));
        }

        // For now, create a threshold signature from the collected shares
        // In production, this would use frost_ed25519::aggregate
        Ok(ThresholdSignature {
            signature: signatures[0].signature.clone(), // Placeholder
            signers,
        })
    }

    /// Get read-only access to the shares
    pub fn shares(&self) -> &BTreeMap<AuthorityId, PartialSignature> {
        &self.shares
    }

    /// Get the number of shares
    pub fn count(&self) -> usize {
        self.shares.len()
    }

    /// Get the list of witnesses who contributed shares
    pub fn witnesses(&self) -> Vec<AuthorityId> {
        self.shares.keys().copied().collect()
    }
}

/// Result of attempting to insert a share
pub enum InsertResult {
    /// Share was recorded, but threshold not yet reached
    Inserted { count: usize },
    /// Threshold reached, sealed share set returned
    ThresholdReached(ThresholdShareSet),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn test_result_id(seed: u8) -> ResultId {
        Hash32::new([seed; 32])
    }

    fn test_signature(seed: u8) -> PartialSignature {
        PartialSignature {
            signer: seed as u16,
            signature: vec![seed; 64],
        }
    }

    #[test]
    fn test_linear_share_set_rejects_duplicates() {
        let mut set = LinearShareSet::new();
        let witness = test_authority(1);
        let share = test_signature(1);

        // First insert succeeds
        assert!(set.try_insert(witness, share.clone()).is_ok());

        // Duplicate insert fails
        assert!(set.try_insert(witness, share).is_err());
    }

    #[test]
    fn test_linear_share_set_seals_at_threshold() {
        let mut set = LinearShareSet::new();
        let threshold = 2;

        // Add first share
        set.try_insert(test_authority(1), test_signature(1))
            .unwrap();
        assert!(!set.is_sealed());
        assert!(set.seal_if_threshold_reached(threshold).is_none());

        // Add second share - should seal
        set.try_insert(test_authority(2), test_signature(2))
            .unwrap();
        let sealed = set.seal_if_threshold_reached(threshold);
        assert!(sealed.is_some());
        assert!(set.is_sealed());
    }

    #[test]
    fn test_sealed_set_rejects_new_shares() {
        let mut set = LinearShareSet::new();
        let threshold = 2;

        // Add shares to reach threshold
        set.try_insert(test_authority(1), test_signature(1))
            .unwrap();
        set.try_insert(test_authority(2), test_signature(2))
            .unwrap();
        set.seal_if_threshold_reached(threshold);

        // Try to add another share - should fail
        let result = set.try_insert(test_authority(3), test_signature(3));
        assert!(result.is_err());
    }

    #[test]
    fn test_threshold_share_set_can_combine() {
        let mut collector = ShareCollector::new(2);
        let rid = test_result_id(1);

        // Insert first share
        let result = collector
            .try_insert(rid, test_authority(1), test_signature(1))
            .unwrap();
        assert!(matches!(result, InsertResult::Inserted { count: 1 }));

        // Insert second share - should reach threshold
        let result = collector
            .try_insert(rid, test_authority(2), test_signature(2))
            .unwrap();

        match result {
            InsertResult::ThresholdReached(threshold_set) => {
                // Should be able to combine
                let signature = threshold_set.combine();
                assert!(signature.is_ok());
            }
            _ => panic!("Expected ThresholdReached"),
        }
    }

    #[test]
    fn test_share_collector_multi_result_id() {
        let mut collector = ShareCollector::new(2);
        let rid1 = test_result_id(1);
        let rid2 = test_result_id(2);

        // Add shares for first result ID
        collector
            .try_insert(rid1, test_authority(1), test_signature(1))
            .unwrap();
        assert_eq!(collector.share_count(&rid1), 1);
        assert_eq!(collector.share_count(&rid2), 0);

        // Add shares for second result ID
        collector
            .try_insert(rid2, test_authority(2), test_signature(2))
            .unwrap();
        assert_eq!(collector.share_count(&rid1), 1);
        assert_eq!(collector.share_count(&rid2), 1);

        // Complete first result ID
        let result = collector
            .try_insert(rid1, test_authority(3), test_signature(3))
            .unwrap();
        assert!(matches!(result, InsertResult::ThresholdReached(_)));
        assert!(collector.has_threshold(&rid1));
        assert!(!collector.has_threshold(&rid2));
    }

    #[test]
    fn test_share_collector_tracks_result_ids() {
        let mut collector = ShareCollector::new(2);
        let rid1 = test_result_id(1);
        let rid2 = test_result_id(2);

        collector
            .try_insert(rid1, test_authority(1), test_signature(1))
            .unwrap();
        collector
            .try_insert(rid2, test_authority(2), test_signature(2))
            .unwrap();

        let mut rids = collector.result_ids();
        rids.sort();
        assert_eq!(rids.len(), 2);
        assert!(rids.contains(&rid1));
        assert!(rids.contains(&rid2));
    }

    #[test]
    fn test_threshold_share_set_witnesses() {
        let mut collector = ShareCollector::new(2);
        let rid = test_result_id(1);

        collector
            .try_insert(rid, test_authority(1), test_signature(1))
            .unwrap();
        let result = collector
            .try_insert(rid, test_authority(2), test_signature(2))
            .unwrap();

        match result {
            InsertResult::ThresholdReached(threshold_set) => {
                let witnesses = threshold_set.witnesses();
                assert_eq!(witnesses.len(), 2);
                assert!(witnesses.contains(&test_authority(1)));
                assert!(witnesses.contains(&test_authority(2)));
            }
            _ => panic!("Expected ThresholdReached"),
        }
    }
}
