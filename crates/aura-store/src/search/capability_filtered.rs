//! Capability-Filtered Search Implementation
//!
//! Addresses system incongruency #7: "Search replies leaking doc identifiers"
//!
//! Fix Pattern: Replies must be cap-filtered at the source. Better: send
//! verifiable aggregates and reveal CIDs only after cap-checked round.

use aura_core::{
    content::ContentId,
    identifiers::{AccountId, DeviceId},
    journal::Cap,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// Search query with capability filtering requirements
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityFilteredQuery {
    /// The search terms or content hash
    pub query: SearchQuery,

    /// Requesting device identity
    pub requester: DeviceId,

    /// Capabilities of the requester (for filtering)
    pub requester_capabilities: Cap,

    /// Search scope and result preferences
    pub scope: SearchScope,

    /// Maximum number of results to return
    pub max_results: usize,
}

/// Search query types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SearchQuery {
    /// Search by content hash
    ContentHash([u8; 32]),
    /// Search by metadata tags
    Tags(Vec<String>),
    /// Search by content type
    ContentType(String),
    /// Full-text search (if supported)
    FullText(String),
}

/// Search scope configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchScope {
    /// Include public content
    pub include_public: bool,

    /// Include content shared with requester
    pub include_shared: bool,

    /// Include content from specific accounts
    pub include_from_accounts: BTreeSet<AccountId>,

    /// Exclude specific content types
    pub exclude_types: BTreeSet<String>,
}

/// Capability-filtered search result that doesn't leak unauthorized CIDs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilteredSearchResult {
    /// Verifiable aggregate information about matches
    pub aggregate: SearchAggregate,

    /// Authorized content identifiers (only what requester can access)
    pub authorized_content: Vec<AuthorizedContent>,

    /// Proof that filtering was properly applied
    pub filter_proof: FilterProof,

    /// Search execution metadata
    pub metadata: SearchMetadata,
}

/// Aggregate information about search results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchAggregate {
    /// Total number of matches found (including unauthorized)
    pub total_matches: usize,

    /// Number of authorized matches returned
    pub authorized_matches: usize,

    /// Hash of all matching CIDs (for verification)
    pub matches_hash: [u8; 32],

    /// Content type distribution (authorized only)
    pub type_distribution: BTreeMap<String, usize>,

    /// Size distribution buckets (authorized only)
    pub size_buckets: SizeBuckets,
}

/// Content item that passed capability filtering
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizedContent {
    /// Content identifier
    pub content_id: ContentId,

    /// Content metadata (what requester is authorized to see)
    pub metadata: FilteredMetadata,

    /// Capability proof for this content
    pub capability_proof: Vec<u8>, // TODO fix - Simplified capability proof

    /// Access level granted
    pub access_level: AccessLevel,
}

/// Filtered metadata that respects capability boundaries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilteredMetadata {
    /// Content type (always visible if searchable)
    pub content_type: String,

    /// Size bucket instead of exact size
    pub size_bucket: SizeBucket,

    /// Creation timestamp bucket
    pub created_bucket: TimeBucket,

    /// Owner account (if publicly visible or shared)
    pub owner: Option<AccountId>,

    /// Public tags only
    pub public_tags: Vec<String>,

    /// Whether full metadata is available with higher capabilities
    pub has_additional_metadata: bool,
}

/// Content size buckets for privacy
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum SizeBucket {
    Tiny,      // < 1KB
    Small,     // 1KB - 100KB
    Medium,    // 100KB - 10MB
    Large,     // 10MB - 1GB
    VeryLarge, // > 1GB
}

impl SizeBucket {
    pub fn from_size(size: u64) -> Self {
        match size {
            0..=1_024 => Self::Tiny,
            1_025..=102_400 => Self::Small,
            102_401..=10_485_760 => Self::Medium,
            10_485_761..=1_073_741_824 => Self::Large,
            _ => Self::VeryLarge,
        }
    }
}

/// Time buckets for privacy
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TimeBucket {
    Recent,    // Last hour
    Today,     // Last 24 hours
    ThisWeek,  // Last week
    ThisMonth, // Last month
    Older,     // Older than a month
}

impl TimeBucket {
    pub fn from_timestamp(timestamp: u64, now: u64) -> Self {
        let age = now.saturating_sub(timestamp);
        match age {
            0..=3_600 => Self::Recent,
            3_601..=86_400 => Self::Today,
            86_401..=604_800 => Self::ThisWeek,
            604_801..=2_592_000 => Self::ThisMonth,
            _ => Self::Older,
        }
    }
}

/// Size distribution across buckets
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SizeBuckets {
    pub tiny: usize,
    pub small: usize,
    pub medium: usize,
    pub large: usize,
    pub very_large: usize,
}

impl Default for SizeBuckets {
    fn default() -> Self {
        Self::new()
    }
}

impl SizeBuckets {
    pub fn new() -> Self {
        Self {
            tiny: 0,
            small: 0,
            medium: 0,
            large: 0,
            very_large: 0,
        }
    }

    pub fn add_size(&mut self, size: u64) {
        match SizeBucket::from_size(size) {
            SizeBucket::Tiny => self.tiny += 1,
            SizeBucket::Small => self.small += 1,
            SizeBucket::Medium => self.medium += 1,
            SizeBucket::Large => self.large += 1,
            SizeBucket::VeryLarge => self.very_large += 1,
        }
    }
}

/// Access level granted to content
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum AccessLevel {
    /// Can see content exists and basic metadata
    Metadata,
    /// Can read content
    Read,
    /// Can modify content
    Write,
    /// Full administrative access
    Admin,
}

/// Proof that capability filtering was properly applied
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterProof {
    /// Hash of the filtering algorithm used
    pub algorithm_hash: [u8; 32],

    /// Hash of the requester capabilities
    pub capabilities_hash: [u8; 32],

    /// Merkle tree root of all checked content
    pub checked_content_root: [u8; 32],

    /// Number of items filtered out
    pub filtered_count: usize,

    /// Signature over the proof (by search node)
    pub signature: Vec<u8>,
}

/// Search execution metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMetadata {
    /// Search execution time
    pub execution_time_ms: u64,

    /// Number of nodes that participated
    pub participating_nodes: usize,

    /// Search depth achieved
    pub search_depth: usize,

    /// Whether search was complete or partial
    pub is_complete: bool,

    /// Any warnings about search quality
    pub warnings: Vec<String>,
}

/// Capability-filtered search engine
pub struct CapabilityFilteredSearchEngine {
    /// Local content store reference
    content_store: Arc<dyn ContentStore>,

    /// Capability evaluation engine
    capability_engine: Arc<dyn CapabilityEngine>,

    /// Search indexing service
    search_index: Arc<dyn SearchIndex>,
}

impl CapabilityFilteredSearchEngine {
    /// Create a new capability-filtered search engine
    pub fn new(
        content_store: Arc<dyn ContentStore>,
        capability_engine: Arc<dyn CapabilityEngine>,
        search_index: Arc<dyn SearchIndex>,
    ) -> Self {
        Self {
            content_store,
            capability_engine,
            search_index,
        }
    }

    /// Execute a capability-filtered search
    pub async fn search(
        &self,
        query: CapabilityFilteredQuery,
        current_time: u64,
    ) -> Result<FilteredSearchResult, SearchError> {
        // Note: Timing tracking should be done via effects, not system time
        // For now using a dummy value - this is a test implementation

        // Step 1: Execute raw search to get potential matches
        let raw_matches = self.execute_raw_search(&query.query).await?;

        // Step 2: Filter matches based on capabilities
        let (authorized_content, filter_stats) = self
            .filter_by_capabilities(
                raw_matches,
                &query.requester,
                &query.requester_capabilities,
                &query.scope,
                query.max_results,
                current_time,
            )
            .await?;

        // Step 3: Create aggregate information
        let aggregate = self
            .create_search_aggregate(
                &authorized_content,
                filter_stats.total_matches,
                filter_stats.filtered_count,
            )
            .await?;

        // Step 4: Generate filter proof
        let filter_proof = self
            .generate_filter_proof(
                &query.requester_capabilities,
                &authorized_content,
                filter_stats.filtered_count,
            )
            .await?;

        // Step 5: Create execution metadata
        let metadata = SearchMetadata {
            execution_time_ms: 0,
            participating_nodes: 1, // Single node TODO fix - For now
            search_depth: 1,
            is_complete: true,
            warnings: Vec::new(),
        };

        Ok(FilteredSearchResult {
            aggregate,
            authorized_content,
            filter_proof,
            metadata,
        })
    }

    /// Execute raw search without capability filtering
    async fn execute_raw_search(&self, query: &SearchQuery) -> Result<Vec<RawMatch>, SearchError> {
        match query {
            SearchQuery::ContentHash(hash) => {
                // Direct hash lookup
                if let Some(content) = self.content_store.get_by_hash(*hash).await? {
                    Ok(vec![RawMatch {
                        content_id: content.id,
                        score: 1.0,
                        match_type: MatchType::ExactHash,
                    }])
                } else {
                    Ok(Vec::new())
                }
            }

            SearchQuery::Tags(tags) => self.search_index.search_by_tags(tags).await,

            SearchQuery::ContentType(content_type) => {
                self.search_index.search_by_type(content_type).await
            }

            SearchQuery::FullText(text) => self.search_index.full_text_search(text).await,
        }
    }

    /// Filter search results by capabilities
    async fn filter_by_capabilities(
        &self,
        raw_matches: Vec<RawMatch>,
        requester: &DeviceId,
        requester_capabilities: &Cap,
        scope: &SearchScope,
        max_results: usize,
        current_time: u64,
    ) -> Result<(Vec<AuthorizedContent>, FilterStats), SearchError> {
        let mut authorized = Vec::new();
        let mut filtered_count = 0;
        let total_matches = raw_matches.len();

        for raw_match in raw_matches.into_iter().take(max_results * 2) {
            // Check extra for filtering
            if authorized.len() >= max_results {
                break;
            }

            // Get content metadata
            let content = match self
                .content_store
                .get_content_metadata(&raw_match.content_id)
                .await?
            {
                Some(content) => content,
                None => {
                    filtered_count += 1;
                    continue;
                }
            };

            // Check scope filters first (fast rejection)
            if !self.check_scope_filters(&content, scope) {
                filtered_count += 1;
                continue;
            }

            // Check capability-based access
            let access_level = self
                .capability_engine
                .check_content_access(requester, requester_capabilities, &content)
                .await?;

            match access_level {
                Some(level) => {
                    // Create filtered metadata based on access level
                    let filtered_metadata =
                        self.create_filtered_metadata(&content, level, current_time);

                    // Generate capability proof
                    let capability_proof = self
                        .capability_engine
                        .generate_access_proof(requester, requester_capabilities, &content, level)
                        .await?;

                    authorized.push(AuthorizedContent {
                        content_id: raw_match.content_id,
                        metadata: filtered_metadata,
                        capability_proof,
                        access_level: level,
                    });
                }
                None => {
                    filtered_count += 1;
                }
            }
        }

        let filter_stats = FilterStats {
            total_matches,
            authorized_matches: authorized.len(),
            filtered_count,
        };

        Ok((authorized, filter_stats))
    }

    /// Check scope-based filters (before expensive capability checks)
    fn check_scope_filters(&self, content: &ContentMetadata, scope: &SearchScope) -> bool {
        // Check public content inclusion
        if content.is_public && !scope.include_public {
            return false;
        }

        // Check shared content inclusion
        if content.is_shared && !scope.include_shared {
            return false;
        }

        // Check account allowlist
        if !scope.include_from_accounts.is_empty()
            && !scope.include_from_accounts.contains(&content.owner)
        {
            return false;
        }

        // Check content type exclusion
        if scope.exclude_types.contains(&content.content_type) {
            return false;
        }

        true
    }

    /// Create filtered metadata based on access level
    fn create_filtered_metadata(
        &self,
        content: &ContentMetadata,
        access_level: AccessLevel,
        current_time: u64,
    ) -> FilteredMetadata {
        let base_metadata = FilteredMetadata {
            content_type: content.content_type.clone(),
            size_bucket: SizeBucket::from_size(content.size),
            created_bucket: TimeBucket::from_timestamp(content.created_at, current_time),
            owner: None,
            public_tags: Vec::new(),
            has_additional_metadata: false,
        };

        match access_level {
            AccessLevel::Metadata => base_metadata,

            AccessLevel::Read | AccessLevel::Write | AccessLevel::Admin => FilteredMetadata {
                owner: Some(content.owner),
                public_tags: content.public_tags.clone(),
                has_additional_metadata: !content.private_metadata.is_empty(),
                ..base_metadata
            },
        }
    }

    /// Create aggregate information about search results
    async fn create_search_aggregate(
        &self,
        authorized_content: &[AuthorizedContent],
        total_matches: usize,
        filtered_count: usize,
    ) -> Result<SearchAggregate, SearchError> {
        let authorized_matches = authorized_content.len();

        // Create content type distribution
        let mut type_distribution = BTreeMap::new();
        for content in authorized_content {
            *type_distribution
                .entry(content.metadata.content_type.clone())
                .or_insert(0) += 1;
        }

        // Create size distribution
        let mut size_buckets = SizeBuckets::new();
        for content in authorized_content {
            match content.metadata.size_bucket {
                SizeBucket::Tiny => size_buckets.tiny += 1,
                SizeBucket::Small => size_buckets.small += 1,
                SizeBucket::Medium => size_buckets.medium += 1,
                SizeBucket::Large => size_buckets.large += 1,
                SizeBucket::VeryLarge => size_buckets.very_large += 1,
            }
        }

        // Hash all matching CIDs for verification
        let mut hasher = aura_core::hash::hasher();
        for content in authorized_content {
            hasher.update(content.content_id.to_hex().as_bytes());
        }
        let matches_hash = hasher.finalize();

        Ok(SearchAggregate {
            total_matches,
            authorized_matches,
            matches_hash,
            type_distribution,
            size_buckets,
        })
    }

    /// Generate cryptographic proof of proper filtering
    async fn generate_filter_proof(
        &self,
        requester_capabilities: &Cap,
        authorized_content: &[AuthorizedContent],
        filtered_count: usize,
    ) -> Result<FilterProof, SearchError> {
        // Hash the filtering algorithm
        let algorithm_hash = aura_core::hash::hash(b"capability-filtered-search-v1");

        // Hash requester capabilities
        let capabilities_serialized = bincode::serialize(requester_capabilities)
            .map_err(|_| SearchError::SerializationError)?;
        let capabilities_hash = aura_core::hash::hash(&capabilities_serialized);

        // Create Merkle tree root of checked content
        let mut content_hashes = Vec::new();
        for content in authorized_content {
            content_hashes.push(aura_core::hash::hash(
                content.content_id.to_hex().as_bytes(),
            ));
        }
        let checked_content_root = self.compute_merkle_root(content_hashes);

        // TODO: Generate actual cryptographic signature
        let signature = vec![0u8; 64]; // Placeholder

        Ok(FilterProof {
            algorithm_hash,
            capabilities_hash,
            checked_content_root,
            filtered_count,
            signature,
        })
    }

    /// Compute Merkle tree root (TODO fix - Simplified implementation)
    fn compute_merkle_root(&self, mut hashes: Vec<[u8; 32]>) -> [u8; 32] {
        if hashes.is_empty() {
            return aura_core::hash::hash(b"");
        }

        while hashes.len() > 1 {
            let mut next_level = Vec::new();

            for chunk in hashes.chunks(2) {
                let combined = if chunk.len() == 2 {
                    let mut hasher = aura_core::hash::hasher();
                    hasher.update(&chunk[0]);
                    hasher.update(&chunk[1]);
                    hasher.finalize()
                } else {
                    chunk[0]
                };
                next_level.push(combined);
            }

            hashes = next_level;
        }

        hashes[0]
    }
}

/// Raw search match before capability filtering
#[derive(Debug, Clone)]
struct RawMatch {
    content_id: ContentId,
    score: f32,
    match_type: MatchType,
}

/// Type of search match
#[derive(Debug, Clone, Copy)]
enum MatchType {
    ExactHash,
    TagMatch,
    TypeMatch,
    FullTextMatch,
}

/// Statistics from capability filtering
#[derive(Debug)]
struct FilterStats {
    total_matches: usize,
    authorized_matches: usize,
    filtered_count: usize,
}

/// Content metadata for filtering decisions
#[derive(Debug, Clone)]
struct ContentMetadata {
    id: ContentId,
    content_type: String,
    size: u64,
    owner: AccountId,
    is_public: bool,
    is_shared: bool,
    created_at: u64,
    public_tags: Vec<String>,
    private_metadata: BTreeMap<String, String>,
}

/// Search error types
#[derive(Debug)]
pub enum SearchError {
    ContentStoreError(String),
    CapabilityError(String),
    IndexError(String),
    SerializationError,
    InvalidQuery,
}

impl std::fmt::Display for SearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SearchError::ContentStoreError(msg) => write!(f, "Content store error: {}", msg),
            SearchError::CapabilityError(msg) => write!(f, "Capability error: {}", msg),
            SearchError::IndexError(msg) => write!(f, "Search index error: {}", msg),
            SearchError::SerializationError => write!(f, "Serialization error"),
            SearchError::InvalidQuery => write!(f, "Invalid search query"),
        }
    }
}

impl std::error::Error for SearchError {}

impl From<String> for SearchError {
    fn from(msg: String) -> Self {
        SearchError::ContentStoreError(msg)
    }
}

impl From<&str> for SearchError {
    fn from(msg: &str) -> Self {
        SearchError::ContentStoreError(msg.to_string())
    }
}

/// Trait for content storage backend
#[async_trait::async_trait]
pub trait ContentStore: Send + Sync {
    async fn get_by_hash(&self, hash: [u8; 32]) -> Result<Option<ContentMetadata>, String>;
    async fn get_content_metadata(&self, id: &ContentId)
        -> Result<Option<ContentMetadata>, String>;
}

/// Trait for capability evaluation
#[async_trait::async_trait]
pub trait CapabilityEngine: Send + Sync {
    async fn check_content_access(
        &self,
        requester: &DeviceId,
        capabilities: &Cap,
        content: &ContentMetadata,
    ) -> Result<Option<AccessLevel>, String>;

    async fn generate_access_proof(
        &self,
        requester: &DeviceId,
        capabilities: &Cap,
        content: &ContentMetadata,
        access_level: AccessLevel,
    ) -> Result<Vec<u8>, String>;
}

/// Trait for search indexing
#[async_trait::async_trait]
pub trait SearchIndex: Send + Sync {
    async fn search_by_tags(&self, tags: &[String]) -> Result<Vec<RawMatch>, SearchError>;
    async fn search_by_type(&self, content_type: &str) -> Result<Vec<RawMatch>, SearchError>;
    async fn full_text_search(&self, text: &str) -> Result<Vec<RawMatch>, SearchError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_size_bucket_classification() {
        assert!(matches!(SizeBucket::from_size(500), SizeBucket::Tiny));
        assert!(matches!(SizeBucket::from_size(50_000), SizeBucket::Small));
        assert!(matches!(
            SizeBucket::from_size(5_000_000),
            SizeBucket::Medium
        ));
        assert!(matches!(
            SizeBucket::from_size(500_000_000),
            SizeBucket::Large
        ));
        assert!(matches!(
            SizeBucket::from_size(2_000_000_000),
            SizeBucket::VeryLarge
        ));
    }

    #[test]
    fn test_time_bucket_classification() {
        let now = 5000000;

        assert!(matches!(
            TimeBucket::from_timestamp(now - 1800, now),
            TimeBucket::Recent
        ));
        assert!(matches!(
            TimeBucket::from_timestamp(now - 43200, now),
            TimeBucket::Today
        ));
        assert!(matches!(
            TimeBucket::from_timestamp(now - 302400, now),
            TimeBucket::ThisWeek
        ));
        assert!(matches!(
            TimeBucket::from_timestamp(now - 1296000, now),
            TimeBucket::ThisMonth
        ));
        assert!(matches!(
            TimeBucket::from_timestamp(now - 3000000, now),
            TimeBucket::Older
        ));
    }

    #[test]
    fn test_size_buckets_accumulation() {
        let mut buckets = SizeBuckets::new();

        buckets.add_size(500); // Tiny
        buckets.add_size(50_000); // Small
        buckets.add_size(5_000_000); // Medium

        assert_eq!(buckets.tiny, 1);
        assert_eq!(buckets.small, 1);
        assert_eq!(buckets.medium, 1);
        assert_eq!(buckets.large, 0);
        assert_eq!(buckets.very_large, 0);
    }
}
