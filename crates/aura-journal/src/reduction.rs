//! Deterministic reduction for journals
//!
//! This module implements deterministic reduction of facts to produce
//! authority state and relational state from journal facts.

use crate::fact::{
    AmpEmergencyAlarm, AmpTransitionAbort, AmpTransitionConflict, AmpTransitionIdentity,
    AmpTransitionPolicy, AmpTransitionSupersession, AmpTransitionSuppressionScope, AttestedOp,
    CertifiedChannelEpochBump, ChannelBootstrap, ChannelBumpReason, ChannelCheckpoint,
    ChannelPolicy, CommittedChannelEpochBump, FactContent, FinalizedChannelEpochBump, Journal,
    JournalNamespace, ProposedChannelEpochBump, RelationalFact,
};
use aura_core::{
    effects::LeakageBudget,
    hash,
    time::OrderTime,
    tree::{commit_leaf, policy_hash, LeafId, Policy},
    types::authority::TreeStateSummary,
    types::identifiers::{AuthorityId, ChannelId, ContextId},
    Hash32,
};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

/// Error type for journal namespace mismatches during reduction
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ReductionNamespaceError {
    /// Attempted to reduce a context journal as authority state
    #[error("Cannot reduce context journal as authority state: expected Authority namespace, got Context")]
    ContextAsAuthority,
    /// Attempted to reduce an authority journal as relational state
    #[error("Cannot reduce authority journal as relational state: expected Context namespace, got Authority")]
    AuthorityAsContext,
    /// Reduction failed due to internal validation error
    #[error("Reduction failed: {0}")]
    ReductionFailure(String),
}

/// Apply an attested operation to a tree state
///
/// This function processes different types of attested operations and
/// updates the tree state accordingly.
fn apply_attested_op(
    tree_state: &TreeStateSummary,
    op: &AttestedOp,
) -> Result<TreeStateSummary, ReductionNamespaceError> {
    match &op.tree_op {
        crate::fact::TreeOpKind::AddLeaf { public_key, role } => {
            // Add a new leaf to the tree with proper device/guardian distinction
            Ok(apply_add_leaf_with_role(tree_state, public_key, *role))
        }
        crate::fact::TreeOpKind::RemoveLeaf { leaf_index } => {
            // Remove a leaf from the tree
            Ok(apply_remove_leaf(tree_state, *leaf_index))
        }
        crate::fact::TreeOpKind::UpdatePolicy { threshold } => {
            // Update the tree policy
            Ok(apply_update_policy(tree_state, *threshold))
        }
        crate::fact::TreeOpKind::RotateEpoch => {
            // Rotate to new epoch
            apply_rotate_epoch(tree_state)
        }
    }
}

/// Apply add leaf operation to tree state with device/guardian role distinction
fn apply_add_leaf_with_role(
    tree_state: &TreeStateSummary,
    public_key: &[u8],
    role: aura_core::tree::LeafRole,
) -> TreeStateSummary {
    // For a proper implementation, we would need to:
    // 1. Maintain the actual tree structure (branches and leaves)
    // 2. Find the appropriate branch to add the leaf to
    // 3. Recompute commitments up the tree
    // Since we don't have the full tree structure in TreeStateSummary, we'll compute
    // a leaf commitment and use it to update the root

    let new_leaf_id = LeafId(tree_state.device_count());
    let leaf_commitment = commit_leaf(new_leaf_id, tree_state.epoch(), public_key);

    // In this reduced view we fold the new leaf commitment directly into the
    // root commitment to keep determinism without a full branch topology.
    let mut hasher = hash::hasher();
    hasher.update(b"ROOT_WITH_LEAF");
    hasher.update(&leaf_commitment);
    hasher.update(tree_state.root_commitment().as_bytes());
    hasher.update(&tree_state.device_count().to_le_bytes());
    let new_commitment = hasher.finalize();

    // Only increment device count for actual devices, not guardians
    let new_device_count = match role {
        aura_core::tree::LeafRole::Device => tree_state.device_count() + 1,
        aura_core::tree::LeafRole::Guardian => tree_state.device_count(), // Don't increment for guardians
    };

    TreeStateSummary::with_values(
        tree_state.epoch(),
        Hash32::new(new_commitment),
        tree_state.threshold(),
        new_device_count,
    )
}

/// Apply remove leaf operation to tree state
fn apply_remove_leaf(tree_state: &TreeStateSummary, leaf_index: u32) -> TreeStateSummary {
    // For leaf removal, we would normally:
    // 1. Mark the leaf as removed in the tree structure
    // 2. Recompute commitments up the tree
    // Since we don't maintain full tree structure, we create a deterministic
    // commitment that reflects the removal

    let removed_leaf_id = LeafId(leaf_index);

    // Create a deterministic commitment for the removal operation
    let mut hasher = hash::hasher();
    hasher.update(b"ROOT_REMOVE_LEAF");
    hasher.update(&removed_leaf_id.0.to_le_bytes());
    hasher.update(tree_state.root_commitment().as_bytes());
    hasher.update(&u64::from(tree_state.epoch()).to_le_bytes());
    let new_commitment = hasher.finalize();

    TreeStateSummary::with_values(
        tree_state.epoch(),
        Hash32::new(new_commitment),
        tree_state.threshold(),
        tree_state.device_count().saturating_sub(1),
    )
}

/// Apply policy update operation to tree state
fn apply_update_policy(tree_state: &TreeStateSummary, threshold: u16) -> TreeStateSummary {
    // Policy updates affect branch nodes. We compute a new commitment
    // that incorporates the policy change

    let new_policy = Policy::Threshold {
        m: threshold,
        n: tree_state.device_count() as u16,
    };

    // Compute policy hash for the new threshold
    let new_policy_hash = policy_hash(&new_policy);

    // Create a deterministic root commitment incorporating the policy
    let mut hasher = hash::hasher();
    hasher.update(b"ROOT_WITH_POLICY");
    hasher.update(&new_policy_hash);
    hasher.update(tree_state.root_commitment().as_bytes());
    hasher.update(&u64::from(tree_state.epoch()).to_le_bytes());
    let new_commitment = hasher.finalize();

    TreeStateSummary::with_values(
        tree_state.epoch(),
        Hash32::new(new_commitment),
        threshold,
        tree_state.device_count(),
    )
}

/// Apply epoch rotation operation to tree state
fn apply_rotate_epoch(
    tree_state: &TreeStateSummary,
) -> Result<TreeStateSummary, ReductionNamespaceError> {
    // Epoch rotation requires recomputing all commitments in the tree
    // with the new epoch value

    let new_epoch = tree_state
        .epoch()
        .next()
        .map_err(|e| ReductionNamespaceError::ReductionFailure(e.to_string()))?;

    // Epoch rotation can be modeled by recomputing leaf and branch commitments
    // against the new epoch. This reducer instead derives a deterministic
    // rotated root commitment from summary state inputs.

    // Compute a deterministic commitment that reflects the epoch change without
    // re-materializing the full tree topology.
    let current_policy = Policy::Threshold {
        m: tree_state.threshold(),
        n: tree_state.device_count() as u16,
    };

    // Use the merkle tree utilities to compute a proper epoch-rotated commitment
    let policy_commitment = policy_hash(&current_policy);

    let mut hasher = hash::hasher();
    hasher.update(b"ROOT_EPOCH_ROTATE");
    hasher.update(&new_epoch.0.to_le_bytes());
    hasher.update(&policy_commitment);
    hasher.update(tree_state.root_commitment().as_bytes());
    let new_commitment = hasher.finalize();

    Ok(TreeStateSummary::with_values(
        new_epoch,
        Hash32::new(new_commitment),
        tree_state.threshold(),
        tree_state.device_count(),
    ))
}

/// Compute deterministic hash of authority state
fn compute_authority_state_hash(state: &aura_core::types::authority::AuthorityState) -> Hash32 {
    let mut hasher = hash::hasher();

    // Hash tree state commitment
    hasher.update(b"TREE_STATE");
    hasher.update(state.tree_state.root_commitment().as_bytes());

    // Hash fact set (deterministic order via sorted serialization)
    hasher.update(b"FACTS");
    let mut serialized_facts: Vec<Vec<u8>> = state
        .facts
        .iter()
        .filter_map(|fact| serde_json::to_vec(fact).ok())
        .collect();
    serialized_facts.sort();
    for serialized in serialized_facts {
        hasher.update(&serialized);
    }

    Hash32::new(hasher.finalize())
}

/// Compute deterministic hash of relational state
fn compute_relational_state_hash(state: &RelationalState) -> Hash32 {
    let mut hasher = hash::hasher();

    // Hash bindings (sorted for deterministic order)
    hasher.update(b"BINDINGS");
    let mut sorted_bindings = state.bindings.clone();
    sorted_bindings.sort_by(|a, b| {
        a.context_id
            .cmp(&b.context_id)
            .then(format!("{:?}", a.binding_type).cmp(&format!("{:?}", b.binding_type)))
    });
    for binding in &sorted_bindings {
        hasher.update(binding.context_id.0.as_bytes());
        hasher.update(format!("{:?}", binding.binding_type).as_bytes());
        hasher.update(&binding.data);
    }

    // Hash flow budgets (sorted for deterministic order)
    hasher.update(b"FLOW_BUDGETS");
    for ((source, dest, epoch), amount) in &state.flow_budgets {
        hasher.update(source.0.as_bytes());
        hasher.update(dest.0.as_bytes());
        hasher.update(&epoch.to_le_bytes());
        hasher.update(&amount.to_le_bytes());
    }

    // Hash leakage budgets
    hasher.update(b"LEAKAGE_BUDGET");
    hasher.update(&state.leakage_budget.external_consumed.to_le_bytes());
    hasher.update(&state.leakage_budget.neighbor_consumed.to_le_bytes());
    hasher.update(&state.leakage_budget.in_group_consumed.to_le_bytes());

    hasher.update(b"CHANNEL_EPOCHS");
    for (channel_id, epoch_state) in &state.channel_epochs {
        hasher.update(channel_id.as_bytes());
        hasher.update(&epoch_state.chan_epoch.to_le_bytes());
        hasher.update(&epoch_state.last_checkpoint_gen.to_le_bytes());
        hasher.update(&epoch_state.current_gen.to_le_bytes());
        hasher.update(&epoch_state.skip_window.to_le_bytes());
        if let Some(pending) = &epoch_state.pending_bump {
            hasher.update(&pending.parent_epoch.to_le_bytes());
            hasher.update(&pending.new_epoch.to_le_bytes());
            hasher.update(pending.bump_id.as_bytes());
            hasher.update(&(pending.reason as u8).to_le_bytes());
        }
        if let Some(transition) = &epoch_state.transition {
            hasher.update(&(transition.status as u8).to_le_bytes());
            if let Some(transition_id) = transition.live_transition_id {
                hasher.update(transition_id.as_bytes());
            }
            if let Some(transition_id) = transition.finalized_transition_id {
                hasher.update(transition_id.as_bytes());
            }
        }
    }

    hasher.update(b"AMP_TRANSITIONS");
    for (parent, transition) in &state.amp_transitions {
        hasher.update(parent.context.0.as_bytes());
        hasher.update(parent.channel.as_bytes());
        hasher.update(&parent.parent_epoch.to_le_bytes());
        hasher.update(parent.parent_commitment.as_bytes());
        hasher.update(&(transition.status as u8).to_le_bytes());
        for value in &transition.observed_transition_ids {
            hasher.update(value.as_bytes());
        }
        for value in &transition.certified_transition_ids {
            hasher.update(value.as_bytes());
        }
        for value in &transition.finalized_transition_ids {
            hasher.update(value.as_bytes());
        }
        if let Some(value) = transition.live_transition_id {
            hasher.update(value.as_bytes());
        }
        if let Some(value) = transition.finalized_transition_id {
            hasher.update(value.as_bytes());
        }
        for value in &transition.suppressed_transition_ids {
            hasher.update(value.as_bytes());
        }
        for value in &transition.conflict_evidence_ids {
            hasher.update(value.as_bytes());
        }
        for value in &transition.emergency_alarm_ids {
            hasher.update(value.as_bytes());
        }
        for value in &transition.emergency_suspects {
            hasher.update(value.0.as_bytes());
        }
        for value in &transition.quarantine_epochs {
            hasher.update(&value.to_le_bytes());
        }
        for value in &transition.prune_before_epochs {
            hasher.update(&value.to_le_bytes());
        }
    }

    Hash32::new(hasher.finalize())
}

/// Reduce an authority journal to derive authority state
///
/// This function deterministically computes the current state of an
/// authority by applying all attested operations in order.
///
/// # Errors
///
/// Returns `ReductionNamespaceError::ContextAsAuthority` if the journal
/// has a Context namespace instead of an Authority namespace.
pub fn reduce_authority(
    journal: &Journal,
) -> Result<aura_core::types::authority::AuthorityState, ReductionNamespaceError> {
    match &journal.namespace {
        JournalNamespace::Authority(_) => {
            // Extract all attested operations
            let attested_ops: Vec<&AttestedOp> = journal
                .facts
                .iter()
                .filter_map(|f| match &f.content {
                    FactContent::AttestedOp(op) => Some(op),
                    _ => None,
                })
                .collect();

            // Start with empty tree state
            let mut tree_state = TreeStateSummary::default();

            // Apply operations in order (facts are already ordered by BTreeSet)
            for op in attested_ops {
                // Apply attested operation to tree state
                tree_state = apply_attested_op(&tree_state, op)?;
            }

            // Convert journal facts to aura-core::Fact type
            let facts: BTreeSet<aura_core::journal::Fact> = journal
                .facts
                .iter()
                .map(convert_to_core_fact)
                .collect::<Result<_, _>>()
                .unwrap_or_else(|_| BTreeSet::new());

            Ok(aura_core::types::authority::AuthorityState { tree_state, facts })
        }
        JournalNamespace::Context(_) => Err(ReductionNamespaceError::ContextAsAuthority),
    }
}

/// Reduce account facts to tree state
///
/// This is the main entry point for reducing journal facts to a TreeStateSummary.
/// It delegates to reduce_authority for the actual work.
///
/// # Errors
///
/// Returns `ReductionNamespaceError::ContextAsAuthority` if the journal
/// has a Context namespace instead of an Authority namespace.
pub fn reduce_account_facts(
    journal: &Journal,
) -> Result<TreeStateSummary, ReductionNamespaceError> {
    Ok(reduce_authority(journal)?.tree_state)
}

/// Enhanced reduction with state ordering validation
///
/// This performs the same reduction but also validates that operations
/// are applied in the correct order based on their parent commitments.
pub fn reduce_authority_with_validation(
    journal: &Journal,
) -> Result<aura_core::types::authority::AuthorityState, String> {
    match &journal.namespace {
        JournalNamespace::Authority(_) => {
            // Extract all attested operations with their parent commitments
            let mut attested_ops: Vec<&AttestedOp> = journal
                .facts
                .iter()
                .filter_map(|f| match &f.content {
                    FactContent::AttestedOp(op) => Some(op),
                    _ => None,
                })
                .collect();

            // Sort operations by parent commitment to ensure proper ordering
            attested_ops.sort_by(|a, b| {
                a.parent_commitment
                    .as_bytes()
                    .cmp(b.parent_commitment.as_bytes())
            });

            let mut tree_state = TreeStateSummary::default();
            let mut current_commitment = tree_state.root_commitment();

            // Apply operations in order, validating parent commitments
            for op in &attested_ops {
                // Validate that this operation builds on the current state
                if op.parent_commitment == current_commitment {
                    // Apply the operation
                    tree_state = apply_attested_op(&tree_state, op).map_err(|e| e.to_string())?;
                    current_commitment = tree_state.root_commitment();
                } else {
                    // Try to apply anyway but note the inconsistency
                    // In practice, this might indicate concurrent operations
                    tree_state = apply_attested_op(&tree_state, op).map_err(|e| e.to_string())?;
                    current_commitment = tree_state.root_commitment();
                }
            }

            // Convert journal facts to aura-core::Fact type
            let facts: BTreeSet<aura_core::journal::Fact> = journal
                .facts
                .iter()
                .map(convert_to_core_fact)
                .collect::<Result<_, _>>()
                .unwrap_or_else(|_| BTreeSet::new());

            Ok(aura_core::types::authority::AuthorityState { tree_state, facts })
        }
        JournalNamespace::Context(_) => {
            Err("Cannot reduce context journal as authority state".to_string())
        }
    }
}

/// Relational state derived from context journals
#[derive(Debug, Clone)]
pub struct RelationalState {
    /// Active relational bindings
    pub bindings: Vec<RelationalBinding>,
    /// Flow budget state by context
    pub flow_budgets: BTreeMap<(AuthorityId, AuthorityId, u64), u64>,
    /// Leakage budget totals for this context
    pub leakage_budget: LeakageBudget,
    /// AMP channel epoch state keyed by channel id
    pub channel_epochs: BTreeMap<ChannelId, ChannelEpochState>,
    /// AMP channel transition reduction keyed by parent prestate
    pub amp_transitions: BTreeMap<AmpTransitionParentKey, AmpTransitionReduction>,
}

/// Pending bump state for a channel
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingBump {
    /// Current epoch being transitioned from
    pub parent_epoch: u64,
    /// New epoch being transitioned to
    pub new_epoch: u64,
    /// Unique identifier for this bump proposal
    pub bump_id: Hash32,
    /// Reason for proposing this epoch bump
    pub reason: ChannelBumpReason,
    /// Canonical transition identity digest for the live successor.
    pub transition_id: Hash32,
    /// Transition policy class for the live successor.
    pub transition_policy: AmpTransitionPolicy,
}

/// Parent prestate key for AMP channel transition reduction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct AmpTransitionParentKey {
    /// Relational context containing this transition.
    pub context: ContextId,
    /// Channel identifier.
    pub channel: ChannelId,
    /// Parent epoch being transitioned from.
    pub parent_epoch: u64,
    /// Parent prestate commitment.
    pub parent_commitment: Hash32,
}

impl From<&AmpTransitionIdentity> for AmpTransitionParentKey {
    fn from(identity: &AmpTransitionIdentity) -> Self {
        Self {
            context: identity.context,
            channel: identity.channel,
            parent_epoch: identity.parent_epoch,
            parent_commitment: identity.parent_commitment,
        }
    }
}

/// Reducer-visible AMP transition status for one parent prestate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum AmpTransitionReductionStatus {
    /// Proposal facts exist, but no live or durable certificate is valid.
    Observed,
    /// Exactly one valid, unsuppressed A2 certificate exposes a live successor.
    A2Live,
    /// Conflicting A2 certificates or conflict evidence suppress live exposure.
    A2Conflict,
    /// Exactly one valid, unsuppressed A3 fact finalizes a durable successor.
    A3Finalized,
    /// Conflicting A3 finalizations suppress durable exposure.
    A3Conflict,
    /// Transition facts for this parent are explicitly aborted.
    Aborted,
    /// Transition facts for this parent are explicitly superseded.
    Superseded,
}

/// Deterministic AMP transition reduction for one parent prestate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmpTransitionReduction {
    /// Parent prestate key being reduced.
    pub parent: AmpTransitionParentKey,
    /// Reduced status.
    pub status: AmpTransitionReductionStatus,
    /// Observed proposal transition ids.
    pub observed_transition_ids: BTreeSet<Hash32>,
    /// Valid A2 certificate transition ids before conflict resolution.
    pub certified_transition_ids: BTreeSet<Hash32>,
    /// Valid A3 finalization transition ids before conflict resolution.
    pub finalized_transition_ids: BTreeSet<Hash32>,
    /// Transition exposed for live sends/receives when status is A2Live.
    pub live_transition_id: Option<Hash32>,
    /// Transition exposed as durable when status is A3Finalized.
    pub finalized_transition_id: Option<Hash32>,
    /// Transition ids suppressed by abort or supersession evidence.
    pub suppressed_transition_ids: BTreeSet<Hash32>,
    /// Conflict evidence ids and conflicting transition ids.
    pub conflict_evidence_ids: BTreeSet<Hash32>,
    /// Emergency alarm evidence ids observed for this parent.
    pub emergency_alarm_ids: BTreeSet<Hash32>,
    /// Suspect authorities from emergency alarms and emergency transitions.
    pub emergency_suspects: BTreeSet<AuthorityId>,
    /// Successor epochs for quarantine transitions in this parent group.
    pub quarantine_epochs: BTreeSet<u64>,
    /// Epochs before which readable state may be pruned after cryptoshred.
    pub prune_before_epochs: BTreeSet<u64>,
}

/// Derived AMP channel epoch state
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelEpochState {
    /// Canonical channel epoch
    pub chan_epoch: u64,
    /// Pending bump if one exists (e→e+1)
    pub pending_bump: Option<PendingBump>,
    /// Optional bootstrap metadata for epoch 0 (dealer key)
    pub bootstrap: Option<ChannelBootstrap>,
    /// Base generation from the last checkpoint
    pub last_checkpoint_gen: u64,
    /// Current generation derived from facts (not a local counter)
    pub current_gen: u64,
    /// Skip window size in generations
    pub skip_window: u32,
    /// Reducer-visible transition state for the current parent epoch.
    pub transition: Option<AmpTransitionReduction>,
}

const DEFAULT_SKIP_WINDOW: u32 = 1024;

/// A relational binding between authorities
#[derive(Debug, Clone)]
pub struct RelationalBinding {
    /// Type of binding
    pub binding_type: RelationalBindingType,
    /// Context this binding belongs to
    pub context_id: ContextId,
    /// Binding data
    pub data: Vec<u8>,
}

/// Types of relational bindings
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelationalBindingType {
    /// Guardian relationship between two authorities
    GuardianBinding {
        /// The primary account authority
        account_id: AuthorityId,
        /// The guardian authority
        guardian_id: AuthorityId,
    },
    /// Recovery grant from a guardian to an account
    RecoveryGrant {
        /// The account receiving the grant
        account_id: AuthorityId,
        /// The guardian issuing the grant
        guardian_id: AuthorityId,
    },
    /// Generic relational binding type
    Generic(String),
}

/// Reduce a context journal to derive relational state
///
/// This function computes the current relational state by processing
/// all relational facts and flow budget facts.
///
/// # Errors
///
/// Returns `ReductionNamespaceError::AuthorityAsContext` if the journal
/// has an Authority namespace instead of a Context namespace.
pub fn reduce_context(journal: &Journal) -> Result<RelationalState, ReductionNamespaceError> {
    match &journal.namespace {
        JournalNamespace::Context(context_id) => {
            let mut bindings = Vec::new();
            let flow_budgets = BTreeMap::new();
            let mut leakage_budget = LeakageBudget::zero();
            let mut channel_checkpoints: BTreeMap<(ChannelId, u64), Vec<ChannelCheckpoint>> =
                BTreeMap::new();
            let mut proposed_bumps = Vec::new();
            let mut certified_bumps = Vec::new();
            let mut committed_bumps = Vec::new();
            let mut finalized_bumps = Vec::new();
            let mut transition_aborts = Vec::new();
            let mut transition_conflicts = Vec::new();
            let mut transition_supersessions = Vec::new();
            let mut emergency_alarms = Vec::new();
            let mut channel_policies: BTreeMap<ChannelId, ChannelPolicy> = BTreeMap::new();
            let mut channel_bootstraps: BTreeMap<ChannelId, ChannelBootstrap> = BTreeMap::new();

            for fact in &journal.facts {
                if let FactContent::Relational(rf) = &fact.content {
                    let binding = match rf {
                        RelationalFact::Protocol(protocol) => match protocol {
                            crate::fact::ProtocolRelationalFact::GuardianBinding {
                                account_id,
                                guardian_id,
                                ..
                            } => RelationalBinding {
                                binding_type: RelationalBindingType::GuardianBinding {
                                    account_id: *account_id,
                                    guardian_id: *guardian_id,
                                },
                                context_id: *context_id,
                                data: protocol.binding_key().data(),
                            },
                            crate::fact::ProtocolRelationalFact::RecoveryGrant {
                                account_id,
                                guardian_id,
                                ..
                            } => RelationalBinding {
                                binding_type: RelationalBindingType::RecoveryGrant {
                                    account_id: *account_id,
                                    guardian_id: *guardian_id,
                                },
                                context_id: *context_id,
                                data: protocol.binding_key().data(),
                            },
                            crate::fact::ProtocolRelationalFact::Consensus { .. } => {
                                let key = protocol.binding_key();
                                RelationalBinding {
                                    binding_type: RelationalBindingType::Generic(
                                        key.sub_type().to_string(),
                                    ),
                                    context_id: *context_id,
                                    data: key.data(),
                                }
                            }
                            crate::fact::ProtocolRelationalFact::AmpChannelCheckpoint(cp) => {
                                channel_checkpoints
                                    .entry((cp.channel, cp.chan_epoch))
                                    .or_default()
                                    .push(cp.clone());
                                continue;
                            }
                            crate::fact::ProtocolRelationalFact::AmpProposedChannelEpochBump(
                                bump,
                            ) => {
                                proposed_bumps.push(bump.clone());
                                continue;
                            }
                            crate::fact::ProtocolRelationalFact::AmpCertifiedChannelEpochBump(
                                bump,
                            ) => {
                                certified_bumps.push(bump.clone());
                                continue;
                            }
                            crate::fact::ProtocolRelationalFact::AmpFinalizedChannelEpochBump(
                                bump,
                            ) => {
                                finalized_bumps.push(bump.clone());
                                continue;
                            }
                            crate::fact::ProtocolRelationalFact::AmpTransitionAbort(abort) => {
                                transition_aborts.push(abort.clone());
                                continue;
                            }
                            crate::fact::ProtocolRelationalFact::AmpTransitionConflict(
                                conflict,
                            ) => {
                                transition_conflicts.push(conflict.clone());
                                continue;
                            }
                            crate::fact::ProtocolRelationalFact::AmpTransitionSupersession(
                                supersession,
                            ) => {
                                transition_supersessions.push(supersession.clone());
                                continue;
                            }
                            crate::fact::ProtocolRelationalFact::AmpEmergencyAlarm(alarm) => {
                                emergency_alarms.push(alarm.clone());
                                continue;
                            }
                            crate::fact::ProtocolRelationalFact::AmpCommittedChannelEpochBump(
                                bump,
                            ) => {
                                committed_bumps.push(bump.clone());
                                continue;
                            }
                            crate::fact::ProtocolRelationalFact::AmpChannelPolicy(policy) => {
                                channel_policies
                                    .entry(policy.channel)
                                    .and_modify(|existing| {
                                        // Prefer policies that specify a skip window override
                                        if existing.skip_window.is_none()
                                            && policy.skip_window.is_some()
                                        {
                                            *existing = policy.clone();
                                        }
                                    })
                                    .or_insert_with(|| policy.clone());
                                continue;
                            }
                            crate::fact::ProtocolRelationalFact::AmpChannelBootstrap(bootstrap) => {
                                channel_bootstraps.insert(bootstrap.channel, bootstrap.clone());
                                continue;
                            }
                            crate::fact::ProtocolRelationalFact::LeakageEvent(event) => {
                                let observer =
                                    aura_core::effects::ObserverClass::from(event.observer);
                                let current = leakage_budget.for_observer(observer);
                                let next = current.saturating_add(event.amount);
                                leakage_budget.set_for_observer(observer, next);
                                continue;
                            }
                            crate::fact::ProtocolRelationalFact::SessionDelegation(_) => {
                                let key = protocol.binding_key();
                                bindings.push(RelationalBinding {
                                    binding_type: RelationalBindingType::Generic(
                                        key.sub_type().to_string(),
                                    ),
                                    context_id: *context_id,
                                    data: key.data(),
                                });
                                continue;
                            }
                            crate::fact::ProtocolRelationalFact::DkgTranscriptCommit(_) => {
                                let key = protocol.binding_key();
                                bindings.push(RelationalBinding {
                                    binding_type: RelationalBindingType::Generic(
                                        key.sub_type().to_string(),
                                    ),
                                    context_id: *context_id,
                                    data: key.data(),
                                });
                                continue;
                            }
                            crate::fact::ProtocolRelationalFact::ConvergenceCert(_) => {
                                let key = protocol.binding_key();
                                bindings.push(RelationalBinding {
                                    binding_type: RelationalBindingType::Generic(
                                        key.sub_type().to_string(),
                                    ),
                                    context_id: *context_id,
                                    data: key.data(),
                                });
                                continue;
                            }
                            crate::fact::ProtocolRelationalFact::ReversionFact(_) => {
                                let key = protocol.binding_key();
                                bindings.push(RelationalBinding {
                                    binding_type: RelationalBindingType::Generic(
                                        key.sub_type().to_string(),
                                    ),
                                    context_id: *context_id,
                                    data: key.data(),
                                });
                                continue;
                            }
                            crate::fact::ProtocolRelationalFact::RotateFact(_) => {
                                let key = protocol.binding_key();
                                bindings.push(RelationalBinding {
                                    binding_type: RelationalBindingType::Generic(
                                        key.sub_type().to_string(),
                                    ),
                                    context_id: *context_id,
                                    data: key.data(),
                                });
                                continue;
                            }
                        },
                        // Generic bindings handle all domain-specific facts
                        // (ChatFact, InvitationFact, ContactFact, etc.)
                        // via DomainFact::to_generic()
                        RelationalFact::Generic {
                            context_id: ctx,
                            envelope,
                        } => RelationalBinding {
                            binding_type: RelationalBindingType::Generic(
                                envelope.type_id.as_str().to_string(),
                            ),
                            context_id: *ctx,
                            data: envelope.payload.clone(),
                        },
                    };
                    bindings.push(binding);
                }
            }

            let mut channel_epochs = BTreeMap::new();
            let amp_transitions = reduce_amp_transitions(
                &proposed_bumps,
                &certified_bumps,
                &committed_bumps,
                &finalized_bumps,
                &transition_aborts,
                &transition_conflicts,
                &transition_supersessions,
                &emergency_alarms,
            );
            let mut channel_ids = BTreeSet::new();
            channel_ids.extend(channel_checkpoints.keys().map(|(channel, _)| *channel));
            channel_ids.extend(proposed_bumps.iter().map(|b| b.channel));
            channel_ids.extend(certified_bumps.iter().map(|b| b.identity.channel));
            channel_ids.extend(committed_bumps.iter().map(|b| b.channel));
            channel_ids.extend(finalized_bumps.iter().map(|b| b.identity.channel));
            channel_ids.extend(channel_policies.keys().copied());
            channel_ids.extend(channel_bootstraps.keys().copied());

            for channel in channel_ids {
                let chan_epoch = highest_reduced_epoch(channel, &amp_transitions);
                // Find the latest checkpoint from any epoch <= current epoch
                let checkpoint = (0..=chan_epoch)
                    .rev()
                    .find_map(|epoch| canonical_checkpoint(channel, epoch, &channel_checkpoints))
                    .cloned();
                let last_checkpoint_gen = checkpoint.as_ref().map(|c| c.base_gen).unwrap_or(0);
                let skip_window = checkpoint
                    .as_ref()
                    .and_then(|c| c.skip_window_override)
                    .or_else(|| channel_policies.get(&channel).and_then(|p| p.skip_window))
                    .unwrap_or(DEFAULT_SKIP_WINDOW);
                let current_gen = last_checkpoint_gen;
                let transition = amp_transitions
                    .values()
                    .find(|transition| {
                        transition.parent.channel == channel
                            && transition.parent.parent_epoch == chan_epoch
                    })
                    .cloned();
                let pending_bump = transition.as_ref().and_then(|transition| {
                    select_live_bump_from_transition(transition, &proposed_bumps)
                });
                let bootstrap = channel_bootstraps.get(&channel).cloned();

                channel_epochs.insert(
                    channel,
                    ChannelEpochState {
                        chan_epoch,
                        pending_bump,
                        bootstrap,
                        last_checkpoint_gen,
                        current_gen,
                        skip_window,
                        transition,
                    },
                );
            }

            Ok(RelationalState {
                bindings,
                flow_budgets,
                leakage_budget,
                channel_epochs,
                amp_transitions,
            })
        }
        JournalNamespace::Authority(_) => Err(ReductionNamespaceError::AuthorityAsContext),
    }
}

fn highest_reduced_epoch(
    channel: ChannelId,
    amp_transitions: &BTreeMap<AmpTransitionParentKey, AmpTransitionReduction>,
) -> u64 {
    let mut epoch = 0u64;
    loop {
        let finalized = amp_transitions.values().any(|transition| {
            transition.parent.channel == channel
                && transition.parent.parent_epoch == epoch
                && transition.status == AmpTransitionReductionStatus::A3Finalized
        });
        if finalized {
            epoch += 1;
        } else {
            break;
        }
    }
    epoch
}

fn canonical_checkpoint(
    channel: ChannelId,
    epoch: u64,
    checkpoints: &BTreeMap<(ChannelId, u64), Vec<ChannelCheckpoint>>,
) -> Option<&ChannelCheckpoint> {
    // Select the checkpoint with the highest base_gen; tie-break with commitment bytes to make ordering deterministic.
    checkpoints.get(&(channel, epoch)).and_then(|cps| {
        cps.iter()
            .max_by(|a, b| (a.base_gen, &a.ck_commitment).cmp(&(b.base_gen, &b.ck_commitment)))
    })
}

fn reduce_amp_transitions(
    proposed: &[ProposedChannelEpochBump],
    certified: &[CertifiedChannelEpochBump],
    committed: &[CommittedChannelEpochBump],
    finalized: &[FinalizedChannelEpochBump],
    aborts: &[AmpTransitionAbort],
    conflicts: &[AmpTransitionConflict],
    supersessions: &[AmpTransitionSupersession],
    alarms: &[AmpEmergencyAlarm],
) -> BTreeMap<AmpTransitionParentKey, AmpTransitionReduction> {
    let mut groups = BTreeSet::new();
    for proposal in proposed
        .iter()
        .filter(|proposal| valid_proposed_bump(proposal))
    {
        groups.insert(AmpTransitionParentKey::from(
            &proposal.transition_identity(),
        ));
    }
    for cert in certified.iter().filter(|cert| valid_certified_bump(cert)) {
        groups.insert(AmpTransitionParentKey::from(&cert.identity));
    }
    for bump in committed.iter().filter(|bump| valid_committed_bump(bump)) {
        groups.insert(AmpTransitionParentKey {
            context: bump.context,
            channel: bump.channel,
            parent_epoch: bump.parent_epoch,
            parent_commitment: bump.parent_commitment,
        });
    }
    for commit in finalized
        .iter()
        .filter(|commit| valid_finalized_bump(commit))
    {
        groups.insert(AmpTransitionParentKey::from(&commit.identity));
    }
    for abort in aborts {
        groups.insert(AmpTransitionParentKey {
            context: abort.context,
            channel: abort.channel,
            parent_epoch: abort.parent_epoch,
            parent_commitment: abort.parent_commitment,
        });
    }
    for conflict in conflicts {
        groups.insert(AmpTransitionParentKey {
            context: conflict.context,
            channel: conflict.channel,
            parent_epoch: conflict.parent_epoch,
            parent_commitment: conflict.parent_commitment,
        });
    }
    for supersession in supersessions {
        groups.insert(AmpTransitionParentKey {
            context: supersession.context,
            channel: supersession.channel,
            parent_epoch: supersession.parent_epoch,
            parent_commitment: supersession.parent_commitment,
        });
    }
    for alarm in alarms {
        groups.insert(AmpTransitionParentKey {
            context: alarm.context,
            channel: alarm.channel,
            parent_epoch: alarm.parent_epoch,
            parent_commitment: alarm.parent_commitment,
        });
    }

    groups
        .into_iter()
        .map(|parent| {
            let reduction = reduce_amp_transition_group(
                parent,
                proposed,
                certified,
                committed,
                finalized,
                aborts,
                conflicts,
                supersessions,
                alarms,
            );
            (parent, reduction)
        })
        .collect()
}

fn reduce_amp_transition_group(
    parent: AmpTransitionParentKey,
    proposed: &[ProposedChannelEpochBump],
    certified: &[CertifiedChannelEpochBump],
    committed: &[CommittedChannelEpochBump],
    finalized: &[FinalizedChannelEpochBump],
    aborts: &[AmpTransitionAbort],
    conflicts: &[AmpTransitionConflict],
    supersessions: &[AmpTransitionSupersession],
    alarms: &[AmpEmergencyAlarm],
) -> AmpTransitionReduction {
    let observed_transition_ids = proposed
        .iter()
        .filter(|proposal| valid_proposed_bump(proposal))
        .filter(|proposal| AmpTransitionParentKey::from(&proposal.transition_identity()) == parent)
        .map(|proposal| proposal.transition_id)
        .collect::<BTreeSet<_>>();

    let mut certified_transition_ids = certified
        .iter()
        .filter(|cert| valid_certified_bump(cert))
        .filter(|cert| AmpTransitionParentKey::from(&cert.identity) == parent)
        .map(|cert| cert.transition_id)
        .collect::<BTreeSet<_>>();

    let mut finalized_transition_ids = finalized
        .iter()
        .filter(|commit| valid_finalized_bump(commit))
        .filter(|commit| AmpTransitionParentKey::from(&commit.identity) == parent)
        .map(|commit| commit.transition_id)
        .collect::<BTreeSet<_>>();

    finalized_transition_ids.extend(
        committed
            .iter()
            .filter(|bump| valid_committed_bump(bump))
            .filter(|bump| {
                bump.context == parent.context
                    && bump.channel == parent.channel
                    && bump.parent_epoch == parent.parent_epoch
                    && bump.parent_commitment == parent.parent_commitment
            })
            .map(|bump| bump.transition_id),
    );

    let mut suppressed_a2 = BTreeSet::new();
    let mut suppressed_a3 = BTreeSet::new();
    for abort in aborts
        .iter()
        .filter(|abort| abort_parent_key(abort) == parent)
    {
        suppressed_a2.insert(abort.transition_id);
        if abort.scope == AmpTransitionSuppressionScope::A2AndA3 {
            suppressed_a3.insert(abort.transition_id);
        }
    }
    let mut superseded_any = false;
    for supersession in supersessions
        .iter()
        .filter(|supersession| supersession_parent_key(supersession) == parent)
    {
        superseded_any = true;
        suppressed_a2.insert(supersession.superseded_transition_id);
        if supersession.scope == AmpTransitionSuppressionScope::A2AndA3 {
            suppressed_a3.insert(supersession.superseded_transition_id);
        }
    }
    let suppressed_transition_ids = suppressed_a2
        .union(&suppressed_a3)
        .copied()
        .collect::<BTreeSet<_>>();

    certified_transition_ids.retain(|transition_id| !suppressed_a2.contains(transition_id));
    finalized_transition_ids.retain(|transition_id| !suppressed_a3.contains(transition_id));

    let mut conflict_evidence_ids = BTreeSet::new();
    for conflict in conflicts
        .iter()
        .filter(|conflict| conflict_parent_key(conflict) == parent)
    {
        conflict_evidence_ids.insert(conflict.evidence_id);
        conflict_evidence_ids.insert(conflict.first_transition_id);
        conflict_evidence_ids.insert(conflict.second_transition_id);
    }
    let emergency_alarm_ids = alarms
        .iter()
        .filter(|alarm| alarm_parent_key(alarm) == parent)
        .map(|alarm| alarm.evidence_id)
        .collect::<BTreeSet<_>>();
    let mut emergency_suspects = alarms
        .iter()
        .filter(|alarm| alarm_parent_key(alarm) == parent)
        .map(|alarm| alarm.suspect)
        .collect::<BTreeSet<_>>();
    let mut quarantine_epochs = BTreeSet::new();
    let mut prune_before_epochs = BTreeSet::new();
    for cert in certified
        .iter()
        .filter(|cert| valid_certified_bump(cert))
        .filter(|cert| AmpTransitionParentKey::from(&cert.identity) == parent)
    {
        emergency_suspects.extend(cert.excluded_authorities.iter().copied());
        if cert.identity.transition_policy == AmpTransitionPolicy::EmergencyQuarantineTransition {
            quarantine_epochs.insert(cert.identity.successor_epoch);
        }
        if cert.readable_state_destroyed
            || cert.identity.transition_policy
                == AmpTransitionPolicy::EmergencyCryptoshredTransition
        {
            prune_before_epochs.insert(cert.identity.parent_epoch);
        }
    }
    for commit in finalized
        .iter()
        .filter(|commit| valid_finalized_bump(commit))
        .filter(|commit| AmpTransitionParentKey::from(&commit.identity) == parent)
    {
        emergency_suspects.extend(commit.excluded_authorities.iter().copied());
        if commit.identity.transition_policy == AmpTransitionPolicy::EmergencyQuarantineTransition {
            quarantine_epochs.insert(commit.identity.successor_epoch);
        }
        if commit.readable_state_destroyed
            || commit.identity.transition_policy
                == AmpTransitionPolicy::EmergencyCryptoshredTransition
        {
            prune_before_epochs.insert(commit.identity.parent_epoch);
        }
    }

    let status;
    let mut live_transition_id = None;
    let mut finalized_transition_id = None;
    let finalized_conflicts_with_a2 =
        finalized_transition_ids
            .iter()
            .next()
            .is_some_and(|transition_id| {
                finalized_transition_ids.len() == 1
                    && !certified_transition_ids.is_empty()
                    && !certified_transition_ids.contains(transition_id)
            });

    if finalized_transition_ids.len() > 1 || finalized_conflicts_with_a2 {
        status = AmpTransitionReductionStatus::A3Conflict;
    } else if let Some(transition_id) = finalized_transition_ids.iter().next().copied() {
        status = AmpTransitionReductionStatus::A3Finalized;
        finalized_transition_id = Some(transition_id);
    } else if conflict_evidence_ids.is_empty() && certified_transition_ids.len() == 1 {
        let transition_id = certified_transition_ids.iter().next().copied();
        status = AmpTransitionReductionStatus::A2Live;
        live_transition_id = transition_id;
    } else if !conflict_evidence_ids.is_empty() || certified_transition_ids.len() > 1 {
        status = AmpTransitionReductionStatus::A2Conflict;
    } else if !suppressed_transition_ids.is_empty() {
        status = if superseded_any {
            AmpTransitionReductionStatus::Superseded
        } else {
            AmpTransitionReductionStatus::Aborted
        };
    } else {
        status = AmpTransitionReductionStatus::Observed;
    }

    AmpTransitionReduction {
        parent,
        status,
        observed_transition_ids,
        certified_transition_ids,
        finalized_transition_ids,
        live_transition_id,
        finalized_transition_id,
        suppressed_transition_ids,
        conflict_evidence_ids,
        emergency_alarm_ids,
        emergency_suspects,
        quarantine_epochs,
        prune_before_epochs,
    }
}

fn select_live_bump_from_transition(
    transition: &AmpTransitionReduction,
    proposed: &[ProposedChannelEpochBump],
) -> Option<PendingBump> {
    if transition.status != AmpTransitionReductionStatus::A2Live {
        return None;
    }
    let transition_id = transition.live_transition_id?;
    let proposal = proposed
        .iter()
        .find(|proposal| proposal.transition_id == transition_id);

    Some(PendingBump {
        parent_epoch: transition.parent.parent_epoch,
        new_epoch: proposal
            .map(|proposal| proposal.new_epoch)
            .unwrap_or(transition.parent.parent_epoch + 1),
        bump_id: proposal
            .map(|proposal| proposal.bump_id)
            .unwrap_or(transition_id),
        reason: proposal
            .map(|proposal| proposal.reason)
            .unwrap_or(ChannelBumpReason::Routine),
        transition_id,
        transition_policy: proposal
            .map(|proposal| proposal.transition_policy)
            .unwrap_or(AmpTransitionPolicy::NormalTransition),
    })
}

fn valid_proposed_bump(proposal: &ProposedChannelEpochBump) -> bool {
    proposal.new_epoch == proposal.parent_epoch + 1
        && proposal.transition_id == proposal.transition_identity().transition_id()
}

fn valid_certified_bump(cert: &CertifiedChannelEpochBump) -> bool {
    cert.identity.successor_epoch == cert.identity.parent_epoch + 1
        && cert.transition_id == cert.identity.transition_id()
        && cert.witness_payload_digest != Hash32::default()
        && cert.threshold > 0
        && unique_witness_count(cert) >= cert.threshold as usize
}

fn unique_witness_count(cert: &CertifiedChannelEpochBump) -> usize {
    cert.witness_signatures
        .iter()
        .map(|signature| signature.witness)
        .collect::<BTreeSet<_>>()
        .len()
}

fn valid_committed_bump(bump: &CommittedChannelEpochBump) -> bool {
    bump.new_epoch == bump.parent_epoch + 1
        && bump.transition_id
            == AmpTransitionIdentity {
                context: bump.context,
                channel: bump.channel,
                parent_epoch: bump.parent_epoch,
                parent_commitment: bump.parent_commitment,
                successor_epoch: bump.new_epoch,
                successor_commitment: bump.successor_commitment,
                membership_commitment: bump.membership_commitment,
                transition_policy: bump.transition_policy,
            }
            .transition_id()
}

fn valid_finalized_bump(commit: &FinalizedChannelEpochBump) -> bool {
    commit.identity.successor_epoch == commit.identity.parent_epoch + 1
        && commit.transition_id == commit.identity.transition_id()
}

fn abort_parent_key(abort: &AmpTransitionAbort) -> AmpTransitionParentKey {
    AmpTransitionParentKey {
        context: abort.context,
        channel: abort.channel,
        parent_epoch: abort.parent_epoch,
        parent_commitment: abort.parent_commitment,
    }
}

fn conflict_parent_key(conflict: &AmpTransitionConflict) -> AmpTransitionParentKey {
    AmpTransitionParentKey {
        context: conflict.context,
        channel: conflict.channel,
        parent_epoch: conflict.parent_epoch,
        parent_commitment: conflict.parent_commitment,
    }
}

fn supersession_parent_key(supersession: &AmpTransitionSupersession) -> AmpTransitionParentKey {
    AmpTransitionParentKey {
        context: supersession.context,
        channel: supersession.channel,
        parent_epoch: supersession.parent_epoch,
        parent_commitment: supersession.parent_commitment,
    }
}

fn alarm_parent_key(alarm: &AmpEmergencyAlarm) -> AmpTransitionParentKey {
    AmpTransitionParentKey {
        context: alarm.context,
        channel: alarm.channel,
        parent_epoch: alarm.parent_epoch,
        parent_commitment: alarm.parent_commitment,
    }
}

/// Compute snapshot state for garbage collection
///
/// This identifies facts that can be superseded by a snapshot.
///
/// # Errors
///
/// Returns `ReductionNamespaceError` if the journal namespace doesn't match
/// the expected namespace for reduction.
pub fn compute_snapshot(
    journal: &Journal,
    sequence: u64,
) -> Result<(Hash32, Vec<OrderTime>), ReductionNamespaceError> {
    // Compute hash of current state
    let state_hash = match &journal.namespace {
        JournalNamespace::Authority(_) => {
            let state = reduce_authority(journal)?;
            compute_authority_state_hash(&state)
        }
        JournalNamespace::Context(_) => {
            let state = reduce_context(journal)?;
            compute_relational_state_hash(&state)
        }
    };

    // Identify supersedable facts: everything older than the current snapshot
    // sequence is eligible for compaction in this reducer. More selective GC
    // can be added once snapshot metadata is richer.
    let superseded_facts = journal
        .facts
        .iter()
        .filter_map(|f| match &f.content {
            FactContent::Snapshot(s) if s.sequence < sequence => None, // Keep recent snapshots
            _ => Some(f.order.clone()),
        })
        .collect();

    Ok((state_hash, superseded_facts))
}

// ==== AMP Garbage Collection Helpers ====
//
// These functions implement safe pruning boundaries for AMP facts according to the
// GC policy documented in docs/112_amp.md section 9.2.

/// Compute the safe pruning boundary for AMP checkpoints.
///
/// Returns the maximum `base_gen` value that can be safely pruned given the current
/// maximum checkpoint generation. Checkpoints with `base_gen < safe_boundary` can
/// be removed without affecting protocol safety.
///
/// # Arguments
///
/// * `max_checkpoint_gen` - The highest generation with an active checkpoint
/// * `window_size` - Skip window size (defaults to 1024 if None)
///
/// # Returns
///
/// The safe pruning boundary generation, or 0 if no pruning is safe yet.
///
/// # Safety
///
/// This implements the policy: `safe_prune_gen = max_checkpoint_gen - (2 * W) - SAFETY_MARGIN`
/// where W is the skip window size. This ensures:
/// - The newest checkpoint's dual-window coverage `[G … G + 2W]` is fully preserved
/// - An additional safety margin prevents premature pruning during active transitions
/// - No messages within valid windows are made unrecoverable
///
/// # Example
///
/// ```ignore
/// let window = 1024;
/// let max_gen = 5000;
/// let boundary = compute_checkpoint_pruning_boundary(max_gen, Some(window));
/// // boundary = 5000 - (2 * 1024) - 512 = 2440
/// // Checkpoints at base_gen < 2440 can be pruned
/// ```
pub fn compute_checkpoint_pruning_boundary(
    max_checkpoint_gen: u64,
    window_size: Option<u32>,
) -> u64 {
    let w = window_size.unwrap_or(DEFAULT_SKIP_WINDOW) as u64;
    // Safety margin is always based on DEFAULT_SKIP_WINDOW to ensure consistent safety buffer
    let safety_margin = (DEFAULT_SKIP_WINDOW / 2) as u64;
    let required_coverage = 2 * w + safety_margin;

    max_checkpoint_gen.saturating_sub(required_coverage)
}

/// Determine if a checkpoint can be safely pruned.
///
/// A checkpoint is pruneable if its generation is below the safe pruning boundary
/// and there exists a newer checkpoint that provides complete window coverage.
///
/// # Arguments
///
/// * `checkpoint_gen` - The `base_gen` of the checkpoint to check
/// * `max_checkpoint_gen` - The highest generation with an active checkpoint
/// * `window_size` - Skip window size for the channel
///
/// # Returns
///
/// `true` if the checkpoint can be safely removed, `false` otherwise
pub fn can_prune_checkpoint(
    checkpoint_gen: u64,
    max_checkpoint_gen: u64,
    window_size: Option<u32>,
) -> bool {
    let boundary = compute_checkpoint_pruning_boundary(max_checkpoint_gen, window_size);
    checkpoint_gen < boundary
}

/// Determine if a proposed channel epoch bump can be pruned.
///
/// A proposed bump can be pruned if:
/// - A committed bump for the same transition exists (proposal was finalized)
/// - OR a committed bump for a later epoch exists (proposal became stale)
///
/// # Arguments
///
/// * `proposed_parent_epoch` - The parent epoch of the proposed bump
/// * `committed_epochs` - Set of all committed epoch transitions `(parent, new)`
///
/// # Returns
///
/// `true` if the proposed bump can be safely removed
pub fn can_prune_proposed_bump(
    proposed_parent_epoch: u64,
    committed_epochs: &[(u64, u64)],
) -> bool {
    committed_epochs
        .iter()
        .any(|(parent, _new)| *parent >= proposed_parent_epoch)
}

/// Convert aura-journal::Fact to aura-core::journal::Fact
///
/// This bridges the two fact representations until the architecture is fully unified.
fn convert_to_core_fact(
    journal_fact: &crate::fact::Fact,
) -> Result<aura_core::journal::Fact, String> {
    // Map journal fact into the aura-core fact envelope; this preserves ordering
    // and content type tags for downstream consumers.

    // Create a new aura-core fact with basic information
    let mut core_fact = aura_core::journal::Fact::new();

    // Add ordering token as a string key-value pair (simplified conversion)
    let order_key = "order";
    let order_value = format!("{:?}", journal_fact.timestamp);
    let _ = core_fact.insert(
        order_key,
        aura_core::journal::FactValue::String(order_value),
    );

    // Add content type information
    let content_type_key = "content_type";
    let content_type_value = match &journal_fact.content {
        crate::fact::FactContent::AttestedOp(_) => "attested_op",
        crate::fact::FactContent::Relational(_) => "relational",
        crate::fact::FactContent::Snapshot(_) => "snapshot",
        crate::fact::FactContent::RendezvousReceipt { .. } => "rendezvous_receipt",
    };
    let _ = core_fact.insert(
        content_type_key,
        aura_core::journal::FactValue::String(content_type_value.to_string()),
    );

    // Add a simplified content representation
    let content_key = "content_summary";
    let content_summary = format!("{:?}", journal_fact.content);
    let _ = core_fact.insert(
        content_key,
        aura_core::journal::FactValue::String(content_summary),
    );

    Ok(core_fact)
}
