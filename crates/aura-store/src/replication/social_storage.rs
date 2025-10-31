//! Social Storage via SSB Integration
//!
//! Integrates storage with SSB relationships to enable trust-based replica placement.
//! Once SSB establishes a relationship, storage can use that authenticated channel
//! for replica operations without additional handshakes.
//!
//! Reference: docs/040_storage.md Section 8.1 "SBB Integration Benefits"
//!          work/ssb_storage.md Phase 6.1

use serde::{Deserialize, Serialize};

/// Trust level for storage relationships
///
/// Represents the minimum trust level required for storage relationships.
/// Higher trust levels require stronger evidence of peer reliability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TrustLevel {
    /// No trust - peer must prove reliability through challenges
    Untrusted = 0,

    /// Low trust - peer has some interaction history
    Low = 1,

    /// Medium trust - peer has proven reliability
    Medium = 2,

    /// High trust - peer has long-term reliability record
    High = 3,

    /// Maximum trust - peer is part of inner circle
    Maximum = 4,
}

impl std::fmt::Display for TrustLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrustLevel::Untrusted => write!(f, "Untrusted"),
            TrustLevel::Low => write!(f, "Low"),
            TrustLevel::Medium => write!(f, "Medium"),
            TrustLevel::High => write!(f, "High"),
            TrustLevel::Maximum => write!(f, "Maximum"),
        }
    }
}

/// Storage operation types
///
/// Represents the various operations a peer can support for storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StorageOperation {
    /// Store new chunks
    Store,

    /// Retrieve stored chunks
    Retrieve,

    /// Delete chunks
    Delete,

    /// List stored chunks
    List,

    /// Get chunk metadata
    GetMetadata,
}

impl std::fmt::Display for StorageOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageOperation::Store => write!(f, "Store"),
            StorageOperation::Retrieve => write!(f, "Retrieve"),
            StorageOperation::Delete => write!(f, "Delete"),
            StorageOperation::List => write!(f, "List"),
            StorageOperation::GetMetadata => write!(f, "GetMetadata"),
        }
    }
}

/// Pricing information for storage services
///
/// Optional pricing model for future economic features (not yet implemented).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PricingInfo {
    /// Cost per MB per day in smallest unit
    pub cost_per_mb_per_day: u64,

    /// Currency code (e.g., "XRP", "ETH", "USD")
    pub currency: String,

    /// Whether pricing is negotiable
    pub negotiable: bool,
}

/// Storage capability announcement in SSB Offer envelopes
///
/// Announces what storage capabilities a peer supports, including capacity,
/// trust requirements, and rate limits. Used during SSB relationship
/// establishment to negotiate storage terms.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StorageCapabilityAnnouncement {
    /// Available storage capacity in bytes
    pub available_capacity_bytes: u64,

    /// Minimum trust level required for storage relationships
    pub min_trust_level: TrustLevel,

    /// Supported storage operations
    pub supported_operations: Vec<StorageOperation>,

    /// Maximum chunk size accepted
    pub max_chunk_size: u32,

    /// Rate limits (chunks per second)
    pub rate_limit_chunks_per_sec: u32,

    /// Whether this peer accepts new storage relationships
    pub accepting_new_relationships: bool,

    /// Optional pricing information (for future economic features)
    pub pricing: Option<PricingInfo>,
}

impl StorageCapabilityAnnouncement {
    /// Create a new storage capability announcement
    pub fn new(
        available_capacity_bytes: u64,
        min_trust_level: TrustLevel,
        max_chunk_size: u32,
    ) -> Self {
        Self {
            available_capacity_bytes,
            min_trust_level,
            supported_operations: vec![
                StorageOperation::Store,
                StorageOperation::Retrieve,
                StorageOperation::Delete,
                StorageOperation::List,
                StorageOperation::GetMetadata,
            ],
            max_chunk_size,
            rate_limit_chunks_per_sec: 100,
            accepting_new_relationships: true,
            pricing: None,
        }
    }

    /// Check if peer can accept a chunk of given size
    pub fn can_accept_chunk(&self, size: u32) -> bool {
        size <= self.max_chunk_size
    }

    /// Check if peer supports specific operation
    pub fn supports_operation(&self, operation: StorageOperation) -> bool {
        self.supported_operations.contains(&operation)
    }

    /// Check if peer is accepting new relationships
    pub fn is_available(&self) -> bool {
        self.accepting_new_relationships && self.available_capacity_bytes > 0
    }

    /// Get remaining available capacity
    pub fn remaining_capacity(&self) -> u64 {
        self.available_capacity_bytes
    }

    /// Mark peer as no longer accepting relationships
    pub fn close_to_new_relationships(&mut self) {
        self.accepting_new_relationships = false;
    }

    /// Reduce available capacity
    pub fn allocate_capacity(&mut self, bytes: u64) -> Result<(), String> {
        if bytes > self.available_capacity_bytes {
            return Err("Insufficient capacity".to_string());
        }
        self.available_capacity_bytes -= bytes;
        Ok(())
    }

    /// Increase available capacity (for testing/simulation)
    pub fn release_capacity(&mut self, bytes: u64) {
        self.available_capacity_bytes += bytes;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trust_level_ordering() {
        assert!(TrustLevel::Untrusted < TrustLevel::Low);
        assert!(TrustLevel::Low < TrustLevel::Medium);
        assert!(TrustLevel::Medium < TrustLevel::High);
        assert!(TrustLevel::High < TrustLevel::Maximum);
    }

    #[test]
    fn test_storage_capability_announcement() {
        let announcement = StorageCapabilityAnnouncement::new(
            1024 * 1024 * 1024, // 1 GB
            TrustLevel::Medium,
            4 * 1024 * 1024, // 4 MB max chunk
        );

        assert!(announcement.is_available());
        assert_eq!(announcement.remaining_capacity(), 1024 * 1024 * 1024);
        assert!(announcement.supports_operation(StorageOperation::Store));
    }

    #[test]
    fn test_can_accept_chunk() {
        let announcement =
            StorageCapabilityAnnouncement::new(1024 * 1024 * 1024, TrustLevel::Medium, 1024 * 1024);

        assert!(announcement.can_accept_chunk(512 * 1024));
        assert!(!announcement.can_accept_chunk(2 * 1024 * 1024));
    }

    #[test]
    fn test_allocate_capacity() {
        let mut announcement = StorageCapabilityAnnouncement::new(
            1024 * 1024 * 1024,
            TrustLevel::Medium,
            4 * 1024 * 1024,
        );

        announcement.allocate_capacity(512 * 1024 * 1024).unwrap();
        assert_eq!(announcement.remaining_capacity(), 512 * 1024 * 1024);

        // Should fail - not enough capacity
        let result = announcement.allocate_capacity(1024 * 1024 * 1024);
        assert!(result.is_err());
    }

    #[test]
    fn test_close_to_new_relationships() {
        let mut announcement = StorageCapabilityAnnouncement::new(
            1024 * 1024 * 1024,
            TrustLevel::Medium,
            4 * 1024 * 1024,
        );

        assert!(announcement.is_available());
        announcement.close_to_new_relationships();
        assert!(!announcement.is_available());
    }
}
