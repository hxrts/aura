//! Effect API test helpers and utilities
//!
//! This module provides standardized helpers for creating and managing test effect_apis
//! (CRDT-based account effect_apis) across the Aura test suite.

use aura_core::hash::hash;
use aura_core::AccountId;
use aura_journal::semilattice::AccountState;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Simple AccountEffectApi replacement for testing
/// This wraps AccountState to provide the expected interface for tests
#[derive(Debug, Clone)]
pub struct AccountEffectApi {
    state: AccountState,
}

impl AccountEffectApi {
    /// Create a new AccountEffectApi with the given initial state
    pub fn new(state: AccountState) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self { state })
    }

    /// Get the current account state
    pub fn state(&self) -> &AccountState {
        &self.state
    }

    /// Get mutable access to the account state
    pub fn state_mut(&mut self) -> &mut AccountState {
        &mut self.state
    }
}

/// Effect API test fixture for consistent test effect_api creation
#[derive(Debug, Clone)]
pub struct LedgerTestFixture {
    account_id: AccountId,
    effect_api: Arc<RwLock<AccountEffectApi>>,
}

impl LedgerTestFixture {
    /// Create a new effect_api test fixture with a specific account ID
    pub async fn new(account_id: AccountId) -> Self {
        // Create a minimal AccountState for testing
        let (_, group_public_key) = crate::test_key_pair(42);
        let initial_state = AccountState::new(account_id, group_public_key);
        let effect_api = Arc::new(RwLock::new(
            AccountEffectApi::new(initial_state).expect("Failed to create AccountEffectApi"),
        ));

        Self {
            account_id,
            effect_api,
        }
    }

    /// Create a random effect_api fixture
    pub async fn random() -> Self {
        let hash_input = "effect_api-fixture-random";
        let hash_bytes = hash(hash_input.as_bytes());
        let uuid = Uuid::from_bytes(hash_bytes[..16].try_into().unwrap());
        let account_id = AccountId(uuid);
        Self::new(account_id).await
    }

    /// Get the account ID
    pub fn account_id(&self) -> AccountId {
        self.account_id
    }

    /// Get a reference to the effect_api
    pub fn effect_api(&self) -> Arc<RwLock<AccountEffectApi>> {
        Arc::clone(&self.effect_api)
    }

    /// Get account state from the effect_api
    pub async fn account_state(&self) -> Result<AccountState, Box<dyn std::error::Error>> {
        let effect_api = self.effect_api.read().await;
        Ok(effect_api.state().clone())
    }
}

/// Builder for creating test effect_apis with specific configuration
#[derive(Debug)]
pub struct LedgerBuilder {
    account_id: Option<AccountId>,
    threshold: Option<u16>,
}

impl LedgerBuilder {
    /// Create a new effect_api builder
    pub fn new() -> Self {
        Self {
            account_id: None,
            threshold: None,
        }
    }

    /// Set a specific account ID
    pub fn with_account_id(mut self, account_id: AccountId) -> Self {
        self.account_id = Some(account_id);
        self
    }

    /// Set the threshold for threshold cryptography
    pub fn with_threshold(mut self, threshold: u16) -> Self {
        self.threshold = Some(threshold);
        self
    }

    /// Build the effect_api fixture
    pub async fn build(self) -> Result<LedgerTestFixture, Box<dyn std::error::Error>> {
        let account_id = self.account_id.unwrap_or_else(|| {
            let hash_input = "effect_api-builder-account";
            let hash_bytes = hash(hash_input.as_bytes());
            let uuid = Uuid::from_bytes(hash_bytes[..16].try_into().unwrap());
            AccountId(uuid)
        });

        let fixture = LedgerTestFixture::new(account_id).await;
        Ok(fixture)
    }
}

impl Default for LedgerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Common test effect_api creation helpers
/// Helper functions for creating test effect_apis
pub mod effect_api_helpers {
    use super::*;

    /// Create a single test effect_api with default configuration
    pub async fn test_effect_api() -> LedgerTestFixture {
        LedgerTestFixture::random().await
    }

    /// Create a test effect_api with a specific account ID
    pub async fn test_effect_api_for_account(account_id: AccountId) -> LedgerTestFixture {
        LedgerTestFixture::new(account_id).await
    }

    /// Create a test effect_api for a threshold scenario
    pub async fn test_effect_api_threshold(
        _threshold: u16,
    ) -> Result<LedgerTestFixture, Box<dyn std::error::Error>> {
        Ok(LedgerBuilder::new()
            .with_threshold(_threshold)
            .build()
            .await?)
    }

    /// Get device count and threshold for a named scenario
    pub fn effect_api_config_for_scenario(config_name: &str) -> (usize, u16) {
        match config_name {
            "three-device" => (3, 2),
            "threshold-2-3" => (3, 2),
            "threshold-3-5" => (5, 3),
            "distributed-4" => (4, 2),
            _ => (2, 1), // Default fallback
        }
    }

    /// Create a two-effect_api pair
    pub async fn test_effect_api_pair(
    ) -> Result<(LedgerTestFixture, LedgerTestFixture), Box<dyn std::error::Error>> {
        let hash_input = "effect_api-pair";
        let hash_bytes = hash(hash_input.as_bytes());
        let uuid = Uuid::from_bytes(hash_bytes[..16].try_into().unwrap());
        let account_id = AccountId(uuid);
        let effect_api1 = LedgerBuilder::new()
            .with_account_id(account_id)
            .build()
            .await?;

        let effect_api2 = LedgerTestFixture::new(account_id).await;

        Ok((effect_api1, effect_api2))
    }

    /// Create a three-effect_api trio
    pub async fn test_effect_api_trio(
    ) -> Result<(LedgerTestFixture, LedgerTestFixture, LedgerTestFixture), Box<dyn std::error::Error>>
    {
        let hash_input = "effect_api-trio";
        let hash_bytes = hash(hash_input.as_bytes());
        let uuid = Uuid::from_bytes(hash_bytes[..16].try_into().unwrap());
        let account_id = AccountId(uuid);

        Ok((
            LedgerTestFixture::new(account_id).await,
            LedgerTestFixture::new(account_id).await,
            LedgerTestFixture::new(account_id).await,
        ))
    }

    /// Verify effect_api consistency across multiple instances
    pub async fn verify_effect_api_consistency(
        effect_apis: &[LedgerTestFixture],
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if effect_apis.is_empty() {
            return Ok(true);
        }

        let first_account = effect_apis[0].account_id();

        for effect_api in effect_apis {
            if effect_api.account_id() != first_account {
                return Ok(false);
            }

            let state = effect_api.account_state().await?;
            if state.account_id != first_account {
                return Ok(false);
            }
        }

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_effect_api_fixture_creation() {
        let effect_api = LedgerTestFixture::random().await;
        assert!(!effect_api.account_id().0.is_nil());
    }

    #[tokio::test]
    async fn test_effect_api_builder() {
        let effect_api = LedgerBuilder::new()
            .build()
            .await
            .expect("failed to build effect_api");

        assert!(!effect_api.account_id().0.is_nil());
    }

    #[tokio::test]
    async fn test_effect_api_with_threshold() {
        let effect_api = LedgerBuilder::new()
            .with_threshold(3)
            .build()
            .await
            .expect("failed to build effect_api");

        assert!(!effect_api.account_id().0.is_nil());
    }

    #[tokio::test]
    async fn test_effect_api_consistency() {
        let account_id = AccountId::new();
        let effect_api1 = LedgerTestFixture::new(account_id).await;
        let effect_api2 = LedgerTestFixture::new(account_id).await;

        let effect_apis = vec![effect_api1, effect_api2];
        let consistent = effect_api_helpers::verify_effect_api_consistency(&effect_apis)
            .await
            .expect("consistency check failed");

        assert!(consistent);
    }

    #[tokio::test]
    async fn test_effect_api_helpers() {
        let effect_api = effect_api_helpers::test_effect_api().await;
        assert!(!effect_api.account_id().0.is_nil());
    }

    #[tokio::test]
    async fn test_effect_api_scenario_configs() {
        let configs = vec![
            "single-device",
            "dual-device",
            "three-device",
            "threshold-2-3",
            "threshold-3-5",
            "distributed-4",
        ];

        for config_name in configs {
            let (device_count, threshold) =
                effect_api_helpers::effect_api_config_for_scenario(config_name);
            assert!(device_count > 0);
            assert!(threshold > 0);
            assert!(threshold <= device_count as u16);
        }
    }
}
