//! Capability-Blinded Manifests
//!
//! Implements privacy-preserving device capability advertisement where only
//! feature buckets and hashes are revealed publicly, with full details
//! revealed lazily in trusted contexts.

use blake3::{Hash, Hasher};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Capability bucket categories for blinded advertisement
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CapabilityBucket {
    /// Basic communication capabilities
    Communication,
    /// Storage capabilities (without capacity details)
    Storage,
    /// Relay/routing capabilities
    Relay,
    /// Computational capabilities
    Compute,
    /// Guardian/recovery capabilities
    Guardian,
    /// Special protocol support
    Protocol,
}

/// Blinded manifest that reveals only capability buckets and feature hashes
///
/// This prevents neighbors from learning exact protocol versions, storage
/// capacities, or other sensitive metadata while still enabling discovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlindedManifest {
    /// Supported capability buckets (coarse-grained)
    pub capability_buckets: BTreeSet<CapabilityBucket>,

    /// Hash of detailed capabilities (for verification after reveal)
    pub capability_hash: [u8; 32],

    /// Feature bucket hashes (protocol support without version details)
    pub feature_buckets: Vec<FeatureBucket>,

    /// Padding to ensure uniform size (privacy protection)
    pub padding: Vec<u8>,

    /// Manifest version for compatibility
    pub version: u16,
}

/// Feature bucket with blinded protocol support information
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureBucket {
    /// Category of features
    pub category: String,

    /// Hash of supported features in this category
    pub features_hash: [u8; 32],

    /// Approximate count (bucketized for privacy)
    pub feature_count_bucket: FeatureCountBucket,
}

/// Bucketized feature counts to prevent exact enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FeatureCountBucket {
    /// 0 features
    None,
    /// 1-3 features  
    Few,
    /// 4-10 features
    Some,
    /// 11+ features
    Many,
}

impl FeatureCountBucket {
    /// Convert exact count to privacy-preserving bucket
    pub fn from_count(count: usize) -> Self {
        match count {
            0 => Self::None,
            1..=3 => Self::Few,
            4..=10 => Self::Some,
            _ => Self::Many,
        }
    }
}

impl BlindedManifest {
    /// Target size for padded manifests (ensures uniform network signatures)
    pub const TARGET_SIZE: usize = 512;

    /// Create a blinded manifest from raw capability data
    pub fn from_capabilities(
        capability_buckets: BTreeSet<CapabilityBucket>,
        detailed_capabilities: &[u8], // Serialized full capability data
        feature_categories: Vec<(String, Vec<String>)>, // (category, features)
    ) -> Self {
        // Hash the detailed capabilities for verification
        let capability_hash = *blake3::hash(detailed_capabilities).as_bytes();

        // Create feature buckets with hashed feature lists
        let feature_buckets: Vec<FeatureBucket> = feature_categories
            .into_iter()
            .map(|(category, features)| {
                let mut hasher = Hasher::new();
                hasher.update(category.as_bytes());
                for feature in &features {
                    hasher.update(feature.as_bytes());
                }

                FeatureBucket {
                    category,
                    features_hash: *hasher.finalize().as_bytes(),
                    feature_count_bucket: FeatureCountBucket::from_count(features.len()),
                }
            })
            .collect();

        // Serialize core manifest data
        let core_manifest = CoreManifest {
            capability_buckets: capability_buckets.clone(),
            capability_hash,
            feature_buckets: feature_buckets.clone(),
            version: 1,
        };

        let core_size = bincode::serialize(&core_manifest)
            .map(|data| data.len())
            .unwrap_or(0);

        // Calculate padding needed to reach target size
        let padding_size = Self::TARGET_SIZE.saturating_sub(core_size);
        let padding = vec![0u8; padding_size];

        Self {
            capability_buckets,
            capability_hash,
            feature_buckets,
            padding,
            version: 1,
        }
    }

    /// Verify that revealed capabilities match the blinded manifest
    pub fn verify_reveal(&self, revealed_capabilities: &[u8]) -> bool {
        blake3::hash(revealed_capabilities) == self.capability_hash
    }

    /// Check if this manifest supports a specific capability bucket
    pub fn supports_bucket(&self, bucket: CapabilityBucket) -> bool {
        self.capability_buckets.contains(&bucket)
    }

    /// Get feature bucket for a specific category
    pub fn get_feature_bucket(&self, category: &str) -> Option<&FeatureBucket> {
        self.feature_buckets
            .iter()
            .find(|bucket| bucket.category == category)
    }

    /// Estimate total feature count across all categories
    pub fn estimate_total_features(&self) -> EstimatedFeatureCount {
        let total_buckets: usize = self
            .feature_buckets
            .iter()
            .map(|bucket| {
                match bucket.feature_count_bucket {
                    FeatureCountBucket::None => 0,
                    FeatureCountBucket::Few => 2,   // Midpoint estimate
                    FeatureCountBucket::Some => 7,  // Midpoint estimate
                    FeatureCountBucket::Many => 15, // Conservative estimate
                }
            })
            .sum();

        EstimatedFeatureCount::from_total(total_buckets)
    }
}

/// Core manifest data (for size calculation)
#[derive(Serialize)]
struct CoreManifest {
    capability_buckets: BTreeSet<CapabilityBucket>,
    capability_hash: [u8; 32],
    feature_buckets: Vec<FeatureBucket>,
    version: u16,
}

/// Estimated feature count for peer selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EstimatedFeatureCount {
    /// Very few features (0-5)
    Minimal,
    /// Basic feature set (6-15)
    Basic,
    /// Standard feature set (16-30)
    Standard,
    /// Rich feature set (31+)
    Rich,
}

impl EstimatedFeatureCount {
    fn from_total(count: usize) -> Self {
        match count {
            0..=5 => Self::Minimal,
            6..=15 => Self::Basic,
            16..=30 => Self::Standard,
            _ => Self::Rich,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blinded_manifest_creation() {
        let mut buckets = BTreeSet::new();
        buckets.insert(CapabilityBucket::Communication);
        buckets.insert(CapabilityBucket::Storage);

        let capabilities = b"detailed capability data";
        let features = vec![
            (
                "protocols".to_string(),
                vec!["dkd".to_string(), "frost".to_string()],
            ),
            ("transports".to_string(), vec!["tcp".to_string()]),
        ];

        let manifest = BlindedManifest::from_capabilities(buckets, capabilities, features);

        assert!(manifest.supports_bucket(CapabilityBucket::Communication));
        assert!(manifest.supports_bucket(CapabilityBucket::Storage));
        assert!(!manifest.supports_bucket(CapabilityBucket::Relay));

        assert_eq!(manifest.feature_buckets.len(), 2);
        assert_eq!(manifest.version, 1);

        // Verify capability hash
        assert!(manifest.verify_reveal(capabilities));
        assert!(!manifest.verify_reveal(b"wrong data"));
    }

    #[test]
    fn test_feature_count_buckets() {
        assert_eq!(FeatureCountBucket::from_count(0), FeatureCountBucket::None);
        assert_eq!(FeatureCountBucket::from_count(2), FeatureCountBucket::Few);
        assert_eq!(FeatureCountBucket::from_count(7), FeatureCountBucket::Some);
        assert_eq!(FeatureCountBucket::from_count(15), FeatureCountBucket::Many);
    }

    #[test]
    fn test_manifest_size_consistency() {
        // Different manifests should have the same serialized size due to padding
        let buckets1 = [CapabilityBucket::Communication].into_iter().collect();
        let buckets2 = [
            CapabilityBucket::Communication,
            CapabilityBucket::Storage,
            CapabilityBucket::Relay,
        ]
        .into_iter()
        .collect();

        let manifest1 = BlindedManifest::from_capabilities(
            buckets1,
            b"small",
            vec![("test".to_string(), vec!["a".to_string()])],
        );

        let manifest2 = BlindedManifest::from_capabilities(
            buckets2,
            b"much larger capability description",
            vec![
                (
                    "protocols".to_string(),
                    vec!["a".to_string(), "b".to_string(), "c".to_string()],
                ),
                (
                    "transports".to_string(),
                    vec!["x".to_string(), "y".to_string()],
                ),
            ],
        );

        let size1 = bincode::serialize(&manifest1).unwrap().len();
        let size2 = bincode::serialize(&manifest2).unwrap().len();

        // Sizes should be very close due to padding (within a few bytes for serialization overhead)
        assert!((size1 as i32 - size2 as i32).abs() < 50);
    }
}
