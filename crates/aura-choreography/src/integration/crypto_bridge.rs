//! Bridge to aura-crypto for choreographic protocols

// TODO: Fix FROST imports after getting basic structure working
// use aura_crypto::middleware::CryptoEffects;
use aura_protocol::effects::Effects;
// use frost_ed25519::{Identifier, KeyPackage, SigningCommitments, SigningNonces, SigningPackage};
use std::collections::BTreeMap;

/// Crypto bridge for DKD operations
pub struct DkdCryptoBridge {
    effects: Effects,
}

impl DkdCryptoBridge {
    pub fn new(effects: Effects) -> Self {
        Self { effects }
    }

    /// Derive key share for DKD protocol
    pub async fn derive_key_share(
        &self,
        app_id: &str,
        context: &str,
        participant_index: usize,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        // Create derivation context
        let mut derivation_data = Vec::new();
        derivation_data.extend_from_slice(app_id.as_bytes());
        derivation_data.extend_from_slice(context.as_bytes());
        derivation_data.extend_from_slice(&participant_index.to_le_bytes());

        // Use effects for deterministic key derivation
        let context_hash = self.effects.blake3_hash(&derivation_data);

        // For MVP, use the hash as the key share
        // TODO: Implement proper threshold key derivation
        Ok(context_hash.to_vec())
    }

    /// Aggregate DKD shares
    pub async fn aggregate_shares(
        &self,
        shares: &[Vec<u8>],
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        if shares.is_empty() {
            return Err("No shares to aggregate".into());
        }

        // Simple aggregation: XOR all shares
        // TODO: Implement proper cryptographic aggregation
        let mut result = shares[0].clone();
        for share in &shares[1..] {
            if share.len() != result.len() {
                return Err("Share length mismatch".into());
            }
            for (i, &byte) in share.iter().enumerate() {
                result[i] ^= byte;
            }
        }

        Ok(result)
    }

    /// Verify DKD result consistency
    pub async fn verify_result(
        &self,
        derived_key: &[u8],
        expected_hash: &[u8; 32],
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let actual_hash = self.effects.blake3_hash(derived_key);
        Ok(actual_hash == *expected_hash)
    }
}

use frost_ed25519::keys::SecretShare;
use frost_ed25519::round1::{SigningCommitments, SigningNonces};
use frost_ed25519::round2::SignatureShare;
use frost_ed25519::{Identifier, SigningPackage};

/// Crypto bridge for FROST operations
pub struct FrostCryptoBridge {
    effects: Effects,
    key_packages: BTreeMap<Identifier, SecretShare>,
}

impl FrostCryptoBridge {
    pub fn new(effects: Effects, key_packages: BTreeMap<Identifier, SecretShare>) -> Self {
        Self {
            effects,
            key_packages,
        }
    }

    /// Get the key packages (for testing and choreography access)
    pub fn key_packages(&self) -> &BTreeMap<Identifier, SecretShare> {
        &self.key_packages
    }

    /// Generate signing nonces for FROST round 1
    pub async fn generate_nonces(
        &self,
        participant_id: Identifier,
    ) -> Result<SigningNonces, Box<dyn std::error::Error>> {
        let secret_share = self
            .key_packages
            .get(&participant_id)
            .ok_or("Secret share not found for participant")?;
        let signing_share = secret_share.signing_share();
        let mut rng = self.effects.rng();
        let (nonces, _commitments) = frost_ed25519::round1::commit(signing_share, &mut rng);
        Ok(nonces)
    }

    /// Create signing commitments from nonces
    pub async fn create_commitments(
        &self,
        nonces: &SigningNonces,
    ) -> Result<SigningCommitments, Box<dyn std::error::Error>> {
        Ok(nonces.commitments().clone())
    }

    /// Generate signature share for FROST round 2
    pub async fn generate_signature_share(
        &self,
        participant_id: Identifier,
        _nonces: &SigningNonces,
        _signing_package: &SigningPackage,
    ) -> Result<SignatureShare, Box<dyn std::error::Error>> {
        let _secret_share = self
            .key_packages
            .get(&participant_id)
            .ok_or("Secret share not found for participant")?;

        // TODO: Implement proper FROST round 2 signature share generation
        // This requires the signing_share from the secret_share
        // For now, return a placeholder error to be completed in integration phase
        Err("FROST signature share generation not yet implemented".into())
    }

    /// Aggregate FROST signature shares
    pub async fn aggregate_signature(
        &self,
        signing_package: &SigningPackage,
        signature_shares: &BTreeMap<Identifier, SignatureShare>,
        public_key_package: &frost_ed25519::keys::PublicKeyPackage,
    ) -> Result<frost_ed25519::Signature, Box<dyn std::error::Error>> {
        let group_signature =
            frost_ed25519::aggregate(signing_package, signature_shares, public_key_package)?;

        Ok(group_signature)
    }

    /// Verify FROST signature
    pub async fn verify_signature(
        &self,
        message: &[u8],
        signature: &frost_ed25519::Signature,
        public_key_package: &frost_ed25519::keys::PublicKeyPackage,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let group_public_key = public_key_package.verifying_key();

        // Use FROST verification directly
        Ok(group_public_key.verify(message, signature).is_ok())
    }
}
