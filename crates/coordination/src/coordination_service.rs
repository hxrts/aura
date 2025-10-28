//! Coordination Service Layer
//!
//! High-level orchestration for distributed protocols with abstraction over
//! complex protocol context construction and session runtime management.

use crate::types::ThresholdSignature;
use crate::{
    execution::{ProtocolContext, context::ProtocolContextBuilder},
    local_runtime::LocalSessionRuntime,
    LifecycleScheduler,
    Transport,
};
use aura_crypto::Effects;
use aura_errors::{AuraError, Result};
use aura_types::{AccountId, DeviceId, GuardianId};
use aura_journal::{AccountLedger, SessionId};
use ed25519_dalek::{SigningKey, VerifyingKey};
use std::sync::Arc;
use tokio::sync::RwLock;

/// High-level coordination service for distributed protocols
///
/// Encapsulates the complexity of protocol context construction and session runtime
/// management, providing clean APIs for protocol orchestration.
pub struct CoordinationService {
    /// Local session runtime for protocol execution
    session_runtime: LocalSessionRuntime,
    /// Protocol context factory for complex setup
    protocol_factory: Arc<ProtocolContextFactory>,
    /// Transport layer for network communication
    transport: Arc<dyn Transport>,
    /// Crypto service for cryptographic operations
    crypto_service: Arc<CryptoService>,
}

impl CoordinationService {
    /// Create new coordination service
    pub fn new(
        session_runtime: LocalSessionRuntime,
        transport: Arc<dyn Transport>,
        crypto_service: Arc<CryptoService>,
        effects: Effects,
    ) -> Self {
        let protocol_factory = Arc::new(ProtocolContextFactory::new(
            transport.clone(),
            crypto_service.clone(),
            effects,
        ));

        Self {
            session_runtime,
            protocol_factory,
            transport,
            crypto_service,
        }
    }

    /// Execute DKD protocol with high-level orchestration
    ///
    /// # Arguments
    /// * `request` - DKD request with app_id, context, and participants
    /// * `ledger` - Active account ledger with current state
    /// * `device_key` - Real device signing key
    /// * `device_secret` - Real device HPKE private key
    ///
    /// # Returns
    /// * `DerivedIdentity` - Successfully derived identity with keys
    pub async fn execute_dkd(
        &mut self,
        request: DkdRequest,
        _ledger: Arc<RwLock<AccountLedger>>,
        _device_key: SigningKey,
        _device_secret: aura_crypto::HpkePrivateKey,
    ) -> Result<DerivedIdentity> {
        // Create protocol context through factory with real data
        let mut protocol_ctx = self
            .protocol_factory
            .create_dkd_context(&request)
            .await
            .map_err(|e| {
                AuraError::coordination_failed(format!("Protocol setup failed: {:?}", e))
            })?;

        // Execute DKD through LifecycleScheduler
        let scheduler = LifecycleScheduler::with_effects(Effects::production());
        let context_id = request.context.as_bytes().to_vec();
        let dkd_result = scheduler.execute_dkd(
            None, // session_id - will be generated
            request.account_id,
            request.device_id,
            request.app_id.clone(),
            request.context.clone(),
            request.participants.clone(),
            request.threshold,
            context_id,
            None, // ledger - use scheduler's default
            None, // transport - use scheduler's default
        )
            .await
            .map_err(|e| {
                AuraError::coordination_failed(format!("DKD lifecycle failed: {:?}", e))
            })?;

        // Convert DkdProtocolResult to DerivedIdentity
        Ok(DerivedIdentity {
            session_id: dkd_result.session_id,
            derived_key: dkd_result.derived_key,
            derived_public_key: dkd_result.derived_public_key,
            transcript_hash: dkd_result.transcript_hash,
            threshold_signature: dkd_result.threshold_signature,
            participants: dkd_result.participants,
        })
    }

    /// Execute resharing protocol coordination
    ///
    /// # Arguments
    /// * `request` - Resharing request with new participants and threshold
    /// * `ledger` - Active account ledger with current state
    /// * `device_key` - Real device signing key
    /// * `device_secret` - Real device HPKE private key
    ///
    /// # Returns
    /// * `ResharingResult` - New key shares and updated configuration
    pub async fn execute_resharing(
        &mut self,
        request: ResharingRequest,
        _ledger: Arc<RwLock<AccountLedger>>,
        _device_key: SigningKey,
        _device_secret: aura_crypto::HpkePrivateKey,
    ) -> Result<ResharingResult> {
        // Create protocol context through factory with real data
        let mut protocol_ctx = self
            .protocol_factory
            .create_resharing_context(&request)
            .await
            .map_err(|e| {
                AuraError::coordination_failed(format!("Protocol setup failed: {:?}", e))
            })?;

        // Execute Resharing through LifecycleScheduler
        let scheduler = LifecycleScheduler::with_effects(Effects::production());
        let result = scheduler.execute_resharing(
            None, // session_id - will be generated
            request.account_id,
            request.device_id,
            request.current_participants.clone(),
            request.new_participants.clone(),
            request.new_threshold,
            None, // ledger - use scheduler's default
            None, // transport - use scheduler's default
        )
        .await
        .map_err(|e| {
            AuraError::coordination_failed(format!("Resharing lifecycle failed: {:?}", e))
        })?;

        // Convert ResharingProtocolResult to ResharingResult
        Ok(ResharingResult {
            session_id: result.session_id,
            new_threshold: result.new_threshold,
            new_participants: result.new_participants.clone(),
            old_participants: result.old_participants.clone(),
            approval_signature: result.approval_signature.clone(),
        })
    }

    /// Execute recovery protocol coordination
    ///
    /// # Arguments
    /// * `request` - Recovery request with guardian approvals and parameters
    /// * `ledger` - Active account ledger with current state
    /// * `device_key` - Real device signing key
    /// * `device_secret` - Real device HPKE private key
    ///
    /// # Returns
    /// * `RecoveryResult` - Reconstructed shares and new device integration
    pub async fn execute_recovery(
        &mut self,
        request: RecoveryRequest,
        _ledger: Arc<RwLock<AccountLedger>>,
        _device_key: SigningKey,
        _device_secret: aura_crypto::HpkePrivateKey,
    ) -> Result<RecoveryResult> {
        // Create protocol context through factory with real data
        let mut protocol_ctx = self
            .protocol_factory
            .create_recovery_context(&request)
            .await
            .map_err(|e| {
                AuraError::coordination_failed(format!("Protocol setup failed: {:?}", e))
            })?;

        // Execute recovery choreography through session runtime
        // Guardian list is already provided in the request
        let guardian_ids = request.guardian_list.clone();

        if guardian_ids.len() < request.required_threshold as usize {
            return Err(AuraError::coordination_failed(format!(
                "Insufficient valid guardians: {} < {}",
                guardian_ids.len(),
                request.required_threshold
            )));
        }

        // Execute Recovery through LifecycleScheduler
        let scheduler = LifecycleScheduler::with_effects(Effects::production());
        let result = scheduler.execute_recovery(
            None, // session_id - will be generated
            request.account_id,
            request.device_id,
            guardian_ids,
            request.device_id, // new_device_id for recovery (same device)
            request.required_threshold as usize,
            None, // ledger - use scheduler's default
            None, // transport - use scheduler's default
        )
        .await
        .map_err(|e| {
            AuraError::coordination_failed(format!("Recovery lifecycle failed: {:?}", e))
        })?;

        // Convert RecoveryProtocolResult to RecoveryResult
        Ok(RecoveryResult {
            session_id: result.session_id,
            new_device_id: result.new_device_id,
            approving_guardians: result.approving_guardians.clone(),
            recovered_share: result.recovered_share.clone(),
        })
    }

    /// Get session runtime statistics
    pub fn get_runtime_stats(&self) -> RuntimeStats {
        RuntimeStats {
            active_sessions: 0,     // Placeholder - would query session runtime
            completed_protocols: 0, // Placeholder - would query session runtime
            connected_peers: 0,     // Placeholder - would query transport
        }
    }
}

/// Protocol context factory for encapsulating complex protocol setup
pub struct ProtocolContextFactory {
    /// Transport layer reference
    transport: Arc<dyn Transport>,
    /// Crypto service reference
    crypto_service: Arc<CryptoService>,
    /// Effects for deterministic operations
    effects: Effects,
}

impl ProtocolContextFactory {
    /// Create new protocol context factory
    pub fn new(
        transport: Arc<dyn Transport>,
        crypto_service: Arc<CryptoService>,
        effects: Effects,
    ) -> Self {
        Self {
            transport,
            crypto_service,
            effects,
        }
    }

    /// Create DKD protocol context with all required dependencies
    ///
    /// Uses ContextBuilder to validate capabilities and load real state
    pub async fn create_dkd_context(&self, request: &DkdRequest) -> Result<ProtocolContext> {
        let mut builder = ProtocolContextBuilder::new(
            self.crypto_service.clone(),
            self.transport.clone(),
            self.effects.clone(),
        );

        // Load and validate ledger
        builder.with_ledger(request.account_id).await?;

        // Validate device authorization
        builder.validate_device(request.device_id).await?;

        // Build validated context
        builder
            .build_dkd_context(
                request.app_id.clone(),
                request.context.clone(),
                request.participants.clone(),
                request.threshold,
            )
            .await
    }

    /// Create resharing protocol context
    pub async fn create_resharing_context(
        &self,
        request: &ResharingRequest,
    ) -> Result<ProtocolContext> {
        let mut builder = ProtocolContextBuilder::new(
            self.crypto_service.clone(),
            self.transport.clone(),
            self.effects.clone(),
        );

        // Load and validate ledger
        builder.with_ledger(request.account_id).await?;

        // Validate device authorization
        builder.validate_device(request.device_id).await?;

        // Build validated context
        builder
            .build_resharing_context(
                request.current_participants.clone(),
                request.new_participants.clone(),
                request.current_threshold,
                request.new_threshold,
            )
            .await
    }

    /// Create recovery protocol context
    pub async fn create_recovery_context(
        &self,
        request: &RecoveryRequest,
    ) -> Result<ProtocolContext> {
        let mut builder = ProtocolContextBuilder::new(
            self.crypto_service.clone(),
            self.transport.clone(),
            self.effects.clone(),
        );

        // Load and validate ledger
        builder.with_ledger(request.account_id).await?;

        // Validate device authorization
        builder.validate_device(request.device_id).await?;

        // Build validated context
        builder
            .build_recovery_context(
                request.guardian_list.clone(),
                request.required_threshold,
                request.cooldown_hours,
            )
            .await
    }
}

/// Crypto service abstraction for key management and signing operations
pub struct CryptoService {
    /// Device key manager for raw key access
    pub key_manager: Arc<aura_crypto::DeviceKeyManager>,
    /// Secure storage for encrypted key shares
    pub secure_storage: Arc<dyn SecureStorage>,
}

impl CryptoService {
    /// Create new crypto service
    pub fn new(
        key_manager: Arc<aura_crypto::DeviceKeyManager>,
        secure_storage: Arc<dyn SecureStorage>,
    ) -> Self {
        Self {
            key_manager,
            secure_storage,
        }
    }

    /// Get signing context for protocol operations
    ///
    /// Abstracts access to cryptographic material without exposing raw keys
    /// to higher-level APIs.
    pub async fn get_signing_context(&self) -> Result<SigningContext> {
        // Abstract key access - don't expose raw keys to high-level APIs
        let device_key_fingerprint = self.extract_device_key_fingerprint().await?;
        let threshold_key_available = self.check_threshold_key_availability().await?;

        Ok(SigningContext {
            device_id: DeviceId(
                self.key_manager
                    .get_device_id()
                    .unwrap_or_else(|| uuid::Uuid::new_v4()),
            ),
            device_key_fingerprint,
            threshold_key_available,
            secure_storage: self.secure_storage.clone(),
        })
    }

    /// Create threshold keys for account initialization
    pub async fn create_threshold_keys(
        &self,
        participants: Vec<DeviceId>,
        threshold: u16,
    ) -> Result<ThresholdKeySet> {
        // High-level key generation workflow
        let key_generation_result = self.execute_key_generation(participants, threshold).await?;

        // Store encrypted shares securely
        self.store_key_shares_securely(&key_generation_result)
            .await?;

        Ok(key_generation_result.into())
    }

    /// Internal: Extract device key fingerprint without exposing raw key
    async fn extract_device_key_fingerprint(&self) -> Result<[u8; 32]> {
        // Extract device public key and compute fingerprint
        match self.key_manager.get_device_key() {
            Some(device_key) => {
                // Compute Blake3 fingerprint of public key
                let fingerprint = blake3::hash(device_key.verifying_key.as_bytes());
                Ok(*fingerprint.as_bytes())
            }
            None => Err(AuraError::crypto_operation_failed(
                "No device key available",
            )),
        }
    }

    /// Internal: Check if threshold key shares are available
    async fn check_threshold_key_availability(&self) -> Result<bool> {
        // Check secure storage for encrypted threshold key shares
        self.secure_storage
            .has_threshold_keys_sync()
            .map_err(|e| AuraError::storage_read_failed(e))
    }

    /// Internal: Execute key generation protocol
    async fn execute_key_generation(
        &self,
        _participants: Vec<DeviceId>,
        _threshold: u16,
    ) -> Result<KeyGenerationResult> {
        // FROST key generation not yet implemented in Phase 0
        unimplemented!("FROST key generation not implemented in Phase 0")
    }

    /// Internal: Store key shares with encryption
    async fn store_key_shares_securely(&self, _result: &KeyGenerationResult) -> Result<()> {
        // This method should only be called after key generation succeeds
        // Since execute_key_generation is unimplemented, this should never be reached
        unreachable!("store_key_shares_securely called but key generation is unimplemented")
    }
}

// Type definitions and error types

/// Signing context for protocol operations
pub struct SigningContext {
    pub device_id: DeviceId,
    pub device_key_fingerprint: [u8; 32],
    pub threshold_key_available: bool,
    pub secure_storage: Arc<dyn SecureStorage>,
}

/// Runtime statistics
pub struct RuntimeStats {
    pub active_sessions: usize,
    pub completed_protocols: usize,
    pub connected_peers: usize,
}

// CoordinationError removed - using AuraError directly

/// Protocol setup errors
#[derive(Debug, thiserror::Error)]
pub enum ProtocolSetupError {
    #[error("Crypto access failed: {0}")]
    CryptoAccess(AuraError),
    #[error("Transport unavailable: {0}")]
    TransportUnavailable(String),
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),
}

// Crypto errors now use unified AuraError system

/// Secure storage trait for protocol execution
pub trait SecureStorage: Send + Sync {
    fn has_threshold_keys_sync(&self) -> std::result::Result<bool, String>;
    fn store_encrypted_share_sync(
        &self,
        share_id: &str,
        encrypted_data: &[u8],
    ) -> std::result::Result<(), String>;
    fn retrieve_encrypted_share_sync(&self, share_id: &str)
        -> std::result::Result<Vec<u8>, String>;
}

// Result types with actual protocol payloads

/// Result of DKD protocol with derived identity and proof
pub struct DerivedIdentity {
    pub session_id: SessionId,
    pub derived_key: Vec<u8>,
    pub derived_public_key: VerifyingKey,
    pub transcript_hash: [u8; 32],
    pub threshold_signature: ThresholdSignature,
    pub participants: Vec<DeviceId>,
}

/// Result of resharing protocol with new shares and proof
pub struct ResharingResult {
    pub session_id: SessionId,
    pub new_threshold: u16,
    pub new_participants: Vec<DeviceId>,
    pub old_participants: Vec<DeviceId>,
    pub approval_signature: ThresholdSignature,
}

/// Result of recovery protocol with new device integration
pub struct RecoveryResult {
    pub session_id: SessionId,
    pub new_device_id: DeviceId,
    pub approving_guardians: Vec<GuardianId>,
    pub recovered_share: Vec<u8>,
}

pub struct ThresholdKeySet {
    pub public_key: VerifyingKey,
    pub threshold: u16,
    pub total_shares: u16,
}

pub struct KeyGenerationResult {
    pub key_set: ThresholdKeySet,
    pub shares: Vec<Vec<u8>>,
}

impl From<KeyGenerationResult> for ThresholdKeySet {
    fn from(result: KeyGenerationResult) -> Self {
        result.key_set
    }
}

// Request types for coordination protocols

/// DKD protocol request
#[derive(Debug, Clone)]
pub struct DkdRequest {
    pub device_id: DeviceId,
    pub account_id: AccountId,
    pub app_id: String,
    pub context: String,
    pub participants: Vec<DeviceId>,
    pub threshold: u16,
    pub requester: DeviceId,
}

/// Resharing protocol request
#[derive(Debug, Clone)]
pub struct ResharingRequest {
    pub device_id: DeviceId,
    pub account_id: AccountId,
    pub current_participants: Vec<DeviceId>,
    pub new_participants: Vec<DeviceId>,
    pub current_threshold: u16,
    pub new_threshold: u16,
}

/// Recovery protocol request
#[derive(Debug, Clone)]
pub struct RecoveryRequest {
    pub device_id: DeviceId,
    pub account_id: AccountId,
    pub guardian_list: Vec<GuardianId>,
    pub required_threshold: u16,
    pub cooldown_hours: u64,
}
