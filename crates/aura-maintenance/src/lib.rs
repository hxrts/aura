//! Maintenance domain facts and reducers.
//!
//! Maintenance facts model snapshot coordination, cache invalidation, OTA upgrades,
//! and admin replacement. Facts are stored in authority journals and reduced
//! deterministically. See docs/116_maintenance.md for behavior.

pub mod facts;
pub mod gc;
pub mod policy;
pub mod release;
pub mod scope;

pub use facts::{
    maintenance_fact_type_id, AdminReplacement, AuraUpgradeFailure, AuraUpgradeFailureClass,
    CacheInvalidated, CacheKey, IdentityEpochFence, MaintenanceEpoch, MaintenanceFact,
    MaintenanceFactDelta, MaintenanceFactKey, MaintenanceFactReducer, ReleaseDistributionFact,
    ReleasePolicyFact, SnapshotCompleted, SnapshotProposed, UpgradeActivated, UpgradeExecutionFact,
    UpgradeProposalMetadata, MAINTENANCE_FACT_SCHEMA_VERSION, MAINTENANCE_FACT_TYPE_ID,
};
pub use gc::{plan_dkg_transcript_gc, TranscriptGcPlan};
pub use policy::{
    AuraActivationTrustPolicy, AuraActivationWindow, AuraReleaseActivationPolicy,
    AuraReleaseDiscoveryPolicy, AuraReleaseSharingPolicy, AuraRollbackPreference,
    AuthoritySelector, ContextSelector, PinPolicy, RecommendationPolicy,
};
pub use release::{
    AuraActivationProfile, AuraArtifactDescriptor, AuraArtifactKind, AuraArtifactPackaging,
    AuraCompatibilityClass, AuraCompatibilityManifest, AuraDataMigration,
    AuraDeterministicBuildCertificate, AuraHealthGate, AuraLauncherEntrypoint, AuraReleaseId,
    AuraReleaseManifest, AuraReleaseProvenance, AuraReleaseSeriesId, AuraRollbackRequirement,
    AuraTargetPlatform, AuraTeeAttestation,
};
pub use scope::{AuraActivationScope, AuraPolicyScope, ReleaseResidency, TransitionState};

/// Operation category for maintenance gating and review.
///
/// - **A**: Low-risk operations that can be applied without special review
/// - **B**: Medium-risk operations requiring coordination
/// - **C**: High-risk operations requiring admin approval
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationCategory {
    /// Low-risk: cache invalidation, routine maintenance
    A,
    /// Medium-risk: snapshot coordination
    B,
    /// High-risk: upgrades, admin replacement
    C,
}

impl OperationCategory {
    /// Get the category as a string identifier.
    pub fn as_str(&self) -> &'static str {
        match self {
            OperationCategory::A => "A",
            OperationCategory::B => "B",
            OperationCategory::C => "C",
        }
    }
}

impl std::fmt::Display for OperationCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Maintenance operation types with compile-time exhaustive category mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MaintenanceOperation {
    /// Snapshot proposal coordination
    SnapshotProposed,
    /// Snapshot completion coordination
    SnapshotCompleted,
    /// Cache invalidation
    CacheInvalidated,
    /// Protocol upgrade activation
    UpgradeActivated,
    /// Admin authority replacement
    AdminReplacement,
    /// Release distribution and certification publication
    ReleaseDistribution,
    /// OTA discovery, sharing, and activation policy publication
    ReleasePolicy,
    /// Scoped OTA execution and outcome recording
    UpgradeExecution,
}

impl MaintenanceOperation {
    /// Get the operation category (compile-time exhaustive).
    ///
    /// Category assignments:
    /// - A: Cache invalidation (low-risk)
    /// - B: Snapshot operations (medium-risk)
    /// - C: Upgrades and admin replacement (high-risk)
    #[must_use]
    pub fn category(&self) -> OperationCategory {
        match self {
            MaintenanceOperation::SnapshotProposed => OperationCategory::B,
            MaintenanceOperation::SnapshotCompleted => OperationCategory::B,
            MaintenanceOperation::CacheInvalidated => OperationCategory::A,
            MaintenanceOperation::UpgradeActivated => OperationCategory::C,
            MaintenanceOperation::AdminReplacement => OperationCategory::C,
            MaintenanceOperation::ReleaseDistribution => OperationCategory::B,
            MaintenanceOperation::ReleasePolicy => OperationCategory::C,
            MaintenanceOperation::UpgradeExecution => OperationCategory::C,
        }
    }

    /// Get the operation name as a string.
    pub fn as_str(&self) -> &'static str {
        match self {
            MaintenanceOperation::SnapshotProposed => "maintenance:snapshot-proposed",
            MaintenanceOperation::SnapshotCompleted => "maintenance:snapshot-completed",
            MaintenanceOperation::CacheInvalidated => "maintenance:cache-invalidated",
            MaintenanceOperation::UpgradeActivated => "maintenance:upgrade-activated",
            MaintenanceOperation::AdminReplacement => "maintenance:admin-replacement",
            MaintenanceOperation::ReleaseDistribution => "maintenance:release-distribution",
            MaintenanceOperation::ReleasePolicy => "maintenance:release-policy",
            MaintenanceOperation::UpgradeExecution => "maintenance:upgrade-execution",
        }
    }

    /// Parse an operation from its string name.
    #[allow(clippy::should_implement_trait)] // Returns Option, not Result
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "maintenance:snapshot-proposed" => Some(MaintenanceOperation::SnapshotProposed),
            "maintenance:snapshot-completed" => Some(MaintenanceOperation::SnapshotCompleted),
            "maintenance:cache-invalidated" => Some(MaintenanceOperation::CacheInvalidated),
            "maintenance:upgrade-activated" => Some(MaintenanceOperation::UpgradeActivated),
            "maintenance:admin-replacement" => Some(MaintenanceOperation::AdminReplacement),
            "maintenance:release-distribution" => Some(MaintenanceOperation::ReleaseDistribution),
            "maintenance:release-policy" => Some(MaintenanceOperation::ReleasePolicy),
            "maintenance:upgrade-execution" => Some(MaintenanceOperation::UpgradeExecution),
            _ => None,
        }
    }

    /// Iterator over all maintenance operations.
    pub fn all() -> impl Iterator<Item = MaintenanceOperation> {
        [
            MaintenanceOperation::SnapshotProposed,
            MaintenanceOperation::SnapshotCompleted,
            MaintenanceOperation::CacheInvalidated,
            MaintenanceOperation::UpgradeActivated,
            MaintenanceOperation::AdminReplacement,
            MaintenanceOperation::ReleaseDistribution,
            MaintenanceOperation::ReleasePolicy,
            MaintenanceOperation::UpgradeExecution,
        ]
        .into_iter()
    }
}

impl std::fmt::Display for MaintenanceOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_operation_category_exhaustive() {
        // This test ensures all operations have a category
        for op in MaintenanceOperation::all() {
            let _ = op.category(); // Would fail to compile if not exhaustive
        }
    }

    #[test]
    fn test_operation_categories() {
        assert_eq!(
            MaintenanceOperation::SnapshotProposed.category(),
            OperationCategory::B
        );
        assert_eq!(
            MaintenanceOperation::SnapshotCompleted.category(),
            OperationCategory::B
        );
        assert_eq!(
            MaintenanceOperation::CacheInvalidated.category(),
            OperationCategory::A
        );
        assert_eq!(
            MaintenanceOperation::UpgradeActivated.category(),
            OperationCategory::C
        );
        assert_eq!(
            MaintenanceOperation::AdminReplacement.category(),
            OperationCategory::C
        );
        assert_eq!(
            MaintenanceOperation::ReleaseDistribution.category(),
            OperationCategory::B
        );
        assert_eq!(
            MaintenanceOperation::ReleasePolicy.category(),
            OperationCategory::C
        );
        assert_eq!(
            MaintenanceOperation::UpgradeExecution.category(),
            OperationCategory::C
        );
    }

    #[test]
    fn test_operation_roundtrip() {
        for op in MaintenanceOperation::all() {
            let s = op.as_str();
            let parsed = MaintenanceOperation::from_str(s);
            assert_eq!(parsed, Some(op), "Roundtrip failed for {s}");
        }
    }
}
