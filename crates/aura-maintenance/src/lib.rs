//! Maintenance domain facts and reducers.
//!
//! Maintenance facts model snapshot coordination, cache invalidation, OTA upgrades,
//! and admin replacement. Facts are stored in authority journals and reduced
//! deterministically. See docs/111_maintenance.md for behavior.

pub mod facts;

pub use facts::{
    AdminReplacement, CacheInvalidated, CacheKey, IdentityEpochFence, MaintenanceFact,
    MaintenanceFactDelta, MaintenanceFactReducer, SnapshotCompleted, SnapshotProposed,
    UpgradeActivated, UpgradeProposalMetadata, MAINTENANCE_FACT_TYPE_ID,
};
