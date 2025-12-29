//! Maintenance domain facts and reducers.
//!
//! Maintenance facts model snapshot coordination, cache invalidation, OTA upgrades,
//! and admin replacement. Facts are stored in authority journals and reduced
//! deterministically. See docs/111_maintenance.md for behavior.

pub mod facts;
pub mod gc;

pub use facts::{
    AdminReplacement, CacheInvalidated, CacheKey, IdentityEpochFence, MaintenanceEpoch,
    MaintenanceFact, MaintenanceFactDelta, MaintenanceFactKey, MaintenanceFactReducer,
    SnapshotCompleted, SnapshotProposed, UpgradeActivated, UpgradeProposalMetadata,
    MAINTENANCE_FACT_TYPE_ID,
};
pub use gc::{plan_dkg_transcript_gc, TranscriptGcPlan};

/// Operation category map (A/B/C) for maintenance gating and review.
pub const OPERATION_CATEGORIES: &[(&str, &str)] = &[
    ("maintenance:snapshot-proposed", "B"),
    ("maintenance:snapshot-completed", "B"),
    ("maintenance:cache-invalidated", "A"),
    ("maintenance:upgrade-activated", "C"),
    ("maintenance:admin-replacement", "C"),
];

/// Lookup the operation category (A/B/C) for a given maintenance operation.
pub fn operation_category(operation: &str) -> Option<&'static str> {
    OPERATION_CATEGORIES
        .iter()
        .find(|(op, _)| *op == operation)
        .map(|(_, category)| *category)
}
