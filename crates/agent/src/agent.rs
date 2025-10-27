//! Unified Agent Implementation
//!
//! This module provides a generic Agent implementation.
//!
//! The unified agent uses session types for compile-time state safety and
//! generic transport/storage abstractions for testability.

use aura_types::{AccountId, AccountIdExt, DeviceId, DeviceIdExt, GuardianId};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Effects;

impl Effects {
    pub fn test() -> Self {
        Self
    }
}

impl aura_types::EffectsLike for Effects {
    fn gen_uuid(&self) -> Uuid {
        Uuid::new_v4()
    }
}
use session_types::{SessionProtocol, SessionState};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

// Temporary placeholder until coordination crate is fixed
#[derive(Debug, Clone)]
pub struct KeyShare {
    pub device_id: DeviceId,
    pub share_data: Vec<u8>,
}

impl Default for KeyShare {
    fn default() -> Self {
        Self {
            device_id: DeviceId::new_with_effects(&Effects::test()),
            share_data: vec![0u8; 32],
        }
    }
}
// use aura_journal::AccountLedger;  // Temporarily disabled
// Minimal stub for AccountLedger
#[derive(Debug, Clone)]
pub struct AccountLedger;

impl AccountLedger {
    pub fn new(_account_state: AccountState) -> crate::Result<Self> {
        Ok(Self)
    }
}

// Minimal stub for AccountState
#[derive(Debug, Clone)]
pub struct AccountState;

impl AccountState {
    pub fn new(
        _account_id: AccountId,
        _verifying_key: ed25519_dalek::VerifyingKey,
        _device_metadata: DeviceMetadata,
        _threshold: u16,
        _share_count: u16,
    ) -> Self {
        Self
    }
}

// Minimal stub for DeviceMetadata
#[derive(Debug, Clone)]
pub struct DeviceMetadata {
    pub device_id: DeviceId,
    pub device_name: String,
    pub device_type: DeviceType,
    pub public_key: ed25519_dalek::VerifyingKey,
    pub added_at: u64,
    pub last_seen: u64,
    pub dkd_commitment_proofs: std::collections::HashMap<String, String>,
    pub next_nonce: u64,
    pub used_nonces: std::collections::HashSet<u64>,
}

// Minimal stub for DeviceType
#[derive(Debug, Clone)]
pub enum DeviceType {
    Native,
    Web,
    Mobile,
}

use crate::traits::{
    Agent, CoordinatingAgent, GroupAgent, IdentityAgent, NetworkAgent, StorageAgent,
};
use crate::{AgentError, DerivedIdentity, Result};
use async_trait::async_trait;

/// Transport abstraction for agent communication
#[async_trait]
pub trait Transport: Send + Sync + 'static {
    /// Get the device ID for this transport
    fn device_id(&self) -> DeviceId;

    /// Send a message to a peer
    async fn send_message(&self, peer_id: DeviceId, message: &[u8]) -> Result<()>;

    /// Receive messages (non-blocking)
    async fn receive_messages(&self) -> Result<Vec<(DeviceId, Vec<u8>)>>;

    /// Connect to a peer
    async fn connect(&self, peer_id: DeviceId) -> Result<()>;

    /// Disconnect from a peer
    async fn disconnect(&self, peer_id: DeviceId) -> Result<()>;

    /// Get list of connected peers
    async fn connected_peers(&self) -> Result<Vec<DeviceId>>;

    /// Check if connected to a specific peer
    async fn is_connected(&self, peer_id: DeviceId) -> Result<bool>;
}

/// Storage abstraction for agent persistence
#[async_trait]
pub trait Storage: Send + Sync + 'static {
    /// Get the account ID for this storage
    fn account_id(&self) -> AccountId;

    /// Store data with a given key
    async fn store(&self, key: &str, data: &[u8]) -> Result<()>;

    /// Retrieve data by key
    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>>;

    /// Delete data by key
    async fn delete(&self, key: &str) -> Result<()>;

    /// List all keys
    async fn list_keys(&self) -> Result<Vec<String>>;

    /// Check if a key exists
    async fn exists(&self, key: &str) -> Result<bool>;

    /// Get storage statistics
    async fn stats(&self) -> Result<StorageStats>;
}

/// Storage statistics
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StorageStats {
    pub total_keys: u64,
    pub total_size_bytes: u64,
    pub available_space_bytes: Option<u64>,
}

/// The core data and dependencies that persist across all agent states
pub struct AgentCore<T: Transport, S: Storage> {
    /// Unique identifier for this device
    pub device_id: DeviceId,
    /// Account identifier this agent belongs to
    pub account_id: AccountId,
    /// Threshold key share for cryptographic operations
    pub key_share: Arc<RwLock<KeyShare>>,
    /// CRDT-based account ledger for state management
    pub ledger: Arc<RwLock<AccountLedger>>,
    /// Transport layer for network communication
    pub transport: Arc<T>,
    /// Storage layer for persistence
    pub storage: Arc<S>,
    /// Session runtime for choreographic protocols
    // session_runtime: Arc<session_types::LocalSessionRuntime>,  // Disabled until session_types is fixed
    /// Injectable effects for deterministic testing
    pub effects: Effects,
}

impl<T: Transport, S: Storage> AgentCore<T, S> {
    /// Create a new agent core with the provided dependencies
    pub fn new(
        device_id: DeviceId,
        account_id: AccountId,
        key_share: KeyShare,
        ledger: AccountLedger,
        transport: Arc<T>,
        storage: Arc<S>,
        // session_runtime: Arc<session_types::LocalSessionRuntime>,  // Disabled until session_types is fixed
        effects: Effects,
    ) -> Self {
        Self {
            device_id,
            account_id,
            key_share: Arc::new(RwLock::new(key_share)),
            ledger: Arc::new(RwLock::new(ledger)),
            transport,
            storage,
            effects,
        }
    }

    /// Get the device ID (available in all states)
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Get the account ID (available in all states)
    pub fn account_id(&self) -> AccountId {
        self.account_id
    }
}

/// Agent session states (manual implementation)
#[derive(Debug, Clone)]
pub struct Uninitialized;

#[derive(Debug, Clone)]
pub struct Idle;

#[derive(Debug, Clone)]
pub struct Coordinating;

#[derive(Debug, Clone)]
pub struct Failed;

impl SessionState for Uninitialized {
    const NAME: &'static str = "Uninitialized";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

impl SessionState for Idle {
    const NAME: &'static str = "Idle";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = true;
}

impl SessionState for Coordinating {
    const NAME: &'static str = "Coordinating";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

impl SessionState for Failed {
    const NAME: &'static str = "Failed";
    const IS_FINAL: bool = true;
    const CAN_TERMINATE: bool = true;
}

/// Session-typed agent protocol
/// Generic over Transport, Storage, and State
pub struct AgentProtocol<T: Transport, S: Storage, State: SessionState> {
    pub inner: AgentCore<T, S>,
    _state: std::marker::PhantomData<State>,
}

impl<T: Transport, S: Storage, State: SessionState> AgentProtocol<T, S, State> {
    /// Create a new agent protocol instance
    pub fn new(core: AgentCore<T, S>) -> Self {
        Self {
            inner: core,
            _state: std::marker::PhantomData,
        }
    }

    /// Transition to a new state (type-safe state transitions)
    pub fn transition_to<NewState: SessionState>(self) -> AgentProtocol<T, S, NewState> {
        AgentProtocol {
            inner: self.inner,
            _state: std::marker::PhantomData,
        }
    }

    /// Get the device ID (available in all states)
    pub fn device_id(&self) -> DeviceId {
        self.inner.device_id()
    }

    /// Get the account ID (available in all states)
    pub fn account_id(&self) -> AccountId {
        self.inner.account_id()
    }
}

/// Type alias for the concrete unified agent
pub type UnifiedAgent<T, S> = AgentProtocol<T, S, Uninitialized>;

/// Configuration for bootstrapping a new agent
#[derive(Debug, Clone)]
pub struct BootstrapConfig {
    /// Initial threshold for key shares
    pub threshold: u16,
    /// Total number of shares
    pub share_count: u16,
    /// Additional configuration parameters
    pub parameters: std::collections::HashMap<String, String>,
}

/// Status of a running protocol
#[derive(Debug, Clone)]
pub enum ProtocolStatus {
    /// Protocol is still running
    InProgress,
    /// Protocol completed successfully
    Completed,
    /// Protocol failed with error
    Failed(String),
}

/// Witness that a protocol has completed successfully
#[derive(Debug)]
pub struct ProtocolCompleted {
    pub protocol_id: uuid::Uuid,
    pub result: serde_json::Value,
}

// Implementation for Uninitialized state
impl<T: Transport, S: Storage> AgentProtocol<T, S, Uninitialized> {
    /// Create a new uninitialized agent
    pub fn new_uninitialized(core: AgentCore<T, S>) -> Self {
        Self::new(core)
    }

    /// Bootstrap the agent with initial configuration
    ///
    /// This consumes the uninitialized agent and returns an idle agent
    pub async fn bootstrap(self, config: BootstrapConfig) -> Result<AgentProtocol<T, S, Idle>> {
        // TODO: Implement bootstrap logic
        // - Initialize key shares
        // - Set up ledger
        // - Configure session runtime

        tracing::info!(
            device_id = %self.inner.device_id,
            account_id = %self.inner.account_id,
            "Bootstrapping agent with config: {:?}", config
        );

        // For now, just transition to idle state
        // Real implementation would perform actual bootstrap operations
        Ok(self.transition_to())
    }
}

// Implementation for Idle state - this is the main operational state
impl<T: Transport, S: Storage> AgentProtocol<T, S, Idle> {
    /// Derive a new identity for a specific context
    pub async fn derive_identity(
        &self,
        app_id: &str,
        context: &str,
    ) -> Result<crate::DerivedIdentity> {
        // TODO: Implement identity derivation using DKD
        tracing::info!(
            device_id = %self.inner.device_id,
            app_id = app_id,
            context = context,
            "Deriving identity"
        );

        // Placeholder implementation
        Err(crate::error::AuraError::not_implemented("derive_identity"))
    }

    /// Store data with capability-based access control
    pub async fn store_data(&self, data: &[u8], capabilities: Vec<String>) -> Result<String> {
        // TODO: Implement capability-protected storage
        tracing::info!(
            device_id = %self.inner.device_id,
            data_len = data.len(),
            capabilities = ?capabilities,
            "Storing data"
        );

        // Placeholder implementation
        Err(crate::error::AuraError::not_implemented("store_data"))
    }

    /// Retrieve data with capability verification
    pub async fn retrieve_data(&self, data_id: &str) -> Result<Vec<u8>> {
        // TODO: Implement capability-protected retrieval
        tracing::info!(
            device_id = %self.inner.device_id,
            data_id = data_id,
            "Retrieving data"
        );

        // Placeholder implementation
        Err(crate::error::AuraError::not_implemented("retrieve_data"))
    }

    /// Initiate a recovery protocol
    ///
    /// This consumes the idle agent and returns a coordinating agent
    pub async fn initiate_recovery(
        self,
        recovery_params: serde_json::Value,
    ) -> Result<AgentProtocol<T, S, Coordinating>> {
        tracing::info!(
            device_id = %self.inner.device_id,
            "Initiating recovery protocol"
        );

        // TODO: Start recovery session in session runtime
        // let session_id = self.inner.session_runtime.start_recovery_session(recovery_params).await?;

        // Transition to coordinating state
        Ok(self.transition_to())
    }

    /// Initiate a resharing protocol
    ///
    /// This consumes the idle agent and returns a coordinating agent
    pub async fn initiate_resharing(
        self,
        new_threshold: u16,
        new_participants: Vec<DeviceId>,
    ) -> Result<AgentProtocol<T, S, Coordinating>> {
        tracing::info!(
            device_id = %self.inner.device_id,
            new_threshold = new_threshold,
            new_participants = ?new_participants,
            "Initiating resharing protocol"
        );

        // TODO: Start resharing session in session runtime

        // Transition to coordinating state
        Ok(self.transition_to())
    }
}

// Implementation for Coordinating state - restricted API while protocol runs
impl<T: Transport, S: Storage> AgentProtocol<T, S, Coordinating> {
    /// Check the status of the currently running protocol
    pub async fn check_protocol_status(&self) -> Result<ProtocolStatus> {
        // TODO: Query session runtime for protocol status
        tracing::debug!(
            device_id = %self.inner.device_id,
            "Checking protocol status"
        );

        // Placeholder implementation
        Ok(ProtocolStatus::InProgress)
    }

    /// Complete the coordination and return to idle state
    ///
    /// Requires a witness proving the protocol completed successfully
    pub fn finish_coordination(self, witness: ProtocolCompleted) -> AgentProtocol<T, S, Idle> {
        tracing::info!(
            device_id = %self.inner.device_id,
            protocol_id = %witness.protocol_id,
            "Finishing coordination with witness"
        );

        // TODO: Verify witness and clean up protocol state

        // Transition back to idle state
        self.transition_to()
    }

    /// Cancel the running protocol and return to idle state
    pub async fn cancel_coordination(self) -> Result<AgentProtocol<T, S, Idle>> {
        tracing::warn!(
            device_id = %self.inner.device_id,
            "Cancelling coordination protocol"
        );

        // TODO: Cancel protocol in session runtime

        // Transition back to idle state
        Ok(self.transition_to())
    }
}

// Implementation for Failed state
impl<T: Transport, S: Storage> AgentProtocol<T, S, Failed> {
    /// Get the error that caused the failure
    pub fn get_failure_reason(&self) -> String {
        // TODO: Store and retrieve actual failure reason
        "Agent failed".to_string()
    }

    /// Attempt to recover from failure
    ///
    /// This may succeed and return to Uninitialized state for re-bootstrap
    pub async fn attempt_recovery(self) -> Result<AgentProtocol<T, S, Uninitialized>> {
        tracing::info!(
            device_id = %self.inner.device_id,
            "Attempting recovery from failed state"
        );

        // TODO: Implement recovery logic

        // If recovery succeeds, return to uninitialized for re-bootstrap
        Ok(self.transition_to())
    }
}

// Implement Agent trait for Idle state
#[async_trait]
impl<T: Transport, S: Storage> Agent for AgentProtocol<T, S, Idle> {
    async fn derive_identity(&self, app_id: &str, context: &str) -> Result<DerivedIdentity> {
        self.derive_identity(app_id, context).await
    }

    async fn store_data(&self, data: &[u8], capabilities: Vec<String>) -> Result<String> {
        self.store_data(data, capabilities).await
    }

    async fn retrieve_data(&self, data_id: &str) -> Result<Vec<u8>> {
        self.retrieve_data(data_id).await
    }

    fn device_id(&self) -> DeviceId {
        self.inner.device_id()
    }

    fn account_id(&self) -> AccountId {
        self.inner.account_id()
    }
}

// Implement CoordinatingAgent trait for Idle state
#[async_trait]
impl<T: Transport, S: Storage> CoordinatingAgent for AgentProtocol<T, S, Idle> {
    async fn initiate_recovery(&mut self, recovery_params: serde_json::Value) -> Result<()> {
        // Note: This is a simplified version that doesn't consume self
        // In practice, you might want to track state differently
        tracing::info!(
            device_id = %self.inner.device_id,
            "Initiating recovery protocol (trait implementation)"
        );
        // TODO: Implement without consuming self
        Err(crate::error::AuraError::not_implemented(
            "initiate_recovery via trait",
        ))
    }

    async fn initiate_resharing(
        &mut self,
        new_threshold: u16,
        new_participants: Vec<DeviceId>,
    ) -> Result<()> {
        tracing::info!(
            device_id = %self.inner.device_id,
            new_threshold = new_threshold,
            "Initiating resharing protocol (trait implementation)"
        );
        // TODO: Implement without consuming self
        Err(crate::error::AuraError::not_implemented(
            "initiate_resharing via trait",
        ))
    }

    async fn check_protocol_status(&self) -> Result<ProtocolStatus> {
        // This doesn't make sense for Idle state as there's no running protocol
        Ok(ProtocolStatus::Completed)
    }
}

// Implement Agent trait for Coordinating state (limited functionality)
#[async_trait]
impl<T: Transport, S: Storage> Agent for AgentProtocol<T, S, Coordinating> {
    async fn derive_identity(&self, _app_id: &str, _context: &str) -> Result<DerivedIdentity> {
        // Identity derivation might be allowed during coordination
        Err(crate::error::AuraError::agent_invalid_state(
            "Cannot derive identity while coordinating",
        ))
    }

    async fn store_data(&self, _data: &[u8], _capabilities: Vec<String>) -> Result<String> {
        // Storage operations might be restricted during coordination
        Err(crate::error::AuraError::agent_invalid_state(
            "Cannot store data while coordinating",
        ))
    }

    async fn retrieve_data(&self, _data_id: &str) -> Result<Vec<u8>> {
        // Retrieval might be allowed during coordination
        Err(crate::error::AuraError::agent_invalid_state(
            "Cannot retrieve data while coordinating",
        ))
    }

    fn device_id(&self) -> DeviceId {
        self.inner.device_id()
    }

    fn account_id(&self) -> AccountId {
        self.inner.account_id()
    }
}

// Implement CoordinatingAgent trait for Coordinating state
#[async_trait]
impl<T: Transport, S: Storage> CoordinatingAgent for AgentProtocol<T, S, Coordinating> {
    async fn initiate_recovery(&mut self, _recovery_params: serde_json::Value) -> Result<()> {
        Err(crate::error::AuraError::agent_invalid_state(
            "Cannot initiate recovery while already coordinating",
        ))
    }

    async fn initiate_resharing(
        &mut self,
        _new_threshold: u16,
        _new_participants: Vec<DeviceId>,
    ) -> Result<()> {
        Err(crate::error::AuraError::agent_invalid_state(
            "Cannot initiate resharing while already coordinating",
        ))
    }

    async fn check_protocol_status(&self) -> Result<ProtocolStatus> {
        self.check_protocol_status().await
    }
}

/// Factory for creating unified agents with different configurations
pub struct AgentFactory;

impl AgentFactory {
    /// Create a production agent with real transport and storage
    pub async fn create_production<T: Transport, S: Storage>(
        device_id: DeviceId,
        account_id: AccountId,
        transport: Arc<T>,
        storage: Arc<S>,
    ) -> Result<UnifiedAgent<T, S>> {
        // TODO: Initialize real key share and ledger from storage
        let key_share = KeyShare::default();
        // Create placeholder account state for now
        use ed25519_dalek::VerifyingKey;
        let dummy_key_bytes = [0u8; 32];
        let verifying_key = VerifyingKey::from_bytes(&dummy_key_bytes).unwrap(); // TODO: use actual key
        let device_metadata = DeviceMetadata {
            device_id,
            device_name: "test-device".to_string(),
            device_type: DeviceType::Native,
            public_key: verifying_key,
            added_at: 0,
            last_seen: 0,
            dkd_commitment_proofs: Default::default(),
            next_nonce: 0,
            used_nonces: Default::default(),
        };
        let threshold = 2;
        let share_count = 3;
        let account_state = AccountState::new(
            account_id,
            verifying_key,
            device_metadata,
            threshold,
            share_count,
        );
        let ledger = AccountLedger::new(account_state)?;
        // let session_runtime = Arc::new(session_types::LocalSessionRuntime::new());  // Disabled until session_types is fixed
        let effects = Effects::test();

        let core = AgentCore::new(
            device_id, account_id, key_share, ledger, transport, storage, effects,
        );

        Ok(UnifiedAgent::new_uninitialized(core))
    }

    /// Create a test agent with mock transport and storage
    #[cfg(test)]
    pub async fn create_test(
        device_id: DeviceId,
        account_id: AccountId,
    ) -> Result<UnifiedAgent<impl Transport, impl Storage>> {
        // Use the mock implementations for testing
        let transport = Arc::new(tests::MockTransport::new(device_id));
        let storage = Arc::new(tests::MockStorage::new(account_id));

        Self::create_production(device_id, account_id, transport, storage).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    // Import mock implementations from tests/mocks.rs
    // Note: In integration tests, use: use aura_agent::test_utils::mocks::{MockTransport, MockStorage};
    // For unit tests in this module, we need to re-export or define minimal mocks here.
    // Since Rust doesn't allow importing from tests/ in src/ files, we keep minimal inline mocks
    // for the create_test factory method, but the detailed mock tests are in tests/mocks.rs

    #[derive(Debug)]
    pub struct MockTransport {
        device_id: DeviceId,
    }

    impl MockTransport {
        pub fn new(device_id: DeviceId) -> Self {
            Self { device_id }
        }
    }

    #[async_trait]
    impl Transport for MockTransport {
        fn device_id(&self) -> DeviceId {
            self.device_id
        }

        async fn send_message(&self, _peer_id: DeviceId, _message: &[u8]) -> Result<()> {
            Ok(())
        }

        async fn receive_messages(&self) -> Result<Vec<(DeviceId, Vec<u8>)>> {
            Ok(Vec::new())
        }

        async fn connect(&self, _peer_id: DeviceId) -> Result<()> {
            Ok(())
        }

        async fn disconnect(&self, _peer_id: DeviceId) -> Result<()> {
            Ok(())
        }

        async fn connected_peers(&self) -> Result<Vec<DeviceId>> {
            Ok(Vec::new())
        }

        async fn is_connected(&self, _peer_id: DeviceId) -> Result<bool> {
            Ok(false)
        }
    }

    #[derive(Debug)]
    pub struct MockStorage {
        account_id: AccountId,
    }

    impl MockStorage {
        pub fn new(account_id: AccountId) -> Self {
            Self { account_id }
        }
    }

    #[async_trait]
    impl Storage for MockStorage {
        fn account_id(&self) -> AccountId {
            self.account_id
        }

        async fn store(&self, _key: &str, _data: &[u8]) -> Result<()> {
            Ok(())
        }

        async fn retrieve(&self, _key: &str) -> Result<Option<Vec<u8>>> {
            Ok(None)
        }

        async fn delete(&self, _key: &str) -> Result<()> {
            Ok(())
        }

        async fn list_keys(&self) -> Result<Vec<String>> {
            Ok(Vec::new())
        }

        async fn exists(&self, _key: &str) -> Result<bool> {
            Ok(false)
        }

        async fn stats(&self) -> Result<StorageStats> {
            Ok(StorageStats {
                total_keys: 0,
                total_size_bytes: 0,
                available_space_bytes: Some(1_000_000_000),
            })
        }
    }

    #[tokio::test]
    async fn test_agent_state_transitions() {
        // Create mock dependencies
        let device_id = DeviceId(Uuid::new_v4());
        let account_id = AccountId::new(Uuid::new_v4());
        let transport = Arc::new(MockTransport::new(device_id));
        let storage = Arc::new(MockStorage::new(account_id));

        // Create minimal test dependencies
        let key_share = KeyShare::default();
        use ed25519_dalek::VerifyingKey;
        let dummy_key_bytes = [0u8; 32];
        let verifying_key = VerifyingKey::from_bytes(&dummy_key_bytes).unwrap();
        let device_metadata = DeviceMetadata {
            device_id,
            device_name: "test-device".to_string(),
            device_type: DeviceType::Native,
            public_key: verifying_key,
            added_at: 0,
            last_seen: 0,
            dkd_commitment_proofs: Default::default(),
            next_nonce: 0,
            used_nonces: Default::default(),
        };
        let threshold = 2;
        let share_count = 3;
        let account_state = AccountState::new(
            account_id,
            verifying_key,
            device_metadata,
            threshold,
            share_count,
        );
        let ledger = AccountLedger::new(account_state).unwrap();
        let effects = Effects::test();

        // Create agent core
        let core = AgentCore::new(
            device_id, account_id, key_share, ledger, transport, storage, effects,
        );

        // 1. Start with uninitialized agent
        let uninit_agent = UnifiedAgent::new_uninitialized(core);

        // Verify we can access common methods
        assert_eq!(uninit_agent.device_id(), device_id);
        assert_eq!(uninit_agent.account_id(), account_id);

        // 2. Bootstrap to idle state
        let bootstrap_config = BootstrapConfig {
            threshold: 2,
            share_count: 3,
            parameters: std::collections::HashMap::new(),
        };

        let idle_agent = uninit_agent.bootstrap(bootstrap_config).await.unwrap();

        // 3. Try to initiate recovery (transitions to coordinating)
        let coordinating_agent = idle_agent
            .initiate_recovery(serde_json::json!({}))
            .await
            .unwrap();

        // 4. Check protocol status
        let status = coordinating_agent.check_protocol_status().await.unwrap();
        assert!(matches!(status, ProtocolStatus::InProgress));

        // 5. Finish coordination (back to idle)
        let witness = ProtocolCompleted {
            protocol_id: Uuid::new_v4(),
            result: serde_json::json!({"success": true}),
        };

        let _idle_agent_again = coordinating_agent.finish_coordination(witness);

        // This test demonstrates the compile-time safety:
        // - Can't call store_data() on uninitialized agent (won't compile)
        // - Can't call initiate_recovery() on coordinating agent (won't compile)
        // - Must follow the state transition protocol
    }

    #[tokio::test]
    async fn test_agent_factory() {
        let device_id = DeviceId(Uuid::new_v4());
        let account_id = AccountId::new(Uuid::new_v4());

        // Test factory creation
        let uninit_agent = AgentFactory::create_test(device_id, account_id)
            .await
            .unwrap();

        // Verify IDs are correct
        assert_eq!(uninit_agent.device_id(), device_id);
        assert_eq!(uninit_agent.account_id(), account_id);

        // Bootstrap agent
        let config = BootstrapConfig {
            threshold: 1,
            share_count: 1,
            parameters: Default::default(),
        };
        let idle_agent = uninit_agent.bootstrap(config).await.unwrap();

        // Verify agent is operational
        assert_eq!(idle_agent.device_id(), device_id);
        assert_eq!(idle_agent.account_id(), account_id);
    }
}
