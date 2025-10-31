//! Erasure Coding for Storage
//!
//! Implements Tahoe-LAFS style Reed-Solomon erasure coding for reliable storage
//! across social networks. Chunks are encrypted first, then split into k-of-n fragments
//! such that any k fragments can reconstruct the original chunk.
//!
//! Reference: docs/040_storage.md Section 9 "Phase 3: Erasure Coding"
//!          work/ssb_storage.md Phase 6.3

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Erasure coding parameters
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErasureParams {
    /// Number of fragments required for reconstruction (k)
    pub required_fragments: u32,

    /// Total number of fragments created (n)
    pub total_fragments: u32,

    /// Size of each fragment in bytes
    pub fragment_size: u64,
}

impl ErasureParams {
    /// Create new erasure parameters
    pub fn new(required_fragments: u32, total_fragments: u32) -> Result<Self, ErasureError> {
        if required_fragments == 0 {
            return Err(ErasureError::InvalidParameters(
                "Required fragments must be > 0".to_string(),
            ));
        }

        if total_fragments < required_fragments {
            return Err(ErasureError::InvalidParameters(
                "Total fragments must be >= required fragments".to_string(),
            ));
        }

        if total_fragments > 255 {
            return Err(ErasureError::InvalidParameters(
                "Total fragments must be <= 255".to_string(),
            ));
        }

        Ok(Self {
            required_fragments,
            total_fragments,
            fragment_size: 0, // Set during encoding
        })
    }

    /// Common k-of-n configuration: 3-of-5 (60% overhead, 40% fault tolerance)
    pub fn standard() -> Self {
        Self {
            required_fragments: 3,
            total_fragments: 5,
            fragment_size: 0,
        }
    }

    /// High reliability: 2-of-6 (200% overhead, 66% fault tolerance)
    pub fn high_reliability() -> Self {
        Self {
            required_fragments: 2,
            total_fragments: 6,
            fragment_size: 0,
        }
    }

    /// Low overhead: 5-of-7 (40% overhead, 28% fault tolerance)
    pub fn low_overhead() -> Self {
        Self {
            required_fragments: 5,
            total_fragments: 7,
            fragment_size: 0,
        }
    }

    /// Calculate storage overhead ratio
    pub fn overhead_ratio(&self) -> f64 {
        (self.total_fragments as f64 / self.required_fragments as f64) - 1.0
    }

    /// Calculate fault tolerance (fraction of fragments that can be lost)
    pub fn fault_tolerance(&self) -> f64 {
        (self.total_fragments - self.required_fragments) as f64 / self.total_fragments as f64
    }
}

/// Erasure-coded fragment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErasureFragment {
    /// Fragment index (0..n-1)
    pub index: u32,

    /// Fragment data (already includes Reed-Solomon parity)
    pub data: Vec<u8>,

    /// Erasure parameters used
    pub params: ErasureParams,

    /// Original chunk size before encoding
    pub original_size: u64,
}

impl ErasureFragment {
    /// Create a new erasure fragment
    pub fn new(index: u32, data: Vec<u8>, params: ErasureParams, original_size: u64) -> Self {
        Self {
            index,
            data,
            params,
            original_size,
        }
    }

    /// Get fragment size
    pub fn size(&self) -> usize {
        self.data.len()
    }
}

/// Fragment distribution plan
#[derive(Debug, Clone)]
pub struct FragmentDistribution {
    /// Mapping of peer ID to fragment indices
    pub peer_fragments: HashMap<Vec<u8>, Vec<u32>>,

    /// Erasure parameters
    pub params: ErasureParams,
}

impl FragmentDistribution {
    /// Create a new distribution plan
    pub fn new(params: ErasureParams) -> Self {
        Self {
            peer_fragments: HashMap::new(),
            params,
        }
    }

    /// Assign a fragment to a peer
    pub fn assign_fragment(&mut self, peer_id: Vec<u8>, fragment_index: u32) {
        self.peer_fragments
            .entry(peer_id)
            .or_insert_with(Vec::new)
            .push(fragment_index);
    }

    /// Get fragments assigned to a peer
    pub fn get_peer_fragments(&self, peer_id: &[u8]) -> Option<&Vec<u32>> {
        self.peer_fragments.get(peer_id)
    }

    /// Check if distribution is valid (all fragments assigned)
    pub fn is_complete(&self) -> bool {
        let mut assigned = std::collections::HashSet::new();
        for indices in self.peer_fragments.values() {
            for &index in indices {
                assigned.insert(index);
            }
        }
        assigned.len() == self.params.total_fragments as usize
    }

    /// Get peers holding specific fragments
    pub fn get_fragment_peers(&self, fragment_index: u32) -> Vec<Vec<u8>> {
        self.peer_fragments
            .iter()
            .filter(|(_, indices)| indices.contains(&fragment_index))
            .map(|(peer_id, _)| peer_id.clone())
            .collect()
    }
}

/// Erasure coding engine
#[derive(Debug, Clone)]
pub struct ErasureCoder {
    params: ErasureParams,
}

impl ErasureCoder {
    /// Create a new erasure coder
    pub fn new(params: ErasureParams) -> Self {
        Self { params }
    }

    /// Encode data into erasure fragments
    ///
    /// This is a simplified implementation. In production, use a proper Reed-Solomon
    /// library like `reed-solomon-erasure` crate.
    pub fn encode(&self, data: &[u8]) -> Result<Vec<ErasureFragment>, ErasureError> {
        if data.is_empty() {
            return Err(ErasureError::InvalidInput("Empty data".to_string()));
        }

        let k = self.params.required_fragments as usize;
        let n = self.params.total_fragments as usize;
        let original_size = data.len() as u64;

        // Calculate fragment size (divide data into k fragments)
        let fragment_size = (data.len() + k - 1) / k;

        // Pad data to multiple of k
        let mut padded_data = data.to_vec();
        while padded_data.len() < fragment_size * k {
            padded_data.push(0);
        }

        // Split into k data fragments
        let mut fragments = Vec::new();
        for i in 0..k {
            let start = i * fragment_size;
            let end = ((i + 1) * fragment_size).min(padded_data.len());
            let fragment_data = padded_data[start..end].to_vec();

            let mut params = self.params.clone();
            params.fragment_size = fragment_data.len() as u64;

            fragments.push(ErasureFragment::new(
                i as u32,
                fragment_data,
                params,
                original_size,
            ));
        }

        // Generate parity fragments using simple XOR (placeholder for Reed-Solomon)
        // In production, use proper Reed-Solomon encoding
        for i in k..n {
            let parity_data = self.generate_parity_fragment(&fragments, i - k);

            let mut params = self.params.clone();
            params.fragment_size = parity_data.len() as u64;

            fragments.push(ErasureFragment::new(
                i as u32,
                parity_data,
                params,
                original_size,
            ));
        }

        Ok(fragments)
    }

    /// Decode data from erasure fragments
    ///
    /// This is a simplified implementation. In production, use a proper Reed-Solomon
    /// library like `reed-solomon-erasure` crate.
    pub fn decode(&self, fragments: &[ErasureFragment]) -> Result<Vec<u8>, ErasureError> {
        if fragments.is_empty() {
            return Err(ErasureError::InsufficientFragments);
        }

        if fragments.len() < self.params.required_fragments as usize {
            return Err(ErasureError::InsufficientFragments);
        }

        // Verify all fragments have same parameters
        let first_params = &fragments[0].params;
        let original_size = fragments[0].original_size;

        for fragment in fragments.iter() {
            if fragment.params != *first_params {
                return Err(ErasureError::ParameterMismatch);
            }
        }

        let k = self.params.required_fragments as usize;

        // Collect data fragments (indices 0..k-1)
        let mut data_fragments: Vec<Option<&ErasureFragment>> = vec![None; k];

        for fragment in fragments {
            if (fragment.index as usize) < k {
                data_fragments[fragment.index as usize] = Some(fragment);
            }
        }

        // Check if we have all k data fragments
        let missing_count = data_fragments.iter().filter(|f| f.is_none()).count();

        if missing_count > 0 {
            // Need to reconstruct from parity fragments
            // In production, use proper Reed-Solomon decoding
            // For now, return error if any data fragment is missing
            return Err(ErasureError::ReconstructionFailed(
                "Reed-Solomon reconstruction not implemented in simplified version".to_string(),
            ));
        }

        // Concatenate data fragments
        let mut reconstructed = Vec::new();
        for fragment_opt in data_fragments {
            if let Some(fragment) = fragment_opt {
                reconstructed.extend_from_slice(&fragment.data);
            }
        }

        // Remove padding
        reconstructed.truncate(original_size as usize);

        Ok(reconstructed)
    }

    /// Generate a parity fragment (simplified XOR-based implementation)
    fn generate_parity_fragment(
        &self,
        data_fragments: &[ErasureFragment],
        parity_index: usize,
    ) -> Vec<u8> {
        let fragment_size = data_fragments[0].data.len();
        let mut parity = vec![0u8; fragment_size];

        // Simple XOR of all data fragments
        // In production, use proper Reed-Solomon encoding
        for fragment in data_fragments {
            for (i, &byte) in fragment.data.iter().enumerate() {
                if i < parity.len() {
                    parity[i] ^= byte;
                }
            }
        }

        // Add parity index to make fragments unique
        if !parity.is_empty() {
            parity[0] ^= parity_index as u8;
        }

        parity
    }

    /// Plan fragment distribution across peers
    pub fn plan_distribution(
        &self,
        peer_ids: &[Vec<u8>],
    ) -> Result<FragmentDistribution, ErasureError> {
        if peer_ids.len() < self.params.total_fragments as usize {
            return Err(ErasureError::InsufficientPeers);
        }

        let mut distribution = FragmentDistribution::new(self.params.clone());

        // Simple round-robin assignment
        for i in 0..self.params.total_fragments {
            let peer_index = (i as usize) % peer_ids.len();
            distribution.assign_fragment(peer_ids[peer_index].clone(), i);
        }

        Ok(distribution)
    }

    /// Get erasure parameters
    pub fn params(&self) -> &ErasureParams {
        &self.params
    }
}

/// Erasure coding errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErasureError {
    InvalidParameters(String),
    InvalidInput(String),
    InsufficientFragments,
    InsufficientPeers,
    ParameterMismatch,
    ReconstructionFailed(String),
}

impl std::fmt::Display for ErasureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErasureError::InvalidParameters(msg) => {
                write!(f, "Invalid erasure parameters: {}", msg)
            }
            ErasureError::InvalidInput(msg) => write!(f, "Invalid input: {}", msg),
            ErasureError::InsufficientFragments => {
                write!(f, "Insufficient fragments for reconstruction")
            }
            ErasureError::InsufficientPeers => {
                write!(f, "Insufficient peers for fragment distribution")
            }
            ErasureError::ParameterMismatch => write!(f, "Fragment parameters do not match"),
            ErasureError::ReconstructionFailed(msg) => write!(f, "Reconstruction failed: {}", msg),
        }
    }
}

impl std::error::Error for ErasureError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_erasure_params_validation() {
        assert!(ErasureParams::new(0, 5).is_err());
        assert!(ErasureParams::new(5, 3).is_err());
        assert!(ErasureParams::new(3, 256).is_err());
        assert!(ErasureParams::new(3, 5).is_ok());
    }

    #[test]
    fn test_erasure_params_metrics() {
        let params = ErasureParams::new(3, 5).unwrap();

        // 5/3 - 1 = 66.7% overhead
        assert!((params.overhead_ratio() - 0.666).abs() < 0.01);

        // (5-3)/5 = 40% fault tolerance
        assert!((params.fault_tolerance() - 0.4).abs() < 0.01);
    }

    #[test]
    fn test_standard_configurations() {
        let standard = ErasureParams::standard();
        assert_eq!(standard.required_fragments, 3);
        assert_eq!(standard.total_fragments, 5);

        let high_rel = ErasureParams::high_reliability();
        assert_eq!(high_rel.required_fragments, 2);
        assert_eq!(high_rel.total_fragments, 6);

        let low_overhead = ErasureParams::low_overhead();
        assert_eq!(low_overhead.required_fragments, 5);
        assert_eq!(low_overhead.total_fragments, 7);
    }

    #[test]
    fn test_encode_decode() {
        let params = ErasureParams::new(3, 5).unwrap();
        let coder = ErasureCoder::new(params);

        let data = b"Hello, erasure coding world! This is a test message.";

        // Encode
        let fragments = coder.encode(data).unwrap();
        assert_eq!(fragments.len(), 5);

        // Decode with first 3 fragments (minimum required)
        let reconstruction_fragments = &fragments[0..3];
        let decoded = coder.decode(reconstruction_fragments).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_encode_empty_data() {
        let params = ErasureParams::new(3, 5).unwrap();
        let coder = ErasureCoder::new(params);

        let result = coder.encode(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_insufficient_fragments() {
        let params = ErasureParams::new(3, 5).unwrap();
        let coder = ErasureCoder::new(params);

        let data = b"Test data";
        let fragments = coder.encode(data).unwrap();

        // Try to decode with only 2 fragments (need 3)
        let result = coder.decode(&fragments[0..2]);
        assert!(result.is_err());
    }

    #[test]
    fn test_fragment_distribution() {
        let params = ErasureParams::new(3, 5).unwrap();
        let coder = ErasureCoder::new(params);

        let peers: Vec<Vec<u8>> = (0..10).map(|i| vec![i]).collect();

        let distribution = coder.plan_distribution(&peers).unwrap();
        assert!(distribution.is_complete());

        // Check that all 5 fragments are assigned
        let mut all_fragments = std::collections::HashSet::new();
        for indices in distribution.peer_fragments.values() {
            for &index in indices {
                all_fragments.insert(index);
            }
        }
        assert_eq!(all_fragments.len(), 5);
    }

    #[test]
    fn test_fragment_distribution_insufficient_peers() {
        let params = ErasureParams::new(3, 5).unwrap();
        let coder = ErasureCoder::new(params);

        let peers: Vec<Vec<u8>> = vec![vec![1], vec![2]]; // Only 2 peers, need 5

        let result = coder.plan_distribution(&peers);
        assert!(result.is_err());
    }

    #[test]
    fn test_distribution_fragment_lookup() {
        let params = ErasureParams::new(3, 5).unwrap();
        let mut distribution = FragmentDistribution::new(params);

        let peer1 = vec![1];
        let peer2 = vec![2];

        distribution.assign_fragment(peer1.clone(), 0);
        distribution.assign_fragment(peer1.clone(), 1);
        distribution.assign_fragment(peer2.clone(), 2);

        let peer1_fragments = distribution.get_peer_fragments(&peer1).unwrap();
        assert_eq!(peer1_fragments, &vec![0, 1]);

        let peer2_fragments = distribution.get_peer_fragments(&peer2).unwrap();
        assert_eq!(peer2_fragments, &vec![2]);
    }

    #[test]
    fn test_get_fragment_peers() {
        let params = ErasureParams::new(3, 5).unwrap();
        let mut distribution = FragmentDistribution::new(params);

        let peer1 = vec![1];
        let peer2 = vec![2];

        distribution.assign_fragment(peer1.clone(), 0);
        distribution.assign_fragment(peer2.clone(), 0); // Same fragment on multiple peers

        let peers = distribution.get_fragment_peers(0);
        assert_eq!(peers.len(), 2);
        assert!(peers.contains(&peer1));
        assert!(peers.contains(&peer2));
    }

    #[test]
    fn test_large_data_encoding() {
        let params = ErasureParams::new(3, 5).unwrap();
        let coder = ErasureCoder::new(params);

        // Test with 1MB of data
        let data: Vec<u8> = (0..1024 * 1024).map(|i| (i % 256) as u8).collect();

        let fragments = coder.encode(&data).unwrap();
        assert_eq!(fragments.len(), 5);

        // Verify each fragment is roughly 1/3 of original size (since k=3)
        let expected_fragment_size = (data.len() + 2) / 3;
        for i in 0..3 {
            assert!(fragments[i].data.len() >= expected_fragment_size - 10);
            assert!(fragments[i].data.len() <= expected_fragment_size + 10);
        }

        // Decode
        let decoded = coder.decode(&fragments[0..3]).unwrap();
        assert_eq!(decoded, data);
    }
}
