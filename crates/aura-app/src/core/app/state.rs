//! Core `AppCore` state, configuration, and constructors.

use crate::core::IntentError;
use crate::runtime_bridge::RuntimeBridge;
use crate::ui_contract::AuthoritativeSemanticFact;
use crate::views::ViewState;
use crate::ReactiveHandler;
use aura_core::hash;
use aura_core::types::identifiers::{AuthorityId, ChannelId};
use aura_core::AccountId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

pub(super) const APP_RUNTIME_QUERY_TIMEOUT: Duration = Duration::from_millis(5_000);
pub(super) const APP_RUNTIME_OPERATION_TIMEOUT: Duration = Duration::from_millis(30_000);

/// Configuration for creating an AppCore instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct AppConfig {
    pub data_dir: String,
    pub debug: bool,
    pub journal_path: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            data_dir: "./data".to_string(),
            debug: false,
            journal_path: None,
        }
    }
}

/// Unique identifier for a subscription (callbacks feature only).
#[cfg(feature = "callbacks")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct SubscriptionId {
    pub id: u64,
}

/// Portable application core state and injected runtime handles.
pub struct AppCore {
    pub(super) authority: Option<AuthorityId>,
    pub(super) account_id: AccountId,
    pub(super) views: ViewState,
    pub(super) active_home_selection: Option<ChannelId>,
    pub(super) authoritative_semantic_facts: Vec<AuthoritativeSemanticFact>,
    pub(super) runtime: Option<Arc<dyn RuntimeBridge>>,
    pub(super) reactive: ReactiveHandler,
    #[cfg(feature = "callbacks")]
    pub(super) observer_registry: crate::bridge::callback::ObserverRegistry,
    pub(super) contacts_refresh_hook_installed: bool,
    pub(super) chat_refresh_hook_installed: bool,
    #[cfg(feature = "signals")]
    pub(super) authoritative_readiness_hook_installed: bool,
}

impl AppCore {
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
            reactive,
            #[cfg(feature = "callbacks")]
            observer_registry: crate::bridge::callback::ObserverRegistry::new(),
            contacts_refresh_hook_installed: false,
            chat_refresh_hook_installed: false,
            #[cfg(feature = "signals")]
            authoritative_readiness_hook_installed: false,
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
            reactive,
            #[cfg(feature = "callbacks")]
            observer_registry: crate::bridge::callback::ObserverRegistry::new(),
            contacts_refresh_hook_installed: false,
            chat_refresh_hook_installed: false,
            #[cfg(feature = "signals")]
            authoritative_readiness_hook_installed: false,
        })
    }

    pub fn account_id(&self) -> AccountId {
        self.account_id
    }

    pub fn set_authority(&mut self, authority: AuthorityId) {
        self.authority = Some(authority);
    }

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
