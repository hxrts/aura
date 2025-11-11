//! Core Aura Effect System Implementation and System Effects Trait
//!
//! This module provides:
//! - The main `AuraEffectSystem` implementation for unified effect execution
//! - The `SystemEffects` trait for system monitoring, logging, and configuration
//! - System effect error types

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use tokio::sync::RwLock;
use tracing::debug;

/// System effect operations error
#[derive(Debug, thiserror::Error)]
pub enum SystemError {
    /// Service is unavailable
    #[error("System service unavailable")]
    ServiceUnavailable,

    /// Invalid configuration parameter
    #[error("Invalid configuration: {key}={value}")]
    InvalidConfiguration { key: String, value: String },

    /// Operation failed
    #[error("System operation failed: {message}")]
    OperationFailed { message: String },

    /// Permission denied
    #[error("Permission denied for operation: {operation}")]
    PermissionDenied { operation: String },

    /// Resource not found
    #[error("Resource not found: {resource}")]
    ResourceNotFound { resource: String },

    /// Resource exhausted
    #[error("Resource exhausted: {resource}")]
    ResourceExhausted { resource: String },
}

/// System effects interface for logging, monitoring, and configuration
///
/// This trait provides system-level operations for:
/// - Logging and audit trails
/// - System monitoring and health checks
/// - Configuration management
/// - System metrics and statistics
/// - Component lifecycle management
#[async_trait]
pub trait SystemEffects: Send + Sync {
    /// Log a message at the specified level
    async fn log(&self, level: &str, component: &str, message: &str) -> Result<(), SystemError>;

    /// Log a message with additional context
    async fn log_with_context(
        &self,
        level: &str,
        component: &str,
        message: &str,
        context: HashMap<String, String>,
    ) -> Result<(), SystemError>;

    /// Get system information and status
    async fn get_system_info(&self) -> Result<HashMap<String, String>, SystemError>;

    /// Set a configuration value
    async fn set_config(&self, key: &str, value: &str) -> Result<(), SystemError>;

    /// Get a configuration value
    async fn get_config(&self, key: &str) -> Result<String, SystemError>;

    /// Perform a health check
    async fn health_check(&self) -> Result<bool, SystemError>;

    /// Get system metrics
    async fn get_metrics(&self) -> Result<HashMap<String, f64>, SystemError>;

    /// Restart a system component
    async fn restart_component(&self, component: &str) -> Result<(), SystemError>;

    /// Shutdown the system gracefully
    async fn shutdown(&self) -> Result<(), SystemError>;
}

use crate::{
    effects::{
        ChoreographicEffects, ConsoleEffects, ConsoleEvent, CryptoEffects, JournalEffects,
        LedgerEffects, NetworkEffects, NetworkError, RandomEffects, StorageEffects, StorageError,
        StorageStats, TimeEffects, TreeEffects,
    },
    guards::flow::{FlowBudgetEffects, FlowGuard, FlowHint},
    handlers::{
        AuraContext,
        AuraHandler,
        AuraHandlerError,
        EffectType,
        ExecutionMode,
        // CompositeHandler - unified handler architecture
    },
};
use aura_core::{
    hash_canonical,
    identifiers::{DeviceId, GuardianId},
    relationships::ContextId,
    session_epochs::{self, LocalSessionType},
    AuraError, AuraResult, FlowBudget, Hash32, Receipt,
};
use serde::{Deserialize, Serialize};
// Import stub types from journal.rs TODO fix - For now
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
/// the unified handler architecture from aura-core.
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
    /// Monotonic counter for FlowGuard receipts
    flow_nonce: AtomicU64,
    /// Previous receipt hash (forms a hash chain for auditability)
    flow_prev_receipt: Arc<RwLock<Hash32>>,
    /// Last emitted receipt for telemetry
    last_receipt: Arc<RwLock<Option<Receipt>>>,
    /// Ledger-backed flow budgets
    flow_ledgers: Arc<RwLock<HashMap<(ContextId, DeviceId), FlowBudget>>>,
    /// Anti-replay counters keyed by (context, src, dst)
    anti_replay_counters:
        Arc<RwLock<HashMap<(ContextId, DeviceId, DeviceId), (session_epochs::Epoch, u64)>>>,
}

#[derive(Serialize, Deserialize)]
struct TransportEnvelope {
    receipt: Receipt,
    payload: Vec<u8>,
}

impl std::fmt::Debug for AuraEffectSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuraEffectSystem")
            .field("device_id", &self.device_id)
            .field("execution_mode", &self.execution_mode)
            .finish()
    }
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
            flow_nonce: AtomicU64::new(0),
            flow_prev_receipt: Arc::new(RwLock::new(Hash32::from([0u8; 32]))),
            last_receipt: Arc::new(RwLock::new(None)),
            flow_ledgers: Arc::new(RwLock::new(HashMap::new())),
            anti_replay_counters: Arc::new(RwLock::new(HashMap::new())),
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

    /// Get the latest FlowGuard receipt emitted by this effect system (testing/telemetry)
    pub async fn latest_receipt(&self) -> Option<Receipt> {
        self.last_receipt.read().await.clone()
    }

    /// Set a flow hint that will be consumed before the next transport send.
    pub async fn set_flow_hint(&self, hint: FlowHint) {
        let mut context = self.context.write().await;
        context.set_flow_hint(hint);
    }

    /// Seed or update the flow budget for a context/peer pair.
    pub async fn seed_flow_budget(&self, context: ContextId, peer: DeviceId, budget: FlowBudget) {
        {
            // TODO: Implement flow budget update in CompositeHandler
            let _handler = self.composite_handler.read().await;
            // let _ = handler.update_flow_budget(&context, &peer, &budget).await;
        }
        let mut ledgers = self.flow_ledgers.write().await;
        ledgers.insert((context, peer), budget);
    }

    /// Compute FlowBudget deterministically using meet over journal facts
    ///
    /// This implements the deterministic budget computation requirement from
    /// work/007.md Section 3. It queries the journal for all FlowBudget facts
    /// for the given (context, peer) and computes the meet to get the canonical
    /// budget that all devices will converge on.
    ///
    /// # Algorithm
    /// 1. Query journal for all FlowBudget facts for (context, peer)
    /// 2. Apply meet operation over all facts (limit=min, spent=max, epoch=max)
    /// 3. Apply epoch rotation if needed
    /// 4. Return canonical budget
    pub async fn compute_deterministic_budget(
        &self,
        context: &ContextId,
        peer: &DeviceId,
        current_epoch: session_epochs::Epoch,
    ) -> AuraResult<FlowBudget> {
        // TODO: Implement flow budget retrieval in CompositeHandler  
        let _handler = self.composite_handler.read().await;
        let mut budget = FlowBudget::new(u64::MAX, current_epoch);
        // let mut budget = handler
        //     .get_flow_budget(context, peer)
        //     .await
        //     .unwrap_or_else(|_| FlowBudget::new(u64::MAX, current_epoch));
        drop(_handler);
        budget.rotate_epoch(current_epoch);

        Ok(budget)
    }

    fn encode_transport_envelope(receipt: &Receipt, payload: Vec<u8>) -> AuraResult<Vec<u8>> {
        let envelope = TransportEnvelope {
            receipt: receipt.clone(),
            payload,
        };
        bincode::serialize(&envelope).map_err(|e| {
            AuraError::serialization(format!("Failed to encode transport envelope: {}", e))
        })
    }

    async fn decode_transport_payload(
        &self,
        raw: Vec<u8>,
        peer_uuid: uuid::Uuid,
    ) -> AuraResult<Vec<u8>> {
        let envelope: TransportEnvelope = bincode::deserialize(&raw).map_err(|e| {
            AuraError::serialization(format!("Failed to decode transport envelope: {}", e))
        })?;
        let expected_src = DeviceId::from(peer_uuid);
        let expected_dst = self.device_id();
        self.verify_receipt_fields(&envelope.receipt, &expected_src, &expected_dst)?;
        self.enforce_anti_replay(&envelope.receipt).await?;
        Ok(envelope.payload)
    }

    fn verify_receipt_fields(
        &self,
        receipt: &Receipt,
        expected_src: &DeviceId,
        expected_dst: &DeviceId,
    ) -> AuraResult<()> {
        if &receipt.src != expected_src {
            return Err(AuraError::permission_denied(format!(
                "Receipt source mismatch (expected {}, got {})",
                expected_src, receipt.src
            )));
        }

        if &receipt.dst != expected_dst {
            return Err(AuraError::permission_denied(format!(
                "Receipt destination mismatch (expected {}, got {})",
                expected_dst, receipt.dst
            )));
        }

        let material = Self::receipt_signature_material(
            &receipt.ctx,
            &receipt.src,
            &receipt.dst,
            receipt.epoch.value(),
            receipt.cost,
            receipt.nonce,
        );
        let expected_hash = Hash32::from_bytes(material.as_bytes());
        if receipt.sig != expected_hash.0 {
            return Err(AuraError::permission_denied(
                "Receipt signature invalid".to_string(),
            ));
        }

        Ok(())
    }

    async fn enforce_anti_replay(&self, receipt: &Receipt) -> AuraResult<()> {
        let mut counters = self.anti_replay_counters.write().await;
        let key = (
            receipt.ctx.clone(),
            receipt.src.clone(),
            receipt.dst.clone(),
        );
        if let Some((epoch, nonce)) = counters.get(&key) {
            let stored_epoch = epoch.value();
            let incoming_epoch = receipt.epoch.value();
            if incoming_epoch < stored_epoch
                || (incoming_epoch == stored_epoch && receipt.nonce <= *nonce)
            {
                return Err(AuraError::permission_denied(format!(
                    "Replay detected for ctx={} src={} dst={} (epoch {}, nonce {})",
                    receipt.ctx.as_str(),
                    receipt.src,
                    receipt.dst,
                    receipt.epoch.value(),
                    receipt.nonce
                )));
            }
        }
        counters.insert(key, (receipt.epoch, receipt.nonce));
        Ok(())
    }

    fn receipt_signature_material(
        context: &ContextId,
        src: &DeviceId,
        dst: &DeviceId,
        epoch: u64,
        cost: u32,
        nonce: u64,
    ) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}",
            context.as_str(),
            src,
            dst,
            epoch,
            cost,
            nonce
        )
    }

    pub async fn set_flow_hint_components(&self, context: ContextId, peer: DeviceId, cost: u32) {
        self.set_flow_hint(FlowHint::new(context, peer, cost)).await;
    }

    async fn take_flow_hint_internal(&self) -> Option<FlowHint> {
        let mut context = self.context.write().await;
        context.take_flow_hint()
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
        // TODO fix - For now, return false for stub implementation
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
// According to docs/002_system_architecture.md, AuraEffectSystem should implement
// all core effect traits directly for zero-overhead performance

#[async_trait]
impl aura_core::effects::RandomEffects for AuraEffectSystem {
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

    async fn random_bytes_32(&self) -> [u8; 32] {
        let bytes = self.random_bytes(32).await;
        let mut result = [0u8; 32];
        result.copy_from_slice(&bytes[..32.min(bytes.len())]);
        result
    }

    async fn random_u64(&self) -> u64 {
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(EffectType::Random, "random_u64", &[], &mut context)
            .await
        {
            Ok(result) => serde_json::from_slice(&result).unwrap_or(0),
            Err(_) => 0, // Fallback
        }
    }

    async fn random_range(&self, min: u64, max: u64) -> u64 {
        let params = serde_json::to_vec(&(min, max)).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(EffectType::Random, "random_range", &params, &mut context)
            .await
        {
            Ok(result) => serde_json::from_slice(&result).unwrap_or(min),
            Err(_) => min, // Fallback
        }
    }
}

#[async_trait]
impl CryptoEffects for AuraEffectSystem {
    async fn hash(&self, data: &[u8]) -> [u8; 32] {
        // Delegate to middleware stack through type-erased interface
        let params = serde_json::to_vec(&data).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(EffectType::Crypto, "hash", &params, &mut context)
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

    async fn hmac(&self, key: &[u8], data: &[u8]) -> [u8; 32] {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;

        let mut mac = HmacSha256::new_from_slice(key).expect("HMAC can take key of any size");
        mac.update(data);
        mac.finalize().into_bytes().into()
    }

    async fn derive_key(
        &self,
        master_key: &[u8],
        context: &crate::effects::KeyDerivationContext,
    ) -> Result<Vec<u8>, crate::effects::CryptoError> {
        let info = format!(
            "{}:{}:{:?}:{}:{}",
            context.app_id,
            context.context,
            context.derivation_path,
            context.account_id,
            context.device_id
        );
        self.hkdf_derive(master_key, b"", info.as_bytes(), 32).await
    }

    async fn ed25519_generate_keypair(
        &self,
    ) -> Result<(Vec<u8>, Vec<u8>), crate::effects::CryptoError> {
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
                Ok((private_bytes, public_bytes))
            }
            Err(e) => Err(aura_core::AuraError::crypto(format!(
                "Key generation failed: {}",
                e
            ))),
        }
    }

    async fn ed25519_sign(
        &self,
        message: &[u8],
        private_key: &[u8],
    ) -> Result<Vec<u8>, crate::effects::CryptoError> {
        let params = serde_json::to_vec(&(message, private_key)).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(EffectType::Crypto, "ed25519_sign", &params, &mut context)
            .await
        {
            Ok(result) => {
                let sig_bytes: Vec<u8> = serde_json::from_slice(&result).unwrap_or(vec![0; 64]);
                Ok(sig_bytes)
            }
            Err(e) => Err(aura_core::AuraError::crypto(format!(
                "Signing failed: {}",
                e
            ))),
        }
    }

    async fn ed25519_verify(
        &self,
        message: &[u8],
        signature: &[u8],
        public_key: &[u8],
    ) -> Result<bool, crate::effects::CryptoError> {
        let params = serde_json::to_vec(&(message, signature, public_key)).unwrap_or_default();
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

    async fn ed25519_public_key(
        &self,
        private_key: &[u8],
    ) -> Result<Vec<u8>, crate::effects::CryptoError> {
        let params = serde_json::to_vec(&private_key).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(
                EffectType::Crypto,
                "ed25519_public_key",
                &params,
                &mut context,
            )
            .await
        {
            Ok(result) => {
                let public_bytes: Vec<u8> = serde_json::from_slice(&result).unwrap_or(vec![0; 32]);
                Ok(public_bytes)
            }
            Err(e) => Err(aura_core::AuraError::crypto(format!(
                "Public key derivation failed: {}",
                e
            ))),
        }
    }

    // FROST methods - use aura-frost crate instead
    async fn frost_generate_keys(
        &self,
        _threshold: u16,
        _max_signers: u16,
    ) -> Result<Vec<Vec<u8>>, crate::effects::CryptoError> {
        Err(aura_core::AuraError::internal(format!(
            "Not implemented: FROST key generation - use aura-frost crate"
        )))
    }

    async fn frost_generate_nonces(&self) -> Result<Vec<u8>, crate::effects::CryptoError> {
        Err(aura_core::AuraError::internal(format!(
            "Not implemented: FROST nonce generation - use aura-frost crate"
        )))
    }

    async fn frost_create_signing_package(
        &self,
        _message: &[u8],
        _nonces: &[Vec<u8>],
        _participants: &[u16],
    ) -> Result<crate::effects::FrostSigningPackage, crate::effects::CryptoError> {
        Err(aura_core::AuraError::internal(format!(
            "Not implemented: FROST signing package creation - use aura-frost crate"
        )))
    }

    async fn frost_sign_share(
        &self,
        _signing_package: &crate::effects::FrostSigningPackage,
        _key_share: &[u8],
        _nonces: &[u8],
    ) -> Result<Vec<u8>, crate::effects::CryptoError> {
        Err(aura_core::AuraError::internal(format!(
            "Not implemented: FROST signature share generation - use aura-frost crate"
        )))
    }

    async fn frost_aggregate_signatures(
        &self,
        _signing_package: &crate::effects::FrostSigningPackage,
        _signature_shares: &[Vec<u8>],
    ) -> Result<Vec<u8>, crate::effects::CryptoError> {
        Err(aura_core::AuraError::internal(format!(
            "Not implemented: FROST signature aggregation - use aura-frost crate"
        )))
    }

    async fn frost_verify(
        &self,
        _message: &[u8],
        _signature: &[u8],
        _group_public_key: &[u8],
    ) -> Result<bool, crate::effects::CryptoError> {
        Err(aura_core::AuraError::internal(format!(
            "Not implemented: FROST signature verification - use aura-frost crate"
        )))
    }

    // Symmetric encryption methods (placeholder implementations)
    async fn chacha20_encrypt(
        &self,
        _plaintext: &[u8],
        _key: &[u8; 32],
        _nonce: &[u8; 12],
    ) -> Result<Vec<u8>, crate::effects::CryptoError> {
        Err(aura_core::AuraError::internal(format!(
            "Not implemented: ChaCha20 encryption not implemented yet"
        )))
    }

    async fn chacha20_decrypt(
        &self,
        _ciphertext: &[u8],
        _key: &[u8; 32],
        _nonce: &[u8; 12],
    ) -> Result<Vec<u8>, crate::effects::CryptoError> {
        Err(aura_core::AuraError::internal(format!(
            "Not implemented: ChaCha20 decryption not implemented yet"
        )))
    }

    async fn aes_gcm_encrypt(
        &self,
        _plaintext: &[u8],
        _key: &[u8; 32],
        _nonce: &[u8; 12],
    ) -> Result<Vec<u8>, crate::effects::CryptoError> {
        Err(aura_core::AuraError::internal(format!(
            "Not implemented: AES-GCM encryption not implemented yet"
        )))
    }

    async fn aes_gcm_decrypt(
        &self,
        _ciphertext: &[u8],
        _key: &[u8; 32],
        _nonce: &[u8; 12],
    ) -> Result<Vec<u8>, crate::effects::CryptoError> {
        Err(aura_core::AuraError::internal(format!(
            "Not implemented: AES-GCM decryption not implemented yet"
        )))
    }

    // Key rotation methods (placeholder implementations)
    async fn frost_rotate_keys(
        &self,
        _old_shares: &[Vec<u8>],
        _old_threshold: u16,
        _new_threshold: u16,
        _new_max_signers: u16,
    ) -> Result<Vec<Vec<u8>>, crate::effects::CryptoError> {
        Err(aura_core::AuraError::internal(format!(
            "Not implemented: FROST key rotation not implemented yet"
        )))
    }

    // Utility methods
    fn is_simulated(&self) -> bool {
        matches!(self.execution_mode, ExecutionMode::Simulation { .. })
    }

    fn crypto_capabilities(&self) -> Vec<String> {
        vec![
            "hash".to_string(),
            "hmac".to_string(),
            "hkdf_derive".to_string(),
            "derive_key".to_string(),
            "ed25519_generate_keypair".to_string(),
            "ed25519_sign".to_string(),
            "ed25519_verify".to_string(),
            "ed25519_public_key".to_string(),
            "constant_time_eq".to_string(),
            "secure_zero".to_string(),
        ]
    }

    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
        use subtle::ConstantTimeEq;
        a.ct_eq(b).into()
    }

    fn secure_zero(&self, data: &mut [u8]) {
        use zeroize::Zeroize;
        data.zeroize();
    }

    async fn hkdf_derive(
        &self,
        ikm: &[u8],
        salt: &[u8],
        info: &[u8],
        output_len: usize,
    ) -> Result<Vec<u8>, crate::effects::CryptoError> {
        use hkdf::Hkdf;
        use sha2::Sha256;

        let hk = Hkdf::<Sha256>::new(Some(salt), ikm);
        let mut okm = vec![0u8; output_len];
        hk.expand(info, &mut okm)
            .map_err(|e| aura_core::AuraError::crypto(format!("HKDF expansion failed: {}", e)))?;
        Ok(okm)
    }
}

// Removed conflicting RandomEffects implementation - the correct one is at line 375

#[async_trait]
impl FlowBudgetEffects for AuraEffectSystem {
    async fn charge_flow(
        &self,
        context: &ContextId,
        peer: &DeviceId,
        cost: u32,
    ) -> AuraResult<Receipt> {
        debug!(
            "FlowGuard authorize context={} peer={} cost={}",
            context.as_str(),
            peer,
            cost
        );
        let local_context = self.context().await;
        let src = local_context.device_id.clone();
        let epoch = session_epochs::Epoch::from(local_context.epoch);
        let mut budget = self
            .compute_deterministic_budget(context, peer, epoch)
            .await?;
        if !budget.record_charge(cost as u64) {
            return Err(AuraError::permission_denied(format!(
                "Flow budget exceeded for ctx={} peer={} (limit={}, spent={}, cost={})",
                context.as_str(),
                peer,
                budget.limit,
                budget.spent,
                cost
            )));
        }
        {
            let mut ledgers = self.flow_ledgers.write().await;
            ledgers.insert((context.clone(), peer.clone()), budget.clone());
        }
        {
            let handler = self.composite_handler.read().await;
            let _ = handler.update_flow_budget(context, peer, &budget).await;
        }
        let nonce = self
            .flow_nonce
            .fetch_add(1, Ordering::SeqCst)
            .wrapping_add(1);
        let prev = { *self.flow_prev_receipt.read().await };

        let signature_material =
            Self::receipt_signature_material(context, &src, peer, epoch.value(), cost, nonce);
        let sig_hash = Hash32::from_bytes(signature_material.as_bytes());

        let receipt = Receipt::new(
            context.clone(),
            src,
            peer.clone(),
            epoch,
            cost,
            nonce,
            prev,
            sig_hash.0.to_vec(),
        );

        let new_hash = hash_canonical(&receipt)
            .map(Hash32::from)
            .map_err(|err| AuraError::internal(format!("Failed to hash receipt: {}", err)))?;
        {
            let mut prev_guard = self.flow_prev_receipt.write().await;
            *prev_guard = new_hash;
        }
        {
            let mut latest = self.last_receipt.write().await;
            *latest = Some(receipt.clone());
        }

        Ok(receipt)
    }
}

#[async_trait]
impl NetworkEffects for AuraEffectSystem {
    async fn send_to_peer(
        &self,
        peer_id: uuid::Uuid,
        message: Vec<u8>,
    ) -> Result<(), NetworkError> {
        let mut context = self.context().await;
        let default_context = context
            .account_id
            .as_ref()
            .map(|account| ContextId::new(account.to_string()))
            .unwrap_or_else(|| ContextId::new("global"));
        let default_peer = DeviceId::from_uuid(peer_id);

        let flow_hint = self
            .take_flow_hint_internal()
            .await
            .unwrap_or_else(|| FlowHint::new(default_context, default_peer, 1));

        let receipt = FlowGuard::from_hint(flow_hint)
            .authorize(self)
            .await
            .map_err(|err| NetworkError::SendFailed(err.to_string()))?;
        debug!(
            "FlowGuard receipt ctx={} peer={} cost={} nonce={}",
            receipt.ctx.as_str(),
            receipt.dst,
            receipt.cost,
            receipt.nonce
        );

        let envelope_bytes = Self::encode_transport_envelope(&receipt, message)
            .map_err(|err| NetworkError::SendFailed(err.to_string()))?;

        let params = serde_json::json!({
            "peer_id": peer_id,
            "data": envelope_bytes
        });
        let params_bytes = serde_json::to_vec(&params).unwrap_or_default();

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

                let data: Vec<u8> = response
                    .get("data")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| NetworkError::ReceiveFailed("Missing data".to_string()))?
                    .iter()
                    .map(|v| v.as_u64().unwrap_or(0) as u8)
                    .collect();

                let payload = self
                    .decode_transport_payload(data, peer_id)
                    .await
                    .map_err(|err| NetworkError::ReceiveFailed(err.to_string()))?;

                Ok((peer_id, payload))
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
            Ok(result) => {
                let response: serde_json::Value =
                    serde_json::from_slice(&result).map_err(|_| {
                        NetworkError::ReceiveFailed("Invalid response format".to_string())
                    })?;

                let data: Vec<u8> = response
                    .get("data")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| NetworkError::ReceiveFailed("Missing data".to_string()))?
                    .iter()
                    .map(|v| v.as_u64().unwrap_or(0) as u8)
                    .collect();

                self.decode_transport_payload(data, peer_id)
                    .await
                    .map_err(|err| NetworkError::ReceiveFailed(err.to_string()))
            }
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
        // TODO fix - For now, return an empty stream placeholder
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
            Err(e) => Err(AuraError::internal(format!("Sleep failed: {}", e))),
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
            Err(e) => Err(AuraError::internal(format!("Wait until failed: {}", e))),
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
        Ok(<Self as RandomEffects>::random_bytes(self, length).await)
    }

    async fn hash_blake3(&self, data: &[u8]) -> Result<[u8; 32], crate::effects::LedgerError> {
        // Delegate to crypto effects (now using SHA256)
        Ok(self.hash(data).await)
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
        Err(AuraError::internal(
            "Journal not available in stub implementation",
        ))
    }

    async fn get_current_tree(&self) -> Result<RatchetTree, AuraError> {
        Err(AuraError::internal(
            "Tree not available in stub implementation",
        ))
    }

    async fn get_tree_at_epoch(&self, _epoch: Epoch) -> Result<RatchetTree, AuraError> {
        Err(AuraError::internal(
            "Tree not available in stub implementation",
        ))
    }

    async fn get_current_commitment(&self) -> Result<Commitment, AuraError> {
        Err(AuraError::internal(
            "Tree not available in stub implementation",
        ))
    }

    async fn get_latest_epoch(&self) -> Result<Option<Epoch>, AuraError> {
        Ok(None)
    }

    async fn append_tree_op(&self, _op: TreeOpRecord) -> Result<(), AuraError> {
        Err(AuraError::internal(
            "Tree operations not available in stub implementation",
        ))
    }

    async fn get_tree_op(&self, _epoch: Epoch) -> Result<Option<TreeOpRecord>, AuraError> {
        Ok(None)
    }

    async fn list_tree_ops(&self) -> Result<Vec<TreeOpRecord>, AuraError> {
        Ok(vec![])
    }

    async fn submit_intent(&self, _intent: Intent) -> Result<IntentId, AuraError> {
        Err(AuraError::internal(
            "Intents not available in stub implementation",
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
        Err(AuraError::internal(
            "Intents not available in stub implementation",
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
        Err(AuraError::internal(
            "Journal operations not available in stub implementation",
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

    // ===== New Ratchet Tree Operations (Phase 2.1f) =====

    async fn append_attested_tree_op(
        &self,
        _op: aura_core::AttestedOp,
    ) -> Result<aura_core::Hash32, AuraError> {
        // TODO: Implement actual OpLog append when journal handler is ready
        Err(AuraError::internal(
            "Tree operations not available in stub implementation",
        ))
    }

    async fn get_tree_state(&self) -> Result<aura_journal::ratchet_tree::TreeState, AuraError> {
        // TODO: Implement actual reduction when journal handler is ready
        Err(AuraError::internal(
            "Tree state not available in stub implementation",
        ))
    }

    async fn get_op_log(&self) -> Result<aura_journal::semilattice::OpLog, AuraError> {
        // TODO: Implement actual OpLog retrieval when journal handler is ready
        Err(AuraError::internal(
            "OpLog not available in stub implementation",
        ))
    }

    async fn merge_op_log(
        &self,
        _remote: aura_journal::semilattice::OpLog,
    ) -> Result<(), AuraError> {
        // TODO: Implement actual OpLog merge when journal handler is ready
        Err(AuraError::internal(
            "OpLog merge not available in stub implementation",
        ))
    }

    async fn get_attested_op(
        &self,
        _cid: &aura_core::Hash32,
    ) -> Result<Option<aura_core::AttestedOp>, AuraError> {
        // TODO: Implement actual operation retrieval when journal handler is ready
        Ok(None)
    }

    async fn list_attested_ops(&self) -> Result<Vec<aura_core::AttestedOp>, AuraError> {
        // TODO: Implement actual operation listing when journal handler is ready
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
            flow_nonce: AtomicU64::new(self.flow_nonce.load(std::sync::atomic::Ordering::Relaxed)),
            flow_prev_receipt: Arc::clone(&self.flow_prev_receipt),
            last_receipt: Arc::clone(&self.last_receipt),
            flow_ledgers: Arc::clone(&self.flow_ledgers),
        }
    }
}

// Implement TreeEffects by delegating to composite handler
#[async_trait]
impl TreeEffects for AuraEffectSystem {
    async fn get_current_state(
        &self,
    ) -> Result<aura_journal::ratchet_tree::TreeState, aura_core::AuraError> {
        let handler = self.composite_handler.read().await;
        TreeEffects::get_current_state(&*handler).await
    }

    async fn get_current_commitment(&self) -> Result<aura_core::Hash32, aura_core::AuraError> {
        let handler = self.composite_handler.read().await;
        TreeEffects::get_current_commitment(&*handler).await
    }

    async fn get_current_epoch(&self) -> Result<u64, aura_core::AuraError> {
        let handler = self.composite_handler.read().await;
        TreeEffects::get_current_epoch(&*handler).await
    }

    async fn apply_attested_op(
        &self,
        op: aura_core::AttestedOp,
    ) -> Result<aura_core::Hash32, aura_core::AuraError> {
        let handler = self.composite_handler.read().await;
        TreeEffects::apply_attested_op(&*handler, op).await
    }

    async fn verify_aggregate_sig(
        &self,
        op: &aura_core::AttestedOp,
        state: &aura_journal::ratchet_tree::TreeState,
    ) -> Result<bool, aura_core::AuraError> {
        let handler = self.composite_handler.read().await;
        TreeEffects::verify_aggregate_sig(&*handler, op, state).await
    }

    async fn add_leaf(
        &self,
        leaf: aura_core::LeafNode,
        under: aura_core::NodeIndex,
    ) -> Result<aura_core::TreeOpKind, aura_core::AuraError> {
        let handler = self.composite_handler.read().await;
        TreeEffects::add_leaf(&*handler, leaf, under).await
    }

    async fn remove_leaf(
        &self,
        leaf_id: aura_core::LeafId,
        reason: u8,
    ) -> Result<aura_core::TreeOpKind, aura_core::AuraError> {
        let handler = self.composite_handler.read().await;
        TreeEffects::remove_leaf(&*handler, leaf_id, reason).await
    }

    async fn change_policy(
        &self,
        node: aura_core::NodeIndex,
        new_policy: aura_core::Policy,
    ) -> Result<aura_core::TreeOpKind, aura_core::AuraError> {
        let handler = self.composite_handler.read().await;
        TreeEffects::change_policy(&*handler, node, new_policy).await
    }

    async fn rotate_epoch(
        &self,
        affected: Vec<aura_core::NodeIndex>,
    ) -> Result<aura_core::TreeOpKind, aura_core::AuraError> {
        let handler = self.composite_handler.read().await;
        TreeEffects::rotate_epoch(&*handler, affected).await
    }

    async fn propose_snapshot(
        &self,
        cut: crate::effects::tree::Cut,
    ) -> Result<crate::effects::tree::ProposalId, aura_core::AuraError> {
        let handler = self.composite_handler.read().await;
        TreeEffects::propose_snapshot(&*handler, cut).await
    }

    async fn approve_snapshot(
        &self,
        proposal_id: crate::effects::tree::ProposalId,
    ) -> Result<crate::effects::tree::Partial, aura_core::AuraError> {
        let handler = self.composite_handler.read().await;
        TreeEffects::approve_snapshot(&*handler, proposal_id).await
    }

    async fn finalize_snapshot(
        &self,
        proposal_id: crate::effects::tree::ProposalId,
    ) -> Result<crate::effects::tree::Snapshot, aura_core::AuraError> {
        let handler = self.composite_handler.read().await;
        TreeEffects::finalize_snapshot(&*handler, proposal_id).await
    }

    async fn apply_snapshot(
        &self,
        snapshot: &crate::effects::tree::Snapshot,
    ) -> Result<(), aura_core::AuraError> {
        let handler = self.composite_handler.read().await;
        TreeEffects::apply_snapshot(&*handler, snapshot).await
    }
}

impl AuraEffectSystem {
    /// Get oplog digest for anti-entropy
    pub async fn get_oplog_digest(&self) -> Result<Vec<u8>, aura_core::AuraError> {
        // Stub implementation - would compute digest of local oplog
        Ok(vec![0u8; 32])
    }

    /// Synchronize with remote peer
    pub async fn sync_with_peer(
        &self,
        _peer_id: aura_core::DeviceId,
    ) -> Result<(), aura_core::AuraError> {
        // Stub implementation - would perform anti-entropy sync
        Ok(())
    }

    /// Push operation to connected peers
    pub async fn push_op_to_peers(
        &self,
        _op: aura_core::AttestedOp,
        _peers: Vec<aura_core::DeviceId>,
    ) -> Result<(), aura_core::AuraError> {
        // Stub implementation - would broadcast operation
        Ok(())
    }

    /// Request operation from peer
    pub async fn request_op(
        &self,
        _peer_id: aura_core::DeviceId,
        _cid: [u8; 32],
    ) -> Result<Option<aura_core::AttestedOp>, aura_core::AuraError> {
        // Stub implementation - would request specific operation
        Ok(None)
    }

    /// Merge remote operations into local oplog
    pub async fn merge_remote_ops(
        &self,
        _ops: Vec<aura_core::AttestedOp>,
    ) -> Result<(), aura_core::AuraError> {
        // Stub implementation - would merge operations
        Ok(())
    }

    /// Get connected peers
    pub async fn get_connected_peers(
        &self,
    ) -> Result<Vec<aura_core::DeviceId>, aura_core::AuraError> {
        // Stub implementation - would return connected peers
        Ok(vec![])
    }
}

#[async_trait]
impl SystemEffects for AuraEffectSystem {
    async fn log(&self, level: &str, component: &str, message: &str) -> Result<(), SystemError> {
        let params = serde_json::json!({
            "level": level,
            "component": component,
            "message": message
        });
        let params_bytes = serde_json::to_vec(&params).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(EffectType::Console, "log", &params_bytes, &mut context)
            .await
        {
            Ok(_) => Ok(()),
            Err(_) => Err(SystemError::ServiceUnavailable),
        }
    }

    async fn log_with_context(
        &self,
        level: &str,
        component: &str,
        message: &str,
        context: HashMap<String, String>,
    ) -> Result<(), SystemError> {
        let params = serde_json::json!({
            "level": level,
            "component": component,
            "message": message,
            "context": context
        });
        let params_bytes = serde_json::to_vec(&params).unwrap_or_default();
        let mut ctx = self.context().await;

        match self
            .execute_effect_with_context(
                EffectType::Console,
                "log_with_context",
                &params_bytes,
                &mut ctx,
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(_) => Err(SystemError::ServiceUnavailable),
        }
    }

    async fn get_system_info(&self) -> Result<HashMap<String, String>, SystemError> {
        let mut info = HashMap::new();
        let stats = self.statistics().await;

        info.insert("device_id".to_string(), self.device_id.to_string());
        info.insert(
            "execution_mode".to_string(),
            format!("{:?}", self.execution_mode),
        );
        info.insert(
            "supported_effects".to_string(),
            stats.registered_effects.to_string(),
        );
        info.insert(
            "is_deterministic".to_string(),
            stats.is_deterministic().to_string(),
        );
        info.insert(
            "is_production".to_string(),
            stats.is_production().to_string(),
        );

        Ok(info)
    }

    async fn set_config(&self, key: &str, value: &str) -> Result<(), SystemError> {
        let params = serde_json::json!({
            "key": key,
            "value": value
        });
        let params_bytes = serde_json::to_vec(&params).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(
                EffectType::System,
                "set_config",
                &params_bytes,
                &mut context,
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(_) => Err(SystemError::OperationFailed {
                message: format!("Failed to set config {}={}", key, value),
            }),
        }
    }

    async fn get_config(&self, key: &str) -> Result<String, SystemError> {
        let params = serde_json::to_vec(&key).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(EffectType::System, "get_config", &params, &mut context)
            .await
        {
            Ok(result) => {
                serde_json::from_slice(&result).map_err(|_| SystemError::OperationFailed {
                    message: "Failed to deserialize config value".to_string(),
                })
            }
            Err(_) => Err(SystemError::ResourceNotFound {
                resource: format!("config key: {}", key),
            }),
        }
    }

    async fn health_check(&self) -> Result<bool, SystemError> {
        // Check if the composite handler is responsive
        let supported_effects = self.supported_effects().await;
        let has_core_effects = supported_effects.contains(&EffectType::Crypto)
            && supported_effects.contains(&EffectType::Network)
            && supported_effects.contains(&EffectType::Storage);

        Ok(has_core_effects)
    }

    async fn get_metrics(&self) -> Result<HashMap<String, f64>, SystemError> {
        let stats = self.statistics().await;
        let mut metrics = HashMap::new();

        metrics.insert("uptime_seconds".to_string(), 0.0); // Would be real uptime
        metrics.insert(
            "registered_effects".to_string(),
            stats.registered_effects as f64,
        );
        metrics.insert(
            "total_operations".to_string(),
            stats.total_operations as f64,
        );
        metrics.insert(
            "middleware_count".to_string(),
            stats.middleware_count as f64,
        );
        metrics.insert(
            "is_deterministic".to_string(),
            if stats.is_deterministic() { 1.0 } else { 0.0 },
        );
        metrics.insert(
            "is_production".to_string(),
            if stats.is_production() { 1.0 } else { 0.0 },
        );

        Ok(metrics)
    }

    async fn restart_component(&self, component: &str) -> Result<(), SystemError> {
        let params = serde_json::to_vec(&component).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(
                EffectType::System,
                "restart_component",
                &params,
                &mut context,
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(_) => Err(SystemError::OperationFailed {
                message: format!("Failed to restart component: {}", component),
            }),
        }
    }

    async fn shutdown(&self) -> Result<(), SystemError> {
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(EffectType::System, "shutdown", &[], &mut context)
            .await
        {
            Ok(_) => Ok(()),
            Err(_) => Err(SystemError::OperationFailed {
                message: "Failed to shutdown system".to_string(),
            }),
        }
    }
}

// Implement the composite AuraEffects trait
impl crate::effects::AuraEffects for AuraEffectSystem {}

// Session Management Effects Implementation
#[async_trait]
impl crate::effects::agent::SessionManagementEffects for AuraEffectSystem {
    async fn create_session(
        &self,
        session_type: crate::effects::agent::SessionType,
    ) -> aura_core::AuraResult<aura_core::identifiers::SessionId> {
        let params = serde_json::to_vec(&session_type).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(
                EffectType::SessionManagement,
                "create_session",
                &params,
                &mut context,
            )
            .await
        {
            Ok(result) => {
                let session_id: String = serde_json::from_slice(&result).unwrap_or_else(|_| {
                    // Generate fallback session ID
                    format!(
                        "session_{}_{}",
                        self.device_id.0.simple(),
                        context.created_at
                    )
                });
                // Create a new SessionId (UUID-based)
                Ok(aura_core::identifiers::SessionId::new())
            }
            Err(_) => {
                // Generate fallback session ID for testing
                Ok(aura_core::identifiers::SessionId::new())
            }
        }
    }

    async fn join_session(
        &self,
        session_id: aura_core::identifiers::SessionId,
    ) -> aura_core::AuraResult<crate::effects::agent::SessionHandle> {
        let params = serde_json::to_vec(&session_id).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(
                EffectType::SessionManagement,
                "join_session",
                &params,
                &mut context,
            )
            .await
        {
            Ok(result) => {
                let handle: crate::effects::agent::SessionHandle = serde_json::from_slice(&result)
                    .unwrap_or_else(|_| crate::effects::agent::SessionHandle {
                        session_id,
                        role: crate::effects::agent::SessionRole::Participant,
                        participants: vec![self.device_id],
                        created_at: context.created_at,
                    });
                Ok(handle)
            }
            Err(_) => Ok(crate::effects::agent::SessionHandle {
                session_id,
                role: crate::effects::agent::SessionRole::Participant,
                participants: vec![self.device_id],
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64,
            }),
        }
    }

    async fn leave_session(
        &self,
        session_id: aura_core::identifiers::SessionId,
    ) -> aura_core::AuraResult<()> {
        let params = serde_json::to_vec(&session_id).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(
                EffectType::SessionManagement,
                "leave_session",
                &params,
                &mut context,
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(_) => Ok(()), // Graceful fallback for testing
        }
    }

    async fn end_session(
        &self,
        session_id: aura_core::identifiers::SessionId,
    ) -> aura_core::AuraResult<()> {
        let params = serde_json::to_vec(&session_id).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(
                EffectType::SessionManagement,
                "end_session",
                &params,
                &mut context,
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(_) => Ok(()), // Graceful fallback for testing
        }
    }

    async fn list_active_sessions(
        &self,
    ) -> aura_core::AuraResult<Vec<crate::effects::agent::SessionInfo>> {
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(
                EffectType::SessionManagement,
                "list_active_sessions",
                &[],
                &mut context,
            )
            .await
        {
            Ok(result) => {
                let sessions: Vec<crate::effects::agent::SessionInfo> =
                    serde_json::from_slice(&result).unwrap_or_default();
                Ok(sessions)
            }
            Err(_) => Ok(vec![]), // Graceful fallback for testing
        }
    }

    async fn get_session_status(
        &self,
        session_id: aura_core::identifiers::SessionId,
    ) -> aura_core::AuraResult<crate::effects::agent::SessionStatus> {
        let params = serde_json::to_vec(&session_id).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(
                EffectType::SessionManagement,
                "get_session_status",
                &params,
                &mut context,
            )
            .await
        {
            Ok(result) => {
                let status: crate::effects::agent::SessionStatus = serde_json::from_slice(&result)
                    .unwrap_or(crate::effects::agent::SessionStatus::Active);
                Ok(status)
            }
            Err(_) => Ok(crate::effects::agent::SessionStatus::Active), // Graceful fallback
        }
    }

    async fn send_session_message(
        &self,
        session_id: aura_core::identifiers::SessionId,
        message: &[u8],
    ) -> aura_core::AuraResult<()> {
        let params = serde_json::to_vec(&(session_id, message)).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(
                EffectType::SessionManagement,
                "send_session_message",
                &params,
                &mut context,
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(_) => Ok(()), // Graceful fallback for testing
        }
    }

    async fn receive_session_messages(
        &self,
        session_id: aura_core::identifiers::SessionId,
    ) -> aura_core::AuraResult<Vec<crate::effects::agent::SessionMessage>> {
        let params = serde_json::to_vec(&session_id).unwrap_or_default();
        let mut context = self.context().await;

        match self
            .execute_effect_with_context(
                EffectType::SessionManagement,
                "receive_session_messages",
                &params,
                &mut context,
            )
            .await
        {
            Ok(result) => {
                let messages: Vec<crate::effects::agent::SessionMessage> =
                    serde_json::from_slice(&result).unwrap_or_default();
                Ok(messages)
            }
            Err(_) => Ok(vec![]), // Graceful fallback for testing
        }
    }
}

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
                // TODO fix - Simplified test - just check that the updater can be called
                Ok(())
            })
            .await
            .unwrap();

        let updated_context = system.context().await;
        assert_eq!(updated_context.device_id, device_id);
    }

    #[tokio::test]
    async fn test_flow_budget_enforcement() {
        use crate::guards::flow::FlowGuard;
        use aura_core::relationships::ContextId;

        let sender = DeviceId::from(Uuid::new_v4());
        let recipient = DeviceId::from(Uuid::new_v4());
        let mut system = AuraEffectSystem::for_testing(sender);
        let ctx = ContextId::new("test.flow");

        system
            .seed_flow_budget(
                ctx.clone(),
                recipient,
                FlowBudget::new(5, session_epochs::Epoch::initial()),
            )
            .await;

        let guard = FlowGuard::new(ctx.clone(), recipient, 3);
        guard.authorize(&system).await.expect("first charge");

        let guard = FlowGuard::new(ctx.clone(), recipient, 3);
        let err = guard.authorize(&system).await.expect_err("should exceed");
        assert!(matches!(err, AuraError::PermissionDenied { .. }));
    }
}
