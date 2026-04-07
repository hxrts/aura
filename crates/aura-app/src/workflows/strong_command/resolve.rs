#![allow(missing_docs)]

use super::execution_model::PlannedCommand;
use super::parse::ParsedCommand;
use super::plan::{
    CommandPlan, CommandScope, MembershipPlan, ModerationPlan, ModeratorPlan, PlanPrecondition,
};
use super::resolved_refs::{
    ChannelResolveOutcome, ExistingChannelResolution, ResolveTarget, ResolvedAuthorityId,
    ResolvedChannelId, ResolvedCommand, ResolvedContextId,
};
use super::snapshot::{ResolverSnapshot, SnapshotToken};
use crate::core::StateSnapshot;
use crate::views::Contact;
use crate::workflows::chat_commands::normalize_channel_name;
use crate::workflows::parse::parse_authority_id;
use crate::AppCore;
use async_lock::RwLock;
use aura_core::types::identifiers::{AuthorityId, ChannelId};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Command resolver errors.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommandResolverError {
    /// Target was not found in the snapshot.
    #[error("unknown {target} target: {input}")]
    UnknownTarget {
        target: ResolveTarget,
        input: String,
    },
    /// Target matched more than one canonical candidate.
    #[error("ambiguous {target} target: {input}")]
    AmbiguousTarget {
        target: ResolveTarget,
        input: String,
        candidates: Vec<String>,
    },
    /// Snapshot token is stale relative to the latest captured token.
    #[error("stale snapshot token {provided}; latest token is {latest}")]
    StaleSnapshot {
        provided: SnapshotToken,
        latest: SnapshotToken,
    },
    #[error("command parse error: {message}")]
    ParseError { message: String },
    #[error("missing current channel for {command}")]
    MissingCurrentChannel { command: &'static str },
}

/// Strong command resolver bound to a single snapshot token contract.
#[derive(Debug)]
pub struct CommandResolver {
    next_token: AtomicU64,
    latest_token: AtomicU64,
}

impl Default for CommandResolver {
    fn default() -> Self {
        Self {
            next_token: AtomicU64::new(1),
            latest_token: AtomicU64::new(0),
        }
    }
}

impl CommandResolver {
    /// Capture a new snapshot token and immutable state for resolution.
    pub async fn capture_snapshot(&self, app_core: &Arc<RwLock<AppCore>>) -> ResolverSnapshot {
        let token = SnapshotToken(self.next_token.fetch_add(1, Ordering::Relaxed));
        self.latest_token.store(token.0, Ordering::Release);
        // OWNERSHIP: observed
        let state = app_core.read().await.snapshot();
        ResolverSnapshot { token, state }
    }

    /// Resolve a parsed command using a previously captured snapshot.
    pub fn resolve(
        &self,
        parsed: ParsedCommand,
        snapshot: &ResolverSnapshot,
    ) -> Result<ResolvedCommand, CommandResolverError> {
        self.ensure_fresh(snapshot)?;

        match parsed {
            ParsedCommand::Msg { target, text } => Ok(ResolvedCommand::Msg {
                target: self.resolve_authority(snapshot.state(), &target)?,
                text,
            }),
            ParsedCommand::Me { action } => Ok(ResolvedCommand::Me { action }),
            ParsedCommand::Nick { name } => Ok(ResolvedCommand::Nick { name }),
            ParsedCommand::Who => Ok(ResolvedCommand::Who),
            ParsedCommand::Whois { target } => Ok(ResolvedCommand::Whois {
                target: self.resolve_authority(snapshot.state(), &target)?,
            }),
            ParsedCommand::Leave => Ok(ResolvedCommand::Leave),
            ParsedCommand::Join { channel } => {
                let channel_name = normalize_channel_name(&channel);
                let channel = self.resolve_channel(snapshot.state(), &channel, true)?;
                Ok(ResolvedCommand::Join {
                    channel_name,
                    channel,
                })
            }
            ParsedCommand::Help { command } => Ok(ResolvedCommand::Help { command }),
            ParsedCommand::Neighborhood { name } => Ok(ResolvedCommand::Neighborhood { name }),
            ParsedCommand::NhAdd { home_id } => Ok(ResolvedCommand::NhAdd { home_id }),
            ParsedCommand::NhLink { home_id } => Ok(ResolvedCommand::NhLink { home_id }),
            ParsedCommand::HomeInvite { target } => Ok(ResolvedCommand::HomeInvite {
                target: self.resolve_authority(snapshot.state(), &target)?,
            }),
            ParsedCommand::HomeAccept => Ok(ResolvedCommand::HomeAccept),
            ParsedCommand::Kick { target, reason } => Ok(ResolvedCommand::Kick {
                target: self.resolve_authority(snapshot.state(), &target)?,
                reason,
            }),
            ParsedCommand::Ban { target, reason } => Ok(ResolvedCommand::Ban {
                target: self.resolve_authority(snapshot.state(), &target)?,
                reason,
            }),
            ParsedCommand::Unban { target } => Ok(ResolvedCommand::Unban {
                target: self.resolve_authority(snapshot.state(), &target)?,
            }),
            ParsedCommand::Mute { target, duration } => Ok(ResolvedCommand::Mute {
                target: self.resolve_authority(snapshot.state(), &target)?,
                duration,
            }),
            ParsedCommand::Unmute { target } => Ok(ResolvedCommand::Unmute {
                target: self.resolve_authority(snapshot.state(), &target)?,
            }),
            ParsedCommand::Invite { target } => Ok(ResolvedCommand::Invite {
                target: self.resolve_authority(snapshot.state(), &target)?,
            }),
            ParsedCommand::Topic { text } => Ok(ResolvedCommand::Topic { text }),
            ParsedCommand::Pin { message_id } => Ok(ResolvedCommand::Pin { message_id }),
            ParsedCommand::Unpin { message_id } => Ok(ResolvedCommand::Unpin { message_id }),
            ParsedCommand::Op { target } => Ok(ResolvedCommand::Op {
                target: self.resolve_authority(snapshot.state(), &target)?,
            }),
            ParsedCommand::Deop { target } => Ok(ResolvedCommand::Deop {
                target: self.resolve_authority(snapshot.state(), &target)?,
            }),
            ParsedCommand::Mode { channel, flags } => {
                let channel_name = normalize_channel_name(&channel);
                let channel = self.resolve_existing_channel(snapshot.state(), &channel)?;
                Ok(ResolvedCommand::Mode {
                    channel_name,
                    channel,
                    flags,
                })
            }
        }
    }

    /// Build a typed executable plan from a resolved command.
    pub fn plan(
        &self,
        resolved: ResolvedCommand,
        snapshot: &ResolverSnapshot,
        current_channel_hint: Option<&str>,
        actor: Option<AuthorityId>,
    ) -> Result<PlannedCommand, CommandResolverError> {
        self.ensure_fresh(snapshot)?;

        let actor = actor.map(ResolvedAuthorityId);

        match resolved {
            ResolvedCommand::Join {
                channel_name,
                channel,
            } => {
                let (scope, preconditions) = match channel {
                    ChannelResolveOutcome::Existing(channel) => (
                        CommandScope::Channel {
                            channel_id: channel.channel_id(),
                            context_id: channel.context_id(),
                        },
                        vec![PlanPrecondition::ChannelExists(channel.channel_id())],
                    ),
                    ChannelResolveOutcome::WillCreate { .. } => (CommandScope::Global, Vec::new()),
                };
                Ok(PlannedCommand::Membership(CommandPlan {
                    actor,
                    scope,
                    preconditions,
                    operation: MembershipPlan {
                        command: ResolvedCommand::Join {
                            channel_name,
                            channel,
                        },
                    },
                }))
            }
            ResolvedCommand::Leave => {
                let channel =
                    self.resolve_current_channel(snapshot, current_channel_hint, "leave")?;
                Ok(PlannedCommand::Membership(CommandPlan {
                    actor,
                    scope: CommandScope::Channel {
                        channel_id: channel.channel_id(),
                        context_id: channel.context_id(),
                    },
                    preconditions: vec![
                        PlanPrecondition::ChannelExists(channel.channel_id()),
                        PlanPrecondition::ActorInScope,
                    ],
                    operation: MembershipPlan {
                        command: ResolvedCommand::Leave,
                    },
                }))
            }
            ResolvedCommand::Kick { target, reason } => {
                let channel =
                    self.resolve_current_channel(snapshot, current_channel_hint, "kick")?;
                Ok(PlannedCommand::Moderation(CommandPlan {
                    actor,
                    scope: CommandScope::Channel {
                        channel_id: channel.channel_id(),
                        context_id: channel.context_id(),
                    },
                    preconditions: vec![
                        PlanPrecondition::TargetExists(target),
                        PlanPrecondition::ChannelExists(channel.channel_id()),
                        PlanPrecondition::ActorInScope,
                    ],
                    operation: ModerationPlan {
                        command: ResolvedCommand::Kick { target, reason },
                    },
                }))
            }
            ResolvedCommand::Ban { target, reason } => {
                let scope = if let Some(hint) = current_channel_hint {
                    let channel = self.resolve_current_channel(snapshot, Some(hint), "ban")?;
                    CommandScope::Channel {
                        channel_id: channel.channel_id(),
                        context_id: channel.context_id(),
                    }
                } else {
                    CommandScope::Global
                };

                Ok(PlannedCommand::Moderation(CommandPlan {
                    actor,
                    scope,
                    preconditions: vec![PlanPrecondition::TargetExists(target)],
                    operation: ModerationPlan {
                        command: ResolvedCommand::Ban { target, reason },
                    },
                }))
            }
            ResolvedCommand::Unban { target } => {
                let scope = if let Some(hint) = current_channel_hint {
                    let channel = self.resolve_current_channel(snapshot, Some(hint), "unban")?;
                    CommandScope::Channel {
                        channel_id: channel.channel_id(),
                        context_id: channel.context_id(),
                    }
                } else {
                    CommandScope::Global
                };

                Ok(PlannedCommand::Moderation(CommandPlan {
                    actor,
                    scope,
                    preconditions: vec![PlanPrecondition::TargetExists(target)],
                    operation: ModerationPlan {
                        command: ResolvedCommand::Unban { target },
                    },
                }))
            }
            ResolvedCommand::Mute { target, duration } => {
                let scope = if let Some(hint) = current_channel_hint {
                    let channel = self.resolve_current_channel(snapshot, Some(hint), "mute")?;
                    CommandScope::Channel {
                        channel_id: channel.channel_id(),
                        context_id: channel.context_id(),
                    }
                } else {
                    CommandScope::Global
                };

                Ok(PlannedCommand::Moderation(CommandPlan {
                    actor,
                    scope,
                    preconditions: vec![PlanPrecondition::TargetExists(target)],
                    operation: ModerationPlan {
                        command: ResolvedCommand::Mute { target, duration },
                    },
                }))
            }
            ResolvedCommand::Unmute { target } => {
                let scope = if let Some(hint) = current_channel_hint {
                    let channel = self.resolve_current_channel(snapshot, Some(hint), "unmute")?;
                    CommandScope::Channel {
                        channel_id: channel.channel_id(),
                        context_id: channel.context_id(),
                    }
                } else {
                    CommandScope::Global
                };

                Ok(PlannedCommand::Moderation(CommandPlan {
                    actor,
                    scope,
                    preconditions: vec![PlanPrecondition::TargetExists(target)],
                    operation: ModerationPlan {
                        command: ResolvedCommand::Unmute { target },
                    },
                }))
            }
            ResolvedCommand::Invite { target } => {
                let channel =
                    self.resolve_current_channel(snapshot, current_channel_hint, "invite")?;
                Ok(PlannedCommand::Moderation(CommandPlan {
                    actor,
                    scope: CommandScope::Channel {
                        channel_id: channel.channel_id(),
                        context_id: channel.context_id(),
                    },
                    preconditions: vec![
                        PlanPrecondition::TargetExists(target),
                        PlanPrecondition::ChannelExists(channel.channel_id()),
                    ],
                    operation: ModerationPlan {
                        command: ResolvedCommand::Invite { target },
                    },
                }))
            }
            ResolvedCommand::Op { target } => {
                let scope = if let Some(hint) = current_channel_hint {
                    let channel = self.resolve_current_channel(snapshot, Some(hint), "op")?;
                    CommandScope::Channel {
                        channel_id: channel.channel_id(),
                        context_id: channel.context_id(),
                    }
                } else {
                    CommandScope::Global
                };

                Ok(PlannedCommand::Moderator(CommandPlan {
                    actor,
                    scope,
                    preconditions: vec![PlanPrecondition::TargetExists(target)],
                    operation: ModeratorPlan {
                        command: ResolvedCommand::Op { target },
                    },
                }))
            }
            ResolvedCommand::Deop { target } => {
                let scope = if let Some(hint) = current_channel_hint {
                    let channel = self.resolve_current_channel(snapshot, Some(hint), "deop")?;
                    CommandScope::Channel {
                        channel_id: channel.channel_id(),
                        context_id: channel.context_id(),
                    }
                } else {
                    CommandScope::Global
                };

                Ok(PlannedCommand::Moderator(CommandPlan {
                    actor,
                    scope,
                    preconditions: vec![PlanPrecondition::TargetExists(target)],
                    operation: ModeratorPlan {
                        command: ResolvedCommand::Deop { target },
                    },
                }))
            }
            ResolvedCommand::Mode {
                channel_name,
                channel,
                flags,
            } => Ok(PlannedCommand::Moderator(CommandPlan {
                actor,
                scope: CommandScope::Channel {
                    channel_id: channel.channel_id(),
                    context_id: channel.context_id(),
                },
                preconditions: vec![PlanPrecondition::ChannelExists(channel.channel_id())],
                operation: ModeratorPlan {
                    command: ResolvedCommand::Mode {
                        channel_name,
                        channel,
                        flags,
                    },
                },
            })),
            command => {
                let scope = match &command {
                    ResolvedCommand::Me { .. }
                    | ResolvedCommand::Who
                    | ResolvedCommand::Topic { .. } => {
                        let channel = self.resolve_current_channel(
                            snapshot,
                            current_channel_hint,
                            command_name(&command),
                        )?;
                        CommandScope::Channel {
                            channel_id: channel.channel_id(),
                            context_id: channel.context_id(),
                        }
                    }
                    _ => CommandScope::Global,
                };

                let mut preconditions = Vec::new();
                match command {
                    ResolvedCommand::Msg { target, .. }
                    | ResolvedCommand::Whois { target }
                    | ResolvedCommand::HomeInvite { target } => {
                        preconditions.push(PlanPrecondition::TargetExists(target));
                    }
                    _ => {}
                }

                Ok(PlannedCommand::General(CommandPlan {
                    actor,
                    scope,
                    preconditions,
                    operation: command,
                }))
            }
        }
    }

    fn resolve_current_channel(
        &self,
        snapshot: &ResolverSnapshot,
        current_channel_hint: Option<&str>,
        command: &'static str,
    ) -> Result<ExistingChannelResolution, CommandResolverError> {
        let Some(current_channel_hint) = current_channel_hint else {
            return Err(CommandResolverError::MissingCurrentChannel { command });
        };
        self.resolve_existing_channel(snapshot.state(), current_channel_hint)
    }

    fn ensure_fresh(&self, snapshot: &ResolverSnapshot) -> Result<(), CommandResolverError> {
        let latest = SnapshotToken(self.latest_token.load(Ordering::Acquire));
        if latest.value() != 0 && latest != snapshot.token() {
            return Err(CommandResolverError::StaleSnapshot {
                provided: snapshot.token(),
                latest,
            });
        }
        Ok(())
    }

    fn resolve_authority(
        &self,
        state: &StateSnapshot,
        input: &str,
    ) -> Result<ResolvedAuthorityId, CommandResolverError> {
        let target = input.trim();
        if target.is_empty() {
            return Err(CommandResolverError::UnknownTarget {
                target: ResolveTarget::Authority,
                input: input.to_string(),
            });
        }

        if let Ok(authority_id) = parse_authority_id(target) {
            return Ok(ResolvedAuthorityId(authority_id));
        }

        let target_lower = target.to_lowercase();
        let mut exact: Vec<&Contact> = Vec::new();
        let mut fuzzy: Vec<&Contact> = Vec::new();

        for contact in state.contacts.all_contacts() {
            let id = contact.id.to_string();
            let nickname = contact.nickname.trim();
            let suggestion = contact.nickname_suggestion.as_deref().unwrap_or("").trim();
            let effective = effective_contact_name(contact);

            if id.eq_ignore_ascii_case(target)
                || (!nickname.is_empty() && nickname.eq_ignore_ascii_case(target))
                || (!suggestion.is_empty() && suggestion.eq_ignore_ascii_case(target))
            {
                exact.push(contact);
                continue;
            }

            if id.to_lowercase().starts_with(&target_lower)
                || effective.to_lowercase().contains(&target_lower)
            {
                fuzzy.push(contact);
            }
        }

        let selected = if exact.is_empty() { fuzzy } else { exact };
        if selected.is_empty() {
            return Err(CommandResolverError::UnknownTarget {
                target: ResolveTarget::Authority,
                input: target.to_string(),
            });
        }

        let mut canonical: BTreeMap<String, AuthorityId> = BTreeMap::new();
        for contact in selected {
            canonical.insert(contact.id.to_string(), contact.id);
        }

        if canonical.len() == 1 {
            if let Some(authority_id) = canonical.values().next().copied() {
                return Ok(ResolvedAuthorityId(authority_id));
            }
        }

        Err(CommandResolverError::AmbiguousTarget {
            target: ResolveTarget::Authority,
            input: target.to_string(),
            candidates: canonical
                .keys()
                .map(std::string::ToString::to_string)
                .collect(),
        })
    }

    fn resolve_channel(
        &self,
        state: &StateSnapshot,
        input: &str,
        allow_create: bool,
    ) -> Result<ChannelResolveOutcome, CommandResolverError> {
        match self.resolve_existing_channel(state, input) {
            Ok(channel) => return Ok(ChannelResolveOutcome::Existing(channel)),
            Err(CommandResolverError::UnknownTarget {
                target: ResolveTarget::Channel,
                ..
            }) => {}
            Err(err) => return Err(err),
        }

        if allow_create {
            let normalized = normalize_channel_name(input);
            let normalized = normalized.trim();
            if normalized.is_empty() {
                return Err(CommandResolverError::UnknownTarget {
                    target: ResolveTarget::Channel,
                    input: input.to_string(),
                });
            }
            if normalized.parse::<ChannelId>().is_ok() {
                return Err(CommandResolverError::UnknownTarget {
                    target: ResolveTarget::Channel,
                    input: input.to_string(),
                });
            }
            return Ok(ChannelResolveOutcome::WillCreate {
                channel_name: normalized.to_string(),
            });
        }

        Err(CommandResolverError::UnknownTarget {
            target: ResolveTarget::Channel,
            input: normalize_channel_name(input),
        })
    }

    fn resolve_existing_channel(
        &self,
        state: &StateSnapshot,
        input: &str,
    ) -> Result<ExistingChannelResolution, CommandResolverError> {
        let normalized = normalize_channel_name(input);
        let normalized = normalized.trim();
        if normalized.is_empty() {
            return Err(CommandResolverError::UnknownTarget {
                target: ResolveTarget::Channel,
                input: input.to_string(),
            });
        }

        if let Ok(channel_id) = normalized.parse::<ChannelId>() {
            if let Some(ctx) = resolve_channel_context(state, channel_id) {
                return Ok(ExistingChannelResolution::new(
                    ResolvedChannelId(channel_id),
                    ctx,
                ));
            }
            return Err(CommandResolverError::UnknownTarget {
                target: ResolveTarget::Channel,
                input: input.to_string(),
            });
        }

        let mut by_id: BTreeMap<ChannelId, (String, Option<ResolvedContextId>)> = BTreeMap::new();
        for channel in state.chat.all_channels() {
            if channel.name.eq_ignore_ascii_case(normalized) {
                let context = channel.context_id.map(ResolvedContextId);
                by_id.insert(channel.id, (channel.name.clone(), context));
            }
        }
        if by_id.len() == 1 {
            if let Some((channel_id, (_, context_id))) = by_id
                .iter()
                .next()
                .map(|(id, (name, context))| (*id, (name, *context)))
            {
                return Ok(ExistingChannelResolution::new(
                    ResolvedChannelId(channel_id),
                    context_id,
                ));
            }
        }

        if by_id.len() > 1 {
            let candidates = by_id
                .iter()
                .map(|(_, (name, _))| name.clone())
                .collect::<Vec<_>>();
            return Err(CommandResolverError::AmbiguousTarget {
                target: ResolveTarget::Channel,
                input: normalized.to_string(),
                candidates,
            });
        }

        Err(CommandResolverError::UnknownTarget {
            target: ResolveTarget::Channel,
            input: normalized.to_string(),
        })
    }
}

fn resolve_channel_context(
    state: &StateSnapshot,
    channel_id: ChannelId,
) -> Option<Option<ResolvedContextId>> {
    if let Some(channel) = state.chat.channel(&channel_id) {
        return Some(channel.context_id.map(ResolvedContextId));
    }
    if let Some(home) = state.homes.home_state(&channel_id) {
        return Some(home.context_id.map(ResolvedContextId));
    }
    None
}

fn effective_contact_name(contact: &Contact) -> String {
    if !contact.nickname.trim().is_empty() {
        return contact.nickname.clone();
    }
    if let Some(suggestion) = contact.nickname_suggestion.as_ref() {
        if !suggestion.trim().is_empty() {
            return suggestion.clone();
        }
    }
    let id = contact.id.to_string();
    let short = id.chars().take(8).collect::<String>();
    format!("{short}...")
}

pub(super) fn command_name(command: &ResolvedCommand) -> &'static str {
    match command {
        ResolvedCommand::Msg { .. } => "msg",
        ResolvedCommand::Me { .. } => "me",
        ResolvedCommand::Nick { .. } => "nick",
        ResolvedCommand::Who => "who",
        ResolvedCommand::Whois { .. } => "whois",
        ResolvedCommand::Leave => "leave",
        ResolvedCommand::Join { .. } => "join",
        ResolvedCommand::Help { .. } => "help",
        ResolvedCommand::Neighborhood { .. } => "neighborhood",
        ResolvedCommand::NhAdd { .. } => "nhadd",
        ResolvedCommand::NhLink { .. } => "nhlink",
        ResolvedCommand::HomeInvite { .. } => "homeinvite",
        ResolvedCommand::HomeAccept => "homeaccept",
        ResolvedCommand::Kick { .. } => "kick",
        ResolvedCommand::Ban { .. } => "ban",
        ResolvedCommand::Unban { .. } => "unban",
        ResolvedCommand::Mute { .. } => "mute",
        ResolvedCommand::Unmute { .. } => "unmute",
        ResolvedCommand::Invite { .. } => "invite",
        ResolvedCommand::Topic { .. } => "topic",
        ResolvedCommand::Pin { .. } => "pin",
        ResolvedCommand::Unpin { .. } => "unpin",
        ResolvedCommand::Op { .. } => "op",
        ResolvedCommand::Deop { .. } => "deop",
        ResolvedCommand::Mode { .. } => "mode",
    }
}
