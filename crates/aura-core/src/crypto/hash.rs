//! Pure synchronous hash trait for content addressing
//!
//! This module provides a synchronous trait-based hashing system. Unlike algebraic
//! effects (which are for operations with side effects), hashing is a pure,
//! deterministic operation that doesn't require the effect system.
//!
//! The trait design allows swapping hash algorithms while maintaining a single
//! source of truth for which algorithm is used throughout the codebase.
//!
//! # Design Philosophy
//!
//! - **Pure**: Hashing is deterministic - same input always produces same output
//! - **Synchronous**: No async overhead or runtime context needed
//! - **Single Source**: One place to change the algorithm if needed
//! - **Trait-Based**: Allows algorithm flexibility without the effect system
//!
//! # Algorithm Selection
//!
//! The hash algorithm is selected once at compile time via the `ALGORITHM` constant.
//! To change algorithms, modify the `ALGORITHM` declaration below. All code that uses
//! `hash()` or `hasher()` functions will automatically use the new algorithm without
//! any call-site changes.
//!
//! Current algorithm: **SHA-256** (256-bit / 32-byte output)
//!
//! # Usage
//!
//! ```ignore
//! use aura_core::hash::hash;
//!
//! let digest = hash(b"hello world");
//! assert_eq!(digest.len(), 32); // 32-byte SHA-256 hash
//! ```
//!
//! For incremental hashing:
//!
//! ```ignore
//! use aura_core::hash::hasher;
//!
//! let mut h = hasher();
//! h.update(b"hello");
//! h.update(b" ");
//! h.update(b"world");
//! let digest = h.finalize();
//! ```

// This module is the implementation of the centralized hash trait strategy.
// It is explicitly allowed to use sha2::Sha256 and related types/methods
// since this is where the hash algorithm is implemented.
#![allow(clippy::disallowed_types, clippy::disallowed_methods)]

use sha2::{Digest, Sha256};
use std::fmt;

/// Synchronous trait for cryptographic hashing
///
/// This trait defines the interface for content-addressing hashing.
///
/// Implementations should provide consistent, reliable hashing suitable
/// for content addressing, deduplication, and commitment schemes.
///
/// The algorithm used is determined by the `ALGORITHM` constant.
pub trait HashAlgorithm: Send + Sync + fmt::Debug {
    /// Hash arbitrary bytes to a 32-byte digest
    ///
    /// The output should be suitable for use as a cryptographic commitment.
    /// Different bytes should (with very high probability) produce different hashes.
    fn hash(&self, data: &[u8]) -> [u8; 32];

    /// Create an incremental hasher for multi-part hashing
    ///
    /// Useful when hashing large amounts of data or data provided in chunks.
    fn hasher(&self) -> Box<dyn Hasher>;
}

/// Trait for incremental hashing of multi-part data
///
/// Allows updating a hash computation with data provided in multiple chunks.
pub trait Hasher: Send {
    /// Update the hasher with more data
    fn update(&mut self, data: &[u8]);

    /// Finalize the hasher and return the 32-byte digest
    ///
    /// Consumes the hasher. The hasher cannot be used after finalization.
    fn finalize(self: Box<Self>) -> [u8; 32];
}

/// SHA-256 hash implementation
///
/// SHA-256 is a widely-used cryptographic hash function with the following properties:
/// - 256-bit (32-byte) output
/// - NIST FIPS 180-4 standard
/// - Suitable for content addressing and cryptographic commitments
/// - Part of the SHA-2 family
#[derive(Debug, Clone, Copy)]
pub struct Sha256Algorithm;

impl HashAlgorithm for Sha256Algorithm {
    fn hash(&self, data: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let result = hasher.finalize();
        let mut output = [0u8; 32];
        output.copy_from_slice(&result);
        output
    }

    fn hasher(&self) -> Box<dyn Hasher> {
        Box::new(Sha256Hasher(Sha256::new()))
    }
}

/// SHA-256 incremental hasher
struct Sha256Hasher(Sha256);

impl Hasher for Sha256Hasher {
    fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }

    fn finalize(self: Box<Self>) -> [u8; 32] {
        let result = self.0.finalize();
        let mut output = [0u8; 32];
        output.copy_from_slice(&result);
        output
    }
}

/// ============================================================================
/// ALGORITHM SELECTION: This is the single point where the hash algorithm
/// is declared for the entire system.
/// ============================================================================
///
/// To change the hash algorithm used throughout Aura:
/// 1. Implement `HashAlgorithm` for your new algorithm
/// 2. Change `ALGORITHM` to point to the new implementation
/// 3. All code using `hash()` and `hasher()` automatically uses the new algorithm
///
/// Example:
/// ```ignore
/// // To switch to Blake3:
/// pub const ALGORITHM: Blake3Algorithm = Blake3Algorithm;
/// ```
/// The global hash algorithm used throughout the system.
///
/// This is the single source of truth for which algorithm is used.
/// Modify this constant to change the algorithm system-wide.
///
/// Current: SHA-256 (NIST FIPS 180-4 standard)
pub const ALGORITHM: Sha256Algorithm = Sha256Algorithm;

// ============================================================================
// Public API - These functions use the algorithm selected above
// ============================================================================

/// Convenience function for hashing using the global algorithm
///
/// This is the primary way to hash data in the system.
/// Equivalent to calling `ALGORITHM.hash(data)`.
///
/// The algorithm used is determined by the `ALGORITHM` constant.
///
/// # Example
///
/// ```ignore
/// use aura_core::hash::hash;
///
/// let digest = hash(b"content");
/// assert_eq!(digest.len(), 32);
/// ```
#[inline]
pub fn hash(data: &[u8]) -> [u8; 32] {
    ALGORITHM.hash(data)
}

/// Convenience function for creating an incremental hasher
///
/// This creates a hasher using the global hash algorithm.
/// Equivalent to calling `ALGORITHM.hasher()`.
///
/// The algorithm used is determined by the `ALGORITHM` constant.
///
/// # Example
///
/// ```ignore
/// use aura_core::hash::hasher;
///
/// let mut h = hasher();
/// h.update(b"part1");
/// h.update(b"part2");
/// let digest = h.finalize();
/// ```
#[inline]
pub fn hasher() -> Box<dyn Hasher> {
    ALGORITHM.hasher()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_determinism() {
        let data = b"hello world";
        let hash1 = hash(data);
        let hash2 = hash(data);
        assert_eq!(hash1, hash2, "hash should be deterministic");
    }

    #[test]
    fn test_hash_length() {
        let hash = hash(b"test");
        assert_eq!(hash.len(), 32, "hash should be 32 bytes");
    }

    #[test]
    fn test_incremental_hasher_equivalence() {
        let data = b"hello world";

        // All at once
        let hash1 = hash(data);

        // Incremental
        let mut h = hasher();
        h.update(b"hello");
        h.update(b" ");
        h.update(b"world");
        let hash2 = h.finalize();

        assert_eq!(
            hash1, hash2,
            "incremental and direct hashing should produce same result"
        );
    }

    #[test]
    fn test_different_inputs_different_hashes() {
        let hash1 = hash(b"data1");
        let hash2 = hash(b"data2");
        assert_ne!(
            hash1, hash2,
            "different inputs should produce different hashes (with overwhelming probability)"
        );
    }

    #[test]
    fn test_empty_input() {
        let hash_empty = hash(b"");
        let hash_nonempty = hash(b"x");
        assert_ne!(
            hash_empty, hash_nonempty,
            "empty and non-empty inputs should produce different hashes"
        );
    }

    #[test]
    fn test_sha256_implementation() {
        // Verify SHA-256 implementation is consistent
        let algo = Sha256Algorithm;
        let hash1 = algo.hash(b"test");
        let hash2 = algo.hash(b"test");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_sha256_known_vector() {
        // Test against known SHA-256 vector
        // SHA256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        let empty_hash = hash(b"");
        let expected = [
            0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f,
            0xb9, 0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b,
            0x78, 0x52, 0xb8, 0x55,
        ];
        assert_eq!(empty_hash, expected, "SHA-256 of empty string mismatch");
    }
}
