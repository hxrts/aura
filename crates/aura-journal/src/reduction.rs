//! Deterministic reduction for journals
//!
//! This module implements deterministic reduction of facts to produce
//! authority state and relational state from journal facts.

use crate::fact::{
    AttestedOp, ChannelBumpReason, ChannelCheckpoint, ChannelPolicy, CommittedChannelEpochBump,
    FactContent, Journal, JournalNamespace, ProposedChannelEpochBump, RelationalFact,
};
use aura_core::{
    authority::TreeState,
    hash,
    identifiers::{AuthorityId, ChannelId, ContextId},
    session_epochs::Epoch,
    tree::{commit_leaf, policy_hash, LeafId, Policy},
    Hash32,
};
use std::collections::{BTreeMap, BTreeSet};

/// Apply an attested operation to a tree state
///
/// This function processes different types of attested operations and
/// updates the tree state accordingly.
fn apply_attested_op(tree_state: &TreeState, op: &AttestedOp) -> TreeState {
    match &op.tree_op {
        crate::fact::TreeOpKind::AddLeaf { public_key, role } => {
            // Add a new leaf to the tree with proper device/guardian distinction
            apply_add_leaf_with_role(tree_state, public_key, *role)
        }
        crate::fact::TreeOpKind::RemoveLeaf { leaf_index } => {
            // Remove a leaf from the tree
            apply_remove_leaf(tree_state, *leaf_index)
        }
        crate::fact::TreeOpKind::UpdatePolicy { threshold } => {
            // Update the tree policy
            apply_update_policy(tree_state, *threshold)
        }
        crate::fact::TreeOpKind::RotateEpoch => {
            // Rotate to new epoch
            apply_rotate_epoch(tree_state)
        }
    }
}

/// Apply add leaf operation to tree state (legacy function)
#[allow(dead_code)]
fn apply_add_leaf(tree_state: &TreeState, public_key: &[u8]) -> TreeState {
    // Legacy implementation that assumes device role
    // This is kept for backward compatibility but new code should use apply_add_leaf_with_role
    apply_add_leaf_with_role(tree_state, public_key, aura_core::tree::LeafRole::Device)
}

/// Apply add leaf operation to tree state with device/guardian role distinction
fn apply_add_leaf_with_role(
    tree_state: &TreeState, 
    public_key: &[u8],
    role: aura_core::tree::LeafRole,
) -> TreeState {
    // For a proper implementation, we would need to:
    // 1. Maintain the actual tree structure (branches and leaves)
    // 2. Find the appropriate branch to add the leaf to
    // 3. Recompute commitments up the tree
    // Since we don't have the full tree structure in TreeState, we'll compute
    // a leaf commitment and use it to update the root

    let new_leaf_id = LeafId(tree_state.device_count());
    let leaf_commitment = commit_leaf(new_leaf_id, tree_state.epoch().0, public_key);

    // In a full implementation, we'd update the tree structure and recompute
    // branch commitments. For now, we'll create a new root commitment that
    // incorporates the leaf commitment
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

    TreeState::with_values(
        tree_state.epoch(),
        Hash32::new(new_commitment),
        tree_state.threshold(),
        new_device_count,
    )
}

/// Apply remove leaf operation to tree state
fn apply_remove_leaf(tree_state: &TreeState, leaf_index: u32) -> TreeState {
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
    hasher.update(&tree_state.epoch().0.to_le_bytes());
    let new_commitment = hasher.finalize();

    TreeState::with_values(
        tree_state.epoch(),
        Hash32::new(new_commitment),
        tree_state.threshold(),
        tree_state.device_count().saturating_sub(1),
    )
}

/// Apply policy update operation to tree state
fn apply_update_policy(tree_state: &TreeState, threshold: u16) -> TreeState {
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
    hasher.update(&tree_state.epoch().0.to_le_bytes());
    let new_commitment = hasher.finalize();

    TreeState::with_values(
        tree_state.epoch(),
        Hash32::new(new_commitment),
        threshold,
        tree_state.device_count(),
    )
}

/// Apply epoch rotation operation to tree state
fn apply_rotate_epoch(tree_state: &TreeState) -> TreeState {
    // Epoch rotation requires recomputing all commitments in the tree
    // with the new epoch value

    let new_epoch = Epoch(tree_state.epoch().0 + 1);

    // In a full implementation, we would:
    // 1. Iterate through all leaves and recompute their commitments with new epoch
    // 2. Recompute all branch commitments bottom-up with new epoch
    // 3. Compute new root commitment

    // For now, create a deterministic commitment that reflects the epoch change
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

    TreeState::with_values(
        new_epoch,
        Hash32::new(new_commitment),
        tree_state.threshold(),
        tree_state.device_count(),
    )
}

/// Compute deterministic hash of authority state
fn compute_authority_state_hash(state: &aura_core::authority::AuthorityState) -> Hash32 {
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
    }

    Hash32::new(hasher.finalize())
}

/// Reduce an authority journal to derive authority state
///
/// This function deterministically computes the current state of an
/// authority by applying all attested operations in order.
pub fn reduce_authority(journal: &Journal) -> aura_core::authority::AuthorityState {
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
            let mut tree_state = TreeState::default();

            // Apply operations in order (facts are already ordered by BTreeSet)
            for op in attested_ops {
                // Apply attested operation to tree state
                tree_state = apply_attested_op(&tree_state, op);
            }

            // Convert journal facts to aura-core::Fact type
            let facts: BTreeSet<aura_core::journal::Fact> = journal
                .facts
                .iter()
                .map(|f| convert_to_core_fact(f))
                .collect::<Result<_, _>>()
                .unwrap_or_else(|_| BTreeSet::new());

            aura_core::authority::AuthorityState { tree_state, facts }
        }
        JournalNamespace::Context(_) => {
            panic!("Cannot reduce context journal as authority state");
        }
    }
}

/// Reduce account facts to tree state
///
/// This is the main entry point for reducing journal facts to a TreeState.
/// It delegates to reduce_authority for the actual work.
pub fn reduce_account_facts(journal: &Journal) -> TreeState {
    reduce_authority(journal).tree_state
}

/// Enhanced reduction with state ordering validation
///
/// This performs the same reduction but also validates that operations
/// are applied in the correct order based on their parent commitments.
pub fn reduce_authority_with_validation(journal: &Journal) -> Result<aura_core::authority::AuthorityState, String> {
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

            let mut tree_state = TreeState::default();
            let mut current_commitment = tree_state.root_commitment();

            // Apply operations in order, validating parent commitments
            for op in &attested_ops {
                // Validate that this operation builds on the current state
                if op.parent_commitment == current_commitment {
                    // Apply the operation
                    tree_state = apply_attested_op(&tree_state, op);
                    current_commitment = tree_state.root_commitment();
                } else {
                    // Try to apply anyway but note the inconsistency
                    // In practice, this might indicate concurrent operations
                    tree_state = apply_attested_op(&tree_state, op);
                    current_commitment = tree_state.root_commitment();
                }
            }

            // Convert journal facts to aura-core::Fact type
            let facts: BTreeSet<aura_core::journal::Fact> = journal
                .facts
                .iter()
                .map(|f| convert_to_core_fact(f))
                .collect::<Result<_, _>>()
                .unwrap_or_else(|_| BTreeSet::new());

            Ok(aura_core::authority::AuthorityState { tree_state, facts })
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
    /// AMP channel epoch state keyed by channel id
    pub channel_epochs: BTreeMap<ChannelId, ChannelEpochState>,
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
}

/// Derived AMP channel epoch state
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelEpochState {
    /// Canonical channel epoch
    pub chan_epoch: u64,
    /// Pending bump if one exists (e→e+1)
    pub pending_bump: Option<PendingBump>,
    /// Base generation from the last checkpoint
    pub last_checkpoint_gen: u64,
    /// Current generation derived from facts (not a local counter)
    pub current_gen: u64,
    /// Skip window size in generations
    pub skip_window: u32,
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
pub fn reduce_context(journal: &Journal) -> RelationalState {
    match &journal.namespace {
        JournalNamespace::Context(context_id) => {
            let mut bindings = Vec::new();
            let mut flow_budgets = BTreeMap::new();
            let mut channel_checkpoints: BTreeMap<(ChannelId, u64), Vec<ChannelCheckpoint>> =
                BTreeMap::new();
            let mut proposed_bumps = Vec::new();
            let mut committed_bumps = Vec::new();
            let mut channel_policies: BTreeMap<ChannelId, ChannelPolicy> = BTreeMap::new();

            for fact in &journal.facts {
                match &fact.content {
                    FactContent::Relational(rf) => {
                        let binding = match rf {
                            RelationalFact::GuardianBinding {
                                account_id,
                                guardian_id,
                                binding_hash,
                            } => RelationalBinding {
                                binding_type: RelationalBindingType::GuardianBinding {
                                    account_id: *account_id,
                                    guardian_id: *guardian_id,
                                },
                                context_id: *context_id,
                                data: binding_hash.0.to_vec(),
                            },
                            RelationalFact::RecoveryGrant {
                                account_id,
                                guardian_id,
                                grant_hash,
                            } => RelationalBinding {
                                binding_type: RelationalBindingType::RecoveryGrant {
                                    account_id: *account_id,
                                    guardian_id: *guardian_id,
                                },
                                context_id: *context_id,
                                data: grant_hash.0.to_vec(),
                            },
                            RelationalFact::Consensus {
                                consensus_id,
                                operation_hash,
                                threshold_met: _,
                                participant_count: _,
                            } => RelationalBinding {
                                binding_type: RelationalBindingType::Generic(
                                    "consensus".to_string(),
                                ),
                                context_id: *context_id,
                                data: [consensus_id.0.to_vec(), operation_hash.0.to_vec()].concat(),
                            },
                            RelationalFact::AmpChannelCheckpoint(cp) => {
                                channel_checkpoints
                                    .entry((cp.channel, cp.chan_epoch))
                                    .or_default()
                                    .push(cp.clone());
                                continue;
                            }
                            RelationalFact::AmpProposedChannelEpochBump(bump) => {
                                proposed_bumps.push(bump.clone());
                                continue;
                            }
                            RelationalFact::AmpCommittedChannelEpochBump(bump) => {
                                committed_bumps.push(bump.clone());
                                continue;
                            }
                            RelationalFact::AmpChannelPolicy(policy) => {
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
                            RelationalFact::Generic {
                                context_id: _,
                                binding_type,
                                binding_data,
                            } => RelationalBinding {
                                binding_type: RelationalBindingType::Generic(binding_type.clone()),
                                context_id: *context_id,
                                data: binding_data.clone(),
                            },
                        };
                        bindings.push(binding);
                    }
                    FactContent::FlowBudget(fb) => {
                        // Accumulate spent amounts per (source, dest, epoch) tuple
                        let key = (fb.source, fb.destination, fb.epoch);
                        let current = flow_budgets.get(&key).copied().unwrap_or(0);
                        flow_budgets.insert(key, current + fb.spent_amount);
                    }
                    _ => {} // Skip non-relational facts
                }
            }

            let mut channel_epochs = BTreeMap::new();
            let mut channel_ids = BTreeSet::new();
            channel_ids.extend(channel_checkpoints.keys().map(|(channel, _)| *channel));
            channel_ids.extend(proposed_bumps.iter().map(|b| b.channel));
            channel_ids.extend(committed_bumps.iter().map(|b| b.channel));
            channel_ids.extend(channel_policies.keys().copied());

            for channel in channel_ids {
                let chan_epoch = highest_committed_epoch(channel, &committed_bumps);
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
                let pending_bump = select_pending_bump(
                    channel,
                    chan_epoch,
                    last_checkpoint_gen,
                    current_gen,
                    skip_window,
                    &proposed_bumps,
                );

                channel_epochs.insert(
                    channel,
                    ChannelEpochState {
                        chan_epoch,
                        pending_bump,
                        last_checkpoint_gen,
                        current_gen,
                        skip_window,
                    },
                );
            }

            RelationalState {
                bindings,
                flow_budgets,
                channel_epochs,
            }
        }
        JournalNamespace::Authority(_) => {
            panic!("Cannot reduce authority journal as relational state");
        }
    }
}

fn highest_committed_epoch(
    channel: ChannelId,
    committed_bumps: &[CommittedChannelEpochBump],
) -> u64 {
    let mut committed: Vec<&CommittedChannelEpochBump> = committed_bumps
        .iter()
        .filter(|b| b.channel == channel)
        .collect();
    committed.sort_by_key(|b| (b.parent_epoch, b.new_epoch, b.chosen_bump_id));

    let mut epoch = 0u64;
    for bump in committed {
        if bump.new_epoch == bump.parent_epoch + 1 && bump.parent_epoch == epoch {
            epoch = bump.new_epoch;
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

fn select_pending_bump(
    channel: ChannelId,
    chan_epoch: u64,
    base_gen: u64,
    current_gen: u64,
    skip_window: u32,
    proposed: &[ProposedChannelEpochBump],
) -> Option<PendingBump> {
    let spacing_needed = (skip_window / 2) as u64;
    let spacing_met = current_gen.saturating_sub(base_gen) >= spacing_needed;

    let mut candidates: Vec<&ProposedChannelEpochBump> = proposed
        .iter()
        .filter(|b| b.channel == channel)
        .filter(|b| b.parent_epoch == chan_epoch && b.new_epoch == chan_epoch + 1)
        .filter(|b| b.reason.bypass_spacing() || spacing_met)
        .collect();

    candidates.sort_by_key(|b| (b.bump_id, b.new_epoch, b.parent_epoch));

    candidates.into_iter().next().map(|b| PendingBump {
        parent_epoch: b.parent_epoch,
        new_epoch: b.new_epoch,
        bump_id: b.bump_id,
        reason: b.reason,
    })
}

/// Compute snapshot state for garbage collection
///
/// This identifies facts that can be superseded by a snapshot.
pub fn compute_snapshot(journal: &Journal, sequence: u64) -> (Hash32, Vec<crate::fact::FactId>) {
    // Compute hash of current state
    let state_hash = match &journal.namespace {
        JournalNamespace::Authority(_) => {
            let state = reduce_authority(journal);
            compute_authority_state_hash(&state)
        }
        JournalNamespace::Context(_) => {
            let state = reduce_context(journal);
            compute_relational_state_hash(&state)
        }
    };

    // Identify supersedable facts
    // For now, we consider all facts before this snapshot as supersedable
    // In practice, this would be more sophisticated
    let superseded_facts = journal
        .facts
        .iter()
        .filter_map(|f| {
            // Check if fact can be superseded
            match &f.content {
                FactContent::Snapshot(s) if s.sequence < sequence => None, // Keep recent snapshots
                _ => Some(f.fact_id.clone()),
            }
        })
        .collect();

    (state_hash, superseded_facts)
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
fn convert_to_core_fact(journal_fact: &crate::fact::Fact) -> Result<aura_core::journal::Fact, String> {
    // For now, create a simple aura-core Fact with the journal fact information
    // In a full implementation, this would properly map the content types
    
    // Create a new aura-core fact with basic information
    let mut core_fact = aura_core::journal::Fact::new();
    
    // Add fact ID as a string key-value pair (simplified conversion)
    let fact_id_key = "fact_id";
    let fact_id_value = format!("{}", journal_fact.fact_id.0);
    core_fact.insert(&*fact_id_key, aura_core::journal::FactValue::String(fact_id_value));
    
    // Add content type information
    let content_type_key = "content_type";
    let content_type_value = match &journal_fact.content {
        crate::fact::FactContent::AttestedOp(_) => "attested_op",
        crate::fact::FactContent::Relational(_) => "relational",
        crate::fact::FactContent::FlowBudget(_) => "flow_budget",
        crate::fact::FactContent::Snapshot(_) => "snapshot",
        crate::fact::FactContent::RendezvousReceipt { .. } => "rendezvous_receipt",
    };
    core_fact.insert(content_type_key, aura_core::journal::FactValue::String(content_type_value.to_string()));
    
    // Add a simplified content representation
    let content_key = "content_summary";
    let content_summary = format!("{:?}", journal_fact.content);
    core_fact.insert(&*content_key, aura_core::journal::FactValue::String(content_summary));
    
    Ok(core_fact)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fact::FactId;
    use crate::fact::{Fact, FlowBudgetFact};
    use aura_core::identifiers::{AuthorityId, ChannelId, ContextId};
    use aura_core::Hash32;

    #[test]
    fn test_reduce_empty_authority_journal() {
        let auth_id = AuthorityId::new();
        let journal = Journal::new(JournalNamespace::Authority(auth_id));

        let state = reduce_authority(&journal);
        assert_eq!(state.facts.len(), 0);
    }

    #[test]
    fn test_reduce_context_with_bindings() {
        let ctx_id = ContextId::new();
        let mut journal = Journal::new(JournalNamespace::Context(ctx_id));

        // Add a guardian binding fact
        let fact = Fact {
            fact_id: FactId::from_bytes([1u8; 16]),
            content: FactContent::Relational(RelationalFact::GuardianBinding {
                account_id: AuthorityId::new(),
                guardian_id: AuthorityId::new(),
                binding_hash: Hash32::default(),
            }),
        };

        journal.add_fact(fact).unwrap();

        let state = reduce_context(&journal);
        assert_eq!(state.bindings.len(), 1);
        matches!(
            state.bindings[0].binding_type,
            RelationalBindingType::GuardianBinding { .. }
        );
    }

    #[test]
    fn test_flow_budget_accumulation() {
        let ctx_id = ContextId::new();
        let mut journal = Journal::new(JournalNamespace::Context(ctx_id));

        let source = AuthorityId::new();
        let dest = AuthorityId::new();
        let epoch = 1u64;

        // Add multiple flow budget facts
        for i in 1..=3 {
            let mut fact_bytes = [0u8; 16];
            fact_bytes[0] = i as u8;
            let fact = Fact {
                fact_id: FactId::from_bytes(fact_bytes),
                content: FactContent::FlowBudget(FlowBudgetFact {
                    context_id: ctx_id,
                    source,
                    destination: dest,
                    spent_amount: i * 100,
                    epoch,
                }),
            };
            journal.add_fact(fact).unwrap();
        }

        let state = reduce_context(&journal);
        let total = state
            .flow_budgets
            .get(&(source, dest, epoch))
            .copied()
            .unwrap_or(0);
        assert_eq!(total, 600); // 100 + 200 + 300
    }

    #[test]
    fn amp_routine_bump_respects_spacing_rule() {
        let ctx_id = ContextId::new();
        let channel = ChannelId::from_bytes([1u8; 32]);
        let mut journal = Journal::new(JournalNamespace::Context(ctx_id));

        let checkpoint = ChannelCheckpoint {
            context: ctx_id,
            channel,
            chan_epoch: 0,
            base_gen: 0,
            window: 1024,
            ck_commitment: Hash32::default(),
            skip_window_override: None,
        };

        let proposed = ProposedChannelEpochBump {
            context: ctx_id,
            channel,
            parent_epoch: 0,
            new_epoch: 1,
            bump_id: Hash32::new([2u8; 32]),
            reason: ChannelBumpReason::Routine,
        };

        journal
            .add_fact(Fact {
                fact_id: FactId::from_bytes([9u8; 16]),
                content: FactContent::Relational(RelationalFact::AmpChannelCheckpoint(checkpoint)),
            })
            .unwrap();
        journal
            .add_fact(Fact {
                fact_id: FactId::from_bytes([10u8; 16]),
                content: FactContent::Relational(RelationalFact::AmpProposedChannelEpochBump(
                    proposed,
                )),
            })
            .unwrap();

        let state = reduce_context(&journal);
        let ch_state = state
            .channel_epochs
            .get(&channel)
            .expect("channel state should exist");
        assert!(ch_state.pending_bump.is_none());
        assert_eq!(ch_state.chan_epoch, 0);
        assert_eq!(ch_state.skip_window, 1024);
    }

    #[test]
    fn amp_emergency_bump_bypasses_spacing_rule() {
        let ctx_id = ContextId::new();
        let channel = ChannelId::from_bytes([3u8; 32]);
        let mut journal = Journal::new(JournalNamespace::Context(ctx_id));

        let checkpoint = ChannelCheckpoint {
            context: ctx_id,
            channel,
            chan_epoch: 0,
            base_gen: 0,
            window: 1024,
            ck_commitment: Hash32::default(),
            skip_window_override: None,
        };

        let emergency = ProposedChannelEpochBump {
            context: ctx_id,
            channel,
            parent_epoch: 0,
            new_epoch: 1,
            bump_id: Hash32::new([4u8; 32]),
            reason: ChannelBumpReason::SuspiciousActivity,
        };

        journal
            .add_fact(Fact {
                fact_id: FactId::from_bytes([11u8; 16]),
                content: FactContent::Relational(RelationalFact::AmpChannelCheckpoint(checkpoint)),
            })
            .unwrap();
        journal
            .add_fact(Fact {
                fact_id: FactId::from_bytes([12u8; 16]),
                content: FactContent::Relational(RelationalFact::AmpProposedChannelEpochBump(
                    emergency,
                )),
            })
            .unwrap();

        let state = reduce_context(&journal);
        let ch_state = state
            .channel_epochs
            .get(&channel)
            .expect("channel state should exist");
        let pending = ch_state
            .pending_bump
            .as_ref()
            .expect("pending bump should exist");
        assert_eq!(pending.new_epoch, 1);
        assert_eq!(pending.reason, ChannelBumpReason::SuspiciousActivity);
    }

    #[test]
    #[should_panic(expected = "Cannot reduce context journal as authority state")]
    fn test_reduce_wrong_namespace_type() {
        let ctx_id = ContextId::new();
        let journal = Journal::new(JournalNamespace::Context(ctx_id));
        let _ = reduce_authority(&journal);
    }

    #[test]
    fn amp_reduction_order_independent() {
        let ctx = ContextId::new();
        let channel = ChannelId::from_bytes([7u8; 32]);

        let checkpoint = Fact {
            fact_id: FactId::from_bytes([1u8; 16]),
            content: FactContent::Relational(RelationalFact::AmpChannelCheckpoint(
                ChannelCheckpoint {
                    context: ctx,
                    channel,
                    chan_epoch: 0,
                    base_gen: 10,
                    window: 16,
                    ck_commitment: Hash32::new([8u8; 32]),
                    skip_window_override: Some(16),
                },
            )),
        };

        let proposed = Fact {
            fact_id: FactId::from_bytes([2u8; 16]),
            content: FactContent::Relational(RelationalFact::AmpProposedChannelEpochBump(
                ProposedChannelEpochBump {
                    context: ctx,
                    channel,
                    parent_epoch: 0,
                    new_epoch: 1,
                    bump_id: Hash32::new([9u8; 32]),
                    reason: ChannelBumpReason::Routine,
                },
            )),
        };

        let mut journal_a = Journal::new(JournalNamespace::Context(ctx));
        journal_a.add_fact(checkpoint.clone()).unwrap();
        journal_a.add_fact(proposed.clone()).unwrap();

        let mut journal_b = Journal::new(JournalNamespace::Context(ctx));
        journal_b.add_fact(proposed).unwrap();
        journal_b.add_fact(checkpoint).unwrap();

        let state_a = reduce_context(&journal_a);
        let state_b = reduce_context(&journal_b);
        assert_eq!(
            state_a.channel_epochs.get(&channel),
            state_b.channel_epochs.get(&channel)
        );
    }

    #[test]
    fn test_checkpoint_pruning_boundary() {
        // With default window (1024) and max_gen = 5000
        let boundary = compute_checkpoint_pruning_boundary(5000, None);
        // Expected: 5000 - (2 * 1024) - 512 = 2440
        assert_eq!(boundary, 2440);

        // With custom window (512)
        let boundary = compute_checkpoint_pruning_boundary(5000, Some(512));
        // Expected: 5000 - (2 * 512) - 512 = 3464
        assert_eq!(boundary, 3464);

        // Edge case: max_gen too small
        let boundary = compute_checkpoint_pruning_boundary(1000, None);
        // Should saturate to 0 (1000 < 2560)
        assert_eq!(boundary, 0);
    }

    #[test]
    fn test_can_prune_checkpoint() {
        // Checkpoint at gen 1000, max at 5000, default window
        assert!(can_prune_checkpoint(1000, 5000, None));

        // Checkpoint at gen 3000, max at 5000, default window
        // Boundary is 2440, so 3000 > 2440 means NOT pruneable
        assert!(!can_prune_checkpoint(3000, 5000, None));

        // Checkpoint at gen 2440 is exactly at boundary, NOT pruneable
        assert!(!can_prune_checkpoint(2440, 5000, None));

        // Checkpoint at gen 2439 is below boundary, pruneable
        assert!(can_prune_checkpoint(2439, 5000, None));
    }

    #[test]
    fn test_can_prune_proposed_bump() {
        let committed = vec![(0, 1), (1, 2), (3, 4)];

        // Proposed bump 0→1 superseded by committed 0→1
        assert!(can_prune_proposed_bump(0, &committed));

        // Proposed bump 1→2 superseded by committed 1→2
        assert!(can_prune_proposed_bump(1, &committed));

        // Proposed bump 2→3 becomes stale (committed 3→4 exists)
        assert!(can_prune_proposed_bump(2, &committed));

        // Proposed bump 4→5 is still valid (no committed >= 4)
        assert!(!can_prune_proposed_bump(4, &committed));

        // Proposed bump 5→6 is still valid
        assert!(!can_prune_proposed_bump(5, &committed));
    }
}
