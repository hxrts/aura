//! Stub Coordinator for compilation
//!
//! This is a minimal stub to allow aura-agent to compile while the full
//! coordinator is being refactored to use the new authority-centric architecture.

use aura_core::effects::*;
use aura_effects::*;
use async_trait::async_trait;
use std::sync::Arc;

/// Minimal stub effect system that composes handlers from aura-effects
pub struct AuraEffectSystem {
    console: Arc<RealConsoleHandler>,
    crypto: Arc<RealCryptoHandler>,
    random: Arc<RealRandomHandler>,
    time: Arc<RealTimeHandler>,
    storage: Arc<MemoryStorageHandler>,
    journal: Arc<StandardJournalHandler>,
    authorization: Arc<StandardAuthorizationHandler>,
    leakage: Arc<NoOpLeakageHandler>,
}

impl AuraEffectSystem {
    /// Create a new stub effect system
    pub fn new() -> Self {
        Self {
            console: Arc::new(RealConsoleHandler::new()),
            crypto: Arc::new(RealCryptoHandler::new()),
            random: Arc::new(RealRandomHandler::new()),
            time: Arc::new(RealTimeHandler::new()),
            storage: Arc::new(MemoryStorageHandler::new()),
            journal: Arc::new(StandardJournalHandler::new()),
            authorization: Arc::new(StandardAuthorizationHandler::new()),
            leakage: Arc::new(NoOpLeakageHandler),
        }
    }
}

impl Default for AuraEffectSystem {
    fn default() -> Self {
        Self::new()
    }
}

// Implement ConsoleEffects by delegating to the console handler
#[async_trait]
impl ConsoleEffects for AuraEffectSystem {
    async fn log_info(&self, message: &str) -> Result<(), aura_core::AuraError> {
        ConsoleEffects::log_info(self.console.as_ref(), message).await
    }

    async fn log_warn(&self, message: &str) -> Result<(), aura_core::AuraError> {
        ConsoleEffects::log_warn(self.console.as_ref(), message).await
    }

    async fn log_error(&self, message: &str) -> Result<(), aura_core::AuraError> {
        ConsoleEffects::log_error(self.console.as_ref(), message).await
    }

    async fn log_debug(&self, message: &str) -> Result<(), aura_core::AuraError> {
        ConsoleEffects::log_debug(self.console.as_ref(), message).await
    }
}

// Implement RandomEffects by delegating to the random handler
#[async_trait]
impl RandomEffects for AuraEffectSystem {
    async fn random_bytes(&self, count: usize) -> Vec<u8> {
        RandomEffects::random_bytes(self.random.as_ref(), count).await
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        RandomEffects::random_bytes_32(self.random.as_ref()).await
    }

    async fn random_range(&self, min: u64, max: u64) -> u64 {
        RandomEffects::random_range(self.random.as_ref(), min, max).await
    }

    fn execution_mode(&self) -> ExecutionMode {
        RandomEffects::execution_mode(self.random.as_ref())
    }
}

// Implement TimeEffects by delegating to the time handler
#[async_trait]
impl TimeEffects for AuraEffectSystem {
    async fn current_timestamp(&self) -> u64 {
        TimeEffects::current_timestamp(self.time.as_ref()).await
    }

    async fn current_timestamp_millis(&self) -> u64 {
        TimeEffects::current_timestamp_millis(self.time.as_ref()).await
    }

    async fn sleep(&self, duration_ms: u64) {
        TimeEffects::sleep(self.time.as_ref(), duration_ms).await
    }

    async fn timeout<F: std::future::Future + Send>(
        &self,
        duration_ms: u64,
        future: F,
    ) -> Result<F::Output, TimeError> {
        TimeEffects::timeout(self.time.as_ref(), duration_ms, future).await
    }

    async fn set_timeout(&self, duration_ms: u64) -> TimeoutHandle {
        TimeEffects::set_timeout(self.time.as_ref(), duration_ms).await
    }

    async fn cancel_timeout(&self, handle: TimeoutHandle) {
        TimeEffects::cancel_timeout(self.time.as_ref(), handle).await
    }

    async fn wait_until(&self, condition: WakeCondition) {
        TimeEffects::wait_until(self.time.as_ref(), condition).await
    }
}

// Implement StorageEffects by delegating to the storage handler
#[async_trait]
impl StorageEffects for AuraEffectSystem {
    async fn read(&self, key: &str) -> Result<Option<Vec<u8>>, aura_core::AuraError> {
        StorageEffects::read(self.storage.as_ref(), key).await
    }

    async fn write(&self, key: &str, value: &[u8]) -> Result<(), aura_core::AuraError> {
        StorageEffects::write(self.storage.as_ref(), key, value).await
    }

    async fn delete(&self, key: &str) -> Result<(), aura_core::AuraError> {
        StorageEffects::delete(self.storage.as_ref(), key).await
    }

    async fn list_keys(&self, prefix: &str) -> Result<Vec<String>, aura_core::AuraError> {
        StorageEffects::list_keys(self.storage.as_ref(), prefix).await
    }
}

// Implement JournalEffects by delegating to the journal handler
#[async_trait]
impl JournalEffects for AuraEffectSystem {
    async fn record_event(&self, event: &str, data: &[u8]) -> Result<(), aura_core::AuraError> {
        JournalEffects::record_event(self.journal.as_ref(), event, data).await
    }

    async fn get_events(&self, since: u64) -> Result<Vec<(u64, String, Vec<u8>)>, aura_core::AuraError> {
        JournalEffects::get_events(self.journal.as_ref(), since).await
    }
}

// Implement AuthorizationEffects by delegating to the authorization handler
#[async_trait]
impl AuthorizationEffects for AuraEffectSystem {
    async fn check_authorization(
        &self,
        token: &[u8],
        resource: &str,
        action: &str,
    ) -> Result<bool, aura_core::AuraError> {
        AuthorizationEffects::check_authorization(self.authorization.as_ref(), token, resource, action).await
    }

    async fn create_token(
        &self,
        authority_id: &aura_core::AuthorityId,
        capabilities: Vec<String>,
    ) -> Result<Vec<u8>, aura_core::AuraError> {
        AuthorizationEffects::create_token(self.authorization.as_ref(), authority_id, capabilities).await
    }

    async fn revoke_token(&self, token_id: &str) -> Result<(), aura_core::AuraError> {
        AuthorizationEffects::revoke_token(self.authorization.as_ref(), token_id).await
    }
}

// Implement LeakageEffects by delegating to the leakage handler
#[async_trait]
impl LeakageEffects for AuraEffectSystem {
    async fn record_leakage(
        &self,
        context_id: &aura_core::ContextId,
        bytes: u64,
    ) -> Result<(), aura_core::AuraError> {
        LeakageEffects::record_leakage(self.leakage.as_ref(), context_id, bytes).await
    }

    async fn get_leakage_budget(&self, context_id: &aura_core::ContextId) -> Result<u64, aura_core::AuraError> {
        LeakageEffects::get_leakage_budget(self.leakage.as_ref(), context_id).await
    }
}
