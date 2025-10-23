// Resharing cryptographic primitives (Shamir secret sharing and Lagrange interpolation)
//
// Reference: 080_architecture_protocol_integration.md - Part 4: P2P Resharing Across Layers
//
// This module implements the cryptographic building blocks for the resharing protocol:
// 1. Polynomial::from_secret(share, threshold) - Generate Shamir polynomial
// 2. polynomial.evaluate(participant_id) - Evaluate polynomial at point (sub-share generation)
// 3. Lagrange::interpolate(sub_shares, target_id) - Reconstruct share from sub-shares

use crate::{CryptoError, Result};
use curve25519_dalek::scalar::Scalar;
use rand::Rng;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Polynomial for Shamir secret sharing
///
/// Represents a polynomial f(x) = a_0 + a_1*x + a_2*x^2 + ... + a_{t-1}*x^{t-1}
/// where a_0 is the secret and coefficients are Ed25519 scalars
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct ShamirPolynomial {
    /// Coefficients [a_0, a_1, ..., a_{t-1}] where a_0 is the secret
    #[zeroize(skip)]
    coefficients: Vec<Scalar>,
}

impl ShamirPolynomial {
    /// Create polynomial from secret with given threshold
    ///
    /// Generates a random polynomial of degree (threshold - 1) with the secret as constant term
    ///
    /// Reference: 080 spec Part 4, Phase 1 (sub-share distribution)
    pub fn from_secret<R: Rng>(
        secret: Scalar,
        threshold: usize,
        rng: &mut R,
    ) -> Self {
        assert!(threshold > 0, "Threshold must be positive");
        
        let mut coefficients = vec![secret]; // a_0 = secret
        
        // Generate random coefficients a_1, ..., a_{threshold-1}
        for _ in 1..threshold {
            let mut bytes = [0u8; 32];
            rng.fill_bytes(&mut bytes);
            coefficients.push(Scalar::from_bytes_mod_order(bytes));
        }
        
        ShamirPolynomial { coefficients }
    }
    
    /// Evaluate polynomial at given x coordinate
    ///
    /// Computes f(x) = a_0 + a_1*x + a_2*x^2 + ... + a_{t-1}*x^{t-1}
    /// using Horner's method for efficiency
    ///
    /// Reference: 080 spec Part 4, Phase 1 (sub-share generation)
    pub fn evaluate(&self, x: Scalar) -> Scalar {
        // Horner's method: f(x) = a_0 + x(a_1 + x(a_2 + ...))
        let mut result = Scalar::ZERO;
        
        for coeff in self.coefficients.iter().rev() {
            result = result * x + coeff;
        }
        
        result
    }
    
    /// Get the secret (constant term)
    pub fn secret(&self) -> Scalar {
        self.coefficients[0]
    }
    
    /// Get the threshold (degree + 1)
    pub fn threshold(&self) -> usize {
        self.coefficients.len()
    }
}

/// A share point (participant_id, share_value)
#[derive(Clone, Copy, Debug)]
pub struct SharePoint {
    pub x: Scalar, // participant_id
    pub y: Scalar, // share value f(x)
}

/// Lagrange interpolation for share reconstruction
///
/// Given M-of-N sub-shares, reconstructs the secret using Lagrange interpolation
///
/// Reference: 080 spec Part 4, Phase 2 (share reconstruction)
pub struct LagrangeInterpolation;

impl LagrangeInterpolation {
    /// Interpolate shares to recover secret at x=0
    ///
    /// Given points (x_i, y_i), computes f(0) = sum_i(y_i * L_i(0))
    /// where L_i(0) is the Lagrange basis polynomial evaluated at 0
    ///
    /// Reference: 080 spec Part 4, Phase 2
    pub fn interpolate_at_zero(shares: &[SharePoint]) -> Result<Scalar> {
        if shares.is_empty() {
            return Err(CryptoError::InvalidKey(
                "Cannot interpolate with zero shares".to_string(),
            ));
        }
        
        let mut result = Scalar::ZERO;
        
        for (i, share_i) in shares.iter().enumerate() {
            // Compute Lagrange basis polynomial L_i(0)
            let mut basis = Scalar::ONE;
            
            for (j, share_j) in shares.iter().enumerate() {
                if i != j {
                    // L_i(0) *= (0 - x_j) / (x_i - x_j)
                    // Since we're evaluating at 0, this simplifies to:
                    // L_i(0) *= -x_j / (x_i - x_j)
                    
                    let numerator = -share_j.x;
                    let denominator = share_i.x - share_j.x;
                    
                    // Division in scalar field (multiply by inverse)
                    let denom_inv = denominator.invert();
                    basis *= numerator * denom_inv;
                }
            }
            
            result += share_i.y * basis;
        }
        
        Ok(result)
    }
    
    /// Interpolate shares to recover polynomial value at target_x
    ///
    /// Reconstructs f(target_x) from the given shares
    ///
    /// Reference: 080 spec Part 4, Phase 2 (general case)
    pub fn interpolate_at(shares: &[SharePoint], target_x: Scalar) -> Result<Scalar> {
        if shares.is_empty() {
            return Err(CryptoError::InvalidKey(
                "Cannot interpolate with zero shares".to_string(),
            ));
        }
        
        let mut result = Scalar::ZERO;
        
        for (i, share_i) in shares.iter().enumerate() {
            // Compute Lagrange basis polynomial L_i(target_x)
            let mut basis = Scalar::ONE;
            
            for (j, share_j) in shares.iter().enumerate() {
                if i != j {
                    // L_i(target_x) = prod_{j != i} (target_x - x_j) / (x_i - x_j)
                    let numerator = target_x - share_j.x;
                    let denominator = share_i.x - share_j.x;
                    let denom_inv = denominator.invert();
                    basis *= numerator * denom_inv;
                }
            }
            
            result += share_i.y * basis;
        }
        
        Ok(result)
    }
}

/// Convert participant ID to scalar for polynomial evaluation
///
/// Maps participant identifiers to field elements for Shamir secret sharing
pub fn participant_id_to_scalar(participant_id: u64) -> Scalar {
    Scalar::from(participant_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::thread_rng;
    
    #[test]
    fn test_polynomial_creation() {
        let mut rng = thread_rng();
        let secret = Scalar::from(42u64);
        let poly = ShamirPolynomial::from_secret(secret, 3, &mut rng);
        
        assert_eq!(poly.secret(), secret);
        assert_eq!(poly.threshold(), 3);
    }
    
    #[test]
    fn test_polynomial_evaluate_at_zero() {
        let mut rng = thread_rng();
        let secret = Scalar::from(123u64);
        let poly = ShamirPolynomial::from_secret(secret, 3, &mut rng);
        
        // f(0) should equal the secret (constant term)
        let result = poly.evaluate(Scalar::ZERO);
        assert_eq!(result, secret);
    }
    
    #[test]
    fn test_polynomial_deterministic_evaluation() {
        let mut rng = thread_rng();
        let secret = Scalar::from(456u64);
        let poly = ShamirPolynomial::from_secret(secret, 2, &mut rng);
        
        let x = Scalar::from(5u64);
        let y1 = poly.evaluate(x);
        let y2 = poly.evaluate(x);
        
        assert_eq!(y1, y2, "Polynomial evaluation should be deterministic");
    }
    
    #[test]
    fn test_shamir_secret_sharing_reconstruction() {
        let mut rng = thread_rng();
        let secret = Scalar::from(999u64);
        let threshold = 3;
        let num_shares = 5;
        
        // Create polynomial and generate shares
        let poly = ShamirPolynomial::from_secret(secret, threshold, &mut rng);
        
        let mut all_shares = Vec::new();
        for i in 1..=num_shares {
            let x = Scalar::from(i as u64);
            let y = poly.evaluate(x);
            all_shares.push(SharePoint { x, y });
        }
        
        // Reconstruct secret from threshold shares (shares 1, 2, 3)
        let threshold_shares = &all_shares[0..threshold];
        let reconstructed = LagrangeInterpolation::interpolate_at_zero(threshold_shares)
            .expect("Reconstruction should succeed");
        
        assert_eq!(reconstructed, secret, "Should reconstruct original secret");
    }
    
    #[test]
    fn test_shamir_any_threshold_subset_works() {
        let mut rng = thread_rng();
        let secret = Scalar::from(777u64);
        let threshold = 3;
        let num_shares = 5;
        
        let poly = ShamirPolynomial::from_secret(secret, threshold, &mut rng);
        
        let mut all_shares = Vec::new();
        for i in 1..=num_shares {
            let x = Scalar::from(i as u64);
            let y = poly.evaluate(x);
            all_shares.push(SharePoint { x, y });
        }
        
        // Try different combinations of threshold shares
        // Shares 1,2,3
        let subset1 = vec![all_shares[0], all_shares[1], all_shares[2]];
        let result1 = LagrangeInterpolation::interpolate_at_zero(&subset1).unwrap();
        assert_eq!(result1, secret);
        
        // Shares 2,3,4
        let subset2 = vec![all_shares[1], all_shares[2], all_shares[3]];
        let result2 = LagrangeInterpolation::interpolate_at_zero(&subset2).unwrap();
        assert_eq!(result2, secret);
        
        // Shares 1,3,5
        let subset3 = vec![all_shares[0], all_shares[2], all_shares[4]];
        let result3 = LagrangeInterpolation::interpolate_at_zero(&subset3).unwrap();
        assert_eq!(result3, secret);
    }
    
    #[test]
    fn test_shamir_insufficient_shares_fails() {
        let mut rng = thread_rng();
        let secret = Scalar::from(888u64);
        let threshold = 3;
        
        let poly = ShamirPolynomial::from_secret(secret, threshold, &mut rng);
        
        // Generate only 2 shares (below threshold)
        let shares = vec![
            SharePoint {
                x: Scalar::from(1u64),
                y: poly.evaluate(Scalar::from(1u64)),
            },
            SharePoint {
                x: Scalar::from(2u64),
                y: poly.evaluate(Scalar::from(2u64)),
            },
        ];
        
        // With only 2 shares for a 3-threshold scheme, we can still interpolate
        // but the result will be wrong (interpolates a degree-1 polynomial)
        let result = LagrangeInterpolation::interpolate_at_zero(&shares).unwrap();
        assert_ne!(result, secret, "Insufficient shares should not recover correct secret");
    }
    
    #[test]
    fn test_lagrange_interpolate_at_arbitrary_point() {
        let mut rng = thread_rng();
        let secret = Scalar::from(555u64);
        let threshold = 3;
        
        let poly = ShamirPolynomial::from_secret(secret, threshold, &mut rng);
        
        // Generate shares at points 1, 2, 3
        let shares = vec![
            SharePoint {
                x: Scalar::from(1u64),
                y: poly.evaluate(Scalar::from(1u64)),
            },
            SharePoint {
                x: Scalar::from(2u64),
                y: poly.evaluate(Scalar::from(2u64)),
            },
            SharePoint {
                x: Scalar::from(3u64),
                y: poly.evaluate(Scalar::from(3u64)),
            },
        ];
        
        // Reconstruct value at x=5
        let target_x = Scalar::from(5u64);
        let expected_y = poly.evaluate(target_x);
        let reconstructed_y = LagrangeInterpolation::interpolate_at(&shares, target_x).unwrap();
        
        assert_eq!(reconstructed_y, expected_y, "Should reconstruct correct value at arbitrary point");
    }
    
    #[test]
    fn test_participant_id_to_scalar() {
        let id1 = participant_id_to_scalar(1);
        let id2 = participant_id_to_scalar(2);
        let id100 = participant_id_to_scalar(100);
        
        assert_eq!(id1, Scalar::from(1u64));
        assert_eq!(id2, Scalar::from(2u64));
        assert_eq!(id100, Scalar::from(100u64));
        
        assert_ne!(id1, id2);
    }
    
    #[test]
    fn test_full_resharing_simulation() {
        // Simulate resharing from 3 old participants to 4 new participants
        // with threshold 2
        
        let mut rng = thread_rng();
        let original_secret = Scalar::from(12345u64);
        
        // Phase 1: Each old participant has a share of the original secret
        // (We simulate this by just using the original secret directly)
        
        // Phase 2: Each old participant creates a polynomial with their share
        // and distributes sub-shares to new participants
        let threshold = 2;
        let poly = ShamirPolynomial::from_secret(original_secret, threshold, &mut rng);
        
        // Old participant 1 evaluates their polynomial at new participant IDs 1,2,3,4
        let sub_share_1_to_1 = poly.evaluate(Scalar::from(1u64));
        let sub_share_1_to_2 = poly.evaluate(Scalar::from(2u64));
        let _sub_share_1_to_3 = poly.evaluate(Scalar::from(3u64));
        let _sub_share_1_to_4 = poly.evaluate(Scalar::from(4u64));
        
        // In real resharing, we'd have sub-shares from multiple old participants
        // For this test, we'll just verify that we can reconstruct the polynomial value
        
        // New participant 1 collects sub-shares from old participants and reconstructs
        // (In this simplified test, we only have one old participant, so we just use their share)
        let _new_share_1 = sub_share_1_to_1;
        
        // Verify that the secret can be recovered
        // (In real scenario, we'd need threshold new participants to reconstruct)
        let shares_for_reconstruction = vec![
            SharePoint { x: Scalar::from(1u64), y: sub_share_1_to_1 },
            SharePoint { x: Scalar::from(2u64), y: sub_share_1_to_2 },
        ];
        
        let reconstructed = LagrangeInterpolation::interpolate_at_zero(&shares_for_reconstruction).unwrap();
        assert_eq!(reconstructed, original_secret);
    }
}

