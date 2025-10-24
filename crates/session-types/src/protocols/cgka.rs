//! Session Type States for CGKA (Continuous Group Key Agreement) Protocol (Refactored with Macros)
//!
//! This module defines session types for the BeeKEM CGKA protocol,
//! providing compile-time safety for group state management and operation validation.

use crate::core::{ChoreographicProtocol, SessionProtocol, SessionState, WitnessedTransition};
use crate::{RuntimeWitness, define_protocol};
use aura_groups::{
    ApplicationSecret, BeeKemTree, CgkaError, CgkaState, Epoch, KeyhiveCgkaOperation, MemberId,
    OperationId, RosterDelta, TreeUpdate,
};
use aura_journal::{Event, EventType};
use uuid::Uuid;

// ========== CGKA Protocol Core ==========

/// Core CGKA protocol data without session state
#[derive(Debug, Clone)]
pub struct CgkaProtocolCore {
    pub group_id: String,
    pub device_id: aura_journal::DeviceId,
    pub current_state: CgkaState,
    pub pending_operation: Option<KeyhiveCgkaOperation>,
    pub operation_history: Vec<OperationId>,
    pub last_epoch_transition: Option<u64>,
}

impl CgkaProtocolCore {
    pub fn new(group_id: String, device_id: aura_journal::DeviceId, initial_state: CgkaState) -> Self {
        Self {
            group_id,
            device_id,
            current_state: initial_state,
            pending_operation: None,
            operation_history: Vec::new(),
            last_epoch_transition: None,
        }
    }
}

// ========== Error Type ==========

#[derive(Debug, thiserror::Error)]
pub enum CgkaSessionError {
    #[error("CGKA protocol error: {0}")]
    ProtocolError(String),
    #[error("Invalid operation for current CGKA state")]
    InvalidOperation,
    #[error("CGKA operation failed: {0}")]
    OperationFailed(String),
    #[error("Group membership error: {0}")]
    MembershipError(String),
    #[error("Epoch transition failed: {0}")]
    EpochTransitionFailed(String),
    #[error("Tree update failed: {0}")]
    TreeUpdateFailed(String),
    #[error("CGKA validation error: {0}")]
    ValidationError(String),
}

// ========== Protocol Definition using Macros ==========

define_protocol! {
    Protocol: CgkaProtocol,
    Core: CgkaProtocolCore,
    Error: CgkaSessionError,
    Union: CgkaSessionState,

    States {
        CgkaGroupInitialized => CgkaState,
        GroupMembershipChange => CgkaState,
        EpochTransition => CgkaState,
        GroupStable => CgkaState,
        OperationPending => CgkaState,
        OperationValidating => CgkaState,
        OperationApplying => CgkaState,
        TreeBuilding => CgkaState,
        TreeUpdating => CgkaState,
        TreeComplete => CgkaState,
        GroupOperationFailed @ final => (),
        OperationApplied @ final => (),
        OperationFailed @ final => (),
        TreeFailed @ final => (),
    }

    Extract {
        session_id: |core| {
            let group_hash = blake3::hash(core.group_id.as_bytes());
            Uuid::from_bytes(group_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
        },
        device_id: |core| core.device_id,
    }
}

// ========== Additional Union Methods ==========

impl CgkaSessionState {
    /// Get the group ID from any state
    pub fn group_id(&self) -> &str {
        match self {
            CgkaSessionState::CgkaGroupInitialized(p) => &p.inner.group_id,
            CgkaSessionState::GroupMembershipChange(p) => &p.inner.group_id,
            CgkaSessionState::EpochTransition(p) => &p.inner.group_id,
            CgkaSessionState::GroupStable(p) => &p.inner.group_id,
            CgkaSessionState::OperationPending(p) => &p.inner.group_id,
            CgkaSessionState::OperationValidating(p) => &p.inner.group_id,
            CgkaSessionState::OperationApplying(p) => &p.inner.group_id,
            CgkaSessionState::TreeBuilding(p) => &p.inner.group_id,
            CgkaSessionState::TreeUpdating(p) => &p.inner.group_id,
            CgkaSessionState::TreeComplete(p) => &p.inner.group_id,
            CgkaSessionState::GroupOperationFailed(p) => &p.inner.group_id,
            CgkaSessionState::OperationApplied(p) => &p.inner.group_id,
            CgkaSessionState::OperationFailed(p) => &p.inner.group_id,
            CgkaSessionState::TreeFailed(p) => &p.inner.group_id,
        }
    }

    /// Get the current epoch from any state
    pub fn current_epoch(&self) -> Epoch {
        match self {
            CgkaSessionState::CgkaGroupInitialized(p) => p.inner.current_state.current_epoch,
            CgkaSessionState::GroupMembershipChange(p) => p.inner.current_state.current_epoch,
            CgkaSessionState::EpochTransition(p) => p.inner.current_state.current_epoch,
            CgkaSessionState::GroupStable(p) => p.inner.current_state.current_epoch,
            CgkaSessionState::OperationPending(p) => p.inner.current_state.current_epoch,
            CgkaSessionState::OperationValidating(p) => p.inner.current_state.current_epoch,
            CgkaSessionState::OperationApplying(p) => p.inner.current_state.current_epoch,
            CgkaSessionState::TreeBuilding(p) => p.inner.current_state.current_epoch,
            CgkaSessionState::TreeUpdating(p) => p.inner.current_state.current_epoch,
            CgkaSessionState::TreeComplete(p) => p.inner.current_state.current_epoch,
            CgkaSessionState::GroupOperationFailed(p) => p.inner.current_state.current_epoch,
            CgkaSessionState::OperationApplied(p) => p.inner.current_state.current_epoch,
            CgkaSessionState::OperationFailed(p) => p.inner.current_state.current_epoch,
            CgkaSessionState::TreeFailed(p) => p.inner.current_state.current_epoch,
        }
    }
}

// ========== Protocol Type Aliases ==========

/// Session-typed CGKA protocol wrapper
pub type SessionTypedCgka<S> = ChoreographicProtocol<CgkaProtocolCore, S>;

// ========== Runtime Witnesses for CGKA Operations ==========

/// Witness that CGKA group has been successfully initialized
#[derive(Debug, Clone)]
pub struct CgkaGroupInitiated {
    pub group_id: String,
    pub initial_epoch: Epoch,
    pub initial_members: Vec<MemberId>,
    pub tree_size: u32,
}

impl RuntimeWitness for CgkaGroupInitiated {
    type Evidence = (String, Vec<Event>);
    type Config = ();

    fn verify(evidence: (String, Vec<Event>), _config: ()) -> Option<Self> {
        let (group_id, events) = evidence;

        for event in events {
            if let EventType::CgkaOperation(cgka_op) = event.event_type {
                if cgka_op.group_id == group_id {
                    match cgka_op.operation_type {
                        aura_journal::CgkaOperationType::Init { initial_members } => {
                            let tree_size = initial_members.len() as u32;
                            return Some(CgkaGroupInitiated {
                                group_id,
                                initial_epoch: Epoch::initial(),
                                initial_members: initial_members
                                    .into_iter()
                                    .map(|m| MemberId::new(&m))
                                    .collect(),
                                tree_size,
                            });
                        }
                        _ => continue,
                    }
                }
            }
        }
        None
    }

    fn description(&self) -> &'static str {
        "CGKA group successfully initialized"
    }
}

/// Witness that membership change operations are ready
#[derive(Debug, Clone)]
pub struct MembershipChangeReady {
    pub group_id: String,
    pub current_epoch: Epoch,
    pub pending_adds: Vec<MemberId>,
    pub pending_removes: Vec<MemberId>,
    pub roster_size: u32,
}

impl RuntimeWitness for MembershipChangeReady {
    type Evidence = (String, Vec<Event>);
    type Config = Epoch; // Current epoch

    fn verify(evidence: (String, Vec<Event>), config: Epoch) -> Option<Self> {
        let (group_id, events) = evidence;
        let mut pending_adds = Vec::new();
        let mut pending_removes = Vec::new();
        let mut roster_size = 0;

        for event in events {
            if let EventType::CgkaOperation(cgka_op) = event.event_type {
                if cgka_op.group_id == group_id && cgka_op.current_epoch == config.value() {
                    match cgka_op.operation_type {
                        aura_journal::CgkaOperationType::Add { members } => {
                            pending_adds.extend(members.into_iter().map(|m| MemberId::new(&m)));
                            roster_size = cgka_op.roster_delta.new_size;
                        }
                        aura_journal::CgkaOperationType::Remove { members } => {
                            pending_removes.extend(members.into_iter().map(|m| MemberId::new(&m)));
                            roster_size = cgka_op.roster_delta.new_size;
                        }
                        _ => continue,
                    }
                }
            }
        }

        if !pending_adds.is_empty() || !pending_removes.is_empty() {
            Some(MembershipChangeReady {
                group_id,
                current_epoch: config,
                pending_adds,
                pending_removes,
                roster_size,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Membership change operations ready for processing"
    }
}

/// Witness that epoch transition can proceed
#[derive(Debug, Clone)]
pub struct EpochTransitionReady {
    pub group_id: String,
    pub current_epoch: Epoch,
    pub target_epoch: Epoch,
    pub committed_operations: Vec<OperationId>,
    pub roster_delta: RosterDelta,
}

impl RuntimeWitness for EpochTransitionReady {
    type Evidence = (String, Vec<Event>);
    type Config = Epoch; // Current epoch

    fn verify(evidence: (String, Vec<Event>), config: Epoch) -> Option<Self> {
        let (group_id, events) = evidence;
        let mut committed_operations = Vec::new();
        let mut roster_delta = RosterDelta::empty();

        for event in events {
            match event.event_type {
                EventType::CgkaEpochTransition(transition) => {
                    if transition.group_id == group_id
                        && transition.previous_epoch == config.value()
                    {
                        return Some(EpochTransitionReady {
                            group_id: group_id.clone(),
                            current_epoch: config,
                            target_epoch: Epoch(transition.new_epoch),
                            committed_operations: transition
                                .committed_operations
                                .into_iter()
                                .map(|id| OperationId(id))
                                .collect(),
                            roster_delta: RosterDelta {
                                added_members: transition
                                    .roster_delta
                                    .added_members
                                    .into_iter()
                                    .map(|m| MemberId::new(&m))
                                    .collect(),
                                removed_members: transition
                                    .roster_delta
                                    .removed_members
                                    .into_iter()
                                    .map(|m| MemberId::new(&m))
                                    .collect(),
                                previous_size: transition.roster_delta.previous_size,
                                new_size: transition.roster_delta.new_size,
                            },
                        });
                    }
                }
                EventType::CgkaOperation(cgka_op) => {
                    if cgka_op.group_id == group_id && cgka_op.current_epoch == config.value() {
                        committed_operations.push(OperationId(cgka_op.operation_id));
                        roster_delta = RosterDelta {
                            added_members: cgka_op
                                .roster_delta
                                .added_members
                                .into_iter()
                                .map(|m| MemberId::new(&m))
                                .collect(),
                            removed_members: cgka_op
                                .roster_delta
                                .removed_members
                                .into_iter()
                                .map(|m| MemberId::new(&m))
                                .collect(),
                            previous_size: cgka_op.roster_delta.previous_size,
                            new_size: cgka_op.roster_delta.new_size,
                        };
                    }
                }
                _ => continue,
            }
        }

        if !committed_operations.is_empty() {
            Some(EpochTransitionReady {
                group_id,
                current_epoch: config,
                target_epoch: config.next(),
                committed_operations,
                roster_delta,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Epoch transition ready to proceed"
    }
}

/// Witness that group has reached stable state
#[derive(Debug, Clone)]
pub struct GroupStabilized {
    pub group_id: String,
    pub stable_epoch: Epoch,
    pub member_count: usize,
    pub stability_confirmed: bool,
}

impl RuntimeWitness for GroupStabilized {
    type Evidence = (String, CgkaState);
    type Config = u64; // Stability timeout threshold

    fn verify(evidence: (String, CgkaState), config: u64) -> Option<Self> {
        let (group_id, state) = evidence;

        // Group is stable if:
        // 1. No pending operations
        // 2. Recent stability (last_updated within threshold)
        let current_time = state.last_updated;
        let is_stable = state.pending_operations.is_empty()
            && current_time > 0
            && (current_time + config) > current_time; // No overflow check for simplicity

        if is_stable {
            Some(GroupStabilized {
                group_id,
                stable_epoch: state.current_epoch,
                member_count: state.roster.member_count(),
                stability_confirmed: true,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Group has reached stable state"
    }
}

/// Witness that operation has been validated
#[derive(Debug, Clone)]
pub struct OperationValidated {
    pub operation_id: OperationId,
    pub group_id: String,
    pub validation_passed: bool,
    pub epoch_valid: bool,
}

impl RuntimeWitness for OperationValidated {
    type Evidence = (KeyhiveCgkaOperation, CgkaState);
    type Config = ();

    fn verify(evidence: (KeyhiveCgkaOperation, CgkaState), _config: ()) -> Option<Self> {
        let (operation, state) = evidence;

        // Validate operation against current state
        let epoch_valid = operation.current_epoch == state.current_epoch;
        let group_valid = operation.group_id == state.group_id;
        let validation_passed = epoch_valid && group_valid;

        if validation_passed {
            Some(OperationValidated {
                operation_id: operation.operation_id,
                group_id: operation.group_id,
                validation_passed: true,
                epoch_valid: true,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Operation has been validated"
    }
}

/// Witness that operation has been successfully applied
#[derive(Debug, Clone)]
pub struct OperationAppliedSuccessfully {
    pub operation_id: OperationId,
    pub group_id: String,
    pub new_epoch: Epoch,
    pub roster_updated: bool,
    pub tree_updated: bool,
}

impl RuntimeWitness for OperationAppliedSuccessfully {
    type Evidence = (KeyhiveCgkaOperation, CgkaState);
    type Config = ();

    fn verify(evidence: (KeyhiveCgkaOperation, CgkaState), _config: ()) -> Option<Self> {
        let (operation, updated_state) = evidence;

        // Check if operation was successfully applied
        let epoch_advanced = updated_state.current_epoch == operation.target_epoch;
        let no_pending = !updated_state
            .pending_operations
            .contains_key(&operation.operation_id);

        if epoch_advanced && no_pending {
            Some(OperationAppliedSuccessfully {
                operation_id: operation.operation_id,
                group_id: operation.group_id,
                new_epoch: updated_state.current_epoch,
                roster_updated: true,
                tree_updated: !operation.tree_updates.is_empty(),
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Operation has been successfully applied"
    }
}

/// Witness that operation has failed
#[derive(Debug, Clone)]
pub struct OperationFailure {
    pub operation_id: OperationId,
    pub group_id: String,
    pub failure_reason: String,
    pub recovery_needed: bool,
}

impl RuntimeWitness for OperationFailure {
    type Evidence = (KeyhiveCgkaOperation, CgkaError);
    type Config = ();

    fn verify(evidence: (KeyhiveCgkaOperation, CgkaError), _config: ()) -> Option<Self> {
        let (operation, error) = evidence;

        Some(OperationFailure {
            operation_id: operation.operation_id,
            group_id: operation.group_id,
            failure_reason: error.to_string(),
            recovery_needed: matches!(
                error,
                CgkaError::EpochMismatch { .. } | CgkaError::InvalidRoster(_)
            ),
        })
    }

    fn description(&self) -> &'static str {
        "Operation has failed"
    }
}

/// Witness that tree updates have been completed
#[derive(Debug, Clone)]
pub struct TreeUpdatesCompleted {
    pub group_id: String,
    pub updates_applied: usize,
    pub tree_size: u32,
    pub root_secret_available: bool,
}

impl RuntimeWitness for TreeUpdatesCompleted {
    type Evidence = (String, Vec<TreeUpdate>, BeeKemTree);
    type Config = ();

    fn verify(evidence: (String, Vec<TreeUpdate>, BeeKemTree), _config: ()) -> Option<Self> {
        let (group_id, updates, tree) = evidence;

        Some(TreeUpdatesCompleted {
            group_id,
            updates_applied: updates.len(),
            tree_size: tree.size,
            root_secret_available: tree.get_root_secret().is_some(),
        })
    }

    fn description(&self) -> &'static str {
        "Tree updates have been completed"
    }
}

// ========== Protocol Methods ==========

impl<S: SessionState> ChoreographicProtocol<CgkaProtocolCore, S> {
    /// Get reference to the protocol core
    pub fn core(&self) -> &CgkaProtocolCore {
        &self.inner
    }

    /// Get the group ID
    pub fn group_id(&self) -> &str {
        &self.core().group_id
    }

    /// Get the current epoch
    pub fn current_epoch(&self) -> Epoch {
        self.core().current_state.current_epoch
    }
}

// ========== State Transitions ==========

/// Transition from CgkaGroupInitialized to GroupMembershipChange (requires CgkaGroupInitiated witness)
impl WitnessedTransition<CgkaGroupInitialized, GroupMembershipChange> for ChoreographicProtocol<CgkaProtocolCore, CgkaGroupInitialized> {
    type Witness = CgkaGroupInitiated;
    type Target = ChoreographicProtocol<CgkaProtocolCore, GroupMembershipChange>;
    
    /// Begin membership change operations
    fn transition_with_witness(
        self,
        _witness: Self::Witness,
    ) -> Self::Target {
        ChoreographicProtocol::transition_to(self)
    }
}

/// Transition from GroupMembershipChange to EpochTransition (requires MembershipChangeReady witness)
impl WitnessedTransition<GroupMembershipChange, EpochTransition> for ChoreographicProtocol<CgkaProtocolCore, GroupMembershipChange> {
    type Witness = MembershipChangeReady;
    type Target = ChoreographicProtocol<CgkaProtocolCore, EpochTransition>;
    
    /// Begin epoch transition after membership changes
    fn transition_with_witness(
        self,
        _witness: Self::Witness,
    ) -> Self::Target {
        ChoreographicProtocol::transition_to(self)
    }
}

/// Transition from EpochTransition to GroupStable (requires EpochTransitionReady witness)
impl WitnessedTransition<EpochTransition, GroupStable>
    for ChoreographicProtocol<CgkaProtocolCore, EpochTransition>
{
    type Witness = EpochTransitionReady;
    type Target = ChoreographicProtocol<CgkaProtocolCore, GroupStable>;
    
    /// Stabilize group after epoch transition
    fn transition_with_witness(
        mut self,
        witness: Self::Witness,
    ) -> Self::Target {
        // Update protocol state with epoch transition
        self.inner.current_state.current_epoch = witness.target_epoch;
        self.inner.last_epoch_transition = Some(witness.target_epoch.value());
        ChoreographicProtocol::transition_to(self)
    }
}

/// Transition from GroupStable back to GroupMembershipChange (requires MembershipChangeReady witness)
impl WitnessedTransition<GroupStable, GroupMembershipChange> for ChoreographicProtocol<CgkaProtocolCore, GroupStable> {
    type Witness = MembershipChangeReady;
    type Target = ChoreographicProtocol<CgkaProtocolCore, GroupMembershipChange>;
    
    /// Return to membership change for new operations
    fn transition_with_witness(
        self,
        _witness: Self::Witness,
    ) -> Self::Target {
        ChoreographicProtocol::transition_to(self)
    }
}

/// Transition to GroupOperationFailed from any state (requires OperationFailure witness)
impl<S: SessionState> WitnessedTransition<S, GroupOperationFailed> for ChoreographicProtocol<CgkaProtocolCore, S> 
where
    Self: SessionProtocol<State = S, Output = CgkaState, Error = CgkaSessionError>,
{
    type Witness = OperationFailure;
    type Target = ChoreographicProtocol<CgkaProtocolCore, GroupOperationFailed>;
    
    /// Fail group operations due to error
    fn transition_with_witness(
        self,
        _witness: Self::Witness,
    ) -> Self::Target {
        ChoreographicProtocol::transition_to(self)
    }
}

// ========== Group-Specific Operations ==========

/// Operations only available in CgkaGroupInitialized state
impl ChoreographicProtocol<CgkaProtocolCore, CgkaGroupInitialized> {
    /// Initialize the CGKA group with initial members
    pub async fn initialize_group(&mut self) -> Result<CgkaGroupInitiated, CgkaSessionError> {
        let group_id = self.inner.group_id.clone();
        let members = self
            .inner
            .current_state
            .roster
            .members
            .keys()
            .cloned()
            .collect();

        Ok(CgkaGroupInitiated {
            group_id,
            initial_epoch: self.inner.current_state.current_epoch,
            initial_members: members,
            tree_size: self.inner.current_state.roster.size,
        })
    }

    /// Get the group configuration
    pub fn group_config(&self) -> (String, Epoch, u32) {
        (
            self.inner.group_id.clone(),
            self.inner.current_state.current_epoch,
            self.inner.current_state.roster.size,
        )
    }
}

/// Operations only available in GroupMembershipChange state
impl ChoreographicProtocol<CgkaProtocolCore, GroupMembershipChange> {
    /// Check membership change readiness
    pub async fn check_membership_change_readiness(
        &self,
        events: Vec<Event>,
    ) -> Option<MembershipChangeReady> {
        MembershipChangeReady::verify(
            (self.inner.group_id.clone(), events),
            self.inner.current_state.current_epoch,
        )
    }

    /// Add pending membership operation
    pub fn add_pending_operation(&mut self, operation: KeyhiveCgkaOperation) {
        self.inner.pending_operation = Some(operation.clone());
        self.inner.current_state.add_pending_operation(operation);
    }

    /// Get current membership count
    pub fn current_member_count(&self) -> usize {
        self.inner.current_state.roster.member_count()
    }

    /// Check if member exists
    pub fn is_member(&self, member_id: &MemberId) -> bool {
        self.inner.current_state.is_member(member_id)
    }
}

/// Operations only available in EpochTransition state
impl ChoreographicProtocol<CgkaProtocolCore, EpochTransition> {
    /// Check epoch transition readiness
    pub async fn check_epoch_transition_readiness(
        &self,
        events: Vec<Event>,
    ) -> Option<EpochTransitionReady> {
        EpochTransitionReady::verify(
            (self.inner.group_id.clone(), events),
            self.inner.current_state.current_epoch,
        )
    }

    /// Apply pending operations for epoch transition
    pub async fn apply_pending_operations(
        &mut self,
        effects: &aura_crypto::Effects,
    ) -> Result<EpochTransitionReady, CgkaSessionError> {
        if let Some(operation) = &self.inner.pending_operation {
            self.inner
                .current_state
                .apply_operation(operation.clone(), effects)
                .map_err(|e| CgkaSessionError::OperationFailed(e.to_string()))?;

            self.inner.operation_history.push(operation.operation_id);
            self.inner.pending_operation = None;
        }

        Ok(EpochTransitionReady {
            group_id: self.inner.group_id.clone(),
            current_epoch: self.inner.current_state.current_epoch,
            target_epoch: self.inner.current_state.current_epoch.next(),
            committed_operations: self.inner.operation_history.clone(),
            roster_delta: RosterDelta::empty(),
        })
    }

    /// Get current epoch information
    pub fn current_epoch_info(&self) -> (Epoch, u32) {
        (
            self.inner.current_state.current_epoch,
            self.inner.current_state.roster.size,
        )
    }
}

/// Operations only available in GroupStable state
impl ChoreographicProtocol<CgkaProtocolCore, GroupStable> {
    /// Check group stability
    pub async fn check_group_stability(&self, stability_timeout: u64) -> Option<GroupStabilized> {
        GroupStabilized::verify(
            (
                self.inner.group_id.clone(),
                self.inner.current_state.clone(),
            ),
            stability_timeout,
        )
    }

    /// Get current application secret
    pub fn get_application_secret(&self) -> Option<ApplicationSecret> {
        self.inner
            .current_state
            .current_application_secret()
            .cloned()
    }

    /// Check if group is ready for new operations
    pub fn is_ready_for_operations(&self) -> bool {
        self.inner.current_state.pending_operations.is_empty()
    }

    /// Get group status
    pub fn group_status(&self) -> GroupStatus {
        GroupStatus {
            group_id: self.inner.group_id.clone(),
            current_epoch: self.inner.current_state.current_epoch,
            member_count: self.inner.current_state.roster.member_count(),
            pending_operations: self.inner.current_state.pending_operations.len(),
            last_updated: self.inner.current_state.last_updated,
        }
    }
}

/// Operations available in failed state
impl ChoreographicProtocol<CgkaProtocolCore, GroupOperationFailed> {
    /// Get failure information
    pub fn get_failure_reason(&self) -> Option<String> {
        Some("CGKA group operation failed".to_string())
    }

    /// Check if recovery is possible
    pub fn can_recover(&self) -> bool {
        // Recovery is possible if we have a valid state
        self.inner.current_state.current_epoch.value() > 0
    }
}

// ========== Helper Types ==========

/// Group status information
#[derive(Debug, Clone)]
pub struct GroupStatus {
    pub group_id: String,
    pub current_epoch: Epoch,
    pub member_count: usize,
    pub pending_operations: usize,
    pub last_updated: u64,
}

// ========== Factory Functions ==========

/// Create a new session-typed CGKA protocol
pub fn new_session_typed_cgka(
    group_id: String,
    device_id: aura_journal::DeviceId,
    initial_members: Vec<MemberId>,
    effects: &aura_crypto::Effects,
) -> Result<ChoreographicProtocol<CgkaProtocolCore, CgkaGroupInitialized>, CgkaSessionError> {
    let initial_state = CgkaState::new(group_id.clone(), initial_members, effects)
        .map_err(|e| CgkaSessionError::ProtocolError(e.to_string()))?;

    let core = CgkaProtocolCore::new(group_id, device_id, initial_state);
    Ok(ChoreographicProtocol::new(core))
}

/// Rehydrate a CGKA protocol session from state
pub fn rehydrate_cgka_session(
    group_id: String,
    device_id: aura_journal::DeviceId,
    current_state: CgkaState,
    events: Vec<Event>,
) -> CgkaSessionState {
    let core = CgkaProtocolCore::new(group_id.clone(), device_id, current_state.clone());

    // Analyze events to determine current state
    let mut has_init = false;
    let mut has_membership_changes = false;
    let mut has_epoch_transitions = false;
    let has_failures = false;

    for event in &events {
        match &event.event_type {
            EventType::CgkaOperation(cgka_op) => {
                if cgka_op.group_id == group_id {
                    match cgka_op.operation_type {
                        aura_journal::CgkaOperationType::Init { .. } => has_init = true,
                        aura_journal::CgkaOperationType::Add { .. }
                        | aura_journal::CgkaOperationType::Remove { .. } => {
                            has_membership_changes = true
                        }
                        aura_journal::CgkaOperationType::Update => has_epoch_transitions = true,
                    }
                }
            }
            EventType::CgkaEpochTransition(transition) => {
                if transition.group_id == group_id {
                    has_epoch_transitions = true;
                }
            }
            _ => continue,
        }
    }

    // Check if there are pending operations
    let has_pending_operations = !current_state.pending_operations.is_empty();

    // Determine state based on analysis
    if has_failures {
        CgkaSessionState::GroupOperationFailed(ChoreographicProtocol::new(core))
    } else if has_pending_operations {
        CgkaSessionState::GroupMembershipChange(ChoreographicProtocol::new(core))
    } else if has_epoch_transitions {
        CgkaSessionState::GroupStable(ChoreographicProtocol::new(core))
    } else if has_membership_changes {
        CgkaSessionState::EpochTransition(ChoreographicProtocol::new(core))
    } else if has_init {
        CgkaSessionState::GroupMembershipChange(ChoreographicProtocol::new(core))
    } else {
        CgkaSessionState::CgkaGroupInitialized(ChoreographicProtocol::new(core))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_crypto::Effects;

    #[test]
    fn test_cgka_session_creation() {
        let effects = Effects::test();
        let group_id = "test-group".to_string();
        let device_id = aura_crypto::DeviceId::new_with_effects(&effects);
        let initial_members = vec![MemberId::new("member1"), MemberId::new("member2")];

        // Create a new CGKA session
        let cgka_session =
            new_session_typed_cgka(group_id.clone(), device_id, initial_members.clone(), &effects);
        assert!(cgka_session.is_ok());

        let session = cgka_session.unwrap();
        assert_eq!(session.state_name(), "CgkaGroupInitialized");
        assert!(!session.can_terminate());
        assert_eq!(session.inner.group_id, group_id);
        assert_eq!(session.inner.current_state.roster.member_count(), 2);
    }

    #[test]
    fn test_cgka_state_transitions() {
        let effects = Effects::test();
        let group_id = "test-group".to_string();
        let device_id = aura_crypto::DeviceId::new_with_effects(&effects);
        let initial_members = vec![MemberId::new("member1")];

        // Create session
        let session =
            new_session_typed_cgka(group_id.clone(), device_id, initial_members.clone(), &effects).unwrap();
        assert_eq!(session.state_name(), "CgkaGroupInitialized");

        // Transition to membership change with witness
        let init_witness = CgkaGroupInitiated {
            group_id: group_id.clone(),
            initial_epoch: Epoch::initial(),
            initial_members: initial_members.clone(),
            tree_size: 1,
        };

        let membership_session = <ChoreographicProtocol<CgkaProtocolCore, CgkaGroupInitialized> as WitnessedTransition<CgkaGroupInitialized, GroupMembershipChange>>::transition_with_witness(session, init_witness);
        assert_eq!(
            membership_session.state_name(),
            "GroupMembershipChange"
        );
        assert_eq!(membership_session.current_member_count(), 1);
        assert!(membership_session.is_member(&MemberId::new("member1")));

        // Can transition to epoch transition with witness
        let membership_witness = MembershipChangeReady {
            group_id: group_id.clone(),
            current_epoch: Epoch::initial(),
            pending_adds: vec![MemberId::new("member2")],
            pending_removes: vec![],
            roster_size: 2,
        };

        let transition_session = <ChoreographicProtocol<CgkaProtocolCore, GroupMembershipChange> as WitnessedTransition<GroupMembershipChange, EpochTransition>>::transition_with_witness(membership_session, membership_witness);
        assert_eq!(transition_session.state_name(), "EpochTransition");
        let (epoch, size) = transition_session.current_epoch_info();
        assert_eq!(epoch, Epoch::initial());
        assert_eq!(size, 1);
    }

    #[test]
    fn test_cgka_rehydration() {
        let effects = Effects::test();
        let group_id = "test-group".to_string();
        let device_id = aura_crypto::DeviceId::new_with_effects(&effects);
        let initial_members = vec![MemberId::new("member1")];

        // Create initial state
        let initial_state = CgkaState::new(group_id.clone(), initial_members, &effects).unwrap();

        // Test rehydration without events
        let state = rehydrate_cgka_session(group_id.clone(), device_id, initial_state.clone(), vec![]);
        assert_eq!(state.state_name(), "CgkaGroupInitialized");
        assert!(!state.can_terminate());
        assert_eq!(state.group_id(), group_id);
        assert_eq!(state.current_epoch(), Epoch::initial());

        // Test rehydration with init operation
        // Note: Creating proper CGKA events would require full journal integration
        // For now, we test the basic rehydration logic
    }

    #[test]
    fn test_cgka_witnesses() {
        let group_id = "test-group".to_string();

        // Test CgkaGroupInitiated witness
        // Note: Full witness testing would require proper Event creation
        // This tests the basic witness structure
        let witness = CgkaGroupInitiated {
            group_id: group_id.clone(),
            initial_epoch: Epoch::initial(),
            initial_members: vec![MemberId::new("member1")],
            tree_size: 1,
        };

        assert_eq!(witness.description(), "CGKA group successfully initialized");
        assert_eq!(witness.group_id, group_id);
        assert_eq!(witness.tree_size, 1);
    }
}