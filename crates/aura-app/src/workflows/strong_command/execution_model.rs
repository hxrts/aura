#![allow(missing_docs)]

use super::plan::{CommandPlan, MembershipPlan, ModerationPlan, ModeratorPlan};
use super::resolved_refs::ResolvedCommand;

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
            Self::General(plan) => super::consistency::consistency_for_resolved(&plan.operation),
            Self::Membership(plan) => match &plan.operation.command {
                ResolvedCommand::Join {
                    channel: super::resolved_refs::ChannelResolveOutcome::WillCreate { .. },
                    ..
                } => ConsistencyRequirement::Accepted,
                _ => super::consistency::consistency_for_resolved(&plan.operation.command),
            },
            Self::Moderation(plan) => {
                super::consistency::consistency_for_resolved(&plan.operation.command)
            }
            Self::Moderator(plan) => {
                super::consistency::consistency_for_resolved(&plan.operation.command)
            }
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

const fn consistency_witness_label(witness: ConsistencyWitness) -> &'static str {
    match witness {
        ConsistencyWitness::Accepted => "accepted",
        ConsistencyWitness::Replicated => "replicated",
        ConsistencyWitness::Enforced => "enforced",
    }
}
