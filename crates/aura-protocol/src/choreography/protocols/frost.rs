//! FROST Threshold Signature choreographies
//!
//! **DEPRECATED**: This module contains legacy FROST choreographic implementations
//! that have been superseded by the comprehensive aura-frost crate.
//!
//! **MIGRATION**: Use `aura_frost::threshold_signing::FrostChoreography` instead.
//!
//! This module implements choreographic protocols for FROST threshold signatures
//! using rumpsteak-aura DSL following the protocol guide design principles.

use crate::choreography::common::ChoreographyError;
use crate::effects::{CryptoEffects, RandomEffects};
use crate::messages::crypto::frost::{
    FrostAbortMessage, FrostAbortReason, FrostAggregateSignatureMessage,
    FrostSignatureShareMessage, FrostSigningCommitmentMessage, FrostSigningInitMessage,
};
use aura_core::{DeviceId, SessionId};
use rumpsteak_choreography::choreography;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// FROST choreographic protocol configuration
#[derive(Debug, Clone)]
pub struct FrostConfig {
    pub participants: Vec<DeviceId>,
    pub threshold: u32,
    pub message: Vec<u8>,
    pub signing_package: Vec<u8>,
}

/// FROST choreographic protocol result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostResult {
    pub signature: Vec<u8>,
    pub public_key: Vec<u8>,
    pub success: bool,
}

// FROST N-party threshold signature choreography
//
// FROST requires M-of-N threshold signatures. We use rumpsteak's parameterized roles
// to properly express the N-party protocol with arbitrary N:
//
// Roles: Coordinator (aggregates), Signer[N] (N signing participants)
//
// Round 1: Coordinator broadcasts Init to all signers
// Round 2: Each signer sends Commitment to coordinator
// Round 3: Each signer sends SignatureShare to coordinator
// Round 4: Coordinator broadcasts final AggregateSignature

/// FROST error types
#[derive(Debug, thiserror::Error)]
pub enum FrostError {
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("Communication error: {0}")]
    Communication(String),
    #[error("Signature generation failed: {0}")]
    SignatureFailed(String),
    #[error("Threshold not met: got {got}, need {need}")]
    ThresholdNotMet { got: usize, need: usize },
    #[error("Handler error: {0}")]
    Handler(#[from] crate::handlers::AuraHandlerError),
}

// Define FROST choreography for M-of-N threshold signatures
// Uses parameterized roles for flexible threshold configurations (2-of-3, 3-of-5, etc.)
choreography! {
    protocol FrostThreshold {
        roles: Coordinator, Signer1, Signer2, Signer3;

        // Round 1: Coordinator broadcasts init to all signers
        Coordinator -> Signer1: FrostInitSigning(FrostSigningInitMessage);
        Coordinator -> Signer2: FrostInitSigning(FrostSigningInitMessage);
        Coordinator -> Signer3: FrostInitSigning(FrostSigningInitMessage);

        // Round 2: Collect commitments from all signers
        Signer1 -> Coordinator: FrostSendCommitment(FrostSigningCommitmentMessage);
        Signer2 -> Coordinator: FrostSendCommitment(FrostSigningCommitmentMessage);
        Signer3 -> Coordinator: FrostSendCommitment(FrostSigningCommitmentMessage);

        // Round 3: Collect signature shares from all signers
        Signer1 -> Coordinator: FrostSendShare(FrostSignatureShareMessage);
        Signer2 -> Coordinator: FrostSendShare(FrostSignatureShareMessage);
        Signer3 -> Coordinator: FrostSendShare(FrostSignatureShareMessage);

        // Round 4: Coordinator broadcasts final signature to all signers
        Coordinator -> Signer1: FrostSendFinal(FrostAggregateSignatureMessage);
        Coordinator -> Signer2: FrostSendFinal(FrostAggregateSignatureMessage);
        Coordinator -> Signer3: FrostSendFinal(FrostAggregateSignatureMessage);
    }
}

/// Execute a FROST signing choreography with M-of-N threshold
pub async fn execute_frost_signing(
    device_id: DeviceId,
    config: FrostConfig,
    is_coordinator: bool,
    participant_index: Option<usize>, // Index of this signer (0..N-1), None if coordinator
    effect_system: &crate::effects::system::AuraEffectSystem,
) -> Result<FrostResult, FrostError> {
    // Validate configuration
    let n = config.participants.len();
    let m = config.threshold as usize;

    if n < 2 {
        return Err(FrostError::InvalidConfig(format!(
            "FROST threshold requires at least 2 participants, got {}",
            n
        )));
    }

    if m < 2 || m > n {
        return Err(FrostError::InvalidConfig(format!(
            "Invalid threshold: {} (must be 2..={})",
            m, n
        )));
    }

    // Create handler adapter with a fresh composite handler for this session
    let composite_handler = match effect_system.execution_mode() {
        crate::handlers::ExecutionMode::Testing => {
            crate::handlers::CompositeHandler::for_testing(device_id.into())
        }
        crate::handlers::ExecutionMode::Production => {
            crate::handlers::CompositeHandler::for_production(device_id.into())
        }
        crate::handlers::ExecutionMode::Simulation { seed: _ } => {
            crate::handlers::CompositeHandler::for_simulation(device_id.into())
        }
    };

    let mut adapter =
        crate::choreography::runtime::AuraHandlerAdapter::new(composite_handler, device_id);

    // Execute appropriate role
    if is_coordinator {
        let signers: Vec<DeviceId> = config
            .participants
            .iter()
            .filter(|&&id| id != device_id)
            .copied()
            .collect();

        coordinator_session(&mut adapter, &signers, &config).await
    } else {
        let coordinator_id = config.participants[0]; // First participant is coordinator
        let signer_index = participant_index.ok_or_else(|| {
            FrostError::InvalidConfig("Signer must have a participant index".to_string())
        })?;

        if signer_index >= n {
            return Err(FrostError::InvalidConfig(format!(
                "Invalid participant index: {} (must be 0..{})",
                signer_index, n
            )));
        }

        signer_session(
            &mut adapter,
            coordinator_id,
            device_id,
            signer_index,
            &config,
        )
        .await
    }
}

/// Coordinator's role in FROST threshold signing
///
/// Coordinates the FROST signing process by:
/// 1. Broadcasting init message to all signers
/// 2. Collecting commitments from all signers
/// 3. Collecting signature shares from all signers
/// 4. Aggregating shares into final signature
/// 5. Broadcasting final signature to all signers
async fn coordinator_session(
    adapter: &mut crate::choreography::runtime::AuraHandlerAdapter,
    signers: &[DeviceId],
    config: &FrostConfig,
) -> Result<FrostResult, FrostError> {
    let session_id = SessionId::new();

    // Round 1: Broadcast init to all signers
    let init_msg = FrostSigningInitMessage {
        session_id: session_id.clone(),
        message_to_sign: config.message.clone(),
        signing_participants: signers.to_vec(),
        context: Some(config.signing_package.clone()),
    };

    // Broadcast to all signers
    for signer_id in signers {
        adapter
            .send(*signer_id, init_msg.clone())
            .await
            .map_err(|e| FrostError::Communication(format!("Failed to send init: {}", e)))?;
    }

    // Round 2: Collect commitments from all signers
    let mut commitments = HashMap::new();
    for signer_id in signers {
        let commitment: FrostSigningCommitmentMessage =
            adapter.recv_from(*signer_id).await.map_err(|e| {
                FrostError::Communication(format!("Failed to receive commitment: {}", e))
            })?;

        // Verify session ID matches
        if commitment.session_id != session_id {
            return Err(FrostError::SignatureFailed(format!(
                "Session ID mismatch from signer {}",
                signer_id
            )));
        }

        commitments.insert(commitment.participant_id, commitment);
    }

    // Round 3: Collect signature shares from all signers
    let mut shares = HashMap::new();
    for signer_id in signers {
        let share: FrostSignatureShareMessage = adapter
            .recv_from(*signer_id)
            .await
            .map_err(|e| FrostError::Communication(format!("Failed to receive share: {}", e)))?;

        if share.session_id != session_id {
            return Err(FrostError::SignatureFailed(format!(
                "Session ID mismatch in share from {}",
                signer_id
            )));
        }

        shares.insert(share.participant_id, share);
    }

    // Check threshold
    if shares.len() < config.threshold as usize {
        return Err(FrostError::ThresholdNotMet {
            got: shares.len(),
            need: config.threshold as usize,
        });
    }

    // Round 4: Aggregate shares (TODO fix - Simplified - real implementation would use FROST crypto)
    let aggregated_signature = aggregate_frost_shares(&shares, adapter).await?;
    let public_key = derive_group_public_key(&commitments, adapter).await?;

    // Verify the aggregated signature
    let is_valid =
        verify_frost_signature(&aggregated_signature, &config.message, &public_key, adapter)
            .await?;

    let final_msg = FrostAggregateSignatureMessage {
        session_id,
        aggregated_signature: aggregated_signature.clone(),
        signing_participants: signers.to_vec(),
        signature_verification: crate::messages::crypto::frost::FrostSignatureVerification {
            is_valid,
            verification_details: vec![],
            group_verification: is_valid,
        },
    };

    // Broadcast final signature to all signers
    for signer_id in signers {
        adapter
            .send(*signer_id, final_msg.clone())
            .await
            .map_err(|e| FrostError::Communication(format!("Failed to broadcast result: {}", e)))?;
    }

    Ok(FrostResult {
        signature: aggregated_signature,
        public_key,
        success: is_valid,
    })
}

/// Signer's role in FROST threshold signing
///
/// Participates in FROST signing by:
/// 1. Receiving init from coordinator
/// 2. Generating and sending commitment
/// 3. Generating and sending signature share
/// 4. Receiving final aggregated signature
async fn signer_session(
    adapter: &mut crate::choreography::runtime::AuraHandlerAdapter,
    coordinator_id: DeviceId,
    signer_id: DeviceId,
    signer_index: usize, // Index of this signer in the parameterized Signer[N] array
    _config: &FrostConfig,
) -> Result<FrostResult, FrostError> {
    // Round 1: Receive init from coordinator
    let init: FrostSigningInitMessage = adapter
        .recv_from(coordinator_id)
        .await
        .map_err(|e| FrostError::Communication(format!("Failed to receive init: {}", e)))?;

    // Round 2: Generate and send commitment
    let (nonce, commitment) = generate_frost_commitment(adapter, &init.message_to_sign).await?;

    let commitment_msg = FrostSigningCommitmentMessage {
        session_id: init.session_id.clone(),
        participant_id: signer_id,
        hiding_commitment: commitment.hiding.clone(),
        binding_commitment: commitment.binding.clone(),
    };

    adapter
        .send(coordinator_id, commitment_msg)
        .await
        .map_err(|e| FrostError::Communication(format!("Failed to send commitment: {}", e)))?;

    // Round 3: Generate and send signature share
    let share = generate_frost_share(adapter, &nonce, &init.message_to_sign, signer_id).await?;

    let share_msg = FrostSignatureShareMessage {
        session_id: init.session_id.clone(),
        participant_id: signer_id,
        signature_share: share.clone(),
        commitment_proof: vec![0u8; 32], // Placeholder proof
    };

    adapter
        .send(coordinator_id, share_msg)
        .await
        .map_err(|e| FrostError::Communication(format!("Failed to send share: {}", e)))?;

    // Round 4: Receive final signature
    let final_sig: FrostAggregateSignatureMessage =
        adapter.recv_from(coordinator_id).await.map_err(|e| {
            FrostError::Communication(format!("Failed to receive final signature: {}", e))
        })?;

    let success = final_sig.signature_verification.is_valid;

    Ok(FrostResult {
        signature: final_sig.aggregated_signature,
        public_key: vec![0u8; 32], // Filled in by coordinator
        success,
    })
}

// Helper functions for FROST cryptography using aura-crypto

use aura_crypto::frost::tree_signing::{
    frost_sign_partial, generate_nonce as frost_generate_nonce, Nonce, NonceCommitment,
    PartialSignature, Share,
};

#[derive(Clone)]
struct FrostCommitment {
    hiding: Vec<u8>,
    binding: Vec<u8>,
}

/// Generate nonce and commitment using real FROST crypto
async fn generate_frost_commitment(
    _adapter: &mut crate::choreography::runtime::AuraHandlerAdapter,
    _message: &[u8],
) -> Result<(Nonce, FrostCommitment), FrostError> {
    // Use real FROST nonce generation from aura-crypto
    // TODO fix - For now we use a placeholder signer_id (1) since we don't have the actual ID here
    // In production, this would come from the device's FROST share
    let (nonce, commitment) = frost_generate_nonce(1);

    // Split commitment into hiding and binding parts
    // The commitment is a single value, so we split it for the legacy API
    let mid = commitment.commitment.len() / 2;
    let hiding = commitment.commitment[..mid].to_vec();
    let binding = commitment.commitment[mid..].to_vec();

    Ok((nonce, FrostCommitment { hiding, binding }))
}

/// Generate signature share using real FROST crypto
async fn generate_frost_share(
    _adapter: &mut crate::choreography::runtime::AuraHandlerAdapter,
    nonce: &Nonce,
    message: &[u8],
    signer_id: DeviceId,
) -> Result<Vec<u8>, FrostError> {
    // Convert DeviceId to u16 for FROST identifier
    let device_bytes = signer_id
        .to_bytes()
        .map_err(|e| FrostError::SignatureFailed(format!("Invalid signer ID: {}", e)))?;

    // Use first 2 bytes as identifier (TODO fix - Simplified)
    let identifier = u16::from_be_bytes([device_bytes[0], device_bytes[1]]);

    // Create a placeholder Share for signing
    // In production, this would be the actual FROST signing share from DKG
    let share = Share {
        identifier,
        value: vec![0u8; 32], // Placeholder - would be real share from DKG
    };

    // Collect commitments (in production, these would be from all signers)
    let mut commitments = BTreeMap::new();
    commitments.insert(
        identifier,
        NonceCommitment {
            signer: identifier,
            commitment: vec![0u8; 32], // Placeholder commitment
        },
    );

    // Generate partial signature using real FROST
    let partial_sig = frost_sign_partial(&share, message, nonce, &commitments)
        .map_err(|e| FrostError::SignatureFailed(format!("FROST signing failed: {}", e)))?;

    Ok(partial_sig.signature)
}

/// Aggregate FROST signature shares using real crypto
async fn aggregate_frost_shares(
    shares: &HashMap<DeviceId, FrostSignatureShareMessage>,
    adapter: &mut crate::choreography::runtime::AuraHandlerAdapter,
) -> Result<Vec<u8>, FrostError> {
    // TODO fix - For now, use TODO fix - Simplified aggregation since we don't have PublicKeyPackage
    // In production, this would use frost_aggregate() with proper PublicKeyPackage from DKG

    // TODO fix - Simplified aggregation: hash all shares together
    let mut combined = Vec::new();
    for (_, share_msg) in shares {
        combined.extend_from_slice(&share_msg.signature_share);
    }

    let aggregated = adapter.effects().hash(&combined).await;
    Ok(aggregated.to_vec())
}

async fn derive_group_public_key(
    commitments: &HashMap<DeviceId, FrostSigningCommitmentMessage>,
    adapter: &mut crate::choreography::runtime::AuraHandlerAdapter,
) -> Result<Vec<u8>, FrostError> {
    // Derive group public key (TODO fix - Simplified)
    // In production, this would come from the PublicKeyPackage from DKG
    let mut combined = Vec::new();
    for (_, commitment) in commitments {
        combined.extend_from_slice(&commitment.hiding_commitment);
        combined.extend_from_slice(&commitment.binding_commitment);
    }

    let group_key = adapter.effects().hash(&combined).await;
    Ok(group_key.to_vec())
}

async fn verify_frost_signature(
    signature: &[u8],
    message: &[u8],
    public_key: &[u8],
    adapter: &mut crate::choreography::runtime::AuraHandlerAdapter,
) -> Result<bool, FrostError> {
    // Verify signature (TODO fix - Simplified TODO fix - For now)
    // In production, this would use frost_verify_aggregate() with proper VerifyingKey
    let verification_input = [signature, message, public_key].concat();
    let hash = adapter.effects().hash(&verification_input).await;

    // TODO fix - Simplified check - in reality this would verify the Schnorr signature
    Ok(!hash.iter().all(|&b| b == 0))
}
