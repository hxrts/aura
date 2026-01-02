use aura_core::types::facts::FactDelta;
use aura_core::types::FactDeltaReducer;
use aura_core::types::Epoch;
use aura_core::{AuthorityId, ContextId, Hash32, SemanticVersion};
use aura_maintenance::{
    CacheInvalidated, CacheKey, IdentityEpochFence, MaintenanceFact, MaintenanceFactDelta,
    MaintenanceFactReducer, SnapshotProposed, UpgradeActivated, UpgradeProposalMetadata,
    MAINTENANCE_FACT_TYPE_ID,
};
use aura_journal::{DomainFact, FactReducer};
use uuid::Uuid;

fn authority(seed: u8) -> AuthorityId {
    AuthorityId::new_from_entropy([seed; 32])
}

fn assert_delta_eq(left: &MaintenanceFactDelta, right: &MaintenanceFactDelta) {
    assert_eq!(left.snapshot_proposals, right.snapshot_proposals);
    assert_eq!(left.snapshot_completions, right.snapshot_completions);
    assert_eq!(left.cache_invalidations, right.cache_invalidations);
    assert_eq!(left.cache_keys_invalidated, right.cache_keys_invalidated);
    assert_eq!(left.upgrades_activated, right.upgrades_activated);
    assert_eq!(left.admin_replacements, right.admin_replacements);
}

#[test]
fn reducer_apply_is_deterministic_and_order_independent() {
    let reducer = MaintenanceFactReducer;
    let fact_a = MaintenanceFact::SnapshotProposed(SnapshotProposed::new(
        authority(1),
        Uuid::from_bytes(2u128.to_be_bytes()),
        Epoch::new(3),
        Hash32([4u8; 32]),
    ));
    let fact_b = MaintenanceFact::CacheInvalidated(CacheInvalidated::new(
        authority(2),
        vec![CacheKey("cache-key".to_string()), CacheKey("other".to_string())],
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

#[test]
fn reducer_binding_is_idempotent() {
    let reducer = MaintenanceFactReducer;
    let context_id = ContextId::new_from_entropy([9u8; 32]);
    let metadata = UpgradeProposalMetadata {
        package_id: Uuid::from_bytes(7u128.to_be_bytes()),
        version: SemanticVersion::new(1, 2, 3),
        artifact_hash: Hash32([8u8; 32]),
    };
    let fact = MaintenanceFact::UpgradeActivated(UpgradeActivated::new(
        authority(4),
        Uuid::from_bytes(6u128.to_be_bytes()),
        SemanticVersion::new(2, 0, 0),
        IdentityEpochFence::new(aura_core::AccountId::from_bytes([5u8; 32]), Epoch::new(10)),
        metadata,
    ));

    let bytes = fact.to_bytes();
    let first = reducer
        .reduce(context_id, MAINTENANCE_FACT_TYPE_ID.as_str(), &bytes)
        .expect("binding");
    let second = reducer
        .reduce(context_id, MAINTENANCE_FACT_TYPE_ID.as_str(), &bytes)
        .expect("binding");

    assert_eq!(first.binding_type, second.binding_type);
    assert_eq!(first.context_id, second.context_id);
    assert_eq!(first.data, second.data);
}
