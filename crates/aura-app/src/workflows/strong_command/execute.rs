#![allow(missing_docs)]

#[cfg(feature = "signals")]
use super::consistency::{validate_preconditions, wait_for_consistency};
#[cfg(feature = "signals")]
use super::dispatch::{execute_general, execute_membership, execute_moderation, execute_moderator};
use super::execution_model::{CommandExecutionResult, PlannedCommand};
use crate::AppCore;
use async_lock::RwLock;
use aura_core::AuraError;
use std::sync::Arc;

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
        if let Err(error) = check {
            return Err(AuraError::invalid(format!("precondition failed: {error}")));
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
