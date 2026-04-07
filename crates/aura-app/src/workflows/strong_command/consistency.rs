#![allow(missing_docs)]

#[cfg(feature = "signals")]
use super::execution_model::{
    CommandCompletionOutcome, ConsistencyDegradedReason, ConsistencyWitness, PlannedCommand,
};
use super::execution_model::{ConsistencyRequirement, COMMAND_CONSISTENCY_TABLE};
#[cfg(feature = "signals")]
use super::plan::{CommandPlan, CommandScope, PlanPrecondition};
#[cfg(feature = "signals")]
use super::resolve::CommandResolverError;
#[cfg(feature = "signals")]
use super::resolved_refs::ResolvedChannelId;
use super::resolved_refs::ResolvedCommand;
#[cfg(feature = "signals")]
use crate::core::StateSnapshot;
#[cfg(feature = "signals")]
use crate::signal_defs::{CHAT_SIGNAL, CHAT_SIGNAL_NAME};
#[cfg(feature = "signals")]
use crate::workflows::observed_projection::homes_signal_snapshot;
#[cfg(feature = "signals")]
use crate::workflows::runtime::{converge_runtime, cooperative_yield, require_runtime};
#[cfg(feature = "signals")]
use crate::workflows::signals::read_signal;
#[cfg(feature = "signals")]
use crate::AppCore;
#[cfg(feature = "signals")]
use async_lock::RwLock;
#[cfg(feature = "signals")]
use aura_core::AuraError;
#[cfg(feature = "signals")]
use std::sync::Arc;

#[cfg(feature = "signals")]
pub(super) fn validate_preconditions<T>(
    plan: &CommandPlan<T>,
    snapshot: &crate::core::StateSnapshot,
) -> Result<(), CommandResolverError> {
    for precondition in &plan.preconditions {
        match precondition {
            PlanPrecondition::TargetExists(target) => {
                if snapshot.contacts.contact(&target.0).is_none() {
                    return Err(CommandResolverError::UnknownTarget {
                        target: super::ResolveTarget::Authority,
                        input: target.0.to_string(),
                    });
                }
            }
            PlanPrecondition::ChannelExists(channel) => {
                if snapshot.chat.channel(&channel.0).is_none() {
                    return Err(CommandResolverError::UnknownTarget {
                        target: super::ResolveTarget::Channel,
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
    let name = super::resolve::command_name(command);
    COMMAND_CONSISTENCY_TABLE
        .iter()
        .find_map(|spec| (spec.command == name).then_some(spec.requirement))
        .unwrap_or(ConsistencyRequirement::Accepted)
}

#[cfg(feature = "signals")]
pub(super) fn scope_channel_id(
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
