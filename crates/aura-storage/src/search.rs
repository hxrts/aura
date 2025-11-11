//! G_search Choreography Implementation
//!
//! This module implements the G_search choreography for privacy-preserving
//! distributed search following the formal model from work/whole.md.

use crate::access_control::{
    StorageAccessControl, StorageAccessRequest, StorageOperation, StorageResource,
};
use aura_core::{AccountId, AuraResult, ContentId, DeviceId};
use aura_mpst::CapabilityGuard;
use aura_protocol::effects::{AuraEffectSystem, NetworkEffects};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Messages for the G_search choreography
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SearchMessage {
    /// Search query request
    SearchQuery {
        /// Querying device
        querier_id: DeviceId,
        /// Search terms (DKD-encrypted)
        encrypted_terms: Vec<u8>,
        /// DKD context for query isolation
        dkd_context: Vec<u8>,
        /// Maximum results requested
        limit: usize,
        /// Query nonce for privacy
        query_nonce: [u8; 32],
    },

    /// Search results response
    SearchResults {
        /// Responding index node
        node_id: DeviceId,
        /// Capability-filtered results
        results: Vec<SearchResult>,
        /// Partial result signature
        partial_sig: Vec<u8>,
        /// Result count (for leakage tracking)
        result_count: usize,
    },

    /// Search completion notification
    SearchComplete {
        /// Final aggregated results
        final_results: Vec<SearchResult>,
        /// Participating nodes
        participating_nodes: Vec<DeviceId>,
        /// Threshold signature over results
        threshold_signature: Vec<u8>,
    },
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

/// Search query specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
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

/// Aggregated search results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
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

/// G_search choreography implementation
#[derive(Debug, Clone)]
pub struct SearchChoreography {
    /// Current device role
    role: SearchRole,
    /// Storage access control
    access_control: StorageAccessControl,
    /// Effect system for handling operations
    effects: AuraEffectSystem,
    /// Search index (TODO fix - Simplified)
    search_index: HashMap<String, HashSet<ContentId>>,
}

impl SearchChoreography {
    /// Create new search choreography
    pub fn new(
        role: SearchRole,
        access_control: StorageAccessControl,
        effects: AuraEffectSystem,
    ) -> Self {
        Self {
            role,
            access_control,
            effects,
            search_index: HashMap::new(),
        }
    }

    /// Execute the G_search choreography following the formal model
    ///
    /// ```rust,ignore
    /// choreography! {
    ///     G_search[Roles: Querier, IndexNodes(k)] {
    ///         // Query phase with DKD privacy isolation
    ///         [guard: need(search_query) ≤ caps_Querier]
    ///         [Context: DKD(query_context, isolation_key)]
    ///         Querier -> IndexNodes*: SearchQuery {
    ///             encrypted_terms: DKD_encrypt(terms, isolation_key),
    ///             limit,
    ///             query_nonce
    ///         }
    ///
    ///         // Each index node processes query independently
    ///         parallel {
    ///             IndexNodes*: local_results = search_local_index(terms)
    ///             IndexNodes*: filtered_results = capability_filter(local_results, querier_caps)
    ///         }
    ///
    ///         // Index nodes return capability-filtered results
    ///         choice IndexNodes* {
    ///             has_results {
    ///                 [guard: need(search_respond) ≤ caps_IndexNode]
    ///                 IndexNodes* -> Querier: SearchResults {
    ///                     results: capability_filter(local_results),
    ///                     partial_sig,
    ///                     result_count
    ///                 }
    ///             }
    ///             no_results {
    ///                 IndexNodes* -> Querier: SearchResults { results: [], ... }
    ///             }
    ///         }
    ///
    ///         // Querier aggregates (⊓ over all responses)
    ///         Querier: final_results = ⊓ all_results
    ///
    ///         // Privacy: DKD context isolates query from identity
    ///         [Leakage: ℓ_ext=0, ℓ_ngh=log(|results|), ℓ_grp=full]
    ///     }
    /// }
    /// ```
    pub async fn execute_search(
        &mut self,
        query: SearchQuery,
    ) -> AuraResult<Option<SearchResults>> {
        match &self.role {
            SearchRole::Querier(device_id) => self.execute_as_querier(*device_id, query).await,
            SearchRole::IndexNode(node_id) => self.execute_as_index_node(*node_id).await,
            SearchRole::Coordinator(coordinator_id) => {
                self.execute_as_coordinator(*coordinator_id).await
            }
        }
    }

    /// Execute choreography as querier
    async fn execute_as_querier(
        &mut self,
        device_id: DeviceId,
        query: SearchQuery,
    ) -> AuraResult<Option<SearchResults>> {
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
        let search_msg = SearchMessage::SearchQuery {
            querier_id: device_id,
            encrypted_terms,
            dkd_context: dkd_context.clone(),
            limit: query.limit,
            query_nonce,
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
            if let Ok(response_result) = tokio::time::timeout(
                std::time::Duration::from_secs(30),
                self.effects.receive_from(node_id.0),
            )
            .await
            {
                if let Ok(response_bytes) = response_result {
                    if let Ok(response) = serde_json::from_slice::<SearchMessage>(&response_bytes) {
                        if let SearchMessage::SearchResults {
                            node_id,
                            results,
                            result_count,
                            ..
                        } = response
                        {
                            all_results.extend(results);
                            participating_nodes.push(node_id);

                            // Update leakage tracking
                            total_leakage.neighbor += (result_count as f64).log2().max(0.0);
                            total_leakage.group = 1.0; // Full leakage within search context
                        }
                    }
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

        let results = SearchResults {
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
    ) -> AuraResult<Option<SearchResults>> {
        // 1. Receive search query
        let query_bytes = self
            .effects
            .receive_from(node_id.0)
            .await
            .map_err(|e| aura_core::AuraError::network(format!("Receive error: {}", e)))?;
        let query_msg = serde_json::from_slice::<SearchMessage>(&query_bytes)
            .map_err(|e| aura_core::AuraError::internal(format!("Deserialization error: {}", e)))?;

        if let SearchMessage::SearchQuery {
            querier_id,
            encrypted_terms,
            dkd_context,
            limit,
            query_nonce,
        } = query_msg
        {
            // 2. Decrypt terms using DKD context
            let search_terms = self.decrypt_terms_with_dkd(&encrypted_terms, &dkd_context)?;

            // 3. Search local index
            let local_results = self.search_local_index(&search_terms, limit).await?;

            // 4. Apply capability filtering
            let filtered_results = self
                .filter_results_by_capabilities(local_results, querier_id)
                .await?;

            // 5. Check response capability guard - simplified check
            // TODO: Implement proper capability checking
            if !filtered_results.is_empty() {
                // Send results
                let result_count = filtered_results.len();
                let partial_sig = self.create_result_signature(&filtered_results, query_nonce)?;

                let results_msg = SearchMessage::SearchResults {
                    node_id,
                    results: filtered_results,
                    partial_sig,
                    result_count,
                };

                let message_bytes = serde_json::to_vec(&results_msg).map_err(|e| {
                    aura_core::AuraError::internal(format!("Serialization error: {}", e))
                })?;
                self.effects
                    .send_to_peer(querier_id.0, message_bytes)
                    .await
                    .map_err(|e| aura_core::AuraError::network(format!("Send error: {}", e)))?;
            } else {
                // Send empty results
                let results_msg = SearchMessage::SearchResults {
                    node_id,
                    results: vec![],
                    partial_sig: vec![],
                    result_count: 0,
                };

                let message_bytes = serde_json::to_vec(&results_msg).map_err(|e| {
                    aura_core::AuraError::internal(format!("Serialization error: {}", e))
                })?;
                self.effects
                    .send_to_peer(querier_id.0, message_bytes)
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
    ) -> AuraResult<Option<SearchResults>> {
        // Coordinator manages search routing and result aggregation
        // TODO fix - For now, pass through to index node behavior
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
        terms: &[String],
        isolation_key: &[u8],
    ) -> AuraResult<Vec<u8>> {
        // Encrypt terms using DKD with isolation key
        // This ensures query privacy from identity
        Ok(vec![0u8; 64]) // Placeholder
    }

    /// Decrypt search terms with DKD
    fn decrypt_terms_with_dkd(
        &self,
        encrypted_terms: &[u8],
        dkd_context: &[u8],
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
        query: &SearchQuery,
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
        results: &[SearchResult],
        query_nonce: [u8; 32],
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
    use aura_wot::CapabilityEvaluator;

    #[test]
    fn test_search_query_creation() {
        let query = SearchQuery {
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
        let effects = AuraEffectSystem::for_testing(DeviceId::new());

        let choreography = SearchChoreography::new(
            SearchRole::Querier(DeviceId::new()),
            access_control,
            effects,
        );

        let results = vec![
            SearchResult {
                content_id: ContentId::new([0u8; 32]),
                owner: None,
                score: 0.9,
                snippet: None,
                required_capabilities: vec![],
            },
            SearchResult {
                content_id: ContentId::new([0u8; 32]),
                owner: None,
                score: 0.7,
                snippet: None,
                required_capabilities: vec![],
            },
        ];

        let query = SearchQuery {
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
