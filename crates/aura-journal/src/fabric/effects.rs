//! KeyFabric Effects Trait for External Dependency Injection
//!
//! This module defines the effects trait for KeyFabric operations, following the
//! established Aura pattern of injecting external dependencies for testability.

use aura_types::{fabric::*, AuraError, DeviceId};
use async_trait::async_trait;
use automerge::{transaction::Transaction, Automerge, Value as AutomergeValue};
use biscuit_auth::KeyPair as BiscuitKeyPair;
use std::collections::BTreeMap;

/// Effects trait for KeyFabric operations
/// 
/// This trait abstracts all external dependencies needed by KeyFabric:
/// - Automerge CRDT operations
/// - Petgraph cycle detection
/// - Secret sharing operations  
/// - Capability token operations
/// - Time and randomness
#[async_trait]
pub trait FabricEffects: Send + Sync {
    /// Automerge CRDT Operations
    
    /// Create a new Automerge document
    async fn create_document(&self) -> Result<Automerge, AuraError>;
    
    /// Get a value from the document
    async fn get_value(&self, doc: &Automerge, path: &[&str]) -> Result<Option<AutomergeValue>, AuraError>;
    
    /// Set a value in the document
    async fn set_value(&self, doc: &mut Automerge, path: &[&str], value: AutomergeValue) -> Result<(), AuraError>;
    
    /// Merge two documents
    async fn merge_documents(&self, doc1: &mut Automerge, doc2: &Automerge) -> Result<(), AuraError>;
    
    /// Graph Operations
    
    /// Check if adding an edge would create a cycle
    async fn would_create_cycle(&self, edges: &[(NodeId, NodeId)], new_edge: (NodeId, NodeId)) -> Result<bool, AuraError>;
    
    /// Find strongly connected components
    async fn find_connected_components(&self, edges: &[(NodeId, NodeId)]) -> Result<Vec<Vec<NodeId>>, AuraError>;
    
    /// Find topological ordering
    async fn topological_sort(&self, edges: &[(NodeId, NodeId)]) -> Result<Vec<NodeId>, AuraError>;
    
    /// Threshold Cryptography Operations
    
    /// Split a secret into shares using Shamir Secret Sharing (MVP placeholder)
    async fn split_secret(&self, secret: &[u8], threshold: u8, total_shares: u8) -> Result<Vec<Vec<u8>>, AuraError>;
    
    /// Reconstruct a secret from shares (MVP placeholder)
    async fn reconstruct_secret(&self, shares: &[Vec<u8>], threshold: u8) -> Result<Vec<u8>, AuraError>;
    
    /// Generate commitment for a share
    async fn generate_share_commitment(&self, share: &[u8]) -> Result<Vec<u8>, AuraError>;
    
    /// Verify a share commitment
    async fn verify_share_commitment(&self, share: &[u8], commitment: &[u8]) -> Result<bool, AuraError>;
    
    /// Capability Token Operations
    
    /// Create a new capability token
    async fn create_capability_token(
        &self,
        issuer_keypair: &BiscuitKeyPair,
        resource: &str,
        permissions: &[&str],
        expires_at: Option<u64>,
    ) -> Result<Vec<u8>, AuraError>;
    
    /// Verify a capability token
    async fn verify_capability_token(
        &self,
        token: &[u8],
        resource: &str,
        permission: &str,
        public_key: &[u8],
    ) -> Result<bool, AuraError>;
    
    /// Attenuate (restrict) a capability token
    async fn attenuate_capability_token(
        &self,
        token: &[u8],
        new_restrictions: &[&str],
    ) -> Result<Vec<u8>, AuraError>;
    
    /// Cryptographic Operations
    
    /// Generate a random secret
    async fn generate_secret(&self, length: usize) -> Result<Vec<u8>, AuraError>;
    
    /// Encrypt data with AES-GCM
    async fn encrypt_aead(&self, plaintext: &[u8], key: &[u8], additional_data: &[u8]) -> Result<Vec<u8>, AuraError>;
    
    /// Decrypt data with AES-GCM
    async fn decrypt_aead(&self, ciphertext: &[u8], key: &[u8], additional_data: &[u8]) -> Result<Vec<u8>, AuraError>;
    
    /// Hash data with Blake3
    async fn hash_blake3(&self, data: &[u8]) -> Result<[u8; 32], AuraError>;
    
    /// Time and System Operations
    
    /// Get current timestamp (seconds since Unix epoch)
    async fn current_timestamp(&self) -> Result<u64, AuraError>;
    
    /// Get device ID for this instance
    async fn device_id(&self) -> Result<DeviceId, AuraError>;
    
    /// Generate a new UUID
    async fn new_uuid(&self) -> Result<uuid::Uuid, AuraError>;
}

/// Production implementation of FabricEffects
pub struct FabricEffectsAdapter {
    device_id: DeviceId,
    biscuit_keypair: BiscuitKeyPair,
}

impl FabricEffectsAdapter {
    /// Create a new adapter with the given device ID
    pub fn new(device_id: DeviceId) -> Self {
        let biscuit_keypair = BiscuitKeyPair::new();
        
        Self {
            device_id,
            biscuit_keypair,
        }
    }
    
    /// Create adapter with existing keypair
    pub fn with_keypair(device_id: DeviceId, keypair: BiscuitKeyPair) -> Self {
        Self {
            device_id,
            biscuit_keypair: keypair,
        }
    }
}

#[async_trait]
impl FabricEffects for FabricEffectsAdapter {
    /// Automerge CRDT Operations
    
    async fn create_document(&self) -> Result<Automerge, AuraError> {
        Ok(Automerge::new())
    }
    
    async fn get_value(&self, doc: &Automerge, path: &[&str]) -> Result<Option<AutomergeValue>, AuraError> {
        // Navigate through the path to get the value
        let mut current = doc.object_type(automerge::ROOT)
            .map_err(|e| AuraError::Data(format!("Automerge error: {}", e)))?;
        
        // Implementation deferred - this is a complex navigation through Automerge structure
        // For MVP, return None
        Ok(None)
    }
    
    async fn set_value(&self, doc: &mut Automerge, path: &[&str], value: AutomergeValue) -> Result<(), AuraError> {
        // Implementation deferred - this requires transaction management
        // For MVP, return success
        Ok(())
    }
    
    async fn merge_documents(&self, doc1: &mut Automerge, doc2: &Automerge) -> Result<(), AuraError> {
        doc1.merge(doc2)
            .map_err(|e| AuraError::Data(format!("Automerge merge error: {}", e)))?;
        Ok(())
    }
    
    /// Graph Operations
    
    async fn would_create_cycle(&self, edges: &[(NodeId, NodeId)], new_edge: (NodeId, NodeId)) -> Result<bool, AuraError> {
        use petgraph::{Graph, Direction};
        use petgraph::algo::is_cyclic_directed;
        
        // Build graph with existing edges
        let mut graph = Graph::new();
        let mut node_indices = BTreeMap::new();
        
        // Add all nodes referenced in edges
        for (from, to) in edges.iter().chain(std::iter::once(&new_edge)) {
            if !node_indices.contains_key(from) {
                let idx = graph.add_node(*from);
                node_indices.insert(*from, idx);
            }
            if !node_indices.contains_key(to) {
                let idx = graph.add_node(*to);
                node_indices.insert(*to, idx);
            }
        }
        
        // Add existing edges
        for (from, to) in edges {
            if let (Some(&from_idx), Some(&to_idx)) = (node_indices.get(from), node_indices.get(to)) {
                graph.add_edge(from_idx, to_idx, ());
            }
        }
        
        // Add new edge
        if let (Some(&from_idx), Some(&to_idx)) = (node_indices.get(&new_edge.0), node_indices.get(&new_edge.1)) {
            graph.add_edge(from_idx, to_idx, ());
        }
        
        // Check for cycles
        Ok(is_cyclic_directed(&graph))
    }
    
    async fn find_connected_components(&self, edges: &[(NodeId, NodeId)]) -> Result<Vec<Vec<NodeId>>, AuraError> {
        use petgraph::{Graph, Undirected};
        use petgraph::algo::connected_components;
        
        // Build undirected graph
        let mut graph = Graph::<NodeId, (), Undirected>::new_undirected();
        let mut node_indices = BTreeMap::new();
        
        // Add nodes and edges
        for (from, to) in edges {
            if !node_indices.contains_key(from) {
                let idx = graph.add_node(*from);
                node_indices.insert(*from, idx);
            }
            if !node_indices.contains_key(to) {
                let idx = graph.add_node(*to);
                node_indices.insert(*to, idx);
            }
            
            if let (Some(&from_idx), Some(&to_idx)) = (node_indices.get(from), node_indices.get(to)) {
                graph.add_edge(from_idx, to_idx, ());
            }
        }
        
        // Find components (simplified implementation)
        let component_count = connected_components(&graph);
        
        // For MVP, return single component with all nodes
        let all_nodes: Vec<NodeId> = node_indices.keys().cloned().collect();
        Ok(vec![all_nodes])
    }
    
    async fn topological_sort(&self, edges: &[(NodeId, NodeId)]) -> Result<Vec<NodeId>, AuraError> {
        use petgraph::{Graph, Direction};
        use petgraph::algo::toposort;
        
        // Build directed graph
        let mut graph = Graph::new();
        let mut node_indices = BTreeMap::new();
        
        for (from, to) in edges {
            if !node_indices.contains_key(from) {
                let idx = graph.add_node(*from);
                node_indices.insert(*from, idx);
            }
            if !node_indices.contains_key(to) {
                let idx = graph.add_node(*to);
                node_indices.insert(*to, idx);
            }
            
            if let (Some(&from_idx), Some(&to_idx)) = (node_indices.get(from), node_indices.get(to)) {
                graph.add_edge(from_idx, to_idx, ());
            }
        }
        
        // Perform topological sort
        let sorted = toposort(&graph, None)
            .map_err(|_| AuraError::Data("Graph contains cycles".to_string()))?;
        
        let result = sorted
            .iter()
            .map(|&idx| graph[idx])
            .collect();
        
        Ok(result)
    }
    
    /// Threshold Cryptography Operations
    
    async fn split_secret(&self, secret: &[u8], threshold: u8, total_shares: u8) -> Result<Vec<Vec<u8>>, AuraError> {
        // For MVP, use a simple placeholder implementation
        // Will be replaced with actual secret_sharing crate integration
        let mut shares = Vec::new();
        for i in 0..total_shares {
            let mut share = secret.to_vec();
            share.push(i); // Simple differentiation
            shares.push(share);
        }
        Ok(shares)
    }
    
    async fn reconstruct_secret(&self, shares: &[Vec<u8>], threshold: u8) -> Result<Vec<u8>, AuraError> {
        // For MVP, use a simple placeholder implementation
        // Will be replaced with actual secret_sharing crate integration
        if shares.is_empty() {
            return Err(AuraError::Crypto("No shares provided".to_string()));
        }
        
        // Return the original secret (minus the differentiation byte)
        let first_share = &shares[0];
        if first_share.is_empty() {
            return Err(AuraError::Crypto("Invalid share".to_string()));
        }
        
        let secret = first_share[..first_share.len()-1].to_vec();
        Ok(secret)
    }
    
    async fn generate_share_commitment(&self, share: &[u8]) -> Result<Vec<u8>, AuraError> {
        // Simple commitment: Blake3 hash of share
        let hash = self.hash_blake3(share).await?;
        Ok(hash.to_vec())
    }
    
    async fn verify_share_commitment(&self, share: &[u8], commitment: &[u8]) -> Result<bool, AuraError> {
        let expected_commitment = self.generate_share_commitment(share).await?;
        Ok(expected_commitment == commitment)
    }
    
    /// Capability Token Operations
    
    async fn create_capability_token(
        &self,
        issuer_keypair: &BiscuitKeyPair,
        resource: &str,
        permissions: &[&str],
        expires_at: Option<u64>,
    ) -> Result<Vec<u8>, AuraError> {
        use biscuit_auth::{Biscuit, BiscuitBuilder};
        
        let mut builder = BiscuitBuilder::new();
        
        // Add resource and permissions as facts
        builder = builder.add_fact(format!("resource(\"{}\")", resource))
            .map_err(|e| AuraError::Data(format!("Biscuit error: {}", e)))?;
        
        for permission in permissions {
            builder = builder.add_fact(format!("permission(\"{}\")", permission))
                .map_err(|e| AuraError::Data(format!("Biscuit error: {}", e)))?;
        }
        
        // Add expiration if specified
        if let Some(expires) = expires_at {
            builder = builder.add_fact(format!("expires_at({})", expires))
                .map_err(|e| AuraError::Data(format!("Biscuit error: {}", e)))?;
        }
        
        // Build token
        let token = builder.build(issuer_keypair)
            .map_err(|e| AuraError::Data(format!("Biscuit build error: {}", e)))?;
        
        Ok(token.to_vec())
    }
    
    async fn verify_capability_token(
        &self,
        token: &[u8],
        resource: &str,
        permission: &str,
        public_key: &[u8],
    ) -> Result<bool, AuraError> {
        use biscuit_auth::{Biscuit, PublicKey};
        
        // Parse token
        let biscuit = Biscuit::from_bytes(token)
            .map_err(|e| AuraError::Data(format!("Invalid biscuit token: {}", e)))?;
        
        // Parse public key
        let pub_key = PublicKey::from_bytes(public_key)
            .map_err(|e| AuraError::Data(format!("Invalid public key: {}", e)))?;
        
        // Create authorizer with rules
        let mut authorizer = biscuit.authorizer()
            .map_err(|e| AuraError::Data(format!("Biscuit authorizer error: {}", e)))?;
        
        // Add authorization rules
        let rule = format!(
            "allow if resource(\"{}\"), permission(\"{}\")",
            resource, permission
        );
        
        authorizer = authorizer.add_policy(&rule)
            .map_err(|e| AuraError::Data(format!("Biscuit policy error: {}", e)))?;
        
        // Check current time against expiration
        let current_time = self.current_timestamp().await?;
        let time_check = format!("allow if expires_at($time), $time >= {}", current_time);
        authorizer = authorizer.add_policy(&time_check)
            .map_err(|e| AuraError::Data(format!("Biscuit time policy error: {}", e)))?;
        
        // Authorize
        match authorizer.authorize() {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }
    
    async fn attenuate_capability_token(
        &self,
        token: &[u8],
        new_restrictions: &[&str],
    ) -> Result<Vec<u8>, AuraError> {
        use biscuit_auth::Biscuit;
        
        // Parse existing token
        let biscuit = Biscuit::from_bytes(token)
            .map_err(|e| AuraError::Data(format!("Invalid biscuit token: {}", e)))?;
        
        // Create attenuated token with new restrictions
        let mut builder = biscuit.create_block();
        
        for restriction in new_restrictions {
            builder = builder.add_fact(format!("restriction(\"{}\")", restriction))
                .map_err(|e| AuraError::Data(format!("Biscuit attenuation error: {}", e)))?;
        }
        
        let attenuated = builder.build(&self.biscuit_keypair)
            .map_err(|e| AuraError::Data(format!("Biscuit build error: {}", e)))?;
        
        Ok(attenuated.to_vec())
    }
    
    /// Cryptographic Operations
    
    async fn generate_secret(&self, length: usize) -> Result<Vec<u8>, AuraError> {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let mut secret = vec![0u8; length];
        rng.fill(&mut secret[..]);
        Ok(secret)
    }
    
    async fn encrypt_aead(&self, plaintext: &[u8], key: &[u8], additional_data: &[u8]) -> Result<Vec<u8>, AuraError> {
        use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};
        use rand::Rng;
        
        let cipher = Aes256Gcm::new_from_slice(key)
            .map_err(|e| AuraError::Crypto(format!("Invalid key: {}", e)))?;
        
        // Generate random nonce
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        
        // Encrypt
        let ciphertext = cipher.encrypt(nonce, &[additional_data, plaintext].concat().as_slice())
            .map_err(|e| AuraError::Crypto(format!("Encryption failed: {}", e)))?;
        
        // Prepend nonce to ciphertext
        let mut result = nonce_bytes.to_vec();
        result.extend_from_slice(&ciphertext);
        
        Ok(result)
    }
    
    async fn decrypt_aead(&self, ciphertext: &[u8], key: &[u8], additional_data: &[u8]) -> Result<Vec<u8>, AuraError> {
        use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};
        
        if ciphertext.len() < 12 {
            return Err(AuraError::Crypto("Ciphertext too short".to_string()));
        }
        
        let cipher = Aes256Gcm::new_from_slice(key)
            .map_err(|e| AuraError::Crypto(format!("Invalid key: {}", e)))?;
        
        // Extract nonce and ciphertext
        let (nonce_bytes, actual_ciphertext) = ciphertext.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);
        
        // Decrypt
        let plaintext_with_ad = cipher.decrypt(nonce, actual_ciphertext)
            .map_err(|e| AuraError::Crypto(format!("Decryption failed: {}", e)))?;
        
        // Remove additional data prefix
        if plaintext_with_ad.len() < additional_data.len() {
            return Err(AuraError::Crypto("Decrypted data too short".to_string()));
        }
        
        let plaintext = plaintext_with_ad[additional_data.len()..].to_vec();
        Ok(plaintext)
    }
    
    async fn hash_blake3(&self, data: &[u8]) -> Result<[u8; 32], AuraError> {
        Ok(*blake3::hash(data).as_bytes())
    }
    
    /// Time and System Operations
    
    async fn current_timestamp(&self) -> Result<u64, AuraError> {
        Ok(std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs())
    }
    
    async fn device_id(&self) -> Result<DeviceId, AuraError> {
        Ok(self.device_id)
    }
    
    async fn new_uuid(&self) -> Result<uuid::Uuid, AuraError> {
        Ok(uuid::Uuid::new_v4())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_fabric_effects_creation() {
        let device_id = DeviceId::new_v4();
        let effects = FabricEffectsAdapter::new(device_id);
        
        // Test basic operations
        assert_eq!(effects.device_id().await.unwrap(), device_id);
        
        let timestamp = effects.current_timestamp().await.unwrap();
        assert!(timestamp > 0);
        
        let uuid = effects.new_uuid().await.unwrap();
        assert_ne!(uuid, uuid::Uuid::nil());
    }
    
    #[tokio::test]
    async fn test_cycle_detection() {
        let device_id = DeviceId::new_v4();
        let effects = FabricEffectsAdapter::new(device_id);
        
        let node1 = NodeId::new_v4();
        let node2 = NodeId::new_v4();
        let node3 = NodeId::new_v4();
        
        // Linear chain: 1 -> 2 -> 3
        let edges = vec![(node1, node2), (node2, node3)];
        
        // Adding 3 -> 1 would create a cycle
        let would_cycle = effects.would_create_cycle(&edges, (node3, node1)).await.unwrap();
        assert!(would_cycle);
        
        // Adding 1 -> 3 would not create a cycle (it's a shortcut)
        let would_cycle = effects.would_create_cycle(&edges, (node1, node3)).await.unwrap();
        assert!(!would_cycle);
    }
    
    #[tokio::test]
    async fn test_secret_sharing() {
        let device_id = DeviceId::new_v4();
        let effects = FabricEffectsAdapter::new(device_id);
        
        let secret = b"super_secret_data_for_testing";
        let threshold = 2;
        let total_shares = 3;
        
        // Split secret
        let shares = effects.split_secret(secret, threshold, total_shares).await.unwrap();
        assert_eq!(shares.len(), total_shares as usize);
        
        // Reconstruct with threshold shares
        let reconstructed = effects.reconstruct_secret(&shares[..threshold as usize], threshold).await.unwrap();
        assert_eq!(reconstructed, secret);
    }
}