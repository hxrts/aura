//! Invitation cache manager.

use super::enrollment_window::EnrollmentExecutionRoot;
use super::state::with_state_mut_validated;
use crate::handlers::invitation::enrollment_manifest_admission::AdmittedEnrollmentManifest;
use crate::handlers::Invitation;
use aura_chat::{ChannelContextIndex, ChatFact};
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId, InvitationId};
use aura_relational::{ContactExistenceIndex, ContactFact};
use std::collections::{BTreeMap, HashMap};
use tokio::sync::RwLock;

const DEFAULT_INVITATION_CACHE_CAPACITY: usize = 1_000;

#[allow(dead_code)] // Declaration-layer ingress inventory; runtime actor wiring lands incrementally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InvitationManagerCommand {
    CacheInvitation,
    CacheAdmittedEnrollment,
    TakeAdmittedEnrollment,
    UpdateInvitation,
    RemoveInvitation,
    ReplaceContactIndex,
    ReplaceChannelContextIndex,
}

enum CachedInvitationEntry {
    Observed(Invitation),
    AdmittedEnrollment {
        invitation: Invitation,
        root: Box<EnrollmentExecutionRoot<AdmittedEnrollmentManifest>>,
    },
    RecoveredNegative {
        invitation: Invitation,
        admitted: std::sync::Arc<AdmittedEnrollmentManifest>,
        // Keeps actual required custody alive even when no external observer exists.
        _task: crate::task_registry::OriginalRequiredTaskHandle,
    },
    RequiredEnrollment {
        invitation: Invitation,
        admitted: std::sync::Arc<AdmittedEnrollmentManifest>,
        task: std::sync::Arc<tokio::sync::Mutex<crate::task_registry::OriginalRequiredTaskHandle>>,
    },
}

impl CachedInvitationEntry {
    fn reapable_after_original_ack(&self) -> bool {
        match self {
            Self::Observed(_) => true,
            Self::RequiredEnrollment { task, .. } if std::sync::Arc::strong_count(task) == 1 => {
                task.try_lock()
                    .is_ok_and(|original| original.acknowledged())
            }
            _ => false,
        }
    }
    fn invitation(&self) -> &Invitation {
        match self {
            Self::Observed(invitation)
            | Self::AdmittedEnrollment { invitation, .. }
            | Self::RecoveredNegative { invitation, .. }
            | Self::RequiredEnrollment { invitation, .. } => invitation,
        }
    }
    fn commit_update(&mut self, updated: Invitation) -> Result<(), aura_core::AuraError> {
        let original = match self {
            Self::AdmittedEnrollment { root, .. } => Some(root.origin().canonical_invitation()),
            Self::RecoveredNegative { admitted, .. } => Some(admitted.canonical_invitation()),
            Self::RequiredEnrollment { admitted, .. } => Some(admitted.canonical_invitation()),
            Self::Observed(_) => None,
        };
        if let Some(original) = original {
            // Exhaustive classification forces new invitation fields to choose
            // between immutable admission binding and mutable lifecycle state.
            let Invitation {
                invitation_id,
                context_id,
                sender_id,
                receiver_id,
                invitation_type,
                status: _,
                created_at,
                expires_at,
                message,
                receiver_nickname: _,
            } = &updated;
            if invitation_id != &original.invitation_id
                || context_id != &original.context_id
                || sender_id != &original.sender_id
                || receiver_id != &original.receiver_id
                || invitation_type != &original.invitation_type
                || created_at != &original.created_at
                || expires_at != &original.expires_at
                || message != &original.message
            {
                return Err(owner_refused(InvitationExecutionOwnerError::ForeignBinding));
            }
        }
        match self {
            Self::Observed(invitation)
            | Self::AdmittedEnrollment { invitation, .. }
            | Self::RecoveredNegative { invitation, .. }
            | Self::RequiredEnrollment { invitation, .. } => {
                *invitation = updated;
            }
        }
        Ok(())
    }
}

impl std::fmt::Debug for CachedInvitationEntry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CachedInvitationEntry")
            .field("invitation", self.invitation())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, thiserror::Error)]
enum InvitationExecutionOwnerError {
    #[error("cache update changes the original admitted invitation binding")]
    ForeignBinding,
    #[error("invitation has no retained original enrollment execution root")]
    Missing,
    #[error("invitation already retains its original enrollment execution root")]
    AlreadyRetained,
    #[error("original enrollment owner is busy before its window can be borrowed")]
    ObservationContended(#[source] tokio::sync::TryLockError),
}

fn owner_refused(source: InvitationExecutionOwnerError) -> aura_core::AuraError {
    aura_core::AuraError::PermissionDenied {
        message: "original invitation execution owner refused".into(),
        source: Some(std::sync::Arc::new(source)),
    }
}

#[derive(Debug)]
struct InvitationState {
    invitations: HashMap<InvitationId, CachedInvitationEntry>,
    invitation_access: HashMap<InvitationId, u64>,
    invitation_lru: BTreeMap<u64, InvitationId>,
    next_access_tick: u64,
    contact_index: ContactExistenceIndex,
    contact_index_seeded: bool,
    channel_context_index: ChannelContextIndex,
    channel_context_index_seeded: bool,
}

impl Default for InvitationState {
    fn default() -> Self {
        Self {
            invitations: HashMap::new(),
            invitation_access: HashMap::new(),
            invitation_lru: BTreeMap::new(),
            next_access_tick: 0,
            contact_index: ContactExistenceIndex::new(),
            contact_index_seeded: false,
            channel_context_index: ChannelContextIndex::new(),
            channel_context_index_seeded: false,
        }
    }
}

/// Manages cached invitations for the invitation handler.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OriginalInviteeStage {
    VerifiedRequestBeforeResponse,
    VerifiedRequestBeforeResponseWithCloseFault,
    SignedResponseCandidate,
    EligibleResponseBeforeClose,
    SignedNoticeCandidate,
    EligibleNoticeBeforeClose,
    EligibleNoticeBeforeCloseWithCloseFault,
}

#[cfg(test)]
struct OriginalInviteeStageGate {
    invitation: InvitationId,
    stage: OriginalInviteeStage,
    entered: tokio::sync::oneshot::Sender<()>,
    release: tokio::sync::oneshot::Receiver<()>,
    fault_task: Option<
        std::sync::Arc<
            crate::runtime::services::issuer_error_custody::SynchronousCustodyCell<
                Option<tokio::task::Id>,
            >,
        >,
    >,
}

#[aura_macros::actor_owned(
    owner = "invitation_manager",
    domain = "invitation_cache",
    gate = "invitation_cache_command_ingress",
    command = InvitationManagerCommand,
    capacity = 128,
    category = "actor_owned"
)]
pub struct InvitationManager {
    #[cfg(test)]
    #[allow(clippy::disallowed_types)]
    // The named fixture gate is removed under this lock before awaiting.
    original_invitee_stage_gate: parking_lot::Mutex<Option<OriginalInviteeStageGate>>,
    #[cfg(test)]
    #[allow(clippy::disallowed_types)]
    // Test gate transfer is synchronous and released before any await.
    recovered_notice_crash_gate:
        parking_lot::Mutex<Option<crate::handlers::invitation::RecoveredNoticeCrashGate>>,
    state: RwLock<InvitationState>,
    max_invitations: usize,
}

impl Default for InvitationManager {
    fn default() -> Self {
        Self::new()
    }
}

impl InvitationManager {
    /// Create a new invitation manager.
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_INVITATION_CACHE_CAPACITY)
    }

    /// Create a new invitation manager with an explicit cache bound.
    pub fn with_capacity(max_invitations: usize) -> Self {
        Self {
            #[cfg(test)]
            #[allow(clippy::disallowed_types)]
            original_invitee_stage_gate: parking_lot::Mutex::new(None),
            #[cfg(test)]
            #[allow(clippy::disallowed_types)] // Synchronous test gate slot declared above.
            recovered_notice_crash_gate: parking_lot::Mutex::new(None),
            state: RwLock::new(InvitationState::default()),
            max_invitations: max_invitations.max(1),
        }
    }

    fn touch_invitation(state: &mut InvitationState, invitation_id: &InvitationId) {
        state.next_access_tick = state.next_access_tick.saturating_add(1);
        let access_tick = state.next_access_tick;
        if let Some(previous_tick) = state
            .invitation_access
            .insert(invitation_id.clone(), access_tick)
        {
            state.invitation_lru.remove(&previous_tick);
        }
        state
            .invitation_lru
            .insert(access_tick, invitation_id.clone());
    }

    fn evict_excess_invitations(state: &mut InvitationState, max_invitations: usize) {
        while state.invitations.len() > max_invitations {
            let Some((oldest_tick, oldest_invitation_id)) = state
                .invitation_lru
                .iter()
                .find(|(_, id)| {
                    state
                        .invitations
                        .get(*id)
                        .is_some_and(CachedInvitationEntry::reapable_after_original_ack)
                })
                .map(|(tick, invitation_id)| (*tick, invitation_id.clone()))
            else {
                break;
            };
            state.invitation_lru.remove(&oldest_tick);
            state.invitation_access.remove(&oldest_invitation_id);
            state.invitations.remove(&oldest_invitation_id);
        }
    }

    /// Cache an invitation by ID.
    pub async fn cache_invitation(&self, invitation: Invitation) {
        let max_invitations = self.max_invitations;
        with_state_mut_validated(
            &self.state,
            |state| {
                match state.invitations.entry(invitation.invitation_id.clone()) {
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(CachedInvitationEntry::Observed(invitation.clone()));
                    }
                    std::collections::hash_map::Entry::Occupied(mut entry) => {
                        if matches!(entry.get(), CachedInvitationEntry::Observed(_)) {
                            entry.insert(CachedInvitationEntry::Observed(invitation.clone()));
                        }
                    }
                }
                Self::touch_invitation(state, &invitation.invitation_id);
                Self::evict_excess_invitations(state, max_invitations);
            },
            |_| Ok(()),
        )
        .await;
    }

    /// Get a cached invitation.
    pub async fn get_invitation(&self, invitation_id: &InvitationId) -> Option<Invitation> {
        with_state_mut_validated(
            &self.state,
            |state| {
                let invitation = state
                    .invitations
                    .get(invitation_id)
                    .map(|entry| entry.invitation().clone());
                if invitation.is_some() {
                    Self::touch_invitation(state, invitation_id);
                }
                invitation
            },
            |_| Ok(()),
        )
        .await
    }

    /// Update a cached invitation if present.
    pub async fn update_invitation<R>(
        &self,
        invitation_id: &InvitationId,
        f: impl FnOnce(&mut Invitation) -> R,
    ) -> Result<Option<R>, aura_core::AuraError> {
        with_state_mut_validated(
            &self.state,
            |state| {
                let result = if let Some(entry) = state.invitations.get_mut(invitation_id) {
                    // Mutate detached observations, then validate the original
                    // root binding before committing anything to actor state.
                    let mut updated = entry.invitation().clone();
                    let result = f(&mut updated);
                    entry.commit_update(updated)?;
                    Some(result)
                } else {
                    None
                };
                if result.is_some() {
                    Self::touch_invitation(state, invitation_id);
                }
                Ok(result)
            },
            |_| Ok(()),
        )
        .await
    }

    /// List cached invitations matching a predicate.
    pub async fn list_matching(&self, predicate: impl Fn(&Invitation) -> bool) -> Vec<Invitation> {
        self.state
            .read()
            .await
            .invitations
            .values()
            .map(CachedInvitationEntry::invitation)
            .filter(|inv| predicate(inv))
            .cloned()
            .collect()
    }

    /// Repeat transfer observes the retained original owner under its existing
    /// physical window. No serialized record or cached projection can mint it.
    pub(crate) async fn observe_admitted_enrollment(
        &self,
        effects: &crate::runtime::AuraEffectSystem,
        code: &str,
        pin: &aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentManifest,
    ) -> Result<Option<Invitation>, aura_core::AuraError> {
        // Borrowing the original window requires this actor's original entry.
        // Contention cannot authorize an unbounded wait or a fresh retry window.
        let state = self.state.try_read().map_err(|source| {
            owner_refused(InvitationExecutionOwnerError::ObservationContended(source))
        })?;
        let Some(CachedInvitationEntry::AdmittedEnrollment { invitation, root }) =
            state.invitations.get(&pin.manifest().invitation)
        else {
            return Ok(None);
        };
        if !root.origin().matches_original_transfer(code, pin) {
            return Err(owner_refused(InvitationExecutionOwnerError::ForeignBinding));
        }
        root.child()
            .remaining_ms(effects)
            .await
            .map_err(aura_core::AuraError::from)?;
        Ok(Some(invitation.clone()))
    }

    /// Retain the actual fresh root in the existing bounded canonical entry.
    /// Observational imports cannot replace this origin or duplicate its root.
    pub(crate) async fn cache_admitted_enrollment(
        &self,
        root: EnrollmentExecutionRoot<AdmittedEnrollmentManifest>,
    ) -> Result<(), aura_core::AuraError> {
        let invitation = root.origin().canonical_invitation().clone();
        let max_invitations = self.max_invitations;
        with_state_mut_validated(
            &self.state,
            |state| {
                let id = invitation.invitation_id.clone();
                if matches!(
                    state.invitations.get(&id),
                    Some(
                        CachedInvitationEntry::AdmittedEnrollment { .. }
                            | CachedInvitationEntry::RequiredEnrollment { .. }
                            | CachedInvitationEntry::RecoveredNegative { .. }
                    )
                ) {
                    return Err(owner_refused(
                        InvitationExecutionOwnerError::AlreadyRetained,
                    ));
                }
                state.invitations.insert(
                    id.clone(),
                    CachedInvitationEntry::AdmittedEnrollment {
                        invitation,
                        root: Box::new(root),
                    },
                );
                Self::touch_invitation(state, &id);
                Self::evict_excess_invitations(state, max_invitations);
                Ok(())
            },
            |_| Ok(()),
        )
        .await
    }

    #[cfg(test)]
    pub(crate) fn install_original_invitee_stage_gate_for_test(
        &self,
        invitation: InvitationId,
        stage: OriginalInviteeStage,
        fault_task: Option<
            std::sync::Arc<
                crate::runtime::services::issuer_error_custody::SynchronousCustodyCell<
                    Option<tokio::task::Id>,
                >,
            >,
        >,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (entered, observed) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        let mut slot = self.original_invitee_stage_gate.lock();
        assert!(
            slot.is_none(),
            "one original invitee stage gate per fixture"
        );
        *slot = Some(OriginalInviteeStageGate {
            invitation,
            stage,
            entered,
            release: released,
            fault_task,
        });
        (observed, release)
    }

    #[cfg(test)]
    pub(crate) fn original_invitee_stage_matches_for_test(
        &self,
        invitation: &InvitationId,
        stage: OriginalInviteeStage,
    ) -> bool {
        self.original_invitee_stage_gate
            .lock()
            .as_ref()
            .is_some_and(|gate| gate.invitation == *invitation && gate.stage == stage)
    }

    #[cfg(test)]
    pub(crate) async fn await_original_invitee_stage_gate_for_test(
        &self,
        invitation: &InvitationId,
        stage: OriginalInviteeStage,
    ) -> bool {
        let gate = {
            let mut slot = self.original_invitee_stage_gate.lock();
            if slot
                .as_ref()
                .is_some_and(|gate| gate.invitation == *invitation && gate.stage == stage)
            {
                slot.take()
            } else {
                None
            }
        };
        if let Some(gate) = gate {
            let _ = gate.entered.send(());
            let _ = gate.release.await;
            if let Some(target) = gate.fault_task {
                target.with(|target| *target = Some(tokio::task::id()));
            }
            true
        } else {
            false
        }
    }

    #[cfg(test)]
    pub(crate) async fn cancel_original_required_enrollment_for_test(
        &self,
        invitation: &InvitationId,
    ) -> Result<(), aura_core::AuraError> {
        use aura_core::effects::task::RequiredTaskControl;
        let task = {
            let state = self.state.read().await;
            let Some(CachedInvitationEntry::RequiredEnrollment { task, .. }) =
                state.invitations.get(invitation)
            else {
                return Err(owner_refused(InvitationExecutionOwnerError::Missing));
            };
            task.clone()
        };
        task.lock().await.request_cancel();
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn install_recovered_notice_crash_gate_for_test(
        &self,
        invitation: InvitationId,
        phase: crate::handlers::invitation::RecoveredNoticeCrashPhase,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (entered, observed) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        let mut slot = self.recovered_notice_crash_gate.lock();
        assert!(slot.is_none(), "one original test boundary");
        *slot = Some(crate::handlers::invitation::RecoveredNoticeCrashGate {
            invitation,
            phase,
            entered,
            release: released,
        });
        (observed, release)
    }
    pub(crate) async fn launch_recovered_negative(
        &self,
        effects: std::sync::Arc<crate::runtime::AuraEffectSystem>,
        admitted: std::sync::Arc<AdmittedEnrollmentManifest>,
        window: super::enrollment_window::RecoveredEnrollmentNoticeWindowCapability,
        tasks: &crate::task_registry::TaskGroup,
    ) -> Result<(), aura_core::AuraError> {
        window.require_admitted_owner(&admitted, effects.as_ref())?;
        let mut state = self.state.write().await;
        let invitation = admitted.canonical_invitation().clone();
        let id = invitation.invitation_id.clone();
        if matches!(
            state.invitations.get(&id),
            Some(
                CachedInvitationEntry::AdmittedEnrollment { .. }
                    | CachedInvitationEntry::RequiredEnrollment { .. }
                    | CachedInvitationEntry::RecoveredNegative { .. }
            )
        ) {
            return Err(owner_refused(
                InvitationExecutionOwnerError::AlreadyRetained,
            ));
        }
        if state.invitations.len() >= self.max_invitations && !state.invitations.contains_key(&id) {
            return Err(owner_refused(
                InvitationExecutionOwnerError::AlreadyRetained,
            ));
        }
        let task = crate::handlers::invitation::launch_recovered_enrollment_notice(
            effects,
            admitted.clone(),
            window,
            tasks,
            #[cfg(test)]
            self.recovered_notice_crash_gate.lock().take(),
        )?;
        // No suspension separates actual required admission from owner deposit.
        state.invitations.insert(
            id.clone(),
            CachedInvitationEntry::RecoveredNegative {
                invitation,
                admitted,
                _task: task,
            },
        );
        Self::touch_invitation(&mut state, &id);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) async fn recovered_negative_disposal(
        &self,
        invitation: &InvitationId,
    ) -> Result<crate::task_registry::OriginalRequiredTaskDisposalObserver, aura_core::AuraError>
    {
        let state = self.state.read().await;
        match state.invitations.get(invitation) {
            Some(CachedInvitationEntry::RecoveredNegative { _task: task, .. }) => {
                Ok(task.disposal_observer())
            }
            _ => Err(owner_refused(InvitationExecutionOwnerError::Missing)),
        }
    }

    /// Transfer the same retained root exactly once. Later canonical queries
    /// return observations; they cannot recreate execution after handoff/drop.
    pub(crate) async fn take_admitted_enrollment(
        &self,
        invitation_id: &InvitationId,
    ) -> Result<EnrollmentExecutionRoot<AdmittedEnrollmentManifest>, aura_core::AuraError> {
        with_state_mut_validated(
            &self.state,
            |state| {
                let entry = state
                    .invitations
                    .get_mut(invitation_id)
                    .ok_or_else(|| owner_refused(InvitationExecutionOwnerError::Missing))?;
                let observation = entry.invitation().clone();
                if matches!(
                    entry,
                    CachedInvitationEntry::RecoveredNegative { .. }
                        | CachedInvitationEntry::RequiredEnrollment { .. }
                ) {
                    return Err(owner_refused(
                        InvitationExecutionOwnerError::AlreadyRetained,
                    ));
                }
                match std::mem::replace(entry, CachedInvitationEntry::Observed(observation)) {
                    CachedInvitationEntry::AdmittedEnrollment { root, .. } => Ok(*root),
                    CachedInvitationEntry::Observed(_) => {
                        Err(owner_refused(InvitationExecutionOwnerError::Missing))
                    }
                    CachedInvitationEntry::RecoveredNegative { .. } => {
                        unreachable!("checked before transfer")
                    }
                    CachedInvitationEntry::RequiredEnrollment { .. } => {
                        unreachable!("checked before transfer")
                    }
                }
            },
            |_| Ok(()),
        )
        .await
    }

    /// Transfer the exact root into the caller's original required slot while
    /// holding the invitation owner lock. A rejected synchronous admission
    /// restores that same root before any cancellation point or observer sees
    /// an unowned `Observed` entry.
    #[cfg(test)]
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "original_required_invitee_task_custody",
        receiver_type = InvitationManager,
        family = "runtime_helper"
    )]
    pub(crate) async fn admit_admitted_enrollment<T, E, F>(
        &self,
        invitation_id: &InvitationId,
        admit: F,
    ) -> Result<T, E>
    where
        E: From<aura_core::AuraError>,
        F: FnOnce(
            EnrollmentExecutionRoot<AdmittedEnrollmentManifest>,
        ) -> Result<T, (EnrollmentExecutionRoot<AdmittedEnrollmentManifest>, E)>,
    {
        let mut state = self.state.write().await;
        let entry = state
            .invitations
            .get_mut(invitation_id)
            .ok_or_else(|| E::from(owner_refused(InvitationExecutionOwnerError::Missing)))?;
        if matches!(
            entry,
            CachedInvitationEntry::RecoveredNegative { .. }
                | CachedInvitationEntry::RequiredEnrollment { .. }
        ) {
            return Err(E::from(owner_refused(
                InvitationExecutionOwnerError::AlreadyRetained,
            )));
        }
        let observation = entry.invitation().clone();
        match std::mem::replace(entry, CachedInvitationEntry::Observed(observation)) {
            CachedInvitationEntry::AdmittedEnrollment { invitation, root } => match admit(*root) {
                Ok(admitted) => Ok(admitted),
                Err((root, source)) => {
                    *entry = CachedInvitationEntry::AdmittedEnrollment {
                        invitation,
                        root: Box::new(root),
                    };
                    Err(source)
                }
            },
            CachedInvitationEntry::Observed(_) => Err(E::from(owner_refused(
                InvitationExecutionOwnerError::Missing,
            ))),
            CachedInvitationEntry::RecoveredNegative { .. } => {
                unreachable!("recovered negative was refused before transfer")
            }
            CachedInvitationEntry::RequiredEnrollment { .. } => {
                unreachable!("required enrollment was refused before transfer")
            }
        }
    }

    /// Admit the original required slot and deposit its only reconciliation
    /// control in this manager before releasing the original root lock.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "original_required_invitee_task_custody",
        receiver_type = InvitationManager,
        family = "runtime_helper"
    )]
    pub(crate) async fn admit_required_enrollment<E, F>(
        &self,
        invitation_id: &InvitationId,
        admit: F,
    ) -> Result<(), E>
    where
        E: From<aura_core::AuraError>,
        F: FnOnce(
            EnrollmentExecutionRoot<AdmittedEnrollmentManifest>,
        ) -> Result<
            crate::task_registry::OriginalRequiredTaskHandle,
            (EnrollmentExecutionRoot<AdmittedEnrollmentManifest>, E),
        >,
    {
        with_state_mut_validated(
            &self.state,
            |state| {
                let entry = state.invitations.get_mut(invitation_id).ok_or_else(|| {
                    E::from(owner_refused(InvitationExecutionOwnerError::Missing))
                })?;
                if !matches!(entry, CachedInvitationEntry::AdmittedEnrollment { .. }) {
                    return Err(E::from(owner_refused(
                        InvitationExecutionOwnerError::AlreadyRetained,
                    )));
                }
                let observation = entry.invitation().clone();
                let CachedInvitationEntry::AdmittedEnrollment { invitation, root } =
                    std::mem::replace(entry, CachedInvitationEntry::Observed(observation))
                else {
                    unreachable!("checked before transfer")
                };
                let admitted = root.origin_observation();
                match admit(*root) {
                    Ok(task) => {
                        *entry = CachedInvitationEntry::RequiredEnrollment {
                            invitation,
                            admitted,
                            task: std::sync::Arc::new(tokio::sync::Mutex::new(task)),
                        };
                        Ok(())
                    }
                    Err((root, source)) => {
                        *entry = CachedInvitationEntry::AdmittedEnrollment {
                            invitation,
                            root: Box::new(root),
                        };
                        Err(source)
                    }
                }
            },
            |_| Ok(()),
        )
        .await
    }

    pub(crate) async fn await_required_enrollment_cleanup(
        &self,
        invitation_id: &InvitationId,
    ) -> Result<(), aura_core::AuraError> {
        let task = {
            let state = self.state.read().await;
            let Some(CachedInvitationEntry::RequiredEnrollment { task, .. }) =
                state.invitations.get(invitation_id)
            else {
                return Err(owner_refused(InvitationExecutionOwnerError::Missing));
            };
            task.clone()
        };
        task.lock().await.await_original_cleanup_decision().await?;
        // The retained control is removed only after the original required
        // slot acknowledges cleanup. A canceled waiter never reaches this
        // transition and leaves the manager-held control retryable.
        self.release_acknowledged_required_enrollment(invitation_id, &task)
            .await
    }

    async fn release_acknowledged_required_enrollment(
        &self,
        invitation_id: &InvitationId,
        expected: &std::sync::Arc<
            tokio::sync::Mutex<crate::task_registry::OriginalRequiredTaskHandle>,
        >,
    ) -> Result<(), aura_core::AuraError> {
        with_state_mut_validated(
            &self.state,
            |state| {
                let entry = state
                    .invitations
                    .get_mut(invitation_id)
                    .ok_or_else(|| owner_refused(InvitationExecutionOwnerError::Missing))?;
                let CachedInvitationEntry::RequiredEnrollment {
                    invitation, task, ..
                } = entry
                else {
                    return Err(owner_refused(InvitationExecutionOwnerError::Missing));
                };
                if !std::sync::Arc::ptr_eq(task, expected) {
                    return Err(owner_refused(InvitationExecutionOwnerError::ForeignBinding));
                }
                if !task
                    .try_lock()
                    .map_err(|_| owner_refused(InvitationExecutionOwnerError::AlreadyRetained))?
                    .acknowledged()
                {
                    return Err(owner_refused(
                        InvitationExecutionOwnerError::AlreadyRetained,
                    ));
                }
                *entry = CachedInvitationEntry::Observed(invitation.clone());
                Ok(())
            },
            |_| Ok(()),
        )
        .await
    }

    /// Retry cleanup through the same original admitted task slot. A failed
    /// cleanup leaves the control and required custody in their original owners.
    #[cfg(test)]
    pub(crate) async fn reconcile_required_enrollment_cleanup(
        &self,
        invitation_id: &InvitationId,
    ) -> Result<(), aura_core::AuraError> {
        use aura_core::effects::task::RequiredTaskControl;
        let task = {
            let state = self.state.read().await;
            let Some(CachedInvitationEntry::RequiredEnrollment { task, .. }) =
                state.invitations.get(invitation_id)
            else {
                return Err(owner_refused(InvitationExecutionOwnerError::Missing));
            };
            task.clone()
        };
        {
            let mut original = task.lock().await;
            #[cfg(target_arch = "wasm32")]
            original.reconcile_local().await?;
            #[cfg(not(target_arch = "wasm32"))]
            original.reconcile().await?;
        }
        self.release_acknowledged_required_enrollment(invitation_id, &task)
            .await
    }

    pub(crate) async fn original_required_enrollment_exit_after_refusal(
        &self,
        invitation_id: &InvitationId,
    ) -> Result<aura_core::effects::task::RequiredTaskExit, aura_core::AuraError> {
        let task = {
            let state = self.state.read().await;
            let Some(CachedInvitationEntry::RequiredEnrollment { task, .. }) =
                state.invitations.get(invitation_id)
            else {
                return Err(owner_refused(InvitationExecutionOwnerError::Missing));
            };
            task.clone()
        };
        let exit = task.lock().await.original_exit_after_cleanup_refusal();
        exit
    }

    pub async fn contact_index_seeded(&self) -> bool {
        self.state.read().await.contact_index_seeded
    }

    pub async fn replace_contact_index(&self, index: ContactExistenceIndex) {
        with_state_mut_validated(
            &self.state,
            |state| {
                state.contact_index = index;
                state.contact_index_seeded = true;
            },
            |_| Ok(()),
        )
        .await;
    }

    pub async fn record_contact_fact(&self, fact: &ContactFact) {
        with_state_mut_validated(
            &self.state,
            |state| state.contact_index.apply_fact(fact),
            |_| Ok(()),
        )
        .await;
    }

    pub async fn contact_exists(&self, owner_id: AuthorityId, contact_id: AuthorityId) -> bool {
        self.state
            .read()
            .await
            .contact_index
            .contains(owner_id, contact_id)
    }

    pub async fn channel_context_index_seeded(&self) -> bool {
        self.state.read().await.channel_context_index_seeded
    }

    pub async fn replace_channel_context_index(&self, index: ChannelContextIndex) {
        with_state_mut_validated(
            &self.state,
            |state| {
                state.channel_context_index = index;
                state.channel_context_index_seeded = true;
            },
            |_| Ok(()),
        )
        .await;
    }

    pub async fn record_chat_fact(&self, fact: &ChatFact) {
        with_state_mut_validated(
            &self.state,
            |state| state.channel_context_index.apply_fact(fact),
            |_| Ok(()),
        )
        .await;
    }

    pub async fn channel_context(
        &self,
        channel_id: ChannelId,
        creator_id: AuthorityId,
    ) -> Option<ContextId> {
        self.state
            .read()
            .await
            .channel_context_index
            .context_for_channel(channel_id, creator_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::default_context_id_for_authority;
    use aura_core::types::identifiers::AuthorityId;
    use std::sync::Arc;

    fn invitation(seed: u8) -> Invitation {
        let sender_id = AuthorityId::new_from_entropy([seed; 32]);
        let receiver_id = AuthorityId::new_from_entropy([seed.wrapping_add(1); 32]);
        Invitation {
            invitation_id: InvitationId::new(format!("invitation-{seed}")),
            context_id: default_context_id_for_authority(sender_id),
            sender_id,
            receiver_id,
            invitation_type: crate::handlers::InvitationType::Contact { nickname: None },
            status: crate::handlers::InvitationStatus::Pending,
            created_at: u64::from(seed),
            expires_at: None,
            receiver_nickname: None,
            message: None,
        }
    }

    #[tokio::test]
    async fn invitation_cache_respects_capacity_bound() {
        let manager = InvitationManager::with_capacity(2);

        manager.cache_invitation(invitation(1)).await;
        manager.cache_invitation(invitation(2)).await;
        manager.cache_invitation(invitation(3)).await;

        let cached = manager.list_matching(|_| true).await;
        assert_eq!(cached.len(), 2);
        assert!(manager
            .get_invitation(&InvitationId::new("invitation-1"))
            .await
            .is_none());
        assert!(manager
            .get_invitation(&InvitationId::new("invitation-2"))
            .await
            .is_some());
        assert!(manager
            .get_invitation(&InvitationId::new("invitation-3"))
            .await
            .is_some());
    }

    #[test]
    fn rejected_original_slot_admission_restores_same_root_before_error_drop() {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async {
            let (_issuer, invitee, admitted_invitation, _, _, _) = Box::pin(
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                    "manager-rejected-original-slot",
                ),
            )
            .await;
            let root = invitee
                .invitations()
                .unwrap()
                .take_original_enrollment_execution(&admitted_invitation.invitation_id)
                .await
                .unwrap();
            let original = root.origin_observation();
            let manager = InvitationManager::with_capacity(2);
            manager.cache_admitted_enrollment(root).await.unwrap();
            let refused = manager
                .admit_admitted_enrollment(&admitted_invitation.invitation_id, |root| {
                    Err::<(), _>((
                        root,
                        aura_core::AuraError::invalid("original task admission refused"),
                    ))
                })
                .await
                .expect_err("original admission refusal must restore custody");
            drop(refused);
            let retained = manager
                .take_admitted_enrollment(&admitted_invitation.invitation_id)
                .await
                .expect("same original root remains retryable after refusal error drop");
            assert!(Arc::ptr_eq(&original, &retained.origin_observation()));
        });
    }

    #[test]
    fn rejected_required_registration_rolls_back_original_invitee_root() {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async {
            use aura_core::effects::task::{
                RequiredTaskCustody, RequiredTaskExit, RequiredTaskSettlement,
            };
            struct UnpolledRoot(EnrollmentExecutionRoot<AdmittedEnrollmentManifest>);
            impl RequiredTaskCustody for UnpolledRoot {
                fn execute(
                    &mut self,
                ) -> futures::future::BoxFuture<'_, Result<(), aura_core::AuraError>>
                {
                    panic!("rejected original invitee task must never execute")
                }
                fn cleanup<'a>(
                    &'a mut self,
                    _: &'a RequiredTaskExit,
                ) -> futures::future::BoxFuture<
                    'a,
                    Result<RequiredTaskSettlement, aura_core::AuraError>,
                > {
                    panic!("rejected original invitee task has no cleanup registration")
                }
            }
            let (_issuer, invitee, invitation, _, _, _) = Box::pin(
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                    "manager-actual-rejected-original-slot",
                ),
            )
            .await;
            let root = invitee
                .invitations()
                .unwrap()
                .take_original_enrollment_execution(&invitation.invitation_id)
                .await
                .unwrap();
            let original = root.origin_observation();
            let manager = InvitationManager::with_capacity(2);
            manager.cache_admitted_enrollment(root).await.unwrap();
            let supervisor = crate::runtime::TaskSupervisor::new();
            let group = supervisor.group("rejected-original-invitee-registration");
            let seal = group.seal_admission();
            let refused = manager
                .admit_admitted_enrollment(&invitation.invitation_id, |root| {
                    let rejected = group
                        .spawn_original_required_custody_typed(
                            "invitee".into(),
                            Box::new(UnpolledRoot(root)),
                            None,
                        )
                        .err()
                        .expect("sealed group refuses actual registration");
                    let UnpolledRoot(root) = *rejected.obligation;
                    Err::<(), _>((root, rejected.source))
                })
                .await
                .expect_err("original required slot admission was refused");
            drop(refused);
            let retained = manager
                .take_admitted_enrollment(&invitation.invitation_id)
                .await
                .expect("dropping refusal preserves exact original manager root");
            assert!(Arc::ptr_eq(&original, &retained.origin_observation()));
            drop(seal);
        });
    }

    #[test]
    fn dropped_original_result_waiter_retains_same_reconciliation_control() {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async {
            use aura_core::effects::task::{
                RequiredTaskCustody, RequiredTaskExit, RequiredTaskSettlement,
            };
            use std::sync::atomic::{AtomicUsize, Ordering};

            struct OriginalInviteeProbe {
                _root: EnrollmentExecutionRoot<AdmittedEnrollmentManifest>,
                cleanup_attempts: Arc<AtomicUsize>,
                cleanup_started: Option<tokio::sync::oneshot::Sender<()>>,
                cleanup_release: Option<tokio::sync::oneshot::Receiver<()>>,
                retry_started: Option<tokio::sync::oneshot::Sender<()>>,
                retry_release: Option<tokio::sync::oneshot::Receiver<()>>,
            }
            impl RequiredTaskCustody for OriginalInviteeProbe {
                fn execute(
                    &mut self,
                ) -> futures::future::BoxFuture<'_, Result<(), aura_core::AuraError>>
                {
                    Box::pin(async { Ok(()) })
                }
                fn cleanup<'a>(
                    &'a mut self,
                    _: &'a RequiredTaskExit,
                ) -> futures::future::BoxFuture<
                    'a,
                    Result<RequiredTaskSettlement, aura_core::AuraError>,
                > {
                    Box::pin(async move {
                        let attempt = self.cleanup_attempts.fetch_add(1, Ordering::SeqCst);
                        if attempt == 0 {
                            if let Some(started) = self.cleanup_started.take() {
                                let _ = started.send(());
                            }
                            if let Some(release) = self.cleanup_release.take() {
                                release
                                    .await
                                    .map_err(|source| aura_core::AuraError::Internal {
                                        message: "original cleanup release sender was lost".into(),
                                        source: Some(Arc::new(source)),
                                    })?;
                            }
                            return Err(aura_core::AuraError::invalid(
                                "first original invitee cleanup failed",
                            ));
                        }
                        if attempt == 1 {
                            if let Some(started) = self.retry_started.take() {
                                let _ = started.send(());
                            }
                            if let Some(release) = self.retry_release.take() {
                                let _ = release.await;
                            }
                        }
                        Ok(RequiredTaskSettlement::Resolved)
                    })
                }
            }

            let (_issuer, invitee, invitation, _, _, _) = Box::pin(
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                    "manager-original-invitee-reconcile-after-waiter-drop",
                ),
            )
            .await;
            let root = invitee
                .invitations()
                .unwrap()
                .take_original_enrollment_execution(&invitation.invitation_id)
                .await
                .unwrap();
            let manager = InvitationManager::with_capacity(2);
            manager.cache_admitted_enrollment(root).await.unwrap();
            let supervisor = crate::runtime::TaskSupervisor::new();
            let group = supervisor.group("original-invitee-manager-reconcile");
            let attempts = Arc::new(AtomicUsize::new(0));
            let (cleanup_started, started) = tokio::sync::oneshot::channel();
            let (release_cleanup, cleanup_release) = tokio::sync::oneshot::channel();
            let (retry_started, retry_entered) = tokio::sync::oneshot::channel();
            let (_release_retry, retry_release) = tokio::sync::oneshot::channel();
            manager
                .admit_required_enrollment(&invitation.invitation_id, |root| {
                    group
                        .spawn_original_required_custody_typed(
                            "invitee".into(),
                            Box::new(OriginalInviteeProbe {
                                _root: root,
                                cleanup_attempts: attempts.clone(),
                                cleanup_started: Some(cleanup_started),
                                cleanup_release: Some(cleanup_release),
                                retry_started: Some(retry_started),
                                retry_release: Some(retry_release),
                            }),
                            None,
                        )
                        .map_err(|rejected| {
                            let OriginalInviteeProbe { _root, .. } = *rejected.obligation;
                            (_root, rejected.source)
                        })
                })
                .await
                .expect("original required slot admitted under manager lock");
            drop(group);
            started
                .await
                .expect("actual original cleanup entered its first attempt");
            let mut waiter =
                Box::pin(manager.await_required_enrollment_cleanup(&invitation.invitation_id));
            assert!(
                futures::poll!(waiter.as_mut()).is_pending(),
                "waiter observes actual pending cleanup"
            );
            drop(waiter);
            release_cleanup
                .send(())
                .expect("original cleanup still owns release receiver");
            let first = manager
                .await_required_enrollment_cleanup(&invitation.invitation_id)
                .await
                .expect_err("actual first cleanup failure remains typed");
            assert!(matches!(
                first,
                aura_core::AuraError::Invalid { message, .. }
                    if message == "first original invitee cleanup failed"
            ));
            assert!(matches!(
                manager
                    .state
                    .read()
                    .await
                    .invitations
                    .get(&invitation.invitation_id),
                Some(CachedInvitationEntry::RequiredEnrollment { .. })
            ));
            let mut interrupted =
                Box::pin(manager.reconcile_required_enrollment_cleanup(&invitation.invitation_id));
            assert!(futures::poll!(interrupted.as_mut()).is_pending());
            retry_entered
                .await
                .expect("same original cleanup entered retry");
            drop(interrupted);
            assert!(matches!(
                manager
                    .state
                    .read()
                    .await
                    .invitations
                    .get(&invitation.invitation_id),
                Some(CachedInvitationEntry::RequiredEnrollment { .. })
            ));
            manager
                .reconcile_required_enrollment_cleanup(&invitation.invitation_id)
                .await
                .expect("same manager-held original control settles after canceled retry waiter");
            assert_eq!(attempts.load(Ordering::SeqCst), 3);
            assert!(matches!(
                manager
                    .state
                    .read()
                    .await
                    .invitations
                    .get(&invitation.invitation_id),
                Some(CachedInvitationEntry::Observed(_))
            ));
        });
    }

    #[test]
    fn acknowledged_required_slot_without_caller_observer_releases_bounded_cache_capacity() {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async {
            use aura_core::effects::task::{
                RequiredTaskCustody, RequiredTaskExit, RequiredTaskSettlement,
            };
            struct SettledOriginalRoot(EnrollmentExecutionRoot<AdmittedEnrollmentManifest>);
            impl RequiredTaskCustody for SettledOriginalRoot {
                fn execute(
                    &mut self,
                ) -> futures::future::BoxFuture<'_, Result<(), aura_core::AuraError>>
                {
                    Box::pin(async { Ok(()) })
                }
                fn cleanup<'a>(
                    &'a mut self,
                    _: &'a RequiredTaskExit,
                ) -> futures::future::BoxFuture<
                    'a,
                    Result<RequiredTaskSettlement, aura_core::AuraError>,
                > {
                    Box::pin(async { Ok(RequiredTaskSettlement::Resolved) })
                }
            }
            let (_issuer, invitee, admitted_invitation, _, _, _) = Box::pin(
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                    "manager-canceled-success-reaps-only-after-original-ack",
                ),
            )
            .await;
            let root = invitee
                .invitations()
                .unwrap()
                .take_original_enrollment_execution(&admitted_invitation.invitation_id)
                .await
                .unwrap();
            let manager = InvitationManager::with_capacity(1);
            manager.cache_admitted_enrollment(root).await.unwrap();
            let supervisor = crate::runtime::TaskSupervisor::new();
            let group = supervisor.group("manager-canceled-success");
            manager
                .admit_required_enrollment(&admitted_invitation.invitation_id, |root| {
                    group
                        .spawn_original_required_custody_typed(
                            "invitee".into(),
                            Box::new(SettledOriginalRoot(root)),
                            None,
                        )
                        .map_err(|rejected| {
                            let SettledOriginalRoot(root) = *rejected.obligation;
                            (root, rejected.source)
                        })
                })
                .await
                .expect("exact original slot admitted");
            let waiter =
                manager.await_required_enrollment_cleanup(&admitted_invitation.invitation_id);
            drop(waiter);
            let original = {
                let state = manager.state.read().await;
                let Some(CachedInvitationEntry::RequiredEnrollment { task, .. }) =
                    state.invitations.get(&admitted_invitation.invitation_id)
                else {
                    panic!("original control retained")
                };
                task.clone()
            };
            original
                .lock()
                .await
                .await_original_cleanup_decision()
                .await
                .expect("actual cleanup ACK");
            drop(original);
            manager.cache_invitation(invitation(91)).await;
            assert!(
                manager
                    .get_invitation(&admitted_invitation.invitation_id)
                    .await
                    .is_none(),
                "ACKed original slot with no waiter is eligible for bounded eviction"
            );
            assert!(manager
                .get_invitation(&InvitationId::new("invitation-91"))
                .await
                .is_some());
        });
    }

    #[test]
    fn cancelled_original_execution_without_result_retains_exit_and_cleanup_refusal() {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async {
            use aura_core::effects::task::{
                RequiredTaskControl, RequiredTaskCustody, RequiredTaskExit, RequiredTaskSettlement,
            };
            struct PendingOriginalRoot {
                _root: EnrollmentExecutionRoot<AdmittedEnrollmentManifest>,
                started: Option<tokio::sync::oneshot::Sender<()>>,
                _unpublished_reply: tokio::sync::oneshot::Sender<()>,
            }
            impl RequiredTaskCustody for PendingOriginalRoot {
                fn execute(
                    &mut self,
                ) -> futures::future::BoxFuture<'_, Result<(), aura_core::AuraError>>
                {
                    Box::pin(async move {
                        if let Some(started) = self.started.take() {
                            let _ = started.send(());
                        }
                        futures::future::pending::<()>().await;
                        unreachable!("cancelled original execution cannot publish result")
                    })
                }
                fn cleanup<'a>(
                    &'a mut self,
                    _: &'a RequiredTaskExit,
                ) -> futures::future::BoxFuture<
                    'a,
                    Result<RequiredTaskSettlement, aura_core::AuraError>,
                > {
                    Box::pin(async {
                        Err(aura_core::AuraError::invalid(
                            "original cancelled cleanup refused",
                        ))
                    })
                }
            }
            let (_issuer, invitee, invitation, _, _, _) = Box::pin(
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                    "manager-cancel-before-original-public-reply",
                ),
            )
            .await;
            let root = invitee
                .invitations()
                .unwrap()
                .take_original_enrollment_execution(&invitation.invitation_id)
                .await
                .unwrap();
            let manager = InvitationManager::with_capacity(1);
            manager.cache_admitted_enrollment(root).await.unwrap();
            let supervisor = crate::runtime::TaskSupervisor::new();
            let group = supervisor.group("original-invitee-cancel-before-reply");
            let (started_tx, started) = tokio::sync::oneshot::channel();
            let (reply_tx, mut reply) = tokio::sync::oneshot::channel();
            manager
                .admit_required_enrollment(&invitation.invitation_id, |root| {
                    group
                        .spawn_original_required_custody_typed(
                            "invitee".into(),
                            Box::new(PendingOriginalRoot {
                                _root: root,
                                started: Some(started_tx),
                                _unpublished_reply: reply_tx,
                            }),
                            None,
                        )
                        .map_err(|rejected| {
                            let PendingOriginalRoot { _root, .. } = *rejected.obligation;
                            (_root, rejected.source)
                        })
                })
                .await
                .expect("real original task slot admitted");
            started
                .await
                .expect("actual execution entered before cancellation");
            let task = {
                let state = manager.state.read().await;
                let Some(CachedInvitationEntry::RequiredEnrollment { task, .. }) =
                    state.invitations.get(&invitation.invitation_id)
                else {
                    panic!("original handle remains manager-owned")
                };
                task.clone()
            };
            task.lock().await.request_cancel();
            drop(task);
            let cleanup = manager
                .await_required_enrollment_cleanup(&invitation.invitation_id)
                .await
                .expect_err("actual cancelled cleanup refusal remains retained");
            assert!(
                matches!(cleanup, aura_core::AuraError::Invalid { message, .. } if message == "original cancelled cleanup refused")
            );
            assert!(
                matches!(
                    reply.try_recv(),
                    Err(tokio::sync::oneshot::error::TryRecvError::Empty)
                ),
                "original owner still retains unpublished reply; awaiting it would hang"
            );
            assert!(matches!(
                manager
                    .original_required_enrollment_exit_after_refusal(&invitation.invitation_id)
                    .await
                    .unwrap(),
                RequiredTaskExit::Cancelled
            ));
            assert!(
                matches!(
                    manager
                        .state
                        .read()
                        .await
                        .invitations
                        .get(&invitation.invitation_id),
                    Some(CachedInvitationEntry::RequiredEnrollment { .. })
                ),
                "failed cleanup cannot release original root or control"
            );
        });
    }

    #[test]
    fn canceled_original_slot_waiter_cannot_take_root_before_manager_lock() {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async {
            let (_issuer, invitee, invitation, _, _, _) = Box::pin(
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                    "manager-canceled-original-slot",
                ),
            )
            .await;
            let root = invitee
                .invitations()
                .unwrap()
                .take_original_enrollment_execution(&invitation.invitation_id)
                .await
                .unwrap();
            let original = root.origin_observation();
            let manager = InvitationManager::with_capacity(2);
            manager.cache_admitted_enrollment(root).await.unwrap();
            let held = manager.state.write().await;
            let mut pending = Box::pin(manager.admit_admitted_enrollment(
                &invitation.invitation_id,
                |root| {
                    Ok::<
                        _,
                        (
                            EnrollmentExecutionRoot<AdmittedEnrollmentManifest>,
                            aura_core::AuraError,
                        ),
                    >(root)
                },
            ));
            assert!(futures::poll!(pending.as_mut()).is_pending());
            drop(pending);
            drop(held);
            let retained = manager
                .take_admitted_enrollment(&invitation.invitation_id)
                .await
                .expect("canceled waiter never removed original root");
            assert!(Arc::ptr_eq(&original, &retained.origin_observation()));
        });
    }

    #[test]
    fn admitted_generic_update_cannot_replace_original_binding() {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async {
            let (_issuer, invitee, original, _, _, _) = Box::pin(
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                    "manager-original-binding",
                ),
            )
            .await;
            let root = invitee
                .invitations()
                .unwrap()
                .take_original_enrollment_execution(&original.invitation_id)
                .await
                .unwrap();
            let original_digest = root.origin().manifest_digest();
            let manager = InvitationManager::with_capacity(2);
            manager.cache_admitted_enrollment(root).await.unwrap();
            for field in 0..8 {
                let result = manager
                    .update_invitation(&original.invitation_id, |candidate| {
                        // Exercise the actual generic mutation closure, including
                        // fields that could redirect the retained physical owner.
                        match field {
                            0 => candidate.invitation_id = InvitationId::new("foreign-original"),
                            1 => {
                                candidate.context_id =
                                    aura_core::ContextId::new_from_entropy([91; 32]);
                            }
                            2 => candidate.sender_id = AuthorityId::new_from_entropy([92; 32]),
                            3 => candidate.receiver_id = AuthorityId::new_from_entropy([93; 32]),
                            4 => {
                                candidate.invitation_type =
                                    crate::handlers::InvitationType::Contact { nickname: None }
                            }
                            5 => candidate.created_at = candidate.created_at.saturating_add(1),
                            6 => candidate.expires_at = None,
                            7 => candidate.message = Some("foreign binding".into()),
                            _ => unreachable!(),
                        }
                    })
                    .await;
                assert!(matches!(
                    result,
                    Err(aura_core::AuraError::PermissionDenied { .. })
                ));
                let retained = manager
                    .get_invitation(&original.invitation_id)
                    .await
                    .unwrap();
                assert_eq!(
                    aura_core::util::serialization::to_vec(&retained).unwrap(),
                    aura_core::util::serialization::to_vec(&original).unwrap(),
                    "refusal must leave the canonical entry unchanged for field {field}",
                );
            }
            manager
                .update_invitation(&original.invitation_id, |candidate| {
                    candidate.receiver_nickname = Some("local nickname".into());
                    candidate.status = crate::handlers::InvitationStatus::Accepted;
                })
                .await
                .unwrap();
            let mut foreign_refresh = original.clone();
            foreign_refresh.invitation_type =
                crate::handlers::InvitationType::Contact { nickname: None };
            foreign_refresh.context_id = aura_core::ContextId::new_from_entropy([94; 32]);
            manager.cache_invitation(foreign_refresh).await;
            let observed = manager
                .get_invitation(&original.invitation_id)
                .await
                .unwrap();
            assert_eq!(observed.context_id, original.context_id);
            assert_eq!(observed.invitation_type, original.invitation_type);
            assert_eq!(observed.status, crate::handlers::InvitationStatus::Accepted);
            assert_eq!(
                observed.receiver_nickname.as_deref(),
                Some("local nickname")
            );
            let retained_root = manager
                .take_admitted_enrollment(&original.invitation_id)
                .await
                .unwrap();
            assert_eq!(retained_root.origin().manifest_digest(), original_digest);
            assert_eq!(
                retained_root
                    .origin()
                    .canonical_invitation()
                    .receiver_nickname,
                original.receiver_nickname
            );
            assert_eq!(
                retained_root.origin().canonical_invitation().status,
                original.status
            );
            assert!(manager
                .take_admitted_enrollment(&original.invitation_id)
                .await
                .is_err());
        });
    }

    #[test]
    fn repeated_transfer_keeps_original_owner_and_rejects_foreign_code_before_clock() {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async {
            use aura_core::effects::{PhysicalTimeEffects, StorageCoreEffects, TimeError};
            use futures::FutureExt;
            let clock = std::sync::Arc::new(aura_testkit::time::ManualPhysicalClock::new(
                1_700_000_000_000,
            ));
            let (_issuer, invitee, original, start, _, _) = Box::pin(
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture_with_clock(
                    "manager-repeat-original-owner",
                    clock.clone(),
                ),
            )
            .await;
            let app = std::sync::Arc::new(async_lock::RwLock::new(
                aura_app::AppCore::with_runtime(
                    aura_app::AppConfig::default(),
                    std::sync::Arc::new(crate::runtime_bridge::AgentRuntimeBridge::new(
                        invitee.clone(),
                    )),
                )
                .unwrap(),
            ));
            let transfer = start.manifest_transfer.as_ref().unwrap();
            let pin =
                aura_app::ui::workflows::ceremonies::pin_user_transferred_enrollment_manifest(
                    &app,
                    transfer.manifest_code.clone(),
                    transfer.initiator_verifier_code.clone(),
                )
                .await
                .unwrap();
            let service = invitee.invitations().unwrap();
            let root = service
                .take_original_enrollment_execution(&original.invitation_id)
                .await
                .unwrap();
            let effects = invitee.runtime().effects();
            let remaining = root.child().remaining_ms(effects.as_ref()).await.unwrap();
            let manager = InvitationManager::with_capacity(1);
            manager.cache_admitted_enrollment(root).await.unwrap();
            let keys = effects.list_keys(None).await.unwrap();
            let mut before_storage = Vec::new();
            for key in &keys {
                before_storage.push((key.clone(), effects.retrieve(key).await.unwrap()));
            }
            let held_write = manager.state.write().await;
            let before_invitation = aura_core::util::serialization::to_vec(
                held_write
                    .invitations
                    .get(&original.invitation_id)
                    .unwrap()
                    .invitation(),
            )
            .unwrap();
            let refusal = manager
                .observe_admitted_enrollment(effects.as_ref(), &start.enrollment_code, &pin)
                .now_or_never()
                .expect("held actor lock must refuse on its first poll")
                .expect_err("actor contention cannot masquerade as a missing original owner");
            let owner_source = std::error::Error::source(&refusal).unwrap();
            assert!(matches!(
                owner_source.downcast_ref::<InvitationExecutionOwnerError>(),
                Some(InvitationExecutionOwnerError::ObservationContended(_)),
            ));
            assert!(std::error::Error::source(owner_source)
                .unwrap()
                .downcast_ref::<tokio::sync::TryLockError>()
                .is_some());
            assert_eq!(
                aura_core::util::serialization::to_vec(
                    held_write
                        .invitations
                        .get(&original.invitation_id)
                        .unwrap()
                        .invitation(),
                )
                .unwrap(),
                before_invitation,
            );
            assert_eq!(effects.list_keys(None).await.unwrap(), keys);
            for (key, value) in before_storage {
                assert_eq!(effects.retrieve(&key).await.unwrap(), value);
            }
            drop(held_write);
            clock.advance(7);
            assert!(manager
                .observe_admitted_enrollment(effects.as_ref(), &start.enrollment_code, &pin)
                .await
                .unwrap()
                .is_some());
            let root = manager
                .take_admitted_enrollment(&original.invitation_id)
                .await
                .unwrap();
            assert_eq!(
                root.child().remaining_ms(effects.as_ref()).await.unwrap(),
                remaining - 7,
                "observing the retained root cannot renew its original deadline",
            );
            manager.cache_admitted_enrollment(root).await.unwrap();
            clock
                .fail_next_observation(TimeError::OperationFailed {
                    reason: "original retained provider fault".into(),
                })
                .await;
            let error = manager
                .observe_admitted_enrollment(effects.as_ref(), "foreign-code", &pin)
                .await
                .expect_err("foreign transfer must fail before reading the original clock");
            assert!(matches!(
                error,
                aura_core::AuraError::PermissionDenied { .. }
            ));
            assert!(matches!(
                clock.physical_time().await,
                Err(TimeError::OperationFailed { reason })
                    if reason == "original retained provider fault"
            ));
            clock
                .fail_next_observation(TimeError::OperationFailed {
                    reason: "required retained observation fault".into(),
                })
                .await;
            let failure = manager
                .observe_admitted_enrollment(effects.as_ref(), &start.enrollment_code, &pin)
                .await
                .expect_err("a required observation must preserve its selected provider failure");
            let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(&failure);
            let mut found = false;
            while let Some(error) = cause {
                if matches!(
                    error.downcast_ref::<TimeError>(),
                    Some(TimeError::OperationFailed { reason })
                        if reason == "required retained observation fault"
                ) {
                    found = true;
                }
                cause = error.source();
            }
            assert!(
                found,
                "the actual native clock cause must survive the owner boundary"
            );
            assert_eq!(
                manager
                    .get_invitation(&original.invitation_id)
                    .await
                    .unwrap()
                    .status,
                original.status,
                "failed observation cannot publish acceptance",
            );
            assert!(manager
                .take_admitted_enrollment(&original.invitation_id)
                .await
                .is_ok());
        });
    }

    #[test]
    fn evicted_pending_admission_cannot_reimport_an_execution_root() {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async {
            let (_issuer, invitee, original, start, _, _) = Box::pin(
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                    "manager-pending-eviction",
                ),
            )
            .await;
            let service = invitee.invitations().unwrap();
            let root = service
                .take_original_enrollment_execution(&original.invitation_id)
                .await
                .unwrap();
            let manager = InvitationManager::with_capacity(1);
            manager.cache_admitted_enrollment(root).await.unwrap();
            manager.cache_invitation(invitation(241)).await;
            assert!(manager
                .get_invitation(&original.invitation_id)
                .await
                .is_none());
            assert!(manager
                .take_admitted_enrollment(&original.invitation_id)
                .await
                .is_err());
            let app = std::sync::Arc::new(async_lock::RwLock::new(
                aura_app::AppCore::with_runtime(
                    aura_app::AppConfig::default(),
                    std::sync::Arc::new(crate::runtime_bridge::AgentRuntimeBridge::new(
                        invitee.clone(),
                    )),
                )
                .unwrap(),
            ));
            let transfer = start.manifest_transfer.as_ref().unwrap();
            let pin =
                aura_app::ui::workflows::ceremonies::pin_user_transferred_enrollment_manifest(
                    &app,
                    transfer.manifest_code.clone(),
                    transfer.initiator_verifier_code.clone(),
                )
                .await
                .unwrap();
            let error = service
                .import_enrollment_and_cache(&start.enrollment_code, &pin)
                .await
                .expect_err("eviction drops original custody rather than renewing Pending");
            let aura_invitation::enrollment_manifest::EnrollmentManifestError::Runtime(source) =
                error
            else {
                panic!("expected original admission birth refusal");
            };
            let native = source.downcast_ref::<aura_core::AuraError>().unwrap();
            assert!(matches!(
                std::error::Error::source(native).unwrap().downcast_ref::<
                    crate::handlers::invitation::enrollment_manifest_admission::EnrollmentAdmissionBirthError
                >(),
                Some(crate::handlers::invitation::enrollment_manifest_admission::EnrollmentAdmissionBirthError::AlreadyPublished),
            ));
            assert!(service
                .take_original_enrollment_execution(&original.invitation_id)
                .await
                .is_err());
        });
    }
}
