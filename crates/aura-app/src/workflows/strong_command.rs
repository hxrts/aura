//! Strongly typed command boundary for slash command execution.
//!
//! This module introduces a strict parse/resolve boundary:
//! - `ParsedCommand` carries syntax-level values.
//! - `ResolvedCommand` carries canonical identifiers for executable targets.
//! - `CommandResolver` resolves against a single snapshot token.

#![allow(missing_docs)] // This API is being introduced incrementally.

use crate::core::StateSnapshot;
use crate::views::Contact;
use crate::workflows::chat_commands::{
    normalize_channel_name, parse_chat_command, ChatCommand, CommandError,
};
use crate::workflows::parse::parse_authority_id;
#[cfg(feature = "signals")]
use crate::workflows::runtime::{converge_runtime, cooperative_yield, require_runtime};
#[cfg(feature = "signals")]
use crate::workflows::{context, invitation, messaging, moderation, moderator, query, settings};
use crate::AppCore;
use async_lock::RwLock;
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_core::AuraError;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Canonical authority target resolved by `CommandResolver`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResolvedAuthorityId(pub AuthorityId);

/// Canonical channel target resolved by `CommandResolver`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResolvedChannelId(pub ChannelId);

/// Canonical context target resolved by `CommandResolver`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResolvedContextId(pub ContextId);

/// Canonical existing channel target resolved by `CommandResolver`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExistingChannelResolution {
    channel_id: ResolvedChannelId,
    context_id: Option<ResolvedContextId>,
}

impl ExistingChannelResolution {
    #[must_use]
    pub const fn channel_id(&self) -> ResolvedChannelId {
        self.channel_id
    }

    #[must_use]
    pub const fn context_id(&self) -> Option<ResolvedContextId> {
        self.context_id
    }
}

/// Canonical result of channel resolution for commands that may target an
/// existing channel or intentionally create one later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelResolveOutcome {
    Existing(ExistingChannelResolution),
    WillCreate { channel_name: String },
}

impl ChannelResolveOutcome {
    #[must_use]
    pub const fn context_id(&self) -> Option<ResolvedContextId> {
        match self {
            Self::Existing(channel) => channel.context_id(),
            Self::WillCreate { .. } => None,
        }
    }

    #[must_use]
    pub const fn existing_channel(&self) -> Option<ExistingChannelResolution> {
        match self {
            Self::Existing(channel) => Some(*channel),
            Self::WillCreate { .. } => None,
        }
    }

    #[must_use]
    pub fn is_will_create(&self) -> bool {
        matches!(self, Self::WillCreate { .. })
    }
}

/// Snapshot token used to guarantee single-snapshot command resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SnapshotToken(u64);

impl SnapshotToken {
    #[must_use]
    pub fn value(self) -> u64 {
        self.0
    }
}

impl fmt::Display for SnapshotToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "snapshot-{}", self.0)
    }
}

/// Snapshot captured for command resolution.
#[derive(Debug, Clone)]
pub struct ResolverSnapshot {
    token: SnapshotToken,
    state: StateSnapshot,
}

impl ResolverSnapshot {
    #[must_use]
    pub fn token(&self) -> SnapshotToken {
        self.token
    }

    #[must_use]
    pub fn state(&self) -> &StateSnapshot {
        &self.state
    }
}

/// Parse-level command values (never executable directly).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedCommand {
    Msg {
        target: String,
        text: String,
    },
    Me {
        action: String,
    },
    Nick {
        name: String,
    },
    Who,
    Whois {
        target: String,
    },
    Leave,
    Join {
        channel: String,
    },
    Help {
        command: Option<String>,
    },
    Neighborhood {
        name: String,
    },
    NhAdd {
        home_id: String,
    },
    NhLink {
        home_id: String,
    },
    HomeInvite {
        target: String,
    },
    HomeAccept,
    Kick {
        target: String,
        reason: Option<String>,
    },
    Ban {
        target: String,
        reason: Option<String>,
    },
    Unban {
        target: String,
    },
    Mute {
        target: String,
        duration: Option<std::time::Duration>,
    },
    Unmute {
        target: String,
    },
    Invite {
        target: String,
    },
    Topic {
        text: String,
    },
    Pin {
        message_id: String,
    },
    Unpin {
        message_id: String,
    },
    Op {
        target: String,
    },
    Deop {
        target: String,
    },
    Mode {
        channel: String,
        flags: String,
    },
}

impl ParsedCommand {
    /// Parse a user input command string into `ParsedCommand`.
    pub fn parse(input: &str) -> Result<Self, CommandError> {
        parse_chat_command(input).map(Self::from)
    }
}

impl From<ChatCommand> for ParsedCommand {
    fn from(value: ChatCommand) -> Self {
        match value {
            ChatCommand::Msg { target, text } => Self::Msg { target, text },
            ChatCommand::Me { action } => Self::Me { action },
            ChatCommand::Nick { name } => Self::Nick { name },
            ChatCommand::Who => Self::Who,
            ChatCommand::Whois { target } => Self::Whois { target },
            ChatCommand::Leave => Self::Leave,
            ChatCommand::Join { channel } => Self::Join { channel },
            ChatCommand::Help { command } => Self::Help { command },
            ChatCommand::Neighborhood { name } => Self::Neighborhood { name },
            ChatCommand::NhAdd { home_id } => Self::NhAdd { home_id },
            ChatCommand::NhLink { home_id } => Self::NhLink { home_id },
            ChatCommand::HomeInvite { target } => Self::HomeInvite { target },
            ChatCommand::HomeAccept => Self::HomeAccept,
            ChatCommand::Kick { target, reason } => Self::Kick { target, reason },
            ChatCommand::Ban { target, reason } => Self::Ban { target, reason },
            ChatCommand::Unban { target } => Self::Unban { target },
            ChatCommand::Mute { target, duration } => Self::Mute { target, duration },
            ChatCommand::Unmute { target } => Self::Unmute { target },
            ChatCommand::Invite { target } => Self::Invite { target },
            ChatCommand::Topic { text } => Self::Topic { text },
            ChatCommand::Pin { message_id } => Self::Pin { message_id },
            ChatCommand::Unpin { message_id } => Self::Unpin { message_id },
            ChatCommand::Op { target } => Self::Op { target },
            ChatCommand::Deop { target } => Self::Deop { target },
            ChatCommand::Mode { channel, flags } => Self::Mode { channel, flags },
        }
    }
}

/// Executable command values with canonical IDs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedCommand {
    Msg {
        target: ResolvedAuthorityId,
        text: String,
    },
    Me {
        action: String,
    },
    Nick {
        name: String,
    },
    Who,
    Whois {
        target: ResolvedAuthorityId,
    },
    Leave,
    Join {
        channel_name: String,
        channel: ChannelResolveOutcome,
    },
    Help {
        command: Option<String>,
    },
    Neighborhood {
        name: String,
    },
    NhAdd {
        home_id: String,
    },
    NhLink {
        home_id: String,
    },
    HomeInvite {
        target: ResolvedAuthorityId,
    },
    HomeAccept,
    Kick {
        target: ResolvedAuthorityId,
        reason: Option<String>,
    },
    Ban {
        target: ResolvedAuthorityId,
        reason: Option<String>,
    },
    Unban {
        target: ResolvedAuthorityId,
    },
    Mute {
        target: ResolvedAuthorityId,
        duration: Option<std::time::Duration>,
    },
    Unmute {
        target: ResolvedAuthorityId,
    },
    Invite {
        target: ResolvedAuthorityId,
    },
    Topic {
        text: String,
    },
    Pin {
        message_id: String,
    },
    Unpin {
        message_id: String,
    },
    Op {
        target: ResolvedAuthorityId,
    },
    Deop {
        target: ResolvedAuthorityId,
    },
    Mode {
        channel_name: String,
        channel: ExistingChannelResolution,
        flags: String,
    },
}

/// Common execution scope for command plans.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandScope {
    Global,
    Channel {
        channel_id: ResolvedChannelId,
        context_id: Option<ResolvedContextId>,
    },
    Context {
        context_id: ResolvedContextId,
    },
}

/// Planning preconditions that must hold before execution.
///
/// These are validated against the current snapshot at execution time via
/// [`validate_preconditions`] to detect TOCTOU drift between plan resolution
/// and execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanPrecondition {
    TargetExists(ResolvedAuthorityId),
    ChannelExists(ResolvedChannelId),
    ActorInScope,
}

/// Validate that all plan preconditions still hold against the current state.
#[cfg(feature = "signals")]
fn validate_preconditions<T>(
    plan: &CommandPlan<T>,
    snapshot: &crate::core::StateSnapshot,
) -> Result<(), CommandResolverError> {
    for precondition in &plan.preconditions {
        match precondition {
            PlanPrecondition::TargetExists(target) => {
                if snapshot.contacts.contact(&target.0).is_none() {
                    return Err(CommandResolverError::UnknownTarget {
                        target: ResolveTarget::Authority,
                        input: target.0.to_string(),
                    });
                }
            }
            PlanPrecondition::ChannelExists(channel) => {
                if snapshot.chat.channel(&channel.0).is_none() {
                    return Err(CommandResolverError::UnknownTarget {
                        target: ResolveTarget::Channel,
                        input: channel.0.to_string(),
                    });
                }
            }
            PlanPrecondition::ActorInScope => {
                // Actor-in-scope is verified by the resolver; re-check is
                // deferred until actor-scoped capability gates are in place.
            }
        }
    }
    Ok(())
}

/// Typed command plan for execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPlan<T> {
    pub actor: Option<ResolvedAuthorityId>,
    pub scope: CommandScope,
    pub preconditions: Vec<PlanPrecondition>,
    pub operation: T,
}

/// Membership operation plan family (`/join`, `/leave`, `/part`, `/quit`, `/j`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MembershipPlan {
    pub command: ResolvedCommand,
}

/// Moderation operation plan family (`/kick`, `/ban`, `/mute`, etc.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModerationPlan {
    pub command: ResolvedCommand,
}

/// Moderator operation plan family (`/op`, `/deop`, `/mode`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeratorPlan {
    pub command: ResolvedCommand,
}

/// Plan construction errors.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommandPlanError {
    #[error("command is not a membership command")]
    NotMembershipCommand,
    #[error("command is not a moderation command")]
    NotModerationCommand,
    #[error("command is not a moderator command")]
    NotModeratorCommand,
}

impl MembershipPlan {
    pub fn from_resolved(command: ResolvedCommand) -> Result<Self, CommandPlanError> {
        if matches!(
            command,
            ResolvedCommand::Join { .. } | ResolvedCommand::Leave
        ) {
            return Ok(Self { command });
        }
        Err(CommandPlanError::NotMembershipCommand)
    }
}

impl ModerationPlan {
    pub fn from_resolved(command: ResolvedCommand) -> Result<Self, CommandPlanError> {
        if matches!(
            command,
            ResolvedCommand::Kick { .. }
                | ResolvedCommand::Ban { .. }
                | ResolvedCommand::Unban { .. }
                | ResolvedCommand::Mute { .. }
                | ResolvedCommand::Unmute { .. }
                | ResolvedCommand::Invite { .. }
        ) {
            return Ok(Self { command });
        }
        Err(CommandPlanError::NotModerationCommand)
    }
}

impl ModeratorPlan {
    pub fn from_resolved(command: ResolvedCommand) -> Result<Self, CommandPlanError> {
        if matches!(
            command,
            ResolvedCommand::Op { .. }
                | ResolvedCommand::Deop { .. }
                | ResolvedCommand::Mode { .. }
        ) {
            return Ok(Self { command });
        }
        Err(CommandPlanError::NotModeratorCommand)
    }
}

/// Explicit consistency requirement for command completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConsistencyRequirement {
    Accepted,
    Replicated,
    Enforced,
}

/// Strongest completion witness observed for a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConsistencyWitness {
    Accepted,
    Replicated,
    Enforced,
}

impl ConsistencyWitness {
    #[must_use]
    pub const fn satisfies(self, requirement: ConsistencyRequirement) -> bool {
        match requirement {
            ConsistencyRequirement::Accepted => {
                matches!(self, Self::Accepted | Self::Replicated | Self::Enforced)
            }
            ConsistencyRequirement::Replicated => matches!(self, Self::Replicated | Self::Enforced),
            ConsistencyRequirement::Enforced => matches!(self, Self::Enforced),
        }
    }
}

/// Explicit degraded completion reasons for strong-command consistency checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConsistencyDegradedReason {
    RuntimeUnavailable,
    OperationTimedOut,
}

impl ConsistencyDegradedReason {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RuntimeUnavailable => "runtime-unavailable",
            Self::OperationTimedOut => "partial-timeout",
        }
    }

    #[must_use]
    pub const fn default_detail(self) -> &'static str {
        match self {
            Self::RuntimeUnavailable => {
                "consistency barrier unavailable because no runtime bridge was registered"
            }
            Self::OperationTimedOut => "consistency barrier timed out",
        }
    }
}

/// Typed command completion outcome for strong-command execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandCompletionOutcome {
    Satisfied(ConsistencyWitness),
    Degraded {
        requirement: ConsistencyRequirement,
        reason: ConsistencyDegradedReason,
    },
}

impl CommandCompletionOutcome {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Satisfied(witness) => consistency_witness_label(witness),
            Self::Degraded { reason, .. } => reason.as_str(),
        }
    }

    #[must_use]
    pub const fn satisfies(self, requirement: ConsistencyRequirement) -> bool {
        match self {
            Self::Satisfied(witness) => witness.satisfies(requirement),
            Self::Degraded { .. } => false,
        }
    }

    #[must_use]
    pub const fn terminal_classification(self) -> Option<CommandTerminalClassification> {
        match self {
            Self::Satisfied(_) => None,
            Self::Degraded {
                reason: ConsistencyDegradedReason::RuntimeUnavailable,
                ..
            } => Some(CommandTerminalClassification::new(
                CommandTerminalOutcomeStatus::Failed,
                CommandTerminalReasonCode::Unavailable,
            )),
            Self::Degraded {
                reason: ConsistencyDegradedReason::OperationTimedOut,
                ..
            } => Some(CommandTerminalClassification::new(
                CommandTerminalOutcomeStatus::Failed,
                CommandTerminalReasonCode::OperationTimedOut,
            )),
        }
    }

    #[must_use]
    pub const fn default_detail(self) -> Option<&'static str> {
        match self {
            Self::Satisfied(_) => None,
            Self::Degraded { reason, .. } => Some(reason.default_detail()),
        }
    }
}

/// Terminal-facing status metadata for strong command outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandTerminalOutcomeStatus {
    Ok,
    Invalid,
    Denied,
    Failed,
}

impl CommandTerminalOutcomeStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Invalid => "invalid",
            Self::Denied => "denied",
            Self::Failed => "failed",
        }
    }
}

/// Terminal-facing reason metadata for strong command outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandTerminalReasonCode {
    None,
    MissingActiveContext,
    PermissionDenied,
    NotMember,
    NotFound,
    InvalidArgument,
    InvalidState,
    Muted,
    Banned,
    Unavailable,
    OperationTimedOut,
    Internal,
}

impl CommandTerminalReasonCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::MissingActiveContext => "missing_active_context",
            Self::PermissionDenied => "permission_denied",
            Self::NotMember => "not_member",
            Self::NotFound => "not_found",
            Self::InvalidArgument => "invalid_argument",
            Self::InvalidState => "invalid_state",
            Self::Muted => "muted",
            Self::Banned => "banned",
            Self::Unavailable => "unavailable",
            Self::OperationTimedOut => "operation_timed_out",
            Self::Internal => "internal",
        }
    }
}

/// Typed terminal-facing classification for strong command execution failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CommandTerminalClassification {
    pub status: CommandTerminalOutcomeStatus,
    pub reason: CommandTerminalReasonCode,
}

impl CommandTerminalClassification {
    #[must_use]
    pub const fn new(
        status: CommandTerminalOutcomeStatus,
        reason: CommandTerminalReasonCode,
    ) -> Self {
        Self { status, reason }
    }
}

fn message_contains_any(message: &str, needles: &[&str]) -> bool {
    let lowered = message.to_ascii_lowercase();
    needles.iter().any(|needle| lowered.contains(needle))
}

fn classify_invalid_terminal_command_message(message: &str) -> CommandTerminalClassification {
    if message_contains_any(
        message,
        &[
            "no active home",
            "missing current channel",
            "missing channel scope",
        ],
    ) {
        CommandTerminalClassification::new(
            CommandTerminalOutcomeStatus::Invalid,
            CommandTerminalReasonCode::MissingActiveContext,
        )
    } else if message_contains_any(
        message,
        &[
            "unknown authority target",
            "unknown channel target",
            "unknown context target",
            "not found",
        ],
    ) {
        CommandTerminalClassification::new(
            CommandTerminalOutcomeStatus::Invalid,
            CommandTerminalReasonCode::NotFound,
        )
    } else if message_contains_any(message, &["parse error", "missing required argument"]) {
        CommandTerminalClassification::new(
            CommandTerminalOutcomeStatus::Invalid,
            CommandTerminalReasonCode::InvalidArgument,
        )
    } else if message_contains_any(
        message,
        &["stale snapshot", "invalid state", "precondition failed"],
    ) {
        let reason = CommandTerminalReasonCode::InvalidState;
        CommandTerminalClassification::new(CommandTerminalOutcomeStatus::Failed, reason)
    } else {
        CommandTerminalClassification::new(
            CommandTerminalOutcomeStatus::Invalid,
            CommandTerminalReasonCode::InvalidArgument,
        )
    }
}

fn classify_permission_terminal_command_message(message: &str) -> CommandTerminalClassification {
    let reason = if message_contains_any(message, &["not a member"]) {
        CommandTerminalReasonCode::NotMember
    } else if message_contains_any(message, &["muted"]) {
        CommandTerminalReasonCode::Muted
    } else if message_contains_any(message, &["banned", "ban "]) {
        CommandTerminalReasonCode::Banned
    } else {
        CommandTerminalReasonCode::PermissionDenied
    };
    CommandTerminalClassification::new(CommandTerminalOutcomeStatus::Denied, reason)
}

/// Classify strong command execution failures for terminal-facing outcome
/// rendering without requiring Layer 7 string parsing.
#[must_use]
pub fn classify_terminal_execution_error(error: &AuraError) -> CommandTerminalClassification {
    match error {
        AuraError::Invalid { message } => classify_invalid_terminal_command_message(message),
        AuraError::NotFound { .. } => CommandTerminalClassification::new(
            CommandTerminalOutcomeStatus::Invalid,
            CommandTerminalReasonCode::NotFound,
        ),
        AuraError::PermissionDenied { message } => {
            classify_permission_terminal_command_message(message)
        }
        AuraError::Crypto { .. }
        | AuraError::Network { .. }
        | AuraError::Serialization { .. }
        | AuraError::Storage { .. }
        | AuraError::Internal { .. }
        | AuraError::Terminal(_) => CommandTerminalClassification::new(
            CommandTerminalOutcomeStatus::Failed,
            CommandTerminalReasonCode::Internal,
        ),
    }
}

/// Planned command family produced after parse/resolve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlannedCommand {
    General(CommandPlan<ResolvedCommand>),
    Membership(CommandPlan<MembershipPlan>),
    Moderation(CommandPlan<ModerationPlan>),
    Moderator(CommandPlan<ModeratorPlan>),
}

impl PlannedCommand {
    #[must_use]
    pub fn consistency_requirement(&self) -> ConsistencyRequirement {
        match self {
            Self::General(plan) => consistency_for_resolved(&plan.operation),
            Self::Membership(plan) => match &plan.operation.command {
                ResolvedCommand::Join {
                    channel: ChannelResolveOutcome::WillCreate { .. },
                    ..
                } => ConsistencyRequirement::Accepted,
                _ => consistency_for_resolved(&plan.operation.command),
            },
            Self::Moderation(plan) => consistency_for_resolved(&plan.operation.command),
            Self::Moderator(plan) => consistency_for_resolved(&plan.operation.command),
        }
    }
}

/// Execution output for a planned command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandExecutionResult {
    pub consistency_requirement: ConsistencyRequirement,
    pub completion_outcome: CommandCompletionOutcome,
    pub details: Option<String>,
}

impl CommandExecutionResult {
    #[must_use]
    pub const fn consistency_label(&self) -> &'static str {
        self.completion_outcome.label()
    }

    #[must_use]
    pub const fn terminal_classification(&self) -> Option<CommandTerminalClassification> {
        self.completion_outcome.terminal_classification()
    }

    #[must_use]
    pub const fn default_terminal_detail(&self) -> Option<&'static str> {
        self.completion_outcome.default_detail()
    }
}

/// Static consistency mapping for slash command execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandConsistencySpec {
    pub command: &'static str,
    pub requirement: ConsistencyRequirement,
}

pub const COMMAND_CONSISTENCY_TABLE: &[CommandConsistencySpec] = &[
    CommandConsistencySpec {
        command: "join",
        requirement: ConsistencyRequirement::Replicated,
    },
    CommandConsistencySpec {
        command: "leave",
        requirement: ConsistencyRequirement::Replicated,
    },
    CommandConsistencySpec {
        command: "kick",
        requirement: ConsistencyRequirement::Enforced,
    },
    CommandConsistencySpec {
        command: "ban",
        requirement: ConsistencyRequirement::Enforced,
    },
    CommandConsistencySpec {
        command: "unban",
        requirement: ConsistencyRequirement::Enforced,
    },
    CommandConsistencySpec {
        command: "mute",
        requirement: ConsistencyRequirement::Enforced,
    },
    CommandConsistencySpec {
        command: "unmute",
        requirement: ConsistencyRequirement::Enforced,
    },
    CommandConsistencySpec {
        command: "invite",
        requirement: ConsistencyRequirement::Accepted,
    },
    CommandConsistencySpec {
        command: "op",
        requirement: ConsistencyRequirement::Enforced,
    },
    CommandConsistencySpec {
        command: "deop",
        requirement: ConsistencyRequirement::Enforced,
    },
    CommandConsistencySpec {
        command: "mode",
        requirement: ConsistencyRequirement::Enforced,
    },
];

/// Resolution target namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveTarget {
    Authority,
    Channel,
    Context,
}

impl fmt::Display for ResolveTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Authority => write!(f, "authority"),
            Self::Channel => write!(f, "channel"),
            Self::Context => write!(f, "context"),
        }
    }
}

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
                return Ok(ExistingChannelResolution {
                    channel_id: ResolvedChannelId(channel_id),
                    context_id: ctx,
                });
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
                return Ok(ExistingChannelResolution {
                    channel_id: ResolvedChannelId(channel_id),
                    context_id,
                });
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

/// Execute a pre-planned command with no string re-resolution.
#[cfg(feature = "signals")]
// OWNERSHIP: observed
pub async fn execute_planned(
    app_core: &Arc<RwLock<AppCore>>,
    plan: PlannedCommand,
) -> Result<CommandExecutionResult, AuraError> {
    // Validate preconditions against current state to detect TOCTOU drift.
    {
        let snapshot = app_core.read().await.snapshot();
        let check = match &plan {
            PlannedCommand::Membership(p) => validate_preconditions(p, &snapshot),
            PlannedCommand::Moderation(p) => validate_preconditions(p, &snapshot),
            PlannedCommand::Moderator(p) => validate_preconditions(p, &snapshot),
            PlannedCommand::General(p) => validate_preconditions(p, &snapshot),
        };
        if let Err(e) = check {
            return Err(AuraError::invalid(format!("precondition failed: {e}")));
        }
    }

    let requirement = plan.consistency_requirement();
    let details = match &plan {
        PlannedCommand::Membership(plan) => execute_membership(app_core, plan).await?,
        PlannedCommand::Moderation(plan) => execute_moderation(app_core, plan).await?,
        PlannedCommand::Moderator(plan) => execute_moderator(app_core, plan).await?,
        PlannedCommand::General(plan) => execute_general(app_core, plan).await?,
    };

    let completion_outcome = wait_for_consistency(app_core, &plan, requirement).await;

    Ok(CommandExecutionResult {
        consistency_requirement: requirement,
        completion_outcome,
        details,
    })
}

#[cfg(not(feature = "signals"))]
pub async fn execute_planned(
    _app_core: &Arc<RwLock<AppCore>>,
    _plan: PlannedCommand,
) -> Result<CommandExecutionResult, AuraError> {
    Err(AuraError::agent(
        "strong command execution requires the `signals` feature",
    ))
}

#[cfg(feature = "signals")]
async fn wait_for_consistency(
    app_core: &Arc<RwLock<AppCore>>,
    plan: &PlannedCommand,
    requirement: ConsistencyRequirement,
) -> CommandCompletionOutcome {
    if requirement == ConsistencyRequirement::Accepted {
        return CommandCompletionOutcome::Satisfied(ConsistencyWitness::Accepted);
    }

    const CONSISTENCY_MAX_PASSES: usize = 8;
    let mut runtime_available = false;
    for _pass in 0..CONSISTENCY_MAX_PASSES {
        if consistency_invariant_holds(app_core, plan).await {
            return CommandCompletionOutcome::Satisfied(match requirement {
                ConsistencyRequirement::Accepted => ConsistencyWitness::Accepted,
                ConsistencyRequirement::Replicated => ConsistencyWitness::Replicated,
                ConsistencyRequirement::Enforced => ConsistencyWitness::Enforced,
            });
        }

        if let Ok(runtime) = require_runtime(app_core).await {
            runtime_available = true;
            converge_runtime(&runtime).await;
        } else if !runtime_available {
            // No runtime has been available for any pass.  Convergence
            // cannot make progress so bail early instead of spinning.
            #[cfg(feature = "instrumented")]
            tracing::warn!("consistency wait: no runtime available, returning degraded outcome");
            return CommandCompletionOutcome::Degraded {
                requirement,
                reason: ConsistencyDegradedReason::RuntimeUnavailable,
            };
        }
        cooperative_yield().await;
    }

    CommandCompletionOutcome::Degraded {
        requirement,
        reason: ConsistencyDegradedReason::OperationTimedOut,
    }
}

#[cfg(feature = "signals")]
async fn consistency_invariant_holds(
    app_core: &Arc<RwLock<AppCore>>,
    plan: &PlannedCommand,
) -> bool {
    // OWNERSHIP: observed
    let snapshot = app_core.read().await.snapshot();
    match plan {
        PlannedCommand::Membership(plan) => match &plan.operation.command {
            ResolvedCommand::Join { channel, .. } => channel
                .existing_channel()
                .is_some_and(|channel| snapshot.chat.channel(&channel.channel_id().0).is_some()),
            ResolvedCommand::Leave => match scope_channel_id(&plan.scope, "leave") {
                Ok(channel_id) => snapshot
                    .chat
                    .channel(&channel_id.0)
                    .is_none_or(|channel| channel.member_count == 0),
                Err(_) => false,
            },
            _ => false,
        },
        PlannedCommand::Moderation(plan) => {
            let home = match home_for_scope(&snapshot, &plan.scope) {
                Some(value) => value,
                None => return false,
            };
            match &plan.operation.command {
                ResolvedCommand::Kick { target, .. } => home.member(&target.0).is_none(),
                ResolvedCommand::Ban { target, .. } => home.ban_list.contains_key(&target.0),
                ResolvedCommand::Unban { target } => !home.ban_list.contains_key(&target.0),
                ResolvedCommand::Mute { target, .. } => home.mute_list.contains_key(&target.0),
                ResolvedCommand::Unmute { target } => !home.mute_list.contains_key(&target.0),
                ResolvedCommand::Invite { .. } => false,
                _ => false,
            }
        }
        PlannedCommand::Moderator(plan) => {
            let home = match home_for_scope(&snapshot, &plan.scope) {
                Some(value) => value,
                None => return false,
            };
            match &plan.operation.command {
                ResolvedCommand::Op { target } => home.member(&target.0).is_some_and(|member| {
                    matches!(member.role, crate::views::home::HomeRole::Moderator)
                }),
                ResolvedCommand::Deop { target } => home.member(&target.0).is_some_and(|member| {
                    matches!(member.role, crate::views::home::HomeRole::Participant)
                }),
                ResolvedCommand::Mode { flags, .. } => home.mode_flags.as_ref() == Some(flags),
                _ => false,
            }
        }
        // General commands are local-only operations (settings, config) that
        // take effect immediately without requiring convergence verification.
        PlannedCommand::General(_plan) => true,
    }
}

#[cfg(feature = "signals")]
fn home_for_scope<'a>(
    snapshot: &'a StateSnapshot,
    scope: &CommandScope,
) -> Option<&'a crate::views::home::HomeState> {
    match scope {
        CommandScope::Channel {
            channel_id,
            context_id,
        } => snapshot.homes.home_state(&channel_id.0).or_else(|| {
            context_id.and_then(|context| {
                snapshot
                    .homes
                    .iter()
                    .find(|(_, home)| home.context_id == Some(context.0))
                    .map(|(_, home)| home)
            })
        }),
        CommandScope::Context { context_id } => snapshot
            .homes
            .iter()
            .find(|(_, home)| home.context_id == Some(context_id.0))
            .map(|(_, home)| home),
        CommandScope::Global => snapshot.homes.current_home(),
    }
}

fn consistency_for_resolved(command: &ResolvedCommand) -> ConsistencyRequirement {
    let name = command_name(command);
    COMMAND_CONSISTENCY_TABLE
        .iter()
        .find_map(|spec| (spec.command == name).then_some(spec.requirement))
        .unwrap_or(ConsistencyRequirement::Accepted)
}

const fn consistency_witness_label(value: ConsistencyWitness) -> &'static str {
    match value {
        ConsistencyWitness::Accepted => "accepted",
        ConsistencyWitness::Replicated => "replicated",
        ConsistencyWitness::Enforced => "enforced",
    }
}

#[cfg(feature = "signals")]
async fn execute_membership(
    app_core: &Arc<RwLock<AppCore>>,
    plan: &CommandPlan<MembershipPlan>,
) -> Result<Option<String>, AuraError> {
    match &plan.operation.command {
        ResolvedCommand::Join {
            channel_name,
            channel,
        } => match channel {
            ChannelResolveOutcome::Existing(channel) => {
                let authoritative_channel =
                    messaging::require_authoritative_context_id_for_channel(
                        app_core,
                        channel.channel_id().0,
                    )
                    .await
                    .map(|context_id| {
                        messaging::authoritative_channel_ref(channel.channel_id().0, context_id)
                    })?;
                messaging::join_channel(app_core, authoritative_channel)
                    .await
                    .map(|_| ())
            }
            ChannelResolveOutcome::WillCreate { .. } => {
                messaging::join_channel_by_name(app_core, channel_name)
                    .await
                    .map(|_| ())
            }
        },
        ResolvedCommand::Leave => {
            let channel_id = scope_channel_id(&plan.scope, "leave")?;
            messaging::leave_channel(app_core, channel_id.0)
                .await
                .map(|_| ())
        }
        _ => Err(AuraError::invalid("invalid membership command")),
    }?;
    Ok(Some("membership updated".to_string()))
}

#[cfg(feature = "signals")]
async fn execute_moderation(
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
            moderation::ban_user_resolved(
                app_core,
                optional_scope_channel_id(&plan.scope),
                target.0,
                reason.as_deref(),
                timestamp_ms,
            )
            .await?;
            Ok(Some("ban applied".to_string()))
        }
        ResolvedCommand::Unban { target } => {
            moderation::unban_user_resolved(
                app_core,
                optional_scope_channel_id(&plan.scope),
                target.0,
            )
            .await?;
            Ok(Some("unban applied".to_string()))
        }
        ResolvedCommand::Mute { target, duration } => {
            moderation::mute_user_resolved(
                app_core,
                optional_scope_channel_id(&plan.scope),
                target.0,
                duration.map(|value| value.as_secs()),
                timestamp_ms,
            )
            .await?;
            Ok(Some("mute applied".to_string()))
        }
        ResolvedCommand::Unmute { target } => {
            moderation::unmute_user_resolved(
                app_core,
                optional_scope_channel_id(&plan.scope),
                target.0,
            )
            .await?;
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
async fn execute_moderator(
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
async fn execute_general(
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
            messaging::set_topic(app_core, channel_id.0, text, timestamp_ms)
                .await
                .map(|_| Some("topic updated".to_string()))
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
fn scope_channel_id(
    scope: &CommandScope,
    command: &'static str,
) -> Result<ResolvedChannelId, AuraError> {
    match scope {
        CommandScope::Channel { channel_id, .. } => Ok(*channel_id),
        _ => Err(AuraError::invalid(format!(
            "missing channel scope for /{command}"
        ))),
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

fn command_name(command: &ResolvedCommand) -> &'static str {
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

/// Declare a command executor that can only accept `ResolvedCommand`.
///
/// This is a compile-time signature guard to prevent new executor APIs from
/// accepting untyped command payloads.
///
/// ```rust,compile_fail
/// use aura_app::workflows::strong_command::strong_command_executor;
///
/// strong_command_executor!(
///     fn bad_executor(_app: (), _cmd: String) -> () {}
/// );
/// ```
#[macro_export]
#[allow(unused_macros)]
macro_rules! strong_command_executor {
    (
        $(#[$meta:meta])*
        $vis:vis fn $name:ident(
            $app:ident : $app_ty:ty,
            $cmd:ident : $cmd_ty:ty $(,)?
        ) -> $ret:ty $body:block
    ) => {
        const _: fn() = || {
            let _signature_guard: fn($cmd_ty) =
                |_resolved: $crate::ui::workflows::strong_command::ResolvedCommand| {};
        };

        $(#[$meta])*
        $vis fn $name($app: $app_ty, $cmd: $cmd_ty) -> $ret $body
    };
}

#[cfg(test)]
#[allow(clippy::default_trait_access, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::views::{Channel, ChannelType, ChatState, Contact, ContactsState};
    #[cfg(feature = "signals")]
    use crate::AppConfig;
    #[cfg(feature = "signals")]
    use crate::{
        signal_defs::AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL,
        ui_contract::{
            AuthoritativeSemanticFact, OperationId, SemanticOperationKind, SemanticOperationPhase,
        },
        workflows::signals::read_signal_or_default,
    };
    use proptest::prelude::*;

    #[tokio::test]
    async fn resolver_is_deterministic_for_repeated_resolution() {
        let app_core = crate::testing::default_test_app_core();
        let bob = Contact {
            id: AuthorityId::new_from_entropy([1u8; 32]),
            nickname: "bob".to_string(),
            nickname_suggestion: Some("Bobby".to_string()),
            is_guardian: false,
            is_member: true,
            last_interaction: None,
            is_online: true,
            read_receipt_policy: Default::default(),
            relationship_state: crate::views::contacts::ContactRelationshipState::Contact,
        };

        {
            let mut core = app_core.write().await;
            core.views_mut()
                .set_contacts(ContactsState::from_contacts(vec![bob.clone()]));
        }

        let resolver = CommandResolver::default();
        let snapshot = resolver.capture_snapshot(&app_core).await;
        let parsed = ParsedCommand::Kick {
            target: "bob".to_string(),
            reason: None,
        };

        let a = resolver.resolve(parsed.clone(), &snapshot).unwrap();
        let b = resolver.resolve(parsed, &snapshot).unwrap();
        assert_eq!(a, b);
    }

    #[tokio::test]
    async fn resolver_reports_ambiguous_authority_matches() {
        let app_core = crate::testing::default_test_app_core();
        let bob = Contact {
            id: AuthorityId::new_from_entropy([2u8; 32]),
            nickname: "bob".to_string(),
            nickname_suggestion: None,
            is_guardian: false,
            is_member: true,
            last_interaction: None,
            is_online: true,
            read_receipt_policy: Default::default(),
            relationship_state: crate::views::contacts::ContactRelationshipState::Contact,
        };
        let bobby = Contact {
            id: AuthorityId::new_from_entropy([3u8; 32]),
            nickname: "bobby".to_string(),
            nickname_suggestion: None,
            is_guardian: false,
            is_member: true,
            last_interaction: None,
            is_online: true,
            read_receipt_policy: Default::default(),
            relationship_state: crate::views::contacts::ContactRelationshipState::Contact,
        };

        {
            let mut core = app_core.write().await;
            core.views_mut()
                .set_contacts(ContactsState::from_contacts(vec![
                    bob.clone(),
                    bobby.clone(),
                ]));
        }

        let resolver = CommandResolver::default();
        let snapshot = resolver.capture_snapshot(&app_core).await;
        let err = resolver
            .resolve(
                ParsedCommand::Mute {
                    target: "bo".to_string(),
                    duration: None,
                },
                &snapshot,
            )
            .expect_err("expected ambiguity");

        match err {
            CommandResolverError::AmbiguousTarget {
                target,
                input,
                candidates,
            } => {
                assert_eq!(target, ResolveTarget::Authority);
                assert_eq!(input, "bo");
                assert_eq!(candidates.len(), 2);
                assert!(candidates.iter().any(|c| c == &bob.id.to_string()));
                assert!(candidates.iter().any(|c| c == &bobby.id.to_string()));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn resolver_reports_stale_snapshot_token() {
        let app_core = crate::testing::default_test_app_core();
        let bob = Contact {
            id: AuthorityId::new_from_entropy([4u8; 32]),
            nickname: "bob".to_string(),
            nickname_suggestion: None,
            is_guardian: false,
            is_member: true,
            last_interaction: None,
            is_online: true,
            read_receipt_policy: Default::default(),
            relationship_state: crate::views::contacts::ContactRelationshipState::Contact,
        };

        {
            let mut core = app_core.write().await;
            core.views_mut()
                .set_contacts(ContactsState::from_contacts(vec![bob]));
        }

        let resolver = CommandResolver::default();
        let stale_snapshot = resolver.capture_snapshot(&app_core).await;
        let _fresh_snapshot = resolver.capture_snapshot(&app_core).await;

        let err = resolver
            .resolve(
                ParsedCommand::Whois {
                    target: "bob".to_string(),
                },
                &stale_snapshot,
            )
            .expect_err("expected stale snapshot");

        match err {
            CommandResolverError::StaleSnapshot { provided, latest } => {
                assert!(provided.value() < latest.value());
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn resolver_accepts_explicit_authority_id_without_contact_entry() {
        let app_core = crate::testing::default_test_app_core();
        let resolver = CommandResolver::default();
        let snapshot = resolver.capture_snapshot(&app_core).await;
        let authority_id = AuthorityId::new_from_entropy([5u8; 32]);

        let resolved = resolver
            .resolve(
                ParsedCommand::Whois {
                    target: authority_id.to_string(),
                },
                &snapshot,
            )
            .expect("explicit authority ids should resolve without local contacts");

        match resolved {
            ResolvedCommand::Whois { target } => {
                assert_eq!(target, ResolvedAuthorityId(authority_id));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[tokio::test]
    async fn resolver_resolves_existing_channel_for_mode() {
        let app_core = crate::testing::default_test_app_core();
        let channel_id = ChannelId::from_bytes([9u8; 32]);

        {
            let mut core = app_core.write().await;
            core.views_mut()
                .set_chat(ChatState::from_channels(vec![Channel {
                    id: channel_id,
                    context_id: None,
                    name: "slash-lab".to_string(),
                    topic: None,
                    channel_type: ChannelType::Home,
                    unread_count: 0,
                    is_dm: false,
                    member_ids: Vec::new(),
                    member_count: 0,
                    last_message: None,
                    last_message_time: None,
                    last_activity: 0,
                    last_finalized_epoch: 0,
                }]));
        }

        let resolver = CommandResolver::default();
        let snapshot = resolver.capture_snapshot(&app_core).await;
        let resolved = resolver
            .resolve(
                ParsedCommand::Mode {
                    channel: "slash-lab".to_string(),
                    flags: "+m".to_string(),
                },
                &snapshot,
            )
            .expect("channel should resolve");

        match resolved {
            ResolvedCommand::Mode {
                channel,
                channel_name,
                flags,
                ..
            } => {
                assert_eq!(channel.channel_id(), ResolvedChannelId(channel_id));
                assert_eq!(channel_name, "slash-lab");
                assert_eq!(flags, "+m");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[tokio::test]
    async fn resolver_marks_unknown_join_channel_as_will_create() {
        let app_core = crate::testing::default_test_app_core();
        let resolver = CommandResolver::default();
        let snapshot = resolver.capture_snapshot(&app_core).await;

        let resolved = resolver
            .resolve(
                ParsedCommand::Join {
                    channel: "typo-room".to_string(),
                },
                &snapshot,
            )
            .expect("join should preserve create semantics");

        match resolved {
            ResolvedCommand::Join {
                channel_name,
                channel:
                    ChannelResolveOutcome::WillCreate {
                        channel_name: outcome_name,
                    },
            } => {
                assert_eq!(channel_name, "typo-room");
                assert_eq!(outcome_name, "typo-room");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[tokio::test]
    async fn resolver_rejects_unknown_join_channel_id_without_materialization() {
        let app_core = crate::testing::default_test_app_core();
        let resolver = CommandResolver::default();
        let snapshot = resolver.capture_snapshot(&app_core).await;
        let unknown_channel_id = ChannelId::from_bytes([9u8; 32]).to_string();

        let resolved = resolver.resolve(
            ParsedCommand::Join {
                channel: unknown_channel_id.clone(),
            },
            &snapshot,
        );

        assert_eq!(
            resolved,
            Err(CommandResolverError::UnknownTarget {
                target: ResolveTarget::Channel,
                input: unknown_channel_id,
            })
        );
    }

    #[tokio::test]
    async fn resolver_does_not_treat_home_names_as_existing_channels() {
        let app_core = crate::testing::default_test_app_core();
        let home_id = ChannelId::from_bytes([10u8; 32]);
        let owner = AuthorityId::new_from_entropy([16u8; 32]);
        let context_id = ContextId::new_from_entropy([17u8; 32]);

        {
            let mut core = app_core.write().await;
            let mut homes = core.views().get_homes();
            homes.add_home(crate::views::home::HomeState::new(
                home_id,
                Some("slash-lab".to_string()),
                owner,
                0,
                context_id,
            ));
            core.views_mut().set_homes(homes);
        }

        let resolver = CommandResolver::default();
        let snapshot = resolver.capture_snapshot(&app_core).await;

        let join = resolver
            .resolve(
                ParsedCommand::Join {
                    channel: "slash-lab".to_string(),
                },
                &snapshot,
            )
            .expect("join should treat unmatched channel name as create intent");
        match join {
            ResolvedCommand::Join {
                channel: ChannelResolveOutcome::WillCreate { channel_name },
                ..
            } => {
                assert_eq!(channel_name, "slash-lab");
            }
            other => panic!("unexpected join resolution: {other:?}"),
        }

        let mode = resolver.resolve(
            ParsedCommand::Mode {
                channel: "slash-lab".to_string(),
                flags: "+m".to_string(),
            },
            &snapshot,
        );
        assert!(matches!(
            mode,
            Err(CommandResolverError::UnknownTarget {
                target: ResolveTarget::Channel,
                ..
            })
        ));
    }

    #[test]
    fn moderation_plan_accepts_only_moderation_commands() {
        let target = ResolvedAuthorityId(AuthorityId::new_from_entropy([11u8; 32]));
        let valid = ResolvedCommand::Mute {
            target,
            duration: None,
        };
        let invalid = ResolvedCommand::Who;

        assert!(ModerationPlan::from_resolved(valid).is_ok());
        assert_eq!(
            ModerationPlan::from_resolved(invalid),
            Err(CommandPlanError::NotModerationCommand)
        );
    }

    #[test]
    fn moderator_plan_accepts_only_moderator_commands() {
        let target = ResolvedAuthorityId(AuthorityId::new_from_entropy([12u8; 32]));
        let valid = ResolvedCommand::Op { target };
        let invalid = ResolvedCommand::Leave;

        assert!(ModeratorPlan::from_resolved(valid).is_ok());
        assert_eq!(
            ModeratorPlan::from_resolved(invalid),
            Err(CommandPlanError::NotModeratorCommand)
        );
    }

    #[test]
    fn membership_plan_accepts_join_and_leave() {
        let valid_join = ResolvedCommand::Join {
            channel_name: "slash-lab".to_string(),
            channel: ChannelResolveOutcome::Existing(ExistingChannelResolution {
                channel_id: ResolvedChannelId(ChannelId::from_bytes([13u8; 32])),
                context_id: None,
            }),
        };
        let valid_leave = ResolvedCommand::Leave;
        let invalid = ResolvedCommand::Nick {
            name: "new-name".to_string(),
        };

        assert!(MembershipPlan::from_resolved(valid_join).is_ok());
        assert!(MembershipPlan::from_resolved(valid_leave).is_ok());
        assert_eq!(
            MembershipPlan::from_resolved(invalid),
            Err(CommandPlanError::NotMembershipCommand)
        );
    }

    #[test]
    fn consistency_table_matches_command_requirements() {
        let target = ResolvedAuthorityId(AuthorityId::new_from_entropy([14u8; 32]));
        let channel = ResolvedChannelId(ChannelId::from_bytes([15u8; 32]));
        let channel_mode = ResolvedCommand::Mode {
            channel_name: "slash-lab".to_string(),
            channel: ExistingChannelResolution {
                channel_id: channel,
                context_id: None,
            },
            flags: "+m".to_string(),
        };

        assert_eq!(
            consistency_for_resolved(&ResolvedCommand::Join {
                channel_name: "slash-lab".to_string(),
                channel: ChannelResolveOutcome::Existing(ExistingChannelResolution {
                    channel_id: channel,
                    context_id: None,
                }),
            }),
            ConsistencyRequirement::Replicated
        );
        assert_eq!(
            consistency_for_resolved(&ResolvedCommand::Mute {
                target,
                duration: None,
            }),
            ConsistencyRequirement::Enforced
        );
        assert_eq!(
            consistency_for_resolved(&channel_mode),
            ConsistencyRequirement::Enforced
        );
        assert_eq!(
            consistency_for_resolved(&ResolvedCommand::Who),
            ConsistencyRequirement::Accepted
        );
    }

    #[tokio::test]
    async fn join_create_plan_uses_global_scope_and_accepted_consistency() {
        let app_core = crate::testing::default_test_app_core();
        let resolver = CommandResolver::default();
        let snapshot = resolver.capture_snapshot(&app_core).await;

        let resolved = resolver
            .resolve(
                ParsedCommand::Join {
                    channel: "future-room".to_string(),
                },
                &snapshot,
            )
            .expect("join create intent should resolve");
        let plan = resolver
            .plan(resolved, &snapshot, None, None)
            .expect("join create plan should succeed");

        match &plan {
            PlannedCommand::Membership(plan) => {
                assert_eq!(plan.scope, CommandScope::Global);
                assert!(
                    plan.preconditions.is_empty(),
                    "create intent should not claim canonical channel preconditions"
                );
            }
            other => panic!("unexpected plan: {other:?}"),
        }
        assert_eq!(
            plan.consistency_requirement(),
            ConsistencyRequirement::Accepted
        );
    }

    proptest! {
        #[test]
        fn moderation_plan_preserves_canonical_target_id(
            entropy in any::<[u8; 32]>()
        ) {
            let expected = ResolvedAuthorityId(AuthorityId::new_from_entropy(entropy));
            let plan = ModerationPlan::from_resolved(ResolvedCommand::Mute {
                target: expected,
                duration: None,
            })
            .expect("mute command should plan");

            match plan.command {
                ResolvedCommand::Mute { target, .. } => prop_assert_eq!(target, expected),
                other => prop_assert!(false, "unexpected command shape: {other:?}"),
            }
        }
    }

    #[cfg(feature = "signals")]
    #[tokio::test]
    async fn whois_plan_does_not_reresolve_after_contacts_change() {
        let app_core = crate::testing::default_test_app_core();
        let original = Contact {
            id: AuthorityId::new_from_entropy([21u8; 32]),
            nickname: "bob".to_string(),
            nickname_suggestion: None,
            is_guardian: false,
            is_member: true,
            last_interaction: None,
            is_online: true,
            read_receipt_policy: Default::default(),
            relationship_state: crate::views::contacts::ContactRelationshipState::Contact,
        };
        let replacement = Contact {
            id: AuthorityId::new_from_entropy([22u8; 32]),
            nickname: "bob".to_string(),
            nickname_suggestion: None,
            is_guardian: false,
            is_member: true,
            last_interaction: None,
            is_online: true,
            read_receipt_policy: Default::default(),
            relationship_state: crate::views::contacts::ContactRelationshipState::Contact,
        };

        {
            let mut core = app_core.write().await;
            core.views_mut()
                .set_contacts(ContactsState::from_contacts(vec![original.clone()]));
        }

        let resolver = CommandResolver::default();
        let snapshot = resolver.capture_snapshot(&app_core).await;
        let resolved = resolver
            .resolve(
                ParsedCommand::Whois {
                    target: "bob".to_string(),
                },
                &snapshot,
            )
            .expect("initial resolve should succeed");
        let plan = resolver
            .plan(resolved, &snapshot, None, None)
            .expect("planning should succeed");

        {
            let mut core = app_core.write().await;
            core.views_mut()
                .set_contacts(ContactsState::from_contacts(vec![replacement]));
        }

        let error = execute_planned(&app_core, plan)
            .await
            .expect_err("planned whois should not reresolve to replacement contact");
        assert!(
            error.to_string().contains(&original.id.to_string()),
            "expected missing original authority id in error, got: {error}"
        );
    }

    #[cfg(feature = "signals")]
    #[tokio::test]
    async fn consistency_barrier_reports_runtime_unavailable_degraded_state() {
        let app_core = crate::testing::default_test_app_core();
        let target = ResolvedAuthorityId(AuthorityId::new_from_entropy([31u8; 32]));
        let plan = PlannedCommand::Moderator(CommandPlan {
            actor: None,
            scope: CommandScope::Global,
            preconditions: vec![PlanPrecondition::TargetExists(target)],
            operation: ModeratorPlan {
                command: ResolvedCommand::Op { target },
            },
        });

        let state = wait_for_consistency(&app_core, &plan, ConsistencyRequirement::Enforced).await;
        assert_eq!(
            state,
            CommandCompletionOutcome::Degraded {
                requirement: ConsistencyRequirement::Enforced,
                reason: ConsistencyDegradedReason::RuntimeUnavailable,
            }
        );
    }

    #[cfg(feature = "signals")]
    #[tokio::test]
    async fn consistency_barrier_reports_replicated_when_join_is_visible() {
        let app_core = crate::testing::default_test_app_core();
        let channel_id = ChannelId::from_bytes([32u8; 32]);

        {
            let mut core = app_core.write().await;
            core.views_mut()
                .set_chat(ChatState::from_channels(vec![Channel {
                    id: channel_id,
                    context_id: None,
                    name: "replicated-room".to_string(),
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
                }]));
        }

        let plan = PlannedCommand::Membership(CommandPlan {
            actor: None,
            scope: CommandScope::Channel {
                channel_id: ResolvedChannelId(channel_id),
                context_id: None,
            },
            preconditions: vec![PlanPrecondition::ChannelExists(ResolvedChannelId(
                channel_id,
            ))],
            operation: MembershipPlan {
                command: ResolvedCommand::Join {
                    channel_name: "replicated-room".to_string(),
                    channel: ChannelResolveOutcome::Existing(ExistingChannelResolution {
                        channel_id: ResolvedChannelId(channel_id),
                        context_id: None,
                    }),
                },
            },
        });

        let state =
            wait_for_consistency(&app_core, &plan, ConsistencyRequirement::Replicated).await;
        assert_eq!(
            state,
            CommandCompletionOutcome::Satisfied(ConsistencyWitness::Replicated)
        );
    }

    #[cfg(feature = "signals")]
    #[tokio::test]
    async fn consistency_barrier_treats_missing_leave_scope_as_replicated() {
        let app_core = crate::testing::default_test_app_core();
        let missing_channel = ChannelId::from_bytes([44u8; 32]);

        let plan = PlannedCommand::Membership(CommandPlan {
            actor: None,
            scope: CommandScope::Channel {
                channel_id: ResolvedChannelId(missing_channel),
                context_id: None,
            },
            preconditions: Vec::new(),
            operation: MembershipPlan {
                command: ResolvedCommand::Leave,
            },
        });

        let state =
            wait_for_consistency(&app_core, &plan, ConsistencyRequirement::Replicated).await;
        assert_eq!(
            state,
            CommandCompletionOutcome::Satisfied(ConsistencyWitness::Replicated)
        );
    }

    #[cfg(feature = "signals")]
    #[tokio::test]
    async fn consistency_barrier_treats_missing_home_scope_as_timed_out_degraded_state() {
        let authority = AuthorityId::new_from_entropy([90u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(authority));
        let runtime_bridge: Arc<dyn crate::runtime_bridge::RuntimeBridge> = runtime;
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(AppConfig::default(), runtime_bridge).unwrap(),
        ));
        let missing_channel = ChannelId::from_bytes([45u8; 32]);
        let target = ResolvedAuthorityId(AuthorityId::new_from_entropy([46u8; 32]));

        let plan = PlannedCommand::Moderation(CommandPlan {
            actor: None,
            scope: CommandScope::Channel {
                channel_id: ResolvedChannelId(missing_channel),
                context_id: None,
            },
            preconditions: vec![PlanPrecondition::TargetExists(target)],
            operation: ModerationPlan {
                command: ResolvedCommand::Kick {
                    target,
                    reason: None,
                },
            },
        });

        let state = wait_for_consistency(&app_core, &plan, ConsistencyRequirement::Enforced).await;
        assert_eq!(
            state,
            CommandCompletionOutcome::Degraded {
                requirement: ConsistencyRequirement::Enforced,
                reason: ConsistencyDegradedReason::OperationTimedOut,
            }
        );
    }

    #[tokio::test]
    async fn invite_plan_uses_accepted_consistency_requirement() {
        let target = ResolvedAuthorityId(AuthorityId::new_from_entropy([49u8; 32]));
        let channel_id = ResolvedChannelId(ChannelId::from_bytes([50u8; 32]));
        let plan = PlannedCommand::Moderation(CommandPlan {
            actor: None,
            scope: CommandScope::Channel {
                channel_id,
                context_id: None,
            },
            preconditions: vec![
                PlanPrecondition::TargetExists(target),
                PlanPrecondition::ChannelExists(channel_id),
            ],
            operation: ModerationPlan {
                command: ResolvedCommand::Invite { target },
            },
        });

        assert_eq!(
            plan.consistency_requirement(),
            ConsistencyRequirement::Accepted
        );
    }

    #[cfg(feature = "signals")]
    #[tokio::test]
    async fn home_for_channel_scope_does_not_fallback_to_current_home() {
        use crate::views::home::HomesState;

        let scoped_channel_id = ChannelId::from_bytes([51u8; 32]);
        let current_home_id = ChannelId::from_bytes([52u8; 32]);
        let current_context_id = ContextId::new_from_entropy([53u8; 32]);
        let creator = AuthorityId::new_from_entropy([54u8; 32]);

        let mut homes = HomesState::new();
        let result = homes.add_home(crate::views::home::HomeState::new(
            current_home_id,
            Some("current-home".to_string()),
            creator,
            0,
            current_context_id,
        ));
        if result.was_first {
            homes.select_home(Some(result.home_id));
        }

        let snapshot = StateSnapshot {
            homes,
            ..StateSnapshot::default()
        };

        let resolved = home_for_scope(
            &snapshot,
            &CommandScope::Channel {
                channel_id: ResolvedChannelId(scoped_channel_id),
                context_id: None,
            },
        );
        assert!(
            resolved.is_none(),
            "channel-scoped lookup should not silently fall back to current home"
        );
    }

    #[cfg(feature = "signals")]
    #[tokio::test]
    async fn execute_planned_join_preserves_join_operation_id() {
        let app_core = crate::testing::default_test_app_core();
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let resolver = CommandResolver::default();
        let snapshot = resolver.capture_snapshot(&app_core).await;
        let resolved = resolver
            .resolve(
                ParsedCommand::Join {
                    channel: "semantic-room".to_string(),
                },
                &snapshot,
            )
            .expect("join should resolve");
        let plan = resolver
            .plan(resolved, &snapshot, None, None)
            .expect("join should plan");

        execute_planned(&app_core, plan)
            .await
            .expect("join should execute");

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts.iter().any(|fact| matches!(
            fact,
            AuthoritativeSemanticFact::OperationStatus {
                operation_id,
                status,
                ..
            } if *operation_id == OperationId::join_channel()
                && status.kind == SemanticOperationKind::JoinChannel
                && status.phase == SemanticOperationPhase::Succeeded
        )));
    }

    #[cfg(feature = "signals")]
    #[tokio::test]
    async fn execute_planned_me_preserves_send_message_operation_id() {
        let authority = AuthorityId::new_from_entropy([47u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(authority));
        let channel_id = ChannelId::from_bytes([44u8; 32]);
        let context_id = ContextId::new_from_entropy([48u8; 32]);
        runtime.set_materialized_channel_name_matches("semantic-room", vec![channel_id]);
        runtime.set_amp_channel_context(channel_id, context_id);
        let runtime_bridge: Arc<dyn crate::runtime_bridge::RuntimeBridge> = runtime;
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(AppConfig::default(), runtime_bridge).unwrap(),
        ));
        {
            let core = app_core.read().await;
            crate::signal_defs::register_app_signals(&*core)
                .await
                .unwrap();
        }

        {
            let mut core = app_core.write().await;
            core.views_mut()
                .set_chat(ChatState::from_channels(vec![Channel {
                    id: channel_id,
                    context_id: Some(context_id),
                    name: "semantic-room".to_string(),
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
                }]));
        }

        let resolver = CommandResolver::default();
        let snapshot = resolver.capture_snapshot(&app_core).await;
        let resolved = resolver
            .resolve(
                ParsedCommand::Me {
                    action: "wave".to_string(),
                },
                &snapshot,
            )
            .expect("me should resolve");
        let plan = resolver
            .plan(resolved, &snapshot, Some("semantic-room"), None)
            .expect("me should plan");

        let _error = execute_planned(&app_core, plan)
            .await
            .expect_err("me should still fail explicitly without a full runtime transport");

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts.iter().any(|fact| matches!(
            fact,
            AuthoritativeSemanticFact::OperationStatus {
                operation_id,
                status,
                ..
            } if *operation_id == OperationId::send_message()
                && status.kind == SemanticOperationKind::SendChatMessage
        )));
    }

    #[test]
    fn command_completion_outcome_maps_timeout_without_terminal_string_parsing() {
        let classification = CommandCompletionOutcome::Degraded {
            requirement: ConsistencyRequirement::Enforced,
            reason: ConsistencyDegradedReason::OperationTimedOut,
        }
        .terminal_classification()
        .expect("degraded timeout should classify");

        assert_eq!(classification.status, CommandTerminalOutcomeStatus::Failed);
        assert_eq!(
            classification.reason,
            CommandTerminalReasonCode::OperationTimedOut
        );
    }

    #[test]
    fn command_completion_outcome_maps_runtime_unavailable_without_terminal_string_parsing() {
        let classification = CommandCompletionOutcome::Degraded {
            requirement: ConsistencyRequirement::Replicated,
            reason: ConsistencyDegradedReason::RuntimeUnavailable,
        }
        .terminal_classification()
        .expect("runtime unavailable should classify");

        assert_eq!(classification.status, CommandTerminalOutcomeStatus::Failed);
        assert_eq!(
            classification.reason,
            CommandTerminalReasonCode::Unavailable
        );
    }

    #[test]
    fn classify_terminal_execution_error_maps_unknown_precondition_to_not_found() {
        let classification = classify_terminal_execution_error(&AuraError::invalid(
            "precondition failed: unknown channel target: channel-123",
        ));

        assert_eq!(classification.status, CommandTerminalOutcomeStatus::Invalid);
        assert_eq!(classification.reason, CommandTerminalReasonCode::NotFound);
    }

    #[test]
    fn classify_terminal_execution_error_maps_permission_detail_without_terminal_string_parsing() {
        let classification =
            classify_terminal_execution_error(&AuraError::permission_denied("target is muted"));

        assert_eq!(classification.status, CommandTerminalOutcomeStatus::Denied);
        assert_eq!(classification.reason, CommandTerminalReasonCode::Muted);
    }
}
