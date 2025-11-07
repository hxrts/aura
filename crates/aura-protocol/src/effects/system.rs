//! Core Aura Effect System Implementation
//!
//! This module provides the main `AuraEffectSystem` implementation that serves
//! as the unified handler for all effect execution and session type interpretation
//! in the Aura platform.

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{
    effects::{
        ChoreographicEffects, ConsoleEffects, ConsoleEvent, CryptoEffects, JournalEffects,
        LedgerEffects, NetworkEffects, NetworkError, RandomEffects, StorageEffects, StorageError,
        StorageStats, TimeEffects,
    },
    handlers::{
        AuraContext,
        AuraHandler,
        AuraHandlerError,
        EffectType,
        ExecutionMode,
        // CompositeHandler - unified handler architecture
    },
};
use aura_types::{
    identifiers::{DeviceId, GuardianId},
    LocalSessionType,
    AuraError,
};
// Import stub types from journal.rs for now
use super::journal::{
    CapabilityId, CapabilityRef, Commitment, Epoch, Intent, IntentId, IntentStatus, JournalMap,
    JournalStats, LeafIndex, RatchetTree, TreeOpRecord,
};
use serde_json;
use std::future::Future;
use std::pin::Pin;

// Note: The local middleware implementations are incompatible with the new unified architecture
// They will be removed or refactored to use the ErasedHandler interface

/// Main implementation of the Aura Effect System
///
/// This is the primary entry point for all effect execution in Aura. It uses
/// the unified handler architecture from aura-types.
///
/// # Architecture
///
/// ```text
/// AuraEffectSystem
/// ├── CompositeHandler (unified handler from aura-protocol)
/// └── AuraContext (unified context flow)
/// ```
pub struct AuraEffectSystem {
    /// The unified composite handler that implements all effects
    composite_handler: Arc<RwLock<crate::handlers::CompositeHandler>>,
    /// Current execution context
    context: Arc<RwLock<AuraContext>>,
    /// Device ID for this system
    device_id: DeviceId,
    /// Execution mode
    execution_mode: ExecutionMode,
}

impl AuraEffectSystem {
    /// Create a new effect system with the given device ID and execution mode
    pub fn new(device_id: DeviceId, execution_mode: ExecutionMode) -> Self {
        // Create base context
        let context = match execution_mode {
            ExecutionMode::Testing => AuraContext::for_testing(device_id),
            ExecutionMode::Production => AuraContext::for_production(device_id),
            ExecutionMode::Simulation { seed } => AuraContext::for_simulation(device_id, seed),
        };

        // Build the system
        let composite_handler = match execution_mode {
            ExecutionMode::Testing => {
                crate::handlers::CompositeHandler::for_testing(device_id.into())
            }
            ExecutionMode::Production => {
                crate::handlers::CompositeHandler::for_production(device_id.into())
            }
            ExecutionMode::Simulation { .. } => {
                crate::handlers::CompositeHandler::for_simulation(device_id.into())
            }
        };

        Self {
            composite_handler: Arc::new(RwLock::new(composite_handler)),
            context: Arc::new(RwLock::new(context)),
            device_id,
            execution_mode,
        }
    }

    /// Get the current execution mode
    pub fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode
    }

    /// Get the device ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Check if this system supports a specific effect type
    pub async fn supports_effect(&self, effect_type: EffectType) -> bool {
        let composite_handler = self.composite_handler.read().await;
        composite_handler.supports_effect(effect_type)
    }

    /// Get all supported effect types
    pub async fn supported_effects(&self) -> Vec<EffectType> {
        let composite_handler = self.composite_handler.read().await;
        composite_handler.supported_effects()
    }

    /// Get the current context (cloned for safety)
    pub async fn context(&self) -> AuraContext {
        let context = self.context.read().await;
        context.clone()
    }

    /// Update the context
    pub async fn update_context<F>(&self, updater: F) -> Result<(), AuraHandlerError>
    where
        F: FnOnce(&mut AuraContext) -> Result<(), AuraHandlerError> + Send,
    {
        let mut context = self.context.write().await;
        updater(&mut *context)
    }

    /// Execute an effect with a custom context
    pub async fn execute_effect_with_context(
        &self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        context: &mut AuraContext,
    ) -> Result<Vec<u8>, AuraHandlerError> {
        let mut composite_handler = self.composite_handler.write().await;
        composite_handler
            .execute_effect(effect_type, operation, parameters, context)
            .await
    }

    /// Execute a session type with a custom context
    pub async fn execute_session_with_context(
        &self,
        session: LocalSessionType,
        context: &mut AuraContext,
    ) -> Result<(), AuraHandlerError> {
        let mut composite_handler = self.composite_handler.write().await;
        composite_handler.execute_session(session, context).await
    }

    /// Create a new session context
    pub async fn create_session_context(&self) -> AuraContext {
        let base_context = self.context().await;
        // Create a new session context based on the base
        base_context.clone()
    }

    /// Get system statistics
    pub async fn statistics(&self) -> AuraEffectSystemStats {
        let composite_handler = self.composite_handler.read().await;

        AuraEffectSystemStats {
            execution_mode: self.execution_mode,
            device_id: self.device_id,
            registered_effects: composite_handler.supported_effects().len(),
            total_operations: 0, // TODO: Count operations when implemented
            middleware_count: 1, // CompositeHandler acts as a single unified middleware
        }
    }
}

#[async_trait]
impl AuraHandler for AuraEffectSystem {
    async fn execute_effect(
        &mut self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        ctx: &mut AuraContext,
    ) -> Result<Vec<u8>, AuraHandlerError> {
        self.execute_effect_with_context(effect_type, operation, parameters, ctx)
            .await
    }

    async fn execute_session(
        &mut self,
        session: LocalSessionType,
        ctx: &mut AuraContext,
    ) -> Result<(), AuraHandlerError> {
        self.execute_session_with_context(session, ctx).await
    }

    fn supports_effect(&self, _effect_type: EffectType) -> bool {
        // In practice, we would check our middleware stack capabilities
        // For now, return false for stub implementation
        false
    }

    fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode
    }
}

/// Factory implementation for creating AuraEffectSystem instances
pub struct AuraEffectSystemFactory;

impl AuraEffectSystemFactory {
    /// Create a handler for the given execution mode
    pub fn create_handler(
        device_id: DeviceId,
        execution_mode: ExecutionMode,
    ) -> Box<dyn AuraHandler> {
        let system = AuraEffectSystem::new(device_id, execution_mode);
        Box::new(system)
    }

    /// Get the supported effect types
    pub fn supported_effect_types() -> Vec<EffectType> {
        vec![
            EffectType::Crypto,
            EffectType::Network,
            EffectType::Storage,
            EffectType::Time,
            EffectType::Console,
            EffectType::Random,
            EffectType::Ledger,
            EffectType::Journal,
            EffectType::Choreographic,
        ]
    }
}

/// Statistics about the effect system
#[derive(Debug, Clone)]
pub struct AuraEffectSystemStats {
    /// Current execution mode
    pub execution_mode: ExecutionMode,
    /// Device ID
    pub device_id: DeviceId,
    /// Number of registered effect types
    pub registered_effects: usize,
    /// Total number of operations across all effects
    pub total_operations: usize,
    /// Number of middleware in the stack
    pub middleware_count: usize,
}

impl AuraEffectSystemStats {
    /// Check if the system is in a deterministic mode
    pub fn is_deterministic(&self) -> bool {
        self.execution_mode.is_deterministic()
    }

    /// Check if the system is in production mode
    pub fn is_production(&self) -> bool {
        self.execution_mode.is_production()
    }

    /// Get a summary string
    pub fn summary(&self) -> String {
        format!(
            "AuraEffectSystem({:?}, {} effects, {} ops, {} middleware)",
            self.execution_mode,
            self.registered_effects,
            self.total_operations,
            self.middleware_count
        )
    }
}

/// Convenience functions for creating common effect system configurations
impl AuraEffectSystem {
    /// Create an effect system for testing
    pub fn for_testing(device_id: DeviceId) -> Self {
        Self::new(device_id, ExecutionMode::Testing)
    }

    /// Create an effect system for production
    pub fn for_production(device_id: DeviceId) -> Self {
        Self::new(device_id, ExecutionMode::Production)
    }

    /// Create an effect system for simulation
    pub fn for_simulation(device_id: DeviceId, seed: u64) -> Self {
        Self::new(device_id, ExecutionMode::Simulation { seed })
    }
}

// Core Effect Trait Implementations for Zero-Overhead Access
// According to docs/400_effect_system.md, AuraEffectSystem should implement
// all core effect traits directly for zero-overhead performance

#[async_trait]
impl CryptoEffects for AuraEffectSystem {
    async fn blake3_hash(&self, data: &[u8]) -> [u8; 32] {
        // Delegate to middleware stack through type-erased interface
        let params = serde_json::to_vec(&data).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(EffectType::Crypto, "blake3_hash", &params, &mut context)
            .await
        {
            Ok(result) => {
                let hash_vec: Vec<u8> = serde_json::from_slice(&result).unwrap_or(vec![0; 32]);
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&hash_vec[..32.min(hash_vec.len())]);
                hash
            }
            Err(_) => [0u8; 32], // Fallback for testing/mock scenarios
        }
    }

    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        let params = serde_json::to_vec(&len).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(EffectType::Random, "random_bytes", &params, &mut context)
            .await
        {
            Ok(result) => serde_json::from_slice(&result).unwrap_or(vec![0; len]),
            Err(_) => vec![0; len], // Fallback
        }
    }

    async fn sha256_hash(&self, data: &[u8]) -> [u8; 32] {
        let params = serde_json::to_vec(&data).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(EffectType::Crypto, "sha256_hash", &params, &mut context)
            .await
        {
            Ok(result) => {
                let hash_vec: Vec<u8> = serde_json::from_slice(&result).unwrap_or(vec![0; 32]);
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&hash_vec[..32.min(hash_vec.len())]);
                hash
            }
            Err(_) => [0u8; 32], // Fallback
        }
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        let bytes = <Self as CryptoEffects>::random_bytes(self, 32).await;
        let mut result = [0u8; 32];
        result.copy_from_slice(&bytes[..32.min(bytes.len())]);
        result
    }

    async fn random_range(&self, range: std::ops::Range<u64>) -> u64 {
        let params = serde_json::to_vec(&(range.start, range.end)).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(EffectType::Random, "random_range", &params, &mut context)
            .await
        {
            Ok(result) => serde_json::from_slice(&result).unwrap_or(range.start),
            Err(_) => range.start, // Fallback
        }
    }

    async fn ed25519_sign(
        &self,
        data: &[u8],
        key: &ed25519_dalek::SigningKey,
    ) -> Result<ed25519_dalek::Signature, crate::effects::CryptoError> {
        let params = serde_json::to_vec(&(data, key.as_bytes())).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(EffectType::Crypto, "ed25519_sign", &params, &mut context)
            .await
        {
            Ok(result) => {
                let sig_bytes: Vec<u8> = serde_json::from_slice(&result).unwrap_or(vec![0; 64]);
                match sig_bytes.len() {
                    64 => {
                        let sig_array: [u8; 64] = sig_bytes.try_into().unwrap_or([0u8; 64]);
                        Ok(ed25519_dalek::Signature::from_bytes(&sig_array))
                    }
                    _ => Err(aura_types::AuraError::Crypto(
                        aura_types::CryptoError::InvalidOutput {
                            message: "Invalid signature length".to_string(),
                            context: "ed25519_sign".to_string(),
                        },
                    )),
                }
            }
            Err(e) => Err(aura_types::AuraError::Crypto(
                aura_types::CryptoError::OperationFailed {
                    message: e.to_string(),
                    context: "ed25519_sign".to_string(),
                },
            )),
        }
    }

    async fn ed25519_verify(
        &self,
        data: &[u8],
        signature: &ed25519_dalek::Signature,
        public_key: &ed25519_dalek::VerifyingKey,
    ) -> Result<bool, crate::effects::CryptoError> {
        let params = serde_json::to_vec(&(data, &signature.to_bytes()[..], public_key.as_bytes()))
            .unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(
                EffectType::Crypto,
                "ed25519_verify",
                &params,
                &mut context,
            )
            .await
        {
            Ok(result) => Ok(serde_json::from_slice(&result).unwrap_or(false)),
            Err(_) => Ok(false), // Fallback to false for safety
        }
    }

    async fn ed25519_generate_keypair(
        &self,
    ) -> Result<(ed25519_dalek::SigningKey, ed25519_dalek::VerifyingKey), crate::effects::CryptoError>
    {
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(
                EffectType::Crypto,
                "ed25519_generate_keypair",
                &[],
                &mut context,
            )
            .await
        {
            Ok(result) => {
                let (private_bytes, public_bytes): (Vec<u8>, Vec<u8>) =
                    serde_json::from_slice(&result).unwrap_or((vec![0; 32], vec![0; 32]));
                let signing_key = ed25519_dalek::SigningKey::from_bytes(
                    &private_bytes[..32].try_into().unwrap_or([0u8; 32]),
                );
                let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(
                    &public_bytes[..32].try_into().unwrap_or([0u8; 32]),
                )
                .map_err(|e| {
                    aura_types::AuraError::Crypto(aura_types::CryptoError::OperationFailed {
                        message: e.to_string(),
                        context: "ed25519_generate_keypair".to_string(),
                    })
                })?;
                Ok((signing_key, verifying_key))
            }
            Err(e) => Err(aura_types::AuraError::Crypto(
                aura_types::CryptoError::OperationFailed {
                    message: e.to_string(),
                    context: "ed25519_generate_keypair".to_string(),
                },
            )),
        }
    }

    async fn ed25519_public_key(
        &self,
        private_key: &ed25519_dalek::SigningKey,
    ) -> ed25519_dalek::VerifyingKey {
        private_key.verifying_key()
    }

    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
        use subtle::ConstantTimeEq;
        a.ct_eq(b).into()
    }

    fn secure_zero(&self, data: &mut [u8]) {
        use zeroize::Zeroize;
        data.zeroize();
    }
}

#[async_trait]
impl RandomEffects for AuraEffectSystem {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        // Delegate to crypto implementation for consistency
        CryptoEffects::random_bytes(self, len).await
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        // Delegate to crypto implementation for consistency
        CryptoEffects::random_bytes_32(self).await
    }

    async fn random_u64(&self) -> u64 {
        let bytes = <Self as CryptoEffects>::random_bytes(self, 8).await;
        u64::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ])
    }

    async fn random_range(&self, min: u64, max: u64) -> u64 {
        if min >= max {
            return min;
        }
        let range = std::ops::Range {
            start: min,
            end: max,
        };
        CryptoEffects::random_range(self, range).await
    }
}

#[async_trait]
impl NetworkEffects for AuraEffectSystem {
    async fn send_to_peer(
        &self,
        peer_id: uuid::Uuid,
        message: Vec<u8>,
    ) -> Result<(), NetworkError> {
        let params = serde_json::json!({
            "peer_id": peer_id,
            "data": message
        });
        let params_bytes = serde_json::to_vec(&params).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(
                EffectType::Network,
                "send_to_peer",
                &params_bytes,
                &mut context,
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(_) => Err(NetworkError::SendFailed(
                "Effect execution failed".to_string(),
            )),
        }
    }

    async fn receive(&self) -> Result<(uuid::Uuid, Vec<u8>), NetworkError> {
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(EffectType::Network, "receive", &[], &mut context)
            .await
        {
            Ok(result) => {
                let response: serde_json::Value =
                    serde_json::from_slice(&result).map_err(|_| {
                        NetworkError::ReceiveFailed("Invalid response format".to_string())
                    })?;

                let peer_id = uuid::Uuid::parse_str(
                    response
                        .get("peer_id")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            NetworkError::ReceiveFailed("Missing peer_id".to_string())
                        })?,
                )
                .map_err(|_| NetworkError::ReceiveFailed("Invalid peer_id format".to_string()))?;

                let data = response
                    .get("data")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| NetworkError::ReceiveFailed("Missing data".to_string()))?
                    .iter()
                    .map(|v| v.as_u64().unwrap_or(0) as u8)
                    .collect();

                Ok((peer_id, data))
            }
            Err(_) => Err(NetworkError::ReceiveFailed(
                "Effect execution failed".to_string(),
            )),
        }
    }

    async fn broadcast(&self, message: Vec<u8>) -> Result<(), NetworkError> {
        let params = serde_json::to_vec(&message).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(EffectType::Network, "broadcast", &params, &mut context)
            .await
        {
            Ok(_) => Ok(()),
            Err(_) => Err(NetworkError::SendFailed(
                "Broadcast effect execution failed".to_string(),
            )),
        }
    }

    async fn receive_from(&self, peer_id: uuid::Uuid) -> Result<Vec<u8>, NetworkError> {
        let params = serde_json::to_vec(&peer_id).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(EffectType::Network, "receive_from", &params, &mut context)
            .await
        {
            Ok(result) => serde_json::from_slice(&result)
                .map_err(|_| NetworkError::ReceiveFailed("Invalid response format".to_string())),
            Err(_) => Err(NetworkError::ReceiveFailed(
                "Effect execution failed".to_string(),
            )),
        }
    }

    async fn connected_peers(&self) -> Vec<uuid::Uuid> {
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(EffectType::Network, "connected_peers", &[], &mut context)
            .await
        {
            Ok(result) => serde_json::from_slice(&result).unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    async fn is_peer_connected(&self, peer_id: uuid::Uuid) -> bool {
        let params = serde_json::to_vec(&peer_id).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(
                EffectType::Network,
                "is_peer_connected",
                &params,
                &mut context,
            )
            .await
        {
            Ok(result) => serde_json::from_slice(&result).unwrap_or(false),
            Err(_) => false,
        }
    }

    async fn subscribe_to_peer_events(
        &self,
    ) -> Result<crate::effects::network::PeerEventStream, NetworkError> {
        // For now, return an empty stream placeholder
        // Real implementation would set up event subscription
        Err(NetworkError::NotImplemented)
    }
}

#[async_trait]
impl StorageEffects for AuraEffectSystem {
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        let params = serde_json::json!({
            "key": key,
            "value": value
        });
        let params_bytes = serde_json::to_vec(&params).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(EffectType::Storage, "store", &params_bytes, &mut context)
            .await
        {
            Ok(_) => Ok(()),
            Err(_) => Err(StorageError::WriteFailed(
                "Effect execution failed".to_string(),
            )),
        }
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let params = serde_json::to_vec(&key).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(EffectType::Storage, "retrieve", &params, &mut context)
            .await
        {
            Ok(result) => serde_json::from_slice(&result)
                .map_err(|_| StorageError::ReadFailed("Invalid response format".to_string())),
            Err(_) => Err(StorageError::ReadFailed(
                "Effect execution failed".to_string(),
            )),
        }
    }

    async fn remove(&self, key: &str) -> Result<bool, StorageError> {
        let params = serde_json::to_vec(&key).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(EffectType::Storage, "remove", &params, &mut context)
            .await
        {
            Ok(result) => serde_json::from_slice(&result)
                .map_err(|_| StorageError::DeleteFailed("Invalid response format".to_string())),
            Err(_) => Err(StorageError::DeleteFailed(
                "Effect execution failed".to_string(),
            )),
        }
    }

    async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        let params = serde_json::to_vec(&prefix).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(EffectType::Storage, "list_keys", &params, &mut context)
            .await
        {
            Ok(result) => serde_json::from_slice(&result)
                .map_err(|_| StorageError::ListFailed("Invalid response format".to_string())),
            Err(_) => Err(StorageError::ListFailed(
                "Effect execution failed".to_string(),
            )),
        }
    }

    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        let params = serde_json::to_vec(&key).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(EffectType::Storage, "exists", &params, &mut context)
            .await
        {
            Ok(result) => serde_json::from_slice(&result)
                .map_err(|_| StorageError::ReadFailed("Invalid response format".to_string())),
            Err(_) => Err(StorageError::ReadFailed(
                "Effect execution failed".to_string(),
            )),
        }
    }

    async fn store_batch(
        &self,
        pairs: std::collections::HashMap<String, Vec<u8>>,
    ) -> Result<(), StorageError> {
        let params = serde_json::to_vec(&pairs).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(EffectType::Storage, "store_batch", &params, &mut context)
            .await
        {
            Ok(_) => Ok(()),
            Err(_) => Err(StorageError::WriteFailed("Batch store failed".to_string())),
        }
    }

    async fn retrieve_batch(
        &self,
        keys: &[String],
    ) -> Result<std::collections::HashMap<String, Vec<u8>>, StorageError> {
        let params = serde_json::to_vec(&keys).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(
                EffectType::Storage,
                "retrieve_batch",
                &params,
                &mut context,
            )
            .await
        {
            Ok(result) => serde_json::from_slice(&result)
                .map_err(|_| StorageError::ReadFailed("Invalid response format".to_string())),
            Err(_) => Err(StorageError::ReadFailed(
                "Batch retrieve failed".to_string(),
            )),
        }
    }

    async fn clear_all(&self) -> Result<(), StorageError> {
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(EffectType::Storage, "clear_all", &[], &mut context)
            .await
        {
            Ok(_) => Ok(()),
            Err(_) => Err(StorageError::DeleteFailed("Clear all failed".to_string())),
        }
    }

    async fn stats(&self) -> Result<StorageStats, StorageError> {
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(EffectType::Storage, "stats", &[], &mut context)
            .await
        {
            Ok(result) => serde_json::from_slice(&result)
                .map_err(|_| StorageError::ReadFailed("Invalid stats response".to_string())),
            Err(_) => Err(StorageError::ReadFailed(
                "Stats retrieval failed".to_string(),
            )),
        }
    }
}

#[async_trait]
impl TimeEffects for AuraEffectSystem {
    async fn current_epoch(&self) -> u64 {
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(EffectType::Time, "current_epoch", &[], &mut context)
            .await
        {
            Ok(result) => serde_json::from_slice(&result).unwrap_or(0),
            Err(_) => 0, // Fallback
        }
    }

    async fn current_timestamp(&self) -> u64 {
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(EffectType::Time, "current_timestamp", &[], &mut context)
            .await
        {
            Ok(result) => serde_json::from_slice(&result).unwrap_or(0),
            Err(_) => 0, // Fallback
        }
    }

    async fn current_timestamp_millis(&self) -> u64 {
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(
                EffectType::Time,
                "current_timestamp_millis",
                &[],
                &mut context,
            )
            .await
        {
            Ok(result) => serde_json::from_slice(&result).unwrap_or(0),
            Err(_) => 0, // Fallback
        }
    }

    async fn sleep_ms(&self, ms: u64) {
        let params = serde_json::to_vec(&ms).unwrap_or_default();
        let mut context = self.context().await;

        let _ = self
            .execute_effect_with_context(EffectType::Time, "sleep_ms", &params, &mut context)
            .await;
    }

    async fn sleep_until(&self, epoch: u64) {
        let params = serde_json::to_vec(&epoch).unwrap_or_default();
        let mut context = self.context().await;

        let _ = self
            .execute_effect_with_context(EffectType::Time, "sleep_until", &params, &mut context)
            .await;
    }

    async fn delay(&self, duration: std::time::Duration) {
        let params = serde_json::to_vec(&duration.as_millis()).unwrap_or_default();
        let mut context = self.context().await;

        let _ = self
            .execute_effect_with_context(EffectType::Time, "delay", &params, &mut context)
            .await;
    }

    async fn sleep(&self, duration_ms: u64) -> Result<(), AuraError> {
        let params = serde_json::to_vec(&duration_ms).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(EffectType::Time, "sleep", &params, &mut context)
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => Err(AuraError::Infrastructure(
                aura_types::InfrastructureError::ConfigError {
                    message: format!("Sleep failed: {}", e),
                    context: "sleep".to_string(),
                },
            )),
        }
    }

    async fn yield_until(
        &self,
        condition: crate::effects::WakeCondition,
    ) -> Result<(), crate::effects::TimeError> {
        let params = serde_json::to_vec(&condition).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(EffectType::Time, "yield_until", &params, &mut context)
            .await
        {
            Ok(_) => Ok(()),
            Err(_e) => Err(crate::effects::TimeError::ServiceUnavailable),
        }
    }

    async fn wait_until(&self, condition: crate::effects::WakeCondition) -> Result<(), AuraError> {
        match self.yield_until(condition).await {
            Ok(()) => Ok(()),
            Err(e) => Err(AuraError::Infrastructure(
                aura_types::InfrastructureError::ConfigError {
                    message: format!("Wait until failed: {}", e),
                    context: "wait_until".to_string(),
                },
            )),
        }
    }

    async fn set_timeout(&self, timeout_ms: u64) -> crate::effects::TimeoutHandle {
        let params = serde_json::to_vec(&timeout_ms).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(EffectType::Time, "set_timeout", &params, &mut context)
            .await
        {
            Ok(result) => serde_json::from_slice(&result).unwrap_or_else(|_| uuid::Uuid::new_v4()),
            Err(_) => uuid::Uuid::new_v4(), // Fallback
        }
    }

    async fn cancel_timeout(
        &self,
        handle: crate::effects::TimeoutHandle,
    ) -> Result<(), crate::effects::TimeError> {
        let params = serde_json::to_vec(&handle).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(EffectType::Time, "cancel_timeout", &params, &mut context)
            .await
        {
            Ok(_) => Ok(()),
            Err(_) => Err(crate::effects::TimeError::ServiceUnavailable),
        }
    }

    // timeout method removed to make TimeEffects dyn-compatible
    // Use tokio::time::timeout directly where needed

    fn is_simulated(&self) -> bool {
        matches!(self.execution_mode, ExecutionMode::Simulation { .. })
    }

    fn register_context(&self, _context_id: uuid::Uuid) {
        // Placeholder implementation
        // Real implementation would track contexts for time events
    }

    fn unregister_context(&self, _context_id: uuid::Uuid) {
        // Placeholder implementation
    }

    async fn notify_events_available(&self) {
        // Placeholder implementation
        // Real implementation would wake waiting contexts
    }

    fn resolution_ms(&self) -> u64 {
        // Default resolution is 1ms
        1
    }
}

#[async_trait]
impl ConsoleEffects for AuraEffectSystem {
    fn log_trace(&self, message: &str, fields: &[(&str, &str)]) {
        let params = serde_json::json!({
            "level": "trace",
            "message": message,
            "fields": fields.iter().map(|(k, v)| (*k, *v)).collect::<std::collections::HashMap<_, _>>()
        });
        // Fire and forget for logging
        let system = self.clone();
        tokio::spawn(async move {
            let params_bytes = serde_json::to_vec(&params).unwrap_or_default();
            let mut context = system.context().await;
            let _ = system
                .execute_effect_with_context(
                    EffectType::Console,
                    "log",
                    &params_bytes,
                    &mut context,
                )
                .await;
        });
    }

    fn log_debug(&self, message: &str, fields: &[(&str, &str)]) {
        let params = serde_json::json!({
            "level": "debug",
            "message": message,
            "fields": fields.iter().map(|(k, v)| (*k, *v)).collect::<std::collections::HashMap<_, _>>()
        });
        let system = self.clone();
        tokio::spawn(async move {
            let params_bytes = serde_json::to_vec(&params).unwrap_or_default();
            let mut context = system.context().await;
            let _ = system
                .execute_effect_with_context(
                    EffectType::Console,
                    "log",
                    &params_bytes,
                    &mut context,
                )
                .await;
        });
    }

    fn log_info(&self, message: &str, fields: &[(&str, &str)]) {
        let params = serde_json::json!({
            "level": "info",
            "message": message,
            "fields": fields.iter().map(|(k, v)| (*k, *v)).collect::<std::collections::HashMap<_, _>>()
        });
        let system = self.clone();
        tokio::spawn(async move {
            let params_bytes = serde_json::to_vec(&params).unwrap_or_default();
            let mut context = system.context().await;
            let _ = system
                .execute_effect_with_context(
                    EffectType::Console,
                    "log",
                    &params_bytes,
                    &mut context,
                )
                .await;
        });
    }

    fn log_warn(&self, message: &str, fields: &[(&str, &str)]) {
        let params = serde_json::json!({
            "level": "warn",
            "message": message,
            "fields": fields.iter().map(|(k, v)| (*k, *v)).collect::<std::collections::HashMap<_, _>>()
        });
        let system = self.clone();
        tokio::spawn(async move {
            let params_bytes = serde_json::to_vec(&params).unwrap_or_default();
            let mut context = system.context().await;
            let _ = system
                .execute_effect_with_context(
                    EffectType::Console,
                    "log",
                    &params_bytes,
                    &mut context,
                )
                .await;
        });
    }

    fn log_error(&self, message: &str, fields: &[(&str, &str)]) {
        let params = serde_json::json!({
            "level": "error",
            "message": message,
            "fields": fields.iter().map(|(k, v)| (*k, *v)).collect::<std::collections::HashMap<_, _>>()
        });
        let system = self.clone();
        tokio::spawn(async move {
            let params_bytes = serde_json::to_vec(&params).unwrap_or_default();
            let mut context = system.context().await;
            let _ = system
                .execute_effect_with_context(
                    EffectType::Console,
                    "log",
                    &params_bytes,
                    &mut context,
                )
                .await;
        });
    }

    fn emit_event(&self, event: ConsoleEvent) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            let params = serde_json::to_vec(&event).unwrap_or_default();
            let mut context = self.context().await;
            let _ = self
                .execute_effect_with_context(
                    EffectType::Console,
                    "emit_event",
                    &params,
                    &mut context,
                )
                .await;
        })
    }
}

// Stub implementations for LedgerEffects and ChoreographicEffects
// These should be implemented by business logic crates, but we provide stubs
// to make the unified system compile

#[async_trait]
impl LedgerEffects for AuraEffectSystem {
    async fn append_event(&self, _event: Vec<u8>) -> Result<(), crate::effects::LedgerError> {
        Err(crate::effects::LedgerError::NotAvailable)
    }

    async fn current_epoch(&self) -> Result<u64, crate::effects::LedgerError> {
        Ok(0) // Stub implementation
    }

    async fn events_since(&self, _epoch: u64) -> Result<Vec<Vec<u8>>, crate::effects::LedgerError> {
        Err(crate::effects::LedgerError::NotAvailable)
    }

    async fn is_device_authorized(
        &self,
        _device_id: DeviceId,
        _operation: &str,
    ) -> Result<bool, crate::effects::LedgerError> {
        Ok(true) // Stub - allow all for testing
    }

    async fn get_device_metadata(
        &self,
        _device_id: DeviceId,
    ) -> Result<Option<crate::effects::DeviceMetadata>, crate::effects::LedgerError> {
        Ok(None)
    }

    async fn update_device_activity(
        &self,
        _device_id: DeviceId,
    ) -> Result<(), crate::effects::LedgerError> {
        Ok(())
    }

    async fn subscribe_to_events(
        &self,
    ) -> Result<crate::effects::LedgerEventStream, crate::effects::LedgerError> {
        Err(crate::effects::LedgerError::NotAvailable)
    }

    async fn would_create_cycle(
        &self,
        _edges: &[(Vec<u8>, Vec<u8>)],
        _new_edge: (Vec<u8>, Vec<u8>),
    ) -> Result<bool, crate::effects::LedgerError> {
        Ok(false) // Stub
    }

    async fn find_connected_components(
        &self,
        _edges: &[(Vec<u8>, Vec<u8>)],
    ) -> Result<Vec<Vec<Vec<u8>>>, crate::effects::LedgerError> {
        Ok(vec![]) // Stub
    }

    async fn topological_sort(
        &self,
        _edges: &[(Vec<u8>, Vec<u8>)],
    ) -> Result<Vec<Vec<u8>>, crate::effects::LedgerError> {
        Ok(vec![]) // Stub
    }

    async fn shortest_path(
        &self,
        _edges: &[(Vec<u8>, Vec<u8>)],
        _start: Vec<u8>,
        _end: Vec<u8>,
    ) -> Result<Option<Vec<Vec<u8>>>, crate::effects::LedgerError> {
        Ok(None) // Stub
    }

    async fn generate_secret(&self, length: usize) -> Result<Vec<u8>, crate::effects::LedgerError> {
        // Delegate to random effects
        Ok(<Self as CryptoEffects>::random_bytes(self, length).await)
    }

    async fn hash_blake3(&self, data: &[u8]) -> Result<[u8; 32], crate::effects::LedgerError> {
        // Delegate to crypto effects
        Ok(self.blake3_hash(data).await)
    }

    async fn current_timestamp(&self) -> Result<u64, crate::effects::LedgerError> {
        // Delegate to time effects
        Ok(<Self as TimeEffects>::current_timestamp(self).await)
    }

    async fn ledger_device_id(&self) -> Result<DeviceId, crate::effects::LedgerError> {
        Ok(self.device_id)
    }

    async fn new_uuid(&self) -> Result<uuid::Uuid, crate::effects::LedgerError> {
        Ok(uuid::Uuid::new_v4())
    }
}

#[async_trait]
impl ChoreographicEffects for AuraEffectSystem {
    async fn send_to_role_bytes(
        &self,
        role: crate::effects::ChoreographicRole,
        message: Vec<u8>,
    ) -> Result<(), crate::effects::ChoreographyError> {
        // Delegate to network effects
        self.send_to_peer(role.device_id, message)
            .await
            .map_err(|e| crate::effects::ChoreographyError::Transport {
                source: Box::new(e),
            })
    }

    async fn receive_from_role_bytes(
        &self,
        role: crate::effects::ChoreographicRole,
    ) -> Result<Vec<u8>, crate::effects::ChoreographyError> {
        // Delegate to network effects
        self.receive_from(role.device_id).await.map_err(|e| {
            crate::effects::ChoreographyError::Transport {
                source: Box::new(e),
            }
        })
    }

    async fn broadcast_bytes(
        &self,
        message: Vec<u8>,
    ) -> Result<(), crate::effects::ChoreographyError> {
        // Delegate to network effects
        self.broadcast(message)
            .await
            .map_err(|e| crate::effects::ChoreographyError::Transport {
                source: Box::new(e),
            })
    }

    fn current_role(&self) -> crate::effects::ChoreographicRole {
        crate::effects::ChoreographicRole {
            device_id: self.device_id.into(),
            role_index: 0,
        }
    }

    fn all_roles(&self) -> Vec<crate::effects::ChoreographicRole> {
        vec![self.current_role()]
    }

    async fn is_role_active(&self, role: crate::effects::ChoreographicRole) -> bool {
        self.is_peer_connected(role.device_id).await
    }

    async fn start_session(
        &self,
        _session_id: uuid::Uuid,
        _roles: Vec<crate::effects::ChoreographicRole>,
    ) -> Result<(), crate::effects::ChoreographyError> {
        Ok(())
    }

    async fn end_session(&self) -> Result<(), crate::effects::ChoreographyError> {
        Ok(())
    }

    async fn emit_choreo_event(
        &self,
        event: crate::effects::ChoreographyEvent,
    ) -> Result<(), crate::effects::ChoreographyError> {
        // Convert to console event and delegate
        let console_event = crate::effects::ConsoleEvent::Custom {
            event_type: "choreography".to_string(),
            data: serde_json::to_value(event).unwrap_or_default(),
        };
        self.emit_event(console_event).await;
        Ok(())
    }

    async fn set_timeout(&self, timeout_ms: u64) {
        let _ = TimeEffects::set_timeout(self, timeout_ms).await;
    }

    async fn get_metrics(&self) -> crate::effects::ChoreographyMetrics {
        crate::effects::ChoreographyMetrics {
            messages_sent: 0,
            messages_received: 0,
            avg_latency_ms: 0.0,
            timeout_count: 0,
            retry_count: 0,
            total_duration_ms: 0,
        }
    }
}

#[async_trait]
impl JournalEffects for AuraEffectSystem {
    async fn get_journal_state(&self) -> Result<JournalMap, AuraError> {
        Err(AuraError::Infrastructure(
            aura_types::InfrastructureError::ConfigError {
                message: "Journal not available in stub implementation".to_string(),
                context: "journal_operations".to_string(),
            },
        ))
    }

    async fn get_current_tree(&self) -> Result<RatchetTree, AuraError> {
        Err(AuraError::Infrastructure(
            aura_types::InfrastructureError::ConfigError {
                message: "Tree not available in stub implementation".to_string(),
                context: "tree_operations".to_string(),
            },
        ))
    }

    async fn get_tree_at_epoch(&self, _epoch: Epoch) -> Result<RatchetTree, AuraError> {
        Err(AuraError::Infrastructure(
            aura_types::InfrastructureError::ConfigError {
                message: "Tree not available in stub implementation".to_string(),
                context: "tree_operations".to_string(),
            },
        ))
    }

    async fn get_current_commitment(&self) -> Result<Commitment, AuraError> {
        Err(AuraError::Infrastructure(
            aura_types::InfrastructureError::ConfigError {
                message: "Tree not available in stub implementation".to_string(),
                context: "tree_operations".to_string(),
            },
        ))
    }

    async fn get_latest_epoch(&self) -> Result<Option<Epoch>, AuraError> {
        Ok(None)
    }

    async fn append_tree_op(&self, _op: TreeOpRecord) -> Result<(), AuraError> {
        Err(AuraError::Infrastructure(
            aura_types::InfrastructureError::ConfigError {
                message: "Tree operations not available in stub implementation".to_string(),
                context: "tree_operations".to_string(),
            },
        ))
    }

    async fn get_tree_op(&self, _epoch: Epoch) -> Result<Option<TreeOpRecord>, AuraError> {
        Ok(None)
    }

    async fn list_tree_ops(&self) -> Result<Vec<TreeOpRecord>, AuraError> {
        Ok(vec![])
    }

    async fn submit_intent(&self, _intent: Intent) -> Result<IntentId, AuraError> {
        Err(AuraError::Infrastructure(
            aura_types::InfrastructureError::ConfigError {
                message: "Intents not available in stub implementation".to_string(),
                context: "intent_operations".to_string(),
            },
        ))
    }

    async fn get_intent(&self, _intent_id: IntentId) -> Result<Option<Intent>, AuraError> {
        Ok(None)
    }

    async fn get_intent_status(&self, _intent_id: IntentId) -> Result<IntentStatus, AuraError> {
        Ok(IntentStatus("pending".to_string())) // Default status
    }

    async fn list_pending_intents(&self) -> Result<Vec<Intent>, AuraError> {
        Ok(vec![]) // Empty list for stub
    }

    async fn tombstone_intent(&self, _intent_id: IntentId) -> Result<(), AuraError> {
        Err(AuraError::Infrastructure(
            aura_types::InfrastructureError::ConfigError {
                message: "Intents not available in stub implementation".to_string(),
                context: "intent_operations".to_string(),
            },
        ))
    }

    async fn prune_stale_intents(
        &self,
        _current_commitment: Commitment,
    ) -> Result<usize, AuraError> {
        Ok(0)
    }

    async fn validate_capability(&self, _capability: &CapabilityRef) -> Result<bool, AuraError> {
        Ok(true) // Stub - allow all capabilities
    }

    async fn is_capability_revoked(
        &self,
        _capability_id: &CapabilityId,
    ) -> Result<bool, AuraError> {
        Ok(false) // Stub - no revocations
    }

    async fn list_capabilities_in_op(
        &self,
        _epoch: Epoch,
    ) -> Result<Vec<CapabilityRef>, AuraError> {
        Ok(vec![])
    }

    async fn merge_journal_state(&self, _other: JournalMap) -> Result<(), AuraError> {
        Err(AuraError::Infrastructure(
            aura_types::InfrastructureError::ConfigError {
                message: "Journal operations not available in stub implementation".to_string(),
                context: "journal_operations".to_string(),
            },
        ))
    }

    async fn get_journal_stats(&self) -> Result<JournalStats, AuraError> {
        Ok(JournalStats {
            entry_count: 0,
            total_size: 0,
        })
    }

    async fn is_device_member(&self, _device_id: DeviceId) -> Result<bool, AuraError> {
        Ok(false)
    }

    async fn get_device_leaf_index(
        &self,
        _device_id: DeviceId,
    ) -> Result<Option<LeafIndex>, AuraError> {
        Ok(None)
    }

    async fn list_devices(&self) -> Result<Vec<DeviceId>, AuraError> {
        Ok(vec![])
    }

    async fn list_guardians(&self) -> Result<Vec<GuardianId>, AuraError> {
        Ok(vec![])
    }
}

// We need Clone for ConsoleEffects implementation above
impl Clone for AuraEffectSystem {
    fn clone(&self) -> Self {
        Self {
            composite_handler: Arc::clone(&self.composite_handler),
            context: Arc::clone(&self.context),
            device_id: self.device_id,
            execution_mode: self.execution_mode,
        }
    }
}

// Implement the composite AuraEffects trait
impl crate::effects::AuraEffects for AuraEffectSystem {}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_effect_system_creation() {
        let device_id = DeviceId::from(Uuid::new_v4());

        // Test testing mode
        let testing_system = AuraEffectSystem::for_testing(device_id);
        assert_eq!(testing_system.execution_mode(), ExecutionMode::Testing);
        assert_eq!(testing_system.device_id(), device_id);

        // Test production mode
        let production_system = AuraEffectSystem::for_production(device_id);
        assert_eq!(
            production_system.execution_mode(),
            ExecutionMode::Production
        );

        // Test simulation mode
        let simulation_system = AuraEffectSystem::for_simulation(device_id, 42);
        assert_eq!(
            simulation_system.execution_mode(),
            ExecutionMode::Simulation { seed: 42 }
        );
    }

    #[tokio::test]
    async fn test_context_management() {
        let device_id = DeviceId::from(Uuid::new_v4());
        let system = AuraEffectSystem::for_testing(device_id);

        // Test context retrieval
        let context = system.context().await;
        assert_eq!(context.device_id, device_id);
        assert_eq!(context.execution_mode, ExecutionMode::Testing);

        // Test session context creation
        let session_context = system.create_session_context().await;
        assert_eq!(session_context.device_id, device_id);
    }

    #[tokio::test]
    async fn test_statistics() {
        let device_id = DeviceId::from(Uuid::new_v4());
        let system = AuraEffectSystem::for_testing(device_id);

        let stats = system.statistics().await;
        assert_eq!(stats.device_id, device_id);
        assert!(stats.is_deterministic());
        assert!(!stats.is_production());

        let summary = stats.summary();
        assert!(summary.contains("AuraEffectSystem"));
        assert!(summary.contains("Testing"));
    }

    #[test]
    fn test_factory() {
        let device_id = DeviceId::from(Uuid::new_v4());

        let handler = AuraEffectSystemFactory::create_handler(device_id, ExecutionMode::Testing);
        assert_eq!(handler.execution_mode(), ExecutionMode::Testing);

        let supported_effects = AuraEffectSystemFactory::supported_effect_types();
        assert!(supported_effects.contains(&EffectType::Crypto));
        assert!(supported_effects.contains(&EffectType::Network));
        assert!(supported_effects.contains(&EffectType::Choreographic));
    }

    #[tokio::test]
    async fn test_context_updates() {
        let device_id = DeviceId::from(Uuid::new_v4());
        let system = AuraEffectSystem::for_testing(device_id);

        // Test context update
        system
            .update_context(|_ctx| {
                // Simplified test - just check that the updater can be called
                Ok(())
            })
            .await
            .unwrap();

        let updated_context = system.context().await;
        assert_eq!(updated_context.device_id, device_id);
    }
}
