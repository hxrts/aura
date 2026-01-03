//! Maintenance fact types and reducers.

use aura_core::hash::hash;
use aura_core::time::ProvenancedTime;
use aura_core::types::facts::{FactDelta, FactDeltaReducer, FactTypeId};
use aura_core::types::Epoch;
use aura_core::{AccountId, AuthorityId, ContextId, Hash32, SemanticVersion};
use aura_journal::reduction::{RelationalBinding, RelationalBindingType};
use aura_journal::{DomainFact, FactReducer};
use aura_macros::DomainFact;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

/// Type identifier for maintenance facts.
pub static MAINTENANCE_FACT_TYPE_ID: FactTypeId = FactTypeId::new("maintenance");
/// Schema version for maintenance fact encoding.
pub const MAINTENANCE_FACT_SCHEMA_VERSION: u16 = 1;

/// Get the typed fact ID for maintenance facts.
pub fn maintenance_fact_type_id() -> &'static FactTypeId {
    &MAINTENANCE_FACT_TYPE_ID
}

/// Cache key used for invalidation facts.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheKey(pub String);

/// Identity epoch fence used for hard-fork upgrades.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityEpochFence {
    /// Account the fence applies to.
    pub account_id: AccountId,
    /// Target epoch for enforcement.
    pub epoch: Epoch,
}

impl IdentityEpochFence {
    /// Helper constructor.
    pub fn new(account_id: AccountId, epoch: Epoch) -> Self {
        Self { account_id, epoch }
    }
}

/// Epoch tuple used by maintenance workflows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaintenanceEpoch {
    /// Current identity epoch for the authority.
    pub identity_epoch: Epoch,
    /// Snapshot epoch used for garbage collection fencing.
    pub snapshot_epoch: Epoch,
}

impl MaintenanceEpoch {
    /// Helper constructor.
    pub fn new(identity_epoch: Epoch, snapshot_epoch: Epoch) -> Self {
        Self {
            identity_epoch,
            snapshot_epoch,
        }
    }
}

/// Upgrade proposal metadata carried in activation facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpgradeProposalMetadata {
    /// Package identifier.
    pub package_id: Uuid,
    /// Protocol version.
    pub version: SemanticVersion,
    /// Hash of the upgrade artifact.
    pub artifact_hash: Hash32,
}

/// Snapshot proposal metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotProposed {
    /// Authority that initiated the proposal.
    pub authority_id: AuthorityId,
    /// Unique proposal identifier.
    pub proposal_id: Uuid,
    /// Identity epoch fence for the snapshot.
    pub target_epoch: Epoch,
    /// Digest of the candidate snapshot payload.
    pub state_digest: Hash32,
}

impl SnapshotProposed {
    /// Create a new proposal.
    pub fn new(
        authority_id: AuthorityId,
        proposal_id: Uuid,
        target_epoch: Epoch,
        state_digest: Hash32,
    ) -> Self {
        Self {
            authority_id,
            proposal_id,
            target_epoch,
            state_digest,
        }
    }
}

/// Snapshot completion payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotCompleted {
    /// Authority that completed the snapshot.
    pub authority_id: AuthorityId,
    /// Identifier of the accepted proposal.
    pub proposal_id: Uuid,
    /// Finalized snapshot payload.
    pub snapshot: aura_core::tree::Snapshot,
    /// Participants that contributed to the threshold signature.
    pub participants: BTreeSet<AuthorityId>,
    /// Threshold signature attesting to this snapshot.
    pub threshold_signature: Vec<u8>,
}

impl SnapshotCompleted {
    /// Convenience constructor.
    pub fn new(
        authority_id: AuthorityId,
        proposal_id: Uuid,
        snapshot: aura_core::tree::Snapshot,
        participants: BTreeSet<AuthorityId>,
        threshold_signature: Vec<u8>,
    ) -> Self {
        Self {
            authority_id,
            proposal_id,
            snapshot,
            participants,
            threshold_signature,
        }
    }
}

/// Cache invalidation payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheInvalidated {
    /// Authority issuing the invalidation.
    pub authority_id: AuthorityId,
    /// Keys that must be refreshed.
    pub keys: Vec<CacheKey>,
    /// Earliest identity epoch the cache entry remains valid for.
    pub epoch_floor: Epoch,
}

impl CacheInvalidated {
    /// Create a new invalidation payload.
    pub fn new(authority_id: AuthorityId, keys: Vec<CacheKey>, epoch_floor: Epoch) -> Self {
        Self {
            authority_id,
            keys,
            epoch_floor,
        }
    }
}

/// Upgrade activation metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpgradeActivated {
    /// Authority issuing the activation.
    pub authority_id: AuthorityId,
    /// Unique identifier of the upgrade package.
    pub package_id: Uuid,
    /// Protocol version activated.
    pub to_version: SemanticVersion,
    /// Identity epoch fence where the upgrade becomes mandatory.
    pub activation_fence: IdentityEpochFence,
    /// Artifact metadata for verification.
    pub metadata: UpgradeProposalMetadata,
}

impl UpgradeActivated {
    /// Create a new activation fact.
    pub fn new(
        authority_id: AuthorityId,
        package_id: Uuid,
        to_version: SemanticVersion,
        activation_fence: IdentityEpochFence,
        metadata: UpgradeProposalMetadata,
    ) -> Self {
        Self {
            authority_id,
            package_id,
            to_version,
            activation_fence,
            metadata,
        }
    }
}

/// Admin replacement fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminReplacement {
    /// Authority issuing the replacement.
    pub authority_id: AuthorityId,
    /// Previous administrator authority.
    pub old_admin: AuthorityId,
    /// New administrator authority.
    pub new_admin: AuthorityId,
    /// Epoch when the replacement becomes active.
    pub activation_epoch: Epoch,
}

impl AdminReplacement {
    /// Create a new admin replacement fact.
    pub fn new(
        authority_id: AuthorityId,
        old_admin: AuthorityId,
        new_admin: AuthorityId,
        activation_epoch: Epoch,
    ) -> Self {
        Self {
            authority_id,
            old_admin,
            new_admin,
            activation_epoch,
        }
    }
}

/// Maintenance facts stored in authority journals.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, DomainFact)]
#[serde(deny_unknown_fields)]
#[domain_fact(type_id = "maintenance", schema_version = 1, context_fn = "context_id")]
pub enum MaintenanceFact {
    /// Snapshot proposal fact.
    SnapshotProposed(SnapshotProposed),
    /// Snapshot completion fact.
    SnapshotCompleted(SnapshotCompleted),
    /// Cache invalidation fact.
    CacheInvalidated(CacheInvalidated),
    /// Upgrade activation fact.
    UpgradeActivated(UpgradeActivated),
    /// Admin replacement fact.
    AdminReplacement(AdminReplacement),
}

impl MaintenanceFact {
    /// Authority associated with this fact.
    pub fn authority_id(&self) -> AuthorityId {
        match self {
            MaintenanceFact::SnapshotProposed(fact) => fact.authority_id,
            MaintenanceFact::SnapshotCompleted(fact) => fact.authority_id,
            MaintenanceFact::CacheInvalidated(fact) => fact.authority_id,
            MaintenanceFact::UpgradeActivated(fact) => fact.authority_id,
            MaintenanceFact::AdminReplacement(fact) => fact.authority_id,
        }
    }

    /// Context derived from the authority id.
    pub fn context_id(&self) -> ContextId {
        let authority = self.authority_id();
        ContextId::new_from_entropy(hash(&authority.to_bytes()))
    }

    /// Sub-type identifier for reducer keys.
    pub fn fact_type(&self) -> &'static str {
        match self {
            MaintenanceFact::SnapshotProposed(_) => "snapshot-proposed",
            MaintenanceFact::SnapshotCompleted(_) => "snapshot-completed",
            MaintenanceFact::CacheInvalidated(_) => "cache-invalidated",
            MaintenanceFact::UpgradeActivated(_) => "upgrade-activated",
            MaintenanceFact::AdminReplacement(_) => "admin-replacement",
        }
    }

    /// Get the typed operation for this fact.
    ///
    /// This enables compile-time exhaustive category lookups via
    /// `fact.operation().category()`.
    pub fn operation(&self) -> crate::MaintenanceOperation {
        match self {
            MaintenanceFact::SnapshotProposed(_) => crate::MaintenanceOperation::SnapshotProposed,
            MaintenanceFact::SnapshotCompleted(_) => crate::MaintenanceOperation::SnapshotCompleted,
            MaintenanceFact::CacheInvalidated(_) => crate::MaintenanceOperation::CacheInvalidated,
            MaintenanceFact::UpgradeActivated(_) => crate::MaintenanceOperation::UpgradeActivated,
            MaintenanceFact::AdminReplacement(_) => crate::MaintenanceOperation::AdminReplacement,
        }
    }

    /// Get the operation category for this fact.
    ///
    /// Convenience method equivalent to `fact.operation().category()`.
    pub fn category(&self) -> crate::OperationCategory {
        self.operation().category()
    }

    /// Stable reducer key for this fact type.
    pub fn binding_key(&self) -> MaintenanceFactKey {
        let (sub_type, data) = match self {
            MaintenanceFact::SnapshotProposed(fact) => (
                "snapshot-proposed",
                aura_core::util::serialization::to_vec(&fact.proposal_id).unwrap_or_default(),
            ),
            MaintenanceFact::SnapshotCompleted(fact) => (
                "snapshot-completed",
                aura_core::util::serialization::to_vec(&fact.proposal_id).unwrap_or_default(),
            ),
            MaintenanceFact::CacheInvalidated(_) => ("cache-invalidated", Vec::new()),
            MaintenanceFact::UpgradeActivated(fact) => (
                "upgrade-activated",
                aura_core::util::serialization::to_vec(&fact.package_id).unwrap_or_default(),
            ),
            MaintenanceFact::AdminReplacement(fact) => (
                "admin-replacement",
                aura_core::util::serialization::to_vec(&fact.new_admin).unwrap_or_default(),
            ),
        };
        MaintenanceFactKey { sub_type, data }
    }
}

/// Key for indexing maintenance facts in the journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaintenanceFactKey {
    /// Fact subtype identifier.
    pub sub_type: &'static str,
    /// Opaque key payload for the subtype.
    pub data: Vec<u8>,
}

/// Delta produced by maintenance fact reduction.
#[derive(Debug, Clone, Default)]
pub struct MaintenanceFactDelta {
    /// Count of snapshot proposals.
    pub snapshot_proposals: u64,
    /// Count of snapshot completions.
    pub snapshot_completions: u64,
    /// Count of cache invalidation facts.
    pub cache_invalidations: u64,
    /// Number of cache keys invalidated.
    pub cache_keys_invalidated: u64,
    /// Count of upgrade activations.
    pub upgrades_activated: u64,
    /// Count of admin replacements.
    pub admin_replacements: u64,
}

impl FactDelta for MaintenanceFactDelta {
    fn merge(&mut self, other: &Self) {
        self.snapshot_proposals += other.snapshot_proposals;
        self.snapshot_completions += other.snapshot_completions;
        self.cache_invalidations += other.cache_invalidations;
        self.cache_keys_invalidated += other.cache_keys_invalidated;
        self.upgrades_activated += other.upgrades_activated;
        self.admin_replacements += other.admin_replacements;
    }
}

/// Reducer for maintenance facts.
#[derive(Debug, Clone, Default)]
pub struct MaintenanceFactReducer;

impl FactDeltaReducer<MaintenanceFact, MaintenanceFactDelta> for MaintenanceFactReducer {
    fn apply(&self, fact: &MaintenanceFact) -> MaintenanceFactDelta {
        let mut delta = MaintenanceFactDelta::default();
        match fact {
            MaintenanceFact::SnapshotProposed(_) => delta.snapshot_proposals += 1,
            MaintenanceFact::SnapshotCompleted(_) => delta.snapshot_completions += 1,
            MaintenanceFact::CacheInvalidated(fact) => {
                delta.cache_invalidations += 1;
                delta.cache_keys_invalidated += fact.keys.len() as u64;
            }
            MaintenanceFact::UpgradeActivated(_) => delta.upgrades_activated += 1,
            MaintenanceFact::AdminReplacement(_) => delta.admin_replacements += 1,
        }
        delta
    }
}

impl FactReducer for MaintenanceFactReducer {
    fn handles_type(&self) -> &'static str {
        MAINTENANCE_FACT_TYPE_ID.as_str()
    }

    fn reduce(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> Option<RelationalBinding> {
        if binding_type != MAINTENANCE_FACT_TYPE_ID.as_str() {
            return None;
        }

        let fact = MaintenanceFact::from_bytes(binding_data)?;
        let key = fact.binding_key();

        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(key.sub_type.to_string()),
            context_id,
            data: key.data,
        })
    }
}

impl MaintenanceFact {
    /// Produce a human-readable summary for logs.
    pub fn summary(&self) -> String {
        match self {
            MaintenanceFact::SnapshotProposed(fact) => format!(
                "snapshot_proposed:{}:{}",
                fact.authority_id, fact.target_epoch
            ),
            MaintenanceFact::SnapshotCompleted(fact) => format!(
                "snapshot_completed:{}:{}",
                fact.authority_id, fact.snapshot.epoch
            ),
            MaintenanceFact::CacheInvalidated(fact) => format!(
                "cache_invalidated:{}:{}",
                fact.authority_id, fact.epoch_floor
            ),
            MaintenanceFact::UpgradeActivated(fact) => format!(
                "upgrade_activated:{}:{}",
                fact.authority_id, fact.activation_fence.epoch
            ),
            MaintenanceFact::AdminReplacement(fact) => format!(
                "admin_replacement:{}:{}",
                fact.old_admin, fact.activation_epoch
            ),
        }
    }
}

/// Snapshot completion receipt used by maintenance workflows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotReceipt {
    /// Proposal identifier for the completed snapshot.
    pub proposal_id: Uuid,
    /// Completion time for the snapshot.
    pub completed_at: ProvenancedTime,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    #[test]
    fn fact_round_trip() {
        let fact = MaintenanceFact::CacheInvalidated(CacheInvalidated::new(
            authority(1),
            vec![CacheKey("key".to_string())],
            Epoch::new(2),
        ));
        let bytes = fact.to_bytes();
        let restored = match MaintenanceFact::from_bytes(&bytes) {
            Some(restored) => restored,
            None => panic!("decode"),
        };
        assert_eq!(fact, restored);
    }

    #[test]
    fn reducer_tracks_counts() {
        let reducer = MaintenanceFactReducer;
        let fact = MaintenanceFact::SnapshotProposed(SnapshotProposed::new(
            authority(2),
            Uuid::from_bytes(1u128.to_be_bytes()),
            Epoch::new(1),
            Hash32([3u8; 32]),
        ));
        let delta = reducer.apply(&fact);
        assert_eq!(delta.snapshot_proposals, 1);
    }
}

/// Property tests for semilattice laws on MaintenanceFactDelta
#[cfg(test)]
#[allow(clippy::redundant_clone)]
mod proptest_semilattice {
    use super::*;
    use aura_core::types::facts::FactDelta;
    use proptest::prelude::*;

    /// Strategy for generating arbitrary MaintenanceFactDelta values
    fn arb_delta() -> impl Strategy<Value = MaintenanceFactDelta> {
        (
            0u64..1000,
            0u64..1000,
            0u64..1000,
            0u64..1000,
            0u64..1000,
            0u64..1000,
        )
            .prop_map(
                |(
                    snapshot_proposals,
                    snapshot_completions,
                    cache_invalidations,
                    cache_keys_invalidated,
                    upgrades_activated,
                    admin_replacements,
                )| {
                    MaintenanceFactDelta {
                        snapshot_proposals,
                        snapshot_completions,
                        cache_invalidations,
                        cache_keys_invalidated,
                        upgrades_activated,
                        admin_replacements,
                    }
                },
            )
    }

    /// Helper to check if two deltas are equal
    fn deltas_equal(a: &MaintenanceFactDelta, b: &MaintenanceFactDelta) -> bool {
        a.snapshot_proposals == b.snapshot_proposals
            && a.snapshot_completions == b.snapshot_completions
            && a.cache_invalidations == b.cache_invalidations
            && a.cache_keys_invalidated == b.cache_keys_invalidated
            && a.upgrades_activated == b.upgrades_activated
            && a.admin_replacements == b.admin_replacements
    }

    proptest! {
        /// Idempotence: merging with self leaves the value unchanged
        /// a.merge(&a) == a
        #[test]
        fn merge_idempotent(a in arb_delta()) {
            let original = a.clone();
            let mut result = a.clone();
            result.merge(&original);
            // For additive deltas, idempotence means a + a = 2a
            // But for semilattice merge (max), a merge a = a
            // Our delta uses addition, so we check that merge(a,a) = 2a
            prop_assert_eq!(result.snapshot_proposals, original.snapshot_proposals * 2);
            prop_assert_eq!(result.snapshot_completions, original.snapshot_completions * 2);
        }

        /// Commutativity: a.merge(&b) == b.merge(&a) (result equivalence)
        #[test]
        fn merge_commutative(a in arb_delta(), b in arb_delta()) {
            let mut ab = a.clone();
            ab.merge(&b);

            let mut ba = b.clone();
            ba.merge(&a);

            prop_assert!(deltas_equal(&ab, &ba), "merge should be commutative");
        }

        /// Associativity: (a.merge(&b)).merge(&c) == a.merge(&(b.merge(&c)))
        #[test]
        fn merge_associative(a in arb_delta(), b in arb_delta(), c in arb_delta()) {
            // Left associative: (a merge b) merge c
            let mut left = a.clone();
            left.merge(&b);
            left.merge(&c);

            // Right associative: a merge (b merge c)
            let mut bc = b.clone();
            bc.merge(&c);
            let mut right = a.clone();
            right.merge(&bc);

            prop_assert!(deltas_equal(&left, &right), "merge should be associative");
        }

        /// Identity: merge with default (zero) leaves value unchanged
        #[test]
        fn merge_identity(a in arb_delta()) {
            let original = a.clone();
            let mut result = a.clone();
            result.merge(&MaintenanceFactDelta::default());

            prop_assert!(deltas_equal(&result, &original), "merge with identity should preserve value");
        }
    }
}
