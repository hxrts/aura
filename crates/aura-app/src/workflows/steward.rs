//! Steward Workflow - Portable Business Logic
//!
//! This module contains steward role management operations that are portable across all frontends.
//! Stewards (Admins) have elevated permissions within a home.

use crate::workflows::parse::parse_authority_id;
use crate::workflows::runtime::require_runtime;
use crate::{views::home::ResidentRole, AppCore};
use async_lock::RwLock;
use aura_core::identifiers::AuthorityId;
use aura_core::AuraError;
use aura_journal::DomainFact;
use aura_social::moderation::facts::{HomeGrantStewardFact, HomeRevokeStewardFact};
use std::sync::Arc;

fn map_runtime_error(operation: &str, error: impl std::fmt::Display) -> AuraError {
    AuraError::agent(format!("{operation} failed: {error}"))
}

/// Grant steward (Admin) role to a resident.
///
/// Authorization: Only Owner or Admin can grant steward role.
/// Cannot promote Owner (Owner is immutable).
pub async fn grant_steward(app_core: &Arc<RwLock<AppCore>>, target: &str) -> Result<(), AuraError> {
    let target_id = parse_authority_id(target)?;

    // Validate current view and collect context/peer fanout.
    let (context_id, peers) = {
        let core = app_core.read().await;
        let homes = core.views().get_homes();
        let home_state = homes
            .current_home()
            .ok_or_else(|| AuraError::not_found("No current home selected"))?;

        if !home_state.is_admin() {
            return Err(AuraError::permission_denied(
                "Only stewards can grant steward role",
            ));
        }

        let resident = home_state
            .resident(&target_id)
            .ok_or_else(|| AuraError::not_found(format!("Resident not found: {target}")))?;

        if matches!(resident.role, ResidentRole::Owner) {
            return Err(AuraError::invalid("Cannot modify Owner role"));
        }

        let context_id = home_state
            .context_id
            .ok_or_else(|| AuraError::not_found("Home has no context ID"))?;
        let peers = home_state
            .residents
            .iter()
            .map(|resident| resident.id)
            .collect::<Vec<AuthorityId>>();

        (context_id, peers)
    };

    // Runtime-backed propagation when available. Keep local mutation below for
    // immediate UX even if runtime is not configured (tests/local-only callers).
    if let Ok(runtime) = require_runtime(app_core).await {
        let now_ms = runtime
            .current_time_ms()
            .await
            .map_err(|e| map_runtime_error("Grant steward timestamp", e))?;
        let actor = runtime.authority_id();
        let fact = HomeGrantStewardFact::new_ms(context_id, target_id, actor, now_ms).to_generic();

        runtime
            .commit_relational_facts(std::slice::from_ref(&fact))
            .await
            .map_err(|e| map_runtime_error("Commit steward grant fact", e))?;

        for peer in peers {
            if peer == actor {
                continue;
            }
            let _ = runtime.send_chat_fact(peer, context_id, &fact).await;
        }
    }

    // Local state mutation.
    let mut core = app_core.write().await;
    let mut homes = core.views().get_homes();
    let home_state = homes
        .current_home_mut()
        .ok_or_else(|| AuraError::not_found("No current home selected"))?;

    let resident = home_state
        .resident_mut(&target_id)
        .ok_or_else(|| AuraError::not_found(format!("Resident not found: {target}")))?;

    if matches!(resident.role, ResidentRole::Owner) {
        return Err(AuraError::invalid("Cannot modify Owner role"));
    }

    resident.role = ResidentRole::Admin;
    core.views_mut().set_homes(homes);

    Ok(())
}

/// Revoke steward (Admin) role from a resident.
///
/// Authorization: Only Owner or Admin can revoke steward role.
/// Can only demote Admin, not Owner or Resident.
pub async fn revoke_steward(
    app_core: &Arc<RwLock<AppCore>>,
    target: &str,
) -> Result<(), AuraError> {
    let target_id = parse_authority_id(target)?;

    let (context_id, peers) = {
        let core = app_core.read().await;
        let homes = core.views().get_homes();
        let home_state = homes
            .current_home()
            .ok_or_else(|| AuraError::not_found("No current home selected"))?;

        if !home_state.is_admin() {
            return Err(AuraError::permission_denied(
                "Only stewards can revoke steward role",
            ));
        }

        let resident = home_state
            .resident(&target_id)
            .ok_or_else(|| AuraError::not_found(format!("Resident not found: {target}")))?;

        if !matches!(resident.role, ResidentRole::Admin) {
            return Err(AuraError::invalid(
                "Can only revoke Admin role, not Owner or Resident",
            ));
        }

        let context_id = home_state
            .context_id
            .ok_or_else(|| AuraError::not_found("Home has no context ID"))?;
        let peers = home_state
            .residents
            .iter()
            .map(|resident| resident.id)
            .collect::<Vec<AuthorityId>>();

        (context_id, peers)
    };

    if let Ok(runtime) = require_runtime(app_core).await {
        let now_ms = runtime
            .current_time_ms()
            .await
            .map_err(|e| map_runtime_error("Revoke steward timestamp", e))?;
        let actor = runtime.authority_id();
        let fact = HomeRevokeStewardFact::new_ms(context_id, target_id, actor, now_ms).to_generic();

        runtime
            .commit_relational_facts(std::slice::from_ref(&fact))
            .await
            .map_err(|e| map_runtime_error("Commit steward revoke fact", e))?;

        for peer in peers {
            if peer == actor {
                continue;
            }
            let _ = runtime.send_chat_fact(peer, context_id, &fact).await;
        }
    }

    let mut core = app_core.write().await;
    let mut homes = core.views().get_homes();
    let home_state = homes
        .current_home_mut()
        .ok_or_else(|| AuraError::not_found("No current home selected"))?;

    let resident = home_state
        .resident_mut(&target_id)
        .ok_or_else(|| AuraError::not_found(format!("Resident not found: {target}")))?;

    if !matches!(resident.role, ResidentRole::Admin) {
        return Err(AuraError::invalid(
            "Can only revoke Admin role, not Owner or Resident",
        ));
    }

    resident.role = ResidentRole::Resident;
    core.views_mut().set_homes(homes);

    Ok(())
}

/// Check if current user is admin in current home.
pub async fn is_admin(app_core: &Arc<RwLock<AppCore>>) -> bool {
    let core = app_core.read().await;
    let homes = core.views().get_homes();

    homes
        .current_home()
        .map(|home_state| home_state.is_admin())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppConfig;

    #[tokio::test]
    async fn test_is_admin_no_home() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let is_admin_result = is_admin(&app_core).await;
        assert!(!is_admin_result);
    }

    #[tokio::test]
    async fn test_grant_steward_no_home() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let result = grant_steward(&app_core, "user-123").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_revoke_steward_no_home() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let result = revoke_steward(&app_core, "user-123").await;
        assert!(result.is_err());
    }
}
