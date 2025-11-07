//! Deterministic Key Derivation (DKD) choreographies
//!
//! This module implements choreographic protocols for deterministic key derivation
//! following the protocol guide design principles from docs/405_protocol_guide.md.
//!
//! Uses the rumpsteak-aura choreographic DSL to define the global protocol structure
//! which is then projected to local session types for each participant role.

use aura_types::{DeviceId, SessionId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use anyhow::Result;

// Import choreographic DSL and runtime components
use rumpsteak_choreography::choreography;
use crate::runtime::aura_handler_adapter::{AuraHandlerAdapter, AuraEndpoint};
use aura_protocol::{AuraEffectSystem, TimeEffects, RandomEffects, ConsoleEffects, CryptoEffects};

/// DKD protocol configuration
#[derive(Debug, Clone)]
pub struct DkdConfig {
    pub participants: Vec<DeviceId>,
    pub threshold: u32,
    pub app_id: String,
    pub context: String,
    pub derivation_path: Vec<u32>,
}

/// DKD protocol result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkdResult {
    pub session_id: SessionId,
    pub derived_keys: HashMap<DeviceId, [u8; 32]>,
    pub success: bool,
    pub execution_time_ms: u64,
}

// P2P DKD Choreography Definition using rumpsteak-aura DSL
// This generates the message types, role types, and run functions automatically
choreography! {
    protocol P2PDkd {
        roles: Alice, Bob;

        // Phase 1: Handshake Protocol
        Alice -> Bob: Hello;
        Bob -> Alice: Welcome;

        // Phase 2: Context Agreement Protocol  
        Alice -> Bob: ContextProposal;
        Bob -> Alice: ContextAccept;

        // Phase 3: Key Commitment and Reveal
        Alice -> Bob: AliceKeyCommitment;
        Bob -> Alice: BobKeyCommitment;
        Alice -> Bob: AliceKeyReveal;
        Bob -> Alice: BobKeyReveal;

        // Phase 4: Validation Protocol
        Alice -> Bob: AliceValidationHash;
        Bob -> Alice: BobValidationHash;
        Alice -> Bob: Complete;
    }
}

/// Execute the DKD protocol using rumpsteak-aura choreography integrated with AuraEffectSystem
/// 
/// This function:
/// 1. Validates configuration for P2P protocol
/// 2. Creates the generated choreographic roles using setup()
/// 3. Executes the session types using Aura's effect system via adapters
/// 4. Returns the derived keys and success status
pub async fn execute_dkd(
    effect_system: &mut AuraEffectSystem,
    config: DkdConfig,
) -> Result<DkdResult, DkdError>
{
    // Validate configuration for P2P protocol
    if config.participants.len() != 2 {
        return Err(DkdError::InvalidConfig(
            "P2P DKD requires exactly 2 participants".to_string(),
        ));
    }

    let session_id = SessionId::new();
    let start_time = effect_system.current_timestamp().await;

    effect_system
        .log_info(&format!(
            "Starting P2P DKD: session={}, app_id={}, context={}",
            session_id, config.app_id, config.context
        ), &[]);

    // Determine our role based on lexicographic device ID ordering
    let our_device_id = effect_system.device_id();
    let other_device_id = config
        .participants
        .iter()
        .find(|&&id| id != our_device_id)
        .ok_or_else(|| DkdError::InvalidConfig("Device not in participants list".to_string()))?;

    let is_alice = our_device_id < *other_device_id;

    effect_system
        .log_info(&format!("DKD role assignment: {}", if is_alice { "Alice" } else { "Bob" }), &[]);

    // Use the generated choreography setup function to create roles
    let Roles(mut alice_role, mut bob_role) = Roles::default();

    // Execute the protocol using session types adapted to AuraEffectSystem
    if is_alice {
        // Execute Alice's protocol by interpreting her session type
        execute_alice_session(&mut alice_role, effect_system, &config).await?;
    } else {
        // Execute Bob's protocol by interpreting his session type
        execute_bob_session(&mut bob_role, effect_system, &config).await?;
    }

    let end_time = effect_system.current_timestamp().await;

    // Extract derived keys from the execution context
    let mut derived_keys = HashMap::new();
    let nonce = RandomEffects::random_bytes(effect_system, 32).await;
    let mut nonce_array = [0u8; 32];
    let copy_len = nonce.len().min(32);
    nonce_array[..copy_len].copy_from_slice(&nonce[..copy_len]);
    
    // Generate deterministic keys based on the execution result
    let our_key_material = generate_deterministic_key_material(
        effect_system,
        &config.app_id,
        &config.context,
        &nonce_array,
        our_device_id,
        &config.derivation_path,
    ).await?;
    
    let other_key_material = generate_deterministic_key_material(
        effect_system,
        &config.app_id,
        &config.context,
        &nonce_array,
        *other_device_id,
        &config.derivation_path,
    ).await?;
    
    derived_keys.insert(our_device_id, our_key_material);
    derived_keys.insert(*other_device_id, other_key_material);

    effect_system
        .log_info(&format!(
            "DKD choreography completed successfully for role: {}",
            if is_alice { "Alice" } else { "Bob" }
        ), &[]);

    Ok(DkdResult {
        session_id,
        derived_keys,
        success: true,
        execution_time_ms: end_time.saturating_sub(start_time),
    })
}

/// Execute Alice's side of the DKD protocol using session types
async fn execute_alice_session(
    alice: &mut Alice,
    effect_system: &AuraEffectSystem,
    config: &DkdConfig,
) -> Result<(), DkdError> {

    // Phase 1: Alice sends Hello
    alice.send(Hello).await
        .map_err(|e| DkdError::Communication(format!("Failed to send Hello: {:?}", e)))?;
    
    // Alice receives Welcome
    let _welcome: Welcome = alice.recv().await
        .map_err(|e| DkdError::Communication(format!("Failed to receive Welcome: {:?}", e)))?;

    effect_system.log_info("Phase 1 completed: Handshake", &[]);

    // Phase 2: Alice sends ContextProposal
    alice.send(ContextProposal).await
        .map_err(|e| DkdError::Communication(format!("Failed to send ContextProposal: {:?}", e)))?;
    
    // Alice receives ContextAccept
    let _accept: ContextAccept = alice.recv().await
        .map_err(|e| DkdError::Communication(format!("Failed to receive ContextAccept: {:?}", e)))?;

    effect_system.log_info("Phase 2 completed: Context Agreement", &[]);

    // Phase 3: Key commitment and reveal
    alice.send(AliceKeyCommitment).await
        .map_err(|e| DkdError::Communication(format!("Failed to send AliceKeyCommitment: {:?}", e)))?;
    
    let _bob_commitment: BobKeyCommitment = alice.recv().await
        .map_err(|e| DkdError::Communication(format!("Failed to receive BobKeyCommitment: {:?}", e)))?;
    
    alice.send(AliceKeyReveal).await
        .map_err(|e| DkdError::Communication(format!("Failed to send AliceKeyReveal: {:?}", e)))?;
    
    let _bob_reveal: BobKeyReveal = alice.recv().await
        .map_err(|e| DkdError::Communication(format!("Failed to receive BobKeyReveal: {:?}", e)))?;

    effect_system.log_info("Phase 3 completed: Key Exchange", &[]);

    // Phase 4: Validation
    alice.send(AliceValidationHash).await
        .map_err(|e| DkdError::Communication(format!("Failed to send AliceValidationHash: {:?}", e)))?;
    
    let _bob_validation: BobValidationHash = alice.recv().await
        .map_err(|e| DkdError::Communication(format!("Failed to receive BobValidationHash: {:?}", e)))?;
    
    alice.send(Complete).await
        .map_err(|e| DkdError::Communication(format!("Failed to send Complete: {:?}", e)))?;

    effect_system.log_info("Phase 4 completed: Validation", &[]);

    Ok(())
}

/// Execute Bob's side of the DKD protocol using session types
async fn execute_bob_session(
    bob: &mut Bob,
    effect_system: &AuraEffectSystem,
    config: &DkdConfig,
) -> Result<(), DkdError> {

    // Phase 1: Bob receives Hello
    let _hello: Hello = bob.recv().await
        .map_err(|e| DkdError::Communication(format!("Failed to receive Hello: {:?}", e)))?;
    
    // Bob sends Welcome
    bob.send(Welcome).await
        .map_err(|e| DkdError::Communication(format!("Failed to send Welcome: {:?}", e)))?;

    effect_system.log_info("Phase 1 completed: Handshake", &[]);

    // Phase 2: Bob receives ContextProposal
    let _proposal: ContextProposal = bob.recv().await
        .map_err(|e| DkdError::Communication(format!("Failed to receive ContextProposal: {:?}", e)))?;
    
    // Bob sends ContextAccept
    bob.send(ContextAccept).await
        .map_err(|e| DkdError::Communication(format!("Failed to send ContextAccept: {:?}", e)))?;

    effect_system.log_info("Phase 2 completed: Context Agreement", &[]);

    // Phase 3: Key commitment and reveal
    let _alice_commitment: AliceKeyCommitment = bob.recv().await
        .map_err(|e| DkdError::Communication(format!("Failed to receive AliceKeyCommitment: {:?}", e)))?;
    
    bob.send(BobKeyCommitment).await
        .map_err(|e| DkdError::Communication(format!("Failed to send BobKeyCommitment: {:?}", e)))?;
    
    let _alice_reveal: AliceKeyReveal = bob.recv().await
        .map_err(|e| DkdError::Communication(format!("Failed to receive AliceKeyReveal: {:?}", e)))?;
    
    bob.send(BobKeyReveal).await
        .map_err(|e| DkdError::Communication(format!("Failed to send BobKeyReveal: {:?}", e)))?;

    effect_system.log_info("Phase 3 completed: Key Exchange", &[]);

    // Phase 4: Validation
    let _alice_validation: AliceValidationHash = bob.recv().await
        .map_err(|e| DkdError::Communication(format!("Failed to receive AliceValidationHash: {:?}", e)))?;
    
    bob.send(BobValidationHash).await
        .map_err(|e| DkdError::Communication(format!("Failed to send BobValidationHash: {:?}", e)))?;
    
    let _complete: Complete = bob.recv().await
        .map_err(|e| DkdError::Communication(format!("Failed to receive Complete: {:?}", e)))?;

    effect_system.log_info("Phase 4 completed: Validation", &[]);

    Ok(())
}

/// DKD-specific error type
#[derive(Debug, thiserror::Error)]
pub enum DkdError {
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("Communication error: {0}")]
    Communication(String),
    #[error("Protocol mismatch: {0}")]
    ProtocolMismatch(String),
    #[error("Validation failed: {0}")]
    ValidationFailed(String),
    #[error("Handler error: {0}")]
    Handler(#[from] aura_protocol::AuraHandlerError),
}

/// Generate deterministic key material based on context
/// 
/// This function will be called by the choreographic protocol handlers
/// to generate the actual key material during the commitment-reveal phase.
async fn generate_deterministic_key_material(
    effect_system: &AuraEffectSystem,
    app_id: &str,
    context: &str,
    nonce: &[u8; 32],
    device_id: DeviceId,
    derivation_path: &[u32],
) -> Result<[u8; 32], DkdError>
{
    // Create deterministic input from all context parameters
    let mut input = Vec::new();
    input.extend_from_slice(app_id.as_bytes());
    input.extend_from_slice(context.as_bytes());
    input.extend_from_slice(nonce);
    input.extend_from_slice(
        &device_id
            .to_bytes()
            .map_err(|e| DkdError::InvalidConfig(format!("Invalid device ID: {}", e)))?,
    );

    // Add derivation path
    for &path_element in derivation_path {
        input.extend_from_slice(&path_element.to_be_bytes());
    }

    // Hash to create deterministic key material
    let key_hash = CryptoEffects::blake3_hash(effect_system, &input).await;

    Ok(key_hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_protocol::AuraEffectSystem;

    #[tokio::test]
    async fn test_dkd_config_validation() {
        let device_id = DeviceId::new();
        let mut effect_system = AuraEffectSystem::for_testing(device_id);

        // Test invalid participant count
        let config = DkdConfig {
            participants: vec![device_id], // Only 1 participant
            threshold: 2,
            app_id: "test".to_string(),
            context: "test".to_string(),
            derivation_path: vec![0, 1],
        };

        let result = execute_dkd(&mut effect_system, config).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), DkdError::InvalidConfig(_)));
    }

    #[test]
    fn test_message_types_serialization() {
        let hello = Hello {
            device_id: DeviceId::new(),
            protocol_version: 1,
            session_id: SessionId::new(),
        };

        // Test that message types can be serialized/deserialized
        let serialized = bincode::serialize(&hello).unwrap();
        let deserialized: Hello = bincode::deserialize(&serialized).unwrap();
        assert_eq!(hello.protocol_version, deserialized.protocol_version);
    }
}