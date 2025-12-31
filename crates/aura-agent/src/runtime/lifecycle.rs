//! Lifecycle Manager
//!
//! Manages component lifecycle and system shutdown.

use super::EffectContext;
use crate::handlers::SessionServiceApi;
use crate::runtime::AuraEffectSystem;
use std::sync::Arc;

/// Lifecycle manager for coordinating system startup and shutdown
pub struct LifecycleManager {
    /// Session cleanup timeout in seconds (default: 1 hour)
    session_cleanup_timeout: u64,
}

impl LifecycleManager {
    /// Create a new lifecycle manager
    pub fn new() -> Self {
        Self {
            session_cleanup_timeout: 3600, // 1 hour default
        }
    }

    /// Create lifecycle manager with custom session timeout
    pub fn with_session_timeout(timeout_seconds: u64) -> Self {
        Self {
            session_cleanup_timeout: timeout_seconds,
        }
    }

    /// Initialize services on startup
    ///
    /// Performs startup tasks like cleaning up stale sessions.
    pub async fn initialize(
        &self,
        effects: Arc<AuraEffectSystem>,
        authority_context: crate::core::AuthorityContext,
    ) -> Result<(), String> {
        // Clean up any stale sessions from previous runs
        let account_id = aura_core::identifiers::AccountId::new_from_entropy(
            aura_core::hash::hash(&authority_context.authority_id().to_bytes()),
        );

        let session_service = SessionServiceApi::new(effects, authority_context, account_id)
            .map_err(|e| format!("Session service init failed: {e}"))?;

        // Clean up sessions older than the timeout
        match session_service
            .cleanup_expired(self.session_cleanup_timeout)
            .await
        {
            Ok(cleaned) => {
                if !cleaned.is_empty() {
                    tracing::info!("Cleaned up {} expired sessions on startup", cleaned.len());
                }
            }
            Err(e) => {
                tracing::warn!("Failed to cleanup expired sessions on startup: {}", e);
            }
        }

        Ok(())
    }

    /// Shutdown all managed components
    pub async fn shutdown(self, _ctx: &EffectContext) -> Result<(), String> {
        // Coordinate clean shutdown of all components
        // Session data is already persisted to storage, so no action needed
        tracing::info!("Lifecycle manager shutdown complete");
        Ok(())
    }
}

impl Default for LifecycleManager {
    fn default() -> Self {
        Self::new()
    }
}
