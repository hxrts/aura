//! Search Infrastructure for Encrypted Storage
//!
//! Provides capability-aware search functionality that prevents metadata
//! leakage while enabling efficient content discovery.

pub mod capability_filtered;

pub use capability_filtered::{
    AccessLevel, CapabilityFilteredQuery, CapabilityFilteredSearchEngine, FilteredSearchResult,
    SearchError, SearchQuery, SearchScope,
};
