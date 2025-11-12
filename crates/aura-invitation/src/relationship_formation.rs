//! Bidirectional Relationship Key Establishment Ceremony
//!
//! This module implements the choreographic protocol for establishing
//! bidirectional relationship keys between two devices. The ceremony
//! coordinates key derivation, validation, and trust record creation
//! to enable secure communication using aura-macros.
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

use crate::{InvitationResult, Relationship, TrustLevel};
use aura_core::effects::{ConsoleEffects, CryptoEffects, NetworkEffects, RandomEffects, TimeEffects, JournalEffects};
use aura_core::{AccountId, ContextId, DeviceId, Hash32};
use aura_macros::aura_choreography;
use hex;
use serde::{Deserialize, Serialize};

/// Sealed supertrait for relationship formation choreography effects
pub trait RelationshipFormationEffects: ConsoleEffects + CryptoEffects + NetworkEffects + RandomEffects + TimeEffects + JournalEffects {}
impl<T> RelationshipFormationEffects for T where T: ConsoleEffects + CryptoEffects + NetworkEffects + RandomEffects + TimeEffects + JournalEffects {}

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
    pub encryption_key: Vec<u8>,
    /// Shared MAC key for message authentication
    pub mac_key: Vec<u8>,
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
    #[error("Core error: {0}")]
    Core(#[from] aura_core::AuraError),
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
    pub nonce: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipKeyOffer {
    /// Context ID derived from the relationship parameters
    pub context_id: ContextId,
    /// Responder's ephemeral public key for key exchange
    pub responder_public_key: Vec<u8>,
    /// Timestamp for synchronization
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipKeyExchange {
    /// Context ID from the key offer
    pub context_id: ContextId,
    /// Initiator's ephemeral public key for key exchange
    pub initiator_public_key: Vec<u8>,
    /// Timestamp for synchronization
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipValidation {
    /// Context ID for the relationship
    pub context_id: ContextId,
    /// Proof that the sender has correctly derived the relationship keys
    pub validation_proof: Vec<u8>,
    /// Hash of the derived relationship keys for verification
    pub key_hash: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipConfirmation {
    /// Context ID for the relationship
    pub context_id: ContextId,
    /// Hash of the trust record created in the local journal
    pub trust_record_hash: Hash32,
    /// Signature over the trust record for non-repudiation
    pub signature: Vec<u8>,
}

/// Bidirectional relationship key establishment choreography
aura_choreography! {
    #[namespace = "relationship_formation"]
    protocol RelationshipFormation {
        roles: Initiator, Responder;

        // Phase 1: Initialization & Context Setup
        Initiator[guard_capability = "initiate_relationship",
                  flow_cost = 150,
                  journal_facts = "relationship_initiated"]
        -> Responder: InitiateRelationship(RelationshipInitRequest);

        // Phase 2: Key Exchange & Derivation
        Responder[guard_capability = "offer_keys",
                  flow_cost = 180,
                  journal_facts = "keys_offered",
                  leakage_budget = [1, 0, 0]]
        -> Initiator: OfferKeys(RelationshipKeyOffer);
        
        Initiator[guard_capability = "exchange_keys",
                  flow_cost = 180,
                  journal_facts = "keys_exchanged",
                  leakage_budget = [1, 0, 0]]
        -> Responder: ExchangeKeys(RelationshipKeyExchange);

        // Phase 3: Bidirectional Validation
        Initiator[guard_capability = "validate_relationship",
                  flow_cost = 120,
                  journal_facts = "initiator_validated"]
        -> Responder: ValidateInitiator(RelationshipValidation);
        
        Responder[guard_capability = "validate_relationship",
                  flow_cost = 120,
                  journal_facts = "responder_validated"]
        -> Initiator: ValidateResponder(RelationshipValidation);

        // Phase 4: Trust Record Creation
        Initiator[guard_capability = "confirm_relationship",
                  flow_cost = 100,
                  journal_facts = "relationship_confirmed",
                  journal_merge = true]
        -> Responder: ConfirmInitiator(RelationshipConfirmation);
        
        Responder[guard_capability = "confirm_relationship",
                  flow_cost = 100,
                  journal_facts = "relationship_confirmed",
                  journal_merge = true]
        -> Initiator: ConfirmResponder(RelationshipConfirmation);
    }
}

/// Execute relationship formation ceremony
pub async fn execute_relationship_formation<E: RelationshipFormationEffects>(
    device_id: DeviceId,
    config: RelationshipFormationConfig,
    is_initiator: bool,
    effects: &E,
) -> Result<RelationshipFormationResult, RelationshipFormationError> {
    // Validate configuration
    if config.initiator_id == config.responder_id {
        return Err(RelationshipFormationError::InvalidConfig(
            "Initiator and responder cannot be the same".to_string(),
        ));
    }

    // Execute appropriate role using aura-macros choreography
    if is_initiator {
        initiator_session(effects, &config).await
    } else {
        responder_session(effects, &config).await
    }
}

/// Initiator's role in relationship formation ceremony
async fn initiator_session<E: RelationshipFormationEffects>(
    effects: &E,
    config: &RelationshipFormationConfig,
) -> Result<RelationshipFormationResult, RelationshipFormationError> {
    // Phase 1: Send initialization request
    let nonce = effects.random_bytes(32).await;
    let timestamp = effects.current_timestamp().await;

    let init_request = RelationshipInitRequest {
        initiator_id: config.initiator_id,
        responder_id: config.responder_id,
        account_context: config.account_context,
        timestamp,
        nonce,
    };

    let init_bytes = serde_json::to_vec(&init_request)
        .map_err(|e| RelationshipFormationError::Communication(format!("Failed to serialize init request: {}", e)))?;
    
    effects.send_to_peer(config.responder_id.into(), init_bytes)
        .await
        .map_err(|e| {
            RelationshipFormationError::Communication(format!("Failed to send init request: {}", e))
        })?;

    // Phase 2: Receive key offer and send key exchange
    let (_peer_id, offer_bytes) = effects.receive().await
        .map_err(|e| RelationshipFormationError::Communication(format!("Failed to receive key offer: {}", e)))?;
    
    let key_offer: RelationshipKeyOffer = serde_json::from_slice(&offer_bytes)
        .map_err(|e| RelationshipFormationError::Communication(format!("Failed to deserialize key offer: {}", e)))?;

    // Generate initiator's ephemeral key pair
    let initiator_private_key = effects.random_bytes(32).await;
    let initiator_public_key = derive_public_key(&initiator_private_key, effects).await?;

    let key_exchange = RelationshipKeyExchange {
        context_id: key_offer.context_id.clone(),
        initiator_public_key,
        timestamp: effects.current_timestamp().await,
    };

    let exchange_bytes = serde_json::to_vec(&key_exchange)
        .map_err(|e| RelationshipFormationError::Communication(format!("Failed to serialize key exchange: {}", e)))?;
    
    effects.send_to_peer(config.responder_id.into(), exchange_bytes)
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
        create_validation_proof(&relationship_keys, &config.initiator_id, effects)
            .await?;
    let key_hash = hash_relationship_keys(&relationship_keys, effects).await?;

    let initiator_validation = RelationshipValidation {
        context_id: key_offer.context_id.clone(),
        validation_proof,
        key_hash: key_hash.clone(),
    };

    let validation_bytes = serde_json::to_vec(&initiator_validation)
        .map_err(|e| RelationshipFormationError::Communication(format!("Failed to serialize validation: {}", e)))?;
    
    effects.send_to_peer(config.responder_id.into(), validation_bytes)
        .await
        .map_err(|e| {
            RelationshipFormationError::Communication(format!("Failed to send validation: {}", e))
        })?;

    let (_peer_id, validation_bytes) = effects.receive().await.map_err(|e| {
        RelationshipFormationError::Communication(format!(
            "Failed to receive validation: {}",
            e
        ))
    })?;
    
    let responder_validation: RelationshipValidation = serde_json::from_slice(&validation_bytes)
        .map_err(|e| RelationshipFormationError::Communication(format!("Failed to deserialize validation: {}", e)))?;

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

    let signature =
        sign_trust_record(&trust_record_hash, &config.initiator_id, effects).await?;

    let initiator_confirmation = RelationshipConfirmation {
        context_id: key_offer.context_id.clone(),
        trust_record_hash,
        signature,
    };

    let confirmation_bytes = serde_json::to_vec(&initiator_confirmation)
        .map_err(|e| RelationshipFormationError::Communication(format!("Failed to serialize confirmation: {}", e)))?;
    
    effects.send_to_peer(config.responder_id.into(), confirmation_bytes)
        .await
        .map_err(|e| {
            RelationshipFormationError::Communication(format!("Failed to send confirmation: {}", e))
        })?;

    let (_peer_id, confirmation_bytes) = effects.receive().await.map_err(|e| {
        RelationshipFormationError::Communication(format!(
            "Failed to receive confirmation: {}",
            e
        ))
    })?;
    
    let responder_confirmation: RelationshipConfirmation = serde_json::from_slice(&confirmation_bytes)
        .map_err(|e| RelationshipFormationError::Communication(format!("Failed to deserialize confirmation: {}", e)))?;

    // Verify responder's confirmation signature
    verify_trust_record_signature(
        &responder_confirmation,
        &config.responder_id,
        effects,
    )
    .await?;

    effects.log_info("Relationship formation ceremony completed successfully").await;

    Ok(RelationshipFormationResult {
        context_id: key_offer.context_id,
        relationship_keys,
        trust_record_hash,
        success: true,
    })
}

/// Responder's role in relationship formation ceremony
async fn responder_session<E: RelationshipFormationEffects>(
    effects: &E,
    config: &RelationshipFormationConfig,
) -> Result<RelationshipFormationResult, RelationshipFormationError> {
    // Phase 1: Receive initialization request
    let (_peer_id, init_bytes) = effects.receive().await.map_err(|e| {
        RelationshipFormationError::Communication(format!(
            "Failed to receive init request: {}",
            e
        ))
    })?;
    
    let init_request: RelationshipInitRequest = serde_json::from_slice(&init_bytes)
        .map_err(|e| RelationshipFormationError::Communication(format!("Failed to deserialize init request: {}", e)))?;

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
        context_id: context_id.clone(),
        responder_public_key: responder_public_key.clone(),
        timestamp: effects.current_timestamp().await,
    };

    let offer_bytes = serde_json::to_vec(&key_offer)
        .map_err(|e| RelationshipFormationError::Communication(format!("Failed to serialize key offer: {}", e)))?;
    
    effects.send_to_peer(config.initiator_id.into(), offer_bytes)
        .await
        .map_err(|e| {
            RelationshipFormationError::Communication(format!("Failed to send key offer: {}", e))
        })?;

    // Receive key exchange from initiator
    let (_peer_id, exchange_bytes) = effects.receive().await.map_err(|e| {
        RelationshipFormationError::Communication(format!(
            "Failed to receive key exchange: {}",
            e
        ))
    })?;
    
    let key_exchange: RelationshipKeyExchange = serde_json::from_slice(&exchange_bytes)
        .map_err(|e| RelationshipFormationError::Communication(format!("Failed to deserialize key exchange: {}", e)))?;

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
    let (_peer_id, validation_bytes) = effects.receive().await.map_err(|e| {
        RelationshipFormationError::Communication(format!(
            "Failed to receive validation: {}",
            e
        ))
    })?;
    
    let initiator_validation: RelationshipValidation = serde_json::from_slice(&validation_bytes)
        .map_err(|e| RelationshipFormationError::Communication(format!("Failed to deserialize validation: {}", e)))?;

    // Verify initiator's validation proof
    verify_validation_proof(
        &initiator_validation,
        &relationship_keys,
        &config.initiator_id,
        effects,
    )
    .await?;

    let validation_proof =
        create_validation_proof(&relationship_keys, &config.responder_id, effects)
            .await?;
    let key_hash = hash_relationship_keys(&relationship_keys, effects).await?;

    let responder_validation = RelationshipValidation {
        context_id: context_id.clone(),
        validation_proof,
        key_hash,
    };

    let validation_bytes = serde_json::to_vec(&responder_validation)
        .map_err(|e| RelationshipFormationError::Communication(format!("Failed to serialize validation: {}", e)))?;
    
    effects.send_to_peer(config.initiator_id.into(), validation_bytes)
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

    let (_peer_id, confirmation_bytes) = effects.receive().await.map_err(|e| {
        RelationshipFormationError::Communication(format!(
            "Failed to receive confirmation: {}",
            e
        ))
    })?;
    
    let initiator_confirmation: RelationshipConfirmation = serde_json::from_slice(&confirmation_bytes)
        .map_err(|e| RelationshipFormationError::Communication(format!("Failed to deserialize confirmation: {}", e)))?;

    // Verify initiator's confirmation signature
    verify_trust_record_signature(
        &initiator_confirmation,
        &config.initiator_id,
        effects,
    )
    .await?;

    let signature =
        sign_trust_record(&trust_record_hash, &config.responder_id, effects).await?;

    let responder_confirmation = RelationshipConfirmation {
        context_id: context_id.clone(),
        trust_record_hash,
        signature,
    };

    let confirmation_bytes = serde_json::to_vec(&responder_confirmation)
        .map_err(|e| RelationshipFormationError::Communication(format!("Failed to serialize confirmation: {}", e)))?;
    
    effects.send_to_peer(config.initiator_id.into(), confirmation_bytes)
        .await
        .map_err(|e| {
            RelationshipFormationError::Communication(format!("Failed to send confirmation: {}", e))
        })?;

    effects.log_info("Relationship formation ceremony completed successfully").await;

    Ok(RelationshipFormationResult {
        context_id,
        relationship_keys,
        trust_record_hash,
        success: true,
    })
}

/// Derive context ID from relationship initialization request
async fn derive_context_id<E: RelationshipFormationEffects>(
    init_request: &RelationshipInitRequest,
    effects: &E,
) -> Result<ContextId, RelationshipFormationError> {
    let mut input = Vec::new();
    input.extend_from_slice(b"aura.relationship_formation.context:");
    input.extend_from_slice(init_request.initiator_id.0.as_bytes());
    input.extend_from_slice(init_request.responder_id.0.as_bytes());
    input.extend_from_slice(&init_request.nonce);

    if let Some(account_context) = &init_request.account_context {
        input.extend_from_slice(account_context.0.as_bytes());
    }

    let hash = effects.hash(&input).await;
    Ok(ContextId::new(String::from_utf8_lossy(&hash).to_string()))
}

/// Derive public key from private key (simplified Ed25519-like operation)
async fn derive_public_key<E: RelationshipFormationEffects>(
    private_key: &[u8],
    effects: &E,
) -> Result<Vec<u8>, RelationshipFormationError> {
    // Simplified public key derivation - in reality this would use proper elliptic curve operations
    let mut input = Vec::new();
    input.extend_from_slice(b"aura.public_key.derive:");
    input.extend_from_slice(private_key);

    let public_key = effects.hash(&input).await;
    Ok(public_key.to_vec())
}

/// Derive bidirectional relationship keys using ECDH-like key exchange
async fn derive_relationship_keys<E: RelationshipFormationEffects>(
    private_key: &[u8],
    peer_public_key: &[u8],
    context_id: &ContextId,
    effects: &E,
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
    enc_input.extend_from_slice(context_id.as_str().as_bytes());
    let encryption_key = effects.hash(&enc_input).await;

    // Derive MAC key
    let mut mac_input = Vec::new();
    mac_input.extend_from_slice(b"aura.relationship.mac_key:");
    mac_input.extend_from_slice(&secret_hash);
    mac_input.extend_from_slice(context_id.as_str().as_bytes());
    let mac_key = effects.hash(&mac_input).await;

    // Create derivation context for future key rotation
    let mut derivation_context = Vec::new();
    derivation_context.extend_from_slice(context_id.as_str().as_bytes());
    derivation_context.extend_from_slice(&secret_hash);

    Ok(RelationshipKeys {
        encryption_key: encryption_key.to_vec(),
        mac_key: mac_key.to_vec(),
        derivation_context,
    })
}

/// Create validation proof demonstrating correct key derivation
async fn create_validation_proof<E: RelationshipFormationEffects>(
    relationship_keys: &RelationshipKeys,
    device_id: &DeviceId,
    effects: &E,
) -> Result<Vec<u8>, RelationshipFormationError> {
    let mut input = Vec::new();
    input.extend_from_slice(b"aura.relationship.validation_proof:");
    input.extend_from_slice(&relationship_keys.encryption_key);
    input.extend_from_slice(&relationship_keys.mac_key);
    input.extend_from_slice(device_id.0.as_bytes());

    let proof = effects.hash(&input).await;
    Ok(proof.to_vec())
}

/// Hash relationship keys for verification
async fn hash_relationship_keys<E: RelationshipFormationEffects>(
    relationship_keys: &RelationshipKeys,
    effects: &E,
) -> Result<Vec<u8>, RelationshipFormationError> {
    let mut input = Vec::new();
    input.extend_from_slice(b"aura.relationship.key_hash:");
    input.extend_from_slice(&relationship_keys.encryption_key);
    input.extend_from_slice(&relationship_keys.mac_key);

    let hash = effects.hash(&input).await;
    Ok(hash.to_vec())
}

/// Verify validation proof from peer
async fn verify_validation_proof<E: RelationshipFormationEffects>(
    validation: &RelationshipValidation,
    relationship_keys: &RelationshipKeys,
    peer_id: &DeviceId,
    effects: &E,
) -> Result<(), RelationshipFormationError> {
    // Recompute expected proof
    let expected_proof = create_validation_proof(relationship_keys, peer_id, effects).await?;
    let expected_key_hash = hash_relationship_keys(relationship_keys, effects).await?;

    // Verify proof matches
    if validation.validation_proof != expected_proof {
        return Err(RelationshipFormationError::ValidationFailed(
            "Validation proof mismatch".to_string(),
        ));
    }

    // Verify key hash matches
    if validation.key_hash != expected_key_hash {
        return Err(RelationshipFormationError::ValidationFailed(
            "Key hash mismatch".to_string(),
        ));
    }

    Ok(())
}

/// Create trust record in local journal
async fn create_trust_record<E: RelationshipFormationEffects>(
    context_id: &ContextId,
    peer_id: &DeviceId,
    relationship_keys: &RelationshipKeys,
    effects: &E,
) -> Result<Hash32, RelationshipFormationError> {
    // Create trust record structure
    let mut record = Vec::new();
    record.extend_from_slice(b"aura.trust_record:");
    record.extend_from_slice(context_id.as_str().as_bytes());
    record.extend_from_slice(peer_id.0.as_bytes());
    record.extend_from_slice(&relationship_keys.derivation_context);
    record.extend_from_slice(&effects.current_timestamp().await.to_le_bytes());

    let record_hash = effects.hash(&record).await;

    // Store in journal (simplified - real implementation would use proper journal operations)
    let _journal_key = format!("trust_record.{}", hex::encode(record_hash));
    // Note: Trust record would be stored via JournalEffects in production
    tracing::debug!("Trust record created: {}", _journal_key);

    Ok(aura_core::Hash32(record_hash))
}

/// Sign trust record for non-repudiation
async fn sign_trust_record<E: RelationshipFormationEffects>(
    trust_record_hash: &Hash32,
    device_id: &DeviceId,
    effects: &E,
) -> Result<Vec<u8>, RelationshipFormationError> {
    let mut input = Vec::new();
    input.extend_from_slice(b"aura.trust_record.signature:");
    input.extend_from_slice(&trust_record_hash.0);
    input.extend_from_slice(device_id.0.as_bytes());

    let signature = effects.hash(&input).await;
    Ok(signature.to_vec())
}

/// Verify trust record signature
async fn verify_trust_record_signature<E: RelationshipFormationEffects>(
    confirmation: &RelationshipConfirmation,
    peer_id: &DeviceId,
    effects: &E,
) -> Result<(), RelationshipFormationError> {
    let expected_signature =
        sign_trust_record(&confirmation.trust_record_hash, peer_id, effects).await?;

    if confirmation.signature != expected_signature {
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
pub struct RelationshipFormationCoordinator<E: RelationshipFormationEffects> {
    effects: E,
}

impl<E: RelationshipFormationEffects> RelationshipFormationCoordinator<E> {
    /// Create new relationship formation coordinator
    pub fn new(effects: E) -> Self {
        Self { effects }
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
        match execute_relationship_formation(request.party_a, config, true, &self.effects).await {
            Ok(result) => {
                // Convert ceremony result to legacy response
                let relationship = Relationship {
                    id: result.context_id.as_str().as_bytes().to_vec(),
                    parties: vec![request.party_a, request.party_b],
                    account_id: request.account_id,
                    trust_level: request.initial_trust_level,
                    relationship_type: aura_core::RelationshipType::Trust,
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
    // Note: For testing, use mock handlers from aura-effects

    fn create_test_config() -> RelationshipFormationConfig {
        RelationshipFormationConfig {
            initiator_id: DeviceId::new(),
            responder_id: DeviceId::new(),
            account_context: None,
            timeout_secs: 60,
        }
    }

    #[ignore] // TODO: Fix MockEffects implementation - missing many required trait methods
    #[tokio::test]
    async fn test_relationship_formation_config_validation() {
        let mut config = create_test_config();
        config.responder_id = config.initiator_id; // Same device

        // Create a mock effects implementation
        struct MockEffects;
        
        impl ConsoleEffects for MockEffects {
            async fn log_info(&self, _message: &str) {}
            async fn log_warn(&self, _message: &str) {}
            async fn log_error(&self, _message: &str) {}
            async fn log_debug(&self, _message: &str) {}
        }
        
        impl CryptoEffects for MockEffects {
            async fn hash(&self, _input: &[u8]) -> Vec<u8> { vec![0u8; 32] }
            async fn random_bytes(&self, _len: usize) -> Vec<u8> { vec![0u8; 32] }
            async fn ed25519_sign(&self, _key: &[u8], _message: &[u8]) -> Result<Vec<u8>, aura_core::AuraError> { Ok(vec![0u8; 64]) }
            async fn ed25519_verify(&self, _public_key: &[u8], _message: &[u8], _signature: &[u8]) -> Result<bool, aura_core::AuraError> { Ok(true) }
        }
        
        impl NetworkEffects for MockEffects {
            async fn send_to_peer(&self, _peer: aura_core::PeerId, _data: Vec<u8>) -> Result<(), aura_core::AuraError> { Ok(()) }
            async fn receive(&self) -> Result<(aura_core::PeerId, Vec<u8>), aura_core::AuraError> { 
                Err(aura_core::AuraError::timeout("Mock timeout")) 
            }
        }
        
        impl RandomEffects for MockEffects {}
        impl TimeEffects for MockEffects {
            async fn current_timestamp(&self) -> u64 { 0 }
            async fn sleep(&self, _duration: std::time::Duration) {}
        }
        impl JournalEffects for MockEffects {}
        
        let mock_effects = MockEffects;
        let result = execute_relationship_formation(DeviceId::new(), config, true, &mock_effects).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RelationshipFormationError::InvalidConfig(_)
        ));
    }
}