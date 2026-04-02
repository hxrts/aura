//! Runtime-owned hold custody service.
//!
//! Owns shared `Hold` substrate state for deferred delivery and cache replicas:
//! selector-based retrieval, bounded rotating holder selection, local GC, and
//! verified-only accountability updates.

use super::config_profiles::impl_service_config_profiles;
use super::service_registry::ServiceRegistry;
use super::state::with_state_mut_validated;
use super::traits::{RuntimeService, RuntimeServiceContext, ServiceError, ServiceHealth};
use async_trait::async_trait;
use aura_core::hash::hash;
use aura_core::service::{
    AccountabilityReplyBlock, HoldDepositReplyBlock, HoldDepositRequest, HoldRequestError,
    HoldRetentionMetadata, HoldRetrievalReplyBlock, HoldRetrievalRequest, MoveReceiptReplyBlock,
    ProviderCandidate, ProviderEvidence, ReplyBlockError, RetrievalCapability,
    RetrievalCapabilityError, SelectionState, ServiceFamily, ServiceProfile,
};
use aura_core::types::epochs::Epoch;
use aura_core::types::identifiers::{AuthorityId, ContextId};
use aura_maintenance::{CacheInvalidated, CacheKey, MaintenanceEpoch};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;

#[allow(dead_code)] // Declaration-layer ingress inventory; sanctioned surfaces call methods directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HoldManagerCommand {
    Deposit,
    Retrieve,
    QueueSyncRetrieval,
    DrainSyncWindow,
    RotateCapabilities,
    GarbageCollect,
    IssueReplyBlock,
    VerifyWitness,
    ApplyVerifiedWitness,
}

/// Configuration for the shared `Hold` substrate.
#[derive(Debug, Clone)]
pub struct HoldManagerConfig {
    /// Max active holders selected for one scope at a time.
    pub max_active_holders: usize,
    /// Residency window before holder rotation is forced.
    pub residency_window_turns: u32,
    /// Max accepted retention for a single deposit.
    pub max_retention_ms: u64,
    /// Default retrieval-capability TTL.
    pub capability_ttl_ms: u64,
    /// Rotation window before capability expiry.
    pub capability_rotation_window_ms: u64,
    /// Reply-block TTL.
    pub reply_block_ttl_ms: u64,
    /// Jitter/batching delay before sync-eligible replies are drained.
    pub reply_jitter_ms: u64,
    /// Max number of pending sync-eligible retrievals or replies to drain per batch.
    pub sync_batch_size: usize,
    /// Max opaque custody bytes retained in local state before storage-pressure GC.
    pub storage_limit_bytes: usize,
}

impl Default for HoldManagerConfig {
    fn default() -> Self {
        Self {
            max_active_holders: 3,
            residency_window_turns: 2,
            // Phase-6 tuning increased the neighborhood hold window so sparse
            // sync and weak-connectivity profiles still retain availability.
            max_retention_ms: 120_000,
            capability_ttl_ms: 30_000,
            // Selector rotation now starts earlier so weak/sparse profiles can
            // refresh before expiry without falling back to stale selectors.
            capability_rotation_window_ms: 10_000,
            reply_block_ttl_ms: 15_000,
            reply_jitter_ms: 250,
            sync_batch_size: 16,
            storage_limit_bytes: 256 * 1024,
        }
    }
}

impl_service_config_profiles!(HoldManagerConfig {
    /// Short deterministic config for tests.
    pub fn for_testing() -> Self {
        Self {
            max_active_holders: 2,
            residency_window_turns: 1,
            max_retention_ms: 2_000,
            capability_ttl_ms: 750,
            capability_rotation_window_ms: 100,
            reply_block_ttl_ms: 1_000,
            reply_jitter_ms: 25,
            sync_batch_size: 4,
            storage_limit_bytes: 4 * 1024,
        }
    }
});

/// Runtime-local local-index projection kept separate from held objects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldLocalIndexEntry {
    pub scope: ContextId,
    pub content_key: String,
    pub profile: ServiceProfile,
    pub selector_count: usize,
    pub last_observed_ms: u64,
}

/// Provider-budget snapshot updated only after witness verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldBudgetSnapshot {
    pub authority_id: AuthorityId,
    pub outstanding_holds: u32,
    pub success_count: u32,
    pub failure_count: u32,
    pub admission_penalty: u32,
    pub last_verified_ms: Option<u64>,
}

/// Summary projection for hold runtime state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldProjection {
    pub stored_objects: usize,
    pub active_selectors: usize,
    pub pending_sync_retrievals: usize,
    pub pending_sync_replies: usize,
    pub local_indexes: usize,
    pub total_custody_bytes: usize,
}

/// Result of holder selection under bounded residency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldSelectionPlan {
    pub selected_authorities: Vec<AuthorityId>,
    pub rotated: bool,
    pub bounded_residency_remaining: u32,
}

/// Result of a hold deposit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldDepositOutcome {
    pub retention: HoldRetentionMetadata,
    pub retrieval_capability: RetrievalCapability,
    pub selected_holders: Vec<AuthorityId>,
    pub witness: AccountabilityWitness,
    pub reply_block: HoldDepositReplyBlock,
    pub maintenance_epoch: MaintenanceEpoch,
    pub invalidation: CacheInvalidated,
}

/// Retrieval result classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldRetrievalStatus {
    Success,
    Miss,
    StaleCapability,
}

/// Result of a hold retrieval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldRetrievalOutcome {
    pub status: HoldRetrievalStatus,
    pub held_object: Option<aura_core::HeldObject>,
    pub witness: AccountabilityWitness,
    pub reply_block: HoldRetrievalReplyBlock,
    pub next_capability: Option<RetrievalCapability>,
    pub redeposit_on_miss: bool,
}

/// Result of capability rotation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityRotationOutcome {
    pub rotated_selectors: Vec<[u8; 32]>,
}

/// Result of local hold GC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldGcOutcome {
    pub removed_objects: usize,
    pub invalidations: Vec<CacheInvalidated>,
}

/// Sync-blended retrieval/reply window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldSyncBatch {
    pub retrievals: Vec<QueuedSyncRetrieval>,
    pub replies: Vec<QueuedAccountabilityReply>,
}

/// Runtime-local accountability witness shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountabilityWitness {
    pub kind: AccountabilityWitnessKind,
    pub scope: ContextId,
    pub family: ServiceFamily,
    pub profile: Option<ServiceProfile>,
    pub providers: Vec<AuthorityId>,
    pub command_scope: [u8; 32],
    pub selector: Option<[u8; 32]>,
    pub observed_at_ms: u64,
    pub success: bool,
}

/// Explicit verifier roles for accountability consequences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifierRole {
    AdjacentMoveHop,
    HoldDepositor,
    HoldRetriever,
    HoldAuditor,
}

/// Witness kinds handled by the hold/runtime accountability path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountabilityWitnessKind {
    MoveReceipt,
    HoldDeposit,
    HoldRetrieval,
    HoldAudit,
}

/// Queue entry for sync-blended retrieval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedSyncRetrieval {
    pub request: HoldRetrievalRequest,
    pub queued_at_ms: u64,
    pub deadline_ms: u64,
}

/// Queue entry for sync-blended accountability replies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedAccountabilityReply {
    pub kind: AccountabilityWitnessKind,
    pub scope: ContextId,
    pub token: [u8; 32],
    pub available_at_ms: u64,
    pub deadline_ms: u64,
}

/// Verified witness token required before local budget or scoring changes.
#[allow(clippy::manual_non_exhaustive)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedServiceWitness {
    pub family: ServiceFamily,
    pub providers: Vec<AuthorityId>,
    pub observed_at_ms: u64,
    pub success: bool,
    pub outstanding_hold_delta: i32,
    _sealed: (),
}

/// Errors produced by the hold manager.
#[derive(Debug, thiserror::Error)]
pub enum HoldManagerError {
    #[error(transparent)]
    Request(#[from] HoldRequestError),
    #[error(transparent)]
    Capability(#[from] RetrievalCapabilityError),
    #[error(transparent)]
    ReplyBlock(#[from] ReplyBlockError),
    #[error("no admissible neighborhood holders for scope {scope}")]
    NoNeighborhoodProviders { scope: ContextId },
    #[error("no stored object matches selector")]
    SelectorMiss,
}

#[derive(Debug, Clone)]
struct SelectorRecord {
    capability: RetrievalCapability,
    used: bool,
}

#[derive(Debug, Clone)]
struct StoredHeldObject {
    profile: ServiceProfile,
    held_object: aura_core::HeldObject,
    retention: HoldRetentionMetadata,
    #[allow(dead_code)]
    handoff: Option<aura_core::MoveToHoldHandoff>,
    #[allow(dead_code)]
    maintenance_epoch: MaintenanceEpoch,
    selectors: Vec<SelectorRecord>,
    selected_holders: Vec<AuthorityId>,
    retrieve_once_consumed: bool,
    retrieval_count: u32,
    invalidation: CacheInvalidated,
    last_observed_ms: u64,
}

#[derive(Debug, Clone)]
struct ReplyBlockRecord {
    #[allow(dead_code)]
    scope: ContextId,
    #[allow(dead_code)]
    kind: AccountabilityWitnessKind,
    command_scope: [u8; 32],
    valid_until: u64,
    used: bool,
}

#[derive(Debug, Clone, Default)]
struct ProviderBudgetAccount {
    outstanding_holds: u32,
    success_count: u32,
    failure_count: u32,
    admission_penalty: u32,
    last_verified_ms: Option<u64>,
}

struct HoldState {
    objects: HashMap<(ContextId, String), StoredHeldObject>,
    selector_index: HashMap<[u8; 32], (ContextId, String)>,
    local_index: HashMap<(ContextId, String), HoldLocalIndexEntry>,
    provider_loads: HashMap<AuthorityId, usize>,
    provider_budget: HashMap<AuthorityId, ProviderBudgetAccount>,
    reply_blocks: HashMap<[u8; 32], ReplyBlockRecord>,
    pending_sync_retrievals: VecDeque<QueuedSyncRetrieval>,
    pending_sync_replies: VecDeque<QueuedAccountabilityReply>,
    lifecycle: ServiceHealth,
    total_custody_bytes: usize,
}

impl Default for HoldState {
    fn default() -> Self {
        Self {
            objects: HashMap::new(),
            selector_index: HashMap::new(),
            local_index: HashMap::new(),
            provider_loads: HashMap::new(),
            provider_budget: HashMap::new(),
            reply_blocks: HashMap::new(),
            pending_sync_retrievals: VecDeque::new(),
            pending_sync_replies: VecDeque::new(),
            lifecycle: ServiceHealth::NotStarted,
            total_custody_bytes: 0,
        }
    }
}

impl HoldState {
    fn validate(&self) -> Result<(), super::invariant::InvariantViolation> {
        for ((scope, content_key), stored) in &self.objects {
            if &stored.held_object.scope != scope {
                return Err(super::invariant::InvariantViolation::new(
                    "HoldManager",
                    "stored held object key/scope mismatch",
                ));
            }
            if &content_key_for(&stored.held_object) != content_key {
                return Err(super::invariant::InvariantViolation::new(
                    "HoldManager",
                    "stored held object content key mismatch",
                ));
            }
        }
        Ok(())
    }
}

/// Actor-owned shared hold substrate.
#[aura_macros::service_surface(
    families = "Hold",
    object_categories = "authoritative_shared,transport_protocol,runtime_derived_local,proof_accounting",
    discover = "rendezvous_hold_surface_and_cached_service_descriptors",
    permit = "social_manager_neighborhood_candidates_only",
    transfer = "hold_manager_custody_and_selector_retrieval",
    select = "hold_manager_rotation_and_service_registry",
    authoritative = "HeldObject,RetrievalCapability,ServiceDescriptor",
    runtime_local = "hold_custody_store,hold_local_index,reply_block_registry,provider_budget",
    category = "service_surface"
)]
#[aura_macros::actor_owned(
    owner = "hold_manager",
    domain = "hold",
    gate = "hold_command_ingress",
    command = HoldManagerCommand,
    capacity = 128,
    category = "actor_owned"
)]
#[derive(Clone)]
pub struct HoldManager {
    authority_id: AuthorityId,
    config: HoldManagerConfig,
    registry: Arc<ServiceRegistry>,
    state: Arc<RwLock<HoldState>>,
}

impl HoldManager {
    pub fn new(
        authority_id: AuthorityId,
        config: HoldManagerConfig,
        registry: Arc<ServiceRegistry>,
    ) -> Self {
        Self {
            authority_id,
            config,
            registry,
            state: Arc::new(RwLock::new(HoldState {
                lifecycle: ServiceHealth::NotStarted,
                ..HoldState::default()
            })),
        }
    }

    pub fn config(&self) -> &HoldManagerConfig {
        &self.config
    }

    pub async fn projection(&self) -> HoldProjection {
        let state = self.state.read().await;
        HoldProjection {
            stored_objects: state.objects.len(),
            active_selectors: state.selector_index.len(),
            pending_sync_retrievals: state.pending_sync_retrievals.len(),
            pending_sync_replies: state.pending_sync_replies.len(),
            local_indexes: state.local_index.len(),
            total_custody_bytes: state.total_custody_bytes,
        }
    }

    pub async fn local_index_entries(&self, scope: ContextId) -> Vec<HoldLocalIndexEntry> {
        self.state
            .read()
            .await
            .local_index
            .values()
            .filter(|entry| entry.scope == scope)
            .cloned()
            .collect()
    }

    pub async fn provider_budget(&self, authority_id: AuthorityId) -> Option<HoldBudgetSnapshot> {
        self.state
            .read()
            .await
            .provider_budget
            .get(&authority_id)
            .map(|account| HoldBudgetSnapshot {
                authority_id,
                outstanding_holds: account.outstanding_holds,
                success_count: account.success_count,
                failure_count: account.failure_count,
                admission_penalty: account.admission_penalty,
                last_verified_ms: account.last_verified_ms,
            })
    }

    pub async fn select_holders(
        &self,
        scope: ContextId,
        candidates: &[ProviderCandidate],
        _now_ms: u64,
        maintenance_epoch: MaintenanceEpoch,
    ) -> Result<HoldSelectionPlan, HoldManagerError> {
        let mut admissible = candidates
            .iter()
            .filter(|candidate| candidate.family == ServiceFamily::Hold)
            .filter(|candidate| {
                candidate
                    .evidence
                    .iter()
                    .all(|evidence| *evidence == ProviderEvidence::Neighborhood)
            })
            .cloned()
            .collect::<Vec<_>>();
        if admissible.is_empty() {
            return Err(HoldManagerError::NoNeighborhoodProviders { scope });
        }

        admissible.sort_by_key(|candidate| candidate.authority_id);

        let existing = self
            .registry
            .selection_state(scope, ServiceFamily::Hold)
            .await
            .filter(|state| state.epoch == Some(maintenance_epoch.identity_epoch.value()));

        if let Some(ref existing) = existing {
            let still_admissible = existing.selected_authorities.iter().all(|authority| {
                admissible
                    .iter()
                    .any(|candidate| candidate.authority_id == *authority)
            });
            if still_admissible && existing.bounded_residency_remaining.unwrap_or_default() > 0 {
                let next_remaining = existing
                    .bounded_residency_remaining
                    .unwrap_or_default()
                    .saturating_sub(1);
                let plan = HoldSelectionPlan {
                    selected_authorities: existing.selected_authorities.clone(),
                    rotated: false,
                    bounded_residency_remaining: next_remaining,
                };
                self.registry
                    .record_selection_state(
                        scope,
                        SelectionState {
                            family: ServiceFamily::Hold,
                            selected_authorities: plan.selected_authorities.clone(),
                            epoch: Some(maintenance_epoch.identity_epoch.value()),
                            bounded_residency_remaining: Some(next_remaining),
                        },
                    )
                    .await;
                return Ok(plan);
            }
        }

        let state = self.state.read().await;
        let mut ranked = admissible
            .into_iter()
            .map(|candidate| {
                let load = state
                    .provider_loads
                    .get(&candidate.authority_id)
                    .copied()
                    .unwrap_or_default();
                let budget = state
                    .provider_budget
                    .get(&candidate.authority_id)
                    .cloned()
                    .unwrap_or_default();
                (candidate, load, budget)
            })
            .collect::<Vec<_>>();
        drop(state);

        ranked.sort_by_key(|(candidate, load, budget)| {
            (
                budget.admission_penalty,
                usize::MAX.saturating_sub(candidate.reachable as usize),
                *load,
                candidate.authority_id,
            )
        });

        let rotation_offset = existing
            .as_ref()
            .and_then(|selection| selection.selected_authorities.first().copied())
            .and_then(|first| {
                ranked
                    .iter()
                    .position(|(candidate, _, _)| candidate.authority_id == first)
            })
            .map(|index| index.saturating_add(1))
            .unwrap_or_default();
        let limit = self.config.max_active_holders.min(ranked.len()).max(1);

        let selected = (0..limit)
            .map(|offset| {
                ranked[(rotation_offset + offset) % ranked.len()]
                    .0
                    .authority_id
            })
            .collect::<Vec<_>>();
        let remaining = self.config.residency_window_turns.saturating_sub(1);
        self.registry
            .record_selection_state(
                scope,
                SelectionState {
                    family: ServiceFamily::Hold,
                    selected_authorities: selected.clone(),
                    epoch: Some(maintenance_epoch.identity_epoch.value()),
                    bounded_residency_remaining: Some(remaining),
                },
            )
            .await;
        Ok(HoldSelectionPlan {
            selected_authorities: selected,
            rotated: true,
            bounded_residency_remaining: remaining,
        })
    }

    pub async fn deposit(
        &self,
        request: HoldDepositRequest,
        candidates: &[ProviderCandidate],
        now_ms: u64,
        maintenance_epoch: MaintenanceEpoch,
    ) -> Result<HoldDepositOutcome, HoldManagerError> {
        request.validate_profile()?;
        let plan = self
            .select_holders(
                request.held_object.scope,
                candidates,
                now_ms,
                maintenance_epoch,
            )
            .await?;

        let accepted_ms = request
            .requested_retention_ms
            .min(self.config.max_retention_ms);
        let retention = HoldRetentionMetadata {
            requested_ms: request.requested_retention_ms,
            accepted_ms,
            deposit_epoch: maintenance_epoch.identity_epoch,
            deposited_at_ms: now_ms,
            expires_at_ms: now_ms.saturating_add(accepted_ms),
        };
        let capability = self.issue_capability(
            request.held_object.scope,
            &request.held_object,
            maintenance_epoch.identity_epoch,
            now_ms,
        );
        let content_key = content_key_for(&request.held_object);
        let invalidation = CacheInvalidated::new(
            self.authority_id,
            vec![CacheKey(format!("hold:{content_key}"))],
            maintenance_epoch.identity_epoch,
        );
        let reply_block = HoldDepositReplyBlock {
            inner: self.issue_reply_block(
                request.held_object.scope,
                AccountabilityWitnessKind::HoldDeposit,
                command_scope(
                    &request.held_object.scope,
                    &content_key,
                    now_ms,
                    b"hold-deposit",
                ),
                now_ms,
            ),
        };
        let witness = AccountabilityWitness {
            kind: AccountabilityWitnessKind::HoldDeposit,
            scope: request.held_object.scope,
            family: ServiceFamily::Hold,
            profile: Some(request.profile.clone()),
            providers: plan.selected_authorities.clone(),
            command_scope: reply_block.inner.command_scope,
            selector: Some(capability.selector),
            observed_at_ms: now_ms,
            success: true,
        };

        let requested_profile = request.profile.clone();
        let held_object = request.held_object.clone();
        let handoff = request.handoff.clone();
        let selected_holders = plan.selected_authorities.clone();
        let capability_for_store = capability.clone();
        let reply_token = reply_block.inner.token;
        let local_index_profile = requested_profile.clone();
        with_state_mut_validated(
            &self.state,
            |state| {
                let bytes = held_object.ciphertext.len();
                state.total_custody_bytes = state.total_custody_bytes.saturating_add(bytes);
                state.selector_index.insert(
                    capability_for_store.selector,
                    (held_object.scope, content_key.clone()),
                );
                state.objects.insert(
                    (held_object.scope, content_key.clone()),
                    StoredHeldObject {
                        profile: requested_profile,
                        held_object: held_object.clone(),
                        retention: retention.clone(),
                        handoff,
                        maintenance_epoch,
                        selectors: vec![SelectorRecord {
                            capability: capability_for_store.clone(),
                            used: false,
                        }],
                        selected_holders: selected_holders.clone(),
                        retrieve_once_consumed: false,
                        retrieval_count: 0,
                        invalidation: invalidation.clone(),
                        last_observed_ms: now_ms,
                    },
                );
                state.local_index.insert(
                    (held_object.scope, content_key.clone()),
                    HoldLocalIndexEntry {
                        scope: held_object.scope,
                        content_key: content_key.clone(),
                        profile: local_index_profile.clone(),
                        selector_count: 1,
                        last_observed_ms: now_ms,
                    },
                );
                for authority in &selected_holders {
                    *state.provider_loads.entry(*authority).or_default() += 1;
                }
                state.reply_blocks.insert(
                    reply_token,
                    ReplyBlockRecord {
                        scope: held_object.scope,
                        kind: AccountabilityWitnessKind::HoldDeposit,
                        command_scope: reply_block.inner.command_scope,
                        valid_until: reply_block.inner.valid_until,
                        used: false,
                    },
                );
                state
                    .pending_sync_replies
                    .push_back(QueuedAccountabilityReply {
                        kind: AccountabilityWitnessKind::HoldDeposit,
                        scope: held_object.scope,
                        token: reply_token,
                        available_at_ms: now_ms.saturating_add(self.config.reply_jitter_ms),
                        deadline_ms: reply_block.inner.valid_until,
                    });
            },
            HoldState::validate,
        )
        .await;

        for authority in &plan.selected_authorities {
            self.registry
                .record_hold_observation(
                    request.held_object.scope,
                    *authority,
                    now_ms,
                    held_object
                        .retention_until
                        .or(Some(retention.expires_at_ms)),
                )
                .await;
        }

        Ok(HoldDepositOutcome {
            retention,
            retrieval_capability: capability,
            selected_holders: plan.selected_authorities,
            witness,
            reply_block,
            maintenance_epoch,
            invalidation,
        })
    }

    pub async fn retrieve(
        &self,
        request: HoldRetrievalRequest,
        now_ms: u64,
        maintenance_epoch: MaintenanceEpoch,
    ) -> Result<HoldRetrievalOutcome, HoldManagerError> {
        request.validate_profile()?;
        let key = {
            let state = self.state.read().await;
            state.selector_index.get(&request.selector).cloned()
        };
        let reply_block = HoldRetrievalReplyBlock {
            inner: self.issue_reply_block(
                request.scope,
                AccountabilityWitnessKind::HoldRetrieval,
                command_scope(
                    &request.scope,
                    &hex_selector(request.selector),
                    now_ms,
                    b"hold-retrieve",
                ),
                now_ms,
            ),
        };

        let Some((scope, content_key)) = key else {
            let witness = AccountabilityWitness {
                kind: AccountabilityWitnessKind::HoldRetrieval,
                scope: request.scope,
                family: ServiceFamily::Hold,
                profile: Some(request.profile),
                providers: Vec::new(),
                command_scope: reply_block.inner.command_scope,
                selector: Some(request.selector),
                observed_at_ms: now_ms,
                success: false,
            };
            with_state_mut_validated(
                &self.state,
                |state| {
                    state.reply_blocks.insert(
                        reply_block.inner.token,
                        ReplyBlockRecord {
                            scope: request.scope,
                            kind: AccountabilityWitnessKind::HoldRetrieval,
                            command_scope: reply_block.inner.command_scope,
                            valid_until: reply_block.inner.valid_until,
                            used: false,
                        },
                    );
                    state
                        .pending_sync_replies
                        .push_back(QueuedAccountabilityReply {
                            kind: AccountabilityWitnessKind::HoldRetrieval,
                            scope: request.scope,
                            token: reply_block.inner.token,
                            available_at_ms: now_ms.saturating_add(self.config.reply_jitter_ms),
                            deadline_ms: reply_block.inner.valid_until,
                        });
                },
                HoldState::validate,
            )
            .await;
            return Ok(HoldRetrievalOutcome {
                status: HoldRetrievalStatus::Miss,
                held_object: None,
                witness,
                reply_block,
                next_capability: None,
                redeposit_on_miss: true,
            });
        };

        let mut next_capability = None;
        let mut held_object = None;
        let mut success = false;
        let mut providers = Vec::new();
        let mut status = HoldRetrievalStatus::Miss;
        let mut release_on_verify = false;
        let reply_token = reply_block.inner.token;

        with_state_mut_validated(
            &self.state,
            |state| {
                let Some(stored) = state.objects.get_mut(&(scope, content_key.clone())) else {
                    return;
                };
                stored.last_observed_ms = now_ms;
                providers = stored.selected_holders.clone();
                let selector_record = stored
                    .selectors
                    .iter_mut()
                    .find(|selector| selector.capability.selector == request.selector);
                let Some(selector_record) = selector_record else {
                    status = HoldRetrievalStatus::Miss;
                    return;
                };
                match selector_record
                    .capability
                    .validate_for(now_ms, maintenance_epoch.identity_epoch)
                {
                    Ok(()) => {
                        success = true;
                        status = HoldRetrievalStatus::Success;
                        held_object = Some(stored.held_object.clone());
                        selector_record.used = true;
                        stored.retrieval_count = stored.retrieval_count.saturating_add(1);
                        if stored.profile == ServiceProfile::DeferredDeliveryHold {
                            stored.retrieve_once_consumed = true;
                            release_on_verify = true;
                        } else if selector_record
                            .capability
                            .valid_until
                            .saturating_sub(now_ms)
                            <= self.config.capability_rotation_window_ms
                        {
                            let rotated = self.issue_capability(
                                scope,
                                &stored.held_object,
                                maintenance_epoch.identity_epoch,
                                now_ms,
                            );
                            next_capability = Some(rotated.clone());
                            state
                                .selector_index
                                .insert(rotated.selector, (scope, content_key.clone()));
                            stored.selectors.push(SelectorRecord {
                                capability: rotated,
                                used: false,
                            });
                        }
                    }
                    Err(_) => {
                        status = HoldRetrievalStatus::StaleCapability;
                    }
                }
                state.reply_blocks.insert(
                    reply_token,
                    ReplyBlockRecord {
                        scope,
                        kind: AccountabilityWitnessKind::HoldRetrieval,
                        command_scope: reply_block.inner.command_scope,
                        valid_until: reply_block.inner.valid_until,
                        used: false,
                    },
                );
                state
                    .pending_sync_replies
                    .push_back(QueuedAccountabilityReply {
                        kind: AccountabilityWitnessKind::HoldRetrieval,
                        scope,
                        token: reply_token,
                        available_at_ms: now_ms.saturating_add(self.config.reply_jitter_ms),
                        deadline_ms: reply_block.inner.valid_until,
                    });
                if let Some(local_index) = state.local_index.get_mut(&(scope, content_key.clone()))
                {
                    local_index.selector_count = stored.selectors.len();
                    local_index.last_observed_ms = now_ms;
                }
            },
            HoldState::validate,
        )
        .await;

        let witness = AccountabilityWitness {
            kind: AccountabilityWitnessKind::HoldRetrieval,
            scope: request.scope,
            family: ServiceFamily::Hold,
            profile: Some(request.profile),
            providers,
            command_scope: reply_block.inner.command_scope,
            selector: Some(request.selector),
            observed_at_ms: now_ms,
            success,
        };

        Ok(HoldRetrievalOutcome {
            status,
            held_object,
            witness,
            reply_block,
            next_capability,
            redeposit_on_miss: !success || release_on_verify,
        })
    }

    pub async fn queue_sync_retrieval(
        &self,
        request: HoldRetrievalRequest,
        queued_at_ms: u64,
        deadline_ms: u64,
    ) {
        let _ = with_state_mut_validated(
            &self.state,
            |state| {
                state
                    .pending_sync_retrievals
                    .push_back(QueuedSyncRetrieval {
                        request,
                        queued_at_ms,
                        deadline_ms,
                    });
            },
            HoldState::validate,
        )
        .await;
    }

    pub async fn drain_sync_window(&self, now_ms: u64) -> HoldSyncBatch {
        let mut state = self.state.write().await;
        let mut retrievals = Vec::new();
        while retrievals.len() < self.config.sync_batch_size {
            let Some(front) = state.pending_sync_retrievals.front() else {
                break;
            };
            if front.deadline_ms < now_ms {
                let _ = state.pending_sync_retrievals.pop_front();
                continue;
            }
            retrievals.push(
                state
                    .pending_sync_retrievals
                    .pop_front()
                    .expect("front existed"),
            );
        }

        let mut replies = Vec::new();
        while replies.len() < self.config.sync_batch_size {
            let Some(front) = state.pending_sync_replies.front() else {
                break;
            };
            if front.available_at_ms > now_ms {
                break;
            }
            replies.push(
                state
                    .pending_sync_replies
                    .pop_front()
                    .expect("front existed"),
            );
        }

        HoldSyncBatch {
            retrievals,
            replies,
        }
    }

    pub async fn rotate_capabilities(
        &self,
        scope: ContextId,
        maintenance_epoch: MaintenanceEpoch,
        now_ms: u64,
    ) -> CapabilityRotationOutcome {
        let mut rotated = Vec::new();
        let _ = with_state_mut_validated(
            &self.state,
            |state| {
                let keys = state
                    .objects
                    .keys()
                    .filter(|(candidate_scope, _)| *candidate_scope == scope)
                    .cloned()
                    .collect::<Vec<_>>();
                for key in keys {
                    let Some(stored) = state.objects.get_mut(&key) else {
                        continue;
                    };
                    let should_rotate = stored.selectors.iter().any(|selector| {
                        selector.capability.epoch != maintenance_epoch.identity_epoch.value()
                            || selector.capability.valid_until.saturating_sub(now_ms)
                                <= self.config.capability_rotation_window_ms
                    });
                    if should_rotate {
                        let rotated_capability = self.issue_capability(
                            stored.held_object.scope,
                            &stored.held_object,
                            maintenance_epoch.identity_epoch,
                            now_ms,
                        );
                        rotated.push(rotated_capability.selector);
                        state
                            .selector_index
                            .insert(rotated_capability.selector, key.clone());
                        stored.selectors.push(SelectorRecord {
                            capability: rotated_capability,
                            used: false,
                        });
                    }
                    if let Some(local_index) = state.local_index.get_mut(&key) {
                        local_index.selector_count = stored.selectors.len();
                        local_index.last_observed_ms = now_ms;
                    }
                }
            },
            HoldState::validate,
        )
        .await;
        CapabilityRotationOutcome {
            rotated_selectors: rotated,
        }
    }

    pub async fn garbage_collect(
        &self,
        scope: Option<ContextId>,
        maintenance_epoch: MaintenanceEpoch,
        now_ms: u64,
    ) -> HoldGcOutcome {
        let mut invalidations = Vec::new();
        let removed = with_state_mut_validated(
            &self.state,
            |state| {
                let mut removable = state
                    .objects
                    .iter()
                    .filter(|((candidate_scope, _), _)| {
                        scope.map_or(true, |value| *candidate_scope == value)
                    })
                    .filter_map(|(key, stored)| {
                        let expired_caps = stored
                            .selectors
                            .iter()
                            .all(|selector| selector.capability.valid_until <= now_ms);
                        let stale_epoch =
                            stored.retention.deposit_epoch != maintenance_epoch.identity_epoch;
                        let expired_retention = stored.retention.is_expired(now_ms);
                        let remove = stored.retrieve_once_consumed
                            || expired_caps
                            || stale_epoch
                            || expired_retention;
                        remove.then_some((key.clone(), stored.invalidation.clone()))
                    })
                    .collect::<Vec<_>>();

                if state.total_custody_bytes > self.config.storage_limit_bytes {
                    let mut extra = state
                        .objects
                        .iter()
                        .filter(|((candidate_scope, _), _)| {
                            scope.map_or(true, |value| *candidate_scope == value)
                        })
                        .map(|(key, stored)| {
                            (
                                key.clone(),
                                stored.retention.deposit_epoch,
                                stored.retention.deposited_at_ms,
                                stored.invalidation.clone(),
                            )
                        })
                        .collect::<Vec<_>>();
                    extra.sort_by_key(|(_, epoch, deposited_at_ms, _)| (*epoch, *deposited_at_ms));
                    let mut bytes_over = state
                        .total_custody_bytes
                        .saturating_sub(self.config.storage_limit_bytes);
                    for (key, _, _, invalidation) in extra {
                        if bytes_over == 0 {
                            break;
                        }
                        if removable.iter().any(|(existing, _)| *existing == key) {
                            continue;
                        }
                        if let Some(stored) = state.objects.get(&key) {
                            bytes_over =
                                bytes_over.saturating_sub(stored.held_object.ciphertext.len());
                        }
                        removable.push((key, invalidation));
                    }
                }

                let mut seen = HashSet::new();
                let unique = removable
                    .into_iter()
                    .filter(|(key, _)| seen.insert(key.clone()))
                    .collect::<Vec<_>>();
                let count = unique.len();
                for (key, invalidation) in unique {
                    if let Some(stored) = state.objects.remove(&key) {
                        state.total_custody_bytes = state
                            .total_custody_bytes
                            .saturating_sub(stored.held_object.ciphertext.len());
                        for selector in stored.selectors {
                            state.selector_index.remove(&selector.capability.selector);
                        }
                        for authority in stored.selected_holders {
                            if let Some(load) = state.provider_loads.get_mut(&authority) {
                                *load = load.saturating_sub(1);
                            }
                        }
                    }
                    state.local_index.remove(&key);
                    invalidations.push(invalidation);
                }
                count
            },
            HoldState::validate,
        )
        .await;
        HoldGcOutcome {
            removed_objects: removed,
            invalidations,
        }
    }

    pub async fn issue_move_receipt_reply_block(
        &self,
        scope: ContextId,
        command_scope: [u8; 32],
        now_ms: u64,
    ) -> MoveReceiptReplyBlock {
        let inner = self.issue_reply_block(
            scope,
            AccountabilityWitnessKind::MoveReceipt,
            command_scope,
            now_ms,
        );
        let token = inner.token;
        let valid_until = inner.valid_until;
        let _ = with_state_mut_validated(
            &self.state,
            |state| {
                state.reply_blocks.insert(
                    token,
                    ReplyBlockRecord {
                        scope,
                        kind: AccountabilityWitnessKind::MoveReceipt,
                        command_scope,
                        valid_until,
                        used: false,
                    },
                );
                state
                    .pending_sync_replies
                    .push_back(QueuedAccountabilityReply {
                        kind: AccountabilityWitnessKind::MoveReceipt,
                        scope,
                        token,
                        available_at_ms: now_ms.saturating_add(self.config.reply_jitter_ms),
                        deadline_ms: valid_until,
                    });
            },
            HoldState::validate,
        )
        .await;
        MoveReceiptReplyBlock { inner }
    }

    pub async fn verify_move_receipt(
        &self,
        role: VerifierRole,
        reply_block: &MoveReceiptReplyBlock,
        witness: &AccountabilityWitness,
        now_ms: u64,
    ) -> Result<VerifiedServiceWitness, HoldManagerError> {
        self.verify_witness(
            role,
            AccountabilityWitnessKind::MoveReceipt,
            ServiceFamily::Move,
            &reply_block.inner,
            witness,
            now_ms,
        )
        .await
    }

    pub async fn verify_hold_witness(
        &self,
        role: VerifierRole,
        reply_block: &AccountabilityReplyBlock,
        witness: &AccountabilityWitness,
        now_ms: u64,
    ) -> Result<VerifiedServiceWitness, HoldManagerError> {
        self.verify_witness(
            role,
            witness.kind,
            ServiceFamily::Hold,
            reply_block,
            witness,
            now_ms,
        )
        .await
    }

    async fn verify_witness(
        &self,
        role: VerifierRole,
        kind: AccountabilityWitnessKind,
        family: ServiceFamily,
        reply_block: &AccountabilityReplyBlock,
        witness: &AccountabilityWitness,
        now_ms: u64,
    ) -> Result<VerifiedServiceWitness, HoldManagerError> {
        reply_block.validate_at(now_ms)?;
        let mut state = self.state.write().await;
        let record = state
            .reply_blocks
            .get_mut(&reply_block.token)
            .ok_or(HoldManagerError::SelectorMiss)?;
        if record.used {
            return Err(HoldManagerError::ReplyBlock(ReplyBlockError::Expired {
                valid_until: record.valid_until,
                now_ms,
            }));
        }
        let role_matches = matches!(
            (kind, role),
            (
                AccountabilityWitnessKind::MoveReceipt,
                VerifierRole::AdjacentMoveHop
            ) | (
                AccountabilityWitnessKind::HoldDeposit,
                VerifierRole::HoldDepositor
            ) | (
                AccountabilityWitnessKind::HoldRetrieval,
                VerifierRole::HoldRetriever
            ) | (
                AccountabilityWitnessKind::HoldAudit,
                VerifierRole::HoldAuditor
            )
        );
        if !role_matches
            || record.command_scope != witness.command_scope
            || witness.kind != kind
            || witness.family != family
        {
            return Err(HoldManagerError::SelectorMiss);
        }
        record.used = true;
        let outstanding_hold_delta = match kind {
            AccountabilityWitnessKind::HoldDeposit if witness.success => 1,
            AccountabilityWitnessKind::HoldRetrieval if witness.success => -1,
            _ => 0,
        };
        Ok(VerifiedServiceWitness {
            family,
            providers: witness.providers.clone(),
            observed_at_ms: witness.observed_at_ms,
            success: witness.success,
            outstanding_hold_delta,
            _sealed: (),
        })
    }

    pub async fn apply_verified_witness(
        &self,
        verified: VerifiedServiceWitness,
    ) -> Result<(), HoldManagerError> {
        for authority in &verified.providers {
            let _ = with_state_mut_validated(
                &self.state,
                |state| {
                    let account = state.provider_budget.entry(*authority).or_default();
                    if verified.success {
                        account.success_count = account.success_count.saturating_add(1);
                    } else {
                        account.failure_count = account.failure_count.saturating_add(1);
                        account.admission_penalty = account.admission_penalty.saturating_add(1);
                    }
                    if verified.outstanding_hold_delta > 0 {
                        account.outstanding_holds = account
                            .outstanding_holds
                            .saturating_add(verified.outstanding_hold_delta as u32);
                    } else if verified.outstanding_hold_delta < 0 {
                        account.outstanding_holds = account
                            .outstanding_holds
                            .saturating_sub(verified.outstanding_hold_delta.unsigned_abs());
                    }
                    account.last_verified_ms = Some(verified.observed_at_ms);
                },
                HoldState::validate,
            )
            .await;
            if verified.success {
                self.registry
                    .record_provider_success(verified.family, *authority, verified.observed_at_ms)
                    .await;
            } else {
                self.registry
                    .record_provider_failure(verified.family, *authority, verified.observed_at_ms)
                    .await;
            }
        }
        Ok(())
    }

    fn issue_capability(
        &self,
        scope: ContextId,
        held_object: &aura_core::HeldObject,
        epoch: Epoch,
        now_ms: u64,
    ) -> RetrievalCapability {
        let selector = hash(&selector_material(
            self.authority_id,
            scope,
            &content_key_for(held_object),
            epoch,
            now_ms,
        ));
        RetrievalCapability {
            scope,
            selector,
            epoch: epoch.value(),
            valid_until: now_ms.saturating_add(self.config.capability_ttl_ms),
        }
    }

    fn issue_reply_block(
        &self,
        scope: ContextId,
        _kind: AccountabilityWitnessKind,
        command_scope: [u8; 32],
        now_ms: u64,
    ) -> AccountabilityReplyBlock {
        AccountabilityReplyBlock {
            scope,
            token: hash(&reply_block_material(
                self.authority_id,
                scope,
                command_scope,
                now_ms,
            )),
            command_scope,
            valid_until: now_ms.saturating_add(self.config.reply_block_ttl_ms),
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl RuntimeService for HoldManager {
    fn name(&self) -> &'static str {
        "hold_manager"
    }

    fn dependencies(&self) -> &[&'static str] {
        &["rendezvous_manager", "social_manager"]
    }

    async fn start(&self, _context: &RuntimeServiceContext) -> Result<(), ServiceError> {
        let _ = with_state_mut_validated(
            &self.state,
            |state| state.lifecycle = ServiceHealth::Healthy,
            HoldState::validate,
        )
        .await;
        Ok(())
    }

    async fn stop(&self) -> Result<(), ServiceError> {
        let _ = with_state_mut_validated(
            &self.state,
            |state| state.lifecycle = ServiceHealth::Stopped,
            HoldState::validate,
        )
        .await;
        Ok(())
    }

    async fn health(&self) -> ServiceHealth {
        self.state.read().await.lifecycle.clone()
    }
}

fn selector_material(
    authority_id: AuthorityId,
    scope: ContextId,
    content_key: &str,
    epoch: Epoch,
    now_ms: u64,
) -> Vec<u8> {
    let mut material = Vec::new();
    material.extend_from_slice(&authority_id.to_bytes());
    material.extend_from_slice(&scope.to_bytes());
    material.extend_from_slice(content_key.as_bytes());
    material.extend_from_slice(&epoch.value().to_be_bytes());
    material.extend_from_slice(&now_ms.to_be_bytes());
    material
}

fn reply_block_material(
    authority_id: AuthorityId,
    scope: ContextId,
    command_scope: [u8; 32],
    now_ms: u64,
) -> Vec<u8> {
    let mut material = Vec::new();
    material.extend_from_slice(&authority_id.to_bytes());
    material.extend_from_slice(&scope.to_bytes());
    material.extend_from_slice(&command_scope);
    material.extend_from_slice(&now_ms.to_be_bytes());
    material
}

fn content_key_for(held_object: &aura_core::HeldObject) -> String {
    hex_selector(*held_object.content_id.hash().as_bytes())
}

fn hex_selector(selector: [u8; 32]) -> String {
    selector.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn command_scope(scope: &ContextId, content_key: &str, now_ms: u64, prefix: &[u8]) -> [u8; 32] {
    let mut material = Vec::new();
    material.extend_from_slice(prefix);
    material.extend_from_slice(&scope.to_bytes());
    material.extend_from_slice(content_key.as_bytes());
    material.extend_from_slice(&now_ms.to_be_bytes());
    hash(&material)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{ContentId, DeviceId};

    fn authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn context(seed: u8) -> ContextId {
        ContextId::new_from_entropy([seed; 32])
    }

    fn held(scope: ContextId, seed: &[u8]) -> aura_core::HeldObject {
        aura_core::HeldObject {
            content_id: ContentId::from_bytes(seed),
            scope,
            retention_until: None,
            ciphertext: seed.to_vec(),
        }
    }

    fn candidate(authority_id: AuthorityId) -> ProviderCandidate {
        ProviderCandidate {
            authority_id,
            device_id: Some(DeviceId::new_from_entropy([9; 32])),
            family: ServiceFamily::Hold,
            evidence: vec![ProviderEvidence::Neighborhood],
            link_endpoints: Vec::new(),
            reachable: true,
        }
    }

    #[tokio::test]
    async fn deposit_and_retrieve_deferred_delivery_use_shared_hold_substrate() {
        let registry = Arc::new(ServiceRegistry::new());
        let manager = HoldManager::new(authority(1), HoldManagerConfig::for_testing(), registry);
        let scope = context(4);
        let epoch = MaintenanceEpoch::new(Epoch::new(3), Epoch::new(3));
        let candidates = vec![candidate(authority(2)), candidate(authority(3))];

        let deposit = manager
            .deposit(
                HoldDepositRequest {
                    profile: ServiceProfile::DeferredDeliveryHold,
                    held_object: held(scope, b"hello"),
                    requested_retention_ms: 500,
                    deposit_epoch: epoch.identity_epoch,
                    handoff: None,
                },
                &candidates,
                100,
                epoch,
            )
            .await
            .expect("deposit");

        let before_verify = manager.provider_budget(authority(2)).await;
        assert!(before_verify.is_none());

        let verified = manager
            .verify_hold_witness(
                VerifierRole::HoldDepositor,
                &deposit.reply_block.inner,
                &deposit.witness,
                120,
            )
            .await
            .expect("verified");
        manager
            .apply_verified_witness(verified)
            .await
            .expect("apply");

        let budget = manager
            .provider_budget(authority(2))
            .await
            .expect("budget after verify");
        assert_eq!(budget.outstanding_holds, 1);
        assert_eq!(budget.success_count, 1);

        let retrieval = manager
            .retrieve(
                HoldRetrievalRequest {
                    profile: ServiceProfile::DeferredDeliveryHold,
                    scope,
                    selector: deposit.retrieval_capability.selector,
                },
                150,
                epoch,
            )
            .await
            .expect("retrieve");

        assert_eq!(retrieval.status, HoldRetrievalStatus::Success);
        assert_eq!(
            retrieval.held_object.expect("object"),
            held(scope, b"hello")
        );
        assert!(retrieval.redeposit_on_miss);
    }

    #[tokio::test]
    async fn cache_replica_retrieval_rotates_capabilities() {
        let registry = Arc::new(ServiceRegistry::new());
        let manager = HoldManager::new(authority(1), HoldManagerConfig::for_testing(), registry);
        let scope = context(7);
        let epoch = MaintenanceEpoch::new(Epoch::new(4), Epoch::new(4));
        let deposit = manager
            .deposit(
                HoldDepositRequest {
                    profile: ServiceProfile::CacheReplicaHold,
                    held_object: held(scope, b"cache"),
                    requested_retention_ms: 500,
                    deposit_epoch: epoch.identity_epoch,
                    handoff: None,
                },
                &[candidate(authority(2)), candidate(authority(3))],
                100,
                epoch,
            )
            .await
            .expect("deposit");

        let retrieval = manager
            .retrieve(
                HoldRetrievalRequest {
                    profile: ServiceProfile::CacheReplicaHold,
                    scope,
                    selector: deposit.retrieval_capability.selector,
                },
                760,
                epoch,
            )
            .await
            .expect("retrieve");

        assert_eq!(retrieval.status, HoldRetrievalStatus::Success);
        assert!(retrieval.next_capability.is_some());
    }

    #[tokio::test]
    async fn holder_selection_rotates_after_residency_window() {
        let registry = Arc::new(ServiceRegistry::new());
        let manager = HoldManager::new(authority(1), HoldManagerConfig::for_testing(), registry);
        let scope = context(8);
        let epoch = MaintenanceEpoch::new(Epoch::new(5), Epoch::new(5));
        let candidates = vec![
            candidate(authority(2)),
            candidate(authority(3)),
            candidate(authority(4)),
        ];

        let first = manager
            .select_holders(scope, &candidates, 100, epoch)
            .await
            .expect("first");
        let second = manager
            .select_holders(scope, &candidates, 110, epoch)
            .await
            .expect("second");

        assert!(first.rotated);
        assert!(second.rotated);
        assert_ne!(first.selected_authorities, second.selected_authorities);
    }

    #[tokio::test]
    async fn sync_window_blends_retrievals_and_replies() {
        let registry = Arc::new(ServiceRegistry::new());
        let manager = HoldManager::new(authority(1), HoldManagerConfig::for_testing(), registry);
        let scope = context(9);
        let epoch = MaintenanceEpoch::new(Epoch::new(6), Epoch::new(6));
        let deposit = manager
            .deposit(
                HoldDepositRequest {
                    profile: ServiceProfile::DeferredDeliveryHold,
                    held_object: held(scope, b"sync"),
                    requested_retention_ms: 500,
                    deposit_epoch: epoch.identity_epoch,
                    handoff: None,
                },
                &[candidate(authority(2))],
                100,
                epoch,
            )
            .await
            .expect("deposit");
        manager
            .queue_sync_retrieval(
                HoldRetrievalRequest {
                    profile: ServiceProfile::DeferredDeliveryHold,
                    scope,
                    selector: deposit.retrieval_capability.selector,
                },
                120,
                400,
            )
            .await;

        let batch = manager.drain_sync_window(130).await;
        assert_eq!(batch.retrievals.len(), 1);
        assert_eq!(batch.replies.len(), 1);
    }

    #[tokio::test]
    async fn gc_uses_epoch_and_storage_pressure_without_social_tiering() {
        let registry = Arc::new(ServiceRegistry::new());
        let mut config = HoldManagerConfig::for_testing();
        config.storage_limit_bytes = 4;
        let manager = HoldManager::new(authority(1), config, registry);
        let scope = context(10);
        let epoch = MaintenanceEpoch::new(Epoch::new(7), Epoch::new(7));
        manager
            .deposit(
                HoldDepositRequest {
                    profile: ServiceProfile::CacheReplicaHold,
                    held_object: held(scope, b"aaaa"),
                    requested_retention_ms: 500,
                    deposit_epoch: epoch.identity_epoch,
                    handoff: None,
                },
                &[candidate(authority(2))],
                100,
                epoch,
            )
            .await
            .expect("first deposit");
        manager
            .deposit(
                HoldDepositRequest {
                    profile: ServiceProfile::CacheReplicaHold,
                    held_object: held(scope, b"bbbb"),
                    requested_retention_ms: 500,
                    deposit_epoch: epoch.identity_epoch,
                    handoff: None,
                },
                &[candidate(authority(2))],
                101,
                epoch,
            )
            .await
            .expect("second deposit");

        let gc = manager.garbage_collect(Some(scope), epoch, 150).await;
        assert!(gc.removed_objects >= 1);
        assert!(!gc.invalidations.is_empty());
    }

    #[tokio::test]
    async fn verified_witness_is_required_before_provider_health_updates() {
        let registry = Arc::new(ServiceRegistry::new());
        let manager = HoldManager::new(
            authority(1),
            HoldManagerConfig::for_testing(),
            registry.clone(),
        );
        let scope = context(11);
        let move_reply = manager
            .issue_move_receipt_reply_block(scope, [7; 32], 100)
            .await;
        let witness = AccountabilityWitness {
            kind: AccountabilityWitnessKind::MoveReceipt,
            scope,
            family: ServiceFamily::Move,
            profile: None,
            providers: vec![authority(2)],
            command_scope: [7; 32],
            selector: None,
            observed_at_ms: 120,
            success: true,
        };

        assert!(registry
            .provider_health(ServiceFamily::Move, authority(2))
            .await
            .is_none());

        let verified = manager
            .verify_move_receipt(VerifierRole::AdjacentMoveHop, &move_reply, &witness, 130)
            .await
            .expect("verified");
        manager
            .apply_verified_witness(verified)
            .await
            .expect("apply");

        let health = registry
            .provider_health(ServiceFamily::Move, authority(2))
            .await
            .expect("provider health");
        assert_eq!(health.success_count, 1);
    }
}
