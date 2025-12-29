//! Search query types and result filtering logic
//!
//! This module defines pure types and functions for storage search operations,
//! capability-based result filtering, and privacy-preserving search.
//!
//! **Time System**: Uses `PhysicalTime` for timestamps per the unified time architecture.

use crate::StorageCapability;
use aura_core::time::PhysicalTime;
use aura_core::AuraError;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Search scope for limiting search domains
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchScope {
    /// Search all accessible content
    Global,
    /// Search within a specific namespace
    Namespace(String),
    /// Search specific content IDs
    Content(Vec<String>),
}

impl SearchScope {
    /// Create namespace scope
    pub fn namespace(namespace: &str) -> Self {
        Self::Namespace(namespace.to_string())
    }

    /// Create content scope
    pub fn content(content_ids: Vec<String>) -> Self {
        Self::Content(content_ids)
    }

    /// Check if a content ID is within this scope
    pub fn contains_content(&self, content_id: &str) -> bool {
        match self {
            Self::Global => true,
            Self::Namespace(ns) => content_id.starts_with(ns),
            Self::Content(ids) => ids.contains(&content_id.to_string()),
        }
    }
}

/// Search query with capability and scope constraints
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    /// Query terms (implementation-specific format)
    pub terms: String,
    /// Search scope limitation
    pub scope: SearchScope,
    /// Required capabilities for accessing results
    pub required_capabilities: Vec<StorageCapability>,
    /// Maximum number of results
    pub limit: Option<usize>,
    /// Query metadata
    pub metadata: BTreeMap<String, String>,
}

impl SearchQuery {
    /// Create a new search query
    pub fn new(terms: String, scope: SearchScope) -> Self {
        Self {
            terms,
            scope,
            required_capabilities: Vec::new(),
            limit: None,
            metadata: BTreeMap::new(),
        }
    }

    /// Add required capability
    pub fn require_capability(mut self, capability: StorageCapability) -> Self {
        self.required_capabilities.push(capability);
        self
    }

    /// Set result limit
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }

    /// Get effective limit (default if not set)
    pub fn effective_limit(&self) -> usize {
        self.limit.unwrap_or(100)
    }
}

/// Individual search result item
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchResultItem {
    /// Content identifier
    pub content_id: String,
    /// Result relevance score (0.0 to 1.0)
    pub score: f64,
    /// Content snippet or summary
    pub snippet: Option<String>,
    /// Required capabilities to access this content
    pub required_capabilities: Vec<StorageCapability>,
    /// Result metadata
    pub metadata: BTreeMap<String, String>,
}

impl SearchResultItem {
    /// Create a new search result item
    pub fn new(
        content_id: String,
        score: f64,
        required_capabilities: Vec<StorageCapability>,
    ) -> Self {
        Self {
            content_id,
            score,
            snippet: None,
            required_capabilities,
            metadata: BTreeMap::new(),
        }
    }

    /// Add snippet to result
    pub fn with_snippet(mut self, snippet: String) -> Self {
        self.snippet = Some(snippet);
        self
    }

    /// Add metadata to result
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }

    // Removed is_accessible method - authorization now handled by Biscuit tokens
}

/// Complete search results with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    /// Original query
    pub query: SearchQuery,
    /// Result items
    pub items: Vec<SearchResultItem>,
    /// Total number of results (before filtering/limiting)
    pub total_count: u32,
    /// Search execution time in milliseconds
    pub execution_time_ms: u64,
    /// Search metadata
    pub metadata: BTreeMap<String, String>,
}

impl SearchResults {
    /// Create new search results
    #[must_use]
    pub fn new(query: SearchQuery, items: Vec<SearchResultItem>, total_count: u32) -> Self {
        Self {
            query,
            items,
            total_count,
            execution_time_ms: 0,
            metadata: BTreeMap::new(),
        }
    }

    /// Set execution time
    #[must_use]
    pub fn with_execution_time(mut self, execution_time_ms: u64) -> Self {
        self.execution_time_ms = execution_time_ms;
        self
    }

    /// Add metadata
    #[must_use]
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }

    /// Number of returned results
    pub fn result_count(&self) -> usize {
        self.items.len()
    }

    /// Check if more results are available
    pub fn has_more_results(&self) -> bool {
        self.total_count as usize > self.items.len()
    }
}

// FilteredResults removed - capability-based filtering superseded by Biscuit tokens
// Authorization checks now handled at effect system layer

/// Search index entry for CRDT-based search
///
/// **Time System**: Uses `PhysicalTime` for entry timestamps.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchIndexEntry {
    /// Content identifier
    pub content_id: String,
    /// Searchable terms
    pub terms: BTreeSet<String>,
    /// Required capabilities
    pub required_capabilities: Vec<StorageCapability>,
    /// Entry timestamp (unified time system)
    pub timestamp: PhysicalTime,
}

impl SearchIndexEntry {
    /// Create new search index entry
    ///
    /// **Time System**: Uses `PhysicalTime` for timestamps.
    pub fn new(
        content_id: String,
        terms: BTreeSet<String>,
        required_capabilities: Vec<StorageCapability>,
        timestamp: PhysicalTime,
    ) -> Self {
        Self {
            content_id,
            terms,
            required_capabilities,
            timestamp,
        }
    }

    /// Create new search index entry from milliseconds timestamp
    ///
    /// Convenience constructor for backward compatibility.
    pub fn new_from_ms(
        content_id: String,
        terms: BTreeSet<String>,
        required_capabilities: Vec<StorageCapability>,
        timestamp_ms: u64,
    ) -> Self {
        Self::new(
            content_id,
            terms,
            required_capabilities,
            PhysicalTime {
                ts_ms: timestamp_ms,
                uncertainty: None,
            },
        )
    }

    /// Get timestamp in milliseconds for backward compatibility
    pub fn timestamp_ms(&self) -> u64 {
        self.timestamp.ts_ms
    }

    /// Check if this entry matches query terms
    pub fn matches_terms(&self, query_terms: &str) -> bool {
        let query_words: BTreeSet<String> = query_terms
            .to_lowercase()
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();

        // Simple term intersection matching
        !query_words.is_disjoint(&self.terms)
    }

    /// Calculate relevance score for query
    pub fn calculate_score(&self, query_terms: &str) -> f64 {
        let query_words: BTreeSet<String> = query_terms
            .to_lowercase()
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();

        if query_words.is_empty() || self.terms.is_empty() {
            return 0.0;
        }

        let intersection_count = query_words.intersection(&self.terms).count();
        let union_count = query_words.union(&self.terms).count();

        intersection_count as f64 / union_count as f64
    }
}

// Removed filter_search_results function - capability-based filtering superseded by Biscuit tokens

/// Pure function to build search index from content
///
/// **Time System**: Uses `PhysicalTime` for entry timestamps.
pub fn build_search_index(
    content_entries: &[(String, String, Vec<StorageCapability>, PhysicalTime)],
) -> Result<Vec<SearchIndexEntry>, AuraError> {
    let mut index_entries = Vec::new();

    for (content_id, content, capabilities, timestamp) in content_entries {
        // Simple tokenization (split on whitespace and punctuation)
        let terms: BTreeSet<String> = content
            .to_lowercase()
            .split(|c: char| c.is_whitespace() || c.is_ascii_punctuation())
            .filter(|s| !s.is_empty() && s.len() > 2) // Filter short terms
            .map(|s| s.to_string())
            .collect();

        let entry = SearchIndexEntry::new(
            content_id.clone(),
            terms,
            capabilities.clone(),
            timestamp.clone(),
        );

        index_entries.push(entry);
    }

    Ok(index_entries)
}

/// Pure function to build search index from content (with milliseconds timestamps)
///
/// Convenience function for backward compatibility.
pub fn build_search_index_from_ms(
    content_entries: &[(String, String, Vec<StorageCapability>, u64)],
) -> Result<Vec<SearchIndexEntry>, AuraError> {
    let entries_with_time: Vec<_> = content_entries
        .iter()
        .map(|(id, content, caps, ts_ms)| {
            (
                id.clone(),
                content.clone(),
                caps.clone(),
                PhysicalTime {
                    ts_ms: *ts_ms,
                    uncertainty: None,
                },
            )
        })
        .collect();
    build_search_index(&entries_with_time)
}

// Removed search_index function - capability-based access checks superseded by Biscuit tokens
// Use filter and score functions separately, with authorization handled by effect system

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StorageResource;

    fn test_time(ts_ms: u64) -> PhysicalTime {
        PhysicalTime {
            ts_ms,
            uncertainty: None,
        }
    }

    #[test]
    fn test_search_scope() {
        let scope = SearchScope::namespace("user/alice");
        assert!(scope.contains_content("user/alice/document1"));
        assert!(!scope.contains_content("user/bob/document1"));
    }

    #[test]
    fn test_search_index_entry() {
        let terms = ["hello", "world", "test"]
            .iter()
            .map(|&s| s.to_string())
            .collect();
        let caps = vec![StorageCapability::read(StorageResource::Global)];

        let entry = SearchIndexEntry::new("test_content".to_string(), terms, caps, test_time(42));

        assert!(entry.matches_terms("hello world"));
        assert!(entry.matches_terms("test"));
        assert!(!entry.matches_terms("nonexistent"));
    }

    #[test]
    fn test_search_score_calculation() {
        let terms = ["hello", "world", "rust"]
            .iter()
            .map(|&s| s.to_string())
            .collect();
        let caps = vec![];
        let entry = SearchIndexEntry::new("test".to_string(), terms, caps, test_time(100));

        let score1 = entry.calculate_score("hello world");
        let score2 = entry.calculate_score("hello");
        let score3 = entry.calculate_score("unrelated terms");

        assert!(score1 > score2);
        assert!(score2 > score3);
        assert_eq!(score3, 0.0);
    }

    // Removed test_filter_search_results - FilteredResults and filter_search_results removed

    #[test]
    fn test_build_search_index() {
        let content_entries = vec![
            (
                "doc1".to_string(),
                "Hello world! This is a test document.".to_string(),
                vec![StorageCapability::read(StorageResource::Global)],
                test_time(1),
            ),
            (
                "doc2".to_string(),
                "Another document with different content.".to_string(),
                vec![],
                test_time(2),
            ),
        ];

        let index = build_search_index(&content_entries).unwrap();

        assert_eq!(index.len(), 2);
        assert!(index[0].terms.contains("hello"));
        assert!(index[0].terms.contains("world"));
        assert!(index[1].terms.contains("another"));
    }
}
