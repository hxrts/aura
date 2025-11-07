//! Composite effect handler
//!
//! Combines multiple effect handlers into a single unified handler that implements all effect traits.

use super::{
    console::{SilentConsoleHandler, StdoutConsoleHandler},
    context::AuraContext,
    crypto::{MockCryptoHandler, RealCryptoHandler},
    // time::{RealTimeHandler, SimulatedTimeHandler}, // Temporarily commented out
    erased::AuraHandler,
    network::{MemoryNetworkHandler, RealNetworkHandler, SimulatedNetworkHandler},
    storage::{FilesystemStorageHandler, MemoryStorageHandler},
    {AuraHandlerError, EffectType, ExecutionMode},
};
use crate::effects::*;
use async_trait::async_trait;
use aura_types::LocalSessionType;
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use serde_json;
use std::future::Future;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// Dummy time handler for stub implementation
/// Dummy time handler for testing and simulation
pub struct DummyTimeHandler;

impl DummyTimeHandler {
    /// Create a new dummy time handler
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl TimeEffects for DummyTimeHandler {
    async fn current_epoch(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    async fn current_timestamp(&self) -> u64 {
        self.current_epoch().await / 1000
    }

    async fn current_timestamp_millis(&self) -> u64 {
        self.current_epoch().await
    }

    async fn sleep_ms(&self, ms: u64) {
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }

    async fn sleep_until(&self, _epoch: u64) {
        // Stub implementation
    }

    async fn delay(&self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }

    async fn sleep(&self, duration_ms: u64) -> Result<(), aura_types::AuraError> {
        self.sleep_ms(duration_ms).await;
        Ok(())
    }

    async fn yield_until(&self, _condition: WakeCondition) -> Result<(), TimeError> {
        Ok(()) // Stub - immediately returns
    }

    async fn wait_until(&self, condition: WakeCondition) -> Result<(), aura_types::AuraError> {
        self.yield_until(condition).await.map_err(|e| {
            aura_types::AuraError::Infrastructure(aura_types::InfrastructureError::ConfigError {
                message: format!("Wait until failed: {}", e),
                context: "wait_until".to_string(),
            })
        })
    }

    async fn set_timeout(&self, _timeout_ms: u64) -> TimeoutHandle {
        Uuid::new_v4()
    }

    async fn cancel_timeout(&self, _handle: TimeoutHandle) -> Result<(), TimeError> {
        Ok(()) // Stub
    }

    // timeout method removed to make TimeEffects dyn-compatible
    // Use tokio::time::timeout directly where needed

    fn is_simulated(&self) -> bool {
        false // Not a simulation
    }

    fn register_context(&self, _context_id: Uuid) {
        // Stub
    }

    fn unregister_context(&self, _context_id: Uuid) {
        // Stub
    }

    async fn notify_events_available(&self) {
        // Stub
    }

    fn resolution_ms(&self) -> u64 {
        1 // 1ms resolution
    }
}

/// Composite handler that implements all effect traits
pub struct CompositeHandler {
    device_id: Uuid,
    is_simulation: bool,
    network: Box<dyn NetworkEffects>,
    storage: Box<dyn StorageEffects>,
    crypto: Box<dyn CryptoEffects>,
    time: DummyTimeHandler,
    console: Box<dyn ConsoleEffects>,
    // Note: LedgerEffects and ChoreographicEffects will be added when their handlers are implemented
}

impl CompositeHandler {
    /// Create a composite handler for testing with all mock/memory implementations
    pub fn for_testing(device_id: Uuid) -> Self {
        Self {
            device_id,
            is_simulation: true,
            network: Box::new(MemoryNetworkHandler::new(device_id)),
            storage: Box::new(MemoryStorageHandler::new()),
            crypto: Box::new(MockCryptoHandler::new(42)),
            time: DummyTimeHandler::new(),
            console: Box::new(SilentConsoleHandler::new()),
        }
    }

    /// Create a composite handler for production with real implementations
    pub fn for_production(device_id: Uuid) -> Self {
        Self {
            device_id,
            is_simulation: false,
            network: Box::new(RealNetworkHandler::new(
                device_id,
                "tcp://0.0.0.0:0".to_string(),
            )),
            storage: Box::new(FilesystemStorageHandler::new("/tmp/aura".into()).unwrap()),
            crypto: Box::new(RealCryptoHandler::new()),
            time: DummyTimeHandler::new(),
            console: Box::new(StdoutConsoleHandler::new()),
        }
    }

    /// Create a composite handler for simulation/deterministic testing
    pub fn for_simulation(device_id: Uuid) -> Self {
        Self {
            device_id,
            is_simulation: true,
            network: Box::new(SimulatedNetworkHandler::new(device_id)),
            storage: Box::new(MemoryStorageHandler::new()),
            crypto: Box::new(MockCryptoHandler::new(device_id.as_u128() as u64)),
            time: DummyTimeHandler::new(),
            console: Box::new(SilentConsoleHandler::new()),
        }
    }

    /// Builder for custom composite handlers
    pub fn builder(device_id: Uuid) -> CompositeHandlerBuilder {
        CompositeHandlerBuilder::new(device_id)
    }
}

// Implement all effect traits by delegating to the individual handlers

#[async_trait]
impl NetworkEffects for CompositeHandler {
    async fn send_to_peer(&self, peer_id: Uuid, message: Vec<u8>) -> Result<(), NetworkError> {
        self.network.send_to_peer(peer_id, message).await
    }

    async fn broadcast(&self, message: Vec<u8>) -> Result<(), NetworkError> {
        self.network.broadcast(message).await
    }

    async fn receive(&self) -> Result<(Uuid, Vec<u8>), NetworkError> {
        self.network.receive().await
    }

    async fn receive_from(&self, peer_id: Uuid) -> Result<Vec<u8>, NetworkError> {
        self.network.receive_from(peer_id).await
    }

    async fn connected_peers(&self) -> Vec<Uuid> {
        self.network.connected_peers().await
    }

    async fn is_peer_connected(&self, peer_id: Uuid) -> bool {
        self.network.is_peer_connected(peer_id).await
    }

    async fn subscribe_to_peer_events(&self) -> Result<PeerEventStream, NetworkError> {
        self.network.subscribe_to_peer_events().await
    }
}

#[async_trait]
impl StorageEffects for CompositeHandler {
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        self.storage.store(key, value).await
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        self.storage.retrieve(key).await
    }

    async fn remove(&self, key: &str) -> Result<bool, StorageError> {
        self.storage.remove(key).await
    }

    async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        self.storage.list_keys(prefix).await
    }

    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        self.storage.exists(key).await
    }

    async fn store_batch(
        &self,
        pairs: std::collections::HashMap<String, Vec<u8>>,
    ) -> Result<(), StorageError> {
        self.storage.store_batch(pairs).await
    }

    async fn retrieve_batch(
        &self,
        keys: &[String],
    ) -> Result<std::collections::HashMap<String, Vec<u8>>, StorageError> {
        self.storage.retrieve_batch(keys).await
    }

    async fn clear_all(&self) -> Result<(), StorageError> {
        self.storage.clear_all().await
    }

    async fn stats(&self) -> Result<StorageStats, StorageError> {
        self.storage.stats().await
    }
}

#[async_trait]
impl CryptoEffects for CompositeHandler {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        self.crypto.random_bytes(len).await
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        self.crypto.random_bytes_32().await
    }

    async fn random_range(&self, range: std::ops::Range<u64>) -> u64 {
        self.crypto.random_range(range).await
    }

    async fn blake3_hash(&self, data: &[u8]) -> [u8; 32] {
        self.crypto.blake3_hash(data).await
    }

    async fn sha256_hash(&self, data: &[u8]) -> [u8; 32] {
        self.crypto.sha256_hash(data).await
    }

    async fn ed25519_sign(&self, data: &[u8], key: &SigningKey) -> Result<Signature, CryptoError> {
        self.crypto.ed25519_sign(data, key).await
    }

    async fn ed25519_verify(
        &self,
        data: &[u8],
        signature: &Signature,
        public_key: &VerifyingKey,
    ) -> Result<bool, CryptoError> {
        self.crypto
            .ed25519_verify(data, signature, public_key)
            .await
    }

    async fn ed25519_generate_keypair(&self) -> Result<(SigningKey, VerifyingKey), CryptoError> {
        self.crypto.ed25519_generate_keypair().await
    }

    async fn ed25519_public_key(&self, private_key: &SigningKey) -> VerifyingKey {
        self.crypto.ed25519_public_key(private_key).await
    }

    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
        self.crypto.constant_time_eq(a, b)
    }

    fn secure_zero(&self, data: &mut [u8]) {
        self.crypto.secure_zero(data)
    }
}

#[async_trait]
impl TimeEffects for CompositeHandler {
    async fn current_epoch(&self) -> u64 {
        self.time.current_epoch().await
    }

    async fn sleep_ms(&self, ms: u64) {
        self.time.sleep_ms(ms).await
    }

    async fn sleep_until(&self, epoch: u64) {
        self.time.sleep_until(epoch).await
    }

    async fn yield_until(&self, condition: WakeCondition) -> Result<(), TimeError> {
        self.time.yield_until(condition).await
    }

    async fn set_timeout(&self, timeout_ms: u64) -> TimeoutHandle {
        self.time.set_timeout(timeout_ms).await
    }

    async fn cancel_timeout(&self, handle: TimeoutHandle) -> Result<(), TimeError> {
        self.time.cancel_timeout(handle).await
    }

    fn is_simulated(&self) -> bool {
        self.time.is_simulated()
    }

    fn register_context(&self, context_id: Uuid) {
        self.time.register_context(context_id)
    }

    fn unregister_context(&self, context_id: Uuid) {
        self.time.unregister_context(context_id)
    }

    async fn notify_events_available(&self) {
        self.time.notify_events_available().await
    }

    fn resolution_ms(&self) -> u64 {
        self.time.resolution_ms()
    }

    async fn current_timestamp(&self) -> u64 {
        self.time.current_timestamp().await
    }

    async fn current_timestamp_millis(&self) -> u64 {
        self.time.current_timestamp_millis().await
    }

    async fn delay(&self, duration: std::time::Duration) {
        self.time.delay(duration).await
    }

    async fn sleep(&self, duration_ms: u64) -> Result<(), aura_types::AuraError> {
        self.time.sleep(duration_ms).await
    }

    async fn wait_until(&self, condition: WakeCondition) -> Result<(), aura_types::AuraError> {
        self.time.wait_until(condition).await
    }

    // timeout method removed to make TimeEffects dyn-compatible
    // Use tokio::time::timeout directly where needed
}

impl ConsoleEffects for CompositeHandler {
    fn log_trace(&self, message: &str, fields: &[(&str, &str)]) {
        self.console.log_trace(message, fields);
    }

    fn log_debug(&self, message: &str, fields: &[(&str, &str)]) {
        self.console.log_debug(message, fields);
    }

    fn log_info(&self, message: &str, fields: &[(&str, &str)]) {
        self.console.log_info(message, fields);
    }

    fn log_warn(&self, message: &str, fields: &[(&str, &str)]) {
        self.console.log_warn(message, fields);
    }

    fn log_error(&self, message: &str, fields: &[(&str, &str)]) {
        self.console.log_error(message, fields);
    }

    fn emit_event(
        &self,
        event: ConsoleEvent,
    ) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        self.console.emit_event(event)
    }
}

// Removed legacy ProtocolEffects and MinimalEffects traits
// Device ID and simulation state are available through the struct fields directly

/// Builder for creating custom composite handlers
pub struct CompositeHandlerBuilder {
    device_id: Uuid,
    is_simulation: bool,
    network: Option<Box<dyn NetworkEffects>>,
    storage: Option<Box<dyn StorageEffects>>,
    crypto: Option<Box<dyn CryptoEffects>>,
    time: Option<DummyTimeHandler>,
    console: Option<Box<dyn ConsoleEffects>>,
}

impl CompositeHandlerBuilder {
    fn new(device_id: Uuid) -> Self {
        Self {
            device_id,
            is_simulation: false,
            network: None,
            storage: None,
            crypto: None,
            time: None,
            console: None,
        }
    }

    /// Configure handler for simulation mode
    pub fn simulation(mut self) -> Self {
        self.is_simulation = true;
        self
    }

    /// Set the network effects handler
    pub fn with_network(mut self, network: Box<dyn NetworkEffects>) -> Self {
        self.network = Some(network);
        self
    }

    /// Set the storage effects handler
    pub fn with_storage(mut self, storage: Box<dyn StorageEffects>) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Set the crypto effects handler
    pub fn with_crypto(mut self, crypto: Box<dyn CryptoEffects>) -> Self {
        self.crypto = Some(crypto);
        self
    }

    /// Set the time effects handler
    pub fn with_time(mut self, time: DummyTimeHandler) -> Self {
        self.time = Some(time);
        self
    }

    /// Set the console effects handler
    pub fn with_console(mut self, console: Box<dyn ConsoleEffects>) -> Self {
        self.console = Some(console);
        self
    }

    /// Build the final composite handler
    pub fn build(self) -> CompositeHandler {
        let device_id = self.device_id;
        let is_simulation = self.is_simulation;

        CompositeHandler {
            device_id,
            is_simulation,
            network: self.network.unwrap_or_else(|| {
                if is_simulation {
                    Box::new(MemoryNetworkHandler::new(device_id))
                } else {
                    Box::new(RealNetworkHandler::new(
                        device_id,
                        "tcp://0.0.0.0:0".to_string(),
                    ))
                }
            }),
            storage: self.storage.unwrap_or_else(|| {
                if is_simulation {
                    Box::new(MemoryStorageHandler::new())
                } else {
                    Box::new(FilesystemStorageHandler::new("/tmp/aura".into()).unwrap())
                }
            }),
            crypto: self.crypto.unwrap_or_else(|| {
                if is_simulation {
                    Box::new(MockCryptoHandler::new(device_id.as_u128() as u64))
                } else {
                    Box::new(RealCryptoHandler::new())
                }
            }),
            time: self.time.unwrap_or_else(|| DummyTimeHandler::new()),
            console: self.console.unwrap_or_else(|| {
                if is_simulation {
                    Box::new(SilentConsoleHandler::new())
                } else {
                    Box::new(StdoutConsoleHandler::new())
                }
            }),
        }
    }
}

// Implement LedgerEffects with placeholder delegation
#[async_trait]
impl LedgerEffects for CompositeHandler {
    // Removed old methods that are no longer part of the trait

    async fn append_event(&self, _event: Vec<u8>) -> Result<(), LedgerError> {
        Err(LedgerError::NotAvailable)
    }

    async fn current_epoch(&self) -> Result<u64, LedgerError> {
        Ok(self.time.current_epoch().await)
    }

    async fn events_since(&self, _epoch: u64) -> Result<Vec<Vec<u8>>, LedgerError> {
        Err(LedgerError::NotAvailable)
    }

    async fn is_device_authorized(
        &self,
        _device_id: aura_types::DeviceId,
        _operation: &str,
    ) -> Result<bool, LedgerError> {
        Ok(true) // Placeholder - always authorized
    }

    async fn get_device_metadata(
        &self,
        _device_id: aura_types::DeviceId,
    ) -> Result<Option<DeviceMetadata>, LedgerError> {
        Ok(None)
    }

    async fn update_device_activity(
        &self,
        _device_id: aura_types::DeviceId,
    ) -> Result<(), LedgerError> {
        Ok(())
    }

    async fn subscribe_to_events(&self) -> Result<LedgerEventStream, LedgerError> {
        Err(LedgerError::NotAvailable)
    }

    async fn would_create_cycle(
        &self,
        _edges: &[(Vec<u8>, Vec<u8>)],
        _new_edge: (Vec<u8>, Vec<u8>),
    ) -> Result<bool, LedgerError> {
        Ok(false) // Stub implementation
    }

    async fn find_connected_components(
        &self,
        _edges: &[(Vec<u8>, Vec<u8>)],
    ) -> Result<Vec<Vec<Vec<u8>>>, LedgerError> {
        Ok(vec![]) // Stub implementation
    }

    async fn topological_sort(
        &self,
        _edges: &[(Vec<u8>, Vec<u8>)],
    ) -> Result<Vec<Vec<u8>>, LedgerError> {
        Ok(vec![]) // Stub implementation
    }

    async fn shortest_path(
        &self,
        _edges: &[(Vec<u8>, Vec<u8>)],
        _start: Vec<u8>,
        _end: Vec<u8>,
    ) -> Result<Option<Vec<Vec<u8>>>, LedgerError> {
        Ok(None) // Stub implementation
    }

    async fn generate_secret(&self, length: usize) -> Result<Vec<u8>, LedgerError> {
        let random_bytes = self.crypto.random_bytes(length).await;
        Ok(random_bytes)
    }

    async fn hash_blake3(&self, _data: &[u8]) -> Result<[u8; 32], LedgerError> {
        let hash = self.crypto.blake3_hash(_data).await;
        Ok(hash)
    }

    async fn current_timestamp(&self) -> Result<u64, LedgerError> {
        let timestamp = self.time.current_timestamp().await;
        Ok(timestamp)
    }

    async fn ledger_device_id(&self) -> Result<aura_types::DeviceId, LedgerError> {
        Ok(self.device_id.into())
    }

    async fn new_uuid(&self) -> Result<Uuid, LedgerError> {
        Ok(Uuid::new_v4())
    }
}

// Implement ChoreographicEffects with placeholder delegation
#[async_trait]
impl ChoreographicEffects for CompositeHandler {
    async fn send_to_role_bytes(
        &self,
        role: ChoreographicRole,
        message: Vec<u8>,
    ) -> Result<(), ChoreographyError> {
        self.network
            .send_to_peer(role.device_id, message)
            .await
            .map_err(|e| ChoreographyError::Transport {
                source: Box::new(e),
            })
    }

    async fn receive_from_role_bytes(
        &self,
        role: ChoreographicRole,
    ) -> Result<Vec<u8>, ChoreographyError> {
        self.network
            .receive_from(role.device_id)
            .await
            .map_err(|e| ChoreographyError::Transport {
                source: Box::new(e),
            })
    }

    async fn broadcast_bytes(&self, message: Vec<u8>) -> Result<(), ChoreographyError> {
        self.network
            .broadcast(message)
            .await
            .map_err(|e| ChoreographyError::Transport {
                source: Box::new(e),
            })
    }

    fn current_role(&self) -> ChoreographicRole {
        ChoreographicRole {
            device_id: self.device_id,
            role_index: 0,
        }
    }

    fn all_roles(&self) -> Vec<ChoreographicRole> {
        vec![self.current_role()]
    }

    async fn is_role_active(&self, role: ChoreographicRole) -> bool {
        self.network.is_peer_connected(role.device_id).await
    }

    async fn start_session(
        &self,
        _session_id: Uuid,
        _roles: Vec<ChoreographicRole>,
    ) -> Result<(), ChoreographyError> {
        Ok(())
    }

    async fn end_session(&self) -> Result<(), ChoreographyError> {
        Ok(())
    }

    async fn emit_choreo_event(&self, event: ChoreographyEvent) -> Result<(), ChoreographyError> {
        // Convert to console event
        let console_event = ConsoleEvent::Custom {
            event_type: "choreography".to_string(),
            data: serde_json::to_value(event).unwrap_or_default(),
        };
        self.console.emit_event(console_event).await;
        Ok(())
    }

    async fn set_timeout(&self, timeout_ms: u64) {
        let _ = self.time.set_timeout(timeout_ms).await;
    }

    async fn get_metrics(&self) -> ChoreographyMetrics {
        ChoreographyMetrics {
            messages_sent: 0,
            messages_received: 0,
            avg_latency_ms: 0.0,
            timeout_count: 0,
            retry_count: 0,
            total_duration_ms: 0,
        }
    }
}

// Implementation of unified AuraHandler trait
#[async_trait]
impl AuraHandler for CompositeHandler {
    /// Execute an effect with serialized parameters and return serialized result
    async fn execute_effect(
        &mut self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        _ctx: &mut AuraContext,
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match effect_type {
            EffectType::Network => self.execute_network_effect(operation, parameters).await,
            EffectType::Storage => self.execute_storage_effect(operation, parameters).await,
            EffectType::Crypto => self.execute_crypto_effect(operation, parameters).await,
            EffectType::Time => self.execute_time_effect(operation, parameters).await,
            EffectType::Console => self.execute_console_effect(operation, parameters).await,
            EffectType::Ledger => self.execute_ledger_effect(operation, parameters).await,
            EffectType::Choreographic => {
                self.execute_choreographic_effect(operation, parameters)
                    .await
            }
            _ => Err(AuraHandlerError::UnsupportedEffect { effect_type }),
        }
    }

    /// Execute a session type
    async fn execute_session(
        &mut self,
        _session: LocalSessionType,
        _ctx: &mut AuraContext,
    ) -> Result<(), AuraHandlerError> {
        // Placeholder implementation - session type execution will be implemented
        // when the session type algebra is fully defined
        Err(AuraHandlerError::UnsupportedEffect {
            effect_type: EffectType::SessionManagement,
        })
    }

    /// Check if this handler supports a specific effect type
    fn supports_effect(&self, effect_type: EffectType) -> bool {
        matches!(
            effect_type,
            EffectType::Network
                | EffectType::Storage
                | EffectType::Crypto
                | EffectType::Time
                | EffectType::Console
                | EffectType::Ledger
                | EffectType::Choreographic
        )
    }

    /// Get the execution mode of this handler
    fn execution_mode(&self) -> ExecutionMode {
        if self.is_simulation {
            ExecutionMode::Simulation { seed: 0 }
        } else {
            ExecutionMode::Production
        }
    }
}

impl CompositeHandler {
    /// Execute network effects through serialized interface
    async fn execute_network_effect(
        &self,
        operation: &str,
        parameters: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match operation {
            "send_to_peer" => {
                let (peer_id, message): (Uuid, Vec<u8>) = serde_json::from_slice(parameters)
                    .map_err(|e| AuraHandlerError::ContextError {
                        message: format!("Failed to deserialize {} parameters: {}", operation, e),
                    })?;
                self.network
                    .send_to_peer(peer_id, message)
                    .await
                    .map_err(|e| AuraHandlerError::ContextError {
                        message: format!("Network effect {} failed: {}", operation, e),
                    })?;
                Ok(serde_json::to_vec(&()).unwrap_or_default())
            }
            "broadcast" => {
                let message: Vec<u8> = serde_json::from_slice(parameters).map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Failed to deserialize {} parameters: {}", operation, e),
                    }
                })?;
                self.network.broadcast(message).await.map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Network effect {} failed: {}", operation, e),
                    }
                })?;
                Ok(serde_json::to_vec(&()).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnknownOperation {
                effect_type: EffectType::Network,
                operation: operation.to_string(),
            }),
        }
    }

    /// Execute storage effects through serialized interface
    async fn execute_storage_effect(
        &self,
        operation: &str,
        parameters: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match operation {
            "store" => {
                let (key, value): (String, Vec<u8>) =
                    serde_json::from_slice(parameters).map_err(|e| {
                        AuraHandlerError::ContextError {
                            message: format!(
                                "Failed to deserialize {} parameters: {}",
                                operation, e
                            ),
                        }
                    })?;
                self.storage.store(&key, value).await.map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Storage effect {} failed: {}", operation, e),
                    }
                })?;
                Ok(serde_json::to_vec(&()).unwrap_or_default())
            }
            "retrieve" => {
                let key: String = serde_json::from_slice(parameters).map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Failed to deserialize {} parameters: {}", operation, e),
                    }
                })?;
                let result = self.storage.retrieve(&key).await.map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Storage effect {} failed: {}", operation, e),
                    }
                })?;
                Ok(serde_json::to_vec(&result).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnknownOperation {
                effect_type: EffectType::Storage,
                operation: operation.to_string(),
            }),
        }
    }

    /// Execute crypto effects through serialized interface
    async fn execute_crypto_effect(
        &self,
        operation: &str,
        parameters: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match operation {
            "random_bytes" => {
                let len: usize = serde_json::from_slice(parameters).map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Failed to deserialize {} parameters: {}", operation, e),
                    }
                })?;
                let result = self.crypto.random_bytes(len).await;
                Ok(serde_json::to_vec(&result).unwrap_or_default())
            }
            "blake3_hash" => {
                let data: Vec<u8> = serde_json::from_slice(parameters).map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Failed to deserialize {} parameters: {}", operation, e),
                    }
                })?;
                let result = self.crypto.blake3_hash(&data).await;
                Ok(serde_json::to_vec(&result).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnknownOperation {
                effect_type: EffectType::Crypto,
                operation: operation.to_string(),
            }),
        }
    }

    /// Execute time effects through serialized interface
    async fn execute_time_effect(
        &self,
        operation: &str,
        parameters: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match operation {
            "current_epoch" => {
                let result = self.time.current_epoch().await;
                Ok(serde_json::to_vec(&result).unwrap_or_default())
            }
            "sleep_ms" => {
                let ms: u64 = serde_json::from_slice(parameters).map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Failed to deserialize {} parameters: {}", operation, e),
                    }
                })?;
                self.time.sleep_ms(ms).await;
                Ok(serde_json::to_vec(&()).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnknownOperation {
                effect_type: EffectType::Time,
                operation: operation.to_string(),
            }),
        }
    }

    /// Execute console effects through serialized interface
    async fn execute_console_effect(
        &self,
        operation: &str,
        parameters: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match operation {
            "log_info" => {
                let (message, fields): (String, Vec<(String, String)>) =
                    serde_json::from_slice(parameters).map_err(|e| {
                        AuraHandlerError::ContextError {
                            message: format!(
                                "Failed to deserialize {} parameters: {}",
                                operation, e
                            ),
                        }
                    })?;
                let field_refs: Vec<(&str, &str)> = fields
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str()))
                    .collect();
                self.console.log_info(&message, &field_refs);
                Ok(serde_json::to_vec(&()).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnknownOperation {
                effect_type: EffectType::Console,
                operation: operation.to_string(),
            }),
        }
    }

    /// Execute ledger effects through serialized interface
    async fn execute_ledger_effect(
        &self,
        operation: &str,
        _parameters: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match operation {
            "current_epoch" => {
                let result = self.time.current_epoch().await;
                Ok(serde_json::to_vec(&result).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnknownOperation {
                effect_type: EffectType::Ledger,
                operation: operation.to_string(),
            }),
        }
    }

    /// Execute choreographic effects through serialized interface
    async fn execute_choreographic_effect(
        &self,
        operation: &str,
        parameters: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match operation {
            "send_to_role_bytes" => {
                let (role, message): (ChoreographicRole, Vec<u8>) =
                    serde_json::from_slice(parameters).map_err(|e| {
                        AuraHandlerError::ContextError {
                            message: format!(
                                "Failed to deserialize {} parameters: {}",
                                operation, e
                            ),
                        }
                    })?;
                self.network
                    .send_to_peer(role.device_id, message)
                    .await
                    .map_err(|e| AuraHandlerError::ContextError {
                        message: format!("Choreographic effect {} failed: {}", operation, e),
                    })?;
                Ok(serde_json::to_vec(&()).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnknownOperation {
                effect_type: EffectType::Choreographic,
                operation: operation.to_string(),
            }),
        }
    }
}
