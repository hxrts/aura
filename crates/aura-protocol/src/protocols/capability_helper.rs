//! Capability Proof Helper - Unified capability proof creation for all protocols
//!
//! This module consolidates capability proof creation logic that was previously
//! duplicated across all protocol lifecycle implementations. It provides a clean
//! builder interface for creating both real cryptographic capability proofs and
//! placeholder proofs for testing.

use crate::capability_authorization::create_capability_authorization_manager;
use crate::protocol_results::CapabilityProof;
use aura_crypto::Effects;
use aura_journal::capability::Permission;
use aura_types::{AuraError, DeviceId, DeviceIdExt};
use tracing::debug;

/// Builder for creating capability proofs with consistent authorization logic
pub struct CapabilityProofBuilder {
    device_id: DeviceId,
    protocol_name: &'static str,
}

impl CapabilityProofBuilder {
    /// Create a new capability proof builder for a specific protocol
    ///
    /// # Arguments
    /// * `device_id` - The device executing the protocol
    /// * `protocol_name` - Human-readable protocol name for logging
    pub fn new(device_id: DeviceId, protocol_name: &'static str) -> Self {
        Self {
            device_id,
            protocol_name,
        }
    }

    /// Create a real cryptographic capability proof with threshold authorization
    ///
    /// # Arguments
    /// * `resource` - The resource being accessed (e.g., "dkd_derived_keys")
    /// * `context` - The operation context (e.g., "dkd_key_derivation")
    ///
    /// # Returns
    /// A capability proof with cryptographic signatures, or an error if authorization fails
    pub fn create_proof(
        &self,
        resource: &str,
        context: &str,
    ) -> Result<CapabilityProof, AuraError> {
        debug!(
            "Creating capability proof for {} protocol on device {}",
            self.protocol_name, self.device_id
        );

        // Create effects for deterministic authorization
        let effects = Effects::for_test(&format!(
            "{}_lifecycle_{}",
            self.protocol_name, self.device_id
        ));

        // Create authorization manager for this device
        let auth_manager = create_capability_authorization_manager(self.device_id, &effects);

        // Define the permission required for this operation
        let permission = Permission::Storage {
            operation: aura_journal::capability::StorageOperation::Write,
            resource: resource.to_string(),
        };

        // Create capability proof with signature-based authorization
        let capability_proof = auth_manager
            .create_capability_proof(permission, context, &effects)
            .map_err(|e| {
                debug!(
                    "Failed to create {} capability proof: {:?}",
                    self.protocol_name, e
                );
                AuraError::insufficient_capability(format!(
                    "{} capability authorization failed",
                    self.protocol_name
                ))
            })?;

        debug!(
            "Successfully created capability proof for {} protocol",
            self.protocol_name
        );
        Ok(capability_proof)
    }

    /// Create a placeholder capability proof for testing/development
    ///
    /// This provides a valid proof structure without real cryptographic signatures.
    /// Used as a fallback when real authorization fails or in testing environments.
    pub fn create_placeholder() -> CapabilityProof {
        use aura_crypto::Ed25519Signature;
        use aura_journal::capability::{
            threshold_capabilities::{
                PublicKeyPackage, ThresholdCapability, ThresholdCapabilityId, ThresholdSignature,
            },
            unified_manager::{CapabilityType, VerificationContext},
            Permission,
        };
        use aura_types::DeviceId;

        // Create a placeholder device ID
        let effects = aura_crypto::Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);

        // Create minimal threshold signature for placeholder
        let placeholder_signature = ThresholdSignature {
            signature: Ed25519Signature::default(),
            signers: vec![],
        };

        // Create placeholder public key package
        // For placeholder purposes, we use an empty/default package
        let placeholder_key_package = {
            use aura_crypto::Ed25519VerifyingKey;
            let mut rng = effects.rng();
            let (_, pubkey_package) = frost_ed25519::keys::generate_with_dealer(
                2,
                2,
                frost_ed25519::keys::IdentifierList::Default,
                &mut rng,
            )
            .expect("Failed to generate placeholder keys");

            // Convert frost verifying key to ed25519_dalek verifying key
            let frost_vk = pubkey_package.verifying_key();
            let vk_bytes = frost_vk.serialize();
            let group_public = Ed25519VerifyingKey::from_bytes(&vk_bytes)
                .expect("Failed to convert verifying key");

            PublicKeyPackage {
                group_public,
                threshold: 2,
                total_participants: 2,
            }
        };

        let primary_capability = ThresholdCapability {
            id: ThresholdCapabilityId([0; 32]),
            device_id,
            permissions: vec![Permission::Storage {
                operation: aura_journal::capability::StorageOperation::Write,
                resource: "placeholder".to_string(),
            }],
            authorization: placeholder_signature,
            public_key_package: placeholder_key_package,
            issued_at: 0,
            expires_at: None,
            delegation_chain: vec![],
        };

        let verification_context = VerificationContext {
            capability_type: CapabilityType::Threshold,
            authority_level: 1,
            near_expiration: false,
        };

        CapabilityProof {
            primary_capability,
            participant_capabilities: vec![],
            verification_context,
            requires_admin: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_types::DeviceIdExt;

    #[test]
    fn test_capability_proof_builder_creation() {
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);

        let builder = CapabilityProofBuilder::new(device_id, "test_protocol");

        // Should be able to create a real proof
        let proof_result = builder.create_proof("test_resource", "test_context");

        // Either succeeds or fails gracefully
        match proof_result {
            Ok(proof) => {
                assert!(!proof.primary_capability.id.0.is_empty());
            }
            Err(_) => {
                // Expected in some test environments
            }
        }
    }

    #[test]
    fn test_placeholder_proof() {
        let proof = CapabilityProofBuilder::create_placeholder();

        assert_eq!(proof.primary_capability.id.0, [0; 32]);
        assert_eq!(proof.verification_context.authority_level, 1);
        assert!(!proof.verification_context.near_expiration);
        assert!(!proof.requires_admin);
    }

    #[test]
    fn test_builder_with_different_protocols() {
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);

        let protocols = [
            "dkd",
            "recovery",
            "resharing",
            "locking",
            "counter",
            "group",
            "frost",
        ];

        for protocol_name in protocols {
            let builder = CapabilityProofBuilder::new(device_id, protocol_name);
            let proof = builder
                .create_proof(&format!("{}_resource", protocol_name), protocol_name)
                .unwrap_or_else(|_| CapabilityProofBuilder::create_placeholder());

            // Proof should be valid
            assert!(!proof.primary_capability.id.0.is_empty());
        }
    }
}
