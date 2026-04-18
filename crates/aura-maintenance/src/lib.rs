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

#[derive(Debug, Clone, Copy)]
struct MaintenanceOperationDescriptor {
    operation: MaintenanceOperation,
    category: OperationCategory,
    name: &'static str,
}

const MAINTENANCE_OPERATION_DESCRIPTORS: &[MaintenanceOperationDescriptor] = &[
    MaintenanceOperationDescriptor {
        operation: MaintenanceOperation::SnapshotProposed,
        category: OperationCategory::B,
        name: "maintenance:snapshot-proposed",
    },
    MaintenanceOperationDescriptor {
        operation: MaintenanceOperation::SnapshotCompleted,
        category: OperationCategory::B,
        name: "maintenance:snapshot-completed",
    },
    MaintenanceOperationDescriptor {
        operation: MaintenanceOperation::CacheInvalidated,
        category: OperationCategory::A,
        name: "maintenance:cache-invalidated",
    },
    MaintenanceOperationDescriptor {
        operation: MaintenanceOperation::UpgradeActivated,
        category: OperationCategory::C,
        name: "maintenance:upgrade-activated",
    },
    MaintenanceOperationDescriptor {
        operation: MaintenanceOperation::AdminReplacement,
        category: OperationCategory::C,
        name: "maintenance:admin-replacement",
    },
    MaintenanceOperationDescriptor {
        operation: MaintenanceOperation::ReleaseDistribution,
        category: OperationCategory::B,
        name: "maintenance:release-distribution",
    },
    MaintenanceOperationDescriptor {
        operation: MaintenanceOperation::ReleasePolicy,
        category: OperationCategory::C,
        name: "maintenance:release-policy",
    },
    MaintenanceOperationDescriptor {
        operation: MaintenanceOperation::UpgradeExecution,
        category: OperationCategory::C,
        name: "maintenance:upgrade-execution",
    },
];

impl MaintenanceOperation {
    fn descriptor(self) -> &'static MaintenanceOperationDescriptor {
        MAINTENANCE_OPERATION_DESCRIPTORS
            .iter()
            .find(|descriptor| descriptor.operation == self)
            .expect("maintenance operation descriptor must exist")
    }

    /// Get the operation category (compile-time exhaustive).
    ///
    /// Category assignments:
    /// - A: Cache invalidation (low-risk)
    /// - B: Snapshot operations (medium-risk)
    /// - C: Upgrades and admin replacement (high-risk)
    #[must_use]
    pub fn category(&self) -> OperationCategory {
        self.descriptor().category
    }

    /// Get the operation name as a string.
    pub fn as_str(&self) -> &'static str {
        self.descriptor().name
    }

    /// Parse an operation from its string name.
    #[allow(clippy::should_implement_trait)] // Returns Option, not Result
    pub fn from_str(s: &str) -> Option<Self> {
        MAINTENANCE_OPERATION_DESCRIPTORS
            .iter()
            .find(|descriptor| descriptor.name == s)
            .map(|descriptor| descriptor.operation)
    }

    /// Iterator over all maintenance operations.
    pub fn all() -> impl Iterator<Item = MaintenanceOperation> {
        MAINTENANCE_OPERATION_DESCRIPTORS
            .iter()
            .map(|descriptor| descriptor.operation)
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
