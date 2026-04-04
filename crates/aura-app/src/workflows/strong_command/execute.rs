#![allow(missing_docs)]

use super::resolve::{
    command_name, CommandPlan, MembershipPlan, ModerationPlan, ModeratorPlan, ResolvedCommand,
};
#[cfg(feature = "signals")]
use crate::signal_defs::{CHAT_SIGNAL, CHAT_SIGNAL_NAME};
#[cfg(feature = "signals")]
use crate::workflows::observed_projection::homes_signal_snapshot;
#[cfg(feature = "signals")]
use crate::workflows::runtime::{converge_runtime, cooperative_yield, require_runtime};
#[cfg(feature = "signals")]
use crate::workflows::signals::read_signal;
#[cfg(feature = "signals")]
use crate::workflows::{context, invitation, messaging, moderation, moderator, query, settings};
use crate::AppCore;
use async_lock::RwLock;
use aura_core::AuraError;
use std::sync::Arc;

#[cfg(feature = "signals")]
use super::resolve::{CommandResolverError, CommandScope, PlanPrecondition, ResolvedChannelId};
#[cfg(feature = "signals")]
use crate::core::StateSnapshot;
#[cfg(feature = "signals")]
use aura_core::types::identifiers::ChannelId;

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
                    channel: super::resolve::ChannelResolveOutcome::WillCreate { .. },
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

/// Execute a pre-planned command with no string re-resolution.
#[cfg(feature = "signals")]
// OWNERSHIP: observed
pub async fn execute_planned(
    app_core: &Arc<RwLock<AppCore>>,
    plan: PlannedCommand,
) -> Result<CommandExecutionResult, AuraError> {
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
fn validate_preconditions<T>(
    plan: &CommandPlan<T>,
    snapshot: &crate::core::StateSnapshot,
) -> Result<(), CommandResolverError> {
    for precondition in &plan.preconditions {
        match precondition {
            PlanPrecondition::TargetExists(target) => {
                if snapshot.contacts.contact(&target.0).is_none() {
                    return Err(CommandResolverError::UnknownTarget {
                        target: super::resolve::ResolveTarget::Authority,
                        input: target.0.to_string(),
                    });
                }
            }
            PlanPrecondition::ChannelExists(channel) => {
                if snapshot.chat.channel(&channel.0).is_none() {
                    return Err(CommandResolverError::UnknownTarget {
                        target: super::resolve::ResolveTarget::Channel,
                        input: channel.0.to_string(),
                    });
                }
            }
            PlanPrecondition::ActorInScope => {}
        }
    }
    Ok(())
}

#[cfg(feature = "signals")]
pub(super) async fn wait_for_consistency(
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
    match plan {
        PlannedCommand::Membership(plan) => {
            let Ok(chat) = read_signal(app_core, &*CHAT_SIGNAL, CHAT_SIGNAL_NAME).await else {
                return false;
            };
            match &plan.operation.command {
                ResolvedCommand::Join { channel, .. } => channel
                    .existing_channel()
                    .is_some_and(|channel| chat.channel(&channel.channel_id().0).is_some()),
                ResolvedCommand::Leave => match scope_channel_id(&plan.scope, "leave") {
                    Ok(channel_id) => chat
                        .channel(&channel_id.0)
                        .is_none_or(|channel| channel.member_count == 0),
                    Err(_) => false,
                },
                _ => false,
            }
        }
        PlannedCommand::Moderation(plan) => {
            let Ok(homes) = homes_signal_snapshot(app_core).await else {
                return false;
            };
            let snapshot = StateSnapshot {
                homes,
                ..StateSnapshot::default()
            };
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
            let Ok(homes) = homes_signal_snapshot(app_core).await else {
                return false;
            };
            let snapshot = StateSnapshot {
                homes,
                ..StateSnapshot::default()
            };
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
        PlannedCommand::General(_) => true,
    }
}

#[cfg(feature = "signals")]
pub(super) fn home_for_scope<'a>(
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

pub(super) fn consistency_for_resolved(command: &ResolvedCommand) -> ConsistencyRequirement {
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
            super::resolve::ChannelResolveOutcome::Existing(channel) => {
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
            super::resolve::ChannelResolveOutcome::WillCreate { .. } => {
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
