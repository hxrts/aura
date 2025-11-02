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
        // Create a mock handler for testing since transport features are disabled
        Ok(Box::new(MockProtocolHandler))
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

/// Mock protocol handler for testing
struct MockProtocolHandler;

#[async_trait::async_trait]
impl AuraProtocolHandler for MockProtocolHandler {
    type DeviceId = aura_types::DeviceId;
    type SessionId = uuid::Uuid;
    type Message = Vec<u8>;

    async fn send_message(
        &mut self,
        _to: Self::DeviceId,
        _msg: Self::Message,
    ) -> aura_protocol::ProtocolResult<()> {
        Ok(())
    }

    async fn receive_message(
        &mut self,
        _from: Self::DeviceId,
    ) -> aura_protocol::ProtocolResult<Self::Message> {
        Ok(vec![])
    }

    async fn start_session(
        &mut self,
        _participants: Vec<Self::DeviceId>,
        _protocol_type: String,
        _metadata: std::collections::HashMap<String, String>,
    ) -> aura_protocol::ProtocolResult<Self::SessionId> {
        // Generate deterministic session ID from timestamp
        let hash_input = format!(
            "session-{}",
            aura_types::time_utils::current_unix_timestamp()
        );
        let hash_bytes = blake3::hash(hash_input.as_bytes());
        Ok(uuid::Uuid::from_bytes(
            hash_bytes.as_bytes()[..16].try_into().unwrap(),
        ))
    }

    async fn end_session(
        &mut self,
        _session_id: Self::SessionId,
    ) -> aura_protocol::ProtocolResult<()> {
        Ok(())
    }

    async fn get_session_info(
        &mut self,
        session_id: Self::SessionId,
    ) -> aura_protocol::ProtocolResult<aura_protocol::middleware::handler::SessionInfo> {
        Ok(aura_protocol::middleware::handler::SessionInfo {
            session_id,
            participants: vec![],
            protocol_type: "mock".to_string(),
            started_at: 0,
            metadata: std::collections::HashMap::new(),
        })
    }

    async fn list_sessions(
        &mut self,
    ) -> aura_protocol::ProtocolResult<Vec<aura_protocol::middleware::handler::SessionInfo>> {
        Ok(vec![])
    }

    async fn verify_capability(
        &mut self,
        _operation: &str,
        _resource: &str,
        _context: std::collections::HashMap<String, String>,
    ) -> aura_protocol::ProtocolResult<bool> {
        Ok(true)
    }

    async fn create_authorization_proof(
        &mut self,
        _operation: &str,
        _resource: &str,
        _context: std::collections::HashMap<String, String>,
    ) -> aura_protocol::ProtocolResult<Vec<u8>> {
        Ok(vec![])
    }

    fn device_id(&self) -> Self::DeviceId {
        aura_types::DeviceId(uuid::Uuid::nil())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StorageStats;
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
        fn account_id(&self) -> AccountId {
            AccountId::new() // Mock implementation
        }
        async fn store(&self, key: &str, value: &[u8]) -> AuraResult<()> {
            let mut data = self.data.lock().unwrap();
            data.insert(key.to_string(), value.to_vec());
            Ok(())
        }

        async fn retrieve(&self, key: &str) -> AuraResult<Option<Vec<u8>>> {
            let data = self.data.lock().unwrap();
            Ok(data.get(key).cloned())
        }

        async fn delete(&self, key: &str) -> AuraResult<()> {
            let mut data = self.data.lock().unwrap();
            data.remove(key);
            Ok(())
        }

        async fn list_keys(&self) -> AuraResult<Vec<String>> {
            let data = self.data.lock().unwrap();
            Ok(data.keys().cloned().collect())
        }

        async fn exists(&self, key: &str) -> AuraResult<bool> {
            let data = self.data.lock().unwrap();
            Ok(data.contains_key(key))
        }

        async fn stats(&self) -> AuraResult<StorageStats> {
            let data = self.data.lock().unwrap();
            Ok(StorageStats {
                total_keys: data.len() as u64,
                total_size_bytes: data.values().map(|v| v.len() as u64).sum(),
                available_space_bytes: Some(1024 * 1024 * 1024), // Mock 1GB available
            })
        }
    }

    #[tokio::test]
    async fn test_agent_creation() {
        let device_id = DeviceId(uuid::Uuid::new_v4());
        let account_id = AccountId(uuid::Uuid::new_v4());
        let storage = Arc::new(MockStorage::new());

        let result = AgentFactory::create_test::<MockStorage>(device_id, account_id, storage);
        if let Err(ref e) = result {
            println!("Agent creation failed: {}", e);
        }
        assert!(result.is_ok());

        let agent = result.unwrap();
        assert_eq!(agent.device_id(), device_id);
        assert_eq!(agent.account_id(), account_id);
    }

    #[tokio::test]
    async fn test_agent_security_validation() {
        let device_id = DeviceId(uuid::Uuid::new_v4());
        let account_id = AccountId(uuid::Uuid::new_v4());
        let storage = Arc::new(MockStorage::new());

        let agent =
            AgentFactory::create_test::<MockStorage>(device_id, account_id, storage).unwrap();

        // Validate initial security state
        let report = agent.validate_security_state().await.unwrap();

        // Initially, the agent should have issues since it hasn't been bootstrapped
        assert!(!report.is_secure());
        assert!(report.has_critical_issues());
    }
}
