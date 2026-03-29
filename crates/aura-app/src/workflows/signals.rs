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
    _name: &str,
) -> Result<T, AuraError>
where
    T: Clone + Send + Sync + 'static,
{
    let core = app_core.read().await;
    core.read(signal)
        .await
        .map_err(|e| AuraError::internal(e.to_string()))
}

/// Read a signal or return its Default value on error.
///
/// This is a convenience helper for non-critical reads where a default is
/// acceptable.  Callers on parity-critical paths should use [`read_signal`]
/// instead so errors are visible.
///
/// When the `instrumented` feature is active, a debug-level message is
/// emitted on fallback so signal-system failures are diagnosable.
#[allow(clippy::manual_unwrap_or_default)] // Intentional: log on fallback when instrumented.
pub async fn read_signal_or_default<T>(app_core: &Arc<RwLock<AppCore>>, signal: &Signal<T>) -> T
where
    T: Clone + Default + Send + Sync + 'static,
{
    let core = app_core.read().await;
    match core.read(signal).await {
        Ok(value) => value,
        Err(_e) => {
            #[cfg(feature = "instrumented")]
            tracing::debug!(error = %_e, "read_signal_or_default: falling back to default");
            T::default()
        }
    }
}

/// Emit a signal or return a standardized AuraError.
pub async fn emit_signal<T>(
    app_core: &Arc<RwLock<AppCore>>,
    signal: &Signal<T>,
    value: T,
    _name: &str,
) -> Result<(), AuraError>
where
    T: Clone + Send + Sync + 'static,
{
    let core = app_core.read().await;
    core.emit(signal, value)
        .await
        .map_err(|e| AuraError::internal(e.to_string()))
}

/// Emit a signal only when its value changes.
pub async fn emit_signal_if_changed<T>(
    app_core: &Arc<RwLock<AppCore>>,
    signal: &Signal<T>,
    value: T,
    name: &str,
) -> Result<(), AuraError>
where
    T: Clone + PartialEq + Send + Sync + 'static,
{
    let current = read_signal(app_core, signal, name).await?;
    if current == value {
        return Ok(());
    }
    emit_signal(app_core, signal, value, name).await
}
