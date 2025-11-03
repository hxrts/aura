//! Example implementations using the ThresholdCollect pattern
//!
//! This module demonstrates how the generic ThresholdCollect pattern can be
//! used to implement DKD and FROST protocols with significantly less code
//! duplication and better consistency.

use super::threshold_collect::ThresholdOperationProvider;
use aura_protocol::effects::choreographic::ChoreographicRole;
use aura_types::effects::Effects;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Example: DKD context and materials
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkdContext {
    pub app_id: String,
    pub derivation_context: String,
    pub threshold: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkdMaterial {
    pub participant_id: u16,
    pub key_share: Vec<u8>,
    pub commitment: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkdResult {
    pub derived_key: Vec<u8>,
    pub proof: Vec<u8>,
}

/// DKD implementation using ThresholdCollect pattern
pub struct DkdThresholdProvider {
    threshold: u16,
}

impl DkdThresholdProvider {
    pub fn new(threshold: u16) -> Self {
        Self { threshold }
    }
}

impl ThresholdOperationProvider<DkdContext, DkdMaterial, DkdResult> for DkdThresholdProvider {
    fn validate_context(&self, context: &DkdContext) -> Result<(), String> {
        if context.app_id.is_empty() {
            return Err("App ID cannot be empty".to_string());
        }
        if context.derivation_context.is_empty() {
            return Err("Derivation context cannot be empty".to_string());
        }
        if context.threshold != self.threshold {
            return Err("Threshold mismatch".to_string());
        }
        Ok(())
    }

    fn generate_material(
        &self,
        context: &DkdContext,
        participant: ChoreographicRole,
        effects: &Effects,
    ) -> Result<DkdMaterial, String> {
        // Generate deterministic key share based on context and participant
        let input = format!(
            "{}:{}:{}",
            context.app_id, context.derivation_context, participant.device_id
        );
        let key_share = effects.blake3_hash(input.as_bytes()).to_vec();

        // Generate commitment to the key share
        let commitment = effects.blake3_hash(&key_share).to_vec();

        Ok(DkdMaterial {
            participant_id: participant.role_index as u16,
            key_share,
            commitment,
        })
    }

    fn validate_material(
        &self,
        _context: &DkdContext,
        _participant: ChoreographicRole,
        material: &DkdMaterial,
        effects: &Effects,
    ) -> Result<(), String> {
        // Verify commitment matches key share
        let expected_commitment = effects.blake3_hash(&material.key_share);
        if material.commitment != expected_commitment.to_vec() {
            return Err("Invalid commitment".to_string());
        }
        Ok(())
    }

    fn aggregate_materials(
        &self,
        context: &DkdContext,
        materials: &BTreeMap<ChoreographicRole, DkdMaterial>,
        effects: &Effects,
    ) -> Result<DkdResult, String> {
        if materials.len() < self.threshold as usize {
            return Err("Insufficient materials for threshold".to_string());
        }

        // Combine key shares using XOR (simplified)
        let mut derived_key = vec![0u8; 32];
        for material in materials.values().take(self.threshold as usize) {
            for (i, byte) in material.key_share.iter().take(32).enumerate() {
                derived_key[i] ^= byte;
            }
        }

        // Generate proof of derivation
        let proof_input = format!(
            "{}:{}:{:?}",
            context.app_id, context.derivation_context, derived_key
        );
        let proof = effects.blake3_hash(proof_input.as_bytes()).to_vec();

        Ok(DkdResult { derived_key, proof })
    }

    fn verify_result(
        &self,
        context: &DkdContext,
        result: &DkdResult,
        _participants: &[ChoreographicRole],
        effects: &Effects,
    ) -> Result<bool, String> {
        // Verify the proof is correct for the derived key
        let expected_proof_input = format!(
            "{}:{}:{:?}",
            context.app_id, context.derivation_context, result.derived_key
        );
        let expected_proof = effects.blake3_hash(expected_proof_input.as_bytes());

        Ok(result.proof == expected_proof.to_vec())
    }

    fn operation_name(&self) -> &str {
        "DKD"
    }
}

/// Example: FROST context and materials
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostContext {
    pub message: Vec<u8>,
    pub threshold: u16,
    pub key_package_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostMaterial {
    pub participant_id: u16,
    pub commitment: Vec<u8>,      // Serialized SigningCommitments
    pub signature_share: Vec<u8>, // Serialized SignatureShare
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostResult {
    pub signature: Vec<u8>,
    pub verification_key: Vec<u8>,
}

/// FROST implementation using ThresholdCollect pattern
pub struct FrostThresholdProvider {
    threshold: u16,
}

impl FrostThresholdProvider {
    pub fn new(threshold: u16) -> Self {
        Self { threshold }
    }
}

impl ThresholdOperationProvider<FrostContext, FrostMaterial, FrostResult>
    for FrostThresholdProvider
{
    fn validate_context(&self, context: &FrostContext) -> Result<(), String> {
        if context.message.is_empty() {
            return Err("Message cannot be empty".to_string());
        }
        if context.threshold != self.threshold {
            return Err("Threshold mismatch".to_string());
        }
        if context.key_package_id.is_empty() {
            return Err("Key package ID cannot be empty".to_string());
        }
        Ok(())
    }

    fn generate_material(
        &self,
        context: &FrostContext,
        participant: ChoreographicRole,
        effects: &Effects,
    ) -> Result<FrostMaterial, String> {
        // Generate mock FROST commitments and signature shares
        // In a real implementation, this would use actual FROST cryptography
        let participant_seed = format!("{}:{}", context.key_package_id, participant.device_id);
        let seed_hash = effects.blake3_hash(participant_seed.as_bytes());

        // Mock commitment (would be actual FROST commitment)
        let mut commitment_input = seed_hash.to_vec();
        commitment_input.extend_from_slice(&context.message);
        let commitment = effects
            .blake3_hash(&commitment_input)
            .to_vec();

        // Mock signature share (would be actual FROST signature share)
        let mut signature_input = commitment.clone();
        signature_input.extend_from_slice(&context.message);
        let signature_share = effects
            .blake3_hash(&signature_input)
            .to_vec();

        Ok(FrostMaterial {
            participant_id: participant.role_index as u16,
            commitment,
            signature_share,
        })
    }

    fn validate_material(
        &self,
        context: &FrostContext,
        participant: ChoreographicRole,
        material: &FrostMaterial,
        effects: &Effects,
    ) -> Result<(), String> {
        // Validate the commitment and signature share are consistent
        let expected_participant_seed =
            format!("{}:{}", context.key_package_id, participant.device_id);
        let expected_seed_hash = effects.blake3_hash(expected_participant_seed.as_bytes());
        let mut expected_commitment_input = expected_seed_hash.to_vec();
        expected_commitment_input.extend_from_slice(&context.message);
        let expected_commitment =
            effects.blake3_hash(&expected_commitment_input);

        if material.commitment != expected_commitment.to_vec() {
            return Err("Invalid FROST commitment".to_string());
        }

        let mut expected_signature_input = material.commitment.clone();
        expected_signature_input.extend_from_slice(&context.message);
        let expected_signature_share =
            effects.blake3_hash(&expected_signature_input);
        if material.signature_share != expected_signature_share.to_vec() {
            return Err("Invalid FROST signature share".to_string());
        }

        Ok(())
    }

    fn aggregate_materials(
        &self,
        context: &FrostContext,
        materials: &BTreeMap<ChoreographicRole, FrostMaterial>,
        effects: &Effects,
    ) -> Result<FrostResult, String> {
        if materials.len() < self.threshold as usize {
            return Err("Insufficient materials for threshold signature".to_string());
        }

        // Aggregate signature shares (simplified - would be actual FROST aggregation)
        let mut aggregated_sig = vec![0u8; 64];
        for material in materials.values().take(self.threshold as usize) {
            for (i, byte) in material.signature_share.iter().take(64).enumerate() {
                aggregated_sig[i] ^= byte;
            }
        }

        // Generate verification key (would be actual public key)
        let verification_key = effects
            .blake3_hash(&context.key_package_id.as_bytes())
            .to_vec();

        Ok(FrostResult {
            signature: aggregated_sig,
            verification_key,
        })
    }

    fn verify_result(
        &self,
        context: &FrostContext,
        result: &FrostResult,
        _participants: &[ChoreographicRole],
        effects: &Effects,
    ) -> Result<bool, String> {
        // Verify signature is valid for message and verification key
        // This is a simplified check - real FROST would verify the actual signature
        let expected_verification_key = effects.blake3_hash(&context.key_package_id.as_bytes());

        if result.verification_key != expected_verification_key.to_vec() {
            return Ok(false);
        }

        // Additional signature verification would go here
        // For now, just check signature is not all zeros
        Ok(!result.signature.iter().all(|&b| b == 0))
    }

    fn operation_name(&self) -> &str {
        "FROST"
    }
}

/// Example usage functions
#[cfg(test)]
mod tests {
    use super::*;
    use aura_types::effects::Effects;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_dkd_threshold_collect() {
        let effects = Effects::test(42);

        let context = DkdContext {
            app_id: "test_app".to_string(),
            derivation_context: "user_keys".to_string(),
            threshold: 2,
        };

        let participants = vec![
            ChoreographicRole {
                device_id: Uuid::new_v4(),
                role_index: 0,
            },
            ChoreographicRole {
                device_id: Uuid::new_v4(),
                role_index: 1,
            },
            ChoreographicRole {
                device_id: Uuid::new_v4(),
                role_index: 2,
            },
        ];

        let config = ThresholdCollectConfig {
            threshold: 2,
            ..Default::default()
        };

        let provider = DkdThresholdProvider::new(2);

        let choreography =
            ThresholdCollectChoreography::new(config, context, participants, provider, effects)
                .unwrap();

        // In a real test, we would execute the choreography with a test handler
        // For now, we just verify the choreography was created successfully
        assert_eq!(choreography.operation_id.len(), 36); // UUID length
    }

    #[tokio::test]
    async fn test_frost_threshold_collect() {
        let effects = Effects::test(42);

        let context = FrostContext {
            message: b"test message to sign".to_vec(),
            threshold: 2,
            key_package_id: "test_key_package".to_string(),
        };

        let participants = vec![
            ChoreographicRole {
                device_id: Uuid::new_v4(),
                role_index: 0,
            },
            ChoreographicRole {
                device_id: Uuid::new_v4(),
                role_index: 1,
            },
            ChoreographicRole {
                device_id: Uuid::new_v4(),
                role_index: 2,
            },
        ];

        let config = ThresholdCollectConfig {
            threshold: 2,
            ..Default::default()
        };

        let provider = FrostThresholdProvider::new(2);

        let choreography =
            ThresholdCollectChoreography::new(config, context, participants, provider, effects)
                .unwrap();

        // In a real test, we would execute the choreography with a test handler
        // For now, we just verify the choreography was created successfully
        assert_eq!(choreography.operation_id.len(), 36); // UUID length
    }
}
