//! Executes [`Request`]s through the shared `aura_app::ui::workflows`.
//!
//! This is the only place CLI and RPC commands touch the account. Every arm
//! calls the same workflow functions the TUI and web call; nothing here
//! reaches agent services or local files.

use super::request::{ExportFormat, InviteRole, Request};
use super::response::{
    AccountView, AmpChannelView, AuthorityView, ChannelView, ContactView, InvitationView,
    MessageView, OperationView, RecoveryView, Response,
};
use super::{CommandError, ErrorCode};
use crate::handlers::AuraEffectSystem;
use async_lock::RwLock;
use aura_app::ui::contract::{
    SemanticFailureCode, WorkflowTerminalOutcome, WorkflowTerminalStatus,
};
use aura_app::ui::signals::SyncStatus;
use aura_app::ui::types::{
    AppCore, Channel, InvitationBridgeStatus, InvitationBridgeType, InvitationInfo, Message,
};
use aura_app::ui::workflows::strong_command::CommandResolver;
use aura_app::ui::workflows::{
    admin, amp, context, invitation, messaging, network, query, recovery, settings, slash_commands,
    snapshot, sync, time,
};
use aura_core::types::identifiers::{
    AccountId, AuthorityId, CeremonyId, ChannelId, ContextId, InvitationId,
};
use aura_core::types::FrostThreshold;
use aura_core::AuraError;
use std::str::FromStr;
use std::sync::Arc;

/// What a request runs against: the app core (workflows) and the effect
/// system the maintenance workflows take.
#[derive(Clone)]
pub struct CommandContext {
    app_core: Arc<RwLock<AppCore>>,
    effects: Arc<AuraEffectSystem>,
    authority_id: AuthorityId,
}

impl CommandContext {
    pub fn new(
        app_core: Arc<RwLock<AppCore>>,
        effects: Arc<AuraEffectSystem>,
        authority_id: AuthorityId,
    ) -> Self {
        Self {
            app_core,
            effects,
            authority_id,
        }
    }

    /// The app core the workflows run on.
    #[must_use]
    pub fn app_core(&self) -> &Arc<RwLock<AppCore>> {
        &self.app_core
    }
}

fn parse<T: FromStr>(label: &str, raw: &str) -> Result<T, CommandError>
where
    T::Err: std::fmt::Display,
{
    raw.trim()
        .parse::<T>()
        .map_err(|e| CommandError::invalid(format!("invalid {label} {raw}: {e}")))
}

/// Authorities are printed as `authority-<uuid>`; accept that or a bare UUID.
fn parse_authority(label: &str, raw: &str) -> Result<AuthorityId, CommandError> {
    let raw = raw.trim();
    AuthorityId::from_str(raw)
        .or_else(|_| uuid::Uuid::from_str(raw).map(AuthorityId::from_uuid))
        .map_err(|e| CommandError::invalid(format!("invalid {label} {raw}: {e}")))
}

fn parse_authorities(label: &str, raw: &[String]) -> Result<Vec<AuthorityId>, CommandError> {
    raw.iter()
        .map(|value| parse_authority(label, value))
        .collect()
}

/// A workflow failure, classified by its typed semantic failure when the
/// workflow published one.
fn terminal_failure(error: AuraError, terminal: Option<&WorkflowTerminalStatus>) -> CommandError {
    let mut failure = CommandError::from(error);
    if let Some(semantic) = terminal.and_then(|t| t.status.error.clone()) {
        failure.code = match semantic.code {
            SemanticFailureCode::InvalidArgument | SemanticFailureCode::UnsupportedCommand => {
                ErrorCode::InvalidInput
            }
            SemanticFailureCode::NotFound => ErrorCode::NotFound,
            SemanticFailureCode::PermissionDenied
            | SemanticFailureCode::NotMember
            | SemanticFailureCode::Muted
            | SemanticFailureCode::Banned
            | SemanticFailureCode::BudgetExceeded => ErrorCode::PermissionDenied,
            SemanticFailureCode::OperationTimedOut => ErrorCode::Timeout,
            SemanticFailureCode::Unavailable
            | SemanticFailureCode::PeerChannelNotEstablished
            | SemanticFailureCode::ChannelBootstrapUnavailable
            | SemanticFailureCode::DeliveryReadinessNotReached
            | SemanticFailureCode::ContactLinkDidNotConverge => ErrorCode::Unavailable,
            _ => failure.code,
        };
        failure.failure = Some(semantic);
    }
    failure
}

/// Unwrap a `*_with_terminal_status` outcome.
fn settle<T>(
    outcome: WorkflowTerminalOutcome<T>,
) -> Result<(T, Option<OperationView>), CommandError> {
    match outcome.result {
        Ok(value) => Ok((value, outcome.terminal.map(OperationView::from))),
        Err(error) => Err(terminal_failure(error, outcome.terminal.as_ref())),
    }
}

async fn now_ms(ctx: &CommandContext) -> Result<u64, CommandError> {
    time::current_time_ms(&ctx.app_core)
        .await
        .map_err(|e| CommandError::from(AuraError::from(e)))
}

async fn channel(ctx: &CommandContext, selector: &str) -> Result<Channel, CommandError> {
    Ok(messaging::resolve_channel(&ctx.app_core, selector).await?)
}

pub(crate) fn channel_view(channel: &Channel, with_members: bool) -> ChannelView {
    ChannelView {
        channel_id: channel.id.to_string(),
        context_id: channel.context_id.map(|c| c.to_string()),
        name: channel.name.clone(),
        topic: channel.topic.clone(),
        is_dm: channel.is_dm,
        member_count: channel.member_count,
        unread: channel.unread_count,
        members: if with_members {
            channel.member_ids.iter().map(ToString::to_string).collect()
        } else {
            Vec::new()
        },
    }
}

pub(crate) fn message_view(message: &Message) -> MessageView {
    MessageView {
        message_id: message.id.clone(),
        channel_id: message.channel_id.to_string(),
        sender_id: message.sender_id.to_string(),
        sender_name: message.sender_name.clone(),
        content: message.content.clone(),
        timestamp_ms: message.timestamp,
        is_own: message.is_own,
    }
}

fn invitation_view(info: &InvitationInfo) -> InvitationView {
    let kind = match &info.invitation_type {
        InvitationBridgeType::Contact { .. } => "contact".to_string(),
        InvitationBridgeType::Guardian { subject_authority } => {
            format!("guardian for {subject_authority}")
        }
        InvitationBridgeType::Channel {
            home_id,
            nickname_suggestion,
            ..
        } => match nickname_suggestion {
            Some(name) => format!("channel {name} ({home_id})"),
            None => format!("channel {home_id}"),
        },
        InvitationBridgeType::DeviceEnrollment { device_id, .. } => {
            format!("device enrollment for {device_id}")
        }
    };
    let status = match info.status {
        InvitationBridgeStatus::Pending => "pending",
        InvitationBridgeStatus::Accepted => "accepted",
        InvitationBridgeStatus::Declined => "declined",
        InvitationBridgeStatus::Cancelled => "cancelled",
        InvitationBridgeStatus::Expired => "expired",
    };
    InvitationView {
        invitation_id: info.invitation_id.to_string(),
        kind,
        sender_id: info.sender_id.to_string(),
        receiver_id: info.receiver_id.to_string(),
        status: status.to_string(),
        expires_at_ms: info.expires_at_ms,
        message: info.message.clone(),
    }
}

fn export_body(format: ExportFormat, messages: &[MessageView]) -> Result<String, CommandError> {
    Ok(match format {
        ExportFormat::Json => serde_json::to_string_pretty(messages)
            .map_err(|e| CommandError::new(ErrorCode::Failed, format!("encode export: {e}")))?,
        ExportFormat::Csv => {
            let quote = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
            let mut rows = vec![
                "message_id,channel_id,sender_id,sender_name,timestamp_ms,content".to_string(),
            ];
            rows.extend(messages.iter().map(|m| {
                [
                    quote(&m.message_id),
                    quote(&m.channel_id),
                    quote(&m.sender_id),
                    quote(&m.sender_name),
                    m.timestamp_ms.to_string(),
                    quote(&m.content),
                ]
                .join(",")
            }));
            rows.join("\n")
        }
        ExportFormat::Text => messages
            .iter()
            .map(|m| format!("[{}] {}: {}", m.timestamp_ms, m.sender_name, m.content))
            .collect::<Vec<_>>()
            .join("\n"),
    })
}

/// Run a chat slash command through the shared slash-command workflow, the
/// path the TUI's chat input takes.
async fn slash(
    ctx: &CommandContext,
    input: &str,
    channel_hint: Option<String>,
) -> Result<Response, CommandError> {
    let resolver = CommandResolver::default();
    let prepared = slash_commands::prepare(
        &resolver,
        &ctx.app_core,
        input,
        channel_hint.as_deref(),
        Some(ctx.authority_id),
    )
    .await
    .map_err(|e| CommandError::invalid(e.to_string()))?;
    let result = slash_commands::execute(&ctx.app_core, &prepared).await?;
    let feedback = slash_commands::feedback_for_execution_result(&prepared, &result);
    Ok(done(feedback.message, None))
}

fn done(summary: impl Into<String>, operation: Option<OperationView>) -> Response {
    Response::Done {
        summary: summary.into(),
        operation,
    }
}

/// Run one request.
pub async fn execute(ctx: &CommandContext, request: Request) -> Result<Response, CommandError> {
    let app = &ctx.app_core;
    match request {
        Request::Status => {
            settings::refresh_settings_from_runtime(app).await?;
            let s = settings::get_settings(app).await?;
            Ok(Response::Account(AccountView {
                authority_id: ctx.authority_id.to_string(),
                nickname: s.nickname_suggestion,
                threshold_k: s.threshold_k,
                threshold_n: s.threshold_n,
                devices: s.devices.len(),
                contacts: s.contact_count,
            }))
        }
        Request::AuthorityList => {
            settings::refresh_settings_from_runtime(app).await?;
            let s = settings::get_settings(app).await?;
            let mut authorities: Vec<AuthorityView> = s
                .authorities
                .iter()
                .map(|a| AuthorityView {
                    authority_id: a.id.to_string(),
                    nickname: a.nickname_suggestion.clone(),
                    current: a.id == ctx.authority_id,
                })
                .collect();
            if !authorities.iter().any(|a| a.current) {
                authorities.insert(
                    0,
                    AuthorityView {
                        authority_id: ctx.authority_id.to_string(),
                        nickname: s.authority_nickname,
                        current: true,
                    },
                );
            }
            Ok(Response::Authorities(authorities))
        }

        Request::ContactList => {
            let mut contacts: Vec<ContactView> = query::list_contacts(app)
                .await
                .into_iter()
                .map(|c| ContactView {
                    authority_id: c.id.to_string(),
                    nickname: if c.nickname.is_empty() {
                        c.nickname_suggestion.clone().unwrap_or_default()
                    } else {
                        c.nickname.clone()
                    },
                    is_guardian: c.is_guardian,
                    is_member: c.is_member,
                })
                .collect();
            contacts.sort_by(|a, b| a.authority_id.cmp(&b.authority_id));
            Ok(Response::Contacts(contacts))
        }

        Request::ChatList => {
            let chat = messaging::observed_chat(app).await;
            let mut channels: Vec<ChannelView> = chat
                .all_channels()
                .map(|c| channel_view(c, false))
                .collect();
            channels.sort_by(|a, b| a.name.cmp(&b.name).then(a.channel_id.cmp(&b.channel_id)));
            Ok(Response::Channels(channels))
        }
        Request::ChatShow { channel: selector } => Ok(Response::Channel(channel_view(
            &channel(ctx, &selector).await?,
            true,
        ))),
        Request::ChatHistory {
            channel: selector,
            limit,
            sender,
        } => {
            let channel = channel(ctx, &selector).await?;
            let sender = sender.map(|s| parse_authority("sender", &s)).transpose()?;
            let messages = messaging::channel_history(app, channel.id, limit, sender).await;
            Ok(Response::Messages(
                messages.iter().map(message_view).collect(),
            ))
        }
        Request::ChatSend {
            channel: selector,
            message,
        } => {
            let channel = channel(ctx, &selector).await?;
            let (message_id, operation) = settle(
                messaging::send_message_now_with_terminal_status(app, channel.id, &message, None)
                    .await,
            )?;
            Ok(Response::MessageSent {
                channel_id: channel.id.to_string(),
                message_id,
                operation,
            })
        }
        Request::ChatCreate {
            name,
            topic,
            members,
        } => {
            let members: Vec<String> = parse_authorities("member", &members)?
                .iter()
                .map(ToString::to_string)
                .collect();
            let created = messaging::create_channel_with_authoritative_binding(
                app,
                &name,
                topic,
                &members,
                0,
                now_ms(ctx).await?,
            )
            .await?;
            Ok(Response::ChannelCreated {
                channel_id: created.channel_id.to_string(),
                context_id: created
                    .context_id
                    .map(|c| c.to_string())
                    .unwrap_or_default(),
            })
        }
        Request::ChatInvite {
            channel: selector,
            authority,
        } => {
            let channel = channel(ctx, &selector).await?;
            let receiver = parse_authority("authority", &authority)?;
            let invitation_id =
                messaging::invite_authority_to_channel(app, receiver, channel.id, None, None)
                    .await?;
            Ok(done(
                format!("Invited {receiver} to {} ({invitation_id})", channel.name),
                None,
            ))
        }
        Request::ChatLeave { channel: selector } => {
            let channel = channel(ctx, &selector).await?;
            messaging::leave_channel(app, channel.id).await?;
            Ok(done(format!("Left {}", channel.name), None))
        }
        Request::ChatUpdate {
            channel: selector,
            name,
            topic,
        } => {
            if name.is_none() && topic.is_none() {
                return Err(CommandError::invalid("pass --name or --topic"));
            }
            let channel = channel(ctx, &selector).await?;
            messaging::update_channel_info(app, channel.id, name, topic, now_ms(ctx).await?)
                .await?;
            Ok(done(format!("Updated {}", channel.name), None))
        }
        Request::ChatSearch {
            query,
            channel: selector,
            sender,
            limit,
        } => {
            let channel_id = match selector {
                Some(selector) => Some(channel(ctx, &selector).await?.id),
                None => None,
            };
            let sender = sender.map(|s| parse_authority("sender", &s)).transpose()?;
            let found = messaging::search_messages(app, &query, channel_id, sender, limit).await;
            Ok(Response::Messages(found.iter().map(message_view).collect()))
        }
        Request::ChatExport {
            channel: selector,
            format,
        } => {
            let channel = channel(ctx, &selector).await?;
            let messages: Vec<MessageView> =
                messaging::channel_history(app, channel.id, None, None)
                    .await
                    .iter()
                    .map(message_view)
                    .collect();
            Ok(Response::Export {
                format,
                body: export_body(format, &messages)?,
            })
        }

        Request::InviteCreate {
            invitee,
            role,
            channel: selector,
            ttl_secs,
        } => {
            let receiver = parse_authority("invitee", &invitee)?;
            let ttl_ms = ttl_secs.map(|s| s.saturating_mul(1000));
            let invitation_id = match role {
                InviteRole::Contact => {
                    invitation::create_contact_invitation(app, receiver, None, None, None, ttl_ms)
                        .await?
                        .invitation_id()
                        .clone()
                }
                InviteRole::Guardian => invitation::create_guardian_invitation(
                    app,
                    receiver,
                    ctx.authority_id,
                    None,
                    ttl_ms,
                )
                .await?
                .invitation_id()
                .clone(),
                InviteRole::Channel => {
                    let selector = selector.ok_or_else(|| {
                        CommandError::invalid("a channel invitation needs --channel")
                    })?;
                    let channel = channel(ctx, &selector).await?;
                    messaging::invite_authority_to_channel(app, receiver, channel.id, None, ttl_ms)
                        .await?
                }
            };
            let code = invitation::export_invitation(app, &invitation_id).await?;
            Ok(Response::InvitationCreated {
                invitation_id: invitation_id.to_string(),
                code,
            })
        }
        Request::InviteAccept { invitation_id } => {
            let (info, operation) = settle(
                invitation::accept_invitation_by_str_with_terminal_status(
                    app,
                    &invitation_id,
                    None,
                )
                .await,
            )?;
            Ok(done(
                format!(
                    "Accepted invitation {} from {}",
                    info.invitation_id, info.sender_id
                ),
                operation,
            ))
        }
        Request::InviteDecline { invitation_id } => {
            let ((), operation) = settle(
                invitation::decline_invitation_by_str_with_terminal_status(
                    app,
                    &invitation_id,
                    None,
                )
                .await,
            )?;
            Ok(done(
                format!("Declined invitation {invitation_id}"),
                operation,
            ))
        }
        Request::InviteCancel { invitation_id } => {
            let ((), operation) = settle(
                invitation::cancel_invitation_by_str_with_terminal_status(
                    app,
                    &invitation_id,
                    None,
                )
                .await,
            )?;
            Ok(done(
                format!("Cancelled invitation {invitation_id}"),
                operation,
            ))
        }
        Request::InviteList => {
            let pending = invitation::list_pending_invitations(app).await?;
            Ok(Response::Invitations(
                pending.iter().map(invitation_view).collect(),
            ))
        }
        Request::InviteExport { invitation_id } => {
            let id = InvitationId::new(invitation_id.trim().to_string());
            let code = invitation::export_invitation(app, &id).await?;
            Ok(Response::InvitationCode {
                invitation_id: id.to_string(),
                code,
            })
        }
        Request::InviteImport { code } => {
            let handle = invitation::import_invitation_details(app, code.trim())
                .await
                .map_err(|e| {
                    let mut error = CommandError::from(e);
                    if error.code == ErrorCode::Failed {
                        error.code = ErrorCode::InvalidInput;
                    }
                    error
                })?;
            Ok(Response::Invitation(invitation_view(handle.info())))
        }

        Request::HomeCreate { name } => {
            let home_id = context::create_home(app, name, None).await?;
            Ok(Response::HomeCreated {
                home_id: home_id.to_string(),
            })
        }
        Request::HomeInvite { authority } => {
            let invitee = parse_authority("authority", &authority)?;
            let home = context::current_home_id(app).await?;
            slash(
                ctx,
                &format!("/homeinvite {invitee}"),
                Some(home.to_string()),
            )
            .await
        }
        Request::HomeAccept => {
            let (invitation_id, operation) = settle(
                invitation::accept_pending_channel_invitation_with_terminal_status(app, None).await,
            )?;
            Ok(done(
                format!("Accepted invitation {invitation_id}"),
                operation,
            ))
        }
        Request::Slash { command, channel } => {
            let hint = match channel {
                Some(selector) => Some(self::channel(ctx, &selector).await?.id.to_string()),
                None => None,
            };
            slash(ctx, &command, hint).await
        }

        Request::RecoveryStart {
            guardians,
            threshold,
        } => {
            let guardians = parse_authorities("guardian", &guardians)?;
            let threshold = FrostThreshold::new(threshold)
                .map_err(|e| CommandError::invalid(format!("invalid threshold: {e:?}")))?;
            let ceremony = recovery::start_recovery(app, guardians, threshold).await?;
            Ok(Response::RecoveryStarted {
                ceremony_id: ceremony.to_string(),
            })
        }
        Request::RecoveryApprove { ceremony_id } => {
            recovery::approve_recovery(app, &CeremonyId::new(ceremony_id.trim().to_string()))
                .await?;
            Ok(done(format!("Approved recovery {ceremony_id}"), None))
        }
        Request::RecoveryDispute {
            ceremony_id,
            reason,
        } => {
            recovery::dispute_recovery(
                app,
                &CeremonyId::new(ceremony_id.trim().to_string()),
                reason,
            )
            .await?;
            Ok(done(format!("Disputed recovery {ceremony_id}"), None))
        }
        Request::RecoveryStatus => {
            let state = recovery::get_recovery_status(app).await?;
            let mut guardians: Vec<String> =
                state.guardian_ids().map(ToString::to_string).collect();
            guardians.sort();
            let active = state.active_recovery();
            Ok(Response::Recovery(RecoveryView {
                guardians,
                threshold: state.threshold(),
                active_ceremony: active.map(|p| p.id.to_string()),
                approvals: active
                    .map(|p| format!("{} of {}", p.approvals_received, p.approvals_required)),
            }))
        }

        Request::ContextInspect { context } => {
            let context: ContextId = parse("context", &context)?;
            let chat = messaging::observed_chat(app).await;
            let mut channels: Vec<ChannelView> = chat
                .all_channels()
                .filter(|c| c.context_id == Some(context))
                .map(|c| channel_view(c, true))
                .collect();
            channels.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(Response::Channels(channels))
        }

        Request::SyncStatus => {
            let (status, detail) = match sync::get_sync_status(app).await {
                SyncStatus::Idle => ("idle", None),
                SyncStatus::Syncing { progress } => ("syncing", Some(format!("{progress}%"))),
                SyncStatus::Synced => ("synced", None),
                SyncStatus::Failed { message } => ("failed", Some(message)),
            };
            Ok(Response::Sync {
                status: status.to_string(),
                detail,
            })
        }
        Request::SyncOnce { peers } => {
            let peers = parse_authorities("peer", &peers)?;
            if peers.is_empty() {
                sync::force_sync(app).await?;
            } else {
                for peer in peers {
                    sync::request_state(app, peer).await?;
                }
            }
            Ok(done("Sync requested", None))
        }
        Request::PeerAdd { peer } => Ok(Response::Peers {
            connected: network::add_peer(app, parse_authority("peer", &peer)?).await?,
        }),
        Request::PeerRemove { peer } => Ok(Response::Peers {
            connected: network::remove_peer(app, &parse_authority("peer", &peer)?).await?,
        }),

        Request::AmpInspect { context, channel } => {
            let context: ContextId = parse("context", &context)?;
            let channel: ChannelId = parse("channel", &channel)?;
            let state = amp::inspect_channel(ctx.effects.as_ref(), context, channel).await?;
            Ok(Response::AmpChannel(AmpChannelView {
                context_id: context.to_string(),
                channel_id: channel.to_string(),
                epoch: state.chan_epoch,
                generation: state.current_gen,
                last_checkpoint_generation: state.last_checkpoint_gen,
                skip_window: u64::from(state.skip_window),
                pending_bump: state
                    .pending_bump
                    .map(|b| format!("{} -> {} ({})", b.parent_epoch, b.new_epoch, b.bump_id)),
            }))
        }
        Request::AmpBump { context, channel } => {
            let context: ContextId = parse("context", &context)?;
            let channel: ChannelId = parse("channel", &channel)?;
            let proposal = amp::propose_bump(ctx.effects.as_ref(), context, channel).await?;
            Ok(Response::AmpBumpProposed {
                parent_epoch: proposal.parent_epoch,
                new_epoch: proposal.new_epoch,
                bump_id: proposal.bump_id.to_string(),
            })
        }
        Request::AmpCheckpoint { context, channel } => {
            let context: ContextId = parse("context", &context)?;
            let channel: ChannelId = parse("channel", &channel)?;
            let checkpoint = amp::create_checkpoint(ctx.effects.as_ref(), context, channel).await?;
            Ok(Response::AmpCheckpoint {
                epoch: checkpoint.chan_epoch,
                base_generation: checkpoint.base_gen,
            })
        }

        Request::SnapshotPropose => Ok(Response::SnapshotProposed {
            proposal_id: snapshot::propose_snapshot(ctx.effects.as_ref(), ctx.authority_id).await?,
        }),
        Request::AdminReplace {
            account,
            new_admin,
            activation_epoch,
        } => {
            let account: AccountId = parse("account", &account)?;
            let new_admin = parse_authority("new admin", &new_admin)?;
            admin::replace_admin(
                ctx.effects.as_ref(),
                ctx.authority_id,
                account,
                new_admin,
                activation_epoch,
            )
            .await?;
            Ok(done(
                format!("Admin {new_admin} activates at epoch {activation_epoch}"),
                None,
            ))
        }
    }
}
