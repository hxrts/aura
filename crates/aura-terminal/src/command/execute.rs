//! Executes [`Request`]s through the shared `aura_app::ui::workflows`.
//!
//! This is the only place CLI and RPC commands touch the account. Every arm
//! calls the same workflow functions the TUI and web call; nothing here
//! reaches agent services or local files.

use super::request::{ExportFormat, InviteRole, Request};
use super::response::{
    AccountView, AmpChannelView, AuthorityView, ChannelView, ContactView, InvitationView,
    MessageView, NotificationView, OperationView, RecoveryView, Response, SettingsView,
};
use super::{CommandError, ErrorCode};
use crate::handlers::AuraEffectSystem;
use async_lock::RwLock;
use aura_app::ui::contract::{
    SemanticFailureCode, WorkflowTerminalOutcome, WorkflowTerminalStatus,
};
use aura_app::ui::signals::{SyncStatus, CONTACTS_SIGNAL};
use aura_app::ui::types::{
    format_budget_status, AccessLevel, AppCore, Channel, Contact, ContactRelationshipState,
    ContactsState, InvitationBridgeStatus, InvitationBridgeType, InvitationInfo, InvitationsState,
    Message, ReadReceiptPolicy, RecoveryState,
};
use aura_app::ui::workflows::signals::read_signal_or_default;
use aura_app::ui::workflows::strong_command::CommandResolver;
use aura_app::ui::workflows::{
    access, admin, amp, budget, ceremonies, contacts, context, invitation, messaging, moderation,
    moderator, network, query, recovery, settings, slash_commands, snapshot, sync, system, time,
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

fn contact_view(c: &Contact) -> ContactView {
    ContactView {
        authority_id: c.id.to_string(),
        nickname: if c.nickname.is_empty() {
            c.nickname_suggestion.clone().unwrap_or_default()
        } else {
            c.nickname.clone()
        },
        is_guardian: c.is_guardian,
        is_member: c.is_member,
    }
}

/// The contact a user named (nickname or authority), as its authority string
/// for the contact workflows.
async fn contact_id(ctx: &CommandContext, target: &str) -> Result<String, CommandError> {
    Ok(query::resolve_contact(&ctx.app_core, target)
        .await?
        .id
        .to_string())
}

async fn current_home(ctx: &CommandContext) -> Result<String, CommandError> {
    Ok(context::current_home_id(&ctx.app_core).await?.to_string())
}

fn notifications(
    contacts: &ContactsState,
    invitations: &InvitationsState,
    recovery: &RecoveryState,
) -> Vec<NotificationView> {
    let mut items: Vec<NotificationView> = contacts
        .all_contacts()
        .filter(|c| c.relationship_state == ContactRelationshipState::PendingInbound)
        .map(|c| NotificationView {
            kind: "friend_request".into(),
            id: c.id.to_string(),
            title: format!("Friend request from {}", contact_view(c).nickname),
        })
        .collect();
    items.extend(invitations.all_pending().iter().map(|i| NotificationView {
        kind: "invitation_received".into(),
        id: i.id.clone(),
        title: format!("{:?} invitation from {}", i.invitation_type, i.from_name),
    }));
    items.extend(invitations.all_sent().iter().map(|i| NotificationView {
        kind: "invitation_sent".into(),
        id: i.id.clone(),
        title: format!(
            "{:?} invitation to {}",
            i.invitation_type,
            i.to_name.clone().unwrap_or_default()
        ),
    }));
    items.extend(
        recovery
            .pending_requests()
            .iter()
            .map(|p| NotificationView {
                kind: "recovery_request".into(),
                id: p.id.to_string(),
                title: format!(
                    "Recovery for {} ({} of {} approvals)",
                    p.account_id, p.approvals_received, p.approvals_required
                ),
            }),
    );
    items
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
                .iter()
                .map(contact_view)
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
        Request::InviteImport { code, accept } => {
            let handle = invitation::import_invitation_details(app, code.trim())
                .await
                .map_err(|e| {
                    let mut error = CommandError::from(e);
                    if error.code == ErrorCode::Failed {
                        error.code = ErrorCode::InvalidInput;
                    }
                    error
                })?;
            let view = invitation_view(handle.info());
            if !accept {
                return Ok(Response::Invitation(view));
            }
            let ((), operation) = settle(
                invitation::accept_imported_invitation_with_terminal_status(app, handle, None)
                    .await,
            )?;
            Ok(done(
                format!(
                    "Accepted invitation {} from {}",
                    view.invitation_id, view.sender_id
                ),
                operation,
            ))
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

        Request::ContactRename { contact, nickname } => {
            let id = contact_id(ctx, &contact).await?;
            contacts::update_contact_nickname(app, &id, &nickname, now_ms(ctx).await?).await?;
            Ok(done(format!("Renamed {id} to {nickname}"), None))
        }
        Request::ContactRemove { contact } => {
            let id = contact_id(ctx, &contact).await?;
            contacts::remove_contact(app, &id, now_ms(ctx).await?).await?;
            Ok(done(format!("Removed contact {id}"), None))
        }
        Request::FriendRequest { contact } => {
            let id = contact_id(ctx, &contact).await?;
            contacts::send_friend_request(app, &id, now_ms(ctx).await?).await?;
            Ok(done(format!("Friend request sent to {id}"), None))
        }
        Request::FriendAccept { contact } => {
            let id = contact_id(ctx, &contact).await?;
            contacts::accept_friend_request(app, &id, now_ms(ctx).await?).await?;
            Ok(done(format!("Accepted friend request from {id}"), None))
        }
        Request::FriendDecline { contact } => {
            let id = contact_id(ctx, &contact).await?;
            contacts::decline_friend_request(app, &id, now_ms(ctx).await?).await?;
            Ok(done(format!("Declined friend request from {id}"), None))
        }
        Request::FriendRevoke { contact } => {
            let id = contact_id(ctx, &contact).await?;
            contacts::revoke_friendship(app, &id, now_ms(ctx).await?).await?;
            Ok(done(format!("Ended friendship with {id}"), None))
        }
        Request::ReadReceipts { contact, enabled } => {
            let id = contact_id(ctx, &contact).await?;
            let policy = if enabled {
                ReadReceiptPolicy::Enabled
            } else {
                ReadReceiptPolicy::Disabled
            };
            contacts::set_read_receipt_policy(app, &id, policy).await?;
            Ok(done(
                format!(
                    "Read receipts to {id} {}",
                    if enabled { "on" } else { "off" }
                ),
                None,
            ))
        }
        Request::Whois { target } => Ok(Response::Contact(contact_view(
            &query::get_user_info(app, &target).await?,
        ))),

        Request::ChatDm { contact, message } => {
            let channel_id =
                messaging::send_direct_message(app, &contact, &message, now_ms(ctx).await?).await?;
            Ok(Response::MessageSent {
                channel_id,
                message_id: String::new(),
                operation: None,
            })
        }
        Request::ChatJoin { channel: name } => {
            let joined = messaging::join_channel_by_name(app, &name).await?;
            Ok(done(format!("Joined {name} ({joined})"), None))
        }
        Request::ChatClose { channel: selector } => {
            let channel = channel(ctx, &selector).await?;
            messaging::close_channel(app, channel.id, now_ms(ctx).await?).await?;
            Ok(done(format!("Closed {}", channel.name), None))
        }
        Request::ChatMembers { channel: selector } => {
            let channel = channel(ctx, &selector).await?;
            Ok(Response::Members(
                query::list_participants_by_channel_id(app, channel.id).await?,
            ))
        }
        Request::ChatRetry {
            channel: selector,
            message_id,
        } => {
            let channel = channel(ctx, &selector).await?;
            let content = messaging::channel_history(app, channel.id, None, None)
                .await
                .into_iter()
                .find(|m| m.id == message_id)
                .map(|m| m.content)
                .ok_or_else(|| CommandError::not_found(format!("message {message_id}")))?;
            let (message_id, operation) = settle(
                messaging::retry_message_with_terminal_status(app, channel.id, &content, None)
                    .await,
            )?;
            Ok(Response::MessageSent {
                channel_id: channel.id.to_string(),
                message_id,
                operation,
            })
        }
        Request::ChatMarkRead { channel: selector } => {
            let channel = channel(ctx, &selector).await?;
            let marked = contacts::mark_channel_viewed(app, channel.id).await?;
            Ok(done(format!("Marked {marked} messages read"), None))
        }

        Request::ProfileNick { nickname } => {
            settings::update_nickname(app, nickname.clone()).await?;
            Ok(done(format!("Nickname set to {nickname}"), None))
        }
        Request::SettingsShow => {
            settings::refresh_settings_from_runtime(app).await?;
            let s = settings::get_settings(app).await?;
            Ok(Response::Settings(SettingsView {
                nickname: s.nickname_suggestion,
                threshold_k: s.threshold_k,
                threshold_n: s.threshold_n,
                mfa_policy: s.mfa_policy,
                devices: s
                    .devices
                    .iter()
                    .map(|d| {
                        let current = if d.is_current { " (this device)" } else { "" };
                        format!("{} {}{current}", d.id, d.name)
                    })
                    .collect(),
                contacts: s.contact_count,
            }))
        }
        Request::SettingsMfa { require } => {
            settings::update_mfa_policy(app, require).await?;
            Ok(done(
                format!(
                    "Multifactor approval {}",
                    if require { "required" } else { "not required" }
                ),
                None,
            ))
        }
        Request::AccountRefresh => {
            system::refresh_account(app).await?;
            Ok(done("Account refreshed", None))
        }

        Request::NeighborhoodCreate { name } => Ok(Response::NeighborhoodCreated {
            neighborhood_id: context::create_neighborhood(app, name).await?,
        }),
        Request::NeighborhoodAdd { home } => {
            context::add_home_to_neighborhood(app, &home).await?;
            Ok(done(format!("Added {home} to the neighborhood"), None))
        }
        Request::NeighborhoodLink { home } => {
            context::link_home_one_hop_link(app, &home).await?;
            Ok(done(format!("Linked {home}"), None))
        }
        Request::HomeEnter { home, depth } => {
            let depth = depth.unwrap_or_else(|| "full".to_string());
            let reached = context::move_position(app, &home, &depth).await?;
            Ok(done(format!("Entered {home} at depth {reached}"), None))
        }

        Request::ModKick {
            target,
            channel: selector,
            reason,
        } => {
            let channel = match selector {
                Some(selector) => self::channel(ctx, &selector).await?.id.to_string(),
                None => current_home(ctx).await?,
            };
            moderation::kick_user(
                app,
                &channel,
                &target,
                reason.as_deref(),
                now_ms(ctx).await?,
            )
            .await?;
            Ok(done(format!("Kicked {target}"), None))
        }
        Request::ModBan { target, reason } => {
            moderation::ban_user(app, &target, reason.as_deref(), now_ms(ctx).await?).await?;
            Ok(done(format!("Banned {target}"), None))
        }
        Request::ModUnban { target } => {
            moderation::unban_user(app, &target).await?;
            Ok(done(format!("Unbanned {target}"), None))
        }
        Request::ModMute {
            target,
            duration_secs,
        } => {
            moderation::mute_user(app, &target, duration_secs, now_ms(ctx).await?).await?;
            Ok(done(format!("Muted {target}"), None))
        }
        Request::ModUnmute { target } => {
            moderation::unmute_user(app, &target).await?;
            Ok(done(format!("Unmuted {target}"), None))
        }
        Request::ModPin { message_id } => {
            moderation::pin_message(app, &message_id).await?;
            Ok(done(format!("Pinned {message_id}"), None))
        }
        Request::ModUnpin { message_id } => {
            moderation::unpin_message(app, &message_id).await?;
            Ok(done(format!("Unpinned {message_id}"), None))
        }
        Request::ModOp { target } => {
            moderator::grant_moderator(app, &target).await?;
            Ok(done(format!("{target} is now a moderator"), None))
        }
        Request::ModDeop { target } => {
            moderator::revoke_moderator(app, &target).await?;
            Ok(done(format!("{target} is no longer a moderator"), None))
        }
        Request::ModAdmit { target } => {
            moderator::admit_member(app, &target).await?;
            Ok(done(format!("Admitted {target}"), None))
        }
        Request::AccessSet {
            target,
            level,
            home,
        } => {
            let authority = parse_authority("target", &target)?;
            let level = match level.trim().to_ascii_lowercase().as_str() {
                "limited" => AccessLevel::Limited,
                "partial" => AccessLevel::Partial,
                "full" => AccessLevel::Full,
                other => {
                    return Err(CommandError::invalid(format!(
                        "unknown access level {other}; expected limited, partial or full"
                    )))
                }
            };
            access::set_access_override(app, home.as_deref(), authority, level).await?;
            Ok(done(
                format!("Access for {authority} set to {level:?}"),
                None,
            ))
        }

        Request::PeerList => Ok(Response::PeerList(
            network::list_peers(app, now_ms(ctx).await?).await?,
        )),
        Request::PeerDiscover => {
            let found = network::discover_peers(app, now_ms(ctx).await?).await?;
            Ok(done(format!("Discovered {found} peers"), None))
        }

        Request::ThresholdSet { k, n } => {
            settings::update_threshold(app, k, n).await?;
            Ok(done(format!("Threshold set to {k} of {n}"), None))
        }
        Request::GuardiansSet {
            guardians,
            threshold,
        } => {
            let guardians = parse_authorities("guardian", &guardians)?;
            let total = u16::try_from(guardians.len())
                .map_err(|_| CommandError::invalid("too many guardians"))?;
            let threshold = FrostThreshold::new(threshold)
                .map_err(|e| CommandError::invalid(format!("invalid threshold: {e:?}")))?;
            let handle =
                ceremonies::start_guardian_ceremony(app, threshold, total, guardians).await?;
            Ok(Response::CeremonyStarted {
                ceremony_id: handle.ceremony_id().to_string(),
            })
        }
        Request::DeviceRemove { device } => {
            let handle = ceremonies::start_device_removal_ceremony(app, device).await?;
            Ok(Response::CeremonyStarted {
                ceremony_id: handle.ceremony_id().to_string(),
            })
        }
        Request::RotationCancel { ceremony_id } => {
            ceremonies::cancel_key_rotation_ceremony_by_id(
                app,
                CeremonyId::new(ceremony_id.trim().to_string()),
            )
            .await?;
            Ok(done(format!("Cancelled ceremony {ceremony_id}"), None))
        }

        Request::Budget => Ok(Response::Budget {
            summary: format_budget_status(&budget::get_current_budget(app).await),
        }),
        Request::NotificationsList => {
            let contacts = read_signal_or_default(app, &*CONTACTS_SIGNAL).await;
            let invitations = invitation::list_invitations(app).await;
            let recovery = recovery::get_recovery_status(app).await?;
            Ok(Response::Notifications(notifications(
                &contacts,
                &invitations,
                &recovery,
            )))
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
