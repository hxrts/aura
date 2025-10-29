//! FROST Key Manager - Unified FROST key generation and management
//!
//! This module consolidates FROST key generation logic that was previously
//! duplicated across frost_lifecycle.rs and local_runtime.rs. It provides
//! a clean interface for generating key shares and managing FROST signing keys.

use aura_crypto::{frost::FrostKeyShare, Effects};
use aura_journal::capability::threshold_capabilities::PublicKeyPackage;
use aura_types::{AuraError, DeviceId};
use frost_ed25519 as frost;
use std::collections::BTreeMap;
use tracing::debug;

/// Manager for FROST key generation and signing operations
pub struct FrostKeyManager<'a> {
    effects: &'a Effects,
    device_id: DeviceId,
}

impl<'a> FrostKeyManager<'a> {
    /// Create a new FROST key manager
    pub fn new(device_id: DeviceId, effects: &'a Effects) -> Self {
        Self { effects, device_id }
    }

    /// Generate a FROST key share for this device using trusted dealer DKG
    ///
    /// # Arguments
    /// * `threshold` - Minimum number of signers required
    /// * `participants` - List of all participant device IDs
    ///
    /// # Returns
    /// The key share for this device and the public key package
    ///
    /// # Note
    /// This uses a trusted dealer for MVP. Production would use distributed DKG.
    pub fn generate_key_share(
        &self,
        threshold: u16,
        participants: &[DeviceId],
    ) -> Result<(FrostKeyShare, PublicKeyPackage), AuraError> {
        let max_participants = participants.len() as u16;

        debug!(
            "Generating FROST key share for device {} with {}-of-{} threshold",
            self.device_id, threshold, max_participants
        );

        // Generate keys using trusted dealer
        let mut rng = self.effects.rng();
        let (secret_shares, pubkey_package) = frost::keys::generate_with_dealer(
            threshold,
            max_participants,
            frost::keys::IdentifierList::Default,
            &mut rng,
        )
        .map_err(|e| {
            AuraError::protocol_invalid_instruction(&format!("FROST DKG failed: {:?}", e))
        })?;

        // Find this device's position in participants list
        let position = participants
            .iter()
            .position(|&id| id == self.device_id)
            .ok_or_else(|| {
                AuraError::protocol_invalid_instruction("Device not in participants list")
            })?;

        let participant_id = frost::Identifier::try_from((position + 1) as u16)
            .map_err(|_| AuraError::protocol_invalid_instruction("Invalid participant ID"))?;

        let secret_share = secret_shares.get(&participant_id).ok_or_else(|| {
            AuraError::protocol_invalid_instruction("No share generated for participant")
        })?;

        let key_package = frost::keys::KeyPackage::try_from(secret_share.clone()).map_err(|e| {
            AuraError::protocol_invalid_instruction(&format!("KeyPackage creation failed: {:?}", e))
        })?;

        let key_share = FrostKeyShare {
            identifier: participant_id,
            signing_share: *key_package.signing_share(),
            verifying_key: *pubkey_package.verifying_key(),
        };

        // Convert frost public key package to aura public key package
        use aura_crypto::Ed25519VerifyingKey;
        let frost_vk = pubkey_package.verifying_key();
        let vk_bytes = frost_vk.serialize();
        let group_public = Ed25519VerifyingKey::from_bytes(&vk_bytes).map_err(|e| {
            AuraError::protocol_invalid_instruction(&format!(
                "Failed to convert verifying key: {:?}",
                e
            ))
        })?;

        let aura_pubkey_package = PublicKeyPackage {
            group_public,
            threshold,
            total_participants: max_participants,
        };

        debug!(
            "Successfully generated FROST key share for device {}",
            self.device_id
        );

        Ok((key_share, aura_pubkey_package))
    }

    /// Generate temporary signing keys for FROST signing ceremony
    ///
    /// # Arguments
    /// * `threshold` - Minimum number of signers required
    /// * `num_participants` - Total number of participants
    ///
    /// # Returns
    /// Map of participant IDs to key packages, and the public key package
    ///
    /// # Note
    /// This is a simplified implementation for MVP. Production would coordinate
    /// across multiple devices using the distributed protocol.
    pub fn generate_signing_keys(
        &self,
        threshold: u16,
        num_participants: u16,
    ) -> Result<
        (
            BTreeMap<frost::Identifier, frost::keys::KeyPackage>,
            frost::keys::PublicKeyPackage,
        ),
        AuraError,
    > {
        debug!(
            "Generating FROST signing keys with {}-of-{} threshold",
            threshold, num_participants
        );

        let mut rng = self.effects.rng();
        let (secret_shares, pubkey_package) = frost::keys::generate_with_dealer(
            threshold,
            num_participants,
            frost::keys::IdentifierList::Default,
            &mut rng,
        )
        .map_err(|e| {
            AuraError::protocol_invalid_instruction(&format!("Key generation failed: {:?}", e))
        })?;

        // Convert SecretShare to KeyPackage for each participant
        let mut key_packages = BTreeMap::new();
        for (id, secret_share) in secret_shares {
            let key_package = frost::keys::KeyPackage::try_from(secret_share).map_err(|e| {
                AuraError::protocol_invalid_instruction(&format!(
                    "KeyPackage conversion failed: {:?}",
                    e
                ))
            })?;
            key_packages.insert(id, key_package);
        }

        debug!(
            "Successfully generated {} FROST signing key packages",
            key_packages.len()
        );

        Ok((key_packages, pubkey_package))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_types::DeviceIdExt;

    #[test]
    fn test_generate_key_share() {
        let effects = Effects::test();
        let device1 = DeviceId::new_with_effects(&effects);
        let device2 = DeviceId::new_with_effects(&effects);
        let device3 = DeviceId::new_with_effects(&effects);
        let participants = vec![device1, device2, device3];

        let manager = FrostKeyManager::new(device1, &effects);
        // Use threshold=3 to satisfy FROST requirements (min_signers >= 2)
        let result = manager.generate_key_share(3, &participants);

        if result.is_err() {
            eprintln!("Error: {:?}", result.as_ref().unwrap_err());
        }
        assert!(
            result.is_ok(),
            "Failed to generate key share: {:?}",
            result.err()
        );
        let (key_share, pubkey_package) = result.unwrap();

        // Verify key share has correct identifier
        assert_eq!(
            key_share.identifier,
            frost::Identifier::try_from(1u16).unwrap()
        );
        // Verify public key package has correct parameters
        assert_eq!(pubkey_package.threshold, 3);
        assert_eq!(pubkey_package.total_participants, 3);
    }

    #[test]
    fn test_generate_signing_keys() {
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);

        let manager = FrostKeyManager::new(device_id, &effects);
        // Use threshold=3 to satisfy FROST requirements
        let result = manager.generate_signing_keys(3, 3);

        assert!(result.is_ok());
        let (key_packages, _pubkey_package) = result.unwrap();

        // Should generate 3 key packages
        assert_eq!(key_packages.len(), 3);
    }

    #[test]
    fn test_invalid_threshold() {
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);

        let manager = FrostKeyManager::new(device_id, &effects);

        // Threshold of 1 with 2 participants should fail (FROST requires threshold >= 2)
        let result = manager.generate_signing_keys(1, 2);
        assert!(result.is_err());
    }

    #[test]
    fn test_device_not_in_participants() {
        let effects = Effects::test();
        let device1 = DeviceId::new_with_effects(&effects);
        let device2 = DeviceId::new_with_effects(&effects);
        let device3 = DeviceId::new_with_effects(&effects);

        // Create manager for device1 but don't include it in participants
        let participants = vec![device2, device3];
        let manager = FrostKeyManager::new(device1, &effects);

        // Use threshold=2 (max for 2 participants)
        let result = manager.generate_key_share(2, &participants);
        assert!(result.is_err());
    }
}
