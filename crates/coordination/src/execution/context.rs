//! Unified protocol context type
//!
//! This module provides a unified ProtocolContext enum that can hold any
//! protocol-specific context, allowing choreography functions to work with
//! the appropriate context type.

use super::base_context::{BaseContext, Transport};
use super::protocol_contexts::*;
use super::time::TimeSource;
use super::types::*;
use aura_crypto::Effects;
use aura_journal::{AccountLedger, DeviceId, GuardianId, Event};
use ed25519_dalek::SigningKey;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Unified protocol context that can hold any protocol-specific context
pub enum ProtocolContext {
    Dkd(DkdContext),
    Resharing(ResharingContext),
    Recovery(RecoveryContext),
    Locking(LockingContext),
    Compaction(CompactionContext),
}

impl ProtocolContext {
    /// Create a new DKD context
    pub fn new_dkd(
        session_id: Uuid,
        device_id: Uuid,
        participants: Vec<DeviceId>,
        threshold: Option<usize>,
        ledger: Arc<RwLock<AccountLedger>>,
        transport: Arc<dyn Transport>,
        effects: Effects,
        device_key: SigningKey,
        time_source: Box<dyn TimeSource>,
    ) -> Self {
        let base = BaseContext::new(
            session_id,
            device_id,
            participants,
            threshold,
            ledger,
            transport,
            effects,
            device_key,
            time_source,
        );
        ProtocolContext::Dkd(DkdContext::new(base))
    }

    /// Create a new Resharing context
    pub fn new_resharing(
        session_id: Uuid,
        device_id: Uuid,
        participants: Vec<DeviceId>,
        threshold: Option<usize>,
        ledger: Arc<RwLock<AccountLedger>>,
        transport: Arc<dyn Transport>,
        effects: Effects,
        device_key: SigningKey,
        time_source: Box<dyn TimeSource>,
        new_participants: Vec<DeviceId>,
        new_threshold: usize,
    ) -> Self {
        let base = BaseContext::new(
            session_id,
            device_id,
            participants,
            threshold,
            ledger,
            transport,
            effects,
            device_key,
            time_source,
        );
        ProtocolContext::Resharing(ResharingContext::new(base, new_participants, new_threshold))
    }

    /// Create a new Recovery context
    pub fn new_recovery(
        session_id: Uuid,
        device_id: Uuid,
        participants: Vec<DeviceId>,
        threshold: Option<usize>,
        ledger: Arc<RwLock<AccountLedger>>,
        transport: Arc<dyn Transport>,
        effects: Effects,
        device_key: SigningKey,
        time_source: Box<dyn TimeSource>,
        guardians: Vec<GuardianId>,
        guardian_threshold: usize,
        cooldown_hours: u64,
    ) -> Self {
        let base = BaseContext::new(
            session_id,
            device_id,
            participants,
            threshold,
            ledger,
            transport,
            effects,
            device_key,
            time_source,
        );
        ProtocolContext::Recovery(RecoveryContext::new(base, guardians, guardian_threshold, cooldown_hours))
    }

    /// Create a new Locking context
    pub fn new_locking(
        session_id: Uuid,
        device_id: Uuid,
        participants: Vec<DeviceId>,
        threshold: Option<usize>,
        ledger: Arc<RwLock<AccountLedger>>,
        transport: Arc<dyn Transport>,
        effects: Effects,
        device_key: SigningKey,
        time_source: Box<dyn TimeSource>,
    ) -> Self {
        let base = BaseContext::new(
            session_id,
            device_id,
            participants,
            threshold,
            ledger,
            transport,
            effects,
            device_key,
            time_source,
        );
        ProtocolContext::Locking(LockingContext::new(base))
    }

    /// Create a generic context (defaults to DKD for backward compatibility)
    pub fn new(
        session_id: Uuid,
        device_id: Uuid,
        participants: Vec<DeviceId>,
        threshold: Option<usize>,
        ledger: Arc<RwLock<AccountLedger>>,
        transport: Arc<dyn Transport>,
        effects: Effects,
        device_key: SigningKey,
        time_source: Box<dyn TimeSource>,
    ) -> Self {
        Self::new_dkd(
            session_id,
            device_id,
            participants,
            threshold,
            ledger,
            transport,
            effects,
            device_key,
            time_source,
        )
    }

    /// Get session ID
    pub fn session_id(&self) -> Uuid {
        match self {
            ProtocolContext::Dkd(ctx) => ctx.base().session_id,
            ProtocolContext::Resharing(ctx) => ctx.base().session_id,
            ProtocolContext::Recovery(ctx) => ctx.base().session_id,
            ProtocolContext::Locking(ctx) => ctx.base().session_id,
            ProtocolContext::Compaction(ctx) => ctx.base().session_id,
        }
    }

    /// Get device ID
    pub fn device_id(&self) -> Uuid {
        match self {
            ProtocolContext::Dkd(ctx) => ctx.base().device_id,
            ProtocolContext::Resharing(ctx) => ctx.base().device_id,
            ProtocolContext::Recovery(ctx) => ctx.base().device_id,
            ProtocolContext::Locking(ctx) => ctx.base().device_id,
            ProtocolContext::Compaction(ctx) => ctx.base().device_id,
        }
    }

    /// Get participants
    pub fn participants(&self) -> &Vec<DeviceId> {
        match self {
            ProtocolContext::Dkd(ctx) => &ctx.base().participants,
            ProtocolContext::Resharing(ctx) => &ctx.base().participants,
            ProtocolContext::Recovery(ctx) => &ctx.base().participants,
            ProtocolContext::Locking(ctx) => &ctx.base().participants,
            ProtocolContext::Compaction(ctx) => &ctx.base().participants,
        }
    }

    /// Get threshold
    pub fn threshold(&self) -> Option<usize> {
        match self {
            ProtocolContext::Dkd(ctx) => ctx.base().threshold,
            ProtocolContext::Resharing(ctx) => ctx.base().threshold,
            ProtocolContext::Recovery(ctx) => ctx.base().threshold,
            ProtocolContext::Locking(ctx) => ctx.base().threshold,
            ProtocolContext::Compaction(ctx) => ctx.base().threshold,
        }
    }

    /// Get effects
    pub fn effects(&self) -> &Effects {
        match self {
            ProtocolContext::Dkd(ctx) => &ctx.base().effects,
            ProtocolContext::Resharing(ctx) => &ctx.base().effects,
            ProtocolContext::Recovery(ctx) => &ctx.base().effects,
            ProtocolContext::Locking(ctx) => &ctx.base().effects,
            ProtocolContext::Compaction(ctx) => &ctx.base().effects,
        }
    }

    /// Create a new RNG from effects
    pub fn create_rng(&self) -> aura_crypto::EffectsRng {
        self.effects().rng()
    }

    /// Execute an instruction
    pub async fn execute(
        &mut self,
        instruction: Instruction,
    ) -> Result<InstructionResult, ProtocolError> {
        match self {
            ProtocolContext::Dkd(ctx) => ctx.execute(instruction).await,
            ProtocolContext::Resharing(ctx) => ctx.execute(instruction).await,
            ProtocolContext::Recovery(ctx) => ctx.execute(instruction).await,
            ProtocolContext::Locking(ctx) => ctx.execute(instruction).await,
            ProtocolContext::Compaction(ctx) => ctx.execute(instruction).await,
        }
    }

    /// Sign an event
    pub fn sign_event(&self, event: &Event) -> Result<ed25519_dalek::Signature, ProtocolError> {
        match self {
            ProtocolContext::Dkd(ctx) => ctx.sign_event(event),
            ProtocolContext::Resharing(ctx) => ctx.sign_event(event),
            ProtocolContext::Recovery(ctx) => ctx.sign_event(event),
            ProtocolContext::Locking(ctx) => ctx.sign_event(event),
            ProtocolContext::Compaction(ctx) => ctx.sign_event(event),
        }
    }

    /// Generate nonce
    pub async fn generate_nonce(&self) -> Result<u64, ProtocolError> {
        match self {
            ProtocolContext::Dkd(ctx) => ctx.generate_nonce().await,
            ProtocolContext::Resharing(ctx) => ctx.generate_nonce().await,
            ProtocolContext::Recovery(ctx) => ctx.generate_nonce().await,
            ProtocolContext::Locking(ctx) => ctx.generate_nonce().await,
            ProtocolContext::Compaction(ctx) => ctx.generate_nonce().await,
        }
    }

    /// Get key share
    pub async fn get_key_share(&self) -> Result<Vec<u8>, ProtocolError> {
        match self {
            ProtocolContext::Dkd(ctx) => ctx.get_key_share().await,
            ProtocolContext::Resharing(ctx) => ctx.get_key_share().await,
            ProtocolContext::Recovery(ctx) => ctx.get_key_share().await,
            ProtocolContext::Locking(ctx) => ctx.get_key_share().await,
            ProtocolContext::Compaction(ctx) => ctx.get_key_share().await,
        }
    }

    /// Set key share
    pub async fn set_key_share(&mut self, share: Vec<u8>) -> Result<(), ProtocolError> {
        match self {
            ProtocolContext::Dkd(ctx) => ctx.set_key_share(share).await,
            ProtocolContext::Resharing(ctx) => ctx.set_key_share(share).await,
            ProtocolContext::Recovery(ctx) => ctx.set_key_share(share).await,
            ProtocolContext::Locking(ctx) => ctx.set_key_share(share).await,
            ProtocolContext::Compaction(ctx) => ctx.set_key_share(share).await,
        }
    }

    /// Get device public key
    pub async fn get_device_public_key(&self, device_id: &DeviceId) -> Result<Vec<u8>, ProtocolError> {
        match self {
            ProtocolContext::Dkd(ctx) => ctx.get_device_public_key(device_id).await,
            ProtocolContext::Resharing(ctx) => ctx.get_device_public_key(device_id).await,
            ProtocolContext::Recovery(ctx) => ctx.get_device_public_key(device_id).await,
            ProtocolContext::Locking(ctx) => ctx.get_device_public_key(device_id).await,
            ProtocolContext::Compaction(ctx) => ctx.get_device_public_key(device_id).await,
        }
    }

    /// Get device HPKE private key
    pub async fn get_device_hpke_private_key(&self) -> Result<aura_crypto::HpkePrivateKey, ProtocolError> {
        match self {
            ProtocolContext::Dkd(ctx) => ctx.get_device_hpke_private_key().await,
            ProtocolContext::Resharing(ctx) => ctx.get_device_hpke_private_key().await,
            ProtocolContext::Recovery(ctx) => ctx.get_device_hpke_private_key().await,
            ProtocolContext::Locking(ctx) => ctx.get_device_hpke_private_key().await,
            ProtocolContext::Compaction(ctx) => ctx.get_device_hpke_private_key().await,
        }
    }

    /// Get merkle proof
    pub async fn get_merkle_proof(&self) -> Result<Vec<u8>, ProtocolError> {
        match self {
            ProtocolContext::Dkd(ctx) => ctx.base().get_merkle_proof().await,
            ProtocolContext::Resharing(ctx) => ctx.base().get_merkle_proof().await,
            ProtocolContext::Recovery(ctx) => ctx.base().get_merkle_proof().await,
            ProtocolContext::Locking(ctx) => ctx.base().get_merkle_proof().await,
            ProtocolContext::Compaction(ctx) => ctx.base().get_merkle_proof().await,
        }
    }

    // Protocol-specific accessors

    /// Get new participants (for resharing context)
    pub fn new_participants(&self) -> Option<&Vec<DeviceId>> {
        match self {
            ProtocolContext::Resharing(ctx) => Some(&ctx.new_participants),
            _ => None,
        }
    }

    /// Get new threshold (for resharing context)
    pub fn new_threshold(&self) -> Option<usize> {
        match self {
            ProtocolContext::Resharing(ctx) => Some(ctx.new_threshold),
            _ => None,
        }
    }

    /// Get guardians (for recovery context)
    pub fn guardians(&self) -> Option<&Vec<GuardianId>> {
        match self {
            ProtocolContext::Recovery(ctx) => Some(&ctx.guardians),
            _ => None,
        }
    }

    /// Get guardian threshold (for recovery context)
    pub fn guardian_threshold(&self) -> Option<usize> {
        match self {
            ProtocolContext::Recovery(ctx) => Some(ctx.guardian_threshold),
            _ => None,
        }
    }

    /// Get cooldown hours (for recovery context)
    pub fn cooldown_hours(&self) -> Option<u64> {
        match self {
            ProtocolContext::Recovery(ctx) => Some(ctx.cooldown_hours),
            _ => None,
        }
    }

    /// Check if recovery initiator (for recovery context)
    pub fn is_recovery_initiator(&self) -> bool {
        match self {
            ProtocolContext::Recovery(ctx) => ctx.is_recovery_initiator,
            _ => false,
        }
    }

    /// Get guardian ID (for recovery context)
    pub fn guardian_id(&self) -> Option<&GuardianId> {
        match self {
            ProtocolContext::Recovery(ctx) => ctx.guardian_id.as_ref(),
            _ => None,
        }
    }

    /// Get new device ID (for recovery context)
    pub fn new_device_id(&self) -> Option<&DeviceId> {
        match self {
            ProtocolContext::Recovery(ctx) => ctx.new_device_id.as_ref(),
            _ => None,
        }
    }

    /// Get guardian share (for recovery context)
    pub async fn get_guardian_share(&self) -> Result<Vec<u8>, ProtocolError> {
        match self {
            ProtocolContext::Recovery(ctx) => ctx.get_guardian_share().await,
            _ => Err(ProtocolError {
                session_id: self.session_id(),
                error_type: ProtocolErrorType::Other,
                message: "Guardian share only available in recovery context".to_string(),
            }),
        }
    }

    /// Get guardian merkle proof (for recovery context)
    pub async fn get_guardian_merkle_proof(&self, guardian_id: GuardianId) -> Result<Vec<u8>, ProtocolError> {
        match self {
            ProtocolContext::Recovery(ctx) => ctx.get_guardian_merkle_proof(guardian_id).await,
            _ => Err(ProtocolError {
                session_id: self.session_id(),
                error_type: ProtocolErrorType::Other,
                message: "Guardian merkle proof only available in recovery context".to_string(),
            }),
        }
    }

    /// Get DKD commitment root (for DKD context)
    pub async fn get_dkd_commitment_root(&self) -> Result<[u8; 32], ProtocolError> {
        match self {
            ProtocolContext::Dkd(ctx) => ctx.get_dkd_commitment_root().await,
            _ => Err(ProtocolError {
                session_id: self.session_id(),
                error_type: ProtocolErrorType::Other,
                message: "DKD commitment root only available in DKD context".to_string(),
            }),
        }
    }

    /// Set context capsule (for DKD context)
    pub fn set_context_capsule(
        &mut self,
        capsule: std::collections::BTreeMap<String, String>,
    ) -> Result<(), ProtocolError> {
        match self {
            ProtocolContext::Dkd(ctx) => ctx.set_context_capsule(capsule),
            _ => Err(ProtocolError {
                session_id: self.session_id(),
                error_type: ProtocolErrorType::Other,
                message: "Context capsule only available in DKD context".to_string(),
            }),
        }
    }

    // Setters for protocol-specific fields

    /// Set new participants (for resharing context)
    pub fn set_new_participants(&mut self, participants: Vec<DeviceId>) -> Result<(), ProtocolError> {
        match self {
            ProtocolContext::Resharing(ctx) => {
                ctx.new_participants = participants;
                Ok(())
            }
            _ => Err(ProtocolError {
                session_id: self.session_id(),
                error_type: ProtocolErrorType::Other,
                message: "New participants only settable in resharing context".to_string(),
            }),
        }
    }

    /// Set new threshold (for resharing context)
    pub fn set_new_threshold(&mut self, threshold: usize) -> Result<(), ProtocolError> {
        match self {
            ProtocolContext::Resharing(ctx) => {
                ctx.new_threshold = threshold;
                Ok(())
            }
            _ => Err(ProtocolError {
                session_id: self.session_id(),
                error_type: ProtocolErrorType::Other,
                message: "New threshold only settable in resharing context".to_string(),
            }),
        }
    }

    /// Set guardians (for recovery context)
    pub fn set_guardians(&mut self, guardians: Vec<GuardianId>) -> Result<(), ProtocolError> {
        match self {
            ProtocolContext::Recovery(ctx) => {
                ctx.guardians = guardians;
                Ok(())
            }
            _ => Err(ProtocolError {
                session_id: self.session_id(),
                error_type: ProtocolErrorType::Other,
                message: "Guardians only settable in recovery context".to_string(),
            }),
        }
    }

    /// Set guardian threshold (for recovery context)
    pub fn set_guardian_threshold(&mut self, threshold: usize) -> Result<(), ProtocolError> {
        match self {
            ProtocolContext::Recovery(ctx) => {
                ctx.guardian_threshold = threshold;
                Ok(())
            }
            _ => Err(ProtocolError {
                session_id: self.session_id(),
                error_type: ProtocolErrorType::Other,
                message: "Guardian threshold only settable in recovery context".to_string(),
            }),
        }
    }

    /// Set cooldown hours (for recovery context)
    pub fn set_cooldown_hours(&mut self, hours: u64) -> Result<(), ProtocolError> {
        match self {
            ProtocolContext::Recovery(ctx) => {
                ctx.cooldown_hours = hours;
                Ok(())
            }
            _ => Err(ProtocolError {
                session_id: self.session_id(),
                error_type: ProtocolErrorType::Other,
                message: "Cooldown hours only settable in recovery context".to_string(),
            }),
        }
    }

    /// Set recovery initiator (for recovery context)
    pub fn set_recovery_initiator(&mut self, is_initiator: bool) -> Result<(), ProtocolError> {
        match self {
            ProtocolContext::Recovery(ctx) => {
                ctx.set_recovery_initiator(is_initiator);
                Ok(())
            }
            _ => Err(ProtocolError {
                session_id: self.session_id(),
                error_type: ProtocolErrorType::Other,
                message: "Recovery initiator only settable in recovery context".to_string(),
            }),
        }
    }

    /// Set guardian ID (for recovery context)
    pub fn set_guardian_id(&mut self, guardian_id: GuardianId) -> Result<(), ProtocolError> {
        match self {
            ProtocolContext::Recovery(ctx) => {
                ctx.set_guardian_id(guardian_id);
                Ok(())
            }
            _ => Err(ProtocolError {
                session_id: self.session_id(),
                error_type: ProtocolErrorType::Other,
                message: "Guardian ID only settable in recovery context".to_string(),
            }),
        }
    }

    /// Set new device ID (for recovery context)
    pub fn set_new_device_id(&mut self, device_id: DeviceId) -> Result<(), ProtocolError> {
        match self {
            ProtocolContext::Recovery(ctx) => {
                ctx.set_new_device_id(device_id);
                Ok(())
            }
            _ => Err(ProtocolError {
                session_id: self.session_id(),
                error_type: ProtocolErrorType::Other,
                message: "New device ID only settable in recovery context".to_string(),
            }),
        }
    }

    /// Clone for subprotocol (creates appropriate context type)
    pub fn clone_for_subprotocol(&self) -> Self {
        match self {
            ProtocolContext::Dkd(ctx) => {
                let base = ctx.base();
                Self::new_dkd(
                    base.session_id,
                    base.device_id,
                    base.participants.clone(),
                    base.threshold,
                    base.ledger.clone(),
                    base.transport.clone(),
                    base.effects.clone(),
                    SigningKey::from_bytes(&base.device_key.to_bytes()),
                    dyn_clone::clone_box(&*base.time_source),
                )
            }
            ProtocolContext::Resharing(ctx) => {
                let base = ctx.base();
                Self::new_resharing(
                    base.session_id,
                    base.device_id,
                    base.participants.clone(),
                    base.threshold,
                    base.ledger.clone(),
                    base.transport.clone(),
                    base.effects.clone(),
                    SigningKey::from_bytes(&base.device_key.to_bytes()),
                    dyn_clone::clone_box(&*base.time_source),
                    ctx.new_participants.clone(),
                    ctx.new_threshold,
                )
            }
            ProtocolContext::Recovery(ctx) => {
                let base = ctx.base();
                let mut new_ctx = Self::new_recovery(
                    base.session_id,
                    base.device_id,
                    base.participants.clone(),
                    base.threshold,
                    base.ledger.clone(),
                    base.transport.clone(),
                    base.effects.clone(),
                    SigningKey::from_bytes(&base.device_key.to_bytes()),
                    dyn_clone::clone_box(&*base.time_source),
                    ctx.guardians.clone(),
                    ctx.guardian_threshold,
                    ctx.cooldown_hours,
                );
                if let ProtocolContext::Recovery(new_recovery_ctx) = &mut new_ctx {
                    new_recovery_ctx.is_recovery_initiator = ctx.is_recovery_initiator;
                    new_recovery_ctx.guardian_id = ctx.guardian_id;
                    new_recovery_ctx.new_device_id = ctx.new_device_id;
                }
                new_ctx
            }
            ProtocolContext::Locking(ctx) => {
                let base = ctx.base();
                Self::new_locking(
                    base.session_id,
                    base.device_id,
                    base.participants.clone(),
                    base.threshold,
                    base.ledger.clone(),
                    base.transport.clone(),
                    base.effects.clone(),
                    SigningKey::from_bytes(&base.device_key.to_bytes()),
                    dyn_clone::clone_box(&*base.time_source),
                )
            }
            ProtocolContext::Compaction(ctx) => {
                let base = ctx.base();
                let base_ctx = BaseContext::new(
                    base.session_id,
                    base.device_id,
                    base.participants.clone(),
                    base.threshold,
                    base.ledger.clone(),
                    base.transport.clone(),
                    base.effects.clone(),
                    SigningKey::from_bytes(&base.device_key.to_bytes()),
                    dyn_clone::clone_box(&*base.time_source),
                );
                ProtocolContext::Compaction(CompactionContext::new(base_ctx))
            }
        }
    }

    /// Push event to pending queue
    pub fn push_event(&mut self, event: Event) {
        let base = match self {
            ProtocolContext::Dkd(ctx) => ctx.base_mut(),
            ProtocolContext::Resharing(ctx) => ctx.base_mut(),
            ProtocolContext::Recovery(ctx) => ctx.base_mut(),
            ProtocolContext::Locking(ctx) => ctx.base_mut(),
            ProtocolContext::Compaction(ctx) => ctx.base_mut(),
        };
        base.pending_events.push_back(event);
    }

    /// Notify new events
    pub fn notify_new_events(&mut self, events: Vec<Event>) {
        let base = match self {
            ProtocolContext::Dkd(ctx) => ctx.base_mut(),
            ProtocolContext::Resharing(ctx) => ctx.base_mut(),
            ProtocolContext::Recovery(ctx) => ctx.base_mut(),
            ProtocolContext::Locking(ctx) => ctx.base_mut(),
            ProtocolContext::Compaction(ctx) => ctx.base_mut(),
        };
        base.pending_events.extend(events);
    }

    /// Run sub-protocol with appropriate context
    pub async fn run_sub_protocol(
        &mut self,
        protocol_type: ProtocolType,
        config: ProtocolConfig,
    ) -> Result<InstructionResult, ProtocolError> {
        use crate::choreography::{dkd_choreography, resharing_choreography, recovery_choreography, locking_choreography};
        
        let result = match (protocol_type, config) {
            (ProtocolType::Dkd, ProtocolConfig::Dkd { .. }) => {
                let context_id = vec![]; // Default empty context
                let key = dkd_choreography(self, context_id).await?;
                ProtocolResult::DkdComplete {
                    session_id: self.session_id(),
                    derived_key: key,
                }
            }

            (ProtocolType::Resharing, ProtocolConfig::Resharing { new_participants, new_threshold }) => {
                let participants_vec: Vec<_> = new_participants.into_iter().collect();
                let shares = resharing_choreography(self, Some(new_threshold), Some(participants_vec)).await?;
                ProtocolResult::ResharingComplete {
                    session_id: self.session_id(),
                    new_share: shares,
                }
            }

            (ProtocolType::Recovery, ProtocolConfig::Recovery { guardians, threshold }) => {
                let guardians_vec: Vec<_> = guardians.into_iter().map(GuardianId).collect();
                let shares = recovery_choreography(self, guardians_vec, threshold as u16).await?;
                ProtocolResult::RecoveryComplete {
                    recovery_id: self.session_id(),
                    recovered_share: shares,
                }
            }

            (ProtocolType::Locking, ProtocolConfig::Locking { operation_type }) => {
                let op_type = match operation_type.as_str() {
                    "dkd" => aura_journal::OperationType::Dkd,
                    "resharing" => aura_journal::OperationType::Resharing,
                    "recovery" => aura_journal::OperationType::Recovery,
                    _ => aura_journal::OperationType::Dkd,
                };
                locking_choreography(self, op_type).await?;
                ProtocolResult::LockAcquired {
                    session_id: self.session_id(),
                }
            }

            (ProtocolType::Compaction, _) => {
                return Err(ProtocolError {
                    session_id: self.session_id(),
                    error_type: ProtocolErrorType::Other,
                    message: "Compaction protocol not yet implemented".to_string(),
                });
            }

            // Mismatched protocol type and config
            (ptype, _) => {
                return Err(ProtocolError {
                    session_id: self.session_id(),
                    error_type: ProtocolErrorType::Other,
                    message: format!("Mismatched protocol type {:?} and config", ptype),
                });
            }
        };

        Ok(InstructionResult::SubProtocolComplete(result))
    }
}

// Re-export stub transport for backward compatibility
// Transport is already imported at the top

/// Stub transport implementation for testing and development
#[derive(Debug, Default, Clone)]
pub struct StubTransport;

#[async_trait::async_trait]
impl Transport for StubTransport {
    async fn send_message(&self, _peer_id: &str, _message: &[u8]) -> Result<(), String> {
        Ok(())
    }
    
    async fn broadcast_message(&self, _message: &[u8]) -> Result<(), String> {
        Ok(())
    }
    
    async fn is_peer_reachable(&self, _peer_id: &str) -> bool {
        true
    }
}