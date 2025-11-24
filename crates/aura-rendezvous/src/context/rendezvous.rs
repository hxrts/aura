//! Context-Aware Rendezvous System
//!
//! This module implements rendezvous using ContextId instead of device identifiers,
//! aligning with the authority-centric architecture.

use crate::sbb::{EnvelopeId, FloodResult};
use aura_core::identifiers::ContextId;
use aura_core::time::{OrderTime, PhysicalTime, TimeStamp};
use aura_core::{AuraError, AuraResult, AuthorityId};
use aura_protocol::effects::AuraEffects;
use aura_relational::RelationalContext;
// GuardChain will be integrated once available from aura-protocol
use aura_journal::{Fact, FactContent};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

/// Context-scoped rendezvous descriptor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextRendezvousDescriptor {
    /// Context ID for rendezvous scope
    pub context_id: ContextId,
    /// Authority offering rendezvous
    pub authority_id: AuthorityId,
    /// Available transport methods
    pub transport_offers: Vec<ContextTransportOffer>,
    /// TTL for flooding
    pub ttl: u8,
    /// Creation timestamp
    pub created_at: u64,
    /// Expiration timestamp
    pub expires_at: u64,
    /// Nonce for replay protection
    pub nonce: [u8; 32],
}

/// Transport offer within a context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextTransportOffer {
    /// Transport protocol (e.g., "quic", "tcp", "webrtc")
    pub protocol: String,
    /// Connection endpoint (e.g., "192.168.1.1:8080")
    pub endpoint: String,
    /// Public key for transport encryption
    pub transport_pubkey: Vec<u8>,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

/// Context-aware rendezvous envelope
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextEnvelope {
    /// Content-addressed envelope ID
    pub id: EnvelopeId,
    /// Context scope
    pub context_id: ContextId,
    /// Source authority
    pub source_authority: AuthorityId,
    /// Payload (encrypted descriptor)
    pub payload: Vec<u8>,
    /// TTL for flooding
    pub ttl: u8,
    /// Guard chain for authorization
    pub guard_chain: Vec<u8>, // Serialized GuardChain
    /// Receipt proof for journal integration
    pub receipt: Option<RendezvousReceipt>,
}

/// Receipt for rendezvous traffic
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RendezvousReceipt {
    /// Envelope ID being receipted
    pub envelope_id: EnvelopeId,
    /// Authority issuing receipt
    pub authority_id: AuthorityId,
    /// Timestamp of receipt
    pub timestamp: u64,
    /// Signature over receipt data
    pub signature: Vec<u8>,
}

/// Serializable guard chain for rendezvous flooding authorization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RendezvousGuardChain {
    /// Context scope for this guard chain
    pub context_id: ContextId,
    /// Authority creating this guard chain
    pub authority_id: AuthorityId,
    /// Required Biscuit authorization tokens (serialized as bytes)
    pub required_tokens: Vec<Vec<u8>>,
    /// Privacy leakage budget for this operation
    pub leakage_budget: aura_protocol::guards::LeakageBudget,
    /// Operation identifier
    pub operation_id: String,
    /// Creation timestamp for replay protection
    pub timestamp: u64,
    /// Random nonce for uniqueness
    pub nonce: [u8; 16],
}

/// Message types for rendezvous protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RendezvousMessageType {
    /// Context envelope for flooding
    ContextEnvelope,
    /// Transport offer advertisement
    TransportOffer,
    /// Direct connection request
    ConnectionRequest,
}

/// Network message for rendezvous protocol transport
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RendezvousMessage {
    /// Type of rendezvous message
    pub message_type: RendezvousMessageType,
    /// Context scope for this message
    pub context_id: ContextId,
    /// Source authority
    pub source_authority: AuthorityId,
    /// Message payload (serialized envelope, offer, etc.)
    pub payload: Vec<u8>,
    /// TTL for flooding messages
    pub ttl: u8,
    /// Envelope ID for tracking
    pub envelope_id: EnvelopeId,
}

/// Connection request between authorities in a context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextConnectionRequest {
    /// Context scope for the connection
    pub context_id: ContextId,
    /// Authority initiating the connection
    pub source_authority: AuthorityId,
    /// Authority being connected to
    pub target_authority: AuthorityId,
    /// Transport protocol to use
    pub transport_protocol: String,
    /// Random nonce for connection uniqueness
    pub connection_nonce: [u8; 32],
    /// Request timestamp
    pub timestamp: u64,
    /// Additional transport metadata
    pub metadata: HashMap<String, String>,
}

/// Established transport connection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportConnection {
    /// Unique connection identifier
    pub connection_id: String,
    /// Transport protocol used
    pub protocol: String,
    /// Connection endpoint
    pub endpoint: String,
    /// When the connection was established
    pub established_at: u64,
    /// Connected peer authority
    pub peer_authority: AuthorityId,
    /// Context scope
    pub context_id: ContextId,
}

/// Authentication challenge for connection establishment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionAuthChallenge {
    /// Context scope for authentication
    pub context_id: ContextId,
    /// Authority issuing the challenge
    pub authority_id: AuthorityId,
    /// Random challenge bytes
    pub challenge: Vec<u8>,
    /// Challenge timestamp
    pub timestamp: u64,
}

/// Context-aware rendezvous coordinator
pub struct ContextRendezvousCoordinator {
    /// Local authority
    local_authority: AuthorityId,
    /// Relational contexts
    contexts: HashMap<ContextId, Arc<RelationalContext>>,
    /// Seen envelopes for deduplication
    seen_envelopes: HashSet<EnvelopeId>,
    /// Cache expiration time (reserved for future use)
    #[allow(dead_code)]
    cache_duration: Duration,
    /// Effects system
    effects: Arc<dyn AuraEffects>,
}

impl ContextRendezvousCoordinator {
    /// Create a new context-aware rendezvous coordinator
    pub fn new(local_authority: AuthorityId, effects: Arc<dyn AuraEffects>) -> Self {
        Self {
            local_authority,
            contexts: HashMap::new(),
            seen_envelopes: HashSet::new(),
            cache_duration: Duration::from_secs(300), // 5 minute cache
            effects,
        }
    }

    /// Add a relational context
    pub fn add_context(&mut self, context: Arc<RelationalContext>) {
        self.contexts.insert(context.context_id, context);
    }

    /// Create a rendezvous descriptor for a context
    pub async fn create_descriptor(
        &self,
        context_id: ContextId,
        transport_offers: Vec<ContextTransportOffer>,
        ttl: u8,
    ) -> AuraResult<ContextRendezvousDescriptor> {
        // Verify we're a participant in this context
        let context = self
            .contexts
            .get(&context_id)
            .ok_or_else(|| AuraError::not_found("Context not found"))?;

        if !context.is_participant(&self.local_authority) {
            return Err(AuraError::permission_denied(
                "Not a participant in context".to_string(),
            ));
        }

        let now = aura_core::TimeEffects::current_timestamp(self.effects.as_ref()).await;
        let expires_at = now + 3600; // 1 hour expiration

        Ok(ContextRendezvousDescriptor {
            context_id,
            authority_id: self.local_authority,
            transport_offers,
            ttl,
            created_at: now,
            expires_at,
            nonce: {
                let bytes = self.effects.random_bytes(32).await;
                bytes.try_into().unwrap()
            },
        })
    }

    /// Create and flood a context envelope
    pub async fn flood_descriptor(
        &mut self,
        descriptor: ContextRendezvousDescriptor,
    ) -> AuraResult<FloodResult> {
        // Serialize and encrypt descriptor
        let payload = self.encrypt_descriptor(&descriptor).await?;

        // Create envelope with guard chain
        let envelope = ContextEnvelope {
            id: self.compute_envelope_id(&payload),
            context_id: descriptor.context_id,
            source_authority: self.local_authority,
            payload,
            ttl: descriptor.ttl,
            guard_chain: self.serialize_guard_chain(&descriptor.context_id).await?,
            receipt: None,
        };

        // Check guard chain before flooding
        if !self.evaluate_guard_chain(&envelope).await? {
            return Ok(FloodResult::Dropped);
        }

        // Mark as seen
        self.seen_envelopes.insert(envelope.id);

        // Get context participants for flooding
        let participants = self
            .get_context_participants(&descriptor.context_id)
            .await?;

        // Forward to participants
        let mut forwarded_count = 0;
        for participant in participants {
            if participant != self.local_authority {
                match self
                    .forward_to_authority(envelope.clone(), participant)
                    .await
                {
                    Ok(_) => forwarded_count += 1,
                    Err(e) => {
                        tracing::warn!("Failed to forward to {}: {}", participant, e);
                    }
                }
            }
        }

        Ok(FloodResult::Forwarded {
            peer_count: forwarded_count,
        })
    }

    /// Handle incoming context envelope
    pub async fn handle_envelope(&mut self, envelope: ContextEnvelope) -> AuraResult<FloodResult> {
        // Check if already seen
        if self.seen_envelopes.contains(&envelope.id) {
            return Ok(FloodResult::Dropped);
        }

        // Evaluate guard chain
        if !self.evaluate_guard_chain(&envelope).await? {
            return Ok(FloodResult::Dropped);
        }

        // Check context participation
        let context = self
            .contexts
            .get(&envelope.context_id)
            .ok_or_else(|| AuraError::not_found("Context not found"))?;

        if !context.is_participant(&self.local_authority) {
            return Ok(FloodResult::Dropped);
        }

        // Mark as seen
        self.seen_envelopes.insert(envelope.id);

        // Create receipt if configured
        if let Some(receipt) = self.create_receipt(&envelope).await? {
            // Store receipt as journal fact
            self.store_receipt_fact(receipt.clone()).await?;
        }

        // Continue flooding if TTL > 0
        if envelope.ttl > 0 {
            let mut forwarded_envelope = envelope.clone();
            forwarded_envelope.ttl -= 1;

            let participants = self.get_context_participants(&envelope.context_id).await?;
            let mut forwarded_count = 0;

            for participant in participants {
                if participant != self.local_authority && participant != envelope.source_authority {
                    match self
                        .forward_to_authority(forwarded_envelope.clone(), participant)
                        .await
                    {
                        Ok(_) => forwarded_count += 1,
                        Err(e) => {
                            tracing::warn!("Failed to forward to {}: {}", participant, e);
                        }
                    }
                }
            }

            Ok(FloodResult::Forwarded {
                peer_count: forwarded_count,
            })
        } else {
            Ok(FloodResult::Dropped)
        }
    }

    /// Encrypt descriptor for privacy using context-derived keys
    async fn encrypt_descriptor(
        &self,
        descriptor: &ContextRendezvousDescriptor,
    ) -> AuraResult<Vec<u8>> {
        // Get context for key derivation
        let context = self
            .contexts
            .get(&descriptor.context_id)
            .ok_or_else(|| AuraError::not_found("Context not found for encryption"))?;

        // Serialize the descriptor first
        let plaintext =
            serde_json::to_vec(descriptor).map_err(|e| AuraError::serialization(e.to_string()))?;

        // Derive encryption key from context
        let encryption_key = self
            .derive_context_encryption_key(&descriptor.context_id, context)
            .await?;

        // Generate random nonce for ChaCha20-Poly1305
        let nonce_bytes = self.effects.random_bytes(12).await;
        let nonce: [u8; 12] = nonce_bytes
            .try_into()
            .map_err(|_| AuraError::crypto("Invalid nonce size"))?;

        // Encrypt using ChaCha20-Poly1305
        let ciphertext = self
            .effects
            .chacha20_encrypt(&plaintext, &encryption_key, &nonce)
            .await
            .map_err(|e| AuraError::crypto(format!("Encryption failed: {}", e)))?;

        // Prepend nonce to ciphertext for transmission
        let mut encrypted_payload = Vec::with_capacity(12 + ciphertext.len());
        encrypted_payload.extend_from_slice(&nonce);
        encrypted_payload.extend_from_slice(&ciphertext);

        tracing::debug!(
            "Encrypted rendezvous descriptor for context {} ({} bytes plaintext -> {} bytes encrypted)",
            descriptor.context_id,
            plaintext.len(),
            encrypted_payload.len()
        );

        Ok(encrypted_payload)
    }

    /// Derive encryption key from context using HKDF
    async fn derive_context_encryption_key(
        &self,
        context_id: &ContextId,
        context: &RelationalContext,
    ) -> AuraResult<[u8; 32]> {
        // Use context's shared secret as input key material
        let context_ikm = context
            .shared_secret()
            .ok_or_else(|| AuraError::crypto("Context missing shared secret"))?;

        // Salt combines context ID and domain separator
        let mut salt = Vec::with_capacity(32 + context_id.as_bytes().len());
        salt.extend_from_slice(b"aura-rendezvous-encryption-v1");
        salt.extend_from_slice(context_id.as_bytes());

        // Info string for key derivation purpose
        let info = format!("rendezvous-encrypt:{}:{}", context_id, self.local_authority);

        // Derive 32-byte key using HKDF
        let derived_key = self
            .effects
            .hkdf_derive(&context_ikm, &salt, info.as_bytes(), 32)
            .await
            .map_err(|e| AuraError::crypto(format!("Key derivation failed: {}", e)))?;

        // Convert to fixed-size array
        let key: [u8; 32] = derived_key
            .try_into()
            .map_err(|_| AuraError::crypto("Invalid derived key size"))?;

        tracing::debug!(
            "Derived encryption key for context {} using HKDF-SHA256",
            context_id
        );

        Ok(key)
    }

    /// Decrypt descriptor received from peer
    #[allow(dead_code)]
    async fn decrypt_descriptor(
        &self,
        context_id: &ContextId,
        encrypted_payload: &[u8],
    ) -> AuraResult<ContextRendezvousDescriptor> {
        // Get context for key derivation
        let context = self
            .contexts
            .get(context_id)
            .ok_or_else(|| AuraError::not_found("Context not found for decryption"))?;

        // Extract nonce and ciphertext
        if encrypted_payload.len() < 12 {
            return Err(AuraError::crypto("Invalid encrypted payload: too short"));
        }

        let (nonce_bytes, ciphertext) = encrypted_payload.split_at(12);
        let nonce: [u8; 12] = nonce_bytes
            .try_into()
            .map_err(|_| AuraError::crypto("Invalid nonce in encrypted payload"))?;

        // Derive the same encryption key
        let encryption_key = self
            .derive_context_encryption_key(context_id, context)
            .await?;

        // Decrypt using ChaCha20-Poly1305
        let plaintext = self
            .effects
            .chacha20_decrypt(ciphertext, &encryption_key, &nonce)
            .await
            .map_err(|e| AuraError::crypto(format!("Decryption failed: {}", e)))?;

        // Deserialize the descriptor
        let descriptor: ContextRendezvousDescriptor =
            serde_json::from_slice(&plaintext).map_err(|e| {
                AuraError::serialization(format!("Failed to deserialize descriptor: {}", e))
            })?;

        tracing::debug!(
            "Decrypted rendezvous descriptor for context {} ({} bytes encrypted -> {} bytes plaintext)",
            context_id,
            encrypted_payload.len(),
            plaintext.len()
        );

        Ok(descriptor)
    }

    /// Compute content-addressed envelope ID
    fn compute_envelope_id(&self, payload: &[u8]) -> EnvelopeId {
        use aura_core::hash::hasher;
        let mut h = hasher();
        h.update(b"aura-context-envelope-v1");
        h.update(payload);
        h.finalize()
    }

    /// Serialize guard chain for transport
    async fn serialize_guard_chain(&self, context_id: &ContextId) -> AuraResult<Vec<u8>> {
        use aura_protocol::guards::{LeakageBudget, ProtocolGuard};

        // Get context for guard chain construction
        let _context = self
            .contexts
            .get(context_id)
            .ok_or_else(|| AuraError::not_found("Context not found for guard chain"))?;

        // Create protocol guard for rendezvous flooding
        // TODO: Create actual Biscuit tokens with proper authorization
        let guard =
            ProtocolGuard::new("rendezvous_flood").leakage_budget(LeakageBudget::new(1, 2, 0)); // Minimal leakage for routing

        // Create guard chain structure for serialization
        let guard_chain = RendezvousGuardChain {
            context_id: *context_id,
            authority_id: self.local_authority,
            required_tokens: Vec::new(), // Empty since we're not using tokens for now
            leakage_budget: guard.leakage_budget,
            operation_id: guard.operation_id,
            timestamp: aura_core::TimeEffects::current_timestamp(self.effects.as_ref()).await,
            nonce: {
                let bytes = self.effects.random_bytes(16).await;
                bytes
                    .try_into()
                    .map_err(|_| AuraError::crypto("Invalid nonce size"))?
            },
        };

        // Serialize the guard chain using a compact binary format
        let serialized = self.serialize_guard_chain_binary(&guard_chain).await?;

        tracing::debug!(
            "Serialized guard chain for context {} ({} bytes)",
            context_id,
            serialized.len()
        );

        Ok(serialized)
    }

    /// Serialize guard chain to compact binary format
    async fn serialize_guard_chain_binary(
        &self,
        guard_chain: &RendezvousGuardChain,
    ) -> AuraResult<Vec<u8>> {
        // Use bincode for compact serialization suitable for network transport
        bincode::serialize(guard_chain).map_err(|e| {
            AuraError::serialization(format!("Guard chain serialization failed: {}", e))
        })
    }

    /// Deserialize guard chain from binary format
    async fn deserialize_guard_chain_binary(
        &self,
        data: &[u8],
    ) -> AuraResult<RendezvousGuardChain> {
        bincode::deserialize(data).map_err(|e| {
            AuraError::serialization(format!("Guard chain deserialization failed: {}", e))
        })
    }

    /// Evaluate guard chain for authorization
    async fn evaluate_guard_chain(&self, envelope: &ContextEnvelope) -> AuraResult<bool> {
        // Deserialize the guard chain from the envelope
        let guard_chain = match self
            .deserialize_guard_chain_binary(&envelope.guard_chain)
            .await
        {
            Ok(chain) => chain,
            Err(e) => {
                tracing::warn!("Failed to deserialize guard chain: {}", e);
                return Ok(false);
            }
        };

        // Verify basic envelope consistency
        if guard_chain.context_id != envelope.context_id
            || guard_chain.authority_id != envelope.source_authority
        {
            tracing::warn!(
                "Guard chain context/authority mismatch: chain({}, {}) vs envelope({}, {})",
                guard_chain.context_id,
                guard_chain.authority_id,
                envelope.context_id,
                envelope.source_authority
            );
            return Ok(false);
        }

        // Check timestamp freshness (prevent replay attacks)
        let current_time = aura_core::TimeEffects::current_timestamp(self.effects.as_ref()).await;
        let max_age = 300; // 5 minutes
        if current_time.saturating_sub(guard_chain.timestamp) > max_age {
            tracing::warn!(
                "Guard chain too old: {} seconds",
                current_time.saturating_sub(guard_chain.timestamp)
            );
            return Ok(false);
        }

        // Verify context participation
        let context = match self.contexts.get(&envelope.context_id) {
            Some(context) => context,
            None => {
                tracing::warn!(
                    "Context {} not found for guard evaluation",
                    envelope.context_id
                );
                return Ok(false);
            }
        };

        if !context.is_participant(&envelope.source_authority) {
            tracing::warn!(
                "Authority {} not a participant in context {}",
                envelope.source_authority,
                envelope.context_id
            );
            return Ok(false);
        }

        // Evaluate Biscuit authorization tokens if available
        if !guard_chain.required_tokens.is_empty() {
            match self
                .evaluate_biscuit_tokens(&guard_chain, envelope, context)
                .await
            {
                Ok(authorized) => {
                    if !authorized {
                        tracing::warn!("Biscuit token authorization failed for guard chain");
                        return Ok(false);
                    }
                }
                Err(e) => {
                    tracing::warn!("Biscuit evaluation error: {}", e);
                    return Ok(false);
                }
            }
        }

        // Check leakage budget if specified
        if let Err(e) = self.check_leakage_budget(&guard_chain, context).await {
            tracing::warn!("Leakage budget exceeded: {}", e);
            return Ok(false);
        }

        tracing::debug!(
            "Guard chain evaluation passed for envelope {} from authority {} in context {}",
            hex::encode(envelope.id),
            envelope.source_authority,
            envelope.context_id
        );

        Ok(true)
    }

    /// Evaluate Biscuit authorization tokens in the guard chain
    async fn evaluate_biscuit_tokens(
        &self,
        guard_chain: &RendezvousGuardChain,
        _envelope: &ContextEnvelope,
        _context: &Arc<aura_relational::RelationalContext>,
    ) -> AuraResult<bool> {
        // TODO: Implement proper Biscuit token validation
        // For now, skip token validation since tokens are empty
        if !guard_chain.required_tokens.is_empty() {
            tracing::debug!(
                "Biscuit token validation not yet implemented, {} tokens present",
                guard_chain.required_tokens.len()
            );
            // In full implementation, use BiscuitGuardEvaluator to validate tokens
        }

        // Token validation passed (placeholder)
        tracing::debug!(
            "Biscuit token validation passed for {} tokens",
            guard_chain.required_tokens.len()
        );
        Ok(true)
    }

    /// Check leakage budget against context limits
    async fn check_leakage_budget(
        &self,
        guard_chain: &RendezvousGuardChain,
        _context: &Arc<aura_relational::RelationalContext>,
    ) -> AuraResult<()> {
        // Define reasonable limits for rendezvous flooding
        let max_budget = aura_protocol::guards::LeakageBudget::new(5, 10, 2); // bits

        if !guard_chain.leakage_budget.is_within_limits(&max_budget) {
            return Err(AuraError::invalid(format!(
                "Leakage budget exceeds limits: {:?} > {:?}",
                guard_chain.leakage_budget, max_budget
            )));
        }

        tracing::debug!(
            "Leakage budget check passed: external={}, neighbor={}, in_group={}",
            guard_chain.leakage_budget.external,
            guard_chain.leakage_budget.neighbor,
            guard_chain.leakage_budget.in_group
        );

        Ok(())
    }

    /// Get participants in a context
    async fn get_context_participants(
        &self,
        context_id: &ContextId,
    ) -> AuraResult<Vec<AuthorityId>> {
        let context = self
            .contexts
            .get(context_id)
            .ok_or_else(|| AuraError::not_found("Context not found"))?;

        Ok(context.get_participants().to_vec())
    }

    /// Forward envelope to another authority via transport effects
    async fn forward_to_authority(
        &self,
        envelope: ContextEnvelope,
        authority: AuthorityId,
    ) -> AuraResult<()> {
        // Serialize the envelope for transport
        let envelope_data = self.serialize_envelope(&envelope).await?;

        // Get authority's current device endpoints from context
        let target_devices = self
            .get_authority_devices(&authority, &envelope.context_id)
            .await?;

        if target_devices.is_empty() {
            return Err(AuraError::not_found(format!(
                "No devices found for authority {}",
                authority
            )));
        }

        // Try forwarding to each device until one succeeds
        let mut last_error = None;
        for device_id in &target_devices {
            match self
                .send_to_device(&envelope_data, *device_id, &envelope)
                .await
            {
                Ok(_) => {
                    tracing::debug!(
                        "Successfully forwarded envelope {} to authority {} via device {}",
                        hex::encode(envelope.id),
                        authority,
                        device_id
                    );
                    return Ok(());
                }
                Err(e) => {
                    tracing::debug!(
                        "Failed to forward envelope {} to device {}: {}",
                        hex::encode(envelope.id),
                        device_id,
                        e
                    );
                    last_error = Some(e);
                }
            }
        }

        // All devices failed
        match last_error {
            Some(e) => Err(AuraError::network(format!(
                "Failed to forward envelope to authority {}: {}",
                authority, e
            ))),
            None => Err(AuraError::network(format!(
                "No devices available for authority {}",
                authority
            ))),
        }
    }

    /// Serialize envelope for network transport
    async fn serialize_envelope(&self, envelope: &ContextEnvelope) -> AuraResult<Vec<u8>> {
        bincode::serialize(envelope)
            .map_err(|e| AuraError::serialization(format!("Envelope serialization failed: {}", e)))
    }

    /// Get devices associated with an authority in a specific context
    async fn get_authority_devices(
        &self,
        authority: &AuthorityId,
        context_id: &ContextId,
    ) -> AuraResult<Vec<aura_core::DeviceId>> {
        // Get context to access authority-device mappings
        let _context = self
            .contexts
            .get(context_id)
            .ok_or_else(|| AuraError::not_found("Context not found for device lookup"))?;

        // Get devices for this authority from context
        // TODO: Implement device discovery for authorities in the new authority-centric architecture
        // The concept of "devices for authority" is being revisited in the new architecture
        let devices = Vec::new(); // Placeholder: no device enumeration in authority-centric model

        tracing::debug!(
            "Device enumeration not implemented for authority {} in context {}",
            authority,
            context_id
        );

        Ok(devices)
    }

    /// Send envelope data to a specific device using network effects
    async fn send_to_device(
        &self,
        envelope_data: &[u8],
        device_id: aura_core::DeviceId,
        envelope: &ContextEnvelope,
    ) -> AuraResult<()> {
        // Create message for network transport
        let message = RendezvousMessage {
            message_type: RendezvousMessageType::ContextEnvelope,
            context_id: envelope.context_id,
            source_authority: envelope.source_authority,
            payload: envelope_data.to_vec(),
            ttl: envelope.ttl,
            envelope_id: envelope.id,
        };

        // Serialize the message for network transport
        let message_data = bincode::serialize(&message).map_err(|e| {
            AuraError::serialization(format!("Message serialization failed: {}", e))
        })?;

        // Send via network effects
        // Convert DeviceId to Uuid for NetworkEffects API
        let peer_uuid: uuid::Uuid = device_id.into();
        let message_size = message_data.len();
        self.effects
            .send_to_peer(peer_uuid, message_data)
            .await
            .map_err(|e| AuraError::network(format!("Failed to send message: {}", e)))?;

        tracing::trace!(
            "Sent rendezvous envelope {} ({} bytes) to device {}",
            hex::encode(envelope.id),
            message_size,
            device_id
        );

        Ok(())
    }

    /// Create and sign receipt for envelope
    async fn create_receipt(
        &self,
        envelope: &ContextEnvelope,
    ) -> AuraResult<Option<RendezvousReceipt>> {
        // Check if receipts are enabled for this context
        let context = self
            .contexts
            .get(&envelope.context_id)
            .ok_or_else(|| AuraError::not_found("Context not found for receipt creation"))?;

        // TODO: Implement receipt generation policy in RelationalContext
        // For now, always generate receipts
        tracing::debug!(
            "Receipt generation enabled for context {}",
            envelope.context_id
        );

        let timestamp = aura_core::TimeEffects::current_timestamp(self.effects.as_ref()).await;

        // Construct receipt message for signing
        let receipt_message =
            self.construct_receipt_message(&envelope.id, &self.local_authority, timestamp);

        // Sign the receipt using context-derived key
        let signature = self
            .sign_receipt(&receipt_message, &envelope.context_id, context)
            .await?;

        let receipt = RendezvousReceipt {
            envelope_id: envelope.id,
            authority_id: self.local_authority,
            timestamp,
            signature,
        };

        tracing::debug!(
            "Created signed receipt for envelope {} from authority {} at timestamp {}",
            hex::encode(envelope.id),
            self.local_authority,
            timestamp
        );

        Ok(Some(receipt))
    }

    /// Construct canonical receipt message for signing
    fn construct_receipt_message(
        &self,
        envelope_id: &EnvelopeId,
        authority_id: &AuthorityId,
        timestamp: u64,
    ) -> Vec<u8> {
        use std::io::Write;

        let mut message = Vec::new();

        // Domain separator
        message.write_all(b"AURA_RENDEZVOUS_RECEIPT_V1").unwrap();

        // Envelope ID
        message.write_all(envelope_id).unwrap();

        // Authority ID
        message.write_all(authority_id.0.as_bytes()).unwrap();

        // Timestamp
        message.write_all(&timestamp.to_le_bytes()).unwrap();

        message
    }

    /// Sign receipt using context-derived signing key
    async fn sign_receipt(
        &self,
        receipt_message: &[u8],
        context_id: &ContextId,
        context: &Arc<aura_relational::RelationalContext>,
    ) -> AuraResult<Vec<u8>> {
        // Derive signing key from context using HKDF
        let signing_key = self.derive_context_signing_key(context_id, context).await?;

        // Sign using Ed25519
        let signature = self
            .effects
            .ed25519_sign(receipt_message, &signing_key)
            .await
            .map_err(|e| AuraError::crypto(format!("Receipt signing failed: {}", e)))?;

        tracing::debug!(
            "Signed receipt for context {} ({} bytes message -> {} bytes signature)",
            context_id,
            receipt_message.len(),
            signature.len()
        );

        Ok(signature)
    }

    /// Derive signing key from context for receipt generation
    async fn derive_context_signing_key(
        &self,
        context_id: &ContextId,
        context: &Arc<aura_relational::RelationalContext>,
    ) -> AuraResult<Vec<u8>> {
        // Use context's shared secret as input key material
        let context_ikm = context
            .shared_secret()
            .ok_or_else(|| AuraError::crypto("Context missing shared secret for signing"))?;

        // Salt combines context ID and domain separator
        let mut salt = Vec::with_capacity(32 + context_id.as_bytes().len());
        salt.extend_from_slice(b"aura-rendezvous-signing-v1");
        salt.extend_from_slice(context_id.as_bytes());

        // Info string for key derivation purpose
        let info = format!("rendezvous-sign:{}:{}", context_id, self.local_authority);

        // Derive 32-byte key using HKDF
        let derived_key = self
            .effects
            .hkdf_derive(&context_ikm, &salt, info.as_bytes(), 32)
            .await
            .map_err(|e| AuraError::crypto(format!("Signing key derivation failed: {}", e)))?;

        tracing::debug!(
            "Derived signing key for context {} using HKDF-SHA256",
            context_id
        );

        Ok(derived_key)
    }

    /// Validate receipt signature
    #[allow(dead_code)]
    async fn validate_receipt(
        &self,
        receipt: &RendezvousReceipt,
        envelope_id: &EnvelopeId,
        context_id: &ContextId,
    ) -> AuraResult<bool> {
        // Get context for key derivation
        let context = self
            .contexts
            .get(context_id)
            .ok_or_else(|| AuraError::not_found("Context not found for receipt validation"))?;

        // Construct the same receipt message
        let receipt_message =
            self.construct_receipt_message(envelope_id, &receipt.authority_id, receipt.timestamp);

        // Derive the public key for verification
        let public_key = self
            .derive_context_public_key(context_id, context, &receipt.authority_id)
            .await?;

        // Verify the signature
        let valid = self
            .effects
            .ed25519_verify(&receipt_message, &receipt.signature, &public_key)
            .await
            .map_err(|e| AuraError::crypto(format!("Receipt verification failed: {}", e)))?;

        if valid {
            tracing::debug!(
                "Receipt validation passed for envelope {} from authority {}",
                hex::encode(envelope_id),
                receipt.authority_id
            );
        } else {
            tracing::warn!(
                "Receipt validation failed for envelope {} from authority {}",
                hex::encode(envelope_id),
                receipt.authority_id
            );
        }

        Ok(valid)
    }

    /// Derive public key for receipt verification
    #[allow(dead_code)]
    async fn derive_context_public_key(
        &self,
        context_id: &ContextId,
        context: &Arc<aura_relational::RelationalContext>,
        authority_id: &AuthorityId,
    ) -> AuraResult<Vec<u8>> {
        // For signature verification, derive the same signing key and extract public key
        // This is a simplified approach; in practice, public keys would be exchanged separately

        // Use context's shared secret as input key material
        let context_ikm = context
            .shared_secret()
            .ok_or_else(|| AuraError::crypto("Context missing shared secret for verification"))?;

        // Salt combines context ID and domain separator
        let mut salt = Vec::with_capacity(32 + context_id.as_bytes().len());
        salt.extend_from_slice(b"aura-rendezvous-signing-v1");
        salt.extend_from_slice(context_id.as_bytes());

        // Info string for key derivation purpose (from authority's perspective)
        let info = format!("rendezvous-sign:{}:{}", context_id, authority_id);

        // Derive 32-byte key using HKDF
        let derived_key = self
            .effects
            .hkdf_derive(&context_ikm, &salt, info.as_bytes(), 32)
            .await
            .map_err(|e| AuraError::crypto(format!("Public key derivation failed: {}", e)))?;

        // Extract public key from private key
        let public_key = self
            .effects
            .ed25519_public_key(&derived_key)
            .await
            .map_err(|e| AuraError::crypto(format!("Public key extraction failed: {}", e)))?;

        tracing::debug!(
            "Derived public key for authority {} in context {} for verification",
            authority_id,
            context_id
        );

        Ok(public_key)
    }

    /// Store receipt as journal fact
    async fn store_receipt_fact(&self, receipt: RendezvousReceipt) -> AuraResult<()> {
        // Generate random fact ID using effect system
        let random_bytes = self.effects.random_bytes_32().await;
        let mut fact_id_bytes = [0u8; 32];
        fact_id_bytes.copy_from_slice(&random_bytes);
        let order_id = OrderTime(fact_id_bytes);
        let fact_ts = TimeStamp::OrderClock(order_id.clone());

        let fact = Fact {
            order: order_id,
            content: FactContent::RendezvousReceipt {
                envelope_id: receipt.envelope_id,
                authority_id: receipt.authority_id,
                timestamp: TimeStamp::PhysicalClock(PhysicalTime {
                    ts_ms: receipt.timestamp,
                    uncertainty: None,
                }),
                signature: receipt.signature,
            },
            timestamp: fact_ts,
        };

        // Store fact in journal for audit trail
        // Note: In production this should use journal namespacing once available
        tracing::debug!("Storing receipt fact: {:?}", fact.order);

        // Currently rendezvous receipts are logged for observability
        // Future: Integrate with JournalEffects::append_fact once journal
        // namespacing for rendezvous receipts is implemented
        Ok(())
    }

    /// Clean expired entries from cache
    ///
    /// Implements a simple size-based cache eviction strategy. When the cache
    /// exceeds a threshold, it is cleared to prevent unbounded growth. A more
    /// sophisticated implementation would use LRU or timestamp-based expiration.
    pub async fn clean_cache(&mut self) -> AuraResult<()> {
        const MAX_CACHE_SIZE: usize = 10000;

        // Simple size-based eviction: clear cache if it grows too large
        if self.seen_envelopes.len() > MAX_CACHE_SIZE {
            tracing::info!(
                "Cache size {} exceeded threshold {}, clearing cache",
                self.seen_envelopes.len(),
                MAX_CACHE_SIZE
            );
            self.seen_envelopes.clear();
        }

        Ok(())
    }
}

/// Context-aware transport bridge
pub struct ContextTransportBridge {
    /// Local authority (reserved for future use)
    #[allow(dead_code)]
    local_authority: AuthorityId,
    /// Active transports by context
    transports: HashMap<ContextId, Vec<ContextTransportOffer>>,
    /// Effects system (reserved for future use)
    #[allow(dead_code)]
    effects: Arc<dyn AuraEffects>,
}

impl ContextTransportBridge {
    /// Create a new context transport bridge
    pub fn new(local_authority: AuthorityId, effects: Arc<dyn AuraEffects>) -> Self {
        Self {
            local_authority,
            transports: HashMap::new(),
            effects,
        }
    }

    /// Register transport offers for a context
    pub fn register_transport(
        &mut self,
        context_id: ContextId,
        offers: Vec<ContextTransportOffer>,
    ) {
        self.transports.insert(context_id, offers);
    }

    /// Get available transports for a context
    pub fn get_transports(&self, context_id: &ContextId) -> Option<&Vec<ContextTransportOffer>> {
        self.transports.get(context_id)
    }

    /// Connect to authority in context using transport effects
    pub async fn connect_to_authority(
        &self,
        context_id: ContextId,
        authority: AuthorityId,
        transport_offer: ContextTransportOffer,
    ) -> AuraResult<TransportConnection> {
        tracing::info!(
            "Establishing connection to authority {} in context {} via {} at endpoint {}",
            authority,
            context_id,
            transport_offer.protocol,
            transport_offer.endpoint
        );

        // Validate transport offer
        self.validate_transport_offer(&transport_offer)?;

        // Create connection request with authentication
        let connection_request = self
            .create_connection_request(&context_id, &authority, &transport_offer)
            .await?;

        // Establish connection based on protocol type
        let connection = match transport_offer.protocol.as_str() {
            "quic" => {
                self.establish_quic_connection(&transport_offer, &connection_request)
                    .await?
            }
            "tcp" => {
                self.establish_tcp_connection(&transport_offer, &connection_request)
                    .await?
            }
            "webrtc" => {
                self.establish_webrtc_connection(&transport_offer, &connection_request)
                    .await?
            }
            _ => {
                return Err(AuraError::invalid(format!(
                    "Unsupported transport protocol: {}",
                    transport_offer.protocol
                )));
            }
        };

        // Perform post-connection authentication and setup
        self.authenticate_connection(&connection, &context_id, &authority)
            .await?;

        tracing::info!(
            "Successfully established authenticated connection to authority {} in context {} (connection_id: {})",
            authority,
            context_id,
            connection.connection_id
        );

        Ok(connection)
    }

    /// Validate transport offer before connection attempt
    fn validate_transport_offer(&self, offer: &ContextTransportOffer) -> AuraResult<()> {
        // Check protocol is supported
        const SUPPORTED_PROTOCOLS: &[&str] = &["quic", "tcp", "webrtc"];
        if !SUPPORTED_PROTOCOLS.contains(&offer.protocol.as_str()) {
            return Err(AuraError::invalid(format!(
                "Transport protocol '{}' not supported",
                offer.protocol
            )));
        }

        // Validate endpoint format
        if offer.endpoint.is_empty() {
            return Err(AuraError::invalid("Empty transport endpoint"));
        }

        // Validate transport public key
        if offer.transport_pubkey.is_empty() {
            return Err(AuraError::invalid("Missing transport public key"));
        }

        // Basic endpoint format validation
        if !offer.endpoint.contains(':') {
            return Err(AuraError::invalid("Invalid endpoint format: missing port"));
        }

        tracing::debug!("Transport offer validation passed for {}", offer.protocol);
        Ok(())
    }

    /// Create connection request with proper authentication
    async fn create_connection_request(
        &self,
        context_id: &ContextId,
        authority: &AuthorityId,
        transport_offer: &ContextTransportOffer,
    ) -> AuraResult<ContextConnectionRequest> {
        // Generate random connection nonce
        let nonce = self.effects.random_bytes(32).await;
        let connection_nonce: [u8; 32] = nonce
            .try_into()
            .map_err(|_| AuraError::crypto("Invalid nonce size"))?;

        // Create connection request
        let request = ContextConnectionRequest {
            context_id: *context_id,
            source_authority: self.local_authority,
            target_authority: *authority,
            transport_protocol: transport_offer.protocol.clone(),
            connection_nonce,
            timestamp: aura_core::TimeEffects::current_timestamp(self.effects.as_ref()).await,
            metadata: transport_offer.metadata.clone(),
        };

        tracing::debug!(
            "Created connection request for authority {} in context {} using protocol {}",
            authority,
            context_id,
            transport_offer.protocol
        );

        Ok(request)
    }

    /// Establish QUIC connection
    async fn establish_quic_connection(
        &self,
        offer: &ContextTransportOffer,
        request: &ContextConnectionRequest,
    ) -> AuraResult<TransportConnection> {
        // Parse endpoint
        let endpoint_parts: Vec<&str> = offer.endpoint.split(':').collect();
        if endpoint_parts.len() != 2 {
            return Err(AuraError::invalid("Invalid QUIC endpoint format"));
        }

        let host = endpoint_parts[0];
        let port: u16 = endpoint_parts[1]
            .parse()
            .map_err(|_| AuraError::invalid("Invalid port number"))?;

        // Create QUIC-specific connection parameters
        let connection_id = self.effects.random_bytes(16).await;

        // TODO: Implement actual QUIC connection using proper transport layer
        tracing::debug!("QUIC connection to {}:{} not yet implemented", host, port);
        // Return mock connection for now

        Ok(TransportConnection {
            connection_id: hex::encode(&connection_id),
            protocol: "quic".to_string(),
            endpoint: offer.endpoint.clone(),
            established_at: aura_core::TimeEffects::current_timestamp(self.effects.as_ref()).await,
            peer_authority: request.target_authority,
            context_id: request.context_id,
        })
    }

    /// Establish TCP connection
    async fn establish_tcp_connection(
        &self,
        offer: &ContextTransportOffer,
        request: &ContextConnectionRequest,
    ) -> AuraResult<TransportConnection> {
        // Parse endpoint
        let endpoint_parts: Vec<&str> = offer.endpoint.split(':').collect();
        if endpoint_parts.len() != 2 {
            return Err(AuraError::invalid("Invalid TCP endpoint format"));
        }

        let host = endpoint_parts[0];
        let port: u16 = endpoint_parts[1]
            .parse()
            .map_err(|_| AuraError::invalid("Invalid port number"))?;

        // Create TCP-specific connection parameters
        let connection_id = self.effects.random_bytes(16).await;

        // TODO: Implement actual TCP connection using proper transport layer
        tracing::debug!("TCP connection to {}:{} not yet implemented", host, port);
        // Return mock connection for now

        Ok(TransportConnection {
            connection_id: hex::encode(&connection_id),
            protocol: "tcp".to_string(),
            endpoint: offer.endpoint.clone(),
            established_at: aura_core::TimeEffects::current_timestamp(self.effects.as_ref()).await,
            peer_authority: request.target_authority,
            context_id: request.context_id,
        })
    }

    /// Establish WebRTC connection
    async fn establish_webrtc_connection(
        &self,
        offer: &ContextTransportOffer,
        request: &ContextConnectionRequest,
    ) -> AuraResult<TransportConnection> {
        // WebRTC requires signaling server endpoint
        let signaling_server = offer
            .metadata
            .get("signaling_server")
            .ok_or_else(|| AuraError::invalid("WebRTC requires signaling_server in metadata"))?;

        // Create WebRTC-specific connection parameters
        let connection_id = self.effects.random_bytes(16).await;

        // TODO: Implement actual WebRTC connection using proper transport layer
        tracing::debug!(
            "WebRTC connection via {} not yet implemented",
            signaling_server
        );
        // Return mock connection for now

        Ok(TransportConnection {
            connection_id: hex::encode(&connection_id),
            protocol: "webrtc".to_string(),
            endpoint: signaling_server.clone(),
            established_at: aura_core::TimeEffects::current_timestamp(self.effects.as_ref()).await,
            peer_authority: request.target_authority,
            context_id: request.context_id,
        })
    }

    /// Authenticate connection after establishment
    async fn authenticate_connection(
        &self,
        connection: &TransportConnection,
        context_id: &ContextId,
        authority: &AuthorityId,
    ) -> AuraResult<()> {
        tracing::debug!(
            "Authenticating connection {} to authority {} in context {}",
            connection.connection_id,
            authority,
            context_id
        );

        // Create authentication challenge
        let challenge = self.effects.random_bytes(32).await;

        // Send challenge to peer
        let auth_message = ConnectionAuthChallenge {
            context_id: *context_id,
            authority_id: self.local_authority,
            challenge,
            timestamp: aura_core::TimeEffects::current_timestamp(self.effects.as_ref()).await,
        };

        let _auth_data = bincode::serialize(&auth_message).map_err(|e| {
            AuraError::serialization(format!("Auth message serialization failed: {}", e))
        })?;

        // TODO: Implement actual data transmission on connections
        tracing::debug!(
            "Sending auth challenge on connection {} not yet implemented",
            connection.connection_id
        );

        // TODO: Wait for and verify challenge response
        // This would involve receiving the response, verifying the signature, etc.

        tracing::info!(
            "Connection {} authenticated successfully",
            connection.connection_id
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_context_rendezvous_creation() {
        use aura_agent::{AgentConfig, AuraEffectSystem};

        let authority = AuthorityId::new();
        let effects = Arc::new(AuraEffectSystem::testing(&AgentConfig::default()));

        let coordinator =
            ContextRendezvousCoordinator::new(authority, effects as Arc<dyn AuraEffects>);

        assert_eq!(coordinator.local_authority, authority);
        assert!(coordinator.contexts.is_empty());
    }
}
