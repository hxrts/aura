use std::sync::Arc;

use async_lock::RwLock;
use aura_app::AppCore;
use aura_core::AuraError;

/// A TUI-local wrapper that guarantees `AppCore::init_signals()` has been called.
///
/// This prevents a class of bugs where screens subscribe/read before signals are
/// registered (or before ViewState forwarding is active), which otherwise shows
/// up as "silent non-updating" UI.
#[derive(Clone)]
pub struct InitializedAppCore {
    app_core: Arc<RwLock<AppCore>>,
}

impl InitializedAppCore {
    pub async fn new(app_core: Arc<RwLock<AppCore>>) -> Result<Self, AuraError> {
        {
            let mut core = app_core.write().await;
            core.init_signals()
                .await
                .map_err(|e| AuraError::internal(format!("Failed to init signals: {e}")))?;
        }

        Ok(Self { app_core })
    }

    #[inline]
    pub fn raw(&self) -> &Arc<RwLock<AppCore>> {
        &self.app_core
    }
}
