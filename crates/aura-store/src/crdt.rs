//! Storage-specific CRDT types and operations
//!
//! This module defines CRDT types for storage state management,
//! implementing join and meet semilattice operations for convergence.

use crate::{SearchIndexEntry, StorageCapabilitySet};
use aura_core::{ChunkId, ContentId, JoinSemilattice};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Storage index CRDT for tracking content and search terms
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageIndex {
    /// Mapping from content ID to search index entries
    pub entries: BTreeMap<ContentId, SearchIndexEntry>,
    /// Version vector for causal ordering
    pub version: u64,
}

impl StorageIndex {
    /// Create a new empty storage index
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            version: 0,
        }
    }

    /// Add or update an index entry
    pub fn add_entry(&mut self, content_id: ContentId, entry: SearchIndexEntry) {
        self.entries.insert(content_id, entry);
        self.version += 1;
    }

    /// Remove an index entry
    pub fn remove_entry(&mut self, content_id: &ContentId) -> Option<SearchIndexEntry> {
        self.version += 1;
        self.entries.remove(content_id)
    }

    /// Get an index entry
    pub fn get_entry(&self, content_id: &ContentId) -> Option<&SearchIndexEntry> {
        self.entries.get(content_id)
    }

    /// Get all content IDs
    pub fn content_ids(&self) -> impl Iterator<Item = &ContentId> {
        self.entries.keys()
    }

    /// Number of entries in the index
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the index is empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Search within this index
    pub fn search(&self, terms: &str) -> Vec<&SearchIndexEntry> {
        self.entries
            .values()
            .filter(|entry| entry.matches_terms(terms))
            .collect()
    }
}

impl Default for StorageIndex {
    fn default() -> Self {
        Self::new()
    }
}

/// Join semilattice implementation for StorageIndex (union of entries)
impl JoinSemilattice for StorageIndex {
    fn join(&self, other: &Self) -> Self {
        let mut merged_entries = self.entries.clone();

        // Merge entries, taking the one with the latest timestamp for conflicts
        for (content_id, other_entry) in &other.entries {
            match merged_entries.get(content_id) {
                Some(existing_entry) => {
                    if other_entry.timestamp > existing_entry.timestamp {
                        merged_entries.insert(content_id.clone(), other_entry.clone());
                    }
                }
                None => {
                    merged_entries.insert(content_id.clone(), other_entry.clone());
                }
            }
        }

        Self {
            entries: merged_entries,
            version: self.version.max(other.version) + 1,
        }
    }
}

/// Storage operation log for causal ordering
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageOpLog {
    /// Ordered list of storage operations
    pub operations: Vec<StorageOperation>,
    /// Operation counter
    pub counter: u64,
}

impl StorageOpLog {
    /// Create a new empty operation log
    pub fn new() -> Self {
        Self {
            operations: Vec::new(),
            counter: 0,
        }
    }

    /// Add an operation to the log
    pub fn add_operation(&mut self, op: StorageOperation) {
        // Update log counter to track the maximum counter seen
        self.counter = self.counter.max(op.counter);
        self.operations.push(op);
    }

    /// Get operations after a certain counter
    pub fn operations_after(&self, after_counter: u64) -> Vec<&StorageOperation> {
        self.operations
            .iter()
            .filter(|op| op.counter > after_counter)
            .collect()
    }

    /// Apply operations to a storage index
    pub fn apply_to_index(&self, index: &mut StorageIndex) {
        for op in &self.operations {
            op.apply_to_index(index);
        }
    }
}

impl Default for StorageOpLog {
    fn default() -> Self {
        Self::new()
    }
}

/// Join semilattice implementation for StorageOpLog (append operations)
impl JoinSemilattice for StorageOpLog {
    fn join(&self, other: &Self) -> Self {
        let mut merged_ops = self.operations.clone();

        // Add operations from other that we don't have
        let max_counter = self.counter;
        for op in &other.operations {
            if op.counter > max_counter {
                merged_ops.push(op.clone());
            }
        }

        // Sort by counter to maintain ordering
        merged_ops.sort_by_key(|op| op.counter);

        Self {
            operations: merged_ops,
            counter: self.counter.max(other.counter),
        }
    }
}

/// Individual storage operation for operation-based CRDT
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageOperation {
    /// Operation type
    pub op_type: StorageOpType,
    /// Operation counter for ordering
    pub counter: u64,
    /// Timestamp when operation was created
    pub timestamp: u64,
    /// Actor who created the operation
    pub actor: String,
}

impl StorageOperation {
    /// Create a new storage operation
    pub fn new(op_type: StorageOpType, counter: u64, actor: String) -> Self {
        Self {
            op_type,
            counter,
            timestamp: aura_core::time::current_unix_timestamp(),
            actor,
        }
    }

    /// Apply this operation to a storage index
    pub fn apply_to_index(&self, index: &mut StorageIndex) {
        match &self.op_type {
            StorageOpType::AddContent { content_id, entry } => {
                index.add_entry(content_id.clone(), entry.clone());
            }
            StorageOpType::RemoveContent { content_id } => {
                index.remove_entry(content_id);
            }
            StorageOpType::UpdateMetadata {
                content_id,
                metadata,
            } => {
                if let Some(entry) = index.entries.get_mut(content_id) {
                    // Update entry with new metadata (simplified)
                    // In a full implementation, this would properly merge metadata
                    if let Some(new_metadata) = metadata.get("updated_terms") {
                        let new_terms: BTreeSet<String> = new_metadata
                            .split_whitespace()
                            .map(|s| s.to_string())
                            .collect();
                        entry.terms = new_terms;
                    }
                }
            }
        }
    }
}

/// Types of storage operations
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageOpType {
    /// Add new content to the index
    AddContent {
        /// Content identifier
        content_id: ContentId,
        /// Search index entry
        entry: SearchIndexEntry,
    },
    /// Remove content from the index
    RemoveContent {
        /// Content identifier
        content_id: ContentId,
    },
    /// Update content metadata
    UpdateMetadata {
        /// Content identifier
        content_id: ContentId,
        /// New metadata
        metadata: BTreeMap<String, String>,
    },
}

/// Complete storage state combining index and capabilities
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageState {
    /// Content search index
    pub index: StorageIndex,
    /// Access control capabilities
    pub capabilities: StorageCapabilitySet,
    /// Operation log for causal consistency
    pub op_log: StorageOpLog,
}

impl StorageState {
    /// Create new storage state
    pub fn new() -> Self {
        Self {
            index: StorageIndex::new(),
            capabilities: StorageCapabilitySet::new(),
            op_log: StorageOpLog::new(),
        }
    }

    /// Add content with capabilities
    pub fn add_content(&mut self, content_id: ContentId, entry: SearchIndexEntry, actor: String) {
        let op = StorageOperation::new(
            StorageOpType::AddContent {
                content_id: content_id.clone(),
                entry: entry.clone(),
            },
            self.op_log.counter + 1,
            actor,
        );

        self.op_log.add_operation(op);
        self.index.add_entry(content_id, entry);
    }

    /// Remove content
    pub fn remove_content(&mut self, content_id: ContentId, actor: String) {
        let op = StorageOperation::new(
            StorageOpType::RemoveContent {
                content_id: content_id.clone(),
            },
            self.op_log.counter + 1,
            actor,
        );

        self.op_log.add_operation(op);
        self.index.remove_entry(&content_id);
    }

    /// Update capabilities (meet operation)
    pub fn refine_capabilities(&mut self, new_capabilities: StorageCapabilitySet) {
        self.capabilities = self.capabilities.meet(&new_capabilities);
    }
}

impl Default for StorageState {
    fn default() -> Self {
        Self::new()
    }
}

/// Join semilattice implementation for StorageState (merge index and op log, meet capabilities)
impl JoinSemilattice for StorageState {
    fn join(&self, other: &Self) -> Self {
        Self {
            index: self.index.join(&other.index),
            capabilities: self.capabilities.meet(&other.capabilities), // Meet for capabilities
            op_log: self.op_log.join(&other.op_log),
        }
    }
}

/// Chunk availability tracker CRDT
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkAvailability {
    /// Mapping from chunk ID to set of nodes that have it
    pub chunk_locations: BTreeMap<ChunkId, BTreeSet<String>>,
    /// Last update timestamp per node
    pub node_timestamps: BTreeMap<String, u64>,
}

impl ChunkAvailability {
    /// Create new chunk availability tracker
    pub fn new() -> Self {
        Self {
            chunk_locations: BTreeMap::new(),
            node_timestamps: BTreeMap::new(),
        }
    }

    /// Mark chunk as available on a node
    pub fn add_chunk(&mut self, chunk_id: ChunkId, node_id: String) {
        self.chunk_locations
            .entry(chunk_id)
            .or_default()
            .insert(node_id.clone());

        self.node_timestamps
            .insert(node_id, aura_core::time::current_unix_timestamp());
    }

    /// Remove chunk from a node
    pub fn remove_chunk(&mut self, chunk_id: &ChunkId, node_id: &str) {
        if let Some(nodes) = self.chunk_locations.get_mut(chunk_id) {
            nodes.remove(node_id);
            if nodes.is_empty() {
                self.chunk_locations.remove(chunk_id);
            }
        }
    }

    /// Get nodes that have a chunk
    pub fn get_chunk_locations(&self, chunk_id: &ChunkId) -> Option<&BTreeSet<String>> {
        self.chunk_locations.get(chunk_id)
    }

    /// Check if a chunk is available
    pub fn is_chunk_available(&self, chunk_id: &ChunkId) -> bool {
        self.chunk_locations
            .get(chunk_id)
            .map(|nodes| !nodes.is_empty())
            .unwrap_or(false)
    }
}

impl Default for ChunkAvailability {
    fn default() -> Self {
        Self::new()
    }
}

/// Join semilattice implementation for ChunkAvailability (union of locations)
impl JoinSemilattice for ChunkAvailability {
    fn join(&self, other: &Self) -> Self {
        let mut merged_locations = self.chunk_locations.clone();

        // Merge chunk locations (union of node sets)
        for (chunk_id, other_nodes) in &other.chunk_locations {
            merged_locations
                .entry(chunk_id.clone())
                .or_default()
                .extend(other_nodes.iter().cloned());
        }

        // Merge node timestamps (take latest)
        let mut merged_timestamps = self.node_timestamps.clone();
        for (node_id, other_timestamp) in &other.node_timestamps {
            merged_timestamps
                .entry(node_id.clone())
                .and_modify(|timestamp| *timestamp = (*timestamp).max(*other_timestamp))
                .or_insert(*other_timestamp);
        }

        Self {
            chunk_locations: merged_locations,
            node_timestamps: merged_timestamps,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{StorageCapability, StorageResource};

    #[test]
    fn test_storage_index_join() {
        let mut index1 = StorageIndex::new();
        let mut index2 = StorageIndex::new();

        let content_id1 = ContentId::from_bytes(b"content1");
        let content_id2 = ContentId::from_bytes(b"content2");

        let entry1 = SearchIndexEntry::new(
            "content1".to_string(),
            ["term1"].iter().map(|&s| s.to_string()).collect(),
            vec![],
        );

        let entry2 = SearchIndexEntry::new(
            "content2".to_string(),
            ["term2"].iter().map(|&s| s.to_string()).collect(),
            vec![],
        );

        index1.add_entry(content_id1.clone(), entry1);
        index2.add_entry(content_id2.clone(), entry2);

        let merged = index1.join(&index2);

        assert_eq!(merged.len(), 2);
        assert!(merged.get_entry(&content_id1).is_some());
        assert!(merged.get_entry(&content_id2).is_some());
    }

    #[test]
    fn test_storage_op_log_join() {
        let mut log1 = StorageOpLog::new();
        let mut log2 = StorageOpLog::new();

        let op1 = StorageOperation::new(
            StorageOpType::AddContent {
                content_id: ContentId::from_bytes(b"content1"),
                entry: SearchIndexEntry::new("content1".to_string(), BTreeSet::new(), vec![]),
            },
            1,
            "node1".to_string(),
        );

        let op2 = StorageOperation::new(
            StorageOpType::AddContent {
                content_id: ContentId::from_bytes(b"content2"),
                entry: SearchIndexEntry::new("content2".to_string(), BTreeSet::new(), vec![]),
            },
            2,
            "node2".to_string(),
        );

        log1.add_operation(op1);
        log2.add_operation(op2);

        let merged = log1.join(&log2);

        assert_eq!(merged.operations.len(), 2);
        assert_eq!(merged.counter, 2);
    }

    #[test]
    fn test_chunk_availability_join() {
        let mut avail1 = ChunkAvailability::new();
        let mut avail2 = ChunkAvailability::new();

        let chunk_id = ChunkId::from_bytes(b"chunk1");

        avail1.add_chunk(chunk_id.clone(), "node1".to_string());
        avail2.add_chunk(chunk_id.clone(), "node2".to_string());

        let merged = avail1.join(&avail2);

        let locations = merged.get_chunk_locations(&chunk_id).unwrap();
        assert_eq!(locations.len(), 2);
        assert!(locations.contains("node1"));
        assert!(locations.contains("node2"));
    }

    #[test]
    fn test_storage_state_capabilities_meet() {
        let cap1 = StorageCapability::read(StorageResource::Global);
        let cap2 = StorageCapability::write(StorageResource::namespace("test"));

        let mut state1 = StorageState::new();
        state1.capabilities.add(cap1.clone());
        state1.capabilities.add(cap2.clone());

        let mut state2 = StorageState::new();
        state2.capabilities.add(cap1.clone());

        let merged = state1.join(&state2);

        // Should only have the intersection (cap1)
        assert_eq!(merged.capabilities.len(), 1);
        assert!(merged.capabilities.contains(&cap1));
        assert!(!merged.capabilities.contains(&cap2));
    }
}
