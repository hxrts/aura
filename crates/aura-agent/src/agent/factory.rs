//! Agent factory and construction
//!
//! This module provides factory methods for creating agents with various configurations.

use crate::agent::capabilities::KeyShare;
use crate::agent::core::AgentCore;
use crate::error::{AgentError, Result};
use crate::Storage;
use aura_crypto::Effects;
use aura_journal::{AccountLedger, AccountState};
use aura_protocol::prelude::*;
use aura_types::{AccountId, DeviceId};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Configuration for transport layer
#[derive(Debug, Clone)]
pub enum TransportConfig {
    /// In-memory transport for testing
    InMemory,
    /// Network transport with URL
    Network(String),
}

/// Factory for creating and configuring agents
pub struct AgentFactory;

impl AgentFactory {
    /// Create a basic agent with provided dependencies
    ///
    /// This creates an agent with the minimal required components. The agent will need
    /// to be bootstrapped before it can perform operations.
    pub fn create_with_dependencies<S: Storage>(
        device_id: DeviceId,
        account_id: AccountId,
        transport_config: TransportConfig,
        storage: Arc<S>,
    ) -> Result<AgentCore<S>> {
        use aura_journal::types::DeviceMetadata;
        use aura_journal::DeviceType;

        // Create initial key share (will be properly initialized during bootstrap)
        let key_share = KeyShare {
            device_id,
            share_data: vec![],
        };

        // Create effects for initialization
        let effects = Effects::production();

        // Generate device signing key
        let device_signing_key = aura_crypto::generate_ed25519_key();
        let device_public_key = device_signing_key.verifying_key();

        // Create initial device metadata
        let initial_device = DeviceMetadata {
            device_id,
            device_name: "Primary Device".to_string(),
            device_type: DeviceType::Native,
            public_key: device_public_key,
            added_at: effects.now().unwrap_or(0),
            last_seen: effects.now().unwrap_or(0),
            dkd_commitment_proofs: Default::default(),
            next_nonce: 1,
            key_share_epoch: 0,
            used_nonces: Default::default(),
        };

        // Create initial account state
        let initial_state = AccountState::new(
            account_id,
            device_public_key, // group_public_key
            initial_device,
            1, // threshold (will be updated during bootstrap)
            1, // total_participants (will be updated during bootstrap)
        );

        // Create account ledger
        let ledger = AccountLedger::new(initial_state).map_err(|e| {
            AgentError::agent_invalid_state(format!("Failed to create ledger: {}", e))
        })?;

        // Create protocol handler with middleware stack
        let protocol_handler = Self::create_protocol_handler(device_id, transport_config)?;

        // Create agent core
        let agent = AgentCore::new(
            device_id,
            account_id,
            key_share,
            Arc::new(RwLock::new(ledger)),
            storage,
            effects,
            protocol_handler,
        );

        Ok(agent)
    }

    /// Create a protocol handler with the standard middleware stack
    fn create_protocol_handler(
        _device_id: DeviceId,
        _transport_config: TransportConfig,
    ) -> Result<
        Box<
            dyn AuraProtocolHandler<
                    DeviceId = aura_types::DeviceId,
                    SessionId = uuid::Uuid,
                    Message = Vec<u8>,
                > + Send,
        >,
    > {
        // Transport feature removed - handler creation not currently supported
        Err(AgentError::agent_invalid_state(
            "Handler creation not currently supported".to_string(),
        ))
    }

    /// Create a test agent with in-memory transport for testing
    #[cfg(test)]
    pub fn create_test<S: Storage>(
        device_id: DeviceId,
        account_id: AccountId,
        storage: Arc<S>,
    ) -> Result<AgentCore<S>> {
        Self::create_with_dependencies(device_id, account_id, TransportConfig::InMemory, storage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_types::AuraResult;
    use std::collections::HashMap;

    /// Mock storage implementation for testing
    struct MockStorage {
        data: std::sync::Mutex<HashMap<String, Vec<u8>>>,
    }

    impl MockStorage {
        fn new() -> Self {
            Self {
                data: std::sync::Mutex::new(HashMap::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl Storage for MockStorage {
        async fn store(&self, key: &str, value: &[u8]) -> AuraResult<()> {
            let mut data = self.data.lock().unwrap();
            data.insert(key.to_string(), value.to_vec());
            Ok(())
        }

        async fn retrieve(&self, key: &str) -> AuraResult<Option<Vec<u8>>> {
            let data = self.data.lock().unwrap();
            Ok(data.get(key).cloned())
        }

        async fn delete(&self, key: &str) -> AuraResult<bool> {
            let mut data = self.data.lock().unwrap();
            Ok(data.remove(key).is_some())
        }

        async fn list_keys(&self, prefix: &str) -> AuraResult<Vec<String>> {
            let data = self.data.lock().unwrap();
            Ok(data
                .keys()
                .filter(|k| k.starts_with(prefix))
                .cloned()
                .collect())
        }
    }

    #[tokio::test]
    async fn test_agent_creation() {
        let device_id = DeviceId::from(uuid::Uuid::new_v4());
        let account_id = AccountId::from(uuid::Uuid::new_v4());
        let storage = Arc::new(MockStorage::new());

        let agent = AgentFactory::create_test(device_id, account_id, storage);
        assert!(agent.is_ok());

        let agent = agent.unwrap();
        assert_eq!(agent.device_id(), device_id);
        assert_eq!(agent.account_id(), account_id);
    }

    #[tokio::test]
    async fn test_agent_security_validation() {
        let device_id = DeviceId::from(uuid::Uuid::new_v4());
        let account_id = AccountId::from(uuid::Uuid::new_v4());
        let storage = Arc::new(MockStorage::new());

        let agent = AgentFactory::create_test(device_id, account_id, storage).unwrap();

        // Validate initial security state
        let report = agent.validate_security_state().await.unwrap();

        // Initially, the agent should have issues since it hasn't been bootstrapped
        assert!(!report.is_secure());
        assert!(report.has_critical_issues());
    }
}
