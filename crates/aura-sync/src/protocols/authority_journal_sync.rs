//! Authority-based Journal Synchronization
//!
//! This module provides journal synchronization for the authority-centric model,
//! removing all device ID references and using authority IDs instead.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::core::{binary_deserialize, binary_serialize, SyncResult};
use crate::infrastructure::RetryPolicy;
use crate::protocols::journal_apply::{JournalApplyService, RemoteJournalDelta};
use aura_core::time::OrderTime;
use aura_core::{hash, Authority, AuthorityId, ContextId};
use aura_guards::{DecodedIngress, VerifiedIngress};
use aura_journal::{Fact, FactJournal as Journal, JournalNamespace};
use aura_protocol::effects::AuraEffects;
use std::collections::BTreeSet;

/// Authority-based journal sync configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorityJournalSyncConfig {
    /// Maximum facts to sync in one batch
    pub batch_size: u32,
    /// Sync timeout
    pub timeout: Duration,
    /// Retry policy for failed syncs
    pub retry_policy: RetryPolicy,
    /// Signature verification policy for incoming facts.
    pub signature_policy: AuthorityJournalSignaturePolicy,
}

/// Explicit signature verification policy for authority journal sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthorityJournalSignaturePolicy {
    /// Incoming facts must carry valid authority signatures.
    Required,
}

impl Default for AuthorityJournalSignaturePolicy {
    fn default() -> Self {
        Self::Required
    }
}

impl Default for AuthorityJournalSyncConfig {
    fn default() -> Self {
        Self {
            batch_size: 1000,
            timeout: Duration::from_secs(30),
            retry_policy: RetryPolicy::default(),
            signature_policy: AuthorityJournalSignaturePolicy::Required,
        }
    }
}

/// Authority journal sync protocol
pub struct AuthorityJournalSyncProtocol {
    #[allow(dead_code)]
    config: AuthorityJournalSyncConfig,
}

/// Journal sync digest for efficient delta computation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorityJournalDigest {
    /// Authority ID this digest belongs to
    pub authority_id: AuthorityId,
    /// Number of facts in journal
    pub fact_count: u64,
    /// Merkle root of fact IDs
    pub fact_root: [u8; 32],
    /// Timestamp of digest creation
    pub timestamp: u64,
}

/// Sync session between two authorities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthoritySyncSession {
    /// Local authority
    pub local_authority: AuthorityId,
    /// Remote authority
    pub remote_authority: AuthorityId,
    /// Session ID
    pub session_id: String,
    /// Facts exchanged
    pub facts_exchanged: u64,
}

/// Result of sync operation
#[derive(Debug, Clone)]
pub struct AuthoritySyncResult {
    /// Facts sent to peer
    pub facts_sent: usize,
    /// Facts received from peer
    pub facts_received: usize,
    /// Authorities synchronized with
    pub synchronized_authorities: Vec<AuthorityId>,
    /// Duration of sync
    pub duration: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuthorityFactsFromPeer {
    remote_journal: Journal,
    fact_ids: Vec<OrderTime>,
}

impl AuthorityJournalSyncProtocol {
    fn authority_sync_context(authority_id: AuthorityId) -> ContextId {
        let mut entropy = [0u8; 32];
        entropy[..16].copy_from_slice(authority_id.uuid().as_bytes());
        ContextId::new_from_entropy(entropy)
    }

    async fn elapsed_duration<E: AuraEffects>(&self, effects: &E, start_ms: u64) -> Duration {
        Duration::from_millis(
            effects
                .physical_time()
                .await
                .map(|t| t.ts_ms.saturating_sub(start_ms))
                .unwrap_or(0),
        )
    }

    fn collect_facts_by_orders(
        &self,
        remote_journal: &Journal,
        fact_ids: &[OrderTime],
    ) -> Vec<Fact> {
        let wanted: BTreeSet<_> = fact_ids.iter().cloned().collect();
        remote_journal
            .iter_facts()
            .filter(|fact| wanted.contains(&fact.order))
            .cloned()
            .collect()
    }

    /// Create a new authority journal sync protocol
    pub fn new(config: AuthorityJournalSyncConfig) -> Self {
        Self { config }
    }

    /// Synchronize with a set of peer authorities
    pub async fn sync_with_peers<E: AuraEffects>(
        &self,
        effects: &E,
        local_authority: &dyn Authority,
        peers: Vec<AuthorityId>,
    ) -> SyncResult<AuthoritySyncResult> {
        let start = effects.physical_time().await.map(|t| t.ts_ms).unwrap_or(0);
        let mut result = AuthoritySyncResult {
            facts_sent: 0,
            facts_received: 0,
            synchronized_authorities: vec![],
            duration: Duration::default(),
        };

        // Get local journal
        let local_journal = self
            .get_authority_journal(effects, local_authority.authority_id())
            .await?;

        // Sync with each peer
        for peer_id in peers {
            match self
                .sync_with_authority(effects, local_authority, &local_journal, peer_id)
                .await
            {
                Ok(peer_result) => {
                    result.facts_sent += peer_result.facts_sent;
                    result.facts_received += peer_result.facts_received;
                    result.synchronized_authorities.push(peer_id);
                }
                Err(e) => {
                    // Log error but continue with other peers
                    eprintln!("Failed to sync with authority {peer_id:?}: {e}");
                }
            }
        }

        result.duration = self.elapsed_duration(effects, start).await;
        Ok(result)
    }

    /// Sync with a single authority
    async fn sync_with_authority<E: AuraEffects>(
        &self,
        effects: &E,
        local_authority: &dyn Authority,
        local_journal: &Journal,
        peer_id: AuthorityId,
    ) -> SyncResult<AuthoritySyncResult> {
        // Create sync session
        let _session = AuthoritySyncSession {
            local_authority: local_authority.authority_id(),
            remote_authority: peer_id,
            session_id: effects.random_uuid().await.to_string(),
            facts_exchanged: 0,
        };

        // Exchange digests
        let now = effects.physical_time().await?.ts_ms;
        let local_digest = self.compute_digest(local_authority.authority_id(), local_journal, now);
        let remote_digest = self.request_digest(effects, peer_id).await?;

        // Fetch remote journal snapshot
        let remote_journal = self.get_authority_journal(effects, peer_id).await?;

        // Compute delta based on current journal snapshots
        let (to_send, to_receive) = self.compute_delta(
            local_journal,
            &remote_journal,
            &local_digest,
            &remote_digest,
        );

        // Apply local → remote (persist remote journal with new facts)
        let facts_sent = self
            .send_facts(effects, peer_id, remote_journal.clone(), to_send)
            .await?;

        // Apply remote → local (persist local journal with received facts)
        let facts_from_peer = crate::protocols::ingress::verified_authority_payload(
            peer_id,
            Self::authority_sync_context(peer_id),
            1,
            AuthorityFactsFromPeer {
                remote_journal,
                fact_ids: to_receive,
            },
        )?;
        let facts_received = self
            .receive_facts(effects, peer_id, facts_from_peer)
            .await?;
        let facts_received_count = facts_received.payload().len();

        let (payload, evidence) = facts_received.into_parts();
        let verified_delta = DecodedIngress::new(
            RemoteJournalDelta::from_facts(payload),
            evidence.metadata().clone(),
        )
        .verify(evidence)
        .map_err(|e| {
            aura_core::AuraError::invalid(format!("verify authority journal delta: {e}"))
        })?;
        let (merged_local, _outcome) = JournalApplyService::new()
            .apply_verified_delta(local_journal.clone(), verified_delta)?;
        self.persist_authority_journal(effects, local_authority.authority_id(), &merged_local)
            .await?;

        Ok(AuthoritySyncResult {
            facts_sent: facts_sent.len(),
            facts_received: facts_received_count,
            synchronized_authorities: vec![peer_id],
            duration: Duration::default(),
        })
    }

    /// Get journal for an authority
    async fn get_authority_journal<E: AuraEffects>(
        &self,
        effects: &E,
        authority_id: AuthorityId,
    ) -> SyncResult<Journal> {
        let key = Self::storage_key(authority_id);
        let maybe_bytes = effects
            .retrieve(&key)
            .await
            .map_err(|e| aura_core::AuraError::storage(format!("load journal: {e}")))?;

        if let Some(bytes) = maybe_bytes {
            binary_deserialize("journal", "stored authority journal", &bytes)
        } else {
            Ok(Journal::new(JournalNamespace::Authority(authority_id)))
        }
    }

    /// Compute digest of journal
    fn compute_digest(
        &self,
        authority_id: AuthorityId,
        journal: &Journal,
        timestamp: u64,
    ) -> AuthorityJournalDigest {
        let facts: Vec<&Fact> = journal.iter_facts().collect();
        let fact_count = facts.len() as u64;

        let mut leaf_hashes: Vec<[u8; 32]> = facts
            .iter()
            .map(|fact| {
                binary_serialize("fact", "authority journal fact", *fact)
                    .map(|bytes| hash::hash(&bytes))
            })
            .collect::<Result<Vec<_>, _>>()
            .unwrap_or_default();

        // Deterministic merkle-like root: hash concatenated sorted leaves
        leaf_hashes.sort();
        let mut accumulator = Vec::with_capacity(leaf_hashes.len() * 32);
        for leaf in &leaf_hashes {
            accumulator.extend_from_slice(leaf);
        }
        let fact_root = hash::hash(&accumulator);

        AuthorityJournalDigest {
            authority_id,
            fact_count,
            fact_root,
            timestamp,
        }
    }

    /// Request digest from peer
    async fn request_digest<E: AuraEffects>(
        &self,
        effects: &E,
        peer_id: AuthorityId,
    ) -> SyncResult<AuthorityJournalDigest> {
        let peer_journal = self.get_authority_journal(effects, peer_id).await?;
        let now = effects.physical_time().await?.ts_ms;
        Ok(self.compute_digest(peer_id, &peer_journal, now))
    }

    /// Compute facts to exchange
    fn compute_delta(
        &self,
        local_journal: &Journal,
        remote_journal: &Journal,
        _local_digest: &AuthorityJournalDigest,
        _remote_digest: &AuthorityJournalDigest,
    ) -> (Vec<Fact>, Vec<OrderTime>) {
        let local_set: BTreeSet<_> = local_journal.facts.iter().cloned().collect();
        let remote_set: BTreeSet<_> = remote_journal.facts.iter().cloned().collect();

        // Facts to send: present locally but not remotely
        let mut to_send: Vec<Fact> = local_set
            .difference(&remote_set)
            .take(self.config.batch_size as usize)
            .cloned()
            .collect();

        // Facts to receive: present remotely but not locally (represented by order)
        let mut to_receive: Vec<OrderTime> = remote_set
            .difference(&local_set)
            .map(|f| f.order.clone())
            .take(self.config.batch_size as usize)
            .collect();

        to_send.shrink_to_fit();
        to_receive.shrink_to_fit();

        (to_send, to_receive)
    }

    /// Send facts to peer (storage-backed merge through canonical apply boundary)
    async fn send_facts<E: AuraEffects>(
        &self,
        effects: &E,
        peer_id: AuthorityId,
        peer_journal: Journal,
        facts: Vec<Fact>,
    ) -> SyncResult<Vec<Fact>> {
        if facts.is_empty() {
            return Ok(facts);
        }

        let verified_delta = crate::protocols::ingress::verified_authority_payload(
            peer_id,
            Self::authority_sync_context(peer_id),
            1,
            RemoteJournalDelta::from_facts(facts.clone()),
        )?;
        let (delta, evidence) = verified_delta.into_parts();
        let verified_delta = DecodedIngress::new(delta, evidence.metadata().clone())
            .verify(evidence)
            .map_err(|e| {
                aura_core::AuraError::invalid(format!("verify authority send facts: {e}"))
            })?;
        let (peer_journal, _outcome) =
            JournalApplyService::new().apply_verified_delta(peer_journal, verified_delta)?;

        self.persist_authority_journal(effects, peer_id, &peer_journal)
            .await?;
        Ok(facts)
    }

    /// Receive facts from peer (select missing facts by order)
    async fn receive_facts<E: AuraEffects>(
        &self,
        _effects: &E,
        peer_id: AuthorityId,
        facts_from_peer: VerifiedIngress<AuthorityFactsFromPeer>,
    ) -> SyncResult<VerifiedIngress<Vec<Fact>>> {
        let (
            AuthorityFactsFromPeer {
                remote_journal,
                fact_ids,
            },
            evidence,
        ) = facts_from_peer.into_parts();

        if fact_ids.is_empty() {
            let verified_empty = DecodedIngress::new(Vec::new(), evidence.metadata().clone())
                .verify(evidence)
                .map_err(|e| {
                    aura_core::AuraError::invalid(format!("verify empty authority facts: {e}"))
                })?;
            return Ok(verified_empty);
        }

        let received = self.collect_facts_by_orders(&remote_journal, &fact_ids);

        tracing::debug!(
            "Fetched {} facts from peer {} based on delta plan",
            received.len(),
            peer_id
        );

        DecodedIngress::new(received, evidence.metadata().clone())
            .verify(evidence)
            .map_err(|e| aura_core::AuraError::invalid(format!("verify authority facts: {e}")))
    }

    async fn persist_authority_journal<E: AuraEffects>(
        &self,
        effects: &E,
        authority_id: AuthorityId,
        journal: &Journal,
    ) -> SyncResult<()> {
        let key = Self::storage_key(authority_id);
        let bytes = binary_serialize("journal", "authority journal", journal)?;
        effects
            .store(&key, bytes)
            .await
            .map_err(|e| aura_core::AuraError::storage(format!("persist journal: {e}")))
    }

    fn storage_key(authority_id: AuthorityId) -> String {
        format!("authority_journal/{authority_id}")
    }
}

/// Create default sync protocol
pub fn create_default_sync_protocol() -> AuthorityJournalSyncProtocol {
    AuthorityJournalSyncProtocol::new(AuthorityJournalSyncConfig::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_config_defaults() {
        let config = AuthorityJournalSyncConfig::default();
        assert_eq!(config.batch_size, 1000);
        assert_eq!(config.timeout, Duration::from_secs(30));
        assert_eq!(
            config.signature_policy,
            AuthorityJournalSignaturePolicy::Required
        );
    }
}
