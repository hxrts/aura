//! Pure synchronous hash trait for content addressing.
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
//! Current algorithm: **BLAKE3** (256-bit / 32-byte output)
//!
//! # Usage
//!
//! ```ignore
//! use aura_core::hash::hash;
//!
//! let digest = hash(b"hello world");
//! assert_eq!(digest.len(), 32); // 32-byte BLAKE3 digest
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
// It is explicitly allowed to use blake3 types and methods since this is where
// the hash algorithm is implemented.
#![allow(clippy::disallowed_types, clippy::disallowed_methods)]

use std::fmt;

/// Synchronous trait for cryptographic hashing.
pub trait HashAlgorithm: Send + Sync + fmt::Debug {
    /// Hash arbitrary bytes to a 32-byte digest.
    fn hash(&self, data: &[u8]) -> [u8; 32];

    /// Create an incremental hasher for multi-part hashing.
    fn hasher(&self) -> Box<dyn Hasher>;
}

/// Trait for incremental hashing of multi-part data.
pub trait Hasher: Send {
    /// Update the hasher with more data.
    fn update(&mut self, data: &[u8]);

    /// Finalize the hasher and return the 32-byte digest.
    fn finalize(self: Box<Self>) -> [u8; 32];
}

/// BLAKE3 hash implementation.
#[derive(Debug, Clone, Copy)]
pub struct Blake3Algorithm;

impl HashAlgorithm for Blake3Algorithm {
    fn hash(&self, data: &[u8]) -> [u8; 32] {
        *blake3::hash(data).as_bytes()
    }

    fn hasher(&self) -> Box<dyn Hasher> {
        Box::new(Blake3Hasher(blake3::Hasher::new()))
    }
}

/// BLAKE3 incremental hasher.
struct Blake3Hasher(blake3::Hasher);

impl Hasher for Blake3Hasher {
    fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }

    fn finalize(self: Box<Self>) -> [u8; 32] {
        *self.0.finalize().as_bytes()
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
/// pub const ALGORITHM: Blake3Algorithm = Blake3Algorithm;
/// ```
/// The global hash algorithm used throughout the system.
///
/// This is the single source of truth for which algorithm is used.
/// Modify this constant to change the algorithm system-wide.
pub const ALGORITHM: Blake3Algorithm = Blake3Algorithm;

/// Convenience function for hashing using the global algorithm.
#[inline]
pub fn hash(data: &[u8]) -> [u8; 32] {
    ALGORITHM.hash(data)
}

/// Convenience function for creating an incremental hasher.
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
        let hash1 = hash(data);

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
    fn test_blake3_implementation() {
        let algo = Blake3Algorithm;
        let hash1 = algo.hash(b"test");
        let hash2 = algo.hash(b"test");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_blake3_known_vector() {
        let empty_hash = hash(b"");
        let expected = [
            0xaf, 0x13, 0x49, 0xb9, 0xf5, 0xf9, 0xa1, 0xa6, 0xa0, 0x40, 0x4d, 0xea, 0x36, 0xdc,
            0xc9, 0x49, 0x9b, 0xcb, 0x25, 0xc9, 0xad, 0xc1, 0x12, 0xb7, 0xcc, 0x9a, 0x93, 0xca,
            0xe4, 0x1f, 0x32, 0x62,
        ];
        assert_eq!(empty_hash, expected, "BLAKE3 of empty string mismatch");
    }
}
