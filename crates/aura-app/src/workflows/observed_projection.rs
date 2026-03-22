//! Observed-only projection mutation helpers.
//!
//! These helpers update both the ViewState (for futures-signals) and the
//! ReactiveHandler signals (for app-level subscriptions) to ensure consistent
//! state across both signal systems without pretending to be authoritative
//! workflow primitives.

use std::sync::Arc;

use async_lock::RwLock;
use aura_chat::{ChatDelta, ChatFact, ChatViewReducer, CHAT_FACT_TYPE_ID};
use aura_composition::{downcast_delta, ViewDeltaReducer};
use aura_core::types::identifiers::{AuthorityId, ChannelId};
use aura_journal::{DomainFact, RelationalFact};

use crate::signal_defs::{
    CHAT_SIGNAL, CHAT_SIGNAL_NAME, CONTACTS_SIGNAL, CONTACTS_SIGNAL_NAME, HOMES_SIGNAL,
    HOMES_SIGNAL_NAME, NEIGHBORHOOD_SIGNAL, NEIGHBORHOOD_SIGNAL_NAME, RECOVERY_SIGNAL,
    RECOVERY_SIGNAL_NAME,
};
use crate::views::{
    chat::{Channel, ChannelType, ChatState, Message, MessageDeliveryStatus},
    contacts::ContactsState,
    home::HomesState,
    neighborhood::NeighborhoodState,
    recovery::RecoveryState,
};
use crate::workflows::parse::{
    parse_authority_id as parse_workflow_authority_id, parse_context_id,
};
use crate::workflows::signals::{emit_signal, read_signal};
use crate::AppCore;
use aura_core::AuraError;

#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub async fn homes_signal_snapshot(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<HomesState, AuraError> {
    read_signal(app_core, &*HOMES_SIGNAL, HOMES_SIGNAL_NAME).await
}

#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub async fn replace_recovery_projection_observed(
    app_core: &Arc<RwLock<AppCore>>,
    state: RecoveryState,
) -> Result<(), AuraError> {
    {
        let mut core = app_core.write().await;
        // OWNERSHIP: observed-display-update
        core.views_mut().set_recovery(state.clone());
    }

    emit_signal(app_core, &*RECOVERY_SIGNAL, state, RECOVERY_SIGNAL_NAME).await
}

#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub async fn replace_homes_projection_observed(
    app_core: &Arc<RwLock<AppCore>>,
    state: HomesState,
) -> Result<(), AuraError> {
    {
        let mut core = app_core.write().await;
        // OWNERSHIP: observed-display-update
        core.views_mut().set_homes(state.clone());
    }

    emit_signal(app_core, &*HOMES_SIGNAL, state, HOMES_SIGNAL_NAME).await
}

/// Observed-only projection update helper for chat state.
///
/// This helper updates both:
/// 1. ViewState (for futures-signals subscribers)
/// 2. CHAT_SIGNAL (for ReactiveEffects subscribers)
///
/// OWNERSHIP: observed-display-update
#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub async fn update_chat_projection_observed<T>(
    app_core: &Arc<RwLock<AppCore>>,
    update: impl FnOnce(&mut ChatState) -> T,
) -> Result<T, AuraError> {
    let (output, state) = {
        let mut core = app_core.write().await;
        let mut state = core.snapshot().chat;
        let output = update(&mut state);
        // OWNERSHIP: observed-display-update
        core.views_mut().set_chat(state.clone());
        (output, state)
    };

    // Also emit to CHAT_SIGNAL for ReactiveEffects subscribers
    emit_signal(app_core, &*CHAT_SIGNAL, state, CHAT_SIGNAL_NAME).await?;

    Ok(output)
}

/// Apply an authoritative chat fact to the local chat projection through the
/// sanctioned chat reducer, then mirror the reduced state into `CHAT_SIGNAL`.
#[cfg_attr(not(feature = "signals"), allow(dead_code))]
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
    let state = {
        let current_state = {
            let core = app_core.read().await;
            core.views().get_chat()
        };
        let mut core = app_core.write().await;
        let mut state = current_state;
        for delta in deltas {
            let Some(chat_delta) = downcast_delta::<ChatDelta>(&delta) else {
                continue;
            };
            apply_chat_delta_reduced(&mut state, chat_delta.clone())?;
        }
        // OWNERSHIP: fact-backed
        core.views_mut().set_chat(state.clone());
        state
    };

    emit_signal(app_core, &*CHAT_SIGNAL, state, CHAT_SIGNAL_NAME).await?;
    Ok(())
}

#[cfg_attr(not(feature = "signals"), allow(dead_code))]
fn parse_channel_id(raw: &str) -> Result<ChannelId, AuraError> {
    raw.parse::<ChannelId>()
        .map_err(|_| AuraError::invalid(format!("Invalid channel ID in chat delta: {raw}")))
}

#[cfg_attr(not(feature = "signals"), allow(dead_code))]
fn parse_authority_id(raw: &str) -> Result<AuthorityId, AuraError> {
    parse_workflow_authority_id(raw)
        .map_err(|_| AuraError::invalid(format!("Invalid authority ID in chat delta: {raw}")))
}

#[cfg_attr(not(feature = "signals"), allow(dead_code))]
#[allow(clippy::manual_unwrap_or_default)]
fn apply_chat_delta_reduced(state: &mut ChatState, delta: ChatDelta) -> Result<(), AuraError> {
    match delta {
        ChatDelta::ChannelAdded {
            channel_id,
            context_id,
            name,
            topic,
            is_dm,
            member_count,
            created_at,
            ..
        } => {
            let channel_id = parse_channel_id(&channel_id)?;
            let context_id = context_id.as_deref().map(parse_context_id).transpose()?;
            let channel_type = if is_dm {
                ChannelType::DirectMessage
            } else {
                ChannelType::Home
            };

            if let Some(channel) = state.channel_mut(&channel_id) {
                channel.context_id = context_id;
                channel.name = name;
                channel.topic = topic;
                channel.is_dm = is_dm;
                channel.channel_type = channel_type;
                channel.member_count = member_count;
                channel.last_activity = created_at;
            } else {
                state.upsert_channel(Channel {
                    id: channel_id,
                    context_id,
                    name,
                    topic,
                    channel_type,
                    unread_count: 0,
                    is_dm,
                    member_ids: Vec::new(),
                    member_count,
                    last_message: None,
                    last_message_time: None,
                    last_activity: created_at,
                    last_finalized_epoch: 0,
                });
            }
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
            if let Some(channel) = state.channel_mut(&channel_id) {
                if let Some(context_id) = context_id {
                    channel.context_id = Some(context_id);
                }
                if let Some(name) = name {
                    channel.name = name;
                }
                if topic.is_some() {
                    channel.topic = topic;
                }
                if let Some(member_count) = member_count {
                    channel.member_count = member_count;
                }
                if let Some(member_ids) = member_ids {
                    channel.member_ids = member_ids;
                }
            } else {
                let Some(name) = name else {
                    return Ok(());
                };
                let initial_member_ids: Vec<AuthorityId> = match member_ids {
                    Some(member_ids) => member_ids,
                    None => Vec::new(),
                };
                let initial_member_count = member_count.unwrap_or(1);
                state.upsert_channel(Channel {
                    id: channel_id,
                    context_id,
                    name,
                    topic,
                    channel_type: ChannelType::Home,
                    unread_count: 0,
                    is_dm: false,
                    member_ids: initial_member_ids,
                    member_count: initial_member_count,
                    last_message: None,
                    last_message_time: None,
                    last_activity: 0,
                    last_finalized_epoch: 0,
                });
            }
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
            if !state.has_channel(&channel_id) {
                state.upsert_channel(Channel {
                    id: channel_id,
                    context_id: None,
                    name: channel_id.to_string(),
                    topic: None,
                    channel_type: ChannelType::Home,
                    unread_count: 0,
                    is_dm: false,
                    member_ids: Vec::new(),
                    member_count: 1,
                    last_message: None,
                    last_message_time: None,
                    last_activity: timestamp,
                    last_finalized_epoch: 0,
                });
            }

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
            ChatDelta::ChannelAdded {
                channel_id: channel_id.to_string(),
                context_id: Some(canonical_context.to_string()),
                name: "shared-parity-lab".to_string(),
                topic: Some("canonical-topic".to_string()),
                is_dm: false,
                member_count: 2,
                created_at: 10,
                creator_id: AuthorityId::new_from_entropy([3u8; 32]).to_string(),
            },
        )
        .expect("apply channel added");

        let channel = state.channel(&channel_id).expect("channel must exist");
        assert_eq!(channel.context_id, Some(canonical_context));
        assert_eq!(channel.name, "shared-parity-lab");
        assert_eq!(channel.topic.as_deref(), Some("canonical-topic"));
        assert_eq!(channel.member_count, 2);
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
            },
        )
        .expect("apply channel updated");

        assert!(state.channel(&channel_id).is_none());
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
            ChatDelta::ChannelAdded {
                channel_id: canonical_id.to_string(),
                context_id: Some(ContextId::new_from_entropy([6u8; 32]).to_string()),
                name: "shared-parity-lab".to_string(),
                topic: None,
                is_dm: false,
                member_count: 2,
                created_at: 10,
                creator_id: AuthorityId::new_from_entropy([7u8; 32]).to_string(),
            },
        )
        .expect("apply channel added");

        assert!(state.channel(&stale_id).is_some());
        assert!(state.channel(&canonical_id).is_some());
        assert_eq!(state.channel_count(), 2);
    }

    async fn init_signals_for_test(app_core: &Arc<RwLock<AppCore>>) {
        let mut core = app_core.write().await;
        core.init_signals().await.unwrap();
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

        replace_homes_projection_observed(&app_core, homes.clone())
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

        replace_recovery_projection_observed(&app_core, recovery.clone())
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

        let context_source =
            std::fs::read_to_string(repo_root.join("crates/aura-app/src/workflows/context.rs"))
                .unwrap_or_else(|error| panic!("failed to read context.rs: {error}"));
        assert!(!context_source.contains("async fn homes_state_signal_snapshot("));
        assert!(context_source.contains("homes_signal_snapshot"));

        let settings_source =
            std::fs::read_to_string(repo_root.join("crates/aura-app/src/workflows/settings.rs"))
                .unwrap_or_else(|error| panic!("failed to read settings.rs: {error}"));
        assert!(!settings_source.contains("async fn emit_recovery_state_observed("));
        assert!(!settings_source.contains("core.views_mut().set_recovery("));
        assert!(settings_source.contains("replace_homes_projection_observed"));
        assert!(settings_source.contains("replace_recovery_projection_observed"));
    }
}

/// Observed-only projection update helper for recovery state.
///
/// This helper updates both:
/// 1. ViewState (for futures-signals subscribers)
/// 2. RECOVERY_SIGNAL (for ReactiveEffects subscribers)
///
/// OWNERSHIP: observed-display-update
#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub async fn update_recovery_projection_observed<T>(
    app_core: &Arc<RwLock<AppCore>>,
    update: impl FnOnce(&mut RecoveryState) -> T,
) -> Result<T, AuraError> {
    let (output, state) = {
        let core = app_core.read().await;
        let mut state = core.snapshot().recovery;
        let output = update(&mut state);
        (output, state)
    };

    replace_recovery_projection_observed(app_core, state).await?;

    Ok(output)
}

/// Observed-only projection update helper for contacts state.
///
/// This helper updates both:
/// 1. ViewState (for futures-signals subscribers)
/// 2. CONTACTS_SIGNAL (for ReactiveEffects subscribers)
///
/// OWNERSHIP: observed-display-update
#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub async fn update_contacts_projection_observed<T>(
    app_core: &Arc<RwLock<AppCore>>,
    update: impl FnOnce(&mut ContactsState) -> T,
) -> Result<T, AuraError> {
    let (output, state) = {
        let mut core = app_core.write().await;
        let mut state = core.snapshot().contacts;
        let output = update(&mut state);
        // OWNERSHIP: observed-display-update
        core.views_mut().set_contacts(state.clone());
        (output, state)
    };

    emit_signal(app_core, &*CONTACTS_SIGNAL, state, CONTACTS_SIGNAL_NAME).await?;

    Ok(output)
}

/// Observed-only projection update helper for homes state.
///
/// This helper updates both:
/// 1. ViewState (for futures-signals subscribers)
/// 2. HOMES_SIGNAL (for ReactiveEffects subscribers)
///
/// OWNERSHIP: observed-display-update
#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub async fn update_homes_projection_observed<T>(
    app_core: &Arc<RwLock<AppCore>>,
    update: impl FnOnce(&mut HomesState) -> T,
) -> Result<T, AuraError> {
    let (output, state) = {
        let core = app_core.read().await;
        let mut state = core.snapshot().homes;
        let output = update(&mut state);
        (output, state)
    };

    replace_homes_projection_observed(app_core, state).await?;

    Ok(output)
}

/// Observed-only projection update helper for neighborhood state.
///
/// This helper updates both:
/// 1. ViewState (for futures-signals subscribers)
/// 2. NEIGHBORHOOD_SIGNAL (for ReactiveEffects subscribers)
///
/// OWNERSHIP: observed-display-update
#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub async fn update_neighborhood_projection_observed<T>(
    app_core: &Arc<RwLock<AppCore>>,
    update: impl FnOnce(&mut NeighborhoodState) -> T,
) -> Result<T, AuraError> {
    let (output, state) = {
        let mut core = app_core.write().await;
        let mut state = core.snapshot().neighborhood;
        let output = update(&mut state);
        // OWNERSHIP: observed-display-update
        core.views_mut().set_neighborhood(state.clone());
        (output, state)
    };

    // Also emit to NEIGHBORHOOD_SIGNAL for ReactiveEffects subscribers
    emit_signal(
        app_core,
        &*NEIGHBORHOOD_SIGNAL,
        state,
        NEIGHBORHOOD_SIGNAL_NAME,
    )
    .await?;

    Ok(output)
}
