//! Ledger test helpers and utilities
//!
//! This module provides standardized helpers for creating and managing test ledgers
//! (CRDT-based account ledgers) across the Aura test suite.

use aura_core::{AccountId, DeviceId};
use aura_core::effects::{RandomEffects, TimeEffects};
use crate::Effects;
use aura_journal::semilattice::ModernAccountState as AccountState;
use aura_journal::{DeviceMetadata, DeviceType};
use ed25519_dalek::SigningKey;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Simple AccountLedger replacement for testing
/// This wraps AccountState to provide the expected interface for tests
#[derive(Debug, Clone)]
pub struct AccountLedger {
    state: AccountState,
}

impl AccountLedger {
    /// Create a new AccountLedger with the given initial state
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

/// Ledger test fixture for consistent test ledger creation
#[derive(Debug, Clone)]
pub struct LedgerTestFixture {
    account_id: AccountId,
    ledger: Arc<RwLock<AccountLedger>>,
    devices: Vec<DeviceId>,
}

impl LedgerTestFixture {
    /// Create a new ledger test fixture with a specific account ID
    pub async fn new(account_id: AccountId) -> Self {
        // Create a minimal AccountState for testing
        let effects = Effects::for_test("ledger_test");
        let key_bytes = effects.random_bytes_32().await;
        let signing_key = SigningKey::from_bytes(&key_bytes);
        let group_public_key = signing_key.verifying_key();

        // Generate deterministic UUID from random bytes
        let uuid_bytes = effects.random_bytes(16).await;
        let uuid_array: [u8; 16] = uuid_bytes.try_into().unwrap();
        let uuid = Uuid::from_bytes(uuid_array);

        let timestamp = effects.current_timestamp_millis().await;
        let device_metadata = DeviceMetadata {
            device_id: DeviceId(uuid),
            device_name: "Test Device".to_string(),
            device_type: DeviceType::Native,
            public_key: group_public_key,
            added_at: timestamp,
            last_seen: timestamp,
            dkd_commitment_proofs: Default::default(),
            next_nonce: 0,
            key_share_epoch: 0,
            used_nonces: Default::default(),
        };

        let mut initial_state = AccountState::new(account_id, group_public_key);
        initial_state.add_device(device_metadata);
        let ledger = Arc::new(RwLock::new(
            AccountLedger::new(initial_state).expect("Failed to create AccountLedger"),
        ));

        Self {
            account_id,
            ledger,
            devices: vec![],
        }
    }

    /// Create a random ledger fixture
    pub async fn random() -> Self {
        let hash_input = "ledger-fixture-random";
        let hash_bytes = blake3::hash(hash_input.as_bytes());
        let uuid = Uuid::from_bytes(hash_bytes.as_bytes()[..16].try_into().unwrap());
        let account_id = AccountId(uuid);
        Self::new(account_id).await
    }

    /// Get the account ID
    pub fn account_id(&self) -> AccountId {
        self.account_id
    }

    /// Get a reference to the ledger
    pub fn ledger(&self) -> Arc<RwLock<AccountLedger>> {
        Arc::clone(&self.ledger)
    }

    /// Get the list of registered devices
    pub fn devices(&self) -> &[DeviceId] {
        &self.devices
    }

    /// Add a device to the ledger
    pub async fn add_device(
        &mut self,
        device_id: DeviceId,
        device_type: DeviceType,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let effects = Effects::for_test("add_device");
        let key_bytes = effects.random_bytes_32().await;
        let signing_key = SigningKey::from_bytes(&key_bytes);
        let public_key = signing_key.verifying_key();

        let timestamp = effects.current_timestamp_millis().await;
        let _metadata = DeviceMetadata {
            device_id,
            device_name: format!("Device {:?}", device_id),
            device_type,
            public_key,
            added_at: timestamp,
            last_seen: timestamp,
            dkd_commitment_proofs: Default::default(),
            next_nonce: 0,
            key_share_epoch: 0,
            used_nonces: Default::default(),
        };

        let _ledger = self.ledger.write().await;
        // TODO fix - In a real implementation, would add device through proper ledger API
        // TODO fix - For now, track in our local device list
        self.devices.push(device_id);

        Ok(())
    }

    /// Get device metadata from the ledger
    pub async fn get_device_metadata(
        &self,
        device_id: DeviceId,
    ) -> Result<Option<DeviceMetadata>, Box<dyn std::error::Error>> {
        let ledger = self.ledger.read().await;
        let state = ledger.state();

        // Find device in account state device registry
        Ok(state.device_registry.devices.get(&device_id).cloned())
    }

    /// Get account state from the ledger
    pub async fn account_state(&self) -> Result<AccountState, Box<dyn std::error::Error>> {
        let ledger = self.ledger.read().await;
        Ok(ledger.state().clone())
    }
}

/// Builder for creating test ledgers with specific configuration
#[derive(Debug)]
pub struct LedgerBuilder {
    account_id: Option<AccountId>,
    device_count: usize,
    threshold: Option<u16>,
}

impl LedgerBuilder {
    /// Create a new ledger builder
    pub fn new() -> Self {
        Self {
            account_id: None,
            device_count: 1,
            threshold: None,
        }
    }

    /// Set a specific account ID
    pub fn with_account_id(mut self, account_id: AccountId) -> Self {
        self.account_id = Some(account_id);
        self
    }

    /// Set the number of devices to register in the ledger
    pub fn with_devices(mut self, count: usize) -> Self {
        self.device_count = count;
        self
    }

    /// Set the threshold for threshold cryptography
    pub fn with_threshold(mut self, threshold: u16) -> Self {
        self.threshold = Some(threshold);
        self
    }

    /// Build the ledger fixture
    pub async fn build(self) -> Result<LedgerTestFixture, Box<dyn std::error::Error>> {
        let account_id = self.account_id.unwrap_or_else(|| {
            let hash_input = "ledger-builder-account";
            let hash_bytes = blake3::hash(hash_input.as_bytes());
            let uuid = Uuid::from_bytes(hash_bytes.as_bytes()[..16].try_into().unwrap());
            AccountId(uuid)
        });

        let mut fixture = LedgerTestFixture::new(account_id).await;

        // Add devices to the ledger
        for i in 0..self.device_count {
            let hash_input = format!("ledger-builder-device-{}", i);
            let hash_bytes = blake3::hash(hash_input.as_bytes());
            let uuid = Uuid::from_bytes(hash_bytes.as_bytes()[..16].try_into().unwrap());
            let device_id = DeviceId(uuid);
            let device_type = match i {
                0 => DeviceType::Native,
                _ => DeviceType::Browser,
            };

            fixture.add_device(device_id, device_type).await?;
        }

        Ok(fixture)
    }
}

impl Default for LedgerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Common test ledger creation helpers
/// Helper functions for creating test ledgers
pub mod ledger_helpers {
    use super::*;

    /// Create a single test ledger with default configuration
    pub async fn test_ledger() -> LedgerTestFixture {
        LedgerTestFixture::random().await
    }

    /// Create a test ledger with a specific account ID
    pub async fn test_ledger_for_account(account_id: AccountId) -> LedgerTestFixture {
        LedgerTestFixture::new(account_id).await
    }

    /// Create a test ledger with multiple devices
    pub async fn test_ledger_with_devices(
        device_count: usize,
    ) -> Result<LedgerTestFixture, Box<dyn std::error::Error>> {
        LedgerBuilder::new()
            .with_devices(device_count)
            .build()
            .await
    }

    /// Create a test ledger for a threshold scenario
    pub async fn test_ledger_threshold(
        device_count: usize,
        threshold: u16,
    ) -> Result<LedgerTestFixture, Box<dyn std::error::Error>> {
        LedgerBuilder::new()
            .with_devices(device_count)
            .with_threshold(threshold)
            .build()
            .await
    }

    /// Create a two-device ledger
    pub async fn test_ledger_pair(
    ) -> Result<(LedgerTestFixture, LedgerTestFixture), Box<dyn std::error::Error>> {
        let hash_input = "ledger-pair";
        let hash_bytes = blake3::hash(hash_input.as_bytes());
        let uuid = Uuid::from_bytes(hash_bytes.as_bytes()[..16].try_into().unwrap());
        let account_id = AccountId(uuid);
        let ledger1 = LedgerBuilder::new()
            .with_account_id(account_id)
            .with_devices(2)
            .build()
            .await?;

        let ledger2 = LedgerTestFixture::new(account_id).await;

        Ok((ledger1, ledger2))
    }

    /// Create a three-device ledger
    pub async fn test_ledger_trio(
    ) -> Result<(LedgerTestFixture, LedgerTestFixture, LedgerTestFixture), Box<dyn std::error::Error>>
    {
        let hash_input = "ledger-trio";
        let hash_bytes = blake3::hash(hash_input.as_bytes());
        let uuid = Uuid::from_bytes(hash_bytes.as_bytes()[..16].try_into().unwrap());
        let account_id = AccountId(uuid);
        let _ledger = LedgerBuilder::new()
            .with_account_id(account_id)
            .with_devices(3)
            .build()
            .await?;

        Ok((
            LedgerTestFixture::new(account_id).await,
            LedgerTestFixture::new(account_id).await,
            LedgerTestFixture::new(account_id).await,
        ))
    }

    /// Verify ledger consistency across multiple instances
    pub async fn verify_ledger_consistency(
        ledgers: &[LedgerTestFixture],
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if ledgers.is_empty() {
            return Ok(true);
        }

        let first_account = ledgers[0].account_id();

        for ledger in ledgers {
            if ledger.account_id() != first_account {
                return Ok(false);
            }

            let state = ledger.account_state().await?;
            if state.account_id != first_account {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Get standard ledger configuration for common scenarios
    pub fn ledger_config_for_scenario(scenario: &str) -> (usize, u16) {
        match scenario {
            "single-device" => (1, 1),
            "dual-device" => (2, 1),
            "three-device" => (3, 2),
            "threshold-2-3" => (3, 2),
            "threshold-3-5" => (5, 3),
            "distributed-4" => (4, 2),
            _ => (1, 1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ledger_fixture_creation() {
        let ledger = LedgerTestFixture::random().await;
        assert_eq!(ledger.devices().len(), 0);
    }

    #[tokio::test]
    async fn test_ledger_builder() {
        let ledger = LedgerBuilder::new()
            .with_devices(3)
            .build()
            .await
            .expect("failed to build ledger");

        assert_eq!(ledger.devices().len(), 3);
    }

    #[tokio::test]
    async fn test_ledger_with_threshold() {
        let ledger = LedgerBuilder::new()
            .with_devices(5)
            .with_threshold(3)
            .build()
            .await
            .expect("failed to build ledger");

        assert_eq!(ledger.devices().len(), 5);
    }

    #[tokio::test]
    async fn test_ledger_consistency() {
        let account_id = AccountId::new();
        let ledger1 = LedgerTestFixture::new(account_id).await;
        let ledger2 = LedgerTestFixture::new(account_id).await;

        let ledgers = vec![ledger1, ledger2];
        let consistent = ledger_helpers::verify_ledger_consistency(&ledgers)
            .await
            .expect("consistency check failed");

        assert!(consistent);
    }

    #[tokio::test]
    async fn test_ledger_helpers() {
        let ledger = ledger_helpers::test_ledger().await;
        assert!(!ledger.account_id().0.is_nil());

        let ledger_with_devices = ledger_helpers::test_ledger_with_devices(3)
            .await
            .expect("failed to create ledger");
        assert_eq!(ledger_with_devices.devices().len(), 3);
    }

    #[tokio::test]
    async fn test_ledger_scenario_configs() {
        let configs = vec![
            "single-device",
            "dual-device",
            "three-device",
            "threshold-2-3",
            "threshold-3-5",
            "distributed-4",
        ];

        for config_name in configs {
            let (device_count, threshold) = ledger_helpers::ledger_config_for_scenario(config_name);
            assert!(device_count > 0);
            assert!(threshold > 0);
            assert!(threshold <= device_count as u16);
        }
    }
}
