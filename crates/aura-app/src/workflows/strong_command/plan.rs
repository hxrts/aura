#![allow(missing_docs)]

use super::resolved_refs::{
    ResolvedAuthorityId, ResolvedChannelId, ResolvedCommand, ResolvedContextId,
};

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanPrecondition {
    TargetExists(ResolvedAuthorityId),
    ChannelExists(ResolvedChannelId),
    ActorInScope,
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
