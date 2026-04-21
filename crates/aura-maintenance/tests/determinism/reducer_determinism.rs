//! Reducer determinism for maintenance facts — same facts must produce
//! identical deltas regardless of application order. If non-deterministic,
//! replicas disagree on upgrade/snapshot state (split-brain).

use aura_core::types::facts::FactDelta;
use aura_core::types::Epoch;
use aura_core::types::FactDeltaReducer;
use aura_core::{AuthorityId, Hash32, SemanticVersion};
use aura_maintenance::{
    CacheInvalidated, CacheKey, IdentityEpochFence, MaintenanceFact, MaintenanceFactDelta,
    MaintenanceFactReducer, SnapshotProposed, UpgradeActivated, UpgradeProposalMetadata,
};
use uuid::Uuid;

fn authority(seed: u8) -> AuthorityId {
    AuthorityId::new_from_entropy([seed; 32])
}

fn hash(seed: u8) -> Hash32 {
    Hash32([seed; 32])
}

fn uuid(seed: u128) -> Uuid {
    Uuid::from_bytes(seed.to_be_bytes())
}

fn assert_delta_eq(left: &MaintenanceFactDelta, right: &MaintenanceFactDelta) {
    assert_eq!(left.snapshot_proposals, right.snapshot_proposals);
    assert_eq!(left.snapshot_completions, right.snapshot_completions);
    assert_eq!(left.cache_invalidations, right.cache_invalidations);
    assert_eq!(left.cache_keys_invalidated, right.cache_keys_invalidated);
    assert_eq!(left.upgrades_activated, right.upgrades_activated);
    assert_eq!(left.admin_replacements, right.admin_replacements);
}

/// Applying the same fact twice produces the same delta, and merging
/// deltas A+B produces the same result as B+A — commutativity + determinism.
#[test]
fn reducer_apply_is_deterministic_and_order_independent() {
    let reducer = MaintenanceFactReducer;
    let fact_a = MaintenanceFact::SnapshotProposed(SnapshotProposed::new(
        authority(1),
        uuid(2),
        Epoch::new(3),
        hash(4),
    ));
    let fact_b = MaintenanceFact::CacheInvalidated(CacheInvalidated::new(
        authority(2),
        vec![
            CacheKey("cache-key".to_string()),
            CacheKey("other".to_string()),
        ],
        Epoch::new(1),
    ));

    let delta_a = reducer.apply(&fact_a);
    let delta_a_again = reducer.apply(&fact_a);
    assert_delta_eq(&delta_a, &delta_a_again);

    let delta_b = reducer.apply(&fact_b);

    let mut merged_left = MaintenanceFactDelta::default();
    merged_left.merge(&delta_a);
    merged_left.merge(&delta_b);

    let mut merged_right = MaintenanceFactDelta::default();
    merged_right.merge(&delta_b);
    merged_right.merge(&delta_a);

    assert_delta_eq(&merged_left, &merged_right);
}

/// Maintenance fact encode → decode roundtrip — if encoding changes,
/// persisted maintenance facts become unreadable on the next release.
#[test]
fn fact_envelope_roundtrip() {
    let metadata = UpgradeProposalMetadata {
        package_id: uuid(7),
        version: SemanticVersion::new(1, 2, 3),
        artifact_hash: hash(8),
    };
    let fact = MaintenanceFact::UpgradeActivated(UpgradeActivated::new(
        authority(4),
        uuid(6),
        SemanticVersion::new(2, 0, 0),
        IdentityEpochFence::new(aura_core::AccountId::from_bytes([5u8; 32]), Epoch::new(10)),
        metadata,
    ));

    // Test encoding/decoding
    let envelope = fact.to_envelope().expect("envelope creation");
    assert_eq!(
        envelope.type_id.as_str(),
        aura_maintenance::maintenance_fact_type_id().as_str()
    );
    assert_eq!(
        envelope.schema_version,
        aura_maintenance::MAINTENANCE_FACT_SCHEMA_VERSION
    );

    // Test bytes roundtrip
    let bytes = fact.to_bytes().expect("encoding");
    let restored = MaintenanceFact::from_bytes(&bytes).expect("decoding");
    assert_eq!(fact, restored);
}

// ============================================================================
// Snapshot proposal → completion lifecycle
// ============================================================================

/// A snapshot proposal followed by completion must produce deltas with
/// both proposal and completion counts incremented. This tests the
/// two-phase snapshot lifecycle through the reducer.
#[test]
fn snapshot_proposal_then_completion_lifecycle() {
    let reducer = MaintenanceFactReducer;
    let auth = authority(10);
    let proposal_id = uuid(20);

    let proposal = MaintenanceFact::SnapshotProposed(SnapshotProposed::new(
        auth,
        proposal_id,
        Epoch::new(5),
        hash(11),
    ));

    let snapshot = aura_core::tree::Snapshot::new(
        Epoch::new(5),
        [0u8; 32],
        vec![],
        std::collections::BTreeMap::new(),
        1000,
    );
    let completion = MaintenanceFact::SnapshotCompleted(aura_maintenance::SnapshotCompleted::new(
        auth,
        proposal_id,
        snapshot,
        std::collections::BTreeSet::new(),
        vec![0xAA; 64],
    ));

    let mut delta = MaintenanceFactDelta::default();
    delta.merge(&reducer.apply(&proposal));
    delta.merge(&reducer.apply(&completion));

    assert_eq!(delta.snapshot_proposals, 1, "one proposal");
    assert_eq!(delta.snapshot_completions, 1, "one completion");
}

// ============================================================================
// Cache invalidation idempotence
// ============================================================================

/// Invalidating the same cache key twice must produce two invalidation
/// events in the delta (facts are append-only), but the cache_keys_invalidated
/// count should reflect total keys across all facts.
#[test]
fn cache_invalidation_is_additive() {
    let reducer = MaintenanceFactReducer;
    let auth = authority(30);
    let key = CacheKey("hot-cache".to_string());

    let inv1 = MaintenanceFact::CacheInvalidated(CacheInvalidated::new(
        auth,
        vec![key.clone()],
        Epoch::new(1),
    ));
    let inv2 =
        MaintenanceFact::CacheInvalidated(CacheInvalidated::new(auth, vec![key], Epoch::new(2)));

    let mut delta = MaintenanceFactDelta::default();
    delta.merge(&reducer.apply(&inv1));
    delta.merge(&reducer.apply(&inv2));

    // Two invalidation facts, each with one key
    assert_eq!(delta.cache_invalidations, 2);
    assert_eq!(delta.cache_keys_invalidated, 2);
}

// ============================================================================
// Upgrade version ordering
// ============================================================================

/// Two upgrade activations with different versions both produce deltas.
/// The reducer counts activations — version ordering policy is enforced
/// at a higher layer (aura-protocol), not at the fact/reducer layer.
#[test]
fn upgrade_activations_counted_independently() {
    let reducer = MaintenanceFactReducer;
    let auth = authority(40);
    let fence =
        IdentityEpochFence::new(aura_core::AccountId::from_bytes([50u8; 32]), Epoch::new(1));

    let v1_metadata = UpgradeProposalMetadata {
        package_id: uuid(60),
        version: SemanticVersion::new(1, 0, 0),
        artifact_hash: hash(61),
    };
    let v2_metadata = UpgradeProposalMetadata {
        package_id: uuid(70),
        version: SemanticVersion::new(2, 0, 0),
        artifact_hash: hash(71),
    };

    let upgrade_v1 = MaintenanceFact::UpgradeActivated(UpgradeActivated::new(
        auth,
        uuid(60),
        SemanticVersion::new(1, 0, 0),
        fence,
        v1_metadata,
    ));
    let upgrade_v2 = MaintenanceFact::UpgradeActivated(UpgradeActivated::new(
        auth,
        uuid(70),
        SemanticVersion::new(2, 0, 0),
        fence,
        v2_metadata,
    ));

    let mut delta = MaintenanceFactDelta::default();
    delta.merge(&reducer.apply(&upgrade_v1));
    delta.merge(&reducer.apply(&upgrade_v2));

    assert_eq!(delta.upgrades_activated, 2, "both activations counted");
}
