//! Core `AppCore` state, configuration, and constructors.

use super::config::AppConfig;
use super::hooks::HookInstallState;
use crate::core::IntentError;
use crate::runtime_bridge::RuntimeBridge;
use crate::ui_contract::AuthoritativeSemanticFact;
use crate::views::ViewState;
use crate::ReactiveHandler;
use aura_core::effects::reactive::SignalId;
use aura_core::hash;
use aura_core::types::identifiers::{AuthorityId, ChannelId};
use aura_core::AccountId;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

pub(super) const APP_RUNTIME_QUERY_TIMEOUT: Duration = Duration::from_millis(5_000);
pub(super) const APP_RUNTIME_OPERATION_TIMEOUT: Duration = Duration::from_millis(30_000);

/// Portable application core state and injected runtime handles.
pub struct AppCore {
    pub(super) authority: Option<AuthorityId>,
    pub(super) account_id: AccountId,
    pub(super) views: ViewState,
    pub(super) active_home_selection: Option<ChannelId>,
    pub(super) authoritative_semantic_facts: Vec<AuthoritativeSemanticFact>,
    pub(super) runtime: Option<Arc<dyn RuntimeBridge>>,
    pub(super) runtime_attachment_spent: bool,
    pub(super) reactive: ReactiveHandler,
    /// Last reactive revision copied into each observed view cell.
    pub(super) projection_revisions: HashMap<SignalId, u64>,
    #[cfg(feature = "callbacks")]
    pub(super) observer_registry: crate::bridge::callback::ObserverRegistry,
    pub(super) hook_install_gate: Arc<async_lock::Mutex<()>>,
    /// Serializes workflow transitions that jointly change home selection and
    /// neighborhood traversal with runtime homes mirroring.
    pub(super) navigation_projection_gate: Arc<async_lock::Mutex<()>>,
    pub(super) hook_install_state: HookInstallState,
}

impl AppCore {
    /// Attach the first runtime to an existing bootstrap app, retaining its
    /// original semantic operation history. Existing runtime custody cannot be
    /// replaced through this API.
    pub fn attach_bootstrap_runtime(
        &mut self,
        runtime: Arc<dyn RuntimeBridge>,
    ) -> Result<(), IntentError> {
        if self.runtime_attachment_spent || self.runtime.is_some() {
            return Err(IntentError::service_error(
                "bootstrap app already owns a runtime",
            ));
        }
        let authority = runtime.authority_id();
        self.authority = Some(authority);
        self.account_id = AccountId::from_bytes(hash::hash(&authority.to_bytes()));
        self.reactive = runtime.reactive_handler();
        self.projection_revisions.clear();
        self.runtime = Some(runtime);
        self.runtime_attachment_spent = true;
        Ok(())
    }

    /// Create a new AppCore instance with the given configuration.
    pub fn new(config: AppConfig) -> Result<Self, IntentError> {
        let config_seed = format!(
            "{}:{}",
            config.data_dir,
            config.journal_path.clone().unwrap_or_default()
        );
        let account_id = AccountId::from_bytes(hash::hash(config_seed.as_bytes()));
        let reactive = ReactiveHandler::new();
        let _ = config;

        Ok(Self {
            authority: None,
            account_id,
            views: ViewState::default(),
            active_home_selection: None,
            authoritative_semantic_facts: Vec::new(),
            runtime: None,
            runtime_attachment_spent: false,
            reactive,
            projection_revisions: HashMap::new(),
            #[cfg(feature = "callbacks")]
            observer_registry: crate::bridge::callback::ObserverRegistry::new(),
            hook_install_gate: Arc::new(async_lock::Mutex::new(())),
            navigation_projection_gate: Arc::new(async_lock::Mutex::new(())),
            hook_install_state: HookInstallState::Stopped,
        })
    }

    /// Create an AppCore with a RuntimeBridge for full runtime capabilities.
    pub fn with_runtime(
        config: AppConfig,
        runtime: Arc<dyn RuntimeBridge>,
    ) -> Result<Self, IntentError> {
        let mut app = Self::new(config)?;
        let authority_id = runtime.authority_id();
        app.authority = Some(authority_id);
        app.account_id = AccountId::from_bytes(hash::hash(&authority_id.to_bytes()));
        app.reactive = runtime.reactive_handler();
        app.runtime = Some(runtime);
        app.runtime_attachment_spent = true;
        Ok(app)
    }

    /// Create an AppCore with a specific account ID and authority.
    pub fn with_identity(
        account_id: AccountId,
        authority: AuthorityId,
        _group_key_bytes: Vec<u8>,
    ) -> Result<Self, IntentError> {
        let reactive = ReactiveHandler::new();

        Ok(Self {
            authority: Some(authority),
            account_id,
            views: ViewState::default(),
            active_home_selection: None,
            authoritative_semantic_facts: Vec::new(),
            runtime: None,
            runtime_attachment_spent: false,
            reactive,
            projection_revisions: HashMap::new(),
            #[cfg(feature = "callbacks")]
            observer_registry: crate::bridge::callback::ObserverRegistry::new(),
            hook_install_gate: Arc::new(async_lock::Mutex::new(())),
            navigation_projection_gate: Arc::new(async_lock::Mutex::new(())),
            hook_install_state: HookInstallState::Stopped,
        })
    }

    /// Return the stable app-owned account identifier for this core instance.
    pub fn account_id(&self) -> AccountId {
        self.account_id
    }

    /// Update the in-memory authority binding for this app core.
    pub fn set_authority(&mut self, authority: AuthorityId) {
        self.authority = Some(authority);
    }

    /// Return the currently bound authority, if one has been staged.
    pub fn authority(&self) -> Option<&AuthorityId> {
        self.authority.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_config_default() {
        let config = AppConfig::default();
        assert_eq!(config.data_dir, "./data");
        assert!(!config.debug);
    }

    #[test]
    fn test_app_core_creation() {
        let config = AppConfig::default();
        let app = AppCore::new(config);
        assert!(app.is_ok());
    }

    #[test]
    fn test_snapshot_empty() {
        let config = AppConfig::default();
        let app = AppCore::new(config).unwrap();
        let snapshot = app.snapshot();
        assert!(snapshot.is_empty());
    }
}
