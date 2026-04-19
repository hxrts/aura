//! Shared typed slash-command preparation and classification.
//!
//! This module keeps slash-command execution on the strong typed
//! `ParsedCommand -> ResolvedCommand -> PlannedCommand` path while exposing
//! explicit metadata for ownership and semantic criticality.

#![allow(missing_docs)] // This API is being introduced incrementally.

use crate::ui_contract::{
    OperationId, SemanticFailureCode, SemanticFailureDomain, SemanticOperationError,
    SemanticOperationKind,
};
use crate::workflows::chat_commands::{
    all_command_help, command_help, CommandCapability, CommandError,
};
use crate::workflows::strong_command::{
    classify_terminal_execution_error, execute_planned, CommandExecutionResult, CommandResolver,
    CommandResolverError, CommandTerminalOutcomeStatus, CommandTerminalReasonCode, ParsedCommand,
    PlannedCommand, ResolvedCommand,
};
use crate::AppCore;
use async_lock::RwLock;
use aura_core::types::identifiers::AuthorityId;
use aura_core::AuraError;
use std::sync::Arc;

/// Exhaustive typed slash-command kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SlashCommandKind {
    Msg,
    Me,
    Nick,
    Who,
    Whois,
    Leave,
    Join,
    Help,
    Neighborhood,
    NhAdd,
    NhLink,
    HomeInvite,
    HomeAccept,
    Kick,
    Ban,
    Unban,
    Mute,
    Unmute,
    Invite,
    Topic,
    Pin,
    Unpin,
    Op,
    Deop,
    Mode,
}

impl SlashCommandKind {
    /// All supported slash-command kinds.
    pub const ALL: [Self; 25] = [
        Self::Msg,
        Self::Me,
        Self::Nick,
        Self::Who,
        Self::Whois,
        Self::Leave,
        Self::Join,
        Self::Help,
        Self::Neighborhood,
        Self::NhAdd,
        Self::NhLink,
        Self::HomeInvite,
        Self::HomeAccept,
        Self::Kick,
        Self::Ban,
        Self::Unban,
        Self::Mute,
        Self::Unmute,
        Self::Invite,
        Self::Topic,
        Self::Pin,
        Self::Unpin,
        Self::Op,
        Self::Deop,
        Self::Mode,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Msg => "msg",
            Self::Me => "me",
            Self::Nick => "nick",
            Self::Who => "who",
            Self::Whois => "whois",
            Self::Leave => "leave",
            Self::Join => "join",
            Self::Help => "help",
            Self::Neighborhood => "neighborhood",
            Self::NhAdd => "nhadd",
            Self::NhLink => "nhlink",
            Self::HomeInvite => "homeinvite",
            Self::HomeAccept => "homeaccept",
            Self::Kick => "kick",
            Self::Ban => "ban",
            Self::Unban => "unban",
            Self::Mute => "mute",
            Self::Unmute => "unmute",
            Self::Invite => "invite",
            Self::Topic => "topic",
            Self::Pin => "pin",
            Self::Unpin => "unpin",
            Self::Op => "op",
            Self::Deop => "deop",
            Self::Mode => "mode",
        }
    }

    #[must_use]
    pub fn metadata(self) -> SlashCommandMetadata {
        match self {
            Self::Who | Self::Whois | Self::Help => SlashCommandMetadata {
                boundary: SlashCommandSemanticBoundary::ObservedRead,
                owner_model: SlashCommandOwnerModel::OwnerlessObserved,
                capability: slash_command_capability(self),
                semantic_operation: slash_command_semantic_operation(self),
            },
            Self::Neighborhood | Self::NhAdd | Self::NhLink => SlashCommandMetadata {
                boundary: SlashCommandSemanticBoundary::SemanticMutation,
                owner_model: SlashCommandOwnerModel::LocalTerminal,
                capability: slash_command_capability(self),
                semantic_operation: slash_command_semantic_operation(self),
            },
            Self::Msg
            | Self::Me
            | Self::Nick
            | Self::Leave
            | Self::Join
            | Self::HomeInvite
            | Self::HomeAccept
            | Self::Kick
            | Self::Ban
            | Self::Unban
            | Self::Mute
            | Self::Unmute
            | Self::Invite
            | Self::Topic
            | Self::Pin
            | Self::Unpin
            | Self::Op
            | Self::Deop
            | Self::Mode => SlashCommandMetadata {
                boundary: SlashCommandSemanticBoundary::CapabilityGatedSemanticMutation,
                owner_model: SlashCommandOwnerModel::LocalTerminal,
                capability: slash_command_capability(self),
                semantic_operation: slash_command_semantic_operation(self),
            },
        }
    }
}

/// Semantic boundary class for slash-command execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SlashCommandSemanticBoundary {
    ObservedRead,
    SemanticMutation,
    CapabilityGatedSemanticMutation,
}

impl SlashCommandSemanticBoundary {
    #[must_use]
    pub const fn is_parity_critical(self) -> bool {
        !matches!(self, Self::ObservedRead)
    }
}

/// Frontend owner model required by a slash command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SlashCommandOwnerModel {
    OwnerlessObserved,
    LocalTerminal,
}

impl SlashCommandOwnerModel {
    #[must_use]
    pub const fn requires_owner(self) -> bool {
        !matches!(self, Self::OwnerlessObserved)
    }
}

/// Typed command metadata derived from workflow semantics.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SlashCommandMetadata {
    pub boundary: SlashCommandSemanticBoundary,
    pub owner_model: SlashCommandOwnerModel,
    pub capability: CommandCapability,
    pub semantic_operation: Option<SlashCommandSemanticOperation>,
}

impl SlashCommandMetadata {
    #[must_use]
    pub const fn is_parity_critical(&self) -> bool {
        self.boundary.is_parity_critical()
    }
}

/// Prepared slash command with typed metadata and executable plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedSlashCommand {
    kind: SlashCommandKind,
    metadata: SlashCommandMetadata,
    resolved: ResolvedCommand,
    plan: PlannedCommand,
}

impl PreparedSlashCommand {
    #[must_use]
    pub const fn kind(&self) -> SlashCommandKind {
        self.kind
    }

    #[must_use]
    pub const fn metadata(&self) -> &SlashCommandMetadata {
        &self.metadata
    }

    #[must_use]
    pub fn plan(&self) -> &PlannedCommand {
        &self.plan
    }

    #[must_use]
    pub fn resolved(&self) -> &ResolvedCommand {
        &self.resolved
    }

    #[must_use]
    pub fn into_plan(self) -> PlannedCommand {
        self.plan
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SlashCommandSemanticOperation {
    pub operation_id: OperationId,
    pub kind: SemanticOperationKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SlashCommandToastKind {
    Success,
    Info,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashCommandTerminalSettlement {
    Succeeded,
    Failed(SemanticOperationError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashCommandFeedback {
    pub topic: &'static str,
    pub toast_kind: SlashCommandToastKind,
    pub message: String,
    pub terminal_settlement: Option<SlashCommandTerminalSettlement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashCommandExecutionReport {
    pub metadata: Option<SlashCommandMetadata>,
    pub feedback: SlashCommandFeedback,
}

/// Prepare-time errors for typed slash-command execution.
#[derive(Debug, thiserror::Error)]
pub enum SlashCommandPrepareError {
    #[error(transparent)]
    Parse(#[from] CommandError),
    #[error(transparent)]
    Resolve(#[from] CommandResolverError),
}

/// Parse, resolve, classify, and plan a slash command.
pub async fn prepare(
    resolver: &CommandResolver,
    app_core: &Arc<RwLock<AppCore>>,
    input: &str,
    current_channel_hint: Option<&str>,
    actor: Option<AuthorityId>,
) -> Result<PreparedSlashCommand, SlashCommandPrepareError> {
    let parsed = ParsedCommand::parse(input)?;
    let snapshot = resolver.capture_snapshot(app_core).await;
    let resolved = resolver.resolve(parsed, &snapshot)?;
    let kind = SlashCommandKind::from_resolved(&resolved);
    let plan = resolver.plan(resolved.clone(), &snapshot, current_channel_hint, actor)?;
    Ok(PreparedSlashCommand {
        kind,
        metadata: kind.metadata(),
        resolved,
        plan,
    })
}

/// Execute a prepared slash command through the shared strong-command layer.
pub async fn execute(
    app_core: &Arc<RwLock<AppCore>>,
    command: &PreparedSlashCommand,
) -> Result<CommandExecutionResult, AuraError> {
    if let ResolvedCommand::Help { command: help } = command.resolved() {
        return Ok(CommandExecutionResult {
            consistency_requirement: command.plan.consistency_requirement(),
            completion_outcome:
                crate::workflows::strong_command::CommandCompletionOutcome::Satisfied(
                    crate::workflows::strong_command::ConsistencyWitness::Accepted,
                ),
            details: Some(render_help_message(help.clone())),
        });
    }
    execute_planned(app_core, command.plan.clone()).await
}

pub async fn prepare_and_execute(
    resolver: &CommandResolver,
    app_core: &Arc<RwLock<AppCore>>,
    input: &str,
    current_channel_hint: Option<&str>,
    actor: Option<AuthorityId>,
) -> SlashCommandExecutionReport {
    match prepare(resolver, app_core, input, current_channel_hint, actor).await {
        Ok(prepared) => {
            let feedback = match execute(app_core, &prepared).await {
                Ok(result) => feedback_for_execution_result(&prepared, &result),
                Err(error) => feedback_for_execute_error(&prepared, &error),
            };
            SlashCommandExecutionReport {
                metadata: Some(prepared.metadata().clone()),
                feedback,
            }
        }
        Err(error) => SlashCommandExecutionReport {
            metadata: None,
            feedback: feedback_for_prepare_error(&error),
        },
    }
}

pub fn feedback_for_prepare_error(error: &SlashCommandPrepareError) -> SlashCommandFeedback {
    let (message, status, reason) = match error {
        SlashCommandPrepareError::Parse(parse) => (
            parse.to_string(),
            classify_chat_command_error(parse).0,
            classify_chat_command_error(parse).1,
        ),
        SlashCommandPrepareError::Resolve(resolve) => (
            resolve.to_string(),
            classify_command_resolver_error(resolve).0,
            classify_command_resolver_error(resolve).1,
        ),
    };
    SlashCommandFeedback {
        topic: "command",
        toast_kind: SlashCommandToastKind::Error,
        message: command_outcome_message(message, status, reason, None),
        terminal_settlement: None,
    }
}

pub fn feedback_for_execute_error(
    command: &PreparedSlashCommand,
    error: &AuraError,
) -> SlashCommandFeedback {
    let classification = classify_terminal_execution_error(error);
    SlashCommandFeedback {
        topic: "command",
        toast_kind: SlashCommandToastKind::Error,
        message: command_outcome_message(
            format!("/{name}: {error}", name = command.kind.as_str()),
            classification.status,
            classification.reason,
            None,
        ),
        terminal_settlement: command.metadata.semantic_operation.as_ref().map(|_| {
            SlashCommandTerminalSettlement::Failed(classification_to_semantic_error(
                classification.status,
                classification.reason,
                error.to_string(),
            ))
        }),
    }
}

pub fn feedback_for_execution_result(
    command: &PreparedSlashCommand,
    result: &CommandExecutionResult,
) -> SlashCommandFeedback {
    let state_label = result.consistency_label();
    if let Some(classification) = result.terminal_classification() {
        let details = result
            .details
            .as_deref()
            .map(str::to_owned)
            .or_else(|| result.default_terminal_detail().map(ToOwned::to_owned))
            .unwrap_or_else(|| "command did not reach the required lifecycle state".to_string());
        return SlashCommandFeedback {
            topic: "command",
            toast_kind: SlashCommandToastKind::Error,
            message: command_outcome_message(
                format!(
                    "/{name}: {details} ({state_label})",
                    name = command.kind.as_str()
                ),
                classification.status,
                classification.reason,
                Some(state_label),
            ),
            terminal_settlement: command.metadata.semantic_operation.as_ref().map(|_| {
                SlashCommandTerminalSettlement::Failed(classification_to_semantic_error(
                    classification.status,
                    classification.reason,
                    details,
                ))
            }),
        };
    }

    let message = if let Some(details) = result.details.as_deref() {
        command_outcome_message(
            format!("{details} ({state_label})"),
            CommandTerminalOutcomeStatus::Ok,
            CommandTerminalReasonCode::None,
            Some(state_label),
        )
    } else {
        command_outcome_message(
            format!("/{name} ({state_label})", name = command.kind.as_str()),
            CommandTerminalOutcomeStatus::Ok,
            CommandTerminalReasonCode::None,
            Some(state_label),
        )
    };
    SlashCommandFeedback {
        topic: if command.kind == SlashCommandKind::Help {
            "help"
        } else {
            "command"
        },
        toast_kind: if command.kind == SlashCommandKind::Help
            || matches!(
                command.kind,
                SlashCommandKind::Who | SlashCommandKind::Whois
            ) {
            SlashCommandToastKind::Info
        } else {
            SlashCommandToastKind::Success
        },
        message,
        terminal_settlement: command
            .metadata
            .semantic_operation
            .as_ref()
            .map(|_| SlashCommandTerminalSettlement::Succeeded),
    }
}

impl SlashCommandKind {
    #[must_use]
    pub const fn from_resolved(command: &ResolvedCommand) -> Self {
        match command {
            ResolvedCommand::Msg { .. } => Self::Msg,
            ResolvedCommand::Me { .. } => Self::Me,
            ResolvedCommand::Nick { .. } => Self::Nick,
            ResolvedCommand::Who => Self::Who,
            ResolvedCommand::Whois { .. } => Self::Whois,
            ResolvedCommand::Leave => Self::Leave,
            ResolvedCommand::Join { .. } => Self::Join,
            ResolvedCommand::Help { .. } => Self::Help,
            ResolvedCommand::Neighborhood { .. } => Self::Neighborhood,
            ResolvedCommand::NhAdd { .. } => Self::NhAdd,
            ResolvedCommand::NhLink { .. } => Self::NhLink,
            ResolvedCommand::HomeInvite { .. } => Self::HomeInvite,
            ResolvedCommand::HomeAccept => Self::HomeAccept,
            ResolvedCommand::Kick { .. } => Self::Kick,
            ResolvedCommand::Ban { .. } => Self::Ban,
            ResolvedCommand::Unban { .. } => Self::Unban,
            ResolvedCommand::Mute { .. } => Self::Mute,
            ResolvedCommand::Unmute { .. } => Self::Unmute,
            ResolvedCommand::Invite { .. } => Self::Invite,
            ResolvedCommand::Topic { .. } => Self::Topic,
            ResolvedCommand::Pin { .. } => Self::Pin,
            ResolvedCommand::Unpin { .. } => Self::Unpin,
            ResolvedCommand::Op { .. } => Self::Op,
            ResolvedCommand::Deop { .. } => Self::Deop,
            ResolvedCommand::Mode { .. } => Self::Mode,
        }
    }
}

const fn slash_command_capability(kind: SlashCommandKind) -> CommandCapability {
    match kind {
        SlashCommandKind::Msg => CommandCapability::SendDm,
        SlashCommandKind::Me => CommandCapability::SendMessage,
        SlashCommandKind::Nick => CommandCapability::UpdateContact,
        SlashCommandKind::Who | SlashCommandKind::Whois => CommandCapability::ViewMembers,
        SlashCommandKind::Leave => CommandCapability::LeaveContext,
        SlashCommandKind::Join | SlashCommandKind::HomeAccept => CommandCapability::JoinChannel,
        SlashCommandKind::Help
        | SlashCommandKind::Neighborhood
        | SlashCommandKind::NhAdd
        | SlashCommandKind::NhLink => CommandCapability::None,
        SlashCommandKind::HomeInvite | SlashCommandKind::Invite => CommandCapability::Invite,
        SlashCommandKind::Kick => CommandCapability::ModerateKick,
        SlashCommandKind::Ban | SlashCommandKind::Unban => CommandCapability::ModerateBan,
        SlashCommandKind::Mute | SlashCommandKind::Unmute => CommandCapability::ModerateMute,
        SlashCommandKind::Topic | SlashCommandKind::Mode => CommandCapability::ManageChannel,
        SlashCommandKind::Pin | SlashCommandKind::Unpin => CommandCapability::PinContent,
        SlashCommandKind::Op | SlashCommandKind::Deop => CommandCapability::GrantModerator,
    }
}

fn slash_command_semantic_operation(
    kind: SlashCommandKind,
) -> Option<SlashCommandSemanticOperation> {
    Some(match kind {
        SlashCommandKind::Msg | SlashCommandKind::Me => SlashCommandSemanticOperation {
            operation_id: OperationId::send_message(),
            kind: SemanticOperationKind::SendChatMessage,
        },
        SlashCommandKind::Nick => SlashCommandSemanticOperation {
            operation_id: OperationId::update_nickname_suggestion(),
            kind: SemanticOperationKind::UpdateNicknameSuggestion,
        },
        SlashCommandKind::Leave => SlashCommandSemanticOperation {
            operation_id: OperationId::close_channel(),
            kind: SemanticOperationKind::CloseChannel,
        },
        SlashCommandKind::Join => SlashCommandSemanticOperation {
            operation_id: OperationId::join_channel(),
            kind: SemanticOperationKind::JoinChannel,
        },
        SlashCommandKind::Neighborhood => SlashCommandSemanticOperation {
            operation_id: OperationId::create_neighborhood(),
            kind: SemanticOperationKind::CreateNeighborhood,
        },
        SlashCommandKind::NhAdd => SlashCommandSemanticOperation {
            operation_id: OperationId::add_home_to_neighborhood(),
            kind: SemanticOperationKind::AddHomeToNeighborhood,
        },
        SlashCommandKind::NhLink => SlashCommandSemanticOperation {
            operation_id: OperationId::link_home_one_hop_link(),
            kind: SemanticOperationKind::LinkHomeOneHopLink,
        },
        SlashCommandKind::HomeInvite => SlashCommandSemanticOperation {
            operation_id: OperationId::home_invitation_create(),
            kind: SemanticOperationKind::CreateHomeInvitation,
        },
        SlashCommandKind::HomeAccept => SlashCommandSemanticOperation {
            operation_id: OperationId::invitation_accept_channel(),
            kind: SemanticOperationKind::AcceptPendingChannelInvitation,
        },
        SlashCommandKind::Kick => SlashCommandSemanticOperation {
            operation_id: OperationId::kick_actor(),
            kind: SemanticOperationKind::KickActor,
        },
        SlashCommandKind::Ban => SlashCommandSemanticOperation {
            operation_id: OperationId::ban_actor(),
            kind: SemanticOperationKind::BanActor,
        },
        SlashCommandKind::Unban => SlashCommandSemanticOperation {
            operation_id: OperationId::unban_actor(),
            kind: SemanticOperationKind::UnbanActor,
        },
        SlashCommandKind::Mute => SlashCommandSemanticOperation {
            operation_id: OperationId::mute_actor(),
            kind: SemanticOperationKind::MuteActor,
        },
        SlashCommandKind::Unmute => SlashCommandSemanticOperation {
            operation_id: OperationId::unmute_actor(),
            kind: SemanticOperationKind::UnmuteActor,
        },
        SlashCommandKind::Invite => SlashCommandSemanticOperation {
            operation_id: OperationId::invitation_create(),
            kind: SemanticOperationKind::InviteActorToChannel,
        },
        SlashCommandKind::Topic => SlashCommandSemanticOperation {
            operation_id: OperationId::set_channel_topic(),
            kind: SemanticOperationKind::SetChannelTopic,
        },
        SlashCommandKind::Pin => SlashCommandSemanticOperation {
            operation_id: OperationId::pin_message(),
            kind: SemanticOperationKind::PinMessage,
        },
        SlashCommandKind::Unpin => SlashCommandSemanticOperation {
            operation_id: OperationId::unpin_message(),
            kind: SemanticOperationKind::UnpinMessage,
        },
        SlashCommandKind::Op => SlashCommandSemanticOperation {
            operation_id: OperationId::grant_moderator(),
            kind: SemanticOperationKind::GrantModerator,
        },
        SlashCommandKind::Deop => SlashCommandSemanticOperation {
            operation_id: OperationId::revoke_moderator(),
            kind: SemanticOperationKind::RevokeModerator,
        },
        SlashCommandKind::Mode => SlashCommandSemanticOperation {
            operation_id: OperationId::set_channel_mode(),
            kind: SemanticOperationKind::SetChannelMode,
        },
        SlashCommandKind::Who | SlashCommandKind::Whois | SlashCommandKind::Help => return None,
    })
}

fn classify_chat_command_error(
    error: &CommandError,
) -> (CommandTerminalOutcomeStatus, CommandTerminalReasonCode) {
    match error {
        CommandError::NotACommand => (
            CommandTerminalOutcomeStatus::Invalid,
            CommandTerminalReasonCode::InvalidArgument,
        ),
        CommandError::UnknownCommand(_) => (
            CommandTerminalOutcomeStatus::Invalid,
            CommandTerminalReasonCode::NotFound,
        ),
        CommandError::MissingArgument { .. } | CommandError::InvalidArgument { .. } => (
            CommandTerminalOutcomeStatus::Invalid,
            CommandTerminalReasonCode::InvalidArgument,
        ),
    }
}

fn classify_command_resolver_error(
    error: &CommandResolverError,
) -> (CommandTerminalOutcomeStatus, CommandTerminalReasonCode) {
    match error {
        CommandResolverError::UnknownTarget { .. } => (
            CommandTerminalOutcomeStatus::Invalid,
            CommandTerminalReasonCode::NotFound,
        ),
        CommandResolverError::AmbiguousTarget { .. } => (
            CommandTerminalOutcomeStatus::Invalid,
            CommandTerminalReasonCode::InvalidArgument,
        ),
        CommandResolverError::StaleSnapshot { .. } => (
            CommandTerminalOutcomeStatus::Failed,
            CommandTerminalReasonCode::InvalidState,
        ),
        CommandResolverError::ParseError { .. } => (
            CommandTerminalOutcomeStatus::Invalid,
            CommandTerminalReasonCode::InvalidArgument,
        ),
        CommandResolverError::MissingCurrentChannel { .. } => (
            CommandTerminalOutcomeStatus::Invalid,
            CommandTerminalReasonCode::MissingActiveContext,
        ),
    }
}

fn command_outcome_message(
    message: impl Into<String>,
    status: CommandTerminalOutcomeStatus,
    reason: CommandTerminalReasonCode,
    consistency: Option<&str>,
) -> String {
    let metadata = format!(
        "[s={} r={} c={}]",
        status.as_str(),
        reason.as_str(),
        consistency.unwrap_or("none")
    );
    let message = message.into();
    if message.is_empty() {
        metadata
    } else {
        format!("{metadata} {message}")
    }
}

fn classification_to_semantic_error(
    status: CommandTerminalOutcomeStatus,
    reason: CommandTerminalReasonCode,
    detail: String,
) -> SemanticOperationError {
    let code = match reason {
        CommandTerminalReasonCode::OperationTimedOut => SemanticFailureCode::OperationTimedOut,
        CommandTerminalReasonCode::MissingActiveContext => {
            SemanticFailureCode::MissingAuthoritativeContext
        }
        CommandTerminalReasonCode::None
        | CommandTerminalReasonCode::PermissionDenied
        | CommandTerminalReasonCode::NotMember
        | CommandTerminalReasonCode::NotFound
        | CommandTerminalReasonCode::InvalidArgument
        | CommandTerminalReasonCode::InvalidState
        | CommandTerminalReasonCode::Muted
        | CommandTerminalReasonCode::Banned
        | CommandTerminalReasonCode::Unavailable
        | CommandTerminalReasonCode::Internal => SemanticFailureCode::InternalError,
    };
    let domain = match status {
        CommandTerminalOutcomeStatus::Ok
        | CommandTerminalOutcomeStatus::Invalid
        | CommandTerminalOutcomeStatus::Denied
        | CommandTerminalOutcomeStatus::Failed => SemanticFailureDomain::Command,
    };
    SemanticOperationError::new(domain, code).with_detail(detail)
}

fn render_help_message(command: Option<String>) -> String {
    if let Some(raw_name) = command
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let normalized = raw_name.trim_start_matches('/').to_lowercase();
        if let Some(help) = command_help(&normalized) {
            format!("{} — {}", help.syntax, help.description)
        } else {
            format!("Unknown command: /{normalized}")
        }
    } else {
        let commands = all_command_help()
            .into_iter()
            .take(8)
            .map(|help| format!("/{}", help.name))
            .collect::<Vec<_>>()
            .join(", ");
        format!("Common commands: {commands}. Use /help <command> for details.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflows::chat_commands::all_command_help;
    use std::collections::BTreeSet;

    #[test]
    fn slash_command_metadata_marks_observed_commands_ownerless() {
        let metadata = SlashCommandKind::Who.metadata();
        assert_eq!(
            metadata.boundary,
            SlashCommandSemanticBoundary::ObservedRead
        );
        assert_eq!(
            metadata.owner_model,
            SlashCommandOwnerModel::OwnerlessObserved
        );
        assert!(!metadata.is_parity_critical());
    }

    #[test]
    fn slash_command_metadata_marks_mutating_commands_local_terminal() {
        let invite = SlashCommandKind::Invite.metadata();
        assert_eq!(
            invite.boundary,
            SlashCommandSemanticBoundary::CapabilityGatedSemanticMutation
        );
        assert_eq!(invite.owner_model, SlashCommandOwnerModel::LocalTerminal);
        assert!(invite.is_parity_critical());

        let neighborhood = SlashCommandKind::Neighborhood.metadata();
        assert_eq!(
            neighborhood.boundary,
            SlashCommandSemanticBoundary::SemanticMutation
        );
        assert_eq!(
            neighborhood.owner_model,
            SlashCommandOwnerModel::LocalTerminal
        );
        assert!(neighborhood.is_parity_critical());
    }

    #[test]
    fn slash_command_kind_inventory_matches_help_inventory() {
        let typed = SlashCommandKind::ALL
            .into_iter()
            .map(SlashCommandKind::as_str)
            .collect::<BTreeSet<_>>();
        let help = all_command_help()
            .into_iter()
            .map(|entry| entry.name)
            .collect::<BTreeSet<_>>();
        assert_eq!(typed, help);
    }

    #[test]
    fn parity_critical_slash_command_metadata_requires_owner_and_semantic_operation() {
        for kind in SlashCommandKind::ALL {
            let metadata = kind.metadata();
            if metadata.is_parity_critical() {
                assert!(metadata.owner_model.requires_owner());
                assert!(metadata.semantic_operation.is_some());
            } else {
                assert_eq!(
                    metadata.owner_model,
                    SlashCommandOwnerModel::OwnerlessObserved
                );
                assert!(metadata.semantic_operation.is_none());
            }
        }
    }
}
