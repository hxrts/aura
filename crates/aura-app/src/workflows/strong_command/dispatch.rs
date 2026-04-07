#![allow(missing_docs)]

#[cfg(feature = "signals")]
use super::consistency::scope_channel_id;
#[cfg(feature = "signals")]
use super::plan::{CommandPlan, CommandScope, MembershipPlan, ModerationPlan, ModeratorPlan};
#[cfg(feature = "signals")]
use super::resolved_refs::ResolvedCommand;
#[cfg(feature = "signals")]
use crate::workflows::{context, invitation, messaging, moderation, moderator, query, settings};
#[cfg(feature = "signals")]
use crate::AppCore;
#[cfg(feature = "signals")]
use async_lock::RwLock;
#[cfg(feature = "signals")]
use aura_core::types::identifiers::ChannelId;
#[cfg(feature = "signals")]
use aura_core::AuraError;
#[cfg(feature = "signals")]
use std::sync::Arc;

#[cfg(feature = "signals")]
pub(super) async fn execute_membership(
    app_core: &Arc<RwLock<AppCore>>,
    plan: &CommandPlan<MembershipPlan>,
) -> Result<Option<String>, AuraError> {
    match &plan.operation.command {
        ResolvedCommand::Join {
            channel_name,
            channel,
        } => match channel {
            super::ChannelResolveOutcome::Existing(channel) => {
                let authoritative_channel =
                    messaging::require_authoritative_context_id_for_channel(
                        app_core,
                        channel.channel_id().0,
                    )
                    .await
                    .map(|context_id| {
                        messaging::authoritative_channel_ref(channel.channel_id().0, context_id)
                    })?;
                messaging::join_channel(app_core, authoritative_channel).await?;
                Ok(())
            }
            super::ChannelResolveOutcome::WillCreate { .. } => {
                messaging::join_channel_by_name(app_core, channel_name).await?;
                Ok(())
            }
        },
        ResolvedCommand::Leave => {
            let channel_id = scope_channel_id(&plan.scope, "leave")?;
            messaging::leave_channel(app_core, channel_id.0).await?;
            Ok(())
        }
        _ => Err(AuraError::invalid("invalid membership command")),
    }?;
    Ok(Some("membership updated".to_string()))
}

#[cfg(feature = "signals")]
pub(super) async fn execute_moderation(
    app_core: &Arc<RwLock<AppCore>>,
    plan: &CommandPlan<ModerationPlan>,
) -> Result<Option<String>, AuraError> {
    let timestamp_ms = crate::workflows::time::current_time_ms(app_core).await?;
    match &plan.operation.command {
        ResolvedCommand::Kick { target, reason } => {
            let channel_id = scope_channel_id(&plan.scope, "kick")?;
            moderation::kick_user_resolved(
                app_core,
                channel_id.0,
                target.0,
                reason.as_deref(),
                timestamp_ms,
            )
            .await?;
            Ok(Some("kick applied".to_string()))
        }
        ResolvedCommand::Ban { target, reason } => {
            let _ = optional_scope_channel_id(&plan.scope);
            moderation::ban_user_resolved(app_core, target.0, reason.as_deref(), timestamp_ms)
                .await?;
            Ok(Some("ban applied".to_string()))
        }
        ResolvedCommand::Unban { target } => {
            let _ = optional_scope_channel_id(&plan.scope);
            moderation::unban_user_resolved(app_core, target.0).await?;
            Ok(Some("unban applied".to_string()))
        }
        ResolvedCommand::Mute { target, duration } => {
            let _ = optional_scope_channel_id(&plan.scope);
            moderation::mute_user_resolved(
                app_core,
                target.0,
                duration.map(|value| value.as_secs()),
                timestamp_ms,
            )
            .await?;
            Ok(Some("mute applied".to_string()))
        }
        ResolvedCommand::Unmute { target } => {
            let _ = optional_scope_channel_id(&plan.scope);
            moderation::unmute_user_resolved(app_core, target.0).await?;
            Ok(Some("unmute applied".to_string()))
        }
        ResolvedCommand::Invite { target } => {
            let channel_id = scope_channel_id(&plan.scope, "invite")?;
            messaging::invite_authority_to_channel(app_core, target.0, channel_id.0, None, None)
                .await?;
            Ok(Some("invitation sent".to_string()))
        }
        _ => Err(AuraError::invalid("invalid moderation command")),
    }
}

#[cfg(feature = "signals")]
pub(super) async fn execute_moderator(
    app_core: &Arc<RwLock<AppCore>>,
    plan: &CommandPlan<ModeratorPlan>,
) -> Result<Option<String>, AuraError> {
    match &plan.operation.command {
        ResolvedCommand::Op { target } => {
            moderator::grant_moderator_resolved(
                app_core,
                optional_scope_channel_id(&plan.scope),
                target.0,
            )
            .await?;
            Ok(Some("moderator granted".to_string()))
        }
        ResolvedCommand::Deop { target } => moderator::revoke_moderator_resolved(
            app_core,
            optional_scope_channel_id(&plan.scope),
            target.0,
        )
        .await
        .map(|_| Some("moderator revoked".to_string())),
        ResolvedCommand::Mode { channel, flags, .. } => {
            settings::set_channel_mode_resolved(app_core, channel.channel_id().0, flags.clone())
                .await
                .map(|_| Some("channel mode updated".to_string()))
        }
        _ => Err(AuraError::invalid("invalid moderator command")),
    }
}

#[cfg(feature = "signals")]
pub(super) async fn execute_general(
    app_core: &Arc<RwLock<AppCore>>,
    plan: &CommandPlan<ResolvedCommand>,
) -> Result<Option<String>, AuraError> {
    match &plan.operation {
        ResolvedCommand::Msg { target, text } => {
            let timestamp_ms = crate::workflows::time::local_first_timestamp_ms(
                app_core,
                "strong-command-msg",
                &[text.as_str()],
            )
            .await?;
            messaging::send_direct_message_to_authority(app_core, target.0, text, timestamp_ms)
                .await?;
            Ok(Some("direct message sent".to_string()))
        }
        ResolvedCommand::Me { action } => {
            let timestamp_ms = crate::workflows::time::local_first_timestamp_ms(
                app_core,
                "strong-command-me",
                &[action.as_str()],
            )
            .await?;
            let channel_id = scope_channel_id(&plan.scope, "me")?;
            messaging::send_action(app_core, channel_id.0, action, timestamp_ms).await?;
            Ok(Some("action sent".to_string()))
        }
        ResolvedCommand::Nick { name } => settings::update_nickname(app_core, name.clone())
            .await
            .map(|_| Some("nickname updated".to_string())),
        ResolvedCommand::Who => {
            let channel_id = scope_channel_id(&plan.scope, "who")?;
            let participants =
                query::list_participants_by_channel_id(app_core, channel_id.0).await?;
            let details = if participants.is_empty() {
                "No participants".to_string()
            } else {
                participants.join(", ")
            };
            Ok(Some(details))
        }
        ResolvedCommand::Whois { target } => {
            let contact = query::get_user_info_by_authority_id(app_core, target.0).await?;
            let id = contact.id.to_string();
            let name = if !contact.nickname.is_empty() {
                contact.nickname
            } else if let Some(value) = contact.nickname_suggestion {
                value
            } else {
                id.chars().take(8).collect::<String>() + "..."
            };
            Ok(Some(format!("User: {name} ({id})")))
        }
        ResolvedCommand::Help { .. } => Ok(None),
        ResolvedCommand::Neighborhood { name } => {
            context::create_neighborhood(app_core, name.clone()).await?;
            Ok(Some("neighborhood updated".to_string()))
        }
        ResolvedCommand::NhAdd { home_id } => context::add_home_to_neighborhood(app_core, home_id)
            .await
            .map(|_| Some("home added to neighborhood".to_string())),
        ResolvedCommand::NhLink { home_id } => context::link_home_one_hop_link(app_core, home_id)
            .await
            .map(|_| Some("home one_hop_link linked".to_string())),
        ResolvedCommand::HomeInvite { target } => {
            let home_id = current_home_id_string(app_core).await?;
            invitation::create_channel_invitation(
                app_core, target.0, home_id, None, None, None, None, None, None, None, None,
            )
            .await?;
            Ok(Some("home invitation sent".to_string()))
        }
        ResolvedCommand::HomeAccept => invitation::accept_pending_channel_invitation(app_core)
            .await
            .map(|_| Some("home invitation accepted".to_string())),
        ResolvedCommand::Topic { text } => {
            let timestamp_ms = crate::workflows::time::current_time_ms(app_core).await?;
            let channel_id = scope_channel_id(&plan.scope, "topic")?;
            messaging::set_topic(app_core, channel_id.0, text, timestamp_ms).await?;
            Ok(Some("topic updated".to_string()))
        }
        ResolvedCommand::Pin { message_id } => moderation::pin_message(app_core, message_id)
            .await
            .map(|_| Some("message pinned".to_string())),
        ResolvedCommand::Unpin { message_id } => moderation::unpin_message(app_core, message_id)
            .await
            .map(|_| Some("message unpinned".to_string())),
        ResolvedCommand::Join { .. }
        | ResolvedCommand::Leave
        | ResolvedCommand::Kick { .. }
        | ResolvedCommand::Ban { .. }
        | ResolvedCommand::Unban { .. }
        | ResolvedCommand::Mute { .. }
        | ResolvedCommand::Unmute { .. }
        | ResolvedCommand::Invite { .. }
        | ResolvedCommand::Op { .. }
        | ResolvedCommand::Deop { .. }
        | ResolvedCommand::Mode { .. } => {
            Err(AuraError::invalid("command requires specialized plan"))
        }
    }
}

#[cfg(feature = "signals")]
fn optional_scope_channel_id(scope: &CommandScope) -> Option<ChannelId> {
    match scope {
        CommandScope::Channel { channel_id, .. } => Some(channel_id.0),
        _ => None,
    }
}

#[cfg(feature = "signals")]
async fn current_home_id_string(app_core: &Arc<RwLock<AppCore>>) -> Result<String, AuraError> {
    let home_id = context::current_home_id(app_core).await?;
    Ok(home_id.to_string())
}
