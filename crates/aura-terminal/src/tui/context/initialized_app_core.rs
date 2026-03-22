use std::sync::Arc;

use async_lock::RwLock;
use aura_app::ui::prelude::*;
use aura_app::ui::types::RuntimeBridge;
use aura_core::AuraError;

/// A TUI-local wrapper that guarantees `AppCore::init_signals()` has been called.
///
/// This prevents a class of bugs where screens subscribe/read before signals are
/// registered (or before ViewState forwarding is active), which otherwise shows
/// up as "silent non-updating" UI.
#[derive(Clone)]
pub struct InitializedAppCore {
    app_core: Arc<RwLock<AppCore>>,
    runtime: Option<Arc<dyn RuntimeBridge>>,
}

impl InitializedAppCore {
    pub async fn new(app_core: Arc<RwLock<AppCore>>) -> Result<Self, AuraError> {
        let runtime = {
            let core = app_core.read().await;
            core.runtime().cloned()
        };

        if runtime.is_some() {
            AppCore::init_signals_with_hooks(&app_core)
                .await
                .map_err(|e| AuraError::internal(e.to_string()))?;
        } else {
            let mut core = app_core.write().await;
            core.init_signals()
                .await
                .map_err(|e| AuraError::internal(e.to_string()))?;
        }

        Ok(Self { app_core, runtime })
    }

    #[inline]
    #[must_use]
    pub fn raw(&self) -> &Arc<RwLock<AppCore>> {
        &self.app_core
    }

    #[inline]
    #[must_use]
    pub fn runtime(&self) -> Option<Arc<dyn RuntimeBridge>> {
        self.runtime.clone()
    }
}
