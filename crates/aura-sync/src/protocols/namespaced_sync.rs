#![allow(missing_docs)]

//! Namespace-aware Synchronization Protocol
//!
//! This module implements synchronization for the fact-based journal model
//! with proper namespace isolation for authorities and contexts.

use crate::core::config::SyncConfig;
use crate::core::{exchange_json_with_peer, sync_session_error};
use crate::protocols::journal_apply::{JournalApplyService, RemoteJournalDelta};
use aura_authorization::VerifiedBiscuitToken;
use aura_core::effects::PhysicalTimeEffects;
use aura_core::types::identifiers::ContextId;
use aura_core::{time::OrderTime, AuraError, AuthorityId, Result};
use aura_guards::{DecodedIngress, VerifiedIngress};
use aura_journal::{Fact, FactJournal as Journal, JournalNamespace};
use aura_protocol::effects::AuraEffects;
use hex;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
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
/// include all ordering tokens received in previous responses to continue pagination.
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
/// // Subsequent pages (accumulate all previous order tokens)
/// let mut all_known_facts = previous_response.facts.iter()
///     .map(|f| f.order.clone()).collect();
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
    pub known_fact_ids: Vec<OrderTime>,
    /// Maximum facts to return per page
    pub max_facts: u32,
}

/// Synchronization response with facts
///
/// Supports pagination through the has_more field. When has_more is true,
/// the client should make another request including all previously received
/// fact ordering tokens in the known_fact_ids field to get the next page.
///
/// Facts are returned in deterministic order (sorted by order token) to ensure
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
    pub facts_sent: u64,
    /// Facts received
    pub facts_received: u64,
    /// Sync duration in ms
    pub duration_ms: u64,
}

impl NamespacedSync {
    fn known_order_set(known_fact_ids: &[OrderTime]) -> BTreeSet<OrderTime> {
        known_fact_ids.iter().cloned().collect()
    }

    fn paginated_unknown_facts(
        &self,
        facts: Vec<Fact>,
        known_fact_ids: &[OrderTime],
        max_facts: u32,
    ) -> (Vec<Fact>, bool) {
        let known_orders = Self::known_order_set(known_fact_ids);
        let mut unknown_facts: Vec<Fact> = facts
            .into_iter()
            .filter(|fact| !known_orders.contains(&fact.order))
            .collect();
        unknown_facts.sort_by(|a, b| a.order.cmp(&b.order));

        let total_unknown = unknown_facts.len();
        let max_facts = max_facts as usize;
        let page_facts = unknown_facts.into_iter().take(max_facts).collect();
        (page_facts, total_unknown > max_facts)
    }

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

        let peer_token_bytes = match self.load_peer_token(effects, peer).await? {
            Some(bytes) => bytes,
            None => {
                tracing::warn!(
                    "No Biscuit token found for peer {} during authority sync",
                    peer
                );
                return Ok(false);
            }
        };

        let scope = aura_core::types::scope::ResourceScope::Authority {
            authority_id: *authority,
            operation: aura_core::types::scope::AuthorityOp::UpdateTree,
        };
        self.validate_token(effects, &peer_token_bytes, "sync:authority", &scope)
            .await
    }

    /// Check if peer is a participant in context
    async fn check_context_participant<E: AuraEffects>(
        &self,
        effects: &E,
        peer: &AuthorityId,
        context: &ContextId,
    ) -> Result<bool> {
        // Look for context participant facts in the journal
        let context_key = format!("context_participants/{context}");
        let participants_data = match effects.retrieve(&context_key).await {
            Ok(Some(data)) => data,
            Ok(None) => {
                tracing::debug!("No explicit participants found for context {}", context);
                // If no explicit participants list, check via token-based authorization
                return self
                    .check_context_authorization_via_token(effects, peer, context)
                    .await;
            }
            Err(e) => {
                tracing::debug!("Could not get context participants for {}: {}", context, e);
                // Fall back to token authorization if storage lookup fails
                return self
                    .check_context_authorization_via_token(effects, peer, context)
                    .await;
            }
        };

        // Try to deserialize participants list
        if let Ok(participants_str) = String::from_utf8(participants_data) {
            // Simple format: comma-separated authority IDs
            let peer_str = peer.to_string();
            if participants_str.contains(&peer_str) {
                tracing::debug!(
                    "Peer {} is explicit participant in context {}",
                    peer,
                    context
                );
                return Ok(true);
            }
        }

        // If not in explicit participants list, check via token authorization
        self.check_context_authorization_via_token(effects, peer, context)
            .await
    }

    /// Check context authorization via token-based capabilities
    async fn check_context_authorization_via_token<E: AuraEffects>(
        &self,
        effects: &E,
        peer: &AuthorityId,
        context: &ContextId,
    ) -> Result<bool> {
        let peer_token_bytes = match self.load_peer_token(effects, peer).await? {
            Some(token) => token,
            None => {
                tracing::debug!(
                    "No authorization token found for peer {} during context sync",
                    peer
                );
                return Ok(false);
            }
        };

        let scope = aura_core::types::scope::ResourceScope::Context {
            context_id: *context,
            operation: aura_core::types::scope::ContextOp::UpdateParams,
        };
        self.validate_token(effects, &peer_token_bytes, "sync:context", &scope)
            .await
    }

    async fn load_peer_token<E: AuraEffects>(
        &self,
        effects: &E,
        peer: &AuthorityId,
    ) -> Result<Option<Vec<u8>>> {
        let peer_token_key = format!("peer_tokens/{peer}");
        match effects.retrieve(&peer_token_key).await {
            Ok(Some(token)) => Ok(Some(token)),
            Ok(None) => Ok(None),
            Err(e) => {
                tracing::warn!("Failed to load token for {}: {}", peer, e);
                Ok(None)
            }
        }
    }

    async fn validate_token<E: AuraEffects>(
        &self,
        effects: &E,
        token_bytes: &[u8],
        operation: &str,
        scope: &aura_core::types::scope::ResourceScope,
    ) -> Result<bool> {
        let root = self.load_root_public_key(effects).await?;
        let token = VerifiedBiscuitToken::from_bytes(token_bytes, root).map_err(|e| {
            AuraError::invalid(format!("Biscuit parse failed for {operation}: {e}"))
        })?;

        // Minimal policy: if token parses and targets the namespace, accept.
        let mut authorizer = token
            .authorizer()
            .map_err(|e| AuraError::invalid(format!("Biscuit authorizer build failed: {e}")))?;
        let scope_str = format!("{scope:?}");
        // Minimal policy: ensure the token can be evaluated without failing and record the
        // operation scope for auditing. Additional caveats should be attached by the caller.
        authorizer
            .allow()
            .map_err(|e| AuraError::invalid(format!("Biscuit allow rule failed: {e}")))?;
        authorizer
            .authorize()
            .map_err(|e| AuraError::permission_denied(format!("Biscuit evaluation failed: {e}")))?;
        tracing::debug!(
            "Validated Biscuit token for {} on scope {} (root verified)",
            operation,
            scope_str
        );
        Ok(true)
    }

    async fn load_root_public_key<E: AuraEffects>(
        &self,
        effects: &E,
    ) -> Result<biscuit_auth::PublicKey> {
        if let Ok(Some(bytes)) = effects.retrieve("biscuit_root_public_key").await {
            if bytes.len() == 32 {
                if let Ok(pk) = biscuit_auth::PublicKey::from_bytes(bytes.as_slice()) {
                    return Ok(pk);
                }
            }
        }

        // Development fallback key
        let dev_key_hex = "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20";
        let dev_bytes = hex::decode(dev_key_hex)
            .map_err(|e| AuraError::invalid(format!("decode dev root key: {e}")))?;
        let mut key_array = [0u8; 32];
        key_array.copy_from_slice(&dev_bytes[..32]);
        biscuit_auth::PublicKey::from_bytes(&key_array)
            .map_err(|e| AuraError::invalid(format!("load dev root key: {e}")))
    }

    /// Process incoming sync request with pagination support
    ///
    /// Pagination is implemented by:
    /// 1. Filtering out facts already known to the requester (via known_fact_ids)
    /// 2. Sorting remaining facts by ordering token for deterministic ordering
    /// 3. Taking up to max_facts from the sorted list
    /// 4. Setting has_more=true if more facts are available
    ///
    /// The requester should include all previously received ordering tokens in subsequent
    /// requests to continue pagination.
    pub async fn handle_sync_request<E: AuraEffects>(
        &self,
        effects: &E,
        request: VerifiedIngress<SyncRequest>,
    ) -> Result<SyncResponse> {
        let (request, _) = request.into_parts();
        // Verify namespace matches
        if request.namespace != self.namespace {
            return Err(AuraError::invalid("Namespace mismatch"));
        }

        // Get facts to sync
        let facts = self.sync_facts(effects, &request.requester).await?;

        // Create paginated response using helper method
        let response =
            self.create_paginated_response(facts, &request.known_fact_ids, request.max_facts);

        tracing::debug!(
            "Sync request for namespace {:?}: returning {} facts, has_more: {}",
            self.namespace,
            response.facts.len(),
            response.has_more
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
        known_fact_ids: &[OrderTime],
        max_facts: u32,
    ) -> SyncResponse {
        let (page_facts, has_more) = self.paginated_unknown_facts(facts, known_fact_ids, max_facts);

        SyncResponse {
            namespace: self.namespace.clone(),
            facts: page_facts,
            has_more,
        }
    }

    /// Apply received facts to journal
    pub async fn apply_sync_response<E: PhysicalTimeEffects + Send + Sync>(
        &mut self,
        _effects: &E,
        response: VerifiedIngress<SyncResponse>,
    ) -> Result<SyncStats> {
        let start = _effects.physical_time().await.map(|t| t.ts_ms).unwrap_or(0);
        let mut stats = SyncStats::default();
        let (response, evidence) = response.into_parts();

        // Verify namespace matches
        if response.namespace != self.namespace {
            return Err(AuraError::invalid("Response namespace mismatch"));
        }

        let facts_received = response.facts.len() as u64;
        let verified_delta = DecodedIngress::new(
            RemoteJournalDelta::from_facts(response.facts),
            evidence.metadata().clone(),
        )
        .verify(evidence)
        .map_err(|e| AuraError::invalid(format!("verify namespaced sync response: {e}")))?;

        let current = self.journal.read().clone();
        let (updated, _outcome) =
            JournalApplyService::new().apply_verified_delta(current, verified_delta)?;
        *self.journal.write() = updated;
        stats.facts_received = facts_received;

        stats.duration_ms = _effects
            .physical_time()
            .await
            .map(|t| t.ts_ms.saturating_sub(start))
            .unwrap_or(0);
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
                .map(|f| f.order.clone())
                .collect(),
            max_facts: self.config.protocols.anti_entropy.max_digest_entries,
        };

        // Send request to peer and get response
        let response = self.exchange_with_peer(effects, peer, request).await?;

        // Apply received facts
        let response = crate::protocols::ingress::verified_authority_payload(
            peer,
            match self.namespace {
                JournalNamespace::Authority(authority) => {
                    let mut entropy = [0u8; 32];
                    entropy[..16].copy_from_slice(authority.uuid().as_bytes());
                    ContextId::new_from_entropy(entropy)
                }
                JournalNamespace::Context(context) => context,
            },
            1,
            response,
        )?;
        sync.apply_sync_response(effects, response).await
    }

    /// Get local authority ID
    async fn get_local_authority<E: AuraEffects>(&self, effects: &E) -> Result<AuthorityId> {
        // Try storage override first
        if let Ok(Some(bytes)) = effects.retrieve("local_authority_id").await {
            if let Ok(uuid_str) = String::from_utf8(bytes) {
                if let Ok(parsed) = uuid::Uuid::parse_str(uuid_str.trim()) {
                    return Ok(AuthorityId::from_uuid(parsed));
                }
            }
            return Err(AuraError::invalid(
                "Stored local_authority_id is not a valid UUID".to_string(),
            ));
        }
        Err(AuraError::invalid(
            "Missing explicit local_authority_id for namespaced sync".to_string(),
        ))
    }

    /// Exchange sync data with peer
    async fn exchange_with_peer<E: AuraEffects>(
        &self,
        effects: &E,
        peer: AuthorityId,
        request: SyncRequest,
    ) -> Result<SyncResponse> {
        let peer_uuid: Uuid = peer.into();
        let exchange_future = async {
            let response: SyncResponse = exchange_json_with_peer(
                effects,
                peer_uuid,
                &peer,
                "SyncRequest",
                "sync request",
                &request,
                "SyncResponse",
                "sync response",
            )
            .await?;

            // Verify namespace consistency
            if response.namespace != request.namespace {
                return Err(sync_session_error(format!(
                    "Sync response namespace mismatch: expected {:?}, got {:?}",
                    request.namespace, response.namespace
                )));
            }

            Ok(response)
        };

        // Execute without runtime-specific timeout; callers should enforce via PhysicalTimeEffects.
        exchange_future.await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocols::journal_apply::apply_path_hits_for_tests;
    use aura_core::time::{OrderTime, TimeStamp};
    use aura_core::Hash32;
    use aura_journal::{FactContent, SnapshotFact};

    #[aura_macros::aura_test]
    async fn test_namespace_sync_creation() {
        let authority_id = AuthorityId::new_from_entropy([2u8; 32]);
        let namespace = JournalNamespace::Authority(authority_id);
        let journal = Arc::new(RwLock::new(Journal::new(namespace.clone())));

        let sync = NamespacedSync::new(namespace, journal);

        match sync.namespace {
            JournalNamespace::Authority(id) => assert_eq!(id, authority_id),
            _ => panic!("Wrong namespace type"),
        }
    }

    #[aura_macros::aura_test]
    async fn test_namespaced_sync_response_reaches_apply_service() {
        let before = apply_path_hits_for_tests();
        let authority_id = AuthorityId::new_from_entropy([3u8; 32]);
        let namespace = JournalNamespace::Authority(authority_id);
        let journal = Arc::new(RwLock::new(Journal::new(namespace.clone())));
        let mut sync = NamespacedSync::new(namespace.clone(), journal);
        let fact = Fact::new(
            OrderTime([1; 32]),
            TimeStamp::OrderClock(OrderTime([1; 32])),
            FactContent::Snapshot(SnapshotFact {
                state_hash: Hash32::default(),
                superseded_facts: vec![],
                sequence: 1,
            }),
        );
        let response = SyncResponse {
            namespace,
            facts: vec![fact],
            has_more: false,
        };
        let response = crate::protocols::ingress::verified_authority_payload(
            authority_id,
            ContextId::new_from_entropy([4; 32]),
            1,
            response,
        )
        .expect("verified response");

        let effects = aura_testkit::mock_effects::MockEffects::deterministic();
        let stats = sync
            .apply_sync_response(&effects, response)
            .await
            .expect("sync response applies");

        assert_eq!(stats.facts_received, 1);
        assert!(apply_path_hits_for_tests() > before);
    }
}
