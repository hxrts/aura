//! Runtime-backed query and operation access for `AppCore`.

use super::state::{AppCore, APP_RUNTIME_OPERATION_TIMEOUT, APP_RUNTIME_QUERY_TIMEOUT};
use crate::core::{IntentError, StateSnapshot};
use crate::runtime_bridge::{
    BootstrapCandidateInfo, BridgeAuthorityInfo, BridgeDeviceInfo, RuntimeBridge,
    SettingsBridgeState, SyncStatus as RuntimeSyncStatus,
};
use crate::ui_contract::AuthoritativeSemanticFact;
use crate::views::ViewState;
use crate::workflows::runtime::timeout_runtime_call as timeout_runtime_call_bounded;
use crate::ReactiveHandler;
use aura_core::tree::{AttestedOp, TreeOp};
use aura_core::types::identifiers::{AuthorityId, CeremonyId, ChannelId};
use aura_core::types::{Epoch, FrostThreshold};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

impl AppCore {
    pub(super) async fn with_runtime_timeout<T, F, Fut>(
        &self,
        no_runtime_message: &'static str,
        operation: &'static str,
        stage: &'static str,
        duration: Duration,
        call: F,
    ) -> Result<T, IntentError>
    where
        F: FnOnce(Arc<dyn RuntimeBridge>) -> Fut,
        Fut: Future<Output = T>,
    {
        let runtime = self
            .runtime
            .as_ref()
            .ok_or_else(|| IntentError::no_agent(no_runtime_message))?;
        let runtime = Arc::clone(runtime);
        let runtime_for_call = Arc::clone(&runtime);
        timeout_runtime_call_bounded(&runtime, operation, stage, duration, move || {
            call(runtime_for_call)
        })
        .await
        .map_err(|error| IntentError::internal_error(error.to_string()))
    }

    /// Get a reference to the runtime bridge, if available.
    pub fn runtime(&self) -> Option<&Arc<dyn RuntimeBridge>> {
        self.runtime.as_ref()
    }

    /// Check if a runtime is available for runtime operations.
    pub fn has_runtime(&self) -> bool {
        self.runtime.is_some()
    }

    /// Return the authoritative active-home selection, if one has been stored.
    pub fn active_home_selection(&self) -> Option<ChannelId> {
        self.active_home_selection
    }

    /// Store the authoritative active-home selection.
    pub fn set_active_home_selection(&mut self, home_id: Option<ChannelId>) {
        self.active_home_selection = home_id;
    }

    /// Return a clone of the authoritative semantic-facts store.
    pub fn authoritative_semantic_facts(&self) -> Vec<AuthoritativeSemanticFact> {
        self.authoritative_semantic_facts.clone()
    }

    /// Replace the authoritative semantic-facts store.
    pub fn set_authoritative_semantic_facts(&mut self, facts: Vec<AuthoritativeSemanticFact>) {
        self.authoritative_semantic_facts = facts;
    }

    /// Get a snapshot of the current state.
    pub fn snapshot(&self) -> StateSnapshot {
        self.views.snapshot()
    }

    /// Get access to the view state for reactive subscriptions.
    #[cfg(feature = "app-internals")]
    pub fn views(&self) -> &ViewState {
        &self.views
    }

    #[cfg(not(feature = "app-internals"))]
    pub(crate) fn views(&self) -> &ViewState {
        &self.views
    }

    /// Get mutable access to view state (for internal updates).
    #[allow(dead_code)]
    pub(crate) fn views_mut(&mut self) -> &mut ViewState {
        &mut self.views
    }

    /// Get a reference to the reactive handler.
    pub fn reactive(&self) -> &ReactiveHandler {
        &self.reactive
    }

    /// Sign a tree operation and return an attested operation.
    pub async fn sign_tree_op(&self, op: &TreeOp) -> Result<AttestedOp, IntentError> {
        self.with_runtime_timeout(
            "No runtime available - cannot sign tree operations",
            "sign_tree_op",
            "sign_tree_op",
            APP_RUNTIME_OPERATION_TIMEOUT,
            |runtime| async move { runtime.sign_tree_op(op).await },
        )
        .await?
    }

    /// Bootstrap signing keys for the current authority.
    pub async fn bootstrap_signing_keys(&self) -> Result<Vec<u8>, IntentError> {
        self.with_runtime_timeout(
            "No runtime available - cannot bootstrap signing keys",
            "bootstrap_signing_keys",
            "bootstrap_signing_keys",
            APP_RUNTIME_OPERATION_TIMEOUT,
            |runtime| async move { runtime.bootstrap_signing_keys().await },
        )
        .await?
    }

    /// Get the threshold signing configuration for the current authority.
    pub async fn threshold_config(&self) -> Option<aura_core::threshold::ThresholdConfig> {
        let runtime = self.runtime.as_ref()?;
        timeout_runtime_call_bounded(
            runtime,
            "threshold_config",
            "get_threshold_config",
            APP_RUNTIME_QUERY_TIMEOUT,
            || runtime.get_threshold_config(),
        )
        .await
        .ok()
        .flatten()
    }

    /// Check if this device has signing capability for the current authority.
    pub async fn has_signing_capability(&self) -> bool {
        let Some(runtime) = self.runtime.as_ref() else {
            return false;
        };
        timeout_runtime_call_bounded(
            runtime,
            "has_signing_capability",
            "has_signing_capability",
            APP_RUNTIME_QUERY_TIMEOUT,
            || runtime.has_signing_capability(),
        )
        .await
        .unwrap_or(false)
    }

    /// Get the public key package for the current authority's signing keys.
    pub async fn threshold_signing_public_key(&self) -> Option<Vec<u8>> {
        let runtime = self.runtime.as_ref()?;
        timeout_runtime_call_bounded(
            runtime,
            "threshold_signing_public_key",
            "get_public_key_package",
            APP_RUNTIME_QUERY_TIMEOUT,
            || runtime.get_public_key_package(),
        )
        .await
        .ok()
        .flatten()
    }

    /// Check if the sync service is running.
    pub async fn is_sync_running(&self) -> bool {
        if let Some(runtime) = &self.runtime {
            return timeout_runtime_call_bounded(
                runtime,
                "is_sync_running",
                "try_get_sync_status",
                APP_RUNTIME_QUERY_TIMEOUT,
                || runtime.try_get_sync_status(),
            )
            .await
            .ok()
            .and_then(Result::ok)
            .map(|status| status.is_running)
            .unwrap_or(false);
        }
        false
    }

    /// Get current sync status from the runtime.
    pub async fn sync_status(&self) -> Result<Option<RuntimeSyncStatus>, IntentError> {
        if let Some(runtime) = &self.runtime {
            return timeout_runtime_call_bounded(
                runtime,
                "sync_status",
                "try_get_sync_status",
                APP_RUNTIME_QUERY_TIMEOUT,
                || runtime.try_get_sync_status(),
            )
            .await
            .map_err(|error| IntentError::internal_error(error.to_string()))?
            .map(Some);
        }
        Ok(None)
    }

    /// Get settings + device list from the runtime.
    pub async fn settings_snapshot(
        &self,
    ) -> Result<
        Option<(
            SettingsBridgeState,
            Vec<BridgeDeviceInfo>,
            Vec<BridgeAuthorityInfo>,
        )>,
        IntentError,
    > {
        let Some(runtime) = self.runtime.as_ref() else {
            return Ok(None);
        };
        let settings = timeout_runtime_call_bounded(
            runtime,
            "settings_snapshot",
            "try_get_settings",
            APP_RUNTIME_QUERY_TIMEOUT,
            || runtime.try_get_settings(),
        )
        .await
        .map_err(|error| IntentError::internal_error(error.to_string()))??;
        let devices = timeout_runtime_call_bounded(
            runtime,
            "settings_snapshot",
            "try_list_devices",
            APP_RUNTIME_QUERY_TIMEOUT,
            || runtime.try_list_devices(),
        )
        .await
        .map_err(|error| IntentError::internal_error(error.to_string()))??;
        let authorities = timeout_runtime_call_bounded(
            runtime,
            "settings_snapshot",
            "try_list_authorities",
            APP_RUNTIME_QUERY_TIMEOUT,
            || runtime.try_list_authorities(),
        )
        .await
        .map_err(|error| IntentError::internal_error(error.to_string()))??;
        Ok(Some((settings, devices, authorities)))
    }

    pub async fn sync_peers(&self) -> Result<Vec<aura_core::DeviceId>, IntentError> {
        self.with_runtime_timeout(
            "sync_peers requires a runtime",
            "sync_peers",
            "try_get_sync_peers",
            APP_RUNTIME_QUERY_TIMEOUT,
            |runtime| async move { runtime.try_get_sync_peers().await },
        )
        .await?
    }

    pub async fn discover_peers(&self) -> Result<Vec<AuthorityId>, IntentError> {
        self.with_runtime_timeout(
            "discover_peers requires a runtime",
            "discover_peers",
            "try_get_discovered_peers",
            APP_RUNTIME_QUERY_TIMEOUT,
            |runtime| async move { runtime.try_get_discovered_peers().await },
        )
        .await?
    }

    pub async fn get_bootstrap_candidates(
        &self,
    ) -> Result<Vec<BootstrapCandidateInfo>, IntentError> {
        if let Some(runtime) = &self.runtime {
            return timeout_runtime_call_bounded(
                runtime,
                "get_bootstrap_candidates",
                "try_get_bootstrap_candidates",
                APP_RUNTIME_QUERY_TIMEOUT,
                || runtime.try_get_bootstrap_candidates(),
            )
            .await
            .map_err(|error| IntentError::internal_error(error.to_string()))?;
        }
        Err(IntentError::no_agent(
            "get_bootstrap_candidates requires a runtime",
        ))
    }

    pub async fn is_online(&self) -> bool {
        if let Some(runtime) = &self.runtime {
            let sync = timeout_runtime_call_bounded(
                runtime,
                "is_online",
                "try_get_sync_status",
                APP_RUNTIME_QUERY_TIMEOUT,
                || runtime.try_get_sync_status(),
            )
            .await
            .ok()
            .and_then(Result::ok);
            let rendezvous = timeout_runtime_call_bounded(
                runtime,
                "is_online",
                "try_get_rendezvous_status",
                APP_RUNTIME_QUERY_TIMEOUT,
                || runtime.try_get_rendezvous_status(),
            )
            .await
            .ok()
            .and_then(Result::ok);
            return sync.as_ref().is_some_and(|status| status.is_running)
                || rendezvous.as_ref().is_some_and(|status| status.is_running);
        }
        false
    }

    pub async fn trigger_sync(&self) -> Result<(), IntentError> {
        self.with_runtime_timeout(
            "trigger_sync requires a runtime",
            "trigger_sync",
            "trigger_sync",
            APP_RUNTIME_OPERATION_TIMEOUT,
            |runtime| async move { runtime.trigger_sync().await },
        )
        .await?
    }

    pub async fn sync_with_peer(&self, peer_id: &str) -> Result<(), IntentError> {
        self.with_runtime_timeout(
            "sync_with_peer requires a runtime",
            "sync_with_peer",
            "sync_with_peer",
            APP_RUNTIME_OPERATION_TIMEOUT,
            |runtime| async move { runtime.sync_with_peer(peer_id).await },
        )
        .await?
    }

    pub async fn export_invitation(&self, invitation_id: &str) -> Result<String, IntentError> {
        self.with_runtime_timeout(
            "export_invitation requires a runtime",
            "export_invitation",
            "export_invitation",
            APP_RUNTIME_OPERATION_TIMEOUT,
            |runtime| async move { runtime.export_invitation(invitation_id).await },
        )
        .await?
    }

    pub async fn authentication_status(
        &self,
    ) -> Result<crate::runtime_bridge::AuthenticationStatus, IntentError> {
        if let Some(runtime) = &self.runtime {
            return timeout_runtime_call_bounded(
                runtime,
                "authentication_status",
                "authentication_status",
                APP_RUNTIME_QUERY_TIMEOUT,
                || runtime.authentication_status(),
            )
            .await
            .map_err(|error| IntentError::internal_error(error.to_string()))?;
        }
        Ok(crate::runtime_bridge::AuthenticationStatus::Unauthenticated)
    }

    pub async fn rotate_guardian_keys(
        &self,
        threshold_k: FrostThreshold,
        total_n: u16,
        guardian_ids: &[AuthorityId],
    ) -> Result<(Epoch, Vec<Vec<u8>>, Vec<u8>), IntentError> {
        self.with_runtime_timeout(
            "rotate_guardian_keys requires a runtime",
            "rotate_guardian_keys",
            "rotate_guardian_keys",
            APP_RUNTIME_OPERATION_TIMEOUT,
            |runtime| async move {
                runtime
                    .rotate_guardian_keys(threshold_k, total_n, guardian_ids)
                    .await
            },
        )
        .await?
    }

    pub async fn commit_guardian_key_rotation(&self, new_epoch: Epoch) -> Result<(), IntentError> {
        self.with_runtime_timeout(
            "commit_guardian_key_rotation requires a runtime",
            "commit_guardian_key_rotation",
            "commit_guardian_key_rotation",
            APP_RUNTIME_OPERATION_TIMEOUT,
            |runtime| async move { runtime.commit_guardian_key_rotation(new_epoch).await },
        )
        .await?
    }

    pub async fn rollback_guardian_key_rotation(
        &self,
        failed_epoch: Epoch,
    ) -> Result<(), IntentError> {
        self.with_runtime_timeout(
            "rollback_guardian_key_rotation requires a runtime",
            "rollback_guardian_key_rotation",
            "rollback_guardian_key_rotation",
            APP_RUNTIME_OPERATION_TIMEOUT,
            |runtime| async move { runtime.rollback_guardian_key_rotation(failed_epoch).await },
        )
        .await?
    }

    pub async fn initiate_guardian_ceremony(
        &self,
        threshold_k: FrostThreshold,
        total_n: u16,
        guardian_ids: &[AuthorityId],
    ) -> Result<CeremonyId, IntentError> {
        self.with_runtime_timeout(
            "initiate_guardian_ceremony requires a runtime",
            "initiate_guardian_ceremony",
            "initiate_guardian_ceremony",
            APP_RUNTIME_OPERATION_TIMEOUT,
            |runtime| async move {
                runtime
                    .initiate_guardian_ceremony(threshold_k, total_n, guardian_ids)
                    .await
            },
        )
        .await?
    }

    pub async fn initiate_device_threshold_ceremony(
        &self,
        threshold_k: FrostThreshold,
        total_n: u16,
        device_ids: &[String],
    ) -> Result<CeremonyId, IntentError> {
        self.with_runtime_timeout(
            "initiate_device_threshold_ceremony requires a runtime",
            "initiate_device_threshold_ceremony",
            "initiate_device_threshold_ceremony",
            APP_RUNTIME_OPERATION_TIMEOUT,
            |runtime| async move {
                runtime
                    .initiate_device_threshold_ceremony(threshold_k, total_n, device_ids)
                    .await
            },
        )
        .await?
    }

    pub async fn initiate_device_enrollment_ceremony(
        &self,
        nickname_suggestion: String,
        invitee_authority_id: AuthorityId,
    ) -> Result<crate::runtime_bridge::DeviceEnrollmentStart, IntentError> {
        self.with_runtime_timeout(
            "initiate_device_enrollment_ceremony requires a runtime",
            "initiate_device_enrollment_ceremony",
            "initiate_device_enrollment_ceremony",
            APP_RUNTIME_OPERATION_TIMEOUT,
            |runtime| async move {
                runtime
                    .initiate_device_enrollment_ceremony(nickname_suggestion, invitee_authority_id)
                    .await
            },
        )
        .await?
    }

    pub async fn initiate_device_removal_ceremony(
        &self,
        device_id: String,
    ) -> Result<CeremonyId, IntentError> {
        self.with_runtime_timeout(
            "initiate_device_removal_ceremony requires a runtime",
            "initiate_device_removal_ceremony",
            "initiate_device_removal_ceremony",
            APP_RUNTIME_OPERATION_TIMEOUT,
            |runtime| async move { runtime.initiate_device_removal_ceremony(device_id).await },
        )
        .await?
    }

    pub async fn get_ceremony_status(
        &self,
        ceremony_id: &CeremonyId,
    ) -> Result<crate::runtime_bridge::CeremonyStatus, IntentError> {
        self.with_runtime_timeout(
            "get_ceremony_status requires a runtime",
            "get_ceremony_status",
            "get_ceremony_status",
            APP_RUNTIME_QUERY_TIMEOUT,
            |runtime| async move { runtime.get_ceremony_status(ceremony_id).await },
        )
        .await?
    }

    pub async fn get_key_rotation_ceremony_status(
        &self,
        ceremony_id: &CeremonyId,
    ) -> Result<crate::runtime_bridge::KeyRotationCeremonyStatus, IntentError> {
        self.with_runtime_timeout(
            "get_key_rotation_ceremony_status requires a runtime",
            "get_key_rotation_ceremony_status",
            "get_key_rotation_ceremony_status",
            APP_RUNTIME_QUERY_TIMEOUT,
            |runtime| async move { runtime.get_key_rotation_ceremony_status(ceremony_id).await },
        )
        .await?
    }

    pub async fn cancel_key_rotation_ceremony(
        &self,
        ceremony_id: &CeremonyId,
    ) -> Result<(), IntentError> {
        self.with_runtime_timeout(
            "cancel_key_rotation_ceremony requires a runtime",
            "cancel_key_rotation_ceremony",
            "cancel_key_rotation_ceremony",
            APP_RUNTIME_OPERATION_TIMEOUT,
            |runtime| async move { runtime.cancel_key_rotation_ceremony(ceremony_id).await },
        )
        .await?
    }
}
