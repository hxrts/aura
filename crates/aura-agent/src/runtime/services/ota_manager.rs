//! Runtime-owned OTA updater/launcher control plane.

use super::state::with_state_mut_validated;
use aura_core::AuthorityId;
use aura_maintenance::{
    AuraActivationScope, AuraCompatibilityClass, AuraReleaseId, AuraRollbackPreference,
    AuraUpgradeFailure, ReleaseResidency, TransitionState,
};
use aura_sync::services::{
    InFlightIncompatibilityAction, RollbackDirective, ScopedOtaTransitionEngine,
    ScopedUpgradeState, SessionCompatibilityPlan,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use tokio::sync::RwLock;

/// Update status for the agent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum UpdateStatus {
    /// No update available.
    #[default]
    UpToDate,
    /// Update available but not yet downloaded.
    Available {
        version: String,
        release_notes: Option<String>,
        size_bytes: u64,
    },
    /// Update is being downloaded.
    Downloading {
        version: String,
        progress_percent: u8,
    },
    /// Update downloaded and verified, ready to install.
    Ready { version: String },
    /// Update is being installed or rolled back by the launcher.
    Installing { version: String },
    /// Update failed.
    Failed { reason: String },
}

#[allow(dead_code)] // Exercised incrementally as OTA launcher wiring lands.
/// Staged release material managed by the OTA control plane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedRelease {
    /// Release identity for the staged bundle.
    pub release_id: AuraReleaseId,
    /// User-facing version string.
    pub version: String,
    /// Storage key for the staged manifest.
    pub manifest_key: String,
    /// Storage keys for staged artifacts.
    pub artifact_keys: Vec<String>,
    /// Storage keys for staged certificates.
    pub certificate_keys: Vec<String>,
}

#[allow(dead_code)] // Exercised incrementally as OTA launcher wiring lands.
/// Explicit launcher commands emitted by the OTA control plane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LauncherCommand {
    /// Materialize or refresh a staged bundle in the launcher environment.
    Stage(StagedRelease),
    /// Activate a staged target release.
    Activate {
        from_release_id: Option<AuraReleaseId>,
        to_release_id: AuraReleaseId,
    },
    /// Roll back from one release to another.
    Rollback {
        from_release_id: AuraReleaseId,
        to_release_id: AuraReleaseId,
        failure: AuraUpgradeFailure,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActivationIntent {
    from_release_id: Option<AuraReleaseId>,
    to_release_id: AuraReleaseId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RollbackIntent {
    from_release_id: AuraReleaseId,
    to_release_id: AuraReleaseId,
    failure: AuraUpgradeFailure,
}

#[derive(Debug, Default)]
#[allow(dead_code)] // Fields are consumed as launcher integration lands.
struct OtaState {
    status: UpdateStatus,
    staged_releases: BTreeMap<AuraReleaseId, StagedRelease>,
    scoped_states: BTreeMap<AuraActivationScope, ScopedUpgradeState>,
    managed_quorum_approvals: BTreeMap<AuraActivationScope, BTreeSet<AuthorityId>>,
    launcher_queue: VecDeque<LauncherCommand>,
    active_release: Option<AuraReleaseId>,
    pending_activation: Option<ActivationIntent>,
    pending_rollback: Option<RollbackIntent>,
}

impl OtaState {
    fn validate(&self) -> Result<(), super::invariant::InvariantViolation> {
        if let Some(intent) = &self.pending_activation {
            if !self.staged_releases.contains_key(&intent.to_release_id) {
                return Err(super::invariant::InvariantViolation::new(
                    "OtaManager",
                    "pending activation target must be staged",
                ));
            }
        }

        if let Some(intent) = &self.pending_rollback {
            if !self.staged_releases.contains_key(&intent.to_release_id) {
                return Err(super::invariant::InvariantViolation::new(
                    "OtaManager",
                    "pending rollback target must be staged",
                ));
            }
        }

        for scoped_state in self.scoped_states.values() {
            if !self
                .staged_releases
                .contains_key(&scoped_state.target_release_id)
            {
                return Err(super::invariant::InvariantViolation::new(
                    "OtaManager",
                    "scoped upgrade target must be staged",
                ));
            }
        }

        for (scope, approvals) in &self.managed_quorum_approvals {
            let AuraActivationScope::ManagedQuorum { participants, .. } = scope else {
                return Err(super::invariant::InvariantViolation::new(
                    "OtaManager",
                    "managed quorum approvals must only exist for managed quorum scopes",
                ));
            };
            if !approvals
                .iter()
                .all(|authority_id| participants.contains(authority_id))
            {
                return Err(super::invariant::InvariantViolation::new(
                    "OtaManager",
                    "managed quorum approvals must be a subset of scope participants",
                ));
            }
        }

        Ok(())
    }
}

#[aura_macros::actor_owned(
    owner = "ota_manager",
    domain = "ota",
    gate = "launcher_command_ingress",
    command = LauncherCommand,
    capacity = 32,
    category = "actor_owned"
)]
#[derive(Debug, Default)]
pub(crate) struct OtaManager {
    state: RwLock<OtaState>,
}

impl OtaManager {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) async fn status(&self) -> UpdateStatus {
        self.state.read().await.status.clone()
    }

    #[allow(dead_code)] // Used by the upcoming launcher bridge.
    pub(crate) async fn scope_state(
        &self,
        scope: &AuraActivationScope,
    ) -> Option<ScopedUpgradeState> {
        self.state.read().await.scoped_states.get(scope).cloned()
    }

    pub(crate) async fn set_status(&self, status: UpdateStatus) {
        with_state_mut_validated(
            &self.state,
            move |state| {
                state.status = status;
            },
            |state| state.validate(),
        )
        .await;
    }

    #[allow(dead_code)] // Used by the upcoming launcher bridge.
    pub(crate) async fn register_staged_release(&self, release: StagedRelease) {
        let version = release.version.clone();
        with_state_mut_validated(
            &self.state,
            move |state| {
                state
                    .launcher_queue
                    .push_back(LauncherCommand::Stage(release.clone()));
                state.staged_releases.insert(release.release_id, release);
                state.status = UpdateStatus::Ready { version };
            },
            |state| state.validate(),
        )
        .await;
    }

    #[allow(dead_code)] // Used by the upcoming launcher bridge.
    pub(crate) async fn stage_scope_upgrade(
        &self,
        scope: AuraActivationScope,
        release: StagedRelease,
        legacy_release_id: Option<AuraReleaseId>,
        compatibility: AuraCompatibilityClass,
    ) -> Result<ScopedUpgradeState, String> {
        let version = release.version.clone();
        let scope_state = ScopedOtaTransitionEngine::new().stage_scope(
            scope.clone(),
            legacy_release_id,
            release.release_id,
            compatibility,
        );
        let mut guard = self.state.write().await;
        guard
            .launcher_queue
            .push_back(LauncherCommand::Stage(release.clone()));
        guard.staged_releases.insert(release.release_id, release);
        if matches!(scope, AuraActivationScope::ManagedQuorum { .. }) {
            guard
                .managed_quorum_approvals
                .insert(scope.clone(), BTreeSet::new());
        }
        guard.scoped_states.insert(scope, scope_state.clone());
        guard.status = UpdateStatus::Ready { version };
        guard.validate().map_err(|v| v.to_string())?;
        Ok(scope_state)
    }

    #[allow(dead_code)] // Used by managed rollout hardening.
    pub(crate) async fn record_managed_quorum_approval(
        &self,
        scope: &AuraActivationScope,
        authority_id: AuthorityId,
    ) -> Result<usize, String> {
        let AuraActivationScope::ManagedQuorum { participants, .. } = scope else {
            return Err("managed quorum approvals require a managed quorum scope".to_string());
        };
        if !participants.contains(&authority_id) {
            return Err("managed quorum approval must come from a scope participant".to_string());
        }

        let mut guard = self.state.write().await;
        let approvals = guard
            .managed_quorum_approvals
            .entry(scope.clone())
            .or_insert_with(BTreeSet::new);
        approvals.insert(authority_id);
        let approval_count = approvals.len();
        guard.validate().map_err(|v| v.to_string())?;
        Ok(approval_count)
    }

    #[allow(dead_code)] // Used by the upcoming launcher bridge.
    pub(crate) async fn begin_scoped_cutover(
        &self,
        scope: &AuraActivationScope,
        preferred_in_flight: InFlightIncompatibilityAction,
        delegation_supported: bool,
    ) -> Result<SessionCompatibilityPlan, String> {
        let engine = ScopedOtaTransitionEngine::new();
        let mut guard = self.state.write().await;
        let current = guard
            .scoped_states
            .get(scope)
            .cloned()
            .ok_or_else(|| "scope is not staged for OTA cutover".to_string())?;
        if let AuraActivationScope::ManagedQuorum { participants, .. } = scope {
            let approval_count = guard
                .managed_quorum_approvals
                .get(scope)
                .map_or(0, BTreeSet::len);
            if approval_count < participants.len() {
                return Err(
                    "managed quorum cutover requires approval from every participant".to_string(),
                );
            }
        }
        let version = guard
            .staged_releases
            .get(&current.target_release_id)
            .map(|release| release.version.clone())
            .ok_or_else(|| "cutover target must be staged".to_string())?;
        let next = engine.begin_cutover(current, preferred_in_flight, delegation_supported);
        let plan = next.session_plan;
        guard.pending_activation = Some(ActivationIntent {
            from_release_id: next.legacy_release_id,
            to_release_id: next.target_release_id,
        });
        guard.launcher_queue.push_back(LauncherCommand::Activate {
            from_release_id: next.legacy_release_id,
            to_release_id: next.target_release_id,
        });
        guard.scoped_states.insert(scope.clone(), next);
        guard.status = UpdateStatus::Installing { version };
        guard.validate().map_err(|v| v.to_string())?;
        Ok(plan)
    }

    #[allow(dead_code)] // Used by the upcoming launcher bridge.
    pub(crate) async fn complete_scoped_cutover(
        &self,
        scope: &AuraActivationScope,
    ) -> Result<ScopedUpgradeState, String> {
        let engine = ScopedOtaTransitionEngine::new();
        let mut guard = self.state.write().await;
        let current = guard
            .scoped_states
            .get(scope)
            .cloned()
            .ok_or_else(|| "scope is not staged for OTA cutover".to_string())?;
        let next = engine.complete_cutover(current);
        guard.active_release = Some(next.target_release_id);
        guard.pending_activation = None;
        guard.pending_rollback = None;
        guard.scoped_states.insert(scope.clone(), next.clone());
        guard.status = UpdateStatus::UpToDate;
        guard.validate().map_err(|v| v.to_string())?;
        Ok(next)
    }

    #[allow(dead_code)] // Used by the upcoming launcher bridge.
    pub(crate) async fn fail_scoped_cutover(
        &self,
        scope: &AuraActivationScope,
        failure: AuraUpgradeFailure,
    ) -> Result<RollbackDirective, String> {
        let engine = ScopedOtaTransitionEngine::new();
        let mut guard = self.state.write().await;
        let current = guard
            .scoped_states
            .get(scope)
            .cloned()
            .ok_or_else(|| "scope is not staged for OTA cutover".to_string())?;
        let (next, directive) = engine
            .begin_rollback(current, failure)
            .ok_or_else(|| "rollback requires a legacy release".to_string())?;
        let version = guard
            .staged_releases
            .get(&directive.to_release_id)
            .map(|release| release.version.clone())
            .ok_or_else(|| "rollback target must be staged".to_string())?;
        guard.pending_activation = None;
        guard.pending_rollback = Some(RollbackIntent {
            from_release_id: directive.from_release_id,
            to_release_id: directive.to_release_id,
            failure: directive.failure.clone(),
        });
        guard.launcher_queue.push_back(LauncherCommand::Rollback {
            from_release_id: directive.from_release_id,
            to_release_id: directive.to_release_id,
            failure: directive.failure.clone(),
        });
        guard.scoped_states.insert(scope.clone(), next);
        guard.status = UpdateStatus::Installing { version };
        guard.validate().map_err(|v| v.to_string())?;
        Ok(directive)
    }

    #[allow(dead_code)] // Used by rollout policy integration.
    pub(crate) async fn handle_scoped_cutover_failure(
        &self,
        scope: &AuraActivationScope,
        failure: AuraUpgradeFailure,
        rollback_preference: AuraRollbackPreference,
    ) -> Result<Option<RollbackDirective>, String> {
        match rollback_preference {
            AuraRollbackPreference::Automatic => {
                self.fail_scoped_cutover(scope, failure).await.map(Some)
            }
            AuraRollbackPreference::ManualApproval => {
                let mut guard = self.state.write().await;
                let current = guard
                    .scoped_states
                    .get(scope)
                    .cloned()
                    .ok_or_else(|| "scope is not staged for OTA cutover".to_string())?;
                let rollback_target = current
                    .legacy_release_id
                    .ok_or_else(|| "rollback requires a legacy release".to_string())?;
                guard.pending_activation = None;
                guard.pending_rollback = Some(RollbackIntent {
                    from_release_id: current.target_release_id,
                    to_release_id: rollback_target,
                    failure: failure.clone(),
                });
                guard.status = UpdateStatus::Failed {
                    reason: failure.detail.clone(),
                };
                guard.validate().map_err(|v| v.to_string())?;
                Ok(None)
            }
        }
    }

    #[allow(dead_code)] // Used by revocation handling.
    pub(crate) async fn apply_scope_revocation(
        &self,
        scope: &AuraActivationScope,
        revoked_release_id: AuraReleaseId,
        failure: AuraUpgradeFailure,
        rollback_preference: AuraRollbackPreference,
    ) -> Result<Option<RollbackDirective>, String> {
        {
            let mut guard = self.state.write().await;
            let current = guard
                .scoped_states
                .get(scope)
                .cloned()
                .ok_or_else(|| "scope is not staged for OTA handling".to_string())?;
            if current.target_release_id != revoked_release_id {
                return Ok(None);
            }

            if current.transition == TransitionState::AwaitingCutover
                && current.residency != ReleaseResidency::TargetOnly
            {
                guard.scoped_states.remove(scope);
                guard.staged_releases.remove(&revoked_release_id);
                if guard
                    .pending_activation
                    .as_ref()
                    .is_some_and(|intent| intent.to_release_id == revoked_release_id)
                {
                    guard.pending_activation = None;
                }
                if guard
                    .pending_rollback
                    .as_ref()
                    .is_some_and(|intent| intent.from_release_id == revoked_release_id)
                {
                    guard.pending_rollback = None;
                }
                guard.status = UpdateStatus::Failed {
                    reason: failure.detail.clone(),
                };
                guard.validate().map_err(|v| v.to_string())?;
                return Ok(None);
            }
        }

        self.handle_scoped_cutover_failure(scope, failure, rollback_preference)
            .await
    }

    #[allow(dead_code)] // Used by the upcoming launcher bridge.
    pub(crate) async fn complete_scoped_rollback(
        &self,
        scope: &AuraActivationScope,
    ) -> Result<ScopedUpgradeState, String> {
        let engine = ScopedOtaTransitionEngine::new();
        let mut guard = self.state.write().await;
        let current = guard
            .scoped_states
            .get(scope)
            .cloned()
            .ok_or_else(|| "scope is not rolling back".to_string())?;
        let active_release = current
            .legacy_release_id
            .ok_or_else(|| "rollback requires a legacy release".to_string())?;
        let next = engine.complete_rollback(current);
        guard.active_release = Some(active_release);
        guard.pending_activation = None;
        guard.pending_rollback = None;
        guard.scoped_states.insert(scope.clone(), next.clone());
        guard.status = UpdateStatus::UpToDate;
        guard.validate().map_err(|v| v.to_string())?;
        Ok(next)
    }

    #[allow(dead_code)] // Used by the upcoming launcher bridge.
    pub(crate) async fn queue_activation(
        &self,
        from_release_id: Option<AuraReleaseId>,
        to_release_id: AuraReleaseId,
    ) -> Result<(), String> {
        let mut guard = self.state.write().await;
        let version = guard
            .staged_releases
            .get(&to_release_id)
            .map(|release| release.version.clone())
            .ok_or_else(|| "activation target must be staged first".to_string())?;

        guard.pending_activation = Some(ActivationIntent {
            from_release_id,
            to_release_id,
        });
        guard.launcher_queue.push_back(LauncherCommand::Activate {
            from_release_id,
            to_release_id,
        });
        guard.status = UpdateStatus::Installing { version };
        guard.validate().map_err(|v| v.to_string())
    }

    #[allow(dead_code)] // Used by the upcoming launcher bridge.
    pub(crate) async fn complete_activation(&self, active_release: AuraReleaseId) {
        with_state_mut_validated(
            &self.state,
            move |state| {
                state.active_release = Some(active_release);
                state.pending_activation = None;
                state.pending_rollback = None;
                state.status = UpdateStatus::UpToDate;
            },
            |state| state.validate(),
        )
        .await;
    }

    #[allow(dead_code)] // Used by the upcoming launcher bridge.
    pub(crate) async fn queue_rollback(
        &self,
        from_release_id: AuraReleaseId,
        to_release_id: AuraReleaseId,
        failure: AuraUpgradeFailure,
    ) -> Result<(), String> {
        let mut guard = self.state.write().await;
        let version = guard
            .staged_releases
            .get(&to_release_id)
            .map(|release| release.version.clone())
            .ok_or_else(|| "rollback target must be staged first".to_string())?;

        guard.pending_rollback = Some(RollbackIntent {
            from_release_id,
            to_release_id,
            failure: failure.clone(),
        });
        guard.launcher_queue.push_back(LauncherCommand::Rollback {
            from_release_id,
            to_release_id,
            failure,
        });
        guard.status = UpdateStatus::Installing { version };
        guard.validate().map_err(|v| v.to_string())
    }

    #[allow(dead_code)] // Used by the upcoming launcher bridge.
    pub(crate) async fn complete_rollback(&self, active_release: AuraReleaseId) {
        with_state_mut_validated(
            &self.state,
            move |state| {
                state.active_release = Some(active_release);
                state.pending_activation = None;
                state.pending_rollback = None;
                state.status = UpdateStatus::UpToDate;
            },
            |state| state.validate(),
        )
        .await;
    }

    #[allow(dead_code)] // Used by the upcoming launcher bridge.
    pub(crate) async fn next_launcher_command(&self) -> Option<LauncherCommand> {
        self.state.write().await.launcher_queue.pop_front()
    }

    #[allow(dead_code)] // Used by the upcoming launcher bridge.
    pub(crate) async fn active_release(&self) -> Option<AuraReleaseId> {
        self.state.read().await.active_release
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{AuthorityId, ContextId, DeviceId, Hash32};
    use aura_maintenance::{
        AuraActivationScope, AuraCompatibilityClass, AuraReleaseId, AuraRollbackPreference,
        AuraUpgradeFailure, AuraUpgradeFailureClass, ReleaseResidency, TransitionState,
    };
    use aura_sync::services::{InFlightIncompatibilityAction, NewSessionAdmission};
    use serde::Serialize;
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::PathBuf;

    fn release(byte: u8, version: &str) -> StagedRelease {
        StagedRelease {
            release_id: AuraReleaseId::new(Hash32([byte; 32])),
            version: version.to_string(),
            manifest_key: format!("ota/releases/{byte}/manifest.cbor"),
            artifact_keys: vec![format!("ota/releases/{byte}/artifacts.bin")],
            certificate_keys: vec![format!("ota/releases/{byte}/certificate.cbor")],
        }
    }

    fn scope() -> AuraActivationScope {
        AuraActivationScope::AuthorityLocal {
            authority_id: AuthorityId::new_from_entropy([9; 32]),
        }
    }

    fn device_scope() -> AuraActivationScope {
        AuraActivationScope::DeviceLocal {
            device_id: DeviceId::new_from_entropy([5; 32]),
        }
    }

    fn managed_quorum_scope() -> AuraActivationScope {
        AuraActivationScope::ManagedQuorum {
            context_id: ContextId::new_from_entropy([7; 32]),
            participants: BTreeSet::from([
                AuthorityId::new_from_entropy([11; 32]),
                AuthorityId::new_from_entropy([12; 32]),
                AuthorityId::new_from_entropy([13; 32]),
            ]),
        }
    }

    #[derive(Debug, Clone, Serialize)]
    struct ScopedScenarioArtifact {
        scenario: &'static str,
        scope: String,
        residency: String,
        transition: String,
        partition_required: bool,
        active_release: Option<String>,
    }

    fn maybe_write_scoped_ota_artifact(rows: &[ScopedScenarioArtifact]) {
        let Ok(path) = std::env::var("AURA_SCOPED_OTA_ARTIFACT") else {
            return;
        };
        let path = PathBuf::from(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create scoped ota artifact directory");
        }
        let payload = serde_json::json!({
            "schema_version": "aura.scoped-ota-transitions.v1",
            "rows": rows,
        });
        fs::write(
            path,
            serde_json::to_vec_pretty(&payload).expect("serialize scoped ota artifact"),
        )
        .expect("write scoped ota artifact");
    }

    #[tokio::test]
    async fn staged_release_queues_launcher_stage_command() {
        let manager = OtaManager::new();
        let staged = release(1, "1.2.3");
        manager.register_staged_release(staged.clone()).await;

        assert_eq!(
            manager.status().await,
            UpdateStatus::Ready {
                version: "1.2.3".to_string()
            }
        );
        assert_eq!(
            manager.next_launcher_command().await,
            Some(LauncherCommand::Stage(staged))
        );
    }

    #[tokio::test]
    async fn activation_and_rollback_are_explicit_launcher_commands() {
        let manager = OtaManager::new();
        let current = release(1, "1.0.0");
        let target = release(2, "2.0.0");
        manager.register_staged_release(current.clone()).await;
        let _ = manager.next_launcher_command().await;
        manager.register_staged_release(target.clone()).await;
        let _ = manager.next_launcher_command().await;

        manager
            .queue_activation(Some(current.release_id), target.release_id)
            .await
            .unwrap();
        assert_eq!(
            manager.next_launcher_command().await,
            Some(LauncherCommand::Activate {
                from_release_id: Some(current.release_id),
                to_release_id: target.release_id,
            })
        );

        manager.complete_activation(target.release_id).await;
        assert_eq!(manager.active_release().await, Some(target.release_id));

        manager
            .queue_rollback(
                target.release_id,
                current.release_id,
                AuraUpgradeFailure::new(
                    AuraUpgradeFailureClass::HealthGateFailed,
                    "health gate failed",
                ),
            )
            .await
            .unwrap();
        assert_eq!(
            manager.next_launcher_command().await,
            Some(LauncherCommand::Rollback {
                from_release_id: target.release_id,
                to_release_id: current.release_id,
                failure: AuraUpgradeFailure::new(
                    AuraUpgradeFailureClass::HealthGateFailed,
                    "health gate failed",
                ),
            })
        );
    }

    #[tokio::test]
    async fn manual_rollback_preference_holds_launcher_rollback() {
        let manager = OtaManager::new();
        let current = release(16, "16.0.0");
        manager.register_staged_release(current.clone()).await;
        let _ = manager.next_launcher_command().await;
        manager
            .stage_scope_upgrade(
                scope(),
                release(17, "17.0.0"),
                Some(current.release_id),
                AuraCompatibilityClass::ScopedHardFork,
            )
            .await
            .unwrap();
        let _ = manager.next_launcher_command().await;
        manager
            .begin_scoped_cutover(&scope(), InFlightIncompatibilityAction::Abort, false)
            .await
            .unwrap();
        let _ = manager.next_launcher_command().await;

        let directive = manager
            .handle_scoped_cutover_failure(
                &scope(),
                AuraUpgradeFailure::new(
                    AuraUpgradeFailureClass::HealthGateFailed,
                    "manual rollback requested after health gate failure",
                ),
                AuraRollbackPreference::ManualApproval,
            )
            .await
            .unwrap();
        assert_eq!(directive, None);
        assert_eq!(manager.next_launcher_command().await, None);
        assert_eq!(
            manager.status().await,
            UpdateStatus::Failed {
                reason: "manual rollback requested after health gate failure".to_string(),
            }
        );
    }

    #[tokio::test]
    async fn staged_revocation_cancels_scope_before_cutover() {
        let manager = OtaManager::new();
        let target = release(18, "18.0.0");
        manager
            .stage_scope_upgrade(
                scope(),
                target.clone(),
                Some(release(1, "1.0.0").release_id),
                AuraCompatibilityClass::BackwardCompatible,
            )
            .await
            .unwrap();
        let _ = manager.next_launcher_command().await;

        let directive = manager
            .apply_scope_revocation(
                &scope(),
                target.release_id,
                AuraUpgradeFailure::new(
                    AuraUpgradeFailureClass::ReleaseRevoked,
                    "staged release revoked",
                ),
                AuraRollbackPreference::Automatic,
            )
            .await
            .unwrap();
        assert_eq!(directive, None);
        assert_eq!(manager.scope_state(&scope()).await, None);
        assert_eq!(
            manager.status().await,
            UpdateStatus::Failed {
                reason: "staged release revoked".to_string(),
            }
        );
        assert_eq!(manager.next_launcher_command().await, None);
    }

    #[tokio::test]
    async fn scoped_soft_fork_cutover_keeps_coexistence_behavior() {
        let manager = OtaManager::new();
        let staged = manager
            .stage_scope_upgrade(
                scope(),
                release(3, "3.0.0"),
                Some(release(1, "1.0.0").release_id),
                AuraCompatibilityClass::MixedCoexistenceAllowed,
            )
            .await
            .unwrap();
        let _ = manager.next_launcher_command().await;
        assert_eq!(staged.residency, ReleaseResidency::Coexisting);

        let plan = manager
            .begin_scoped_cutover(&scope(), InFlightIncompatibilityAction::Drain, false)
            .await
            .unwrap();
        assert_eq!(plan.new_sessions, NewSessionAdmission::Allow);

        let completed = manager.complete_scoped_cutover(&scope()).await.unwrap();
        assert_eq!(completed.residency, ReleaseResidency::TargetOnly);
        assert_eq!(completed.transition, TransitionState::Idle);
    }

    #[tokio::test]
    async fn device_local_scope_can_activate_staged_release() {
        let manager = OtaManager::new();
        let device_scope = device_scope();
        let staged = manager
            .stage_scope_upgrade(
                device_scope.clone(),
                release(6, "6.0.0"),
                Some(release(1, "1.0.0").release_id),
                AuraCompatibilityClass::BackwardCompatible,
            )
            .await
            .unwrap();
        let _ = manager.next_launcher_command().await;
        assert_eq!(staged.residency, ReleaseResidency::Coexisting);

        let plan = manager
            .begin_scoped_cutover(&device_scope, InFlightIncompatibilityAction::Drain, false)
            .await
            .unwrap();
        assert_eq!(plan.new_sessions, NewSessionAdmission::Allow);
        assert!(!plan.partition_required);

        let completed = manager
            .complete_scoped_cutover(&device_scope)
            .await
            .unwrap();
        assert_eq!(completed.residency, ReleaseResidency::TargetOnly);
        assert_eq!(completed.transition, TransitionState::Idle);
    }

    #[tokio::test]
    async fn managed_quorum_scope_enforces_scoped_hard_fork_plan() {
        let manager = OtaManager::new();
        let scope = managed_quorum_scope();
        let current = release(7, "7.0.0");
        manager.register_staged_release(current.clone()).await;
        let _ = manager.next_launcher_command().await;

        manager
            .stage_scope_upgrade(
                scope.clone(),
                release(8, "8.0.0"),
                Some(current.release_id),
                AuraCompatibilityClass::ScopedHardFork,
            )
            .await
            .unwrap();
        let _ = manager.next_launcher_command().await;
        let AuraActivationScope::ManagedQuorum { participants, .. } = &scope else {
            panic!("expected managed quorum scope");
        };
        for participant in participants {
            manager
                .record_managed_quorum_approval(&scope, *participant)
                .await
                .unwrap();
        }

        let plan = manager
            .begin_scoped_cutover(&scope, InFlightIncompatibilityAction::Abort, false)
            .await
            .unwrap();
        assert_eq!(plan.new_sessions, NewSessionAdmission::RejectIncompatible);
        assert_eq!(plan.in_flight, InFlightIncompatibilityAction::Abort);
        assert!(!plan.partition_required);

        let completed = manager.complete_scoped_cutover(&scope).await.unwrap();
        assert_eq!(completed.residency, ReleaseResidency::TargetOnly);
        assert_eq!(completed.transition, TransitionState::Idle);
    }

    #[tokio::test]
    async fn managed_quorum_cutover_requires_member_approval() {
        let manager = OtaManager::new();
        let scope = managed_quorum_scope();
        let current = release(14, "14.0.0");
        manager.register_staged_release(current.clone()).await;
        let _ = manager.next_launcher_command().await;
        manager
            .stage_scope_upgrade(
                scope.clone(),
                release(15, "15.0.0"),
                Some(current.release_id),
                AuraCompatibilityClass::ScopedHardFork,
            )
            .await
            .unwrap();
        let _ = manager.next_launcher_command().await;

        let err = manager
            .begin_scoped_cutover(&scope, InFlightIncompatibilityAction::Abort, false)
            .await
            .unwrap_err();
        assert_eq!(
            err,
            "managed quorum cutover requires approval from every participant"
        );

        let outsider = AuthorityId::new_from_entropy([21; 32]);
        let err = manager
            .record_managed_quorum_approval(&scope, outsider)
            .await
            .unwrap_err();
        assert_eq!(
            err,
            "managed quorum approval must come from a scope participant"
        );
    }

    #[tokio::test]
    async fn scoped_incompatible_cutover_rejects_new_sessions_and_rolls_back() {
        let manager = OtaManager::new();
        let current = release(4, "4.0.0");
        manager.register_staged_release(current.clone()).await;
        let _ = manager.next_launcher_command().await;

        manager
            .stage_scope_upgrade(
                scope(),
                release(5, "5.0.0"),
                Some(current.release_id),
                AuraCompatibilityClass::IncompatibleWithoutPartition,
            )
            .await
            .unwrap();
        let _ = manager.next_launcher_command().await;

        let plan = manager
            .begin_scoped_cutover(&scope(), InFlightIncompatibilityAction::Delegate, true)
            .await
            .unwrap();
        let _ = manager.next_launcher_command().await;
        assert_eq!(plan.new_sessions, NewSessionAdmission::RejectIncompatible);
        assert_eq!(plan.in_flight, InFlightIncompatibilityAction::Delegate);
        assert!(plan.partition_required);

        let rollback = manager
            .fail_scoped_cutover(
                &scope(),
                AuraUpgradeFailure::new(
                    AuraUpgradeFailureClass::HealthGateFailed,
                    "post-cutover health failure",
                ),
            )
            .await
            .unwrap();
        assert_eq!(rollback.to_release_id, current.release_id);
        assert_eq!(
            manager.next_launcher_command().await,
            Some(LauncherCommand::Rollback {
                from_release_id: rollback.from_release_id,
                to_release_id: rollback.to_release_id,
                failure: AuraUpgradeFailure::new(
                    AuraUpgradeFailureClass::HealthGateFailed,
                    "post-cutover health failure",
                ),
            })
        );

        let restored = manager.complete_scoped_rollback(&scope()).await.unwrap();
        assert_eq!(restored.residency, ReleaseResidency::LegacyOnly);
        assert_eq!(restored.transition, TransitionState::Idle);
        assert_eq!(manager.active_release().await, Some(current.release_id));
    }

    #[tokio::test]
    async fn active_revocation_triggers_automatic_rollback() {
        let manager = OtaManager::new();
        let current = release(19, "19.0.0");
        let target = release(20, "20.0.0");
        manager.register_staged_release(current.clone()).await;
        let _ = manager.next_launcher_command().await;
        manager
            .stage_scope_upgrade(
                scope(),
                target.clone(),
                Some(current.release_id),
                AuraCompatibilityClass::ScopedHardFork,
            )
            .await
            .unwrap();
        let _ = manager.next_launcher_command().await;
        manager
            .begin_scoped_cutover(&scope(), InFlightIncompatibilityAction::Abort, false)
            .await
            .unwrap();
        let _ = manager.next_launcher_command().await;
        manager.complete_scoped_cutover(&scope()).await.unwrap();

        let rollback = manager
            .apply_scope_revocation(
                &scope(),
                target.release_id,
                AuraUpgradeFailure::new(
                    AuraUpgradeFailureClass::ReleaseRevoked,
                    "active release revoked",
                ),
                AuraRollbackPreference::Automatic,
            )
            .await
            .unwrap();
        assert_eq!(
            rollback.as_ref().map(|d| d.to_release_id),
            Some(current.release_id)
        );
        assert_eq!(
            manager.next_launcher_command().await,
            Some(LauncherCommand::Rollback {
                from_release_id: target.release_id,
                to_release_id: current.release_id,
                failure: AuraUpgradeFailure::new(
                    AuraUpgradeFailureClass::ReleaseRevoked,
                    "active release revoked",
                ),
            })
        );
    }

    #[tokio::test]
    async fn scoped_ota_runs_emit_deterministic_transition_artifact() {
        let mut rows = Vec::new();

        let device_manager = OtaManager::new();
        let device_scope = device_scope();
        device_manager
            .stage_scope_upgrade(
                device_scope.clone(),
                release(9, "9.0.0"),
                Some(release(2, "2.0.0").release_id),
                AuraCompatibilityClass::BackwardCompatible,
            )
            .await
            .unwrap();
        let _ = device_manager.next_launcher_command().await;
        let device_plan = device_manager
            .begin_scoped_cutover(&device_scope, InFlightIncompatibilityAction::Drain, false)
            .await
            .unwrap();
        let device_state = device_manager
            .complete_scoped_cutover(&device_scope)
            .await
            .unwrap();
        rows.push(ScopedScenarioArtifact {
            scenario: "device_local_activation",
            scope: "device_local".to_string(),
            residency: format!("{:?}", device_state.residency),
            transition: format!("{:?}", device_state.transition),
            partition_required: device_plan.partition_required,
            active_release: device_manager
                .active_release()
                .await
                .map(|id| format!("{id:?}")),
        });

        let authority_manager = OtaManager::new();
        authority_manager
            .stage_scope_upgrade(
                scope(),
                release(10, "10.0.0"),
                Some(release(3, "3.0.0").release_id),
                AuraCompatibilityClass::MixedCoexistenceAllowed,
            )
            .await
            .unwrap();
        let _ = authority_manager.next_launcher_command().await;
        let authority_plan = authority_manager
            .begin_scoped_cutover(&scope(), InFlightIncompatibilityAction::Drain, false)
            .await
            .unwrap();
        let authority_state = authority_manager
            .complete_scoped_cutover(&scope())
            .await
            .unwrap();
        rows.push(ScopedScenarioArtifact {
            scenario: "authority_local_soft_fork",
            scope: "authority_local".to_string(),
            residency: format!("{:?}", authority_state.residency),
            transition: format!("{:?}", authority_state.transition),
            partition_required: authority_plan.partition_required,
            active_release: authority_manager
                .active_release()
                .await
                .map(|id| format!("{id:?}")),
        });

        let quorum_manager = OtaManager::new();
        let quorum_scope = managed_quorum_scope();
        let current = release(11, "11.0.0");
        quorum_manager
            .register_staged_release(current.clone())
            .await;
        let _ = quorum_manager.next_launcher_command().await;
        quorum_manager
            .stage_scope_upgrade(
                quorum_scope.clone(),
                release(12, "12.0.0"),
                Some(current.release_id),
                AuraCompatibilityClass::IncompatibleWithoutPartition,
            )
            .await
            .unwrap();
        let _ = quorum_manager.next_launcher_command().await;
        let AuraActivationScope::ManagedQuorum { participants, .. } = &quorum_scope else {
            panic!("expected managed quorum scope");
        };
        for participant in participants {
            quorum_manager
                .record_managed_quorum_approval(&quorum_scope, *participant)
                .await
                .unwrap();
        }
        let quorum_plan = quorum_manager
            .begin_scoped_cutover(&quorum_scope, InFlightIncompatibilityAction::Delegate, true)
            .await
            .unwrap();
        let rollback = quorum_manager
            .fail_scoped_cutover(
                &quorum_scope,
                AuraUpgradeFailure::new(
                    AuraUpgradeFailureClass::HealthGateFailed,
                    "managed quorum health gate failed",
                ),
            )
            .await
            .unwrap();
        let _ = quorum_manager.next_launcher_command().await;
        let quorum_state = quorum_manager
            .complete_scoped_rollback(&quorum_scope)
            .await
            .unwrap();
        rows.push(ScopedScenarioArtifact {
            scenario: "managed_quorum_failed_cutover_rollback",
            scope: "managed_quorum".to_string(),
            residency: format!("{:?}", quorum_state.residency),
            transition: format!("{:?}", quorum_state.transition),
            partition_required: quorum_plan.partition_required,
            active_release: Some(format!("{:?}", rollback.to_release_id)),
        });

        maybe_write_scoped_ota_artifact(&rows);
    }
}
