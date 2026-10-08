//! Observed OTA state reduced from maintenance facts (docs/116 §4).
//!
//! The projection is a pure, order-independent reduction: releases,
//! artifacts, certificates and recommendations are set unions, and each
//! scope's upgrade stage is the highest stage its execution facts reach, so
//! any delivery order of the same fact set yields the same view.

use crate::facts::{
    MaintenanceFact, ReleaseDistributionFact, ReleasePolicyFact, UpgradeExecutionFact,
};
use crate::release::{AuraReleaseId, AuraReleaseSeriesId};
use crate::scope::{AuraActivationScope, AuraPolicyScope};
use aura_core::{AuthorityId, Hash32, SemanticVersion};
use std::collections::{BTreeMap, BTreeSet};

/// A declared release and the evidence published for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtaRelease {
    /// Declared release identifier.
    pub release_id: AuraReleaseId,
    /// Series containing the release.
    pub series_id: AuraReleaseSeriesId,
    /// Semantic version carried by the release.
    pub version: SemanticVersion,
    /// Content hash of the signed manifest.
    pub manifest_hash: Hash32,
    /// Authorities that declared the release.
    pub declared_by: BTreeSet<AuthorityId>,
}

/// A release recommendation for a policy scope.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct OtaRecommendation {
    /// Recommended release.
    pub release_id: AuraReleaseId,
    /// Scope receiving the recommendation.
    pub scope: AuraPolicyScope,
    /// Authority publishing it.
    pub authority_id: AuthorityId,
}

/// The furthest stage a scope's upgrade to one target release has reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OtaScopeStage {
    /// Staged for the scope.
    Staged,
    /// Cutover approved by at least one authority.
    CutoverApproved,
    /// Cutover completed; the target release is active in the scope.
    CutoverCompleted,
    /// Rolled back after a failure.
    RolledBack,
}

/// One scope's upgrade toward a target release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtaScopeUpgrade {
    /// Scope being upgraded.
    pub scope: AuraActivationScope,
    /// Target release.
    pub to_release_id: AuraReleaseId,
    /// Release the scope ran before, when recorded.
    pub from_release_id: Option<AuraReleaseId>,
    /// Furthest stage reached.
    pub stage: OtaScopeStage,
    /// Authorities that approved the cutover.
    pub approvals: BTreeSet<AuthorityId>,
    /// Failures recorded by rollbacks (`Class: detail`), deduplicated.
    pub rollbacks: BTreeSet<String>,
}

impl OtaScopeUpgrade {
    /// Record the prior release; when facts disagree the smallest id wins,
    /// so the result does not depend on delivery order.
    fn record_from(&mut self, from_release_id: AuraReleaseId) {
        self.from_release_id = Some(
            self.from_release_id
                .map_or(from_release_id, |current| current.min(from_release_id)),
        );
    }
}

/// Observed OTA state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OtaView {
    /// Declared releases by id.
    pub releases: BTreeMap<AuraReleaseId, OtaRelease>,
    /// Artifacts announced as available, by release (kept apart from the
    /// declaration so either may arrive first).
    pub artifacts: BTreeMap<AuraReleaseId, BTreeSet<Hash32>>,
    /// Deterministic build certificates published, by release.
    pub certificates: BTreeMap<AuraReleaseId, BTreeSet<Hash32>>,
    /// Published recommendations.
    pub recommendations: BTreeSet<OtaRecommendation>,
    /// Scoped upgrades by (scope, target release).
    pub upgrades: BTreeMap<(AuraActivationScope, AuraReleaseId), OtaScopeUpgrade>,
}

impl OtaView {
    /// Reduce a set of maintenance facts.
    pub fn from_facts<'a>(facts: impl IntoIterator<Item = &'a MaintenanceFact>) -> Self {
        let mut view = Self::default();
        for fact in facts {
            view.apply(fact);
        }
        view
    }

    /// Apply one maintenance fact; facts that are not OTA facts are ignored.
    pub fn apply(&mut self, fact: &MaintenanceFact) {
        match fact {
            MaintenanceFact::ReleaseDistribution(fact) => self.apply_distribution(fact),
            MaintenanceFact::ReleasePolicy(ReleasePolicyFact::RecommendationPublished {
                authority_id,
                release_id,
                scope,
                ..
            }) => {
                self.recommendations.insert(OtaRecommendation {
                    release_id: *release_id,
                    scope: scope.clone(),
                    authority_id: *authority_id,
                });
            }
            MaintenanceFact::UpgradeExecution(fact) => self.apply_execution(fact),
            _ => {}
        }
    }

    fn apply_distribution(&mut self, fact: &ReleaseDistributionFact) {
        match fact {
            ReleaseDistributionFact::ReleaseDeclared {
                authority_id,
                series_id,
                release_id,
                manifest_hash,
                version,
                ..
            } => {
                let release = self
                    .releases
                    .entry(*release_id)
                    .or_insert_with(|| OtaRelease {
                        release_id: *release_id,
                        series_id: *series_id,
                        version: *version,
                        manifest_hash: *manifest_hash,
                        declared_by: BTreeSet::new(),
                    });
                release.declared_by.insert(*authority_id);
            }
            ReleaseDistributionFact::ArtifactAvailable {
                release_id,
                artifact_hash,
                ..
            } => {
                self.artifacts
                    .entry(*release_id)
                    .or_default()
                    .insert(*artifact_hash);
            }
            ReleaseDistributionFact::BuildCertified {
                release_id,
                certificate_hash,
                ..
            } => {
                self.certificates
                    .entry(*release_id)
                    .or_default()
                    .insert(*certificate_hash);
            }
            ReleaseDistributionFact::SeriesDeclared { .. }
            | ReleaseDistributionFact::UpgradeOfferPublished { .. } => {}
        }
    }

    fn upgrade(
        &mut self,
        scope: &AuraActivationScope,
        to_release_id: AuraReleaseId,
    ) -> &mut OtaScopeUpgrade {
        self.upgrades
            .entry((scope.clone(), to_release_id))
            .or_insert_with(|| OtaScopeUpgrade {
                scope: scope.clone(),
                to_release_id,
                from_release_id: None,
                stage: OtaScopeStage::Staged,
                approvals: BTreeSet::new(),
                rollbacks: BTreeSet::new(),
            })
    }

    fn apply_execution(&mut self, fact: &UpgradeExecutionFact) {
        match fact {
            UpgradeExecutionFact::ReleaseStaged {
                scope,
                from_release_id,
                to_release_id,
                ..
            } => {
                let upgrade = self.upgrade(scope, *to_release_id);
                upgrade.record_from(*from_release_id);
            }
            UpgradeExecutionFact::CutoverApproved {
                authority_id,
                scope,
                from_release_id,
                to_release_id,
                ..
            } => {
                let upgrade = self.upgrade(scope, *to_release_id);
                upgrade.record_from(*from_release_id);
                upgrade.approvals.insert(*authority_id);
                upgrade.stage = upgrade.stage.max(OtaScopeStage::CutoverApproved);
            }
            UpgradeExecutionFact::CutoverCompleted {
                scope,
                to_release_id,
                ..
            } => {
                let upgrade = self.upgrade(scope, *to_release_id);
                upgrade.stage = upgrade.stage.max(OtaScopeStage::CutoverCompleted);
            }
            UpgradeExecutionFact::RollbackExecuted {
                scope,
                from_release_id,
                failure,
                ..
            } => {
                // The rollback is about the release being rolled back from.
                let upgrade = self.upgrade(scope, *from_release_id);
                upgrade.stage = upgrade.stage.max(OtaScopeStage::RolledBack);
                upgrade
                    .rollbacks
                    .insert(format!("{:?}: {}", failure.class, failure.detail));
            }
            UpgradeExecutionFact::ScopeEntered { .. }
            | UpgradeExecutionFact::ReleaseResidencyChanged { .. }
            | UpgradeExecutionFact::ReleaseTransitionChanged { .. }
            | UpgradeExecutionFact::PartitionObserved { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::{AuraUpgradeFailure, AuraUpgradeFailureClass};
    use aura_core::time::{PhysicalTime, TimeStamp};
    use aura_core::DeviceId;

    fn h(seed: u8) -> Hash32 {
        Hash32::new([seed; 32])
    }

    fn ts(ms: u64) -> TimeStamp {
        TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: ms,
            uncertainty: None,
        })
    }

    fn facts() -> Vec<MaintenanceFact> {
        let author = AuthorityId::new_from_entropy([1; 32]);
        let approver = AuthorityId::new_from_entropy([2; 32]);
        let (old, new) = (AuraReleaseId::new(h(10)), AuraReleaseId::new(h(11)));
        let scope = AuraActivationScope::DeviceLocal {
            device_id: DeviceId::new_from_entropy([3; 32]),
        };
        vec![
            MaintenanceFact::ReleaseDistribution(ReleaseDistributionFact::ReleaseDeclared {
                authority_id: author,
                series_id: AuraReleaseSeriesId::new(h(9)),
                release_id: new,
                manifest_hash: h(12),
                version: SemanticVersion::new(2, 0, 0),
                declared_at: ts(1),
            }),
            MaintenanceFact::ReleaseDistribution(ReleaseDistributionFact::ArtifactAvailable {
                authority_id: author,
                release_id: new,
                artifact_hash: h(13),
                published_at: ts(2),
            }),
            MaintenanceFact::ReleasePolicy(ReleasePolicyFact::RecommendationPublished {
                authority_id: author,
                release_id: new,
                scope: AuraPolicyScope::Authority {
                    authority_id: author,
                },
                published_at: ts(3),
            }),
            MaintenanceFact::UpgradeExecution(UpgradeExecutionFact::ReleaseStaged {
                authority_id: author,
                scope: scope.clone(),
                from_release_id: old,
                to_release_id: new,
                staged_at: ts(4),
            }),
            MaintenanceFact::UpgradeExecution(UpgradeExecutionFact::CutoverApproved {
                authority_id: approver,
                scope: scope.clone(),
                from_release_id: old,
                to_release_id: new,
                approved_at: ts(5),
            }),
            MaintenanceFact::UpgradeExecution(UpgradeExecutionFact::CutoverCompleted {
                authority_id: author,
                scope: scope.clone(),
                to_release_id: new,
                completed_at: ts(6),
            }),
            MaintenanceFact::UpgradeExecution(UpgradeExecutionFact::RollbackExecuted {
                authority_id: author,
                scope,
                from_release_id: new,
                to_release_id: old,
                failure: AuraUpgradeFailure::new(
                    AuraUpgradeFailureClass::HealthGateFailed,
                    "probe",
                ),
                rolled_back_at: ts(7),
            }),
        ]
    }

    /// Every delivery order of the same facts reduces to the same view, and
    /// the rollback is the furthest stage of the scope's upgrade.
    #[test]
    fn reduction_is_order_independent() {
        let forward = facts();
        let expected = OtaView::from_facts(&forward);
        let mut reversed = forward.clone();
        reversed.reverse();
        assert_eq!(OtaView::from_facts(&reversed), expected);
        for rotation in 1..forward.len() {
            let mut rotated = forward.clone();
            rotated.rotate_left(rotation);
            assert_eq!(
                OtaView::from_facts(&rotated),
                expected,
                "rotation {rotation}"
            );
        }

        let release = expected
            .releases
            .get(&AuraReleaseId::new(h(11)))
            .expect("declared release");
        assert_eq!(release.version, SemanticVersion::new(2, 0, 0));
        assert!(expected.artifacts[&release.release_id].contains(&h(13)));
        assert_eq!(expected.recommendations.len(), 1);
        let upgrade = expected.upgrades.values().next().expect("scoped upgrade");
        assert_eq!(upgrade.stage, OtaScopeStage::RolledBack);
        assert_eq!(upgrade.from_release_id, Some(AuraReleaseId::new(h(10))));
        assert_eq!(upgrade.approvals.len(), 1);
        assert_eq!(upgrade.rollbacks.len(), 1);
    }
}
