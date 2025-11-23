#![allow(missing_docs)]

//! Namespace-aware Synchronization Protocol
//!
//! This module implements synchronization for the fact-based journal model
//! with proper namespace isolation for authorities and contexts.

use aura_core::identifiers::ContextId;
use aura_core::{AuraError, AuthorityId, Result};
use aura_journal::{Fact, FactJournal as Journal, JournalNamespace};
use aura_protocol::effects::AuraEffects;
use crate::core::config::SyncConfig;
use crate::core::errors::{sync_network_error, sync_serialization_error, sync_session_error};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

/// Namespace-aware synchronization coordinator
#[derive(Debug, Clone)]
pub struct NamespacedSync {
    /// Namespace being synchronized
    pub namespace: JournalNamespace,
    /// Journal instance with interior mutability for concurrent access
    pub journal: Arc<RwLock<Journal>>,
}

/// Synchronization request for a specific namespace
///
/// Supports pagination by including known_fact_ids from previous requests.
/// For the first page, leave known_fact_ids empty. For subsequent pages,
/// include all fact_ids received in previous responses to continue pagination.
///
/// # Example
/// ```rust,ignore
/// // First page
/// let request = SyncRequest {
///     namespace: my_namespace,
///     requester: authority_id,
///     known_fact_ids: vec![], // Empty for first page
///     max_facts: 100,
/// };
///
/// // Subsequent pages (accumulate all previous fact_ids)
/// let mut all_known_facts = previous_response.facts.iter()
///     .map(|f| f.fact_id.clone()).collect();
/// let next_request = SyncRequest {
///     namespace: my_namespace,
///     requester: authority_id,
///     known_fact_ids: all_known_facts, // Include all previous facts
///     max_facts: 100,
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRequest {
    /// Namespace to sync
    pub namespace: JournalNamespace,
    /// Requesting authority
    pub requester: AuthorityId,
    /// Facts already known (for delta sync and pagination)
    pub known_fact_ids: Vec<aura_journal::FactId>,
    /// Maximum facts to return per page
    pub max_facts: usize,
}

/// Synchronization response with facts
///
/// Supports pagination through the has_more field. When has_more is true,
/// the client should make another request including all previously received
/// fact_ids in the known_fact_ids field to get the next page.
///
/// Facts are returned in deterministic order (sorted by fact_id) to ensure
/// consistent pagination across requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResponse {
    /// Namespace being synced
    pub namespace: JournalNamespace,
    /// Facts to sync (up to max_facts from request)
    pub facts: Vec<Fact>,
    /// Whether more facts are available for pagination
    pub has_more: bool,
}

/// Synchronization statistics
#[derive(Debug, Clone, Default)]
pub struct SyncStats {
    /// Facts sent
    pub facts_sent: usize,
    /// Facts received
    pub facts_received: usize,
    /// Sync duration in ms
    pub duration_ms: u64,
}

impl NamespacedSync {
    /// Create a new namespace-aware sync instance
    pub fn new(namespace: JournalNamespace, journal: Arc<RwLock<Journal>>) -> Self {
        Self { namespace, journal }
    }

    /// Synchronize facts with a peer
    pub async fn sync_facts<E: AuraEffects>(
        &self,
        effects: &E,
        peer: &AuthorityId,
    ) -> Result<Vec<Fact>> {
        match self.namespace {
            JournalNamespace::Authority(id) => self.sync_authority_facts(effects, id, peer).await,
            JournalNamespace::Context(id) => self.sync_context_facts(effects, id, peer).await,
        }
    }

    /// Sync facts for an authority namespace
    async fn sync_authority_facts<E: AuraEffects>(
        &self,
        effects: &E,
        authority_id: AuthorityId,
        peer: &AuthorityId,
    ) -> Result<Vec<Fact>> {
        // Only sync if peer is the same authority or has delegation
        if peer != &authority_id {
            // Check if peer has delegation to sync this authority
            if !self
                .check_sync_authorization(effects, peer, &authority_id)
                .await?
            {
                return Err(AuraError::permission_denied(
                    "Peer not authorized to sync this authority",
                ));
            }
        }

        // Get facts from journal (acquire read lock)
        let facts: Vec<Fact> = self.journal.read().iter_facts().cloned().collect();

        Ok(facts)
    }

    /// Sync facts for a context namespace
    async fn sync_context_facts<E: AuraEffects>(
        &self,
        effects: &E,
        context_id: ContextId,
        peer: &AuthorityId,
    ) -> Result<Vec<Fact>> {
        // Check if peer is a participant in this context
        if !self
            .check_context_participant(effects, peer, &context_id)
            .await?
        {
            return Err(AuraError::permission_denied(
                "Peer not a participant in this context",
            ));
        }

        // Get facts from journal (acquire read lock)
        let facts: Vec<Fact> = self.journal.read().iter_facts().cloned().collect();

        Ok(facts)
    }

    /// Check if peer is authorized to sync authority namespace
    async fn check_sync_authorization<E: AuraEffects>(
        &self,
        effects: &E,
        peer: &AuthorityId,
        authority: &AuthorityId,
    ) -> Result<bool> {
        // Check if peer is the same authority (always allowed)
        if peer == authority {
            tracing::debug!("Peer {} authorized to sync own authority", peer);
            return Ok(true);
        }

        // Try to get peer's Biscuit token from storage
        // Using storage effects to look up peer tokens by authority ID
        let peer_token_key = format!("peer_tokens/{}", peer);
        let peer_token_bytes = match effects.retrieve(&peer_token_key).await {
            Ok(Some(token_data)) => token_data,
            Ok(None) => {
                tracing::warn!("No Biscuit token found for peer {} during authority sync", peer);
                // For now, allow if no token found (placeholder until proper token management)
                return Ok(true);
            }
            Err(e) => {
                tracing::warn!("Failed to get peer token for {}: {}", peer, e);
                // For now, allow if token lookup fails (placeholder)
                return Ok(true);
            }
        };

        // For now, do basic validation and allow access
        // TODO: Implement full Biscuit token validation when token infrastructure is ready
        if peer_token_bytes.len() >= 32 {
            tracing::debug!("Peer {} has valid token for authority {} sync", peer, authority);
            Ok(true)
        } else {
            tracing::warn!("Invalid token for peer {} during authority sync", peer);
            Ok(false)
        }
    }

    /// Check if peer is a participant in context
    async fn check_context_participant<E: AuraEffects>(
        &self,
        effects: &E,
        peer: &AuthorityId,
        context: &ContextId,
    ) -> Result<bool> {
        // Look for context participant facts in the journal
        let context_key = format!("context_participants/{}", context);
        let participants_data = match effects.retrieve(&context_key).await {
            Ok(Some(data)) => data,
            Ok(None) => {
                tracing::debug!("No explicit participants found for context {}", context);
                // If no explicit participants list, check via token-based authorization
                return self.check_context_authorization_via_token(effects, peer, context).await;
            }
            Err(e) => {
                tracing::debug!("Could not get context participants for {}: {}", context, e);
                // Fall back to token authorization if storage lookup fails
                return self.check_context_authorization_via_token(effects, peer, context).await;
            }
        };

        // Try to deserialize participants list
        if let Ok(participants_str) = String::from_utf8(participants_data) {
            // Simple format: comma-separated authority IDs
            let peer_str = peer.to_string();
            if participants_str.contains(&peer_str) {
                tracing::debug!("Peer {} is explicit participant in context {}", peer, context);
                return Ok(true);
            }
        }

        // If not in explicit participants list, check via token authorization
        self.check_context_authorization_via_token(effects, peer, context).await
    }

    /// Check context authorization via token-based capabilities
    async fn check_context_authorization_via_token<E: AuraEffects>(
        &self,
        effects: &E,
        peer: &AuthorityId,
        context: &ContextId,
    ) -> Result<bool> {
        // Try to get peer's authorization token
        let peer_token_key = format!("peer_tokens/{}", peer);
        let peer_token_bytes = match effects.retrieve(&peer_token_key).await {
            Ok(Some(token)) => token,
            Ok(None) => {
                tracing::debug!("No authorization token found for peer {} during context sync", peer);
                // For now, allow if no token found (placeholder until proper token management)
                return Ok(true);
            }
            Err(e) => {
                tracing::warn!("Failed to get peer token for {}: {}", peer, e);
                // For now, allow if token lookup fails (placeholder)
                return Ok(true);
            }
        };

        // For now, do basic validation and allow access
        // TODO: Implement full Biscuit token validation when token infrastructure is ready
        if peer_token_bytes.len() >= 32 {
            tracing::debug!("Peer {} has valid token for context {} sync", peer, context);
            Ok(true)
        } else {
            tracing::warn!("Invalid token for peer {} during context sync", peer);
            Ok(false)
        }
    }

    /// Process incoming sync request with pagination support
    ///
    /// Pagination is implemented by:
    /// 1. Filtering out facts already known to the requester (via known_fact_ids)
    /// 2. Sorting remaining facts by fact_id for deterministic ordering
    /// 3. Taking up to max_facts from the sorted list
    /// 4. Setting has_more=true if more facts are available
    ///
    /// The requester should include all previously received fact_ids in subsequent
    /// requests to continue pagination.
    pub async fn handle_sync_request<E: AuraEffects>(
        &self,
        effects: &E,
        request: SyncRequest,
    ) -> Result<SyncResponse> {
        // Verify namespace matches
        if request.namespace != self.namespace {
            return Err(AuraError::invalid("Namespace mismatch"));
        }

        // Get facts to sync
        let facts = self.sync_facts(effects, &request.requester).await?;

        // Create paginated response using helper method
        let response = self.create_paginated_response(
            facts,
            &request.known_fact_ids,
            request.max_facts,
        );

        tracing::debug!(
            "Sync request for namespace {:?}: returning {} facts, has_more: {}",
            self.namespace, response.facts.len(), response.has_more
        );

        Ok(response)
    }

    /// Create a paginated response from a list of facts
    ///
    /// This helper method implements the pagination logic that can be reused
    /// across different sync operations.
    fn create_paginated_response(
        &self,
        facts: Vec<Fact>,
        known_fact_ids: &[aura_journal::FactId],
        max_facts: usize,
    ) -> SyncResponse {
        // Filter out already known facts
        let mut unknown_facts: Vec<Fact> = facts
            .into_iter()
            .filter(|f| !known_fact_ids.contains(&f.fact_id))
            .collect();

        // Sort facts for deterministic pagination ordering
        unknown_facts.sort_by(|a, b| a.fact_id.cmp(&b.fact_id));

        let total_unknown = unknown_facts.len();
        
        // Take the requested page size
        let page_facts: Vec<Fact> = unknown_facts
            .into_iter()
            .take(max_facts)
            .collect();

        // Determine if there are more facts available
        let has_more = total_unknown > max_facts;

        SyncResponse {
            namespace: self.namespace.clone(),
            facts: page_facts,
            has_more,
        }
    }

    /// Apply received facts to journal
    pub async fn apply_sync_response<E: AuraEffects>(
        &mut self,
        _effects: &E,
        response: SyncResponse,
    ) -> Result<SyncStats> {
        let start = std::time::Instant::now();
        let mut stats = SyncStats::default();

        // Verify namespace matches
        if response.namespace != self.namespace {
            return Err(AuraError::invalid("Response namespace mismatch"));
        }

        // Apply facts to journal (merge operation with write lock)
        // Journal merge will handle deduplication via semilattice properties
        {
            let mut journal = self.journal.write();
            for fact in response.facts {
                journal.add_fact(fact)?;
                stats.facts_received += 1;
            }
        }

        stats.duration_ms = start.elapsed().as_millis() as u64;
        Ok(stats)
    }
}

/// Anti-entropy protocol for namespace-aware sync
pub struct NamespacedAntiEntropy {
    /// Namespace to sync
    namespace: JournalNamespace,
    /// Sync configuration
    config: SyncConfig,
}

impl NamespacedAntiEntropy {
    /// Create a new anti-entropy protocol instance
    pub fn new(namespace: JournalNamespace) -> Self {
        Self {
            namespace,
            config: SyncConfig::default(),
        }
    }

    /// Create a new anti-entropy protocol instance with custom configuration
    pub fn with_config(namespace: JournalNamespace, config: SyncConfig) -> Self {
        Self { namespace, config }
    }

    /// Run anti-entropy protocol with a peer
    pub async fn run<E: AuraEffects>(
        &self,
        effects: &E,
        journal: Arc<RwLock<Journal>>,
        peer: AuthorityId,
    ) -> Result<SyncStats> {
        let mut sync = NamespacedSync::new(self.namespace.clone(), journal);

        // Create sync request
        let request = SyncRequest {
            namespace: self.namespace.clone(),
            requester: self.get_local_authority(effects).await?,
            known_fact_ids: sync
                .journal
                .read()
                .iter_facts()
                .map(|f| f.fact_id.clone())
                .collect(),
            max_facts: self.config.protocols.anti_entropy.max_digest_entries,
        };

        // Send request to peer and get response
        let response = self.exchange_with_peer(effects, peer, request).await?;

        // Apply received facts
        sync.apply_sync_response(effects, response).await
    }

    /// Get local authority ID
    async fn get_local_authority<E: AuraEffects>(&self, _effects: &E) -> Result<AuthorityId> {
        // TODO: Get from effects or configuration
        Ok(AuthorityId::new())
    }

    /// Exchange sync data with peer
    async fn exchange_with_peer<E: AuraEffects>(
        &self,
        effects: &E,
        peer: AuthorityId,
        request: SyncRequest,
    ) -> Result<SyncResponse> {
        let peer_uuid: Uuid = peer.into();

        // Serialize the sync request
        let request_data = serde_json::to_vec(&request)
            .map_err(|e| sync_serialization_error("SyncRequest", 
                &format!("Failed to serialize sync request: {}", e)))?;

        // Send request to peer and receive response with timeout
        let exchange_future = async {
            // Send the sync request
            effects
                .send_to_peer(peer_uuid, request_data)
                .await
                .map_err(|e| sync_network_error(&format!("Failed to send sync request to peer {}: {}", peer, e)))?;

            // Receive response from the peer
            let (sender_id, response_data) = effects
                .receive()
                .await
                .map_err(|e| sync_network_error(&format!("Failed to receive sync response from peer {}: {}", peer, e)))?;

            // Verify the response came from the expected peer
            if sender_id != peer_uuid {
                return Err(sync_session_error(&format!(
                    "Received sync response from unexpected peer: expected {}, got {}",
                    peer, sender_id
                )));
            }

            // Deserialize the sync response
            let response: SyncResponse = serde_json::from_slice(&response_data)
                .map_err(|e| sync_serialization_error("SyncResponse", 
                    &format!("Failed to deserialize sync response from peer {}: {}", peer, e)))?;

            // Verify namespace consistency
            if response.namespace != request.namespace {
                return Err(sync_session_error(&format!(
                    "Sync response namespace mismatch: expected {:?}, got {:?}",
                    request.namespace, response.namespace
                )));
            }

            Ok(response)
        };

        // Apply timeout from configuration
        let timeout_duration = self.config.network.sync_timeout;
        match tokio::time::timeout(timeout_duration, exchange_future).await {
            Ok(result) => result,
            Err(_) => Err(sync_network_error(&format!(
                "Sync request to peer {} timed out after {:?}",
                peer, timeout_duration
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_namespace_sync_creation() {
        let authority_id = AuthorityId::new();
        let namespace = JournalNamespace::Authority(authority_id);
        let journal = Arc::new(RwLock::new(Journal::new(namespace.clone())));

        let sync = NamespacedSync::new(namespace, journal);

        match sync.namespace {
            JournalNamespace::Authority(id) => assert_eq!(id, authority_id),
            _ => panic!("Wrong namespace type"),
        }
    }
}
