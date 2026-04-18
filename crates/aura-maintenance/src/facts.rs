//! Maintenance fact types and reducers.
//!
//! # Architecture
//!
//! Layer 2 domain facts following the `aura-core` pattern.
//! Uses `FactTypeId` and `try_encode`/`try_decode` APIs.

use aura_core::hash::hash;
use aura_core::time::ProvenancedTime;
use aura_core::types::facts::{FactDelta, FactDeltaReducer, FactError};
use aura_core::types::Epoch;
use aura_core::{AccountId, AuthorityId, ContextId, Hash32, SemanticVersion, TimeStamp};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

use crate::{
    AuraActivationScope, AuraPolicyScope, AuraReleaseId, AuraReleaseSeriesId, ReleaseResidency,
    TransitionState,
};

aura_core::define_fact_type_id!(maintenance, "maintenance", 1);

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

/// Release distribution and certification facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum ReleaseDistributionFact {
    /// A release series has been declared.
    SeriesDeclared {
        /// Authority publishing the declaration.
        authority_id: AuthorityId,
        /// Declared release series.
        series_id: AuraReleaseSeriesId,
        /// Human-readable series name.
        name: String,
        /// Declaration time carried in the fact.
        declared_at: TimeStamp,
    },
    /// A specific release manifest has been declared.
    ReleaseDeclared {
        /// Authority publishing the declaration.
        authority_id: AuthorityId,
        /// Release series containing this release.
        series_id: AuraReleaseSeriesId,
        /// Declared release identifier.
        release_id: AuraReleaseId,
        /// Content hash of the signed manifest.
        manifest_hash: Hash32,
        /// Semantic version carried by the release.
        version: SemanticVersion,
        /// Declaration time carried in the fact.
        declared_at: TimeStamp,
    },
    /// A deterministic build certificate has been published.
    BuildCertified {
        /// Builder authority publishing the certificate.
        authority_id: AuthorityId,
        /// Release series containing this release.
        series_id: AuraReleaseSeriesId,
        /// Certified release identifier.
        release_id: AuraReleaseId,
        /// Content hash of the build certificate.
        certificate_hash: Hash32,
        /// Output hash attested by the certificate.
        output_hash: Hash32,
        /// Certification time carried in the fact.
        certified_at: TimeStamp,
    },
    /// A release artifact has become available for replication.
    ArtifactAvailable {
        /// Authority pinning or serving the artifact.
        authority_id: AuthorityId,
        /// Release to which the artifact belongs.
        release_id: AuraReleaseId,
        /// Content hash of the available artifact.
        artifact_hash: Hash32,
        /// Availability publication time.
        published_at: TimeStamp,
    },
    /// An upgrade offer has been published into distribution scope.
    UpgradeOfferPublished {
        /// Authority publishing the offer.
        authority_id: AuthorityId,
        /// Release being offered.
        release_id: AuraReleaseId,
        /// Policy or offer descriptor hash.
        policy_hash: Hash32,
        /// Publication time carried in the fact.
        published_at: TimeStamp,
    },
}

impl ReleaseDistributionFact {
    /// Authority associated with this distribution fact.
    pub fn authority_id(&self) -> AuthorityId {
        match self {
            Self::SeriesDeclared { authority_id, .. }
            | Self::ReleaseDeclared { authority_id, .. }
            | Self::BuildCertified { authority_id, .. }
            | Self::ArtifactAvailable { authority_id, .. }
            | Self::UpgradeOfferPublished { authority_id, .. } => *authority_id,
        }
    }
}

/// OTA discovery, sharing, and activation policy facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum ReleasePolicyFact {
    /// Discovery policy was published for a scope.
    DiscoveryPolicyPublished {
        /// Authority publishing the policy.
        authority_id: AuthorityId,
        /// Scope governed by the policy.
        scope: AuraPolicyScope,
        /// Canonical policy hash.
        policy_hash: Hash32,
        /// Publication time carried in the fact.
        published_at: TimeStamp,
    },
    /// Sharing policy was published for a scope.
    SharingPolicyPublished {
        /// Authority publishing the policy.
        authority_id: AuthorityId,
        /// Scope governed by the policy.
        scope: AuraPolicyScope,
        /// Canonical policy hash.
        policy_hash: Hash32,
        /// Publication time carried in the fact.
        published_at: TimeStamp,
    },
    /// Activation policy was published for a scope.
    ActivationPolicyPublished {
        /// Authority publishing the policy.
        authority_id: AuthorityId,
        /// Scope governed by the policy.
        scope: AuraPolicyScope,
        /// Canonical policy hash.
        policy_hash: Hash32,
        /// Publication time carried in the fact.
        published_at: TimeStamp,
    },
    /// A release recommendation was published for a scope.
    RecommendationPublished {
        /// Authority publishing the recommendation.
        authority_id: AuthorityId,
        /// Release being recommended.
        release_id: AuraReleaseId,
        /// Scope receiving the recommendation.
        scope: AuraPolicyScope,
        /// Publication time carried in the fact.
        published_at: TimeStamp,
    },
}

impl ReleasePolicyFact {
    /// Authority associated with this policy fact.
    pub fn authority_id(&self) -> AuthorityId {
        match self {
            Self::DiscoveryPolicyPublished { authority_id, .. }
            | Self::SharingPolicyPublished { authority_id, .. }
            | Self::ActivationPolicyPublished { authority_id, .. }
            | Self::RecommendationPublished { authority_id, .. } => *authority_id,
        }
    }
}

/// Structured failure class for scoped OTA execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuraUpgradeFailureClass {
    /// Post-activation health validation failed.
    HealthGateFailed,
    /// The release was revoked after staging or activation.
    ReleaseRevoked,
    /// Explicit partition handling was required for incompatibility.
    PartitionRequired,
    /// Launcher handoff or activation execution failed.
    LauncherActivationFailed,
    /// An operator or policy explicitly requested rollback.
    ManualRollbackRequested,
}

/// Structured failure payload for rollback and partition execution facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuraUpgradeFailure {
    /// Stable classification for the failure.
    pub class: AuraUpgradeFailureClass,
    /// Human-readable detail for operator audit.
    pub detail: String,
}

impl AuraUpgradeFailure {
    /// Build a structured scoped-upgrade failure.
    pub fn new(class: AuraUpgradeFailureClass, detail: impl Into<String>) -> Self {
        Self {
            class,
            detail: detail.into(),
        }
    }
}

/// Scoped OTA execution and outcome facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum UpgradeExecutionFact {
    /// A release was staged for a scope.
    ReleaseStaged {
        /// Authority recording the event.
        authority_id: AuthorityId,
        /// Scope entering staged state.
        scope: AuraActivationScope,
        /// Prior release in the scope.
        from_release_id: AuraReleaseId,
        /// Target release in the scope.
        to_release_id: AuraReleaseId,
        /// Staging time carried in the fact.
        staged_at: TimeStamp,
    },
    /// A scope explicitly entered execution tracking for a release.
    ScopeEntered {
        /// Authority recording the event.
        authority_id: AuthorityId,
        /// Scope entering upgrade tracking.
        scope: AuraActivationScope,
        /// Release entering the scope state machine.
        release_id: AuraReleaseId,
        /// Entry time carried in the fact.
        entered_at: TimeStamp,
    },
    /// The release residency changed inside a scope.
    ReleaseResidencyChanged {
        /// Authority recording the event.
        authority_id: AuthorityId,
        /// Scope whose residency changed.
        scope: AuraActivationScope,
        /// Release referenced by the change.
        release_id: AuraReleaseId,
        /// New residency value.
        residency: ReleaseResidency,
        /// Transition time carried in the fact.
        entered_at: TimeStamp,
    },
    /// The transition state changed inside a scope.
    ReleaseTransitionChanged {
        /// Authority recording the event.
        authority_id: AuthorityId,
        /// Scope whose transition state changed.
        scope: AuraActivationScope,
        /// Release referenced by the change.
        release_id: AuraReleaseId,
        /// New transition state.
        transition: TransitionState,
        /// Transition time carried in the fact.
        entered_at: TimeStamp,
    },
    /// A scoped cutover was approved.
    CutoverApproved {
        /// Authority recording the approval.
        authority_id: AuthorityId,
        /// Scope whose cutover was approved.
        scope: AuraActivationScope,
        /// Prior release in the scope.
        from_release_id: AuraReleaseId,
        /// Target release in the scope.
        to_release_id: AuraReleaseId,
        /// Approval time carried in the fact.
        approved_at: TimeStamp,
    },
    /// A scoped cutover completed.
    CutoverCompleted {
        /// Authority recording the completion.
        authority_id: AuthorityId,
        /// Scope whose cutover completed.
        scope: AuraActivationScope,
        /// Activated target release.
        to_release_id: AuraReleaseId,
        /// Completion time carried in the fact.
        completed_at: TimeStamp,
    },
    /// A scoped rollback executed.
    RollbackExecuted {
        /// Authority recording the rollback.
        authority_id: AuthorityId,
        /// Scope whose rollback executed.
        scope: AuraActivationScope,
        /// Release being rolled back from.
        from_release_id: AuraReleaseId,
        /// Release being restored.
        to_release_id: AuraReleaseId,
        /// Structured failure that caused rollback.
        failure: AuraUpgradeFailure,
        /// Rollback time carried in the fact.
        rolled_back_at: TimeStamp,
    },
    /// A mixed-version partition or incompatibility outcome was observed.
    PartitionObserved {
        /// Authority recording the observation.
        authority_id: AuthorityId,
        /// Scope in which the partition was observed.
        scope: AuraActivationScope,
        /// Release associated with the partition.
        release_id: AuraReleaseId,
        /// Structured failure that caused the partition observation.
        failure: AuraUpgradeFailure,
        /// Observation time carried in the fact.
        observed_at: TimeStamp,
    },
}

impl UpgradeExecutionFact {
    /// Authority associated with this execution fact.
    pub fn authority_id(&self) -> AuthorityId {
        match self {
            Self::ReleaseStaged { authority_id, .. }
            | Self::ScopeEntered { authority_id, .. }
            | Self::ReleaseResidencyChanged { authority_id, .. }
            | Self::ReleaseTransitionChanged { authority_id, .. }
            | Self::CutoverApproved { authority_id, .. }
            | Self::CutoverCompleted { authority_id, .. }
            | Self::RollbackExecuted { authority_id, .. }
            | Self::PartitionObserved { authority_id, .. } => *authority_id,
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub enum MaintenanceFact {
    /// Snapshot proposal fact.
    SnapshotProposed(SnapshotProposed),
    /// Snapshot completion fact.
    SnapshotCompleted(SnapshotCompleted),
    /// Cache invalidation fact.
    CacheInvalidated(CacheInvalidated),
    /// Upgrade activation fact.
    UpgradeActivated(UpgradeActivated),
    /// Release distribution and certification fact.
    ReleaseDistribution(ReleaseDistributionFact),
    /// OTA policy publication fact.
    ReleasePolicy(ReleasePolicyFact),
    /// Scoped OTA execution fact.
    UpgradeExecution(UpgradeExecutionFact),
    /// Admin replacement fact.
    AdminReplacement(AdminReplacement),
}

impl MaintenanceFact {
    fn descriptor(&self) -> (&'static str, crate::MaintenanceOperation) {
        match self {
            MaintenanceFact::SnapshotProposed(_) => (
                "snapshot-proposed",
                crate::MaintenanceOperation::SnapshotProposed,
            ),
            MaintenanceFact::SnapshotCompleted(_) => (
                "snapshot-completed",
                crate::MaintenanceOperation::SnapshotCompleted,
            ),
            MaintenanceFact::CacheInvalidated(_) => (
                "cache-invalidated",
                crate::MaintenanceOperation::CacheInvalidated,
            ),
            MaintenanceFact::UpgradeActivated(_) => (
                "upgrade-activated",
                crate::MaintenanceOperation::UpgradeActivated,
            ),
            MaintenanceFact::ReleaseDistribution(_) => (
                "release-distribution",
                crate::MaintenanceOperation::ReleaseDistribution,
            ),
            MaintenanceFact::ReleasePolicy(_) => {
                ("release-policy", crate::MaintenanceOperation::ReleasePolicy)
            }
            MaintenanceFact::UpgradeExecution(_) => (
                "upgrade-execution",
                crate::MaintenanceOperation::UpgradeExecution,
            ),
            MaintenanceFact::AdminReplacement(_) => (
                "admin-replacement",
                crate::MaintenanceOperation::AdminReplacement,
            ),
        }
    }

    fn binding_key_data<T: Serialize>(value: &T) -> Vec<u8> {
        aura_core::util::serialization::to_vec(value).unwrap_or_default()
    }

    fn scope_release_binding_key<Scope: Serialize>(
        scope: &Scope,
        release_id: &AuraReleaseId,
    ) -> Vec<u8> {
        Self::binding_key_data(&(scope, release_id))
    }

    fn authority_summary(prefix: &str, authority_id: AuthorityId) -> String {
        format!("{prefix}:{authority_id}")
    }

    fn authority_display_summary<T: std::fmt::Display>(
        prefix: &str,
        authority_id: AuthorityId,
        value: T,
    ) -> String {
        format!("{prefix}:{authority_id}:{value}")
    }

    fn authority_debug_summary<T: std::fmt::Debug>(
        prefix: &str,
        authority_id: AuthorityId,
        value: &T,
    ) -> String {
        format!("{prefix}:{authority_id}:{value:?}")
    }

    /// Authority associated with this fact.
    pub fn authority_id(&self) -> AuthorityId {
        match self {
            MaintenanceFact::SnapshotProposed(fact) => fact.authority_id,
            MaintenanceFact::SnapshotCompleted(fact) => fact.authority_id,
            MaintenanceFact::CacheInvalidated(fact) => fact.authority_id,
            MaintenanceFact::UpgradeActivated(fact) => fact.authority_id,
            MaintenanceFact::ReleaseDistribution(fact) => fact.authority_id(),
            MaintenanceFact::ReleasePolicy(fact) => fact.authority_id(),
            MaintenanceFact::UpgradeExecution(fact) => fact.authority_id(),
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
        self.descriptor().0
    }

    /// Get the typed operation for this fact.
    ///
    /// This enables compile-time exhaustive category lookups via
    /// `fact.operation().category()`.
    pub fn operation(&self) -> crate::MaintenanceOperation {
        self.descriptor().1
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
                Self::binding_key_data(&fact.proposal_id),
            ),
            MaintenanceFact::SnapshotCompleted(fact) => (
                "snapshot-completed",
                Self::binding_key_data(&fact.proposal_id),
            ),
            MaintenanceFact::CacheInvalidated(_) => ("cache-invalidated", Vec::new()),
            MaintenanceFact::UpgradeActivated(fact) => (
                "upgrade-activated",
                Self::binding_key_data(&fact.package_id),
            ),
            MaintenanceFact::ReleaseDistribution(fact) => match fact {
                ReleaseDistributionFact::SeriesDeclared { series_id, .. } => {
                    ("release-distribution", Self::binding_key_data(series_id))
                }
                ReleaseDistributionFact::ReleaseDeclared { release_id, .. }
                | ReleaseDistributionFact::BuildCertified { release_id, .. }
                | ReleaseDistributionFact::ArtifactAvailable { release_id, .. }
                | ReleaseDistributionFact::UpgradeOfferPublished { release_id, .. } => {
                    ("release-distribution", Self::binding_key_data(release_id))
                }
            },
            MaintenanceFact::ReleasePolicy(fact) => match fact {
                ReleasePolicyFact::DiscoveryPolicyPublished { scope, .. }
                | ReleasePolicyFact::SharingPolicyPublished { scope, .. }
                | ReleasePolicyFact::ActivationPolicyPublished { scope, .. } => {
                    ("release-policy", Self::binding_key_data(scope))
                }
                ReleasePolicyFact::RecommendationPublished {
                    scope, release_id, ..
                } => (
                    "release-policy",
                    Self::scope_release_binding_key(scope, release_id),
                ),
            },
            MaintenanceFact::UpgradeExecution(fact) => match fact {
                UpgradeExecutionFact::ReleaseStaged {
                    scope,
                    to_release_id,
                    ..
                }
                | UpgradeExecutionFact::CutoverApproved {
                    scope,
                    to_release_id,
                    ..
                }
                | UpgradeExecutionFact::RollbackExecuted {
                    scope,
                    to_release_id,
                    ..
                } => (
                    "upgrade-execution",
                    Self::scope_release_binding_key(scope, to_release_id),
                ),
                UpgradeExecutionFact::ScopeEntered {
                    scope, release_id, ..
                }
                | UpgradeExecutionFact::ReleaseResidencyChanged {
                    scope, release_id, ..
                }
                | UpgradeExecutionFact::ReleaseTransitionChanged {
                    scope, release_id, ..
                }
                | UpgradeExecutionFact::PartitionObserved {
                    scope, release_id, ..
                } => (
                    "upgrade-execution",
                    Self::scope_release_binding_key(scope, release_id),
                ),
                UpgradeExecutionFact::CutoverCompleted {
                    scope,
                    to_release_id,
                    ..
                } => (
                    "upgrade-execution",
                    Self::scope_release_binding_key(scope, to_release_id),
                ),
            },
            MaintenanceFact::AdminReplacement(fact) => {
                ("admin-replacement", Self::binding_key_data(&fact.new_admin))
            }
        };
        MaintenanceFactKey { sub_type, data }
    }

    /// Encode this fact with a canonical envelope.
    ///
    /// # Errors
    ///
    /// Returns `FactError` if serialization fails.
    pub fn try_encode(&self) -> Result<Vec<u8>, FactError> {
        aura_core::types::facts::try_encode_fact(
            maintenance_fact_type_id(),
            MAINTENANCE_FACT_SCHEMA_VERSION,
            self,
        )
    }

    /// Decode a fact from a canonical envelope.
    ///
    /// # Errors
    ///
    /// Returns `FactError` if deserialization fails or version/type mismatches.
    pub fn try_decode(bytes: &[u8]) -> Result<Self, FactError> {
        aura_core::types::facts::try_decode_fact(
            maintenance_fact_type_id(),
            MAINTENANCE_FACT_SCHEMA_VERSION,
            bytes,
        )
    }

    /// Encode this fact with proper error handling.
    ///
    /// # Errors
    ///
    /// Returns `FactError` if serialization fails.
    pub fn to_bytes(&self) -> Result<Vec<u8>, FactError> {
        self.try_encode()
    }

    /// Decode from raw bytes.
    ///
    /// # Errors
    ///
    /// Returns `FactError` if deserialization fails or type/version mismatches.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FactError> {
        Self::try_decode(bytes)
    }

    /// Create a FactEnvelope for this fact.
    ///
    /// # Errors
    ///
    /// Returns `FactError` if serialization fails.
    pub fn to_envelope(&self) -> Result<aura_core::types::facts::FactEnvelope, FactError> {
        let payload = aura_core::util::serialization::to_vec(self)?;
        Ok(aura_core::types::facts::FactEnvelope {
            type_id: maintenance_fact_type_id().clone(),
            schema_version: MAINTENANCE_FACT_SCHEMA_VERSION,
            encoding: aura_core::types::facts::FactEncoding::DagCbor,
            payload,
        })
    }

    /// Produce a human-readable summary for logs.
    pub fn summary(&self) -> String {
        match self {
            MaintenanceFact::SnapshotProposed(fact) => Self::authority_display_summary(
                "snapshot_proposed",
                fact.authority_id,
                fact.target_epoch,
            ),
            MaintenanceFact::SnapshotCompleted(fact) => Self::authority_display_summary(
                "snapshot_completed",
                fact.authority_id,
                fact.snapshot.epoch,
            ),
            MaintenanceFact::CacheInvalidated(fact) => Self::authority_display_summary(
                "cache_invalidated",
                fact.authority_id,
                fact.epoch_floor,
            ),
            MaintenanceFact::UpgradeActivated(fact) => Self::authority_display_summary(
                "upgrade_activated",
                fact.authority_id,
                fact.activation_fence.epoch,
            ),
            MaintenanceFact::ReleaseDistribution(fact) => match fact {
                ReleaseDistributionFact::SeriesDeclared { authority_id, .. } => {
                    Self::authority_summary("release_series_declared", *authority_id)
                }
                ReleaseDistributionFact::ReleaseDeclared {
                    authority_id,
                    release_id,
                    ..
                } => Self::authority_debug_summary("release_declared", *authority_id, release_id),
                ReleaseDistributionFact::BuildCertified {
                    authority_id,
                    release_id,
                    ..
                } => Self::authority_debug_summary("build_certified", *authority_id, release_id),
                ReleaseDistributionFact::ArtifactAvailable {
                    authority_id,
                    release_id,
                    ..
                } => Self::authority_debug_summary("artifact_available", *authority_id, release_id),
                ReleaseDistributionFact::UpgradeOfferPublished {
                    authority_id,
                    release_id,
                    ..
                } => Self::authority_debug_summary("upgrade_offer", *authority_id, release_id),
            },
            MaintenanceFact::ReleasePolicy(fact) => match fact {
                ReleasePolicyFact::DiscoveryPolicyPublished { authority_id, .. } => {
                    Self::authority_summary("discovery_policy", *authority_id)
                }
                ReleasePolicyFact::SharingPolicyPublished { authority_id, .. } => {
                    Self::authority_summary("sharing_policy", *authority_id)
                }
                ReleasePolicyFact::ActivationPolicyPublished { authority_id, .. } => {
                    Self::authority_summary("activation_policy", *authority_id)
                }
                ReleasePolicyFact::RecommendationPublished {
                    authority_id,
                    release_id,
                    ..
                } => Self::authority_debug_summary(
                    "release_recommendation",
                    *authority_id,
                    release_id,
                ),
            },
            MaintenanceFact::UpgradeExecution(fact) => match fact {
                UpgradeExecutionFact::ReleaseStaged { authority_id, .. } => {
                    Self::authority_summary("release_staged", *authority_id)
                }
                UpgradeExecutionFact::ScopeEntered { authority_id, .. } => {
                    Self::authority_summary("scope_entered", *authority_id)
                }
                UpgradeExecutionFact::ReleaseResidencyChanged { authority_id, .. } => {
                    Self::authority_summary("residency_changed", *authority_id)
                }
                UpgradeExecutionFact::ReleaseTransitionChanged { authority_id, .. } => {
                    Self::authority_summary("transition_changed", *authority_id)
                }
                UpgradeExecutionFact::CutoverApproved { authority_id, .. } => {
                    Self::authority_summary("cutover_approved", *authority_id)
                }
                UpgradeExecutionFact::CutoverCompleted { authority_id, .. } => {
                    Self::authority_summary("cutover_completed", *authority_id)
                }
                UpgradeExecutionFact::RollbackExecuted { authority_id, .. } => {
                    Self::authority_summary("rollback_executed", *authority_id)
                }
                UpgradeExecutionFact::PartitionObserved { authority_id, .. } => {
                    Self::authority_summary("partition_observed", *authority_id)
                }
            },
            MaintenanceFact::AdminReplacement(fact) => Self::authority_display_summary(
                "admin_replacement",
                fact.old_admin,
                fact.activation_epoch,
            ),
        }
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
    /// Count of release distribution and certification facts.
    pub release_distribution_events: u64,
    /// Count of release policy publication facts.
    pub release_policy_events: u64,
    /// Count of scoped OTA execution facts.
    pub upgrade_execution_events: u64,
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
        self.release_distribution_events += other.release_distribution_events;
        self.release_policy_events += other.release_policy_events;
        self.upgrade_execution_events += other.upgrade_execution_events;
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
            MaintenanceFact::ReleaseDistribution(_) => delta.release_distribution_events += 1,
            MaintenanceFact::ReleasePolicy(_) => delta.release_policy_events += 1,
            MaintenanceFact::UpgradeExecution(_) => delta.upgrade_execution_events += 1,
            MaintenanceFact::AdminReplacement(_) => delta.admin_replacements += 1,
        }
        delta
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
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use aura_core::time::PhysicalTime;

    fn authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn hash(seed: u8) -> Hash32 {
        Hash32([seed; 32])
    }

    fn uuid(seed: u128) -> Uuid {
        Uuid::from_bytes(seed.to_be_bytes())
    }

    fn release_id(seed: u8) -> AuraReleaseId {
        AuraReleaseId::new(hash(seed))
    }

    fn ts(ms: u64) -> TimeStamp {
        TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: ms,
            uncertainty: Some(10),
        })
    }

    #[test]
    fn fact_round_trip() {
        let fact = MaintenanceFact::CacheInvalidated(CacheInvalidated::new(
            authority(1),
            vec![CacheKey("key".to_string())],
            Epoch::new(2),
        ));
        let bytes = fact.to_bytes().expect("encoding should succeed");
        let restored = MaintenanceFact::from_bytes(&bytes).expect("decoding should succeed");
        assert_eq!(fact, restored);
    }

    #[test]
    fn ota_fact_round_trip() {
        let fact =
            MaintenanceFact::UpgradeExecution(UpgradeExecutionFact::ReleaseTransitionChanged {
                authority_id: authority(3),
                scope: AuraActivationScope::AuthorityLocal {
                    authority_id: authority(3),
                },
                release_id: release_id(9),
                transition: TransitionState::AwaitingCutover,
                entered_at: ts(42),
            });
        let bytes = fact.to_bytes().expect("encoding should succeed");
        let restored = MaintenanceFact::from_bytes(&bytes).expect("decoding should succeed");
        assert_eq!(fact, restored);
    }

    #[test]
    fn reducer_tracks_counts() {
        let reducer = MaintenanceFactReducer;
        let fact = MaintenanceFact::SnapshotProposed(SnapshotProposed::new(
            authority(2),
            uuid(1),
            Epoch::new(1),
            hash(3),
        ));
        let delta = reducer.apply(&fact);
        assert_eq!(delta.snapshot_proposals, 1);
    }

    #[test]
    fn reducer_tracks_ota_event_counts() {
        let reducer = MaintenanceFactReducer;
        let fact = MaintenanceFact::ReleasePolicy(ReleasePolicyFact::ActivationPolicyPublished {
            authority_id: authority(4),
            scope: AuraPolicyScope::Authority {
                authority_id: authority(4),
            },
            policy_hash: Hash32([7u8; 32]),
            published_at: ts(100),
        });
        let delta = reducer.apply(&fact);
        assert_eq!(delta.release_policy_events, 1);
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
                    release_distribution_events,
                    release_policy_events,
                    upgrade_execution_events,
                    admin_replacements,
                )| {
                    MaintenanceFactDelta {
                        snapshot_proposals,
                        snapshot_completions,
                        cache_invalidations,
                        cache_keys_invalidated,
                        upgrades_activated,
                        release_distribution_events,
                        release_policy_events,
                        upgrade_execution_events,
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
            && a.release_distribution_events == b.release_distribution_events
            && a.release_policy_events == b.release_policy_events
            && a.upgrade_execution_events == b.upgrade_execution_events
            && a.admin_replacements == b.admin_replacements
    }

    proptest! {
        /// Additive merge: merging with self doubles the counters
        /// Note: This is NOT idempotent (a + a = 2a, not a).
        /// Counter-based deltas use additive semantics, not max-semilattice.
        #[test]
        fn merge_additive(a in arb_delta()) {
            let original = a.clone();
            let mut result = a.clone();
            result.merge(&original);
            // Additive deltas: a + a = 2a (not idempotent)
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
