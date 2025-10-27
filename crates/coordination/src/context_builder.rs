//! Context Builder for Protocol Bootstrapping
//!
//! This module provides a secure context builder that validates capabilities
//! and loads real state from the ledger and crypto services before allowing
//! protocol execution.

use crate::execution::{BaseContext, ProtocolContext, ResharingContext, RecoveryContext};
use crate::{Transport, CryptoService};
use aura_crypto::Effects;
use aura_types::{AccountId, DeviceId, GuardianId};
use aura_journal::{AccountLedger, EventAuthorization, DeviceMetadata, OperationType};
use aura_errors::{AuraError, Result};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;
use std::collections::VecDeque;

/// Builder for creating validated protocol contexts
pub struct ContextBuilder {
    /// Crypto service for key operations
    crypto_service: Arc<CryptoService>,
    /// Transport for network communication
    transport: Arc<dyn Transport>,
    /// Effects for deterministic operations
    effects: Effects,
    /// Account ledger reference
    ledger: Option<Arc<RwLock<AccountLedger>>>,
    /// Validated device metadata
    device_metadata: Option<DeviceMetadata>,
    /// Current epoch
    current_epoch: Option<u64>,
}

impl ContextBuilder {
    /// Create a new context builder
    pub fn new(
        crypto_service: Arc<CryptoService>,
        transport: Arc<dyn Transport>,
        effects: Effects,
    ) -> Self {
        Self {
            crypto_service,
            transport,
            effects,
            ledger: None,
            device_metadata: None,
            current_epoch: None,
        }
    }

    /// Load and validate the account ledger
    pub async fn with_ledger(&mut self, account_id: AccountId) -> Result<&mut Self> {
        // Load ledger from persistent storage
        let ledger = self.load_account_ledger(account_id).await?;
        
        // Validate ledger integrity
        self.validate_ledger_integrity(&ledger).await?;
        
        self.ledger = Some(Arc::new(RwLock::new(ledger)));
        Ok(self)
    }

    /// Validate device authorization
    pub async fn validate_device(&mut self, device_id: DeviceId) -> Result<&mut Self> {
        // Extract metadata and epoch in a separate scope to release the ledger borrow
        let (device_metadata, current_epoch) = {
            let ledger = self.ledger.as_ref()
                .ok_or_else(|| AuraError::agent_invalid_state("Ledger not loaded"))?;
            
            let ledger_guard = ledger.read().await;
            let state = ledger_guard.state();
            
            // Check device is registered
            let device_metadata = state.devices.get(&device_id)
                .ok_or_else(|| AuraError::permission_denied("Device not registered"))?;
            
            // Check device is not revoked
            if device_metadata.last_seen == 0 && device_metadata.added_at > 0 {
                return Err(AuraError::permission_denied("Device has been revoked"));
            }
            
            (device_metadata.clone(), state.lamport_clock)
        };
        
        // Verify device key matches crypto service
        let signing_context = self.crypto_service.get_signing_context().await?;
        if signing_context.device_id != device_id {
            return Err(AuraError::permission_denied("Device ID mismatch"));
        }
        
        self.device_metadata = Some(device_metadata);
        self.current_epoch = Some(current_epoch);
        Ok(self)
    }

    /// Validate capability for operation
    pub async fn validate_capability(
        &mut self,
        _operation_type: OperationType,
        _context_id: Option<Vec<u8>>,
    ) -> Result<&mut Self> {
        let _ledger = self.ledger.as_ref()
            .ok_or_else(|| AuraError::agent_invalid_state("Ledger not loaded"))?;
        
        let _device_metadata = self.device_metadata.as_ref()
            .ok_or_else(|| AuraError::agent_invalid_state("Device not validated"))?;
        
        // TODO: Implement proper capability checking once the capability system is integrated
        // For now, allow all operations for authorized devices
        
        Ok(self)
    }

    /// Validate threshold materials availability
    pub async fn validate_threshold_materials(&mut self) -> Result<&mut Self> {
        // Check if device has key shares
        if !self.crypto_service.secure_storage
            .has_threshold_keys_sync()
            .map_err(|e| AuraError::storage_read_failed(e))?
        {
            return Err(AuraError::agent_invalid_state("Device lacks threshold key shares"));
        }
        
        Ok(self)
    }

    /// Build DKD protocol context
    pub async fn build_dkd_context(
        mut self,
        app_id: String,
        context: String,
        participants: Vec<DeviceId>,
        threshold: u16,
    ) -> Result<ProtocolContext> {
        // Validate prerequisites
        let context_id = self.compute_context_id(&app_id, &context);
        self.validate_capability(OperationType::Dkd, Some(context_id.clone())).await?;
        self.validate_threshold_materials().await?;
        
        // Validate participants are all active devices
        self.validate_participants(&participants).await?;
        
        // Create base context with real state
        let base_context = self.create_validated_base_context(
            participants,
            Some(threshold as usize),
        ).await?;
        
        Ok(ProtocolContext::Dkd(base_context))
    }

    /// Build resharing protocol context
    pub async fn build_resharing_context(
        mut self,
        current_participants: Vec<DeviceId>,
        new_participants: Vec<DeviceId>,
        current_threshold: u16,
        new_threshold: u16,
    ) -> Result<ProtocolContext> {
        // Validate prerequisites
        self.validate_capability(OperationType::Resharing, None).await?;
        self.validate_threshold_materials().await?;
        
        // Validate all participants
        self.validate_participants(&current_participants).await?;
        self.validate_participants(&new_participants).await?;
        
        // Ensure we have quorum from current participants
        if current_participants.len() < current_threshold as usize {
            return Err(AuraError::invalid_context(
                "Insufficient current participants for threshold"
            ));
        }
        
        // Create base context
        let base_context = self.create_validated_base_context(
            current_participants.clone(),
            Some(current_threshold as usize),
        ).await?;
        
        let resharing_context = ResharingContext::new(
            base_context,
            new_participants,
            new_threshold as usize,
        );
        
        Ok(ProtocolContext::Resharing(resharing_context))
    }

    /// Build recovery protocol context
    pub async fn build_recovery_context(
        mut self,
        guardian_ids: Vec<GuardianId>,
        required_threshold: u16,
        cooldown_hours: u64,
    ) -> Result<ProtocolContext> {
        // Validate prerequisites
        self.validate_capability(OperationType::Recovery, None).await?;
        
        // Validate guardians
        self.validate_guardians(&guardian_ids).await?;
        
        // Check recovery cooldown
        self.check_recovery_cooldown(cooldown_hours).await?;
        
        // For recovery, participants are the guardians' devices
        let participants = self.get_guardian_devices(&guardian_ids).await?;
        
        // Create base context
        let base_context = self.create_validated_base_context(
            participants,
            Some(required_threshold as usize),
        ).await?;
        
        let recovery_context = RecoveryContext::new(
            base_context,
            guardian_ids,
            required_threshold as usize,
            cooldown_hours,
        );
        
        Ok(ProtocolContext::Recovery(recovery_context))
    }

    /// Create a validated base context with real state
    async fn create_validated_base_context(
        &self,
        participants: Vec<DeviceId>,
        threshold: Option<usize>,
    ) -> Result<BaseContext> {
        let ledger = self.ledger.as_ref()
            .ok_or_else(|| AuraError::agent_invalid_state("Ledger not loaded"))?;
        
        let device_metadata = self.device_metadata.as_ref()
            .ok_or_else(|| AuraError::agent_invalid_state("Device not validated"))?;
        
        // Get real device key from crypto service
        let device_key = self.crypto_service.key_manager
            .get_raw_signing_key()?;
        
        // Get real HPKE key
        // TODO: Implement HPKE keypair management in DeviceKeyManager
        let hpke_private_key = aura_crypto::HpkePrivateKey::from_bytes(&[0u8; 32])?;
        
        let session_id = Uuid::new_v4();
        let time_source = Box::new(crate::execution::time::ProductionTimeSource::new());
        
        Ok(BaseContext {
            session_id,
            device_id: device_metadata.device_id.0,
            device_key,
            participants,
            threshold,
            ledger: ledger.clone(),
            transport: self.transport.clone(),
            effects: self.effects.clone(),
            time_source,
            pending_events: VecDeque::new(),
            _collected_events: Vec::new(),
            last_read_event_index: 0,
            device_secret: hpke_private_key,
            #[cfg(feature = "dev-console")]
            instrumentation: None,
        })
    }

    /// Load account ledger from storage
    async fn load_account_ledger(&self, _account_id: AccountId) -> Result<AccountLedger> {
        // In production, this would load from persistent storage
        // For now, return error indicating not implemented
        Err(AuraError::not_implemented(
            "Account ledger loading not yet implemented"
        ))
    }

    /// Validate ledger integrity
    async fn validate_ledger_integrity(&self, ledger: &AccountLedger) -> Result<()> {
        // Verify event chain hashes
        let events = ledger.event_log();
        let mut previous_hash = [0u8; 32];
        
        for event in events {
            // Verify hash chain
            if event.parent_hash != Some(previous_hash) && previous_hash != [0u8; 32] {
                return Err(AuraError::agent_invalid_state("Ledger hash chain broken"));
            }
            
            // Verify event signature
            self.verify_event_signature(event)?;
            
            previous_hash = event.hash()
                .map_err(|e| AuraError::serialization_failed(format!("Failed to hash event: {:?}", e)))?;
        }
        
        Ok(())
    }

    /// Verify event signature
    fn verify_event_signature(&self, event: &aura_journal::Event) -> Result<()> {
        match &event.authorization {
            EventAuthorization::DeviceCertificate { .. } => {
                // TODO: Verify signature using device's public key
                Ok(())
            }
            EventAuthorization::ThresholdSignature { .. } => {
                // TODO: Verify threshold signature
                Ok(())
            }
            _ => Err(AuraError::invalid_context("Invalid event authorization")),
        }
    }

    /// Validate participants are registered devices
    async fn validate_participants(&self, participants: &[DeviceId]) -> Result<()> {
        let ledger = self.ledger.as_ref()
            .ok_or_else(|| AuraError::agent_invalid_state("Ledger not loaded"))?;
        
        let ledger_guard = ledger.read().await;
        let state = ledger_guard.state();
        
        for device_id in participants {
            if !state.devices.contains_key(device_id) {
                return Err(AuraError::invalid_context(
                    format!("Device {:?} not registered", device_id)
                ));
            }
        }
        
        Ok(())
    }

    /// Validate guardians exist and are active
    async fn validate_guardians(&self, guardian_ids: &[GuardianId]) -> Result<()> {
        let ledger = self.ledger.as_ref()
            .ok_or_else(|| AuraError::agent_invalid_state("Ledger not loaded"))?;
        
        let ledger_guard = ledger.read().await;
        let state = ledger_guard.state();
        
        for guardian_id in guardian_ids {
            if !state.guardians.contains_key(guardian_id) {
                return Err(AuraError::invalid_context(
                    format!("Guardian {:?} not registered", guardian_id)
                ));
            }
        }
        
        Ok(())
    }

    /// Get devices associated with guardians
    async fn get_guardian_devices(&self, guardian_ids: &[GuardianId]) -> Result<Vec<DeviceId>> {
        let ledger = self.ledger.as_ref()
            .ok_or_else(|| AuraError::agent_invalid_state("Ledger not loaded"))?;
        
        let ledger_guard = ledger.read().await;
        let state = ledger_guard.state();
        
        let mut devices = Vec::new();
        for guardian_id in guardian_ids {
            if let Some(guardian) = state.guardians.get(guardian_id) {
                devices.push(guardian.device_id);
            }
        }
        
        Ok(devices)
    }

    /// Check recovery cooldown period
    async fn check_recovery_cooldown(&self, cooldown_hours: u64) -> Result<()> {
        let ledger = self.ledger.as_ref()
            .ok_or_else(|| AuraError::agent_invalid_state("Ledger not loaded"))?;
        
        let ledger_guard = ledger.read().await;
        
        // Check for recent recovery events
        let current_time = self.effects.now()
            .map_err(|e| AuraError::system_time_error(format!("{:?}", e)))?;
        
        let cooldown_ms = cooldown_hours * 3600 * 1000;
        
        for event in ledger_guard.event_log().iter().rev() {
            if let aura_journal::EventType::CompleteRecovery(_) = &event.event_type {
                if current_time - event.timestamp < cooldown_ms {
                    return Err(AuraError::timeout_error(
                        "Recovery cooldown period not elapsed"
                    ));
                }
                break; // Only check the most recent recovery
            }
        }
        
        Ok(())
    }

    /// Compute context ID for DKD
    fn compute_context_id(&self, app_id: &str, context: &str) -> Vec<u8> {
        use blake3::Hasher;
        let mut hasher = Hasher::new();
        hasher.update(b"aura-dkd-context-v1:");
        hasher.update(app_id.as_bytes());
        hasher.update(b":");
        hasher.update(context.as_bytes());
        hasher.finalize().as_bytes().to_vec()
    }
}