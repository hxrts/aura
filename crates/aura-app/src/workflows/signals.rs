//! Helpers for consistent signal read/emit patterns in workflows.

use std::sync::Arc;

use async_lock::RwLock;
use aura_core::effects::reactive::{ReactiveEffects, Signal};
use aura_core::AuraError;

use crate::AppCore;

/// Read a signal or return a standardized AuraError.
pub async fn read_signal<T>(
    app_core: &Arc<RwLock<AppCore>>,
    signal: &Signal<T>,
    name: &str,
) -> Result<T, AuraError>
where
    T: Clone + Send + Sync + 'static,
{
    let core = app_core.read().await;
    core.read(signal)
        .await
        .map_err(|e| AuraError::internal(format!("Failed to read {name}: {e}")))
}

/// Read a signal or return its Default value on error.
pub async fn read_signal_or_default<T>(app_core: &Arc<RwLock<AppCore>>, signal: &Signal<T>) -> T
where
    T: Clone + Default + Send + Sync + 'static,
{
    let core = app_core.read().await;
    core.read(signal).await.unwrap_or_default()
}

/// Emit a signal or return a standardized AuraError.
pub async fn emit_signal<T>(
    app_core: &Arc<RwLock<AppCore>>,
    signal: &Signal<T>,
    value: T,
    name: &str,
) -> Result<(), AuraError>
where
    T: Clone + Send + Sync + 'static,
{
    let core = app_core.read().await;
    core.emit(signal, value)
        .await
        .map_err(|e| AuraError::internal(format!("Failed to emit {name}: {e}")))
}
