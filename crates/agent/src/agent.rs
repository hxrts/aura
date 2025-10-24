// DeviceAgent - high-level API for applications

use crate::{
    ContextCapsule, DerivedIdentity, IdentityConfig, SessionCredential, SessionStatistics, Result,
};
use aura_journal::{AccountLedger, AccountState, DeviceId, DeviceMetadata, SessionEpoch};
use aura_coordination::{KeyShare, ProductionTimeSource};
use aura_crypto::{DeviceKeyManager, Effects};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};
use bincode;

/// DeviceAgent provides the high-level API for identity and key derivation
/// 
/// This is the main interface that applications use to:
/// - Derive app-specific identities
/// - Issue presence tickets
/// - Manage account state via CRDT
/// - Coordinate threshold operations via CRDT ledger (peer-to-peer)
pub struct DeviceAgent {
    config: IdentityConfig,
    key_share: Arc<RwLock<KeyShare>>,
    ledger: Arc<RwLock<AccountLedger>>,
    transport: Arc<dyn aura_transport::Transport>,
    device_key_manager: Arc<RwLock<DeviceKeyManager>>,
    effects: Effects,
}

impl DeviceAgent {
    /// Create a new DeviceAgent
    pub async fn new(
        config: IdentityConfig,
        key_share: KeyShare,
        ledger: AccountLedger,
        transport: Arc<dyn aura_transport::Transport>,
        device_key_manager: DeviceKeyManager,
        effects: Effects,
    ) -> Result<Self> {
        info!("Initializing DeviceAgent for device {}", config.device_id);
        
        Ok(DeviceAgent {
            config,
            key_share: Arc::new(RwLock::new(key_share)),
            ledger: Arc::new(RwLock::new(ledger)),
            transport,
            device_key_manager: Arc::new(RwLock::new(device_key_manager)),
            effects,
        })
    }
    
    /// Connect to an existing account
    pub async fn connect(config: &IdentityConfig) -> Result<Self> {
        info!("Connecting DeviceAgent for device {}", config.device_id);
        
        // Load sealed key share
        // For MVP, we simulate loading - in production would decrypt from OS keystore
        let key_share = load_sealed_share(&config.share_path)?;
        
        // Load ledger state
        // For MVP, we create a mock ledger - in production would sync from peers
        let ledger = create_mock_ledger(config)?;
        
        // Create stub transport for MVP
        let transport = Arc::new(aura_transport::StubTransport::default());
        
        // Create device key manager and generate device key
        let mut device_key_manager = DeviceKeyManager::new(Effects::production());
        device_key_manager.generate_device_key(config.device_id.0)
            .map_err(|e| crate::AgentError::CryptoError(format!("Failed to generate device key: {:?}", e)))?;
        
        Self::new(config.clone(), key_share, ledger, transport, device_key_manager, Effects::production()).await
    }
    
    /// Derive a simple identity for an app context
    /// 
    /// This is the one-line helper that most applications will use.
    pub async fn derive_simple_identity(
        &self,
        app_id: &str,
        context_label: &str,
    ) -> Result<(DerivedIdentity, SessionCredential)> {
        debug!("Deriving simple identity for app={}, context={}", app_id, context_label);
        
        // TODO: Implement with new P2P DKD orchestrator
        // The old single-device DKD was removed in Phase 0
        // This will be re-implemented using aura_coordination::DkdOrchestrator
        let _key_share = self.key_share.read().await;
        let _session_epoch = self.ledger.read().await.state().session_epoch;
        Err(crate::AgentError::NotImplemented(
            "Simple identity derivation pending P2P DKD implementation".to_string()
        ))
    }
    
    /// Derive a context-specific identity with custom capsule using P2P DKD
    pub async fn derive_context_identity(
        &self,
        capsule: &ContextCapsule,
        with_binding_proof: bool,
    ) -> Result<DerivedIdentity> {
        debug!("Deriving context identity for app={}", capsule.app_id);
        
        // Get current account state to determine participants
        let participants = {
            let ledger = self.ledger.read().await;
            let state = ledger.state();
            state.devices.keys().cloned().collect::<Vec<_>>()
        };
        
        if participants.is_empty() {
            return Err(crate::AgentError::InvalidContext(
                "No participants found in account state".to_string()
            ));
        }
        
        // For threshold DKD, use majority threshold
        let threshold = (participants.len() / 2) + 1;
        
        self.derive_context_identity_threshold(capsule, participants, threshold, with_binding_proof).await
    }
    
    /// Derive context-specific identity with custom threshold and participants
    #[allow(unused_variables)]
    pub async fn derive_context_identity_threshold(
        &self,
        capsule: &ContextCapsule,
        participants: Vec<aura_journal::DeviceId>,
        threshold: usize,
        with_binding_proof: bool,
    ) -> Result<DerivedIdentity> {
        debug!(
            "Deriving threshold context identity for app={}, participants={}, threshold={}",
            capsule.app_id, participants.len(), threshold
        );
        
        // Create protocol context for DKD choreography
        let session_id = self.effects.gen_uuid();
        let ledger = self.ledger.clone();
        let _key_share = self.key_share.read().await.clone();
        
        let mut protocol_ctx = aura_coordination::ProtocolContext::new(
            session_id,
            self.config.device_id.0, // Extract Uuid from DeviceId
            participants,
            Some(threshold),
            ledger,
            self.transport.clone(),
            self.effects.clone(),
            // Get device signing key for protocol context
            {
                let device_key_manager = self.device_key_manager.read().await;
                device_key_manager.get_raw_signing_key()
                    .map_err(|e| crate::AgentError::CryptoError(format!("Failed to get device signing key: {:?}", e)))?
            },
            Box::new(ProductionTimeSource::new()),
        );
        
        // Set context for key derivation by creating context ID from capsule
        let context_id = {
            let mut context_bytes = Vec::new();
            context_bytes.extend_from_slice(capsule.app_id.as_bytes());
            context_bytes.push(0); // Separator
            context_bytes.extend_from_slice(capsule.context_label.as_bytes());
            context_bytes
        };
        
        // Execute DKD choreography with context
        let derived_key_bytes = aura_coordination::choreography::dkd_choreography(&mut protocol_ctx, context_id).await
            .map_err(|e| crate::AgentError::DkdFailed(format!("DKD choreography failed: {:?}", e)))?;
        
        // Convert derived key bytes to VerifyingKey
        let pk_derived = if derived_key_bytes.len() >= 32 {
            let mut key_array = [0u8; 32];
            key_array.copy_from_slice(&derived_key_bytes[..32]);
            ed25519_dalek::VerifyingKey::from_bytes(&key_array)
                .map_err(|e| crate::AgentError::DkdFailed(format!("Invalid derived key: {:?}", e)))?
        } else {
            return Err(crate::AgentError::DkdFailed("Derived key too short".to_string()));
        };
        
        // Create seed fingerprint from derived key bytes
        let seed_fingerprint = {
            let mut fingerprint = [0u8; 32];
            if derived_key_bytes.len() >= 32 {
                fingerprint.copy_from_slice(&derived_key_bytes[..32]);
            }
            fingerprint
        };
        
        // Construct derived identity
        let derived_identity = DerivedIdentity {
            capsule: capsule.clone(),
            pk_derived,
            seed_fingerprint,
        };
        
        debug!("Successfully derived identity for app={}, session={}", 
               capsule.app_id, session_id);
        
        Ok(derived_identity)
    }
    
    /// Issue authentication credential - proves device identity
    pub async fn issue_authentication_credential(&self, identity: &DerivedIdentity) -> Result<crate::types::AuthenticationCredential> {
        debug!("Issuing authentication credential for derived identity");
        
        // Generate challenge for credential binding
        let challenge: [u8; 32] = self.effects.random_bytes();
        
        // Get next nonce for replay prevention
        let device_nonce = self.get_next_device_nonce().await?;
        
        // Create device signature for authentication
        let signature_payload = [
            &challenge[..],
            &device_nonce.to_le_bytes()[..],
            identity.capsule.context_id()?.as_slice(),
        ].concat();
        
        // Sign the payload using device key
        let device_signature = {
            let device_key_manager = self.device_key_manager.read().await;
            device_key_manager.sign_message(&signature_payload)
                .map_err(|e| AgentError::CryptoError(format!("Failed to sign authentication credential: {:?}", e)))?
        };
        
        Ok(crate::types::AuthenticationCredential {
            issued_by: self.config.device_id,
            challenge,
            nonce: device_nonce,
            device_attestation: None, // No device attestation in MVP
            device_signature,
        })
    }
    
    /// Issue authorization token - grants specific permissions
    pub async fn issue_authorization_token(&self, device_id: aura_journal::DeviceId, operations: Vec<String>) -> Result<crate::types::AuthorizationToken> {
        debug!("Issuing authorization token for device {} with operations: {:?}", device_id, operations);
        
        // Get current session epoch from ledger
        let ledger = self.ledger.read().await;
        let _session_epoch = ledger.state().session_epoch;
        drop(ledger);
        
        // Calculate expiration (24 hours default)
        let expires_at = self.effects.now()? + (24 * 3600);
        
        // Create capability proof using threshold signature
        // The proof includes session epoch, device ID, operations, and expiration time
        let capability_proof = self.create_capability_proof(
            _session_epoch,
            device_id,
            &operations,
            expires_at,
        ).await?;
        
        Ok(crate::types::AuthorizationToken {
            permitted_operations: operations,
            expires_at,
            capability_proof,
            authorized_device: device_id,
        })
    }
    
    /// Verify authentication credential - check device identity
    pub async fn verify_authentication(&self, credential: &crate::types::AuthenticationCredential) -> Result<()> {
        debug!("Verifying authentication credential for device {}", credential.issued_by);
        
        // Verify device signature
        if !credential.verify_device_signature(&credential.issued_by)? {
            return Err(crate::AgentError::InvalidCredential("Invalid device signature".to_string()));
        }
        
        // Check nonce freshness
        let last_nonce = self.get_last_device_nonce(&credential.issued_by).await?;
        if !credential.is_fresh(last_nonce) {
            return Err(crate::AgentError::InvalidCredential("Stale nonce - potential replay attack".to_string()));
        }
        
        // TODO: Verify device attestation if present
        
        Ok(())
    }
    
    /// Check authorization token - verify permissions
    pub async fn check_authorization(&self, token: &crate::types::AuthorizationToken, operation: &str) -> Result<bool> {
        debug!("Checking authorization for device {} operation {}", token.authorized_device, operation);
        
        // Check if token is still valid
        let current_time = self.effects.now()?;
        if !token.is_valid(current_time) {
            return Ok(false);
        }
        
        // Check if token authorizes the requested operation
        Ok(token.authorizes_operation(operation))
    }
    
    /// Get next device nonce for replay prevention
    async fn get_next_device_nonce(&self) -> Result<u64> {
        let ledger = self.ledger.read().await;
        let _device_metadata = ledger.state().devices.get(&self.config.device_id)
            .ok_or_else(|| crate::AgentError::DeviceNotFound(format!("Device {} not found in ledger", self.config.device_id)))?;
        // TODO: Get actual nonce from device metadata
        Ok(1) // Placeholder
    }
    
    /// Get last seen nonce for a device (for replay protection)
    async fn get_last_device_nonce(&self, device_id: &aura_journal::DeviceId) -> Result<u64> {
        let ledger = self.ledger.read().await;
        let _device_metadata = ledger.state().devices.get(device_id)
            .ok_or_else(|| crate::AgentError::DeviceNotFound(format!("Device {} not found in ledger", device_id)))?;
        // TODO: Get actual last nonce from device metadata
        // For now, return 0 as placeholder (all nonces > 0 will be valid)
        Ok(0)
    }
    
    /// Sync account state from peers
    pub async fn sync_account_state(&self) -> Result<()> {
        info!("Syncing account state for device {}", self.config.device_id);
        
        // For MVP, this is a no-op
        // In production, would:
        // 1. Connect to peer devices
        // 2. Exchange Automerge changes
        // 3. Merge CRDT state
        // 4. Apply any new events
        
        Ok(())
    }
    
    /// Get current account state
    pub async fn account_state(&self) -> AccountState {
        self.ledger.read().await.state().clone()
    }
    
    /// Add a new device to the account via resharing
    pub async fn add_device(&self, new_device_id: DeviceId) -> Result<()> {
        info!("Adding device {} to account", new_device_id);
        
        // Get current participants
        let current_participants = {
            let ledger = self.ledger.read().await;
            let state = ledger.state();
            state.devices.keys().cloned().collect::<Vec<_>>()
        };
        
        let current_threshold = (current_participants.len() / 2) + 1;
        let mut new_participants = current_participants.clone();
        new_participants.push(new_device_id);
        
        self.reshare_with_config(new_participants, current_threshold).await
    }
    
    /// Remove a device from the account via resharing
    pub async fn remove_device(&self, device_to_remove: DeviceId) -> Result<()> {
        info!("Removing device {} from account", device_to_remove);
        
        // Get current participants
        let current_participants = {
            let ledger = self.ledger.read().await;
            let state = ledger.state();
            state.devices.keys().cloned().collect::<Vec<_>>()
        };
        
        let mut new_participants = current_participants.clone();
        new_participants.retain(|&id| id != device_to_remove);
        
        if new_participants.is_empty() {
            return Err(crate::AgentError::InvalidContext(
                "Cannot remove all devices from account".to_string()
            ));
        }
        
        let new_threshold = (new_participants.len() / 2) + 1;
        
        self.reshare_with_config(new_participants, new_threshold).await
    }
    
    /// Adjust the threshold requirement
    pub async fn adjust_threshold(&self, new_threshold: usize) -> Result<()> {
        info!("Adjusting threshold to {}", new_threshold);
        
        // Get current participants
        let current_participants = {
            let ledger = self.ledger.read().await;
            let state = ledger.state();
            state.devices.keys().cloned().collect::<Vec<_>>()
        };
        
        if new_threshold > current_participants.len() {
            return Err(crate::AgentError::InvalidContext(
                format!("Threshold {} cannot exceed participant count {}", 
                        new_threshold, current_participants.len())
            ));
        }
        
        if new_threshold == 0 {
            return Err(crate::AgentError::InvalidContext(
                "Threshold must be at least 1".to_string()
            ));
        }
        
        self.reshare_with_config(current_participants, new_threshold).await
    }
    
    /// Execute resharing with specific configuration
    async fn reshare_with_config(
        &self,
        new_participants: Vec<DeviceId>,
        new_threshold: usize,
    ) -> Result<()> {
        debug!(
            "Executing resharing: participants={}, threshold={}",
            new_participants.len(), new_threshold
        );
        
        // Create protocol context for resharing choreography
        let session_id = self.effects.gen_uuid();
        let ledger = self.ledger.clone();
        let _key_share = self.key_share.read().await.clone();
        
        let current_participants = {
            let ledger = self.ledger.read().await;
            let state = ledger.state();
            state.devices.keys().cloned().collect::<Vec<_>>()
        };
        
        let current_threshold = (current_participants.len() / 2) + 1;
        
        let mut protocol_ctx = aura_coordination::ProtocolContext::new(
            session_id,
            self.config.device_id.0, // Extract Uuid from DeviceId
            current_participants,
            Some(current_threshold),
            ledger,
            self.transport.clone(),
            self.effects.clone(),
            // Get device signing key for protocol context
            {
                let device_key_manager = self.device_key_manager.read().await;
                device_key_manager.get_raw_signing_key()
                    .map_err(|e| crate::AgentError::CryptoError(format!("Failed to get device signing key: {:?}", e)))?
            },
            Box::new(ProductionTimeSource::new()),
        );
        
        // Set resharing parameters
        protocol_ctx.new_participants = Some(new_participants.clone());
        protocol_ctx.new_threshold = Some(new_threshold);
        
        // Execute resharing choreography
        let _result = aura_coordination::choreography::resharing_choreography(&mut protocol_ctx, Some(new_threshold as u16), Some(new_participants)).await
            .map_err(|e| crate::AgentError::DkdFailed(format!("Resharing choreography failed: {:?}", e)))?;
        
        info!("Resharing completed successfully");
        Ok(())
    }
    
    /// Initiate account recovery (user-side)
    pub async fn initiate_recovery(
        &self,
        guardians: Vec<aura_journal::GuardianId>,
        required_threshold: usize,
        cooldown_hours: u64,
    ) -> Result<uuid::Uuid> {
        info!(
            "Initiating recovery with {} guardians, threshold {}, cooldown {}h",
            guardians.len(), required_threshold, cooldown_hours
        );
        
        if required_threshold > guardians.len() {
            return Err(crate::AgentError::InvalidContext(
                "Required threshold cannot exceed guardian count".to_string()
            ));
        }
        
        // Create protocol context for recovery choreography
        let session_id = self.effects.gen_uuid();
        let ledger = self.ledger.clone();
        let _key_share = self.key_share.read().await.clone();
        
        // For recovery, we use the current participant set initially
        let current_participants = {
            let ledger = self.ledger.read().await;
            let state = ledger.state();
            state.devices.keys().cloned().collect::<Vec<_>>()
        };
        
        let current_threshold = (current_participants.len() / 2) + 1;
        
        let mut protocol_ctx = aura_coordination::ProtocolContext::new(
            session_id,
            self.config.device_id.0, // Extract Uuid from DeviceId
            current_participants,
            Some(current_threshold),
            ledger,
            self.transport.clone(),
            self.effects.clone(),
            // Get device signing key for protocol context
            {
                let device_key_manager = self.device_key_manager.read().await;
                device_key_manager.get_raw_signing_key()
                    .map_err(|e| crate::AgentError::CryptoError(format!("Failed to get device signing key: {:?}", e)))?
            },
            Box::new(ProductionTimeSource::new()),
        );
        
        // Set recovery parameters
        protocol_ctx.guardians = Some(guardians.clone());
        protocol_ctx.guardian_threshold = Some(required_threshold);
        protocol_ctx.cooldown_hours = Some(cooldown_hours);
        protocol_ctx.is_recovery_initiator = true;
        
        // Execute recovery choreography with guardian list and threshold
        let _result = aura_coordination::choreography::recovery_choreography(&mut protocol_ctx, guardians.clone(), required_threshold as u16).await
            .map_err(|e| crate::AgentError::DkdFailed(format!("Recovery choreography failed: {:?}", e)))?;
        
        info!("Recovery initiated successfully, session_id: {}", session_id);
        Ok(session_id)
    }
    
    /// Approve recovery request (guardian-side)
    pub async fn approve_recovery(&self, request_id: uuid::Uuid) -> Result<()> {
        info!("Approving recovery request {}", request_id);
        
        // This would typically be called by a guardian device
        // The guardian would have the guardian_id and guardian key material
        
        // Create protocol context
        let ledger = self.ledger.clone();
        let _key_share = self.key_share.read().await.clone();
        
        let current_participants = {
            let ledger = self.ledger.read().await;
            let state = ledger.state();
            state.devices.keys().cloned().collect::<Vec<_>>()
        };
        
        let current_threshold = (current_participants.len() / 2) + 1;
        
        let mut protocol_ctx = aura_coordination::ProtocolContext::new(
            request_id,
            self.config.device_id.0, // Extract Uuid from DeviceId
            current_participants,
            Some(current_threshold),
            ledger,
            self.transport.clone(),
            self.effects.clone(),
            // Get device signing key for protocol context
            {
                let device_key_manager = self.device_key_manager.read().await;
                device_key_manager.get_raw_signing_key()
                    .map_err(|e| crate::AgentError::CryptoError(format!("Failed to get device signing key: {:?}", e)))?
            },
            Box::new(ProductionTimeSource::new()),
        );
        
        // Set guardian context
        protocol_ctx.guardian_id = Some(aura_journal::GuardianId(self.config.device_id.0)); // Simplified: device_id as guardian_id
        
        // Get actual guardian list and threshold from account state
        let (guardian_list, guardian_threshold) = {
            let ledger = self.ledger.read().await;
            let state = ledger.state();
            let guardians: Vec<aura_journal::GuardianId> = state.guardians.keys().cloned().collect();
            let threshold = if guardians.is_empty() {
                // Fallback: use device threshold if no guardians configured
                state.threshold as u16
            } else {
                // Use majority threshold for guardians
                ((guardians.len() / 2) + 1) as u16
            };
            
            // If no guardians configured, use current device as emergency guardian
            if guardians.is_empty() {
                (vec![aura_journal::GuardianId(self.config.device_id.0)], 1u16)
            } else {
                (guardians, threshold)
            }
        };
        
        // Execute recovery choreography with actual guardian configuration
        let _result = aura_coordination::choreography::recovery_choreography(&mut protocol_ctx, guardian_list, guardian_threshold).await
            .map_err(|e| crate::AgentError::DkdFailed(format!("Recovery approval failed: {:?}", e)))?;
        
        info!("Recovery approval completed");
        Ok(())
    }
    
    /// Nudge an unresponsive guardian
    pub async fn nudge_guardian(
        &self,
        request_id: uuid::Uuid,
        guardian_id: aura_journal::GuardianId,
    ) -> Result<()> {
        info!("Nudging guardian {:?} for recovery {}", guardian_id, request_id);
        
        let ledger = self.ledger.clone();
        let _key_share = self.key_share.read().await.clone();
        
        let current_participants = {
            let ledger = self.ledger.read().await;
            let state = ledger.state();
            state.devices.keys().cloned().collect::<Vec<_>>()
        };
        
        let current_threshold = (current_participants.len() / 2) + 1;
        
        let mut protocol_ctx = aura_coordination::ProtocolContext::new(
            request_id,
            self.config.device_id.0, // Extract Uuid from DeviceId
            current_participants,
            Some(current_threshold),
            ledger,
            self.transport.clone(),
            self.effects.clone(),
            // Get device signing key for protocol context
            {
                let device_key_manager = self.device_key_manager.read().await;
                device_key_manager.get_raw_signing_key()
                    .map_err(|e| crate::AgentError::CryptoError(format!("Failed to get device signing key: {:?}", e)))?
            },
            Box::new(ProductionTimeSource::new()),
        );
        
        // Execute nudge
        let recovery_session_id = uuid::Uuid::new_v4(); // Generate recovery session ID
        aura_coordination::choreography::nudge_guardian(&mut protocol_ctx, guardian_id, recovery_session_id).await
            .map_err(|e| crate::AgentError::DkdFailed(format!("Guardian nudge failed: {:?}", e)))?;
        
        info!("Guardian nudge sent");
        Ok(())
    }
    
    /// Generate binding proof for derived identity
    /// 
    /// Creates a cryptographic proof that binds the derived identity to this specific device.
    /// The proof demonstrates that:
    /// 1. The device possesses the private key corresponding to its device ID
    /// 2. The derived key was generated by this device for the specified context
    /// 3. The binding cannot be forged without access to the device's private key
    #[allow(dead_code)]
    async fn generate_binding_proof(
        &self,
        capsule: &ContextCapsule,
        derived_key: &[u8],
    ) -> Result<Vec<u8>> {
        debug!("Generating binding proof for app={}, derived_key={}", 
               capsule.app_id, hex::encode(&derived_key[..8]));
        
        // Create binding proof message that includes:
        // - Device ID (to identify the signing device)
        // - App ID and context (to scope the binding)
        // - Derived key (what we're binding to the device)
        // - Timestamp (to prevent replay attacks)
        let timestamp = self.effects.now()
            .map_err(|e| crate::AgentError::CryptoError(format!("Failed to get timestamp: {:?}", e)))?;
        
        let proof_content = bincode::serialize(&(
            &self.config.device_id,
            &capsule.app_id,
            &capsule.context_label,
            derived_key,
            timestamp,
        )).map_err(|e| crate::AgentError::CryptoError(format!("Failed to serialize proof content: {:?}", e)))?;
        
        // Sign the proof content with device key to create binding proof
        let device_key_manager = self.device_key_manager.read().await;
        let signature = device_key_manager.sign_message(&proof_content)
            .map_err(|e| crate::AgentError::CryptoError(format!("Failed to sign binding proof: {:?}", e)))?;
        
        // Create the complete binding proof structure
        let binding_proof = bincode::serialize(&(
            &self.config.device_id,
            timestamp,
            signature,
        )).map_err(|e| crate::AgentError::CryptoError(format!("Failed to serialize binding proof: {:?}", e)))?;
        
        debug!("Generated binding proof of {} bytes", binding_proof.len());
        Ok(binding_proof)
    }
    
    /// Get current session epoch  
    pub async fn get_current_epoch(&self) -> u64 {
        self.ledger.read().await.lamport_clock()
    }
    
    /// Check for session timeouts
    pub async fn check_session_timeouts(&self) -> Result<Vec<uuid::Uuid>> {
        // TODO: Implement timeout checking for active sessions
        // Would scan active sessions and return timed-out session IDs
        Ok(vec![])
    }
    
    /// Emit a tick event to advance logical time
    pub async fn maybe_emit_tick(&self) -> Result<()> {
        // TODO: Implement tick emission for logical clock advancement
        // Would check if tick is needed and emit EpochTick event
        Ok(())
    }
    
    /// Get device ID
    pub fn device_id(&self) -> DeviceId {
        self.config.device_id
    }
    
    /// Get session epoch
    pub async fn session_epoch(&self) -> SessionEpoch {
        self.ledger.read().await.state().session_epoch
    }
    
    // ========== Session Management ==========
    
    /// Create a new session
    pub async fn create_session(
        &self,
        protocol_type: aura_journal::ProtocolType,
        participants: Vec<aura_journal::ParticipantId>,
        ttl_in_epochs: u64,
    ) -> Result<uuid::Uuid> {
        let session_id = self.effects.gen_uuid();
        let current_epoch = self.get_current_epoch().await;
        let timestamp = self.effects.now().map_err(|e| crate::AgentError::SystemTimeError(e.to_string()))?;
        
        let session = aura_journal::Session::new(
            aura_journal::SessionId(session_id),
            protocol_type,
            participants,
            current_epoch,
            ttl_in_epochs,
            timestamp,
        );
        
        // Add session to ledger
        let mut ledger = self.ledger.write().await;
        ledger.add_session(session, &self.effects);
        
        info!("Created session {} for protocol {:?}", session_id, protocol_type);
        Ok(session_id)
    }
    
    /// Get session by ID
    pub async fn get_session(&self, session_id: &uuid::Uuid) -> Option<aura_journal::Session> {
        self.ledger.read().await.get_session(session_id).cloned()
    }
    
    /// Get all active sessions
    pub async fn active_sessions(&self) -> Vec<aura_journal::Session> {
        self.ledger.read().await.active_sessions().into_iter().cloned().collect()
    }
    
    /// Get sessions by protocol type
    pub async fn sessions_by_protocol(&self, protocol_type: aura_journal::ProtocolType) -> Vec<aura_journal::Session> {
        self.ledger.read().await.sessions_by_protocol(protocol_type).into_iter().cloned().collect()
    }
    
    /// Check if any session of given protocol type is active
    pub async fn has_active_session_of_type(&self, protocol_type: aura_journal::ProtocolType) -> bool {
        self.ledger.read().await.has_active_session_of_type(protocol_type)
    }
    
    /// Update session status
    pub async fn update_session_status(&self, session_id: uuid::Uuid, status: aura_journal::SessionStatus) -> Result<()> {
        let mut ledger = self.ledger.write().await;
        ledger.update_session_status(session_id, status, &self.effects)
            .map_err(|e| crate::AgentError::LedgerError(e.to_string()))?;
        
        debug!("Updated session {} status to {:?}", session_id, status);
        Ok(())
    }
    
    /// Complete a session with success outcome
    pub async fn complete_session(&self, session_id: uuid::Uuid) -> Result<()> {
        let mut ledger = self.ledger.write().await;
        ledger.complete_session(session_id, aura_journal::SessionOutcome::Success, &self.effects)
            .map_err(|e| crate::AgentError::LedgerError(e.to_string()))?;
        
        info!("Completed session {} successfully", session_id);
        Ok(())
    }
    
    /// Abort a session with failure
    pub async fn abort_session(&self, session_id: uuid::Uuid, reason: String, blamed_party: Option<aura_journal::ParticipantId>) -> Result<()> {
        let mut ledger = self.ledger.write().await;
        ledger.abort_session(session_id, reason.clone(), blamed_party, &self.effects)
            .map_err(|e| crate::AgentError::LedgerError(e.to_string()))?;
        
        info!("Aborted session {} with reason: {}", session_id, reason);
        Ok(())
    }
    
    /// Clean up expired sessions
    pub async fn cleanup_expired_sessions(&self) -> Result<()> {
        let mut ledger = self.ledger.write().await;
        ledger.cleanup_expired_sessions(&self.effects);
        
        debug!("Cleaned up expired sessions");
        Ok(())
    }
    
    /// Start a new protocol session with automatic participant selection
    pub async fn start_protocol_session(&self, protocol_type: aura_journal::ProtocolType) -> Result<uuid::Uuid> {
        // Check if this protocol type is already active
        if self.has_active_session_of_type(protocol_type).await {
            return Err(crate::AgentError::InvalidContext(
                format!("Protocol {:?} already has an active session", protocol_type)
            ));
        }
        
        // Get participants based on protocol type
        let participants: Vec<aura_journal::ParticipantId> = match protocol_type {
            aura_journal::ProtocolType::Dkd | 
            aura_journal::ProtocolType::Resharing | 
            aura_journal::ProtocolType::LockAcquisition => {
                // These protocols involve all active devices
                let ledger = self.ledger.read().await;
                let state = ledger.state();
                state.devices.keys()
                    .map(|device_id| aura_journal::ParticipantId::Device(*device_id))
                    .collect()
            },
            aura_journal::ProtocolType::Recovery => {
                // Recovery involves guardians
                let ledger = self.ledger.read().await;
                let state = ledger.state();
                state.guardians.keys()
                    .map(|guardian_id| aura_journal::ParticipantId::Guardian(*guardian_id))
                    .collect()
            },
            aura_journal::ProtocolType::Locking => {
                // Compaction involves all devices
                let ledger = self.ledger.read().await;
                let state = ledger.state();
                state.devices.keys()
                    .map(|device_id| aura_journal::ParticipantId::Device(*device_id))
                    .collect()
            },
        };
        
        if participants.is_empty() {
            return Err(crate::AgentError::InvalidContext(
                format!("No participants available for protocol {:?}", protocol_type)
            ));
        }
        
        // Set appropriate TTL based on protocol type
        let ttl_in_epochs = match protocol_type {
            aura_journal::ProtocolType::Dkd => 50,               // DKD should be fast
            aura_journal::ProtocolType::Resharing => 100,        // Resharing can take time
            aura_journal::ProtocolType::Recovery => 1000, // Recovery has cooldowns
            aura_journal::ProtocolType::Locking => 10,          // Locking should be fast
            aura_journal::ProtocolType::LockAcquisition => 100,   // Lock acquisition can take time
        };
        
        self.create_session(protocol_type, participants, ttl_in_epochs).await
    }
    
    /// Monitor all active sessions and handle timeouts
    pub async fn monitor_sessions(&self) -> Result<Vec<aura_journal::SessionId>> {
        let current_epoch = self.get_current_epoch().await;
        let mut timed_out_sessions = Vec::new();
        
        // Get all active sessions
        let active_sessions = self.active_sessions().await;
        
        for session in active_sessions {
            if session.is_timed_out(current_epoch) {
                // Mark session as timed out
                self.update_session_status(session.session_id.0, aura_journal::SessionStatus::TimedOut).await?;
                timed_out_sessions.push(session.session_id);
                
                info!("Session {} timed out after {} epochs", 
                      session.session_id, current_epoch - session.started_at);
            }
        }
        
        Ok(timed_out_sessions)
    }
    
    /// Get session statistics
    pub async fn session_statistics(&self) -> SessionStatistics {
        let ledger = self.ledger.read().await;
        let state = ledger.state();
        
        let mut stats = SessionStatistics {
            total_sessions: state.sessions.len(),
            active_sessions: 0,
            completed_sessions: 0,
            failed_sessions: 0,
            timed_out_sessions: 0,
            sessions_by_protocol: std::collections::BTreeMap::new(),
        };
        
        for session in state.sessions.values() {
            match session.status {
                aura_journal::SessionStatus::Active => {
                    stats.active_sessions += 1;
                },
                aura_journal::SessionStatus::Completed => {
                    stats.completed_sessions += 1;
                },
                aura_journal::SessionStatus::Failed => {
                    stats.failed_sessions += 1;
                },
                aura_journal::SessionStatus::TimedOut => {
                    stats.timed_out_sessions += 1;
                },
                aura_journal::SessionStatus::Expired => {
                    stats.timed_out_sessions += 1; // Count expired as timed out
                },
            }
            
            *stats.sessions_by_protocol.entry(session.protocol_type).or_insert(0) += 1;
        }
        
        stats
    }
    
    /// Request a distributed lock for a critical operation
    /// 
    /// This creates a Session and executes the locking choreography
    pub async fn request_operation_lock(&self, operation_type: aura_journal::OperationType) -> Result<uuid::Uuid> {
        info!("Requesting operation lock for {:?}", operation_type);
        
        // Create session for lock acquisition
        let session_id = self.start_protocol_session(aura_journal::ProtocolType::LockAcquisition).await?;
        
        // Create protocol context for locking choreography
        let ledger = self.ledger.clone();
        let participants = {
            let ledger = self.ledger.read().await;
            let state = ledger.state();
            state.devices.keys().cloned().collect::<Vec<_>>()
        };
        
        let threshold = (participants.len() / 2) + 1;
        let mut protocol_ctx = aura_coordination::ProtocolContext::new(
            session_id,
            self.config.device_id.0,
            participants,
            Some(threshold), // Majority threshold
            ledger,
            self.transport.clone(),
            self.effects.clone(),
            // Get device signing key for protocol context
            {
                let device_key_manager = self.device_key_manager.read().await;
                device_key_manager.get_raw_signing_key()
                    .map_err(|e| crate::AgentError::CryptoError(format!("Failed to get device signing key: {:?}", e)))?
            },
            Box::new(ProductionTimeSource::new()),
        );
        
        // Execute locking choreography
        match aura_coordination::choreography::locking::locking_choreography(&mut protocol_ctx, operation_type).await {
            Ok(()) => {
                // We won the lock!
                self.complete_session(session_id).await?;
                info!("Successfully acquired operation lock for {:?}", operation_type);
                Ok(session_id)
            },
            Err(e) => {
                // We lost the lottery or failed
                self.abort_session(session_id, e.message.clone(), None).await?;
                Err(crate::AgentError::DkdFailed(format!("Failed to acquire lock: {:?}", e)))
            }
        }
    }
    
    /// Release a previously acquired distributed lock
    pub async fn release_operation_lock(&self, session_id: uuid::Uuid, operation_type: aura_journal::OperationType) -> Result<()> {
        info!("Releasing operation lock for {:?}", operation_type);
        
        // Create protocol context for lock release
        let ledger = self.ledger.clone();
        let participants = {
            let ledger = self.ledger.read().await;
            let state = ledger.state();
            state.devices.keys().cloned().collect::<Vec<_>>()
        };
        
        let threshold = (participants.len() / 2) + 1;
        let mut protocol_ctx = aura_coordination::ProtocolContext::new(
            session_id,
            self.config.device_id.0,
            participants,
            Some(threshold),
            ledger,
            self.transport.clone(),
            self.effects.clone(),
            // Get device signing key for protocol context
            {
                let device_key_manager = self.device_key_manager.read().await;
                device_key_manager.get_raw_signing_key()
                    .map_err(|e| crate::AgentError::CryptoError(format!("Failed to get device signing key: {:?}", e)))?
            },
            Box::new(ProductionTimeSource::new()),
        );
        
        // Execute lock release choreography
        aura_coordination::choreography::locking::release_lock_choreography(&mut protocol_ctx, operation_type).await
            .map_err(|e| crate::AgentError::DkdFailed(format!("Failed to release lock: {:?}", e)))?;
        
        info!("Successfully released operation lock for {:?}", operation_type);
        Ok(())
    }
    
    /// Check if any operation lock is currently active
    pub async fn is_operation_locked(&self, operation_type: aura_journal::OperationType) -> bool {
        let ledger = self.ledger.read().await;
        ledger.is_operation_locked(operation_type)
    }
    
    /// Get the currently active operation lock
    pub async fn active_operation_lock(&self) -> Option<aura_journal::OperationLock> {
        let ledger = self.ledger.read().await;
        ledger.active_operation_lock().cloned()
    }
    
    /// Create capability proof using threshold signature
    /// 
    /// This creates a cryptographic proof that authorizes specific operations for a device.
    /// The proof includes session epoch to tie authorization to current session state.
    async fn create_capability_proof(
        &self,
        session_epoch: u64,
        device_id: aura_journal::DeviceId,
        operations: &[String],
        expires_at: u64,
    ) -> Result<Vec<u8>> {
        use aura_crypto::FrostSigner;
        
        // Create authorization data to sign
        let auth_data = format!(
            "AUTH:{}:{}:{}:{}",
            session_epoch,
            device_id.0,
            operations.join(","),
            expires_at
        );
        let message = auth_data.as_bytes();
        
        // Get our key share for FROST signing
        let key_share = self.key_share.read().await;
        let key_package = &key_share.share;
        
        // Generate nonces for FROST round 1
        let mut rng = self.effects.rng();
        let (_nonces, commitments) = FrostSigner::generate_nonces(key_package.signing_share(), &mut rng);
        
        // For MVP: Create a single-participant signature using device key
        // TODO: In production, coordinate with other participants for proper threshold signature
        let device_key_manager = self.device_key_manager.read().await;
        let device_signature = device_key_manager.sign_message(message)?;
        
        // For now, use device signature as capability proof
        // In production, this would be replaced with proper FROST threshold signature
        Ok(device_signature)
    }
}

/// Load sealed key share from storage
/// 
/// For MVP, this creates a mock share.
/// In production, would decrypt from OS keystore or hardware seal.
fn load_sealed_share(_path: &str) -> Result<KeyShare> {
    // TODO: Implement proper sealed storage
    // For now, return error - shares must be provided during agent creation
    Err(crate::AgentError::DeviceNotFound(
        "Sealed share loading not yet implemented for MVP".to_string(),
    ))
}

/// Create mock ledger for testing
/// 
/// In production, would load from persistent storage and sync with peers.
fn create_mock_ledger(config: &IdentityConfig) -> Result<AccountLedger> {
    use ed25519_dalek::SigningKey;
    
    let signing_key = SigningKey::from_bytes(&rand::random::<[u8; 32]>());
    let group_public_key = signing_key.verifying_key();
    
    let device = DeviceMetadata {
        device_id: config.device_id,
        device_name: "Mock Device".to_string(),
        device_type: aura_journal::DeviceType::Native,
        public_key: group_public_key,
        added_at: 0, // Will be set properly when effects are available
        last_seen: 0, // Will be set properly when effects are available
        dkd_commitment_proofs: std::collections::BTreeMap::new(),
    };
    
    let state = AccountState::new(
        config.account_id,
        group_public_key,
        device,
        config.threshold,
        config.total_participants,
    );
    
    AccountLedger::new(state)
        .map_err(|e| crate::AgentError::LedgerError(e.to_string()))
}

#[allow(dead_code)]
fn current_timestamp_with_effects(effects: &aura_crypto::Effects) -> Result<u64> {
    effects.now().map_err(|e| crate::AgentError::SystemTimeError(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_coordination::{KeyShare, ParticipantId};
    use frost_ed25519 as frost;
    
    // Helper to create test key shares using FROST dealer
    fn setup_test_keyshare() -> (KeyShare, ed25519_dalek::VerifyingKey) {
        let mut rng = aura_crypto::Effects::test().rng();
        
        // Generate 2-of-3 threshold keys
        let result = frost::keys::generate_with_dealer(
            2u16, // threshold
            3u16, // total participants
            frost::keys::IdentifierList::Default,
            &mut rng,
        );
        
        let (shares, pubkey_package) = result.expect("FROST key generation should work");
        
        // Get first participant's share
        let (_id, secret_share) = shares.into_iter().next().unwrap();
        let key_package = frost::keys::KeyPackage::try_from(secret_share).unwrap();
        
        let key_share = KeyShare {
            participant_id: ParticipantId::from_u16_unchecked(1),
            share: key_package,
            threshold: 2,
            total_participants: 3,
        };
        
        // Convert FROST verifying key to dalek
        let frost_vk = pubkey_package.verifying_key();
        let dalek_vk = ed25519_dalek::VerifyingKey::from_bytes(&frost_vk.serialize()).unwrap();
        
        (key_share, dalek_vk)
    }

    #[tokio::test]
    #[ignore] // Temporarily disabled due to FROST key generation issues
    async fn test_device_agent_derive_identity() {
        // Create test key share
        let (share1, pubkey) = setup_test_keyshare();
        
        // Create agent config
        let effects = aura_crypto::Effects::test();
        let config = IdentityConfig {
            device_id: DeviceId::new_with_effects(&effects),
            account_id: aura_journal::AccountId::new_with_effects(&effects),
            participant_id: ParticipantId::from_u16_unchecked(1),
            share_path: "/tmp/test_share".to_string(),
            threshold: 2,
            total_participants: 3,
        };
        
        // Create device metadata
        let device = DeviceMetadata {
            device_id: config.device_id,
            device_name: "Test Device".to_string(),
            device_type: aura_journal::DeviceType::Native,
            public_key: pubkey,
            added_at: aura_crypto::Effects::test().now().unwrap_or(0),
            last_seen: aura_crypto::Effects::test().now().unwrap_or(0),
            dkd_commitment_proofs: std::collections::BTreeMap::new(),
        };
        
        // Create initial state and ledger
        let state = AccountState::new(
            config.account_id,
            pubkey,
            device,
            config.threshold,
            config.total_participants,
        );
        let ledger = AccountLedger::new(state).unwrap();
        
        let device_id = config.device_id;
        
        // Create transport and device key manager for agent
        let transport = Arc::new(aura_transport::StubTransport::default());
        let mut device_key_manager = DeviceKeyManager::new(aura_crypto::Effects::test());
        device_key_manager.generate_device_key(device_id.0).unwrap();
        
        // Create agent
        let agent = DeviceAgent::new(config, share1, ledger, transport, device_key_manager, aura_crypto::Effects::test()).await.unwrap();
        
        // Test that agent was created successfully
        assert_eq!(agent.device_id(), device_id);
        assert_eq!(agent.session_epoch().await, SessionEpoch(1));
        
        // Note: derive_simple_identity is not yet implemented (requires P2P DKD)
        // This test just verifies basic agent initialization
    }
}

