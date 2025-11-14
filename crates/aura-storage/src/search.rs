//! G_search: Privacy-Preserving Distributed Search Choreography
//!
//! This module implements the G_search choreography for privacy-preserving
//! distributed search using DKD context isolation and capability filtering.

use crate::access_control::{
    StorageAccessControl, StorageAccessRequest, StorageOperation, StorageResource,
};
use aura_core::{AccountId, AuraResult, ContentId, DeviceId};
use aura_macros::choreography;
use aura_protocol::effects::{AuraEffectSystem, NetworkEffects};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

// G_search choreography protocol with privacy-preserving distributed search
//
// This choreography implements distributed search with the following features:
// 1. DKD context isolation for query privacy
// 2. Capability-based access control for result filtering
// 3. Semilattice aggregation for result consistency
// 4. Leakage budget tracking for privacy bounds
choreography! {
    #[namespace = "distributed_search"]
    protocol DistributedSearchChoreography {
        roles: Querier, IndexNodes[*], Coordinator;

        // Phase 1: Query Distribution
        // Querier sends DKD-encrypted query to index nodes
        Querier[guard_capability = "submit_search_query",
                flow_cost = 100,
                journal_facts = "search_query_submitted"]
        -> IndexNodes[*]: SearchQuery(SearchQuery);

        // Phase 2: Parallel Search Processing
        // Each index node searches locally and filters by capabilities
        IndexNodes[*][guard_capability = "process_search_query",
                      flow_cost = 150,
                      journal_facts = "search_processed"]
        -> Coordinator: SearchResults(SearchResults);

        // Phase 3: Result Aggregation
        choice Coordinator {
            success: {
                // Coordinator aggregates results using semilattice meet
                Coordinator[guard_capability = "aggregate_search_results",
                           flow_cost = 200,
                           journal_facts = "search_results_aggregated",
                           journal_merge = true]
                -> Querier: SearchComplete(SearchComplete);

                // Notify index nodes of completion
                Coordinator[guard_capability = "notify_search_completion",
                           flow_cost = 50,
                           journal_facts = "search_completion_notified"]
                -> IndexNodes[*]: SearchComplete(SearchComplete);
            }
            failure: {
                // Coordinator returns failure
                Coordinator[guard_capability = "report_search_failure",
                           flow_cost = 100,
                           journal_facts = "search_failed"]
                -> Querier: SearchFailure(SearchFailure);

                // Notify index nodes of failure
                Coordinator[guard_capability = "notify_search_failure",
                           flow_cost = 50,
                           journal_facts = "search_failure_notified"]
                -> IndexNodes[*]: SearchFailure(SearchFailure);
            }
        }
    }
}

// Message types for distributed search choreography

/// Search query message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    /// Querying device ID
    pub querier_id: DeviceId,
    /// Search terms (DKD-encrypted for privacy isolation)
    pub encrypted_terms: Vec<u8>,
    /// DKD context for query isolation
    pub dkd_context: Vec<u8>,
    /// Maximum results requested
    pub limit: usize,
    /// Query nonce for unlinkability
    pub query_nonce: [u8; 32],
    /// Privacy level required
    pub privacy_level: SearchPrivacyLevel,
}

/// Search results message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    /// Index node providing results
    pub node_id: DeviceId,
    /// Capability-filtered search results
    pub results: Vec<SearchResult>,
    /// Partial signature over results
    pub partial_signature: Vec<u8>,
    /// Result count for leakage tracking
    pub result_count: usize,
    /// Leakage budget consumed
    pub leakage_consumed: LeakageBudget,
}

/// Search completion message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchComplete {
    /// Final aggregated results
    pub final_results: Vec<SearchResult>,
    /// Nodes that participated in search
    pub participating_nodes: Vec<DeviceId>,
    /// Threshold signature over final results
    pub threshold_signature: Vec<u8>,
    /// Total leakage budget consumed
    pub total_leakage: LeakageBudget,
}

/// Search failure message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchFailure {
    /// Failure reason
    pub reason: String,
    /// Nodes contacted before failure
    pub contacted_nodes: Vec<DeviceId>,
    /// Failure timestamp
    pub failed_at: u64,
}

/// Individual search result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// Content identifier
    pub content_id: ContentId,
    /// Content owner (if accessible)
    pub owner: Option<AccountId>,
    /// Relevance score
    pub score: f64,
    /// Result snippet (capability-filtered)
    pub snippet: Option<String>,
    /// Access capabilities required
    pub required_capabilities: Vec<String>,
}

/// Roles in the G_search choreography
#[derive(Debug, Clone)]
pub enum SearchRole {
    /// Device performing search query
    Querier(DeviceId),
    /// Index node participating in search
    IndexNode(DeviceId),
    /// Search coordinator
    Coordinator(DeviceId),
}

/// Search query specification for local processing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuerySpec {
    /// Search terms
    pub terms: Vec<String>,
    /// Content type filters
    pub content_types: Vec<String>,
    /// Owner filters
    pub owner_filters: Vec<AccountId>,
    /// Capability requirements
    pub capability_requirements: Vec<String>,
    /// Maximum results
    pub limit: usize,
    /// Privacy level required
    pub privacy_level: SearchPrivacyLevel,
}

/// Privacy levels for search operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SearchPrivacyLevel {
    /// Terms and results fully hidden
    Full,
    /// Result counts observable
    ResultCountOnly,
    /// Metadata observable
    MetadataVisible,
}

/// Aggregated search response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    /// All results from participating nodes
    pub results: Vec<SearchResult>,
    /// Total result count across nodes
    pub total_count: usize,
    /// Nodes that participated
    pub participating_nodes: Vec<DeviceId>,
    /// Query execution metadata
    pub execution_metadata: SearchExecutionMetadata,
}

/// Search execution metadata for privacy tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchExecutionMetadata {
    /// Query timestamp
    pub timestamp: u64,
    /// DKD context used
    pub dkd_context: Vec<u8>,
    /// Leakage budget consumed
    pub leakage_consumed: LeakageBudget,
    /// Nodes contacted
    pub nodes_contacted: usize,
}

/// Privacy leakage budget tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeakageBudget {
    /// External leakage (ℓ_ext = 0 for DKD isolation)
    pub external: f64,
    /// Neighbor leakage (ℓ_ngh = log|results|)
    pub neighbor: f64,
    /// Group leakage (ℓ_grp = full within search context)
    pub group: f64,
}

/// Distributed search coordinator using choreographic protocol
#[derive(Clone)]
pub struct DistributedSearchCoordinator {
    /// Storage access control
    access_control: StorageAccessControl,
    /// Effect system for handling operations
    effects: AuraEffectSystem,
    /// Local search index
    search_index: HashMap<String, HashSet<ContentId>>,
}

impl DistributedSearchCoordinator {
    /// Create new distributed search coordinator
    pub fn new(access_control: StorageAccessControl, effects: AuraEffectSystem) -> Self {
        Self {
            access_control,
            effects,
            search_index: HashMap::new(),
        }
    }

    /// Execute distributed search using choreographic protocol
    pub async fn execute_search(
        &mut self,
        query: SearchQuerySpec,
        role: SearchRole,
    ) -> AuraResult<Option<SearchResponse>> {
        tracing::info!(
            "Starting choreographic distributed search with {} terms",
            query.terms.len()
        );

        // TODO: Execute the choreographic protocol using the generated DistributedSearchChoreography
        // This is a placeholder until the choreography macro is fully integrated

        match role {
            SearchRole::Querier(device_id) => self.execute_as_querier(device_id, query).await,
            SearchRole::IndexNode(node_id) => self.execute_as_index_node(node_id, query).await,
            SearchRole::Coordinator(coordinator_id) => {
                self.execute_as_coordinator(coordinator_id, query).await
            }
        }
    }

    /// Execute choreography as querier
    async fn execute_as_querier(
        &mut self,
        device_id: DeviceId,
        query: SearchQuerySpec,
    ) -> AuraResult<Option<SearchResponse>> {
        // 1. Capability guard: need(search_query) ≤ caps_Querier
        let access_request = StorageAccessRequest {
            device_id,
            operation: StorageOperation::Search {
                terms: query.terms.clone(),
                limit: query.limit,
            },
            resource: StorageResource::SearchIndex,
            capabilities: vec![], // Use registered capabilities
        };

        let guard = self
            .access_control
            .create_capability_guard(&access_request)?;

        // Use the capabilities from the request to check the guard
        let capabilities = aura_core::Cap::default(); // TODO: Use actual device capabilities
        if !guard.check(&capabilities) {
            return Err(aura_core::AuraError::permission_denied(
                "Insufficient capabilities for search operation",
            ));
        }

        // 2. Generate DKD context for query privacy isolation
        let (dkd_context, isolation_key) = self.generate_dkd_context()?;
        let query_nonce = self.generate_query_nonce();

        // 3. Encrypt search terms with DKD for privacy
        let encrypted_terms = self.encrypt_terms_with_dkd(&query.terms, &isolation_key)?;

        // 4. Send query to all index nodes
        let search_msg = SearchQuery {
            querier_id: device_id,
            encrypted_terms,
            dkd_context: dkd_context.clone(),
            limit: query.limit,
            query_nonce,
            privacy_level: SearchPrivacyLevel::Full,
        };

        let index_nodes = self.get_available_index_nodes().await?;
        for node_id in &index_nodes {
            let message_bytes = serde_json::to_vec(&search_msg).map_err(|e| {
                aura_core::AuraError::internal(format!("Serialization error: {}", e))
            })?;
            self.effects
                .send_to_peer(node_id.0, message_bytes)
                .await
                .map_err(|e| aura_core::AuraError::network(format!("Send error: {}", e)))?;
        }

        // 5. Collect results from index nodes
        let mut all_results = Vec::new();
        let mut participating_nodes = Vec::new();
        let mut total_leakage = LeakageBudget {
            external: 0.0, // DKD ensures no external leakage
            neighbor: 0.0,
            group: 0.0,
        };

        // Wait for responses with timeout
        for node_id in &index_nodes {
            if let Ok(Ok(response_bytes)) = tokio::time::timeout(
                std::time::Duration::from_secs(30),
                self.effects.receive_from(node_id.0),
            )
            .await
            {
                if let Ok(response) = serde_json::from_slice::<SearchResults>(&response_bytes) {
                    all_results.extend(response.results);
                    participating_nodes.push(response.node_id);

                    // Update leakage tracking
                    total_leakage.neighbor += response.leakage_consumed.neighbor;
                    total_leakage.group = response.leakage_consumed.group;
                }
            }
        }

        // 6. Aggregate results using semilattice meet (⊓)
        let final_results = self.aggregate_search_results(all_results, &query)?;

        // 7. Create execution metadata
        let execution_metadata = SearchExecutionMetadata {
            timestamp: self.get_current_timestamp(),
            dkd_context,
            leakage_consumed: total_leakage,
            nodes_contacted: index_nodes.len(),
        };

        let results = SearchResponse {
            results: final_results,
            total_count: participating_nodes.len(),
            participating_nodes,
            execution_metadata,
        };

        Ok(Some(results))
    }

    /// Execute choreography as index node
    async fn execute_as_index_node(
        &mut self,
        node_id: DeviceId,
        _query: SearchQuerySpec,
    ) -> AuraResult<Option<SearchResponse>> {
        // 1. Receive search query
        let query_bytes = self
            .effects
            .receive_from(node_id.0)
            .await
            .map_err(|e| aura_core::AuraError::network(format!("Receive error: {}", e)))?;
        let _query_msg = serde_json::from_slice::<SearchQuery>(&query_bytes)
            .map_err(|e| aura_core::AuraError::internal(format!("Deserialization error: {}", e)))?;

        if let Ok(search_query) = serde_json::from_slice::<SearchQuery>(&query_bytes) {
            // 2. Decrypt terms using DKD context
            let search_terms = self
                .decrypt_terms_with_dkd(&search_query.encrypted_terms, &search_query.dkd_context)?;

            // 3. Search local index
            let local_results = self
                .search_local_index(&search_terms, search_query.limit)
                .await?;

            // 4. Apply capability filtering
            let filtered_results = self
                .filter_results_by_capabilities(local_results, search_query.querier_id)
                .await?;

            // 5. Check response capability guard - simplified check
            // TODO: Implement proper capability checking
            if !filtered_results.is_empty() {
                // Send results
                let result_count = filtered_results.len();
                let partial_sig =
                    self.create_result_signature(&filtered_results, search_query.query_nonce)?;

                let results_msg = SearchResults {
                    node_id,
                    results: filtered_results,
                    partial_signature: partial_sig,
                    result_count,
                    leakage_consumed: LeakageBudget {
                        external: 0.0,
                        neighbor: (result_count as f64).log2().max(0.0),
                        group: 1.0,
                    },
                };

                let message_bytes = serde_json::to_vec(&results_msg).map_err(|e| {
                    aura_core::AuraError::internal(format!("Serialization error: {}", e))
                })?;
                self.effects
                    .send_to_peer(search_query.querier_id.0, message_bytes)
                    .await
                    .map_err(|e| aura_core::AuraError::network(format!("Send error: {}", e)))?;
            } else {
                // Send empty results
                let results_msg = SearchResults {
                    node_id,
                    results: vec![],
                    partial_signature: vec![],
                    result_count: 0,
                    leakage_consumed: LeakageBudget {
                        external: 0.0,
                        neighbor: 0.0,
                        group: 0.0,
                    },
                };

                let message_bytes = serde_json::to_vec(&results_msg).map_err(|e| {
                    aura_core::AuraError::internal(format!("Serialization error: {}", e))
                })?;
                self.effects
                    .send_to_peer(search_query.querier_id.0, message_bytes)
                    .await
                    .map_err(|e| aura_core::AuraError::network(format!("Send error: {}", e)))?;
            }
        }

        Ok(None) // Index nodes don't return final results
    }

    /// Execute choreography as coordinator
    async fn execute_as_coordinator(
        &mut self,
        _coordinator_id: DeviceId,
        _query: SearchQuerySpec,
    ) -> AuraResult<Option<SearchResponse>> {
        // Coordinator manages search routing and result aggregation
        // TODO: Implement coordinator logic for the choreographic protocol
        Ok(None)
    }

    /// Generate DKD context for query privacy isolation
    fn generate_dkd_context(&self) -> AuraResult<(Vec<u8>, Vec<u8>)> {
        // Generate DKD context and isolation key
        // This would use the DKD protocol for context derivation
        let context = vec![1, 2, 3, 4]; // Placeholder
        let isolation_key = vec![5, 6, 7, 8]; // Placeholder
        Ok((context, isolation_key))
    }

    /// Generate query nonce for privacy
    fn generate_query_nonce(&self) -> [u8; 32] {
        // Generate cryptographically secure random nonce
        [0u8; 32] // Placeholder
    }

    /// Encrypt search terms with DKD
    fn encrypt_terms_with_dkd(
        &self,
        _terms: &[String],
        _isolation_key: &[u8],
    ) -> AuraResult<Vec<u8>> {
        // Encrypt terms using DKD with isolation key
        // This ensures query privacy from identity
        Ok(vec![0u8; 64]) // Placeholder
    }

    /// Decrypt search terms with DKD
    fn decrypt_terms_with_dkd(
        &self,
        _encrypted_terms: &[u8],
        _dkd_context: &[u8],
    ) -> AuraResult<Vec<String>> {
        // Decrypt terms using DKD context
        Ok(vec!["placeholder".into()]) // Placeholder
    }

    /// Get available index nodes
    async fn get_available_index_nodes(&self) -> AuraResult<Vec<DeviceId>> {
        // Query network for available index nodes
        Ok(vec![DeviceId::new(), DeviceId::new()]) // Placeholder
    }

    /// Search local index for terms
    async fn search_local_index(
        &self,
        terms: &[String],
        limit: usize,
    ) -> AuraResult<Vec<SearchResult>> {
        let mut results = Vec::new();

        for term in terms {
            if let Some(content_ids) = self.search_index.get(term) {
                for (i, content_id) in content_ids.iter().enumerate() {
                    if i >= limit {
                        break;
                    }

                    results.push(SearchResult {
                        content_id: content_id.clone(),
                        owner: None, // Would populate from metadata
                        score: 1.0 - (i as f64 / content_ids.len() as f64),
                        snippet: Some(format!("Result for term: {}", term)),
                        required_capabilities: vec!["content_read".into()],
                    });
                }
            }
        }

        Ok(results)
    }

    /// Filter results by querier capabilities
    async fn filter_results_by_capabilities(
        &self,
        results: Vec<SearchResult>,
        querier_id: DeviceId,
    ) -> AuraResult<Vec<SearchResult>> {
        let mut filtered = Vec::new();

        for result in results {
            let access_request = StorageAccessRequest {
                device_id: querier_id,
                operation: StorageOperation::Read,
                resource: StorageResource::Content(result.content_id.clone()),
                capabilities: vec![],
            };

            if self.access_control.check_access(&access_request).is_ok() {
                filtered.push(result);
            }
        }

        Ok(filtered)
    }

    /// Aggregate search results using semilattice meet
    fn aggregate_search_results(
        &self,
        mut results: Vec<SearchResult>,
        query: &SearchQuerySpec,
    ) -> AuraResult<Vec<SearchResult>> {
        // Sort by relevance score (descending)
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Apply limit
        results.truncate(query.limit);

        // Remove duplicates (semilattice meet operation)
        results.dedup_by(|a, b| a.content_id == b.content_id);

        Ok(results)
    }

    /// Create signature over search results
    fn create_result_signature(
        &self,
        _results: &[SearchResult],
        _query_nonce: [u8; 32],
    ) -> AuraResult<Vec<u8>> {
        // Create partial signature over results for integrity
        Ok(vec![0u8; 64]) // Placeholder
    }

    /// Get current timestamp
    fn get_current_timestamp(&self) -> u64 {
        // Get current timestamp from time effects
        1234567890 // Placeholder
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::Hash32;
    use aura_wot::CapabilityEvaluator;

    #[test]
    fn test_search_query_creation() {
        let query = SearchQuerySpec {
            terms: vec!["test".into(), "search".into()],
            content_types: vec!["document".into()],
            owner_filters: vec![],
            capability_requirements: vec!["read".into()],
            limit: 10,
            privacy_level: SearchPrivacyLevel::Full,
        };

        assert_eq!(query.terms.len(), 2);
        assert_eq!(query.limit, 10);
    }

    #[test]
    fn test_search_result_aggregation() {
        let evaluator = CapabilityEvaluator::new_for_testing();
        let access_control = crate::access_control::StorageAccessControl::new(evaluator);
        let effects = AuraEffectSystem::for_testing_sync(DeviceId::new());

        let choreography = DistributedSearchCoordinator::new(access_control, effects.unwrap());

        let results = vec![
            SearchResult {
                content_id: ContentId::new(Hash32([0u8; 32])),
                owner: None,
                score: 0.9,
                snippet: None,
                required_capabilities: vec![],
            },
            SearchResult {
                content_id: ContentId::new(Hash32([1u8; 32])),
                owner: None,
                score: 0.7,
                snippet: None,
                required_capabilities: vec![],
            },
        ];

        let query = SearchQuerySpec {
            terms: vec!["test".into()],
            content_types: vec![],
            owner_filters: vec![],
            capability_requirements: vec![],
            limit: 10,
            privacy_level: SearchPrivacyLevel::Full,
        };

        let aggregated = choreography
            .aggregate_search_results(results, &query)
            .unwrap();
        assert_eq!(aggregated.len(), 2);
        assert!(aggregated[0].score >= aggregated[1].score); // Sorted by relevance
    }
}
