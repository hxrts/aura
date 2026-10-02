//! Observed-only projection mutation helpers.
//!
//! These helpers update both the ViewState (for futures-signals) and the
//! ReactiveHandler signals (for app-level subscriptions) to ensure consistent
//! state across both signal systems without pretending to be authoritative
//! workflow primitives.
#![allow(dead_code)]
// These helpers are consumed from sibling workflow modules and unit-test-only
// paths; strict all-target dead-code analysis does not model that usage
// consistently across the workspace's clippy lanes.

use std::sync::Arc;

use async_lock::RwLock;
use aura_chat::{ChatDelta, ChatFact, ChatViewReducer, CHAT_FACT_TYPE_ID};
use aura_composition::{downcast_delta, ViewDeltaReducer};
use aura_core::types::identifiers::{AuthorityId, ChannelId};
use aura_journal::{DomainFact, RelationalFact};

#[cfg(test)]
use crate::effects::reactive::ConditionalEmit;
use crate::projection_owner::ProjectionSlot;
use crate::signal_defs::{HOMES_SIGNAL, HOMES_SIGNAL_NAME};
use crate::views::{
    chat::{ChannelProjectionUpdate, ChatState, Message, MessageDeliveryStatus},
    contacts::ContactsState,
    home::HomesState,
    neighborhood::NeighborhoodState,
    recovery::RecoveryState,
};
use crate::workflows::parse::{
    parse_authority_id as parse_workflow_authority_id, parse_context_id,
};
use crate::workflows::signals::read_signal;
use crate::AppCore;
use aura_core::AuraError;

#[cfg(test)]
async fn replace_projection_observed<T>(
    app_core: &Arc<RwLock<AppCore>>,
    expected_revision: u64,
    state: T,
    slot: ProjectionSlot<T>,
    set_view: impl FnOnce(&mut crate::views::ViewState, T),
) -> Result<(), AuraError>
where
    T: Clone + Send + Sync + 'static,
{
    let owner = app_core.read().await.projection_owner();
    let publication = owner
        .replace_if_current(slot, expected_revision, state)
        .await
        .map_err(|error| AuraError::internal(error.to_string()))?;
    let revision = match publication {
        ConditionalEmit::Published { revision } => revision,
        ConditionalEmit::Stale { current_revision } => {
            return Err(AuraError::invalid(format!(
                "stale observed projection replacement: expected {expected_revision}, current {current_revision}"
            )));
        }
    };
    let snapshot = owner
        .snapshot(slot)
        .await
        .map_err(|error| AuraError::internal(error.to_string()))?;
    if snapshot.revision < revision {
        return Err(AuraError::internal(
            "published projection revision unavailable",
        ));
    }
    app_core
        .write()
        .await
        .mirror_projection_snapshot(slot, snapshot, set_view);
    Ok(())
}

/// Mirror the runtime-owned invitations signal into the ViewState cell.
///
/// The runtime emits `INVITATIONS_SIGNAL` directly, so the ViewState copy
/// (read by snapshots and harness exports) would otherwise stay empty. This
/// does not re-emit the signal.
///
/// OWNERSHIP: observed-display-update
pub async fn mirror_invitations_signal_into_view(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    let owner = app_core.read().await.projection_owner();
    let snapshot = owner
        .snapshot(ProjectionSlot::invitations())
        .await
        .map_err(|error| AuraError::internal(error.to_string()))?;
    let mut core = app_core.write().await;
    core.mirror_projection_snapshot(ProjectionSlot::invitations(), snapshot, |views, state| {
        views.set_invitations(state);
    });
    Ok(())
}

/// Mirror the runtime-owned contacts signal into the ViewState cell, which
/// snapshots read (e.g. contact names for new DMs).
///
/// OWNERSHIP: observed-display-update
pub async fn mirror_contacts_signal_into_view(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    let owner = app_core.read().await.projection_owner();
    let snapshot = owner
        .snapshot(ProjectionSlot::contacts())
        .await
        .map_err(|error| AuraError::internal(error.to_string()))?;
    let mut core = app_core.write().await;
    core.mirror_projection_snapshot(ProjectionSlot::contacts(), snapshot, |views, state| {
        views.set_contacts(state);
    });
    Ok(())
}

/// Mirror the current graph revision of chat into the render snapshot without
/// republishing it or falling back to an older view cell.
pub async fn mirror_chat_signal_into_view(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    let owner = app_core.read().await.projection_owner();
    let snapshot = owner
        .snapshot(ProjectionSlot::chat())
        .await
        .map_err(|error| AuraError::internal(error.to_string()))?;
    app_core.write().await.mirror_projection_snapshot(
        ProjectionSlot::chat(),
        snapshot,
        |views, state| views.set_chat(state),
    );
    Ok(())
}

/// Mirror the runtime recovery projection at its committed graph revision.
pub async fn mirror_recovery_signal_into_view(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    let owner = app_core.read().await.projection_owner();
    let snapshot = owner
        .snapshot(ProjectionSlot::recovery())
        .await
        .map_err(|error| AuraError::internal(error.to_string()))?;
    app_core.write().await.mirror_projection_snapshot(
        ProjectionSlot::recovery(),
        snapshot,
        |views, state| views.set_recovery(state),
    );
    Ok(())
}

/// Mirror the runtime-owned homes signal (rebuilt from `SocialFact`s, including
/// after a restart) into the ViewState cell that snapshots read, and anchor the
/// neighborhood at the current home when it has none yet.
///
/// OWNERSHIP: observed-display-update
pub async fn mirror_homes_signal_into_view(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    let gate = app_core.read().await.navigation_projection_gate();
    let _navigation = gate.lock().await;
    mirror_homes_signal_into_view_locked(app_core).await
}

/// Called only while the shared navigation gate is held. A runtime may still
/// emit into the graph; re-read its revision after each mirror so an emission
/// during a paired local transition is reconciled before this pass ends.
pub(crate) async fn mirror_homes_signal_into_view_locked(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    let owner = app_core.read().await.projection_owner();
    loop {
        let snapshot = owner
            .snapshot(ProjectionSlot::homes())
            .await
            .map_err(|error| AuraError::internal(error.to_string()))?;
        let revision = snapshot.revision;
        let homes = snapshot.value.clone();
        let navigation_change = {
            let mut core = app_core.write().await;
            let previous_selection = core.active_home_selection();
            let previous_position = core.views().get_neighborhood().position;
            let copied = core.mirror_projection_snapshot(
                ProjectionSlot::homes(),
                snapshot,
                |views, state| views.set_homes(state),
            );
            if !copied && core.mirrored_homes_revision() != Some(revision) {
                None
            } else {
                let selected = homes
                    .current_home()
                    .map(|home| (home.id, home.name.clone()));
                core.set_active_home_selection(selected.as_ref().map(|(id, _)| *id));
                let should_reconcile = previous_selection != selected.as_ref().map(|(id, _)| *id)
                    && previous_position.as_ref().is_some_and(|position| {
                        Some(position.current_home_id) == previous_selection
                    });
                Some((
                    selected,
                    previous_selection,
                    should_reconcile,
                    previous_position,
                ))
            }
        };
        if let Some((selected, previous_selection, should_reconcile, previous_position)) =
            navigation_change
        {
            let neighborhood = owner
                .snapshot(ProjectionSlot::neighborhood())
                .await
                .map_err(|error| AuraError::internal(error.to_string()))?;
            let needs_anchor =
                neighborhood.value.home_home_id == ChannelId::default() && selected.is_some();
            let needs_reconcile = should_reconcile
                && neighborhood
                    .value
                    .position
                    .as_ref()
                    .is_some_and(|position| Some(position.current_home_id) == previous_selection);
            if needs_anchor || needs_reconcile {
                update_neighborhood_projection_observed(app_core, move |neighborhood| {
                    if neighborhood.home_home_id == ChannelId::default() {
                        if let Some((home_id, home_name)) = selected {
                            neighborhood.home_home_id = home_id;
                            neighborhood.home_name = home_name.clone();
                            neighborhood.position =
                                Some(crate::views::neighborhood::TraversalPosition {
                                    current_home_id: home_id,
                                    current_home_name: home_name,
                                    depth: 2,
                                    path: vec![home_id],
                                });
                        }
                    } else if should_reconcile
                        && neighborhood.position.as_ref().is_some_and(|position| {
                            Some(position.current_home_id) == previous_selection
                        })
                    {
                        neighborhood.position = selected.map(|(home_id, home_name)| {
                            crate::views::neighborhood::TraversalPosition {
                                current_home_id: home_id,
                                current_home_name: home_name,
                                depth: previous_position
                                    .as_ref()
                                    .map_or(2, |position| position.depth),
                                path: vec![home_id],
                            }
                        });
                    }
                })
                .await?;
            }
        }
        let latest = owner
            .snapshot(ProjectionSlot::homes())
            .await
            .map_err(|error| AuraError::internal(error.to_string()))?;
        if latest.revision == revision {
            return Ok(());
        }
    }
}

pub async fn homes_signal_snapshot(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<HomesState, AuraError> {
    read_signal(app_core, &*HOMES_SIGNAL, HOMES_SIGNAL_NAME).await
}

#[cfg(test)]
pub async fn replace_recovery_projection_observed(
    app_core: &Arc<RwLock<AppCore>>,
    expected_revision: u64,
    state: RecoveryState,
) -> Result<(), AuraError> {
    replace_projection_observed(
        app_core,
        expected_revision,
        state,
        ProjectionSlot::recovery(),
        |views, state| {
            // OWNERSHIP: observed-display-update
            views.set_recovery(state);
        },
    )
    .await
}

#[cfg(test)]
pub async fn replace_homes_projection_observed(
    app_core: &Arc<RwLock<AppCore>>,
    expected_revision: u64,
    state: HomesState,
) -> Result<(), AuraError> {
    replace_projection_observed(
        app_core,
        expected_revision,
        state,
        ProjectionSlot::homes(),
        |views, state| {
            // OWNERSHIP: observed-display-update
            views.set_homes(state);
        },
    )
    .await
}

/// Observed-only projection update helper for chat state.
///
/// This helper updates both:
/// 1. ViewState (for futures-signals subscribers)
/// 2. CHAT_SIGNAL (for ReactiveEffects subscribers)
///
/// OWNERSHIP: observed-display-update
async fn update_projection_observed<T, R>(
    app_core: &Arc<RwLock<AppCore>>,
    slot: ProjectionSlot<T>,
    update: impl FnOnce(&mut T) -> R,
    set_view: impl FnOnce(&mut crate::views::ViewState, T),
) -> Result<R, AuraError>
where
    T: Clone + Send + Sync + 'static,
{
    let owner = app_core.read().await.projection_owner();
    let (result, snapshot) = owner
        .update(slot, |state| Ok::<R, AuraError>(update(state)))
        .await
        .map_err(|error| AuraError::internal(error.to_string()))??;
    app_core
        .write()
        .await
        .mirror_projection_snapshot(slot, snapshot, set_view);
    Ok(result)
}

async fn try_update_projection_observed<T, R>(
    app_core: &Arc<RwLock<AppCore>>,
    slot: ProjectionSlot<T>,
    update: impl FnOnce(&mut T) -> Result<R, AuraError>,
    set_view: impl FnOnce(&mut crate::views::ViewState, T),
) -> Result<R, AuraError>
where
    T: Clone + Send + Sync + 'static,
{
    let owner = app_core.read().await.projection_owner();
    let (result, snapshot) = owner
        .update(slot, update)
        .await
        .map_err(|error| AuraError::internal(error.to_string()))??;
    app_core
        .write()
        .await
        .mirror_projection_snapshot(slot, snapshot, set_view);
    Ok(result)
}

pub async fn update_chat_projection_observed<T>(
    app_core: &Arc<RwLock<AppCore>>,
    update: impl FnOnce(&mut ChatState) -> T,
) -> Result<T, AuraError> {
    update_projection_observed(app_core, ProjectionSlot::chat(), update, |views, state| {
        views.set_chat(state);
    })
    .await
}

/// Apply an authoritative chat fact to the local chat projection through the
/// sanctioned chat reducer, then mirror the reduced state into `CHAT_SIGNAL`.
pub async fn reduce_chat_fact_observed(
    app_core: &Arc<RwLock<AppCore>>,
    fact: &ChatFact,
) -> Result<(), AuraError> {
    let RelationalFact::Generic { envelope, .. } = fact.to_generic() else {
        return Err(AuraError::internal(
            "chat fact reduction requires generic relational fact envelope",
        ));
    };

    let reducer = ChatViewReducer;
    let deltas = reducer.reduce_fact(CHAT_FACT_TYPE_ID, &envelope.payload, None);
    let owner = app_core.read().await.projection_owner();
    let (_, snapshot) = owner
        .update(ProjectionSlot::chat(), |state| {
            for delta in deltas {
                let Some(chat_delta) = downcast_delta::<ChatDelta>(&delta) else {
                    continue;
                };
                apply_chat_delta_reduced(state, chat_delta.clone())?;
            }
            Ok::<(), AuraError>(())
        })
        .await
        .map_err(|error| AuraError::internal(error.to_string()))??;
    app_core.write().await.mirror_projection_snapshot(
        ProjectionSlot::chat(),
        snapshot,
        |views, state| views.set_chat(state),
    );
    Ok(())
}

fn parse_channel_id(raw: &str) -> Result<ChannelId, AuraError> {
    raw.parse::<ChannelId>()
        .map_err(|_| AuraError::invalid(format!("Invalid channel ID in chat delta: {raw}")))
}

fn parse_authority_id(raw: &str) -> Result<AuthorityId, AuraError> {
    parse_workflow_authority_id(raw)
        .map_err(|_| AuraError::invalid(format!("Invalid authority ID in chat delta: {raw}")))
}

#[allow(clippy::manual_unwrap_or_default)]
fn apply_chat_delta_reduced(state: &mut ChatState, delta: ChatDelta) -> Result<(), AuraError> {
    match delta {
        ChatDelta::ChannelAdded(creation) => {
            state.materialize_canonical_channel(creation, None);
        }
        ChatDelta::ChannelRemoved { channel_id } => {
            let channel_id = parse_channel_id(&channel_id)?;
            let _ = state.remove_channel(&channel_id);
        }
        ChatDelta::ChannelUpdated {
            channel_id,
            context_id,
            name,
            topic,
            member_count,
            member_ids,
            updated_at,
        } => {
            let channel_id = parse_channel_id(&channel_id)?;
            let context_id = context_id.as_deref().map(parse_context_id).transpose()?;
            let member_ids = member_ids
                .map(|ids| {
                    ids.into_iter()
                        .map(|raw| parse_authority_id(&raw))
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?;
            state.apply_or_stage_channel_update(
                channel_id,
                ChannelProjectionUpdate {
                    context_id,
                    name,
                    topic,
                    member_count,
                    member_ids,
                    updated_at,
                },
            );
        }
        ChatDelta::MessageAdded {
            channel_id,
            message_id,
            sender_id,
            sender_name,
            content,
            timestamp,
            reply_to,
            epoch_hint,
        } => {
            let channel_id = parse_channel_id(&channel_id)?;
            let sender_id = parse_authority_id(&sender_id)?;
            state.apply_message(
                channel_id,
                Message {
                    id: message_id,
                    channel_id,
                    sender_id,
                    sender_name,
                    content,
                    timestamp,
                    reply_to,
                    is_own: false,
                    is_read: false,
                    delivery_status: MessageDeliveryStatus::Sent,
                    epoch_hint,
                    is_finalized: false,
                },
            );
        }
        ChatDelta::MessageRemoved {
            channel_id,
            message_id,
        } => {
            let channel_id = parse_channel_id(&channel_id)?;
            state.remove_message(&channel_id, &message_id);
        }
        ChatDelta::MessageUpdated {
            channel_id,
            message_id,
            new_content,
            ..
        } => {
            let channel_id = parse_channel_id(&channel_id)?;
            if let Some(message) = state.message_mut(&channel_id, &message_id) {
                message.content = new_content;
            }
        }
        ChatDelta::MessageDeliveryUpdated {
            channel_id,
            message_id,
            delivery_status,
        } => {
            let channel_id = parse_channel_id(&channel_id)?;
            if let Some(message) = state.message_mut(&channel_id, &message_id) {
                message.delivery_status = match delivery_status {
                    aura_chat::ChatMessageDeliveryStatus::Sent => MessageDeliveryStatus::Sent,
                    aura_chat::ChatMessageDeliveryStatus::Delivered => {
                        MessageDeliveryStatus::Delivered
                    }
                    aura_chat::ChatMessageDeliveryStatus::Read => MessageDeliveryStatus::Read,
                    aura_chat::ChatMessageDeliveryStatus::Failed => MessageDeliveryStatus::Failed,
                };
            }
        }
        ChatDelta::MessageRead {
            channel_id,
            message_id,
            ..
        } => {
            let channel_id = parse_channel_id(&channel_id)?;
            state.mark_message_read(&channel_id, &message_id);
        }
    }

    Ok(())
}

/// Observed-only projection update helper for recovery state.
///
/// This helper updates both:
/// 1. ViewState (for futures-signals subscribers)
/// 2. RECOVERY_SIGNAL (for ReactiveEffects subscribers)
///
/// OWNERSHIP: observed-display-update
pub async fn update_recovery_projection_observed<T>(
    app_core: &Arc<RwLock<AppCore>>,
    update: impl FnOnce(&mut RecoveryState) -> T,
) -> Result<T, AuraError> {
    update_projection_observed(
        app_core,
        ProjectionSlot::recovery(),
        update,
        |views, state| views.set_recovery(state),
    )
    .await
}

pub async fn try_update_recovery_projection_observed<T>(
    app_core: &Arc<RwLock<AppCore>>,
    update: impl FnOnce(&mut RecoveryState) -> Result<T, AuraError>,
) -> Result<T, AuraError> {
    try_update_projection_observed(
        app_core,
        ProjectionSlot::recovery(),
        update,
        |views, state| views.set_recovery(state),
    )
    .await
}

/// Observed-only projection update helper for contacts state.
///
/// This helper updates both:
/// 1. ViewState (for futures-signals subscribers)
/// 2. CONTACTS_SIGNAL (for ReactiveEffects subscribers)
///
/// OWNERSHIP: observed-display-update
pub async fn update_contacts_projection_observed<T>(
    app_core: &Arc<RwLock<AppCore>>,
    update: impl FnOnce(&mut ContactsState) -> T,
) -> Result<T, AuraError> {
    update_projection_observed(
        app_core,
        ProjectionSlot::contacts(),
        update,
        |views, state| views.set_contacts(state),
    )
    .await
}

/// Observed-only projection update helper for homes state.
///
/// This helper updates both:
/// 1. ViewState (for futures-signals subscribers)
/// 2. HOMES_SIGNAL (for ReactiveEffects subscribers)
///
/// OWNERSHIP: observed-display-update
pub async fn update_homes_projection_observed<T>(
    app_core: &Arc<RwLock<AppCore>>,
    update: impl FnOnce(&mut HomesState) -> T,
) -> Result<T, AuraError> {
    update_homes_projection_with_revision(app_core, update)
        .await
        .map(|(output, _)| output)
}

pub async fn update_homes_projection_with_revision<T>(
    app_core: &Arc<RwLock<AppCore>>,
    update: impl FnOnce(&mut HomesState) -> T,
) -> Result<(T, u64), AuraError> {
    let owner = app_core.read().await.projection_owner();
    let (output, snapshot) = owner
        .update(ProjectionSlot::homes(), |homes| {
            Ok::<T, AuraError>(update(homes))
        })
        .await
        .map_err(|error| AuraError::internal(error.to_string()))??;
    let revision = snapshot.revision;
    app_core.write().await.mirror_projection_snapshot(
        ProjectionSlot::homes(),
        snapshot,
        |views, state| views.set_homes(state),
    );
    Ok((output, revision))
}

pub async fn try_update_homes_projection_observed<T>(
    app_core: &Arc<RwLock<AppCore>>,
    update: impl FnOnce(&mut HomesState) -> Result<T, AuraError>,
) -> Result<T, AuraError> {
    try_update_projection_observed(app_core, ProjectionSlot::homes(), update, |views, state| {
        views.set_homes(state);
    })
    .await
}

/// Observed-only projection update helper for neighborhood state.
///
/// This helper updates both:
/// 1. ViewState (for futures-signals subscribers)
/// 2. NEIGHBORHOOD_SIGNAL (for ReactiveEffects subscribers)
///
/// OWNERSHIP: observed-display-update
pub async fn update_neighborhood_projection_observed<T>(
    app_core: &Arc<RwLock<AppCore>>,
    update: impl FnOnce(&mut NeighborhoodState) -> T,
) -> Result<T, AuraError> {
    update_projection_observed(
        app_core,
        ProjectionSlot::neighborhood(),
        update,
        |views, state| views.set_neighborhood(state),
    )
    .await
}

pub async fn try_update_neighborhood_projection_observed<T>(
    app_core: &Arc<RwLock<AppCore>>,
    update: impl FnOnce(&mut NeighborhoodState) -> Result<T, AuraError>,
) -> Result<T, AuraError> {
    try_update_projection_observed(
        app_core,
        ProjectionSlot::neighborhood(),
        update,
        |views, state| views.set_neighborhood(state),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal_defs::{
        HOMES_SIGNAL, HOMES_SIGNAL_NAME, RECOVERY_SIGNAL, RECOVERY_SIGNAL_NAME,
    };
    use crate::views::chat::{Channel, ChannelType};
    use crate::views::recovery::{Guardian, GuardianStatus, RecoveryState};
    use crate::workflows::signals::read_signal;
    use aura_core::hash::hash;
    use aura_core::types::identifiers::ContextId;
    use std::path::Path;

    fn canonical_channel_added(
        context_id: ContextId,
        channel_id: ChannelId,
        name: &str,
        topic: Option<&str>,
        creator_id: AuthorityId,
    ) -> ChatDelta {
        let fact = ChatFact::channel_created_ms(
            context_id,
            channel_id,
            name.to_string(),
            topic.map(str::to_string),
            false,
            10,
            creator_id,
        );
        let RelationalFact::Generic { envelope, .. } = fact.to_generic() else {
            unreachable!("ChatFact always encodes as a generic relational fact")
        };
        ChatViewReducer
            .reduce_fact(CHAT_FACT_TYPE_ID, &envelope.payload, None)
            .into_iter()
            .filter_map(|delta| downcast_delta::<ChatDelta>(&delta).cloned())
            .next()
            .expect("ChannelCreated carries canonical creation evidence")
    }

    #[test]
    fn channel_added_replaces_canonical_fields_without_preserving_stale_context() {
        let channel_id = ChannelId::from_bytes(hash(b"observed-projection-strict-channel"));
        let stale_context = ContextId::new_from_entropy([1u8; 32]);
        let canonical_context = ContextId::new_from_entropy([2u8; 32]);
        let mut state = ChatState::from_channels([Channel {
            id: channel_id,
            context_id: Some(stale_context),
            name: "old".to_string(),
            topic: Some("old-topic".to_string()),
            channel_type: ChannelType::Home,
            unread_count: 7,
            is_dm: false,
            member_ids: Vec::new(),
            member_count: 9,
            last_message: None,
            last_message_time: None,
            last_activity: 100,
            last_finalized_epoch: 0,
        }]);

        apply_chat_delta_reduced(
            &mut state,
            canonical_channel_added(
                canonical_context,
                channel_id,
                "shared-parity-lab",
                Some("canonical-topic"),
                AuthorityId::new_from_entropy([3u8; 32]),
            ),
        )
        .expect("apply channel added");

        let channel = state.channel(&channel_id).expect("channel must exist");
        assert_eq!(channel.context_id, Some(canonical_context));
        assert_eq!(channel.name, "shared-parity-lab");
        assert_eq!(channel.topic.as_deref(), Some("canonical-topic"));
        assert_eq!(channel.member_count, 1);
        assert_eq!(channel.last_activity, 10);
    }

    #[test]
    fn channel_updated_without_canonical_name_does_not_materialize_unknown_channel() {
        let channel_id = ChannelId::from_bytes(hash(b"observed-projection-missing-name"));
        let mut state = ChatState::default();

        apply_chat_delta_reduced(
            &mut state,
            ChatDelta::ChannelUpdated {
                channel_id: channel_id.to_string(),
                context_id: Some(ContextId::new_from_entropy([4u8; 32]).to_string()),
                name: None,
                topic: Some("topic".to_string()),
                member_count: Some(2),
                member_ids: None,
                updated_at: 20,
            },
        )
        .expect("apply channel updated");

        assert!(state.channel(&channel_id).is_none());
    }

    #[test]
    fn out_of_order_channel_updates_and_message_wait_for_creation_evidence() {
        let channel_id = ChannelId::from_bytes(hash(b"observed-channel-order"));
        let context_id = ContextId::new_from_entropy([91u8; 32]);
        let creator = AuthorityId::new_from_entropy([92u8; 32]);
        let mut state = ChatState::default();
        let creation = canonical_channel_added(context_id, channel_id, "original", None, creator);

        apply_chat_delta_reduced(
            &mut state,
            ChatDelta::ChannelUpdated {
                channel_id: channel_id.to_string(),
                context_id: Some(context_id.to_string()),
                name: Some("newer".to_string()),
                topic: None,
                member_count: Some(3),
                member_ids: None,
                updated_at: 30,
            },
        )
        .unwrap();
        apply_chat_delta_reduced(
            &mut state,
            ChatDelta::MessageAdded {
                channel_id: channel_id.to_string(),
                message_id: "before-creation".to_string(),
                sender_id: creator.to_string(),
                sender_name: "Creator".to_string(),
                content: "hello".to_string(),
                timestamp: 12,
                reply_to: None,
                epoch_hint: None,
            },
        )
        .unwrap();
        assert!(
            state.channel(&channel_id).is_none(),
            "partial facts must remain invisible"
        );
        assert_eq!(state.message_count(), 1, "message is held for replay");

        apply_chat_delta_reduced(&mut state, creation.clone()).unwrap();
        let channel = state.channel(&channel_id).unwrap();
        assert_eq!(channel.name, "newer");
        assert_eq!(channel.member_count, 3);
        assert_eq!(channel.last_message.as_deref(), Some("hello"));

        apply_chat_delta_reduced(
            &mut state,
            ChatDelta::ChannelUpdated {
                channel_id: channel_id.to_string(),
                context_id: Some(context_id.to_string()),
                name: Some("older".to_string()),
                topic: Some("older topic".to_string()),
                member_count: None,
                member_ids: None,
                updated_at: 20,
            },
        )
        .unwrap();
        apply_chat_delta_reduced(&mut state, creation).unwrap();
        let channel = state.channel(&channel_id).unwrap();
        assert_eq!(
            channel.name, "newer",
            "stale update and duplicate creation must not regress name"
        );
        assert_eq!(channel.topic.as_deref(), Some("older topic"));
        assert_eq!(channel.member_count, 3);
        assert_eq!(state.channel_count(), 1);
    }

    #[test]
    fn canonical_channel_metadata_comes_from_creation_witness() {
        let context_id = ContextId::new_from_entropy([103u8; 32]);
        let channel_id = ChannelId::from_bytes(hash(b"canonical-witness-metadata"));
        let creator = AuthorityId::new_from_entropy([104u8; 32]);
        let fact = ChatFact::channel_created_ms(
            context_id,
            channel_id,
            "fact-name".to_string(),
            Some("fact-topic".to_string()),
            true,
            42,
            creator,
        );
        let RelationalFact::Generic { envelope, .. } = fact.to_generic() else {
            unreachable!("ChatFact always encodes as a generic relational fact")
        };
        let ChatDelta::ChannelAdded(creation) = ChatViewReducer
            .reduce_fact(CHAT_FACT_TYPE_ID, &envelope.payload, None)
            .into_iter()
            .filter_map(|delta| downcast_delta::<ChatDelta>(&delta).cloned())
            .next()
            .expect("ChannelCreated reduces to ChannelAdded")
        else {
            unreachable!("creation helper returns ChannelAdded")
        };
        let mut state = ChatState::default();
        state.materialize_canonical_channel(creation, None);
        let channel = state.channel(&channel_id).unwrap();
        assert_eq!(channel.context_id, Some(context_id));
        assert_eq!(channel.name, "fact-name");
        assert_eq!(channel.topic.as_deref(), Some("fact-topic"));
        assert!(channel.is_dm);
        assert_eq!(channel.channel_type, ChannelType::DirectMessage);
        assert_eq!(channel.member_count, 2);
        assert!(
            channel.member_ids.is_empty(),
            "membership is not invented without a local participant"
        );
    }

    #[test]
    fn channel_added_does_not_rebind_existing_channel_by_name() {
        let stale_id = ChannelId::from_bytes(hash(b"observed-projection-stale-id"));
        let canonical_id = ChannelId::from_bytes(hash(b"observed-projection-canonical-id"));
        let mut state = ChatState::from_channels([Channel {
            id: stale_id,
            context_id: Some(ContextId::new_from_entropy([5u8; 32])),
            name: "shared-parity-lab".to_string(),
            topic: None,
            channel_type: ChannelType::Home,
            unread_count: 0,
            is_dm: false,
            member_ids: Vec::new(),
            member_count: 1,
            last_message: None,
            last_message_time: None,
            last_activity: 0,
            last_finalized_epoch: 0,
        }]);

        apply_chat_delta_reduced(
            &mut state,
            canonical_channel_added(
                ContextId::new_from_entropy([6u8; 32]),
                canonical_id,
                "shared-parity-lab",
                None,
                AuthorityId::new_from_entropy([7u8; 32]),
            ),
        )
        .expect("apply channel added");

        assert!(state.channel(&stale_id).is_some());
        assert!(state.channel(&canonical_id).is_some());
        assert_eq!(state.channel_count(), 2);
    }

    async fn init_signals_for_test(app_core: &Arc<RwLock<AppCore>>) {
        AppCore::init_signals_with_hooks(app_core).await.unwrap();
    }

    #[tokio::test]
    async fn replace_homes_projection_observed_updates_view_and_signal_through_one_helper() {
        let app_core = crate::testing::default_test_app_core();
        init_signals_for_test(&app_core).await;

        let home_id = ChannelId::from_bytes(hash(b"observed-projection-homes-shared-helper"));
        let homes = HomesState::from_parts(
            std::collections::HashMap::from([(
                home_id,
                crate::views::home::HomeState::new(
                    home_id,
                    Some("shared-home".to_string()),
                    AuthorityId::new_from_entropy([11u8; 32]),
                    1,
                    ContextId::new_from_entropy([12u8; 32]),
                ),
            )]),
            Some(home_id),
        );

        let revision = app_core
            .read()
            .await
            .projection_owner()
            .snapshot(ProjectionSlot::homes())
            .await
            .expect("read homes revision")
            .revision;
        replace_homes_projection_observed(&app_core, revision, homes.clone())
            .await
            .expect("replace homes projection");

        let signal_state = read_signal(&app_core, &*HOMES_SIGNAL, HOMES_SIGNAL_NAME)
            .await
            .expect("read homes signal");
        let view_state = {
            let core = app_core.read().await;
            core.snapshot().homes
        };

        assert_eq!(signal_state.current_home_id(), homes.current_home_id());
        assert_eq!(signal_state.count(), homes.count());
        assert!(signal_state.home_state(&home_id).is_some());
        assert_eq!(view_state.current_home_id(), homes.current_home_id());
        assert_eq!(view_state.count(), homes.count());
        assert!(view_state.home_state(&home_id).is_some());
    }

    #[tokio::test]
    async fn replace_recovery_projection_observed_updates_view_and_signal_through_one_helper() {
        let app_core = crate::testing::default_test_app_core();
        init_signals_for_test(&app_core).await;

        let recovery = RecoveryState::from_parts(
            [Guardian {
                id: AuthorityId::new_from_entropy([13u8; 32]),
                name: "guardian".to_string(),
                status: GuardianStatus::Active,
                added_at: 1,
                last_seen: Some(2),
            }],
            1,
            None,
            Vec::new(),
            Vec::new(),
        );

        let revision = app_core
            .read()
            .await
            .projection_owner()
            .snapshot(ProjectionSlot::recovery())
            .await
            .expect("read recovery revision")
            .revision;
        replace_recovery_projection_observed(&app_core, revision, recovery.clone())
            .await
            .expect("replace recovery projection");

        let signal_state = read_signal(&app_core, &*RECOVERY_SIGNAL, RECOVERY_SIGNAL_NAME)
            .await
            .expect("read recovery signal");
        let view_state = {
            let core = app_core.read().await;
            core.snapshot().recovery
        };

        assert_eq!(signal_state.guardian_count(), recovery.guardian_count());
        assert_eq!(signal_state.threshold(), recovery.threshold());
        assert_eq!(view_state.guardian_count(), recovery.guardian_count());
        assert_eq!(view_state.threshold(), recovery.threshold());
    }

    #[tokio::test]
    async fn delayed_projection_mirror_and_stale_replacement_cannot_regress_view() {
        let app_core = crate::testing::default_test_app_core();
        init_signals_for_test(&app_core).await;
        let owner = app_core.read().await.projection_owner();
        let before = owner
            .snapshot(ProjectionSlot::homes())
            .await
            .expect("initial homes snapshot");
        let first_id = ChannelId::from_bytes(hash(b"projection-revision-first"));
        let second_id = ChannelId::from_bytes(hash(b"projection-revision-second"));
        let make_home = |id| {
            crate::views::home::HomeState::new(
                id,
                Some(id.to_string()),
                AuthorityId::new_from_entropy([21u8; 32]),
                1,
                ContextId::new_from_entropy([22u8; 32]),
            )
        };
        update_homes_projection_observed(&app_core, |homes| {
            homes.add_home(make_home(first_id));
        })
        .await
        .expect("first update");
        let stale = owner
            .snapshot(ProjectionSlot::homes())
            .await
            .expect("first published snapshot");
        update_homes_projection_observed(&app_core, |homes| {
            homes.add_home(make_home(second_id));
        })
        .await
        .expect("second update");

        let mirrored = app_core.write().await.mirror_projection_snapshot(
            ProjectionSlot::homes(),
            stale.clone(),
            |views, state| views.set_homes(state),
        );
        assert!(!mirrored, "older async mirror must not overwrite the view");
        let error = replace_homes_projection_observed(&app_core, before.revision, stale.value)
            .await
            .expect_err("replacement based on an old revision must fail");
        assert!(error.to_string().contains("stale observed projection"));
        let state = app_core.read().await.snapshot();
        assert!(state.homes.has_home(&first_id));
        assert!(state.homes.has_home(&second_id));
        assert_eq!(
            state.projection_source_revisions.homes,
            Some(
                owner
                    .snapshot(ProjectionSlot::homes())
                    .await
                    .unwrap()
                    .revision
            )
        );
    }

    #[tokio::test]
    async fn homes_mirror_clears_active_selection_after_selected_home_removal() {
        let app_core = crate::testing::default_test_app_core();
        init_signals_for_test(&app_core).await;
        let home_id = ChannelId::from_bytes(hash(b"homes-mirror-removal"));
        update_homes_projection_observed(&app_core, |homes| {
            homes.add_home(crate::views::home::HomeState::new(
                home_id,
                Some("Removed home".to_string()),
                AuthorityId::new_from_entropy([31u8; 32]),
                1,
                ContextId::new_from_entropy([32u8; 32]),
            ));
            homes.select_home(Some(home_id));
        })
        .await
        .unwrap();
        mirror_homes_signal_into_view(&app_core).await.unwrap();
        assert_eq!(app_core.read().await.active_home_selection(), Some(home_id));

        update_homes_projection_observed(&app_core, |homes| {
            homes.remove_home(&home_id);
        })
        .await
        .unwrap();
        mirror_homes_signal_into_view(&app_core).await.unwrap();
        assert_eq!(app_core.read().await.active_home_selection(), None);
    }

    #[tokio::test]
    async fn runtime_homes_revision_reconciles_navigation_after_gate_releases() {
        let app_core = crate::testing::default_test_app_core();
        init_signals_for_test(&app_core).await;
        let first_id = ChannelId::from_bytes(hash(b"runtime-navigation-first"));
        let second_id = ChannelId::from_bytes(hash(b"runtime-navigation-second"));
        for (home_id, name) in [(first_id, "First"), (second_id, "Second")] {
            update_homes_projection_observed(&app_core, move |homes| {
                homes.add_home(crate::views::home::HomeState::new(
                    home_id,
                    Some(name.to_string()),
                    AuthorityId::new_from_entropy([41u8; 32]),
                    1,
                    ContextId::new_from_entropy([42u8; 32]),
                ));
                homes.select_home(Some(first_id));
            })
            .await
            .unwrap();
        }
        mirror_homes_signal_into_view(&app_core).await.unwrap();
        let gate = app_core.read().await.navigation_projection_gate();
        let held = gate.lock().await;
        let owner = app_core.read().await.projection_owner();
        owner
            .update(ProjectionSlot::homes(), |homes| {
                homes.select_home(Some(second_id));
                Ok::<(), AuraError>(())
            })
            .await
            .unwrap()
            .unwrap();
        use futures::FutureExt;
        let mirror = mirror_homes_signal_into_view(&app_core);
        futures::pin_mut!(mirror);
        assert!((&mut mirror).now_or_never().is_none());
        drop(held);
        mirror.await.unwrap();

        let core = app_core.read().await;
        assert_eq!(core.active_home_selection(), Some(second_id));
        assert_eq!(
            core.views().get_homes().current_home_id().copied(),
            Some(second_id)
        );
        assert_eq!(
            core.views()
                .get_neighborhood()
                .position
                .as_ref()
                .map(|position| position.current_home_id),
            Some(second_id)
        );
    }

    #[test]
    fn homes_and_recovery_publication_helpers_are_shared_across_workflows() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for relative_path in [
            "crates/aura-app/src/workflows/settings.rs",
            "crates/aura-app/src/workflows/moderator.rs",
            "crates/aura-app/src/workflows/moderation.rs",
            "crates/aura-app/src/workflows/access.rs",
        ] {
            let source = std::fs::read_to_string(repo_root.join(relative_path))
                .unwrap_or_else(|error| panic!("failed to read {relative_path}: {error}"));
            assert!(!source.contains("async fn emit_homes_state_observed("));
            assert!(!source.contains("core.views_mut().set_homes("));
        }

        let neighborhood_source = std::fs::read_to_string(
            repo_root.join("crates/aura-app/src/workflows/context/neighborhood.rs"),
        )
        .unwrap_or_else(|error| panic!("failed to read context/neighborhood.rs: {error}"));
        assert!(!neighborhood_source.contains("async fn homes_state_signal_snapshot("));
        assert!(neighborhood_source.contains("homes_signal_snapshot"));

        let settings_source =
            std::fs::read_to_string(repo_root.join("crates/aura-app/src/workflows/settings.rs"))
                .unwrap_or_else(|error| panic!("failed to read settings.rs: {error}"));
        assert!(!settings_source.contains("async fn emit_recovery_state_observed("));
        assert!(!settings_source.contains("core.views_mut().set_recovery("));
        assert!(settings_source.contains("try_update_homes_projection_observed"));
        assert!(settings_source.contains("try_update_recovery_projection_observed"));

        let system_refresh_source = std::fs::read_to_string(
            repo_root.join("crates/aura-app/src/workflows/system/refresh.rs"),
        )
        .unwrap_or_else(|error| panic!("failed to read system/refresh.rs: {error}"));
        assert!(!system_refresh_source.contains("emit_signal(app_core, &*CHAT_SIGNAL"));
        assert!(system_refresh_source.contains("mirror_chat_signal_into_view"));
    }
}
