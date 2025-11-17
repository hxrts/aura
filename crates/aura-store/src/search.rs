//! Search query types and result filtering logic
//!
//! This module defines pure types and functions for storage search operations,
//! capability-based result filtering, and privacy-preserving search.

use crate::{StorageCapability, StorageCapabilitySet};
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

    /// Check if user capabilities can access this result
    pub fn is_accessible(&self, user_capabilities: &StorageCapabilitySet) -> bool {
        user_capabilities.satisfies_all(&self.required_capabilities)
    }
}

/// Complete search results with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    /// Original query
    pub query: SearchQuery,
    /// Result items
    pub items: Vec<SearchResultItem>,
    /// Total number of results (before filtering/limiting)
    pub total_count: usize,
    /// Search execution time in milliseconds
    pub execution_time_ms: u64,
    /// Search metadata
    pub metadata: BTreeMap<String, String>,
}

impl SearchResults {
    /// Create new search results
    pub fn new(query: SearchQuery, items: Vec<SearchResultItem>, total_count: usize) -> Self {
        Self {
            query,
            items,
            total_count,
            execution_time_ms: 0,
            metadata: BTreeMap::new(),
        }
    }

    /// Set execution time
    pub fn with_execution_time(mut self, execution_time_ms: u64) -> Self {
        self.execution_time_ms = execution_time_ms;
        self
    }

    /// Add metadata
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
        self.total_count > self.items.len()
    }
}

/// Capability-filtered search results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilteredResults {
    /// Original search results
    pub original_results: SearchResults,
    /// Filtered items (only accessible ones)
    pub filtered_items: Vec<SearchResultItem>,
    /// Number of items filtered out
    pub filtered_count: usize,
    /// Filtering capabilities used
    pub filter_capabilities: StorageCapabilitySet,
}

impl FilteredResults {
    /// Create new filtered results
    pub fn new(
        original_results: SearchResults,
        filtered_items: Vec<SearchResultItem>,
        filtered_count: usize,
        filter_capabilities: StorageCapabilitySet,
    ) -> Self {
        Self {
            original_results,
            filtered_items,
            filtered_count,
            filter_capabilities,
        }
    }

    /// Number of accessible results
    pub fn accessible_count(&self) -> usize {
        self.filtered_items.len()
    }

    /// Get accessibility ratio (accessible / total)
    pub fn accessibility_ratio(&self) -> f64 {
        if self.original_results.result_count() == 0 {
            1.0
        } else {
            self.accessible_count() as f64 / self.original_results.result_count() as f64
        }
    }
}

/// Search index entry for CRDT-based search
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchIndexEntry {
    /// Content identifier
    pub content_id: String,
    /// Searchable terms
    pub terms: BTreeSet<String>,
    /// Required capabilities
    pub required_capabilities: Vec<StorageCapability>,
    /// Entry timestamp
    pub timestamp: u64,
}

impl SearchIndexEntry {
    /// Create new search index entry
    pub fn new(
        content_id: String,
        terms: BTreeSet<String>,
        required_capabilities: Vec<StorageCapability>,
    ) -> Self {
        Self {
            content_id,
            terms,
            required_capabilities,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
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

/// Pure function to filter search results by capabilities
pub fn filter_search_results(
    results: SearchResults,
    user_capabilities: &StorageCapabilitySet,
) -> FilteredResults {
    let mut filtered_items = Vec::new();
    let mut filtered_count = 0;

    for item in &results.items {
        if item.is_accessible(user_capabilities) {
            filtered_items.push(item.clone());
        } else {
            filtered_count += 1;
        }
    }

    FilteredResults::new(
        results,
        filtered_items,
        filtered_count,
        user_capabilities.clone(),
    )
}

/// Pure function to build search index from content
pub fn build_search_index(
    content_entries: &[(String, String, Vec<StorageCapability>)],
) -> Result<Vec<SearchIndexEntry>, AuraError> {
    let mut index_entries = Vec::new();

    for (content_id, content, capabilities) in content_entries {
        // Simple tokenization (split on whitespace and punctuation)
        let terms: BTreeSet<String> = content
            .to_lowercase()
            .split(|c: char| c.is_whitespace() || c.is_ascii_punctuation())
            .filter(|s| !s.is_empty() && s.len() > 2) // Filter short terms
            .map(|s| s.to_string())
            .collect();

        let entry = SearchIndexEntry::new(content_id.clone(), terms, capabilities.clone());

        index_entries.push(entry);
    }

    Ok(index_entries)
}

/// Pure function to search within an index
pub fn search_index(
    index: &[SearchIndexEntry],
    query: &SearchQuery,
    user_capabilities: &StorageCapabilitySet,
) -> Result<SearchResults, AuraError> {
    let mut matching_items = Vec::new();

    for entry in index {
        // Check scope
        if !query.scope.contains_content(&entry.content_id) {
            continue;
        }

        // Check if user can access this content
        if !user_capabilities.satisfies_all(&entry.required_capabilities) {
            continue;
        }

        // Check term matching
        if entry.matches_terms(&query.terms) {
            let score = entry.calculate_score(&query.terms);
            let item = SearchResultItem::new(
                entry.content_id.clone(),
                score,
                entry.required_capabilities.clone(),
            );
            matching_items.push(item);
        }
    }

    // Sort by score (highest first)
    matching_items.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let total_count = matching_items.len();

    // Apply limit
    if let Some(limit) = query.limit {
        matching_items.truncate(limit);
    }

    Ok(SearchResults::new(
        query.clone(),
        matching_items,
        total_count,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StorageResource;

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

        let entry = SearchIndexEntry::new("test_content".to_string(), terms, caps);

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
        let entry = SearchIndexEntry::new("test".to_string(), terms, caps);

        let score1 = entry.calculate_score("hello world");
        let score2 = entry.calculate_score("hello");
        let score3 = entry.calculate_score("unrelated terms");

        assert!(score1 > score2);
        assert!(score2 > score3);
        assert_eq!(score3, 0.0);
    }

    #[test]
    fn test_filter_search_results() {
        let cap = StorageCapability::read(StorageResource::Global);
        let user_caps = StorageCapabilitySet::from_capabilities(vec![cap.clone()]);

        let accessible_item = SearchResultItem::new("accessible".to_string(), 1.0, vec![cap]);

        let restricted_item = SearchResultItem::new(
            "restricted".to_string(),
            1.0,
            vec![StorageCapability::admin(StorageResource::Global)],
        );

        let query = SearchQuery::new("test".to_string(), SearchScope::Global);
        let results = SearchResults::new(query, vec![accessible_item.clone(), restricted_item], 2);

        let filtered = filter_search_results(results, &user_caps);

        assert_eq!(filtered.accessible_count(), 1);
        assert_eq!(filtered.filtered_count, 1);
        assert_eq!(filtered.filtered_items[0], accessible_item);
    }

    #[test]
    fn test_build_search_index() {
        let content_entries = vec![
            (
                "doc1".to_string(),
                "Hello world! This is a test document.".to_string(),
                vec![StorageCapability::read(StorageResource::Global)],
            ),
            (
                "doc2".to_string(),
                "Another document with different content.".to_string(),
                vec![],
            ),
        ];

        let index = build_search_index(&content_entries).unwrap();

        assert_eq!(index.len(), 2);
        assert!(index[0].terms.contains("hello"));
        assert!(index[0].terms.contains("world"));
        assert!(index[1].terms.contains("another"));
    }
}
