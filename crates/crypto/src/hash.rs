//! Hash function abstractions for cryptographic operations
//!
//! Provides unified interfaces for Blake3 hashing used throughout Aura.

use blake3::Hasher;

/// Blake3 hash digest (32 bytes)
pub type Blake3Hash = [u8; 32];

/// Create a Blake3 hash of the input data
pub fn blake3_hash(data: &[u8]) -> Blake3Hash {
    let mut hasher = Hasher::new();
    hasher.update(data);
    *hasher.finalize().as_bytes()
}

/// Create a Blake3 hasher for incremental hashing
pub fn blake3_hasher() -> Hasher {
    Hasher::new()
}

/// Hash multiple data chunks with Blake3
pub fn blake3_hash_chunks(chunks: &[&[u8]]) -> Blake3Hash {
    let mut hasher = Hasher::new();
    for chunk in chunks {
        hasher.update(chunk);
    }
    *hasher.finalize().as_bytes()
}

/// Hash a string with Blake3
pub fn blake3_hash_string(s: &str) -> Blake3Hash {
    blake3_hash(s.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blake3_hash() {
        let data = b"hello world";
        let hash = blake3_hash(data);
        assert_eq!(hash.len(), 32);
        
        // Hash should be deterministic
        let hash2 = blake3_hash(data);
        assert_eq!(hash, hash2);
    }

    #[test]
    fn test_blake3_hash_chunks() {
        let chunks = vec![b"hello".as_slice(), b" ".as_slice(), b"world".as_slice()];
        let hash1 = blake3_hash_chunks(&chunks);
        let hash2 = blake3_hash(b"hello world");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_blake3_hash_string() {
        let s = "hello world";
        let hash1 = blake3_hash_string(s);
        let hash2 = blake3_hash(s.as_bytes());
        assert_eq!(hash1, hash2);
    }
}