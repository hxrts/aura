//! System Workflow - Portable Business Logic
//!
//! This module contains system-level operations that are portable across all frontends.
//! These are mostly lightweight health-check and state refresh operations.

use crate::AppCore;
use async_lock::RwLock;
use aura_core::AuraError;
use std::sync::Arc;

/// Ping operation for health check
///
/// **What it does**: Simple health check operation
/// **Returns**: Unit result
/// **Signal pattern**: Read-only operation (no emission)
///
/// This is a no-op that verifies the workflow layer is responsive.
pub async fn ping(_app_core: &Arc<RwLock<AppCore>>) -> Result<(), AuraError> {
    Ok(())
}

/// Refresh account state
///
/// **What it does**: Triggers state refresh
/// **Returns**: Unit result
/// **Signal pattern**: Could emit multiple signals for full refresh
///
/// This operation triggers a state refresh by re-emitting all signals,
/// causing subscribers to re-render with current state.
///
/// **TODO**: Implement full signal refresh once all workflows are complete.
pub async fn refresh_account(_app_core: &Arc<RwLock<AppCore>>) -> Result<(), AuraError> {
    // TODO: Re-emit all signals for full state refresh
    // For now, this is a no-op
    Ok(())
}

/// Check if app core is accessible
///
/// **What it does**: Verifies AppCore can be accessed
/// **Returns**: Boolean indicating accessibility
/// **Signal pattern**: Read-only operation (no emission)
pub async fn is_available(app_core: &Arc<RwLock<AppCore>>) -> bool {
    app_core.try_read().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppConfig;

    #[tokio::test]
    async fn test_ping() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let result = ping(&app_core).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_is_available() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let available = is_available(&app_core).await;
        assert!(available);
    }

    #[tokio::test]
    async fn test_refresh_account() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let result = refresh_account(&app_core).await;
        assert!(result.is_ok());
    }
}
