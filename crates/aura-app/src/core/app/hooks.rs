//! Hook-installation and runtime-detach responsibilities for `AppCore`.

use super::state::AppCore;
use crate::core::IntentError;
use async_lock::RwLock;
use std::sync::Arc;

impl AppCore {
    pub(crate) fn contacts_refresh_hook_installed(&self) -> bool {
        self.contacts_refresh_hook_installed
    }

    pub(crate) fn mark_contacts_refresh_hook_installed(&mut self) -> bool {
        if self.contacts_refresh_hook_installed {
            false
        } else {
            self.contacts_refresh_hook_installed = true;
            true
        }
    }

    pub(crate) fn chat_refresh_hook_installed(&self) -> bool {
        self.chat_refresh_hook_installed
    }

    pub(crate) fn mark_chat_refresh_hook_installed(&mut self) -> bool {
        if self.chat_refresh_hook_installed {
            false
        } else {
            self.chat_refresh_hook_installed = true;
            true
        }
    }

    #[cfg(feature = "signals")]
    pub(crate) fn authoritative_readiness_hook_installed(&self) -> bool {
        self.authoritative_readiness_hook_installed
    }

    #[cfg(feature = "signals")]
    pub(crate) fn mark_authoritative_readiness_hook_installed(&mut self) -> bool {
        if self.authoritative_readiness_hook_installed {
            false
        } else {
            self.authoritative_readiness_hook_installed = true;
            true
        }
    }

    /// Initialize signals and install runtime-backed hooks.
    pub async fn init_signals_with_hooks(
        app_core: &Arc<RwLock<AppCore>>,
    ) -> Result<(), IntentError> {
        {
            let mut core = app_core.write().await;
            core.init_signals().await?;
        }

        let has_runtime = {
            let core = app_core.read().await;
            core.runtime().is_some()
        };
        if !has_runtime {
            return Ok(());
        }

        crate::workflows::system::install_contacts_refresh_hook(app_core)
            .await
            .map_err(|e| IntentError::internal_error(format!("Failed to install hooks: {e}")))?;
        crate::workflows::system::install_chat_refresh_hook(app_core)
            .await
            .map_err(|e| IntentError::internal_error(format!("Failed to install hooks: {e}")))?;
        crate::workflows::system::install_authoritative_readiness_hook(app_core)
            .await
            .map_err(|e| IntentError::internal_error(format!("Failed to install hooks: {e}")))?;

        Ok(())
    }

    /// Detach the runtime bridge from one AppCore instance.
    ///
    /// This is a teardown-only escape hatch for frontend generation shutdown.
    pub async fn detach_runtime(app_core: &Arc<RwLock<AppCore>>) -> bool {
        let mut core = app_core.write().await;
        let had_runtime = core.runtime.take().is_some();
        if had_runtime {
            core.contacts_refresh_hook_installed = false;
            core.chat_refresh_hook_installed = false;
            #[cfg(feature = "signals")]
            {
                core.authoritative_readiness_hook_installed = false;
            }
        }
        had_runtime
    }
}
