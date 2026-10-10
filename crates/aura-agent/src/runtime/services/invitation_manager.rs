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
}

impl CachedInvitationEntry {
    fn invitation(&self) -> &Invitation {
        match self {
            Self::Observed(invitation) | Self::AdmittedEnrollment { invitation, .. } => invitation,
        }
    }
    fn commit_update(&mut self, updated: Invitation) -> Result<(), aura_core::AuraError> {
        if let Self::AdmittedEnrollment { root, .. } = self {
            let original = root.origin().canonical_invitation();
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
            Self::Observed(invitation) | Self::AdmittedEnrollment { invitation, .. } => {
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
#[aura_macros::actor_owned(
    owner = "invitation_manager",
    domain = "invitation_cache",
    gate = "invitation_cache_command_ingress",
    command = InvitationManagerCommand,
    capacity = 128,
    category = "actor_owned"
)]
pub struct InvitationManager {
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
                .first_key_value()
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
                    Some(CachedInvitationEntry::AdmittedEnrollment { .. })
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
                match std::mem::replace(entry, CachedInvitationEntry::Observed(observation)) {
                    CachedInvitationEntry::AdmittedEnrollment { root, .. } => Ok(*root),
                    CachedInvitationEntry::Observed(_) => {
                        Err(owner_refused(InvitationExecutionOwnerError::Missing))
                    }
                }
            },
            |_| Ok(()),
        )
        .await
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
