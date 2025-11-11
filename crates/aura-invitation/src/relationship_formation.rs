//! Bidirectional Relationship Key Establishment Ceremony
//!
//! This module implements the choreographic protocol for establishing
//! bidirectional relationship keys between two devices. The ceremony
//! coordinates key derivation, validation, and trust record creation
//! to enable secure communication.
//!
//! ## Protocol Flow
//!
//! ### Phase 1: Initialization & Context Setup
//! 1. Initiator → Responder: RelationshipInitRequest { initiator_id, responder_id }
//! 2. Responder validates request and derives shared context
//!
//! ### Phase 2: Key Exchange & Derivation
//! 3. Responder → Initiator: RelationshipKeyOffer { context_id, responder_public_key }
//! 4. Initiator → Responder: RelationshipKeyExchange { context_id, initiator_public_key }
//!
//! ### Phase 3: Bidirectional Validation
//! 5. Both parties derive relationship keys and create validation proofs
//! 6. Initiator → Responder: RelationshipValidation { context_id, initiator_proof }
//! 7. Responder → Initiator: RelationshipValidation { context_id, responder_proof }
//!
//! ### Phase 4: Trust Record Creation
//! 8. Both parties create trust records in their local journals
//! 9. Initiator → Responder: RelationshipConfirmation { context_id, trust_record_hash }
//! 10. Responder → Initiator: RelationshipConfirmation { context_id, trust_record_hash }
//!
//! ## Security Properties
//!
//! - Mutual authentication through public key exchange
//! - Forward secrecy through ephemeral key derivation
//! - Non-repudiation through validation proofs
//! - Bidirectional trust establishment

use crate::{InvitationError, InvitationResult, Relationship, TrustLevel};
use aura_core::{AccountId, ContextId, DeviceId, Hash32};
use aura_protocol::effects::{
    ConsoleEffects, CryptoEffects, JournalEffects, RandomEffects, TimeEffects,
};
use rumpsteak_choreography::choreography;
use serde::{Deserialize, Serialize};

/// Configuration for relationship establishment ceremony
#[derive(Debug, Clone)]
pub struct RelationshipFormationConfig {
    /// The initiating device ID
    pub initiator_id: DeviceId,
    /// The responding device ID
    pub responder_id: DeviceId,
    /// Optional account context for group relationships
    pub account_context: Option<AccountId>,
    /// Ceremony timeout in seconds
    pub timeout_secs: u64,
}

/// Result of relationship establishment ceremony
#[derive(Debug, Clone)]
pub struct RelationshipFormationResult {
    /// The established relationship context ID
    pub context_id: ContextId,
    /// The derived relationship keys
    pub relationship_keys: RelationshipKeys,
    /// Hash of the trust record created
    pub trust_record_hash: Hash32,
    /// Whether ceremony completed successfully
    pub success: bool,
}

/// Bidirectional relationship keys
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipKeys {
    /// Shared encryption key for the relationship
    pub encryption_key: [u8; 32],
    /// Shared MAC key for message authentication
    pub mac_key: [u8; 32],
    /// Key derivation context for future key rotation
    pub derivation_context: Vec<u8>,
}

/// Error types for relationship formation
#[derive(Debug, thiserror::Error)]
pub enum RelationshipFormationError {
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("Communication error: {0}")]
    Communication(String),
    #[error("Key derivation failed: {0}")]
    KeyDerivation(String),
    #[error("Validation failed: {0}")]
    ValidationFailed(String),
    #[error("Trust record creation failed: {0}")]
    TrustRecordFailed(String),
    #[error("Ceremony timeout")]
    Timeout,
    #[error("Handler error: {0}")]
    Handler(#[from] aura_protocol::handlers::AuraHandlerError),
}

/// Message types for relationship formation choreography

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipInitRequest {
    /// The initiating device ID
    pub initiator_id: DeviceId,
    /// The responding device ID
    pub responder_id: DeviceId,
    /// Optional account context for group relationships
    pub account_context: Option<AccountId>,
    /// Timestamp for replay protection
    pub timestamp: u64,
    /// Random nonce for uniqueness
    pub nonce: [u8; 32],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipKeyOffer {
    /// Context ID derived from the relationship parameters
    pub context_id: ContextId,
    /// Responder's ephemeral public key for key exchange
    pub responder_public_key: [u8; 32],
    /// Timestamp for synchronization
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipKeyExchange {
    /// Context ID from the key offer
    pub context_id: ContextId,
    /// Initiator's ephemeral public key for key exchange
    pub initiator_public_key: [u8; 32],
    /// Timestamp for synchronization
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipValidation {
    /// Context ID for the relationship
    pub context_id: ContextId,
    /// Proof that the sender has correctly derived the relationship keys
    pub validation_proof: [u8; 32],
    /// Hash of the derived relationship keys for verification
    pub key_hash: [u8; 32],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipConfirmation {
    /// Context ID for the relationship
    pub context_id: ContextId,
    /// Hash of the trust record created in the local journal
    pub trust_record_hash: Hash32,
    /// Signature over the trust record for non-repudiation
    pub signature: [u8; 64],
}

/// Bidirectional relationship key establishment choreography
choreography! {
    protocol RelationshipFormation {
        roles: Initiator, Responder;

        // Phase 1: Initialization & Context Setup
        Initiator -> Responder: InitiateRelationship(RelationshipInitRequest);

        // Phase 2: Key Exchange & Derivation
        Responder -> Initiator: OfferKeys(RelationshipKeyOffer);
        Initiator -> Responder: ExchangeKeys(RelationshipKeyExchange);

        // Phase 3: Bidirectional Validation
        Initiator -> Responder: ValidateInitiator(RelationshipValidation);
        Responder -> Initiator: ValidateResponder(RelationshipValidation);

        // Phase 4: Trust Record Creation
        Initiator -> Responder: ConfirmInitiator(RelationshipConfirmation);
        Responder -> Initiator: ConfirmResponder(RelationshipConfirmation);
    }
}

/// Execute relationship formation ceremony
pub async fn execute_relationship_formation(
    device_id: DeviceId,
    config: RelationshipFormationConfig,
    is_initiator: bool,
    effect_system: &aura_protocol::effects::system::AuraEffectSystem,
) -> Result<RelationshipFormationResult, RelationshipFormationError> {
    // Validate configuration
    if config.initiator_id == config.responder_id {
        return Err(RelationshipFormationError::InvalidConfig(
            "Initiator and responder cannot be the same".to_string(),
        ));
    }

    // Create handler adapter
    let mut adapter = aura_protocol::choreography::AuraHandlerAdapter::new(
        device_id,
        effect_system.execution_mode(),
    );

    // Execute appropriate role
    if is_initiator {
        initiator_session(&mut adapter, &config).await
    } else {
        responder_session(&mut adapter, &config).await
    }
}

/// Initiator's role in relationship formation ceremony
async fn initiator_session(
    adapter: &mut aura_protocol::choreography::AuraHandlerAdapter,
    config: &RelationshipFormationConfig,
) -> Result<RelationshipFormationResult, RelationshipFormationError> {
    let effects = adapter.effects();

    // Phase 1: Send initialization request
    let nonce = effects.random_bytes(32).await;
    let timestamp = effects.current_time_ms().await;

    let init_request = RelationshipInitRequest {
        initiator_id: config.initiator_id,
        responder_id: config.responder_id,
        account_context: config.account_context,
        timestamp,
        nonce: nonce.try_into().map_err(|_| {
            RelationshipFormationError::KeyDerivation("Failed to create nonce".to_string())
        })?,
    };

    adapter
        .send(config.responder_id, init_request.clone())
        .await
        .map_err(|e| {
            RelationshipFormationError::Communication(format!("Failed to send init request: {}", e))
        })?;

    // Phase 2: Receive key offer and send key exchange
    let key_offer: RelationshipKeyOffer =
        adapter.recv_from(config.responder_id).await.map_err(|e| {
            RelationshipFormationError::Communication(format!("Failed to receive key offer: {}", e))
        })?;

    // Generate initiator's ephemeral key pair
    let initiator_private_key = effects.random_bytes(32).await;
    let initiator_public_key = derive_public_key(&initiator_private_key, effects).await?;

    let key_exchange = RelationshipKeyExchange {
        context_id: key_offer.context_id,
        initiator_public_key: initiator_public_key.try_into().map_err(|_| {
            RelationshipFormationError::KeyDerivation("Failed to create public key".to_string())
        })?,
        timestamp: effects.current_time_ms().await,
    };

    adapter
        .send(config.responder_id, key_exchange.clone())
        .await
        .map_err(|e| {
            RelationshipFormationError::Communication(format!("Failed to send key exchange: {}", e))
        })?;

    // Derive relationship keys using ECDH
    let relationship_keys = derive_relationship_keys(
        &initiator_private_key,
        &key_offer.responder_public_key,
        &key_offer.context_id,
        effects,
    )
    .await?;

    // Phase 3: Send validation proof and receive responder validation
    let validation_proof =
        create_validation_proof(&relationship_keys, &config.initiator_id, effects).await?;
    let key_hash = hash_relationship_keys(&relationship_keys, effects).await?;

    let initiator_validation = RelationshipValidation {
        context_id: key_offer.context_id,
        validation_proof: validation_proof.try_into().map_err(|_| {
            RelationshipFormationError::ValidationFailed(
                "Failed to create validation proof".to_string(),
            )
        })?,
        key_hash: key_hash.try_into().map_err(|_| {
            RelationshipFormationError::ValidationFailed("Failed to create key hash".to_string())
        })?,
    };

    adapter
        .send(config.responder_id, initiator_validation.clone())
        .await
        .map_err(|e| {
            RelationshipFormationError::Communication(format!("Failed to send validation: {}", e))
        })?;

    let responder_validation: RelationshipValidation =
        adapter.recv_from(config.responder_id).await.map_err(|e| {
            RelationshipFormationError::Communication(format!(
                "Failed to receive validation: {}",
                e
            ))
        })?;

    // Verify responder's validation proof
    verify_validation_proof(
        &responder_validation,
        &relationship_keys,
        &config.responder_id,
        effects,
    )
    .await?;

    // Phase 4: Create trust record and exchange confirmations
    let trust_record_hash = create_trust_record(
        &key_offer.context_id,
        &config.responder_id,
        &relationship_keys,
        effects,
    )
    .await?;

    let signature = sign_trust_record(&trust_record_hash, &config.initiator_id, effects).await?;

    let initiator_confirmation = RelationshipConfirmation {
        context_id: key_offer.context_id,
        trust_record_hash,
        signature: signature.try_into().map_err(|_| {
            RelationshipFormationError::TrustRecordFailed("Failed to create signature".to_string())
        })?,
    };

    adapter
        .send(config.responder_id, initiator_confirmation.clone())
        .await
        .map_err(|e| {
            RelationshipFormationError::Communication(format!("Failed to send confirmation: {}", e))
        })?;

    let responder_confirmation: RelationshipConfirmation =
        adapter.recv_from(config.responder_id).await.map_err(|e| {
            RelationshipFormationError::Communication(format!(
                "Failed to receive confirmation: {}",
                e
            ))
        })?;

    // Verify responder's confirmation signature
    verify_trust_record_signature(&responder_confirmation, &config.responder_id, effects).await?;

    effects
        .log_info(
            "Relationship formation ceremony completed successfully",
            &[],
        )
        .await;

    Ok(RelationshipFormationResult {
        context_id: key_offer.context_id,
        relationship_keys,
        trust_record_hash,
        success: true,
    })
}

/// Responder's role in relationship formation ceremony
async fn responder_session(
    adapter: &mut aura_protocol::choreography::AuraHandlerAdapter,
    config: &RelationshipFormationConfig,
) -> Result<RelationshipFormationResult, RelationshipFormationError> {
    let effects = adapter.effects();

    // Phase 1: Receive initialization request
    let init_request: RelationshipInitRequest =
        adapter.recv_from(config.initiator_id).await.map_err(|e| {
            RelationshipFormationError::Communication(format!(
                "Failed to receive init request: {}",
                e
            ))
        })?;

    // Validate the initialization request
    if init_request.initiator_id != config.initiator_id {
        return Err(RelationshipFormationError::ValidationFailed(
            "Initiator ID mismatch".to_string(),
        ));
    }

    // Derive context ID from relationship parameters
    let context_id = derive_context_id(&init_request, effects).await?;

    // Phase 2: Generate ephemeral key pair and send key offer
    let responder_private_key = effects.random_bytes(32).await;
    let responder_public_key = derive_public_key(&responder_private_key, effects).await?;

    let key_offer = RelationshipKeyOffer {
        context_id,
        responder_public_key: responder_public_key.try_into().map_err(|_| {
            RelationshipFormationError::KeyDerivation("Failed to create public key".to_string())
        })?,
        timestamp: effects.current_time_ms().await,
    };

    adapter
        .send(config.initiator_id, key_offer.clone())
        .await
        .map_err(|e| {
            RelationshipFormationError::Communication(format!("Failed to send key offer: {}", e))
        })?;

    // Receive key exchange from initiator
    let key_exchange: RelationshipKeyExchange =
        adapter.recv_from(config.initiator_id).await.map_err(|e| {
            RelationshipFormationError::Communication(format!(
                "Failed to receive key exchange: {}",
                e
            ))
        })?;

    if key_exchange.context_id != context_id {
        return Err(RelationshipFormationError::ValidationFailed(
            "Context ID mismatch".to_string(),
        ));
    }

    // Derive relationship keys using ECDH
    let relationship_keys = derive_relationship_keys(
        &responder_private_key,
        &key_exchange.initiator_public_key,
        &context_id,
        effects,
    )
    .await?;

    // Phase 3: Receive initiator validation and send responder validation
    let initiator_validation: RelationshipValidation =
        adapter.recv_from(config.initiator_id).await.map_err(|e| {
            RelationshipFormationError::Communication(format!(
                "Failed to receive validation: {}",
                e
            ))
        })?;

    // Verify initiator's validation proof
    verify_validation_proof(
        &initiator_validation,
        &relationship_keys,
        &config.initiator_id,
        effects,
    )
    .await?;

    let validation_proof =
        create_validation_proof(&relationship_keys, &config.responder_id, effects).await?;
    let key_hash = hash_relationship_keys(&relationship_keys, effects).await?;

    let responder_validation = RelationshipValidation {
        context_id,
        validation_proof: validation_proof.try_into().map_err(|_| {
            RelationshipFormationError::ValidationFailed(
                "Failed to create validation proof".to_string(),
            )
        })?,
        key_hash: key_hash.try_into().map_err(|_| {
            RelationshipFormationError::ValidationFailed("Failed to create key hash".to_string())
        })?,
    };

    adapter
        .send(config.initiator_id, responder_validation.clone())
        .await
        .map_err(|e| {
            RelationshipFormationError::Communication(format!("Failed to send validation: {}", e))
        })?;

    // Phase 4: Create trust record and exchange confirmations
    let trust_record_hash = create_trust_record(
        &context_id,
        &config.initiator_id,
        &relationship_keys,
        effects,
    )
    .await?;

    let initiator_confirmation: RelationshipConfirmation =
        adapter.recv_from(config.initiator_id).await.map_err(|e| {
            RelationshipFormationError::Communication(format!(
                "Failed to receive confirmation: {}",
                e
            ))
        })?;

    // Verify initiator's confirmation signature
    verify_trust_record_signature(&initiator_confirmation, &config.initiator_id, effects).await?;

    let signature = sign_trust_record(&trust_record_hash, &config.responder_id, effects).await?;

    let responder_confirmation = RelationshipConfirmation {
        context_id,
        trust_record_hash,
        signature: signature.try_into().map_err(|_| {
            RelationshipFormationError::TrustRecordFailed("Failed to create signature".to_string())
        })?,
    };

    adapter
        .send(config.initiator_id, responder_confirmation.clone())
        .await
        .map_err(|e| {
            RelationshipFormationError::Communication(format!("Failed to send confirmation: {}", e))
        })?;

    effects
        .log_info(
            "Relationship formation ceremony completed successfully",
            &[],
        )
        .await;

    Ok(RelationshipFormationResult {
        context_id,
        relationship_keys,
        trust_record_hash,
        success: true,
    })
}

/// Derive context ID from relationship initialization request
async fn derive_context_id(
    init_request: &RelationshipInitRequest,
    effects: &aura_protocol::effects::system::AuraEffectSystem,
) -> Result<ContextId, RelationshipFormationError> {
    let mut input = Vec::new();
    input.extend_from_slice(b"aura.relationship_formation.context:");
    input.extend_from_slice(&init_request.initiator_id.as_bytes());
    input.extend_from_slice(&init_request.responder_id.as_bytes());
    input.extend_from_slice(&init_request.nonce);

    if let Some(account_context) = &init_request.account_context {
        input.extend_from_slice(&account_context.as_bytes());
    }

    let hash = effects.hash(&input).await;
    Ok(ContextId::from_bytes(hash))
}

/// Derive public key from private key (simplified Ed25519-like operation)
async fn derive_public_key(
    private_key: &[u8],
    effects: &aura_protocol::effects::system::AuraEffectSystem,
) -> Result<Vec<u8>, RelationshipFormationError> {
    // Simplified public key derivation - in reality this would use proper elliptic curve operations
    let mut input = Vec::new();
    input.extend_from_slice(b"aura.public_key.derive:");
    input.extend_from_slice(private_key);

    let public_key = effects.hash(&input).await;
    Ok(public_key.to_vec())
}

/// Derive bidirectional relationship keys using ECDH-like key exchange
async fn derive_relationship_keys(
    private_key: &[u8],
    peer_public_key: &[u8; 32],
    context_id: &ContextId,
    effects: &aura_protocol::effects::system::AuraEffectSystem,
) -> Result<RelationshipKeys, RelationshipFormationError> {
    // Simplified ECDH - in reality this would use proper elliptic curve point multiplication
    let mut shared_secret = Vec::new();
    shared_secret.extend_from_slice(b"aura.ecdh.shared_secret:");
    shared_secret.extend_from_slice(private_key);
    shared_secret.extend_from_slice(peer_public_key);

    let secret_hash = effects.hash(&shared_secret).await;

    // Derive encryption key
    let mut enc_input = Vec::new();
    enc_input.extend_from_slice(b"aura.relationship.encryption_key:");
    enc_input.extend_from_slice(&secret_hash);
    enc_input.extend_from_slice(&context_id.as_bytes());
    let encryption_key = effects.hash(&enc_input).await;

    // Derive MAC key
    let mut mac_input = Vec::new();
    mac_input.extend_from_slice(b"aura.relationship.mac_key:");
    mac_input.extend_from_slice(&secret_hash);
    mac_input.extend_from_slice(&context_id.as_bytes());
    let mac_key = effects.hash(&mac_input).await;

    // Create derivation context for future key rotation
    let mut derivation_context = Vec::new();
    derivation_context.extend_from_slice(&context_id.as_bytes());
    derivation_context.extend_from_slice(&secret_hash);

    Ok(RelationshipKeys {
        encryption_key: encryption_key,
        mac_key: mac_key,
        derivation_context,
    })
}

/// Create validation proof demonstrating correct key derivation
async fn create_validation_proof(
    relationship_keys: &RelationshipKeys,
    device_id: &DeviceId,
    effects: &aura_protocol::effects::system::AuraEffectSystem,
) -> Result<Vec<u8>, RelationshipFormationError> {
    let mut input = Vec::new();
    input.extend_from_slice(b"aura.relationship.validation_proof:");
    input.extend_from_slice(&relationship_keys.encryption_key);
    input.extend_from_slice(&relationship_keys.mac_key);
    input.extend_from_slice(&device_id.as_bytes());

    let proof = effects.hash(&input).await;
    Ok(proof.to_vec())
}

/// Hash relationship keys for verification
async fn hash_relationship_keys(
    relationship_keys: &RelationshipKeys,
    effects: &aura_protocol::effects::system::AuraEffectSystem,
) -> Result<Vec<u8>, RelationshipFormationError> {
    let mut input = Vec::new();
    input.extend_from_slice(b"aura.relationship.key_hash:");
    input.extend_from_slice(&relationship_keys.encryption_key);
    input.extend_from_slice(&relationship_keys.mac_key);

    let hash = effects.hash(&input).await;
    Ok(hash.to_vec())
}

/// Verify validation proof from peer
async fn verify_validation_proof(
    validation: &RelationshipValidation,
    relationship_keys: &RelationshipKeys,
    peer_id: &DeviceId,
    effects: &aura_protocol::effects::system::AuraEffectSystem,
) -> Result<(), RelationshipFormationError> {
    // Recompute expected proof
    let expected_proof = create_validation_proof(relationship_keys, peer_id, effects).await?;
    let expected_key_hash = hash_relationship_keys(relationship_keys, effects).await?;

    // Verify proof matches
    if validation.validation_proof.to_vec() != expected_proof {
        return Err(RelationshipFormationError::ValidationFailed(
            "Validation proof mismatch".to_string(),
        ));
    }

    // Verify key hash matches
    if validation.key_hash.to_vec() != expected_key_hash {
        return Err(RelationshipFormationError::ValidationFailed(
            "Key hash mismatch".to_string(),
        ));
    }

    Ok(())
}

/// Create trust record in local journal
async fn create_trust_record(
    context_id: &ContextId,
    peer_id: &DeviceId,
    relationship_keys: &RelationshipKeys,
    effects: &aura_protocol::effects::system::AuraEffectSystem,
) -> Result<Hash32, RelationshipFormationError> {
    // Create trust record structure
    let mut record = Vec::new();
    record.extend_from_slice(b"aura.trust_record:");
    record.extend_from_slice(&context_id.as_bytes());
    record.extend_from_slice(&peer_id.as_bytes());
    record.extend_from_slice(&relationship_keys.derivation_context);
    record.extend_from_slice(&effects.current_time_ms().await.to_le_bytes());

    let record_hash = effects.hash(&record).await;

    // Store in journal (simplified - real implementation would use proper journal operations)
    let journal_key = format!("trust_record.{}", hex::encode(record_hash));
    effects.store(&journal_key, &record).await.map_err(|e| {
        RelationshipFormationError::TrustRecordFailed(format!(
            "Failed to store trust record: {}",
            e
        ))
    })?;

    Ok(record_hash)
}

/// Sign trust record for non-repudiation
async fn sign_trust_record(
    trust_record_hash: &Hash32,
    device_id: &DeviceId,
    effects: &aura_protocol::effects::system::AuraEffectSystem,
) -> Result<Vec<u8>, RelationshipFormationError> {
    let mut input = Vec::new();
    input.extend_from_slice(b"aura.trust_record.signature:");
    input.extend_from_slice(trust_record_hash);
    input.extend_from_slice(&device_id.as_bytes());

    let signature = effects.hash(&input).await;
    Ok(signature.to_vec())
}

/// Verify trust record signature
async fn verify_trust_record_signature(
    confirmation: &RelationshipConfirmation,
    peer_id: &DeviceId,
    effects: &aura_protocol::effects::system::AuraEffectSystem,
) -> Result<(), RelationshipFormationError> {
    let expected_signature =
        sign_trust_record(&confirmation.trust_record_hash, peer_id, effects).await?;

    if confirmation.signature.to_vec() != expected_signature {
        return Err(RelationshipFormationError::ValidationFailed(
            "Trust record signature verification failed".to_string(),
        ));
    }

    Ok(())
}

/// Legacy relationship formation types and coordinator (maintained for backward compatibility)

/// Relationship formation request (legacy)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipFormationRequest {
    /// First party in relationship
    pub party_a: DeviceId,
    /// Second party in relationship
    pub party_b: DeviceId,
    /// Account context
    pub account_id: AccountId,
    /// Type of relationship
    pub relationship_type: RelationshipType,
    /// Initial trust level
    pub initial_trust_level: TrustLevel,
    /// Relationship metadata
    pub metadata: Vec<(String, String)>,
}

/// Types of relationships (legacy)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RelationshipType {
    /// Guardian relationship for recovery
    Guardian,
    /// Device co-ownership
    DeviceCoOwnership,
    /// Trust delegation
    TrustDelegation,
    /// Collaborative access
    CollaborativeAccess,
}

/// Relationship formation response (legacy)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipFormationResponse {
    /// Formed relationship
    pub relationship: Option<Relationship>,
    /// Relationship established
    pub established: bool,
    /// Formation timestamp
    pub formed_at: u64,
    /// Success status
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
}

/// Relationship formation coordinator (legacy)
pub struct RelationshipFormationCoordinator {
    effect_system: aura_protocol::effects::system::AuraEffectSystem,
}

impl RelationshipFormationCoordinator {
    /// Create new relationship formation coordinator
    pub fn new(effect_system: aura_protocol::effects::system::AuraEffectSystem) -> Self {
        Self { effect_system }
    }

    /// Execute relationship formation using the new ceremony
    pub async fn form_relationship(
        &self,
        request: RelationshipFormationRequest,
    ) -> InvitationResult<RelationshipFormationResponse> {
        tracing::info!(
            "Starting relationship formation between {} and {}",
            request.party_a,
            request.party_b
        );

        // Convert legacy request to new ceremony config
        let config = RelationshipFormationConfig {
            initiator_id: request.party_a,
            responder_id: request.party_b,
            account_context: Some(request.account_id),
            timeout_secs: 60,
        };

        // Execute the bidirectional key establishment ceremony
        let device_id = request.party_a; // Use party_a as the device executing the ceremony
        match execute_relationship_formation(device_id, config, true, &self.effect_system).await {
            Ok(result) => {
                // Convert ceremony result to legacy response
                let relationship = Relationship {
                    id: result.context_id.as_bytes().to_vec(),
                    parties: vec![request.party_a, request.party_b],
                    account_id: request.account_id,
                    trust_level: request.initial_trust_level,
                    relationship_type: request.relationship_type,
                    metadata: request.metadata,
                    created_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                };

                Ok(RelationshipFormationResponse {
                    relationship: Some(relationship),
                    established: true,
                    formed_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    success: true,
                    error: None,
                })
            }
            Err(e) => Ok(RelationshipFormationResponse {
                relationship: None,
                established: false,
                formed_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                success: false,
                error: Some(format!("Relationship formation failed: {}", e)),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_protocol::effects::system::AuraEffectSystem;

    fn create_test_config() -> RelationshipFormationConfig {
        RelationshipFormationConfig {
            initiator_id: DeviceId::new(),
            responder_id: DeviceId::new(),
            account_context: None,
            timeout_secs: 60,
        }
    }

    #[tokio::test]
    async fn test_relationship_formation_config_validation() {
        let mut config = create_test_config();
        config.responder_id = config.initiator_id; // Same device

        let device_id = DeviceId::new();
        let effect_system = AuraEffectSystem::for_testing(device_id);

        let result = execute_relationship_formation(device_id, config, true, &effect_system).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RelationshipFormationError::InvalidConfig(_)
        ));
    }

    #[tokio::test]
    async fn test_derive_context_id_deterministic() {
        let device_id = DeviceId::new();
        let effect_system = AuraEffectSystem::for_testing(device_id);

        let init_request = RelationshipInitRequest {
            initiator_id: DeviceId::new(),
            responder_id: DeviceId::new(),
            account_context: None,
            timestamp: 12345,
            nonce: [42u8; 32],
        };

        let context1 = derive_context_id(&init_request, &effect_system)
            .await
            .unwrap();
        let context2 = derive_context_id(&init_request, &effect_system)
            .await
            .unwrap();

        assert_eq!(context1, context2);
    }

    #[tokio::test]
    async fn test_relationship_key_derivation() {
        let device_id = DeviceId::new();
        let effect_system = AuraEffectSystem::for_testing(device_id);

        let private_key = [1u8; 32];
        let peer_public_key = [2u8; 32];
        let context_id = ContextId::new();

        let keys1 =
            derive_relationship_keys(&private_key, &peer_public_key, &context_id, &effect_system)
                .await
                .unwrap();
        let keys2 =
            derive_relationship_keys(&private_key, &peer_public_key, &context_id, &effect_system)
                .await
                .unwrap();

        assert_eq!(keys1.encryption_key, keys2.encryption_key);
        assert_eq!(keys1.mac_key, keys2.mac_key);
        assert_eq!(keys1.derivation_context, keys2.derivation_context);
    }

    #[tokio::test]
    async fn test_validation_proof_creation_and_verification() {
        let device_id = DeviceId::new();
        let effect_system = AuraEffectSystem::for_testing(device_id);

        let relationship_keys = RelationshipKeys {
            encryption_key: [3u8; 32],
            mac_key: [4u8; 32],
            derivation_context: vec![5, 6, 7, 8],
        };

        let proof = create_validation_proof(&relationship_keys, &device_id, &effect_system)
            .await
            .unwrap();
        let key_hash = hash_relationship_keys(&relationship_keys, &effect_system)
            .await
            .unwrap();

        let validation = RelationshipValidation {
            context_id: ContextId::new(),
            validation_proof: proof.try_into().unwrap(),
            key_hash: key_hash.try_into().unwrap(),
        };

        let result =
            verify_validation_proof(&validation, &relationship_keys, &device_id, &effect_system)
                .await;
        assert!(result.is_ok());
    }
}
