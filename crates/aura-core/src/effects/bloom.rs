//! Bloom Filter Effects Trait Definitions
//!
//! This module defines trait interfaces for Bloom filter operations used in
//! efficient set membership testing and data synchronization. Bloom filters
//! provide space-efficient probabilistic data structures for anti-entropy protocols.
//!
//! ## Use Cases
//!
//! - OpLog digest computation for sync protocols
//! - Efficient set difference calculation
//! - Membership testing with controlled false positive rates
//! - Privacy-preserving sync (metadata minimization)
//!
//! ## Security Properties
//!
//! - No false negatives (if element is in set, filter will detect it)
//! - Controlled false positive rate (configurable)
//! - No direct access to original data through filter

use crate::AuraError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Bloom filter operation error
pub type BloomError = AuraError;

/// Configuration for Bloom filter parameters
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BloomConfig {
    /// Expected number of elements to be inserted
    pub expected_elements: u64,
    /// Desired false positive probability (0.0 to 1.0)
    pub false_positive_rate: f64,
    /// Number of hash functions to use
    pub num_hash_functions: u32,
    /// Size of the bit vector in bits
    pub bit_vector_size: u64,
}

impl BloomConfig {
    /// Create optimal configuration for given parameters
    ///
    /// Automatically calculates optimal bit vector size and hash function count
    /// based on expected elements and desired false positive rate.
    pub fn optimal(expected_elements: u64, false_positive_rate: f64) -> Self {
        // Calculate optimal bit vector size: m = -n * ln(p) / (ln(2)^2)
        let n = expected_elements as f64;
        let p = false_positive_rate.max(0.00001).min(0.99999); // Clamp to reasonable range
        let m = (-n * p.ln() / (2.0_f64.ln().powi(2))).ceil() as u64;

        // Calculate optimal number of hash functions: k = (m/n) * ln(2)
        let k = ((m as f64 / n) * 2.0_f64.ln()).round() as u32;

        Self {
            expected_elements,
            false_positive_rate,
            num_hash_functions: k.max(1).min(32), // Reasonable bounds
            bit_vector_size: m.max(64),           // Minimum size
        }
    }

    /// Create a configuration for sync operations
    pub fn for_sync(expected_ops: u64) -> Self {
        // Use 1% false positive rate for sync operations
        Self::optimal(expected_ops, 0.01)
    }

    /// Create a configuration for small sets
    pub fn for_small_set(expected_elements: u64) -> Self {
        // Use 0.1% false positive rate for small sets
        Self::optimal(expected_elements, 0.001)
    }

    /// Validate configuration parameters
    pub fn validate(&self) -> Result<(), BloomError> {
        if self.expected_elements == 0 {
            return Err(BloomError::invalid("Expected elements must be > 0"));
        }
        if self.false_positive_rate <= 0.0 || self.false_positive_rate >= 1.0 {
            return Err(BloomError::invalid(
                "False positive rate must be between 0.0 and 1.0",
            ));
        }
        if self.num_hash_functions == 0 || self.num_hash_functions > 32 {
            return Err(BloomError::invalid(
                "Number of hash functions must be between 1 and 32",
            ));
        }
        if self.bit_vector_size < 64 {
            return Err(BloomError::invalid("Bit vector size must be at least 64"));
        }
        Ok(())
    }
}

/// Bloom filter data structure
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BloomFilter {
    /// Bit vector for the filter
    pub bits: Vec<u8>,
    /// Configuration parameters
    pub config: BloomConfig,
    /// Number of elements currently in the filter
    pub element_count: u64,
}

impl BloomFilter {
    /// Create a new empty bloom filter with the given configuration
    pub fn new(config: BloomConfig) -> Result<Self, BloomError> {
        config.validate()?;

        let byte_size = (config.bit_vector_size + 7) / 8; // Round up to byte boundary
        Ok(Self {
            bits: vec![0u8; byte_size as usize],
            config,
            element_count: 0,
        })
    }

    /// Get the size of the bit vector in bits
    pub fn bit_size(&self) -> u64 {
        self.config.bit_vector_size
    }

    /// Get the size of the bit vector in bytes
    pub fn byte_size(&self) -> u64 {
        self.bits.len() as u64
    }

    /// Check if the filter is empty
    pub fn is_empty(&self) -> bool {
        self.element_count == 0
    }

    /// Get the current false positive probability estimate
    pub fn current_false_positive_rate(&self) -> f64 {
        if self.element_count == 0 {
            return 0.0;
        }

        // Estimate: p ≈ (1 - e^(-kn/m))^k
        let k = self.config.num_hash_functions as f64;
        let n = self.element_count as f64;
        let m = self.config.bit_vector_size as f64;

        (1.0 - (-k * n / m).exp()).powf(k)
    }

    /// Check if the filter should be rebuilt (too many elements)
    pub fn should_rebuild(&self) -> bool {
        self.element_count > self.config.expected_elements * 2
            || self.current_false_positive_rate() > self.config.false_positive_rate * 2.0
    }
}

/// Bloom filter effects interface
///
/// This trait defines operations for creating and using Bloom filters for
/// efficient set membership testing and synchronization protocols.
///
/// # Implementation Notes
///
/// - Production: Hardware-optimized implementations with SIMD instructions
/// - Testing: Simple bit manipulation for deterministic results
/// - Simulation: Configurable false positive injection for testing edge cases
///
/// # Stability: EXPERIMENTAL
/// This API is under development and may change in future versions.
#[async_trait]
pub trait BloomEffects: Send + Sync {
    /// Create a new empty Bloom filter
    ///
    /// Creates a Bloom filter optimized for the given parameters.
    ///
    /// # Parameters
    /// - `config`: Configuration specifying filter parameters
    ///
    /// # Returns
    /// A new empty Bloom filter ready for element insertion
    async fn create_bloom_filter(&self, config: BloomConfig) -> Result<BloomFilter, BloomError>;

    /// Insert an element into a Bloom filter
    ///
    /// Adds an element to the filter by setting the appropriate bits.
    /// This operation is idempotent - inserting the same element multiple times
    /// has no additional effect.
    ///
    /// # Parameters
    /// - `filter`: Mutable reference to the Bloom filter
    /// - `element`: Element data to insert
    ///
    /// # Returns
    /// Updated filter with the element added
    async fn bloom_insert(
        &self,
        filter: &mut BloomFilter,
        element: &[u8],
    ) -> Result<(), BloomError>;

    /// Test if an element might be in the Bloom filter
    ///
    /// Tests for set membership. Returns:
    /// - `true`: Element might be in the set (could be false positive)
    /// - `false`: Element is definitely not in the set (no false negatives)
    ///
    /// # Parameters
    /// - `filter`: Bloom filter to test against
    /// - `element`: Element data to test
    ///
    /// # Returns
    /// `true` if element might be present, `false` if definitely absent
    async fn bloom_contains(
        &self,
        filter: &BloomFilter,
        element: &[u8],
    ) -> Result<bool, BloomError>;

    /// Compute the union of two Bloom filters
    ///
    /// Creates a new filter containing the union of elements from both filters.
    /// The resulting filter may have a higher false positive rate.
    ///
    /// # Parameters
    /// - `filter1`: First Bloom filter
    /// - `filter2`: Second Bloom filter (must have same configuration)
    ///
    /// # Returns
    /// New filter containing union of both input filters
    async fn bloom_union(
        &self,
        filter1: &BloomFilter,
        filter2: &BloomFilter,
    ) -> Result<BloomFilter, BloomError>;

    /// Estimate the number of elements in a Bloom filter
    ///
    /// Estimates the number of distinct elements that have been inserted
    /// based on the number of set bits in the filter.
    ///
    /// # Parameters
    /// - `filter`: Bloom filter to analyze
    ///
    /// # Returns
    /// Estimated number of elements in the filter
    async fn bloom_estimate_count(&self, filter: &BloomFilter) -> Result<u64, BloomError>;

    /// Create a Bloom filter from a set of elements
    ///
    /// Convenience method to create and populate a filter in one operation.
    ///
    /// # Parameters
    /// - `elements`: Elements to insert into the filter
    /// - `config`: Configuration for the filter
    ///
    /// # Returns
    /// New filter containing all the specified elements
    async fn bloom_from_elements(
        &self,
        elements: &[Vec<u8>],
        config: BloomConfig,
    ) -> Result<BloomFilter, BloomError>;

    /// Serialize a Bloom filter to bytes
    ///
    /// Converts a Bloom filter to a compact byte representation for network transmission.
    ///
    /// # Parameters
    /// - `filter`: Bloom filter to serialize
    ///
    /// # Returns
    /// Serialized filter data
    async fn bloom_serialize(&self, filter: &BloomFilter) -> Result<Vec<u8>, BloomError>;

    /// Deserialize a Bloom filter from bytes
    ///
    /// Reconstructs a Bloom filter from serialized data.
    ///
    /// # Parameters
    /// - `data`: Serialized filter data
    ///
    /// # Returns
    /// Reconstructed Bloom filter
    async fn bloom_deserialize(&self, data: &[u8]) -> Result<BloomFilter, BloomError>;

    /// Check if this implementation supports hardware acceleration
    fn supports_hardware_acceleration(&self) -> bool;

    /// Get implementation capabilities
    fn get_bloom_capabilities(&self) -> Vec<String>;
}

/// Helper functions for common Bloom filter operations

impl BloomConfig {
    /// Standard configuration for OpLog sync operations
    pub fn oplog_sync() -> Self {
        Self::optimal(1000, 0.01) // 1000 ops, 1% false positive rate
    }

    /// Configuration for peer discovery operations
    pub fn peer_discovery() -> Self {
        Self::optimal(100, 0.001) // 100 peers, 0.1% false positive rate
    }

    /// Configuration for chunk deduplication
    pub fn chunk_dedup() -> Self {
        Self::optimal(10000, 0.005) // 10k chunks, 0.5% false positive rate
    }
}

impl BloomFilter {
    /// Create a filter optimized for OpLog sync
    pub fn for_oplog_sync() -> Result<Self, BloomError> {
        Self::new(BloomConfig::oplog_sync())
    }

    /// Create a filter for peer discovery
    pub fn for_peer_discovery() -> Result<Self, BloomError> {
        Self::new(BloomConfig::peer_discovery())
    }

    /// Create a filter for chunk deduplication
    pub fn for_chunk_dedup() -> Result<Self, BloomError> {
        Self::new(BloomConfig::chunk_dedup())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bloom_config_optimal() {
        let config = BloomConfig::optimal(1000, 0.01);
        assert!(config.validate().is_ok());
        assert_eq!(config.expected_elements, 1000);
        assert!((config.false_positive_rate - 0.01).abs() < f64::EPSILON);
        assert!(config.num_hash_functions > 0);
        assert!(config.bit_vector_size >= 64);
    }

    #[test]
    fn test_bloom_config_validation() {
        // Valid config
        let valid_config = BloomConfig {
            expected_elements: 100,
            false_positive_rate: 0.01,
            num_hash_functions: 7,
            bit_vector_size: 1000,
        };
        assert!(valid_config.validate().is_ok());

        // Invalid expected_elements
        let invalid_config = BloomConfig {
            expected_elements: 0,
            ..valid_config.clone()
        };
        assert!(invalid_config.validate().is_err());

        // Invalid false_positive_rate
        let invalid_config = BloomConfig {
            false_positive_rate: 0.0,
            ..valid_config.clone()
        };
        assert!(invalid_config.validate().is_err());

        let invalid_config = BloomConfig {
            false_positive_rate: 1.0,
            ..valid_config.clone()
        };
        assert!(invalid_config.validate().is_err());
    }

    #[test]
    fn test_bloom_filter_creation() {
        let config = BloomConfig::optimal(100, 0.01);
        let filter = BloomFilter::new(config.clone()).unwrap();

        assert_eq!(filter.config, config);
        assert_eq!(filter.element_count, 0);
        assert!(filter.is_empty());
        assert!(filter.bit_size() >= 64);
    }

    #[test]
    fn test_bloom_config_presets() {
        let sync_config = BloomConfig::oplog_sync();
        assert!(sync_config.validate().is_ok());
        assert_eq!(sync_config.expected_elements, 1000);

        let peer_config = BloomConfig::peer_discovery();
        assert!(peer_config.validate().is_ok());
        assert_eq!(peer_config.expected_elements, 100);

        let chunk_config = BloomConfig::chunk_dedup();
        assert!(chunk_config.validate().is_ok());
        assert_eq!(chunk_config.expected_elements, 10000);
    }

    #[test]
    fn test_filter_presets() {
        let oplog_filter = BloomFilter::for_oplog_sync().unwrap();
        assert!(oplog_filter.is_empty());

        let peer_filter = BloomFilter::for_peer_discovery().unwrap();
        assert!(peer_filter.is_empty());

        let chunk_filter = BloomFilter::for_chunk_dedup().unwrap();
        assert!(chunk_filter.is_empty());
    }

    #[test]
    fn test_false_positive_rate_estimation() {
        let config = BloomConfig::optimal(100, 0.01);
        let mut filter = BloomFilter::new(config).unwrap();

        // Empty filter should have 0% false positive rate
        assert_eq!(filter.current_false_positive_rate(), 0.0);

        // Add some elements and check rate increases
        filter.element_count = 50;
        let rate_50 = filter.current_false_positive_rate();

        filter.element_count = 100;
        let rate_100 = filter.current_false_positive_rate();

        assert!(rate_100 > rate_50);
    }

    #[test]
    fn test_should_rebuild() {
        let config = BloomConfig::optimal(100, 0.01);
        let mut filter = BloomFilter::new(config).unwrap();

        // Empty filter shouldn't need rebuild
        assert!(!filter.should_rebuild());

        // Normal load shouldn't need rebuild
        filter.element_count = 100;
        assert!(!filter.should_rebuild());

        // Excessive load should trigger rebuild
        filter.element_count = 300;
        assert!(filter.should_rebuild());
    }
}
