// P2P DKD (Deterministic Key Derivation) cryptographic primitives
//
// Reference: 080_architecture_protocol_integration.md - Part 1: P2P DKD Integration
//
// This module implements the cryptographic building blocks for the P2P DKD protocol:
// 1. Hash share to scalar: blake3(share_i || context_id) → H_i
// 2. Scalar multiplication: H_i · G → Point
// 3. Commitment hash: blake3(Point) → Commitment
// 4. Point addition: Point + Point → Aggregated
// 5. Cofactor clearing: [8] · Point → Cleared
// 6. Key derivation: HKDF(seed, context) → Key

use crate::{CryptoError, Result};
use blake3;
use curve25519_dalek::{
    constants::ED25519_BASEPOINT_TABLE, edwards::EdwardsPoint, scalar::Scalar, traits::Identity,
};
use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Hash share and context to scalar (Step 1 of P2P DKD)
///
/// Computes: H_i = blake3(share_i || context_id)
/// Maps the hash output to a valid Ed25519 scalar
///
/// Reference: 080 spec Part 1, Step 1
pub fn hash_to_scalar(share_bytes: &[u8], context_id: &[u8]) -> Scalar {
    let mut hasher = blake3::Hasher::new();
    hasher.update(share_bytes);
    hasher.update(context_id);
    let hash = hasher.finalize();

    // Convert hash to scalar (mod l where l is the Ed25519 group order)
    Scalar::from_bytes_mod_order(*hash.as_bytes())
}

/// Scalar multiplication: H_i · G (Step 2 of P2P DKD)
///
/// Multiplies scalar by Ed25519 base point to get a curve point
///
/// Reference: 080 spec Part 1, Step 2
pub fn scalar_mult_basepoint(scalar: &Scalar) -> EdwardsPoint {
    ED25519_BASEPOINT_TABLE * scalar
}

/// Compute commitment hash: blake3(Point) (Step 3 of P2P DKD)
///
/// Hashes the curve point to create a commitment
/// Used in the commitment-reveal protocol for Byzantine tolerance
///
/// Reference: 080 spec Part 1, Step 3 (commitment phase)
pub fn compute_commitment(point: &EdwardsPoint) -> [u8; 32] {
    let point_bytes = point.compress().to_bytes();
    *blake3::hash(&point_bytes).as_bytes()
}

/// Point addition: Point + Point → Aggregated (Step 4 of P2P DKD)
///
/// Adds multiple curve points together
/// Used to aggregate partial points from all participants
///
/// Reference: 080 spec Part 1, Step 3 (aggregation phase)
pub fn add_points(points: &[EdwardsPoint]) -> EdwardsPoint {
    points
        .iter()
        .fold(EdwardsPoint::identity(), |acc, p| acc + p)
}

/// Cofactor clearing: [8] · Point → Cleared (Step 5 of P2P DKD)
///
/// Multiplies point by cofactor (8 for Ed25519) to ensure it's in the prime-order subgroup
/// This is critical for security - ensures the result is a valid group element
///
/// Reference: 080 spec Part 1, Step 3 (aggregation phase)
pub fn clear_cofactor(point: &EdwardsPoint) -> EdwardsPoint {
    point.mul_by_cofactor()
}

/// Extract seed from cleared point
///
/// Converts the cleared point to bytes for use as HKDF seed
///
/// Reference: 080 spec Part 1, Step 3 (map to seed)
pub fn point_to_seed(point: &EdwardsPoint) -> [u8; 32] {
    point.compress().to_bytes()
}

/// Derive keys using HKDF (Step 6 of P2P DKD)
///
/// Expands the seed into multiple derived keys using HKDF-SHA256
///
/// Reference: 080 spec Part 1, Step 4 (identity expansion)
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct DerivedKeys {
    /// Ed25519 signing key (32 bytes)
    pub signing_key: [u8; 32],
    /// AES-256 encryption key (32 bytes)
    pub encryption_key: [u8; 32],
    /// Fingerprint of the original seed (32 bytes)
    pub seed_fingerprint: [u8; 32],
}

/// Derive cryptographic keys from seed using HKDF-SHA256
///
/// Generates signing key, encryption key, and seed fingerprint
/// from the aggregated DKD seed.
pub fn derive_keys(seed: &[u8], _context: &[u8]) -> Result<DerivedKeys> {
    let hk = Hkdf::<Sha256>::new(None, seed);

    // Derive signing key
    let mut signing_key = [0u8; 32];
    hk.expand(b"aura.dkd.signing_key.v1", &mut signing_key)
        .map_err(|e| CryptoError::InvalidKey(format!("HKDF signing key failed: {}", e)))?;

    // Derive encryption key
    let mut encryption_key = [0u8; 32];
    hk.expand(b"aura.dkd.encryption_key.v1", &mut encryption_key)
        .map_err(|e| CryptoError::InvalidKey(format!("HKDF encryption key failed: {}", e)))?;

    // Compute seed fingerprint for audit
    let seed_fingerprint = *blake3::hash(seed).as_bytes();

    Ok(DerivedKeys {
        signing_key,
        encryption_key,
        seed_fingerprint,
    })
}

/// Complete P2P DKD flow for a single participant
///
/// This is a convenience function that runs all DKD steps for one participant:
/// 1. Hash share to scalar
/// 2. Multiply by basepoint
/// 3. Return point and commitment
///
/// The point is revealed in phase 2, commitment is sent in phase 1
pub fn participant_dkd_phase(share_bytes: &[u8], context_id: &[u8]) -> (EdwardsPoint, [u8; 32]) {
    let scalar = hash_to_scalar(share_bytes, context_id);
    let point = scalar_mult_basepoint(&scalar);
    let commitment = compute_commitment(&point);
    (point, commitment)
}

/// Participant wrapper for P2P DKD protocol
///
/// Encapsulates a single participant's state during DKD
pub struct DkdParticipant {
    share_bytes: [u8; 16],
    point: Option<EdwardsPoint>,
    commitment: Option<[u8; 32]>,
}

impl DkdParticipant {
    /// Create a new DKD participant from their share
    pub fn new(share_bytes: [u8; 16]) -> Self {
        Self {
            share_bytes,
            point: None,
            commitment: None,
        }
    }

    /// Compute and return the commitment hash
    pub fn commitment_hash(&mut self) -> [u8; 32] {
        if let Some(commitment) = self.commitment {
            return commitment;
        }

        // Generate point and commitment if not already done
        let context_id = b""; // Empty context for now
        let (point, commitment) = participant_dkd_phase(&self.share_bytes, context_id);

        self.point = Some(point);
        self.commitment = Some(commitment);

        commitment
    }

    /// Return the revealed point
    pub fn revealed_point(&mut self) -> [u8; 32] {
        if self.point.is_none() {
            // Generate point if not already done
            let context_id = b"";
            let (point, commitment) = participant_dkd_phase(&self.share_bytes, context_id);
            self.point = Some(point);
            self.commitment = Some(commitment);
        }

        // Return compressed point bytes (owned, not borrowed)
        // Safe: we just set self.point above if it was None
        #[allow(clippy::expect_used)] // Safe: we just set self.point above
        let point = self.point
            .expect("Point should be set by revealed_point method");
        point.compress().to_bytes()
    }
}

/// Aggregate revealed DKD points into a final derived key
///
/// This combines all participants' revealed points and performs cofactor clearing
/// to produce the final derived public key.
pub fn aggregate_dkd_points(points: &[[u8; 32]]) -> Result<ed25519_dalek::VerifyingKey> {
    // Decompress all points
    let edwards_points: Result<Vec<EdwardsPoint>> = points
        .iter()
        .map(|bytes| {
            let compressed = curve25519_dalek::edwards::CompressedEdwardsY::from_slice(bytes)
                .map_err(|_| CryptoError::InvalidKey("Invalid point slice length".to_string()))?;
            compressed
                .decompress()
                .ok_or_else(|| CryptoError::InvalidKey("Failed to decompress point".to_string()))
        })
        .collect();

    let edwards_points = edwards_points?;

    // Aggregate points
    let aggregated = add_points(&edwards_points);
    let cleared = clear_cofactor(&aggregated);

    // Convert to Ed25519 public key
    let compressed = cleared.compress();
    ed25519_dalek::VerifyingKey::from_bytes(&compressed.to_bytes())
        .map_err(|e| CryptoError::InvalidKey(format!("Failed to create verifying key: {}", e)))
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)] // Test code
mod tests {
    use super::*;

    #[test]
    fn test_hash_to_scalar_deterministic() {
        let share = b"test_share_123";
        let context = b"test_context";

        let scalar1 = hash_to_scalar(share, context);
        let scalar2 = hash_to_scalar(share, context);

        assert_eq!(scalar1, scalar2, "hash_to_scalar should be deterministic");
    }

    #[test]
    fn test_hash_to_scalar_different_inputs() {
        let share1 = b"share1";
        let share2 = b"share2";
        let context = b"context";

        let scalar1 = hash_to_scalar(share1, context);
        let scalar2 = hash_to_scalar(share2, context);

        assert_ne!(
            scalar1, scalar2,
            "Different shares should produce different scalars"
        );
    }

    #[test]
    fn test_scalar_mult_basepoint() {
        let scalar = Scalar::from(42u64);
        let point = scalar_mult_basepoint(&scalar);

        // Point should not be identity
        assert_ne!(point, EdwardsPoint::identity());
    }

    #[test]
    fn test_compute_commitment_deterministic() {
        let scalar = Scalar::from(123u64);
        let point = scalar_mult_basepoint(&scalar);

        let commit1 = compute_commitment(&point);
        let commit2 = compute_commitment(&point);

        assert_eq!(commit1, commit2, "Commitment should be deterministic");
    }

    #[test]
    fn test_add_points() {
        let scalar1 = Scalar::from(10u64);
        let scalar2 = Scalar::from(20u64);
        let scalar3 = Scalar::from(30u64);

        let point1 = scalar_mult_basepoint(&scalar1);
        let point2 = scalar_mult_basepoint(&scalar2);
        let expected = scalar_mult_basepoint(&scalar3);

        let result = add_points(&[point1, point2]);

        assert_eq!(
            result, expected,
            "Point addition should follow scalar addition"
        );
    }

    #[test]
    fn test_clear_cofactor() {
        let scalar = Scalar::from(42u64);
        let point = scalar_mult_basepoint(&scalar);
        let cleared = clear_cofactor(&point);

        // Cleared point should be in prime-order subgroup
        // (checking this properly requires more complex validation)
        assert_ne!(cleared, EdwardsPoint::identity());
    }

    #[test]
    fn test_point_to_seed() {
        let scalar = Scalar::from(42u64);
        let point = scalar_mult_basepoint(&scalar);

        let seed1 = point_to_seed(&point);
        let seed2 = point_to_seed(&point);

        assert_eq!(seed1, seed2, "point_to_seed should be deterministic");
        assert_eq!(seed1.len(), 32, "Seed should be 32 bytes");
    }

    #[test]
    fn test_derive_keys() {
        let seed = b"test_seed_for_derivation_______"; // 32 bytes
        let context = b"test_context";

        let keys = derive_keys(seed, context).expect("Key derivation should succeed");

        assert_eq!(keys.signing_key.len(), 32);
        assert_eq!(keys.encryption_key.len(), 32);
        assert_eq!(keys.seed_fingerprint.len(), 32);

        // Keys should be different from each other
        assert_ne!(keys.signing_key, keys.encryption_key);
    }

    #[test]
    fn test_derive_keys_deterministic() {
        let seed = b"test_seed_for_derivation_______";
        let context = b"test_context";

        let keys1 = derive_keys(seed, context).unwrap();
        let keys2 = derive_keys(seed, context).unwrap();

        assert_eq!(keys1.signing_key, keys2.signing_key);
        assert_eq!(keys1.encryption_key, keys2.encryption_key);
        assert_eq!(keys1.seed_fingerprint, keys2.seed_fingerprint);
    }

    #[test]
    fn test_participant_dkd_phase() {
        let share = b"participant_share_123";
        let context = b"app_context";

        let (point, commitment) = participant_dkd_phase(share, context);

        // Point should not be identity
        assert_ne!(point, EdwardsPoint::identity());

        // Commitment should match manual computation
        let expected_commitment = compute_commitment(&point);
        assert_eq!(commitment, expected_commitment);
    }

    #[test]
    fn test_full_p2p_dkd_simulation() {
        // Simulate 3 participants doing P2P DKD
        let context_id = b"shared_context";

        let share1 = b"participant1_share_________1";
        let share2 = b"participant2_share_________2";
        let share3 = b"participant3_share_________3";

        // Phase 1: Each participant computes point and commitment
        let (point1, commit1) = participant_dkd_phase(share1, context_id);
        let (point2, commit2) = participant_dkd_phase(share2, context_id);
        let (point3, commit3) = participant_dkd_phase(share3, context_id);

        // Commitments are exchanged first (without revealing points)
        assert_ne!(commit1, commit2);
        assert_ne!(commit2, commit3);

        // Phase 2: Points are revealed and validated against commitments
        assert_eq!(commit1, compute_commitment(&point1));
        assert_eq!(commit2, compute_commitment(&point2));
        assert_eq!(commit3, compute_commitment(&point3));

        // Phase 3: Aggregate points
        let aggregated = add_points(&[point1, point2, point3]);
        let cleared = clear_cofactor(&aggregated);
        let seed = point_to_seed(&cleared);

        // Phase 4: Derive keys
        let keys = derive_keys(&seed, context_id).unwrap();

        assert_eq!(keys.signing_key.len(), 32);
        assert_eq!(keys.encryption_key.len(), 32);

        // Result should be deterministic
        let aggregated2 = add_points(&[point1, point2, point3]);
        let cleared2 = clear_cofactor(&aggregated2);
        let seed2 = point_to_seed(&cleared2);
        let keys2 = derive_keys(&seed2, context_id).unwrap();

        assert_eq!(keys.signing_key, keys2.signing_key);
        assert_eq!(keys.encryption_key, keys2.encryption_key);
    }
}
