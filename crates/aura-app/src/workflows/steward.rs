//! Steward Workflow - Portable Business Logic
//!
//! This module contains steward role management operations that are portable across all frontends.
//! Stewards (Admins) have elevated permissions within a block.

use crate::{views::block::ResidentRole, AppCore};
use async_lock::RwLock;
use aura_core::AuraError;
use std::sync::Arc;

/// Grant steward (Admin) role to a resident
///
/// **What it does**: Promotes a resident to Admin role
/// **Returns**: Unit result
/// **Signal pattern**: Updates blocks view state directly
///
/// Authorization: Only Owner or Admin can grant steward role.
/// Cannot promote Owner (Owner is immutable).
pub async fn grant_steward(app_core: &Arc<RwLock<AppCore>>, target: &str) -> Result<(), AuraError> {
    let mut core = app_core.write().await;
    let mut blocks = core.views().get_blocks().clone();

    let block = blocks
        .current_block_mut()
        .ok_or_else(|| AuraError::not_found("No current block selected"))?;

    // Check if actor is authorized (must be Owner or Admin)
    if !block.is_admin() {
        return Err(AuraError::permission_denied(
            "Only stewards can grant steward role",
        ));
    }

    // Find and update the target resident
    let resident = block
        .resident_mut(target)
        .ok_or_else(|| AuraError::not_found(format!("Resident not found: {}", target)))?;

    // Can't promote an Owner
    if matches!(resident.role, ResidentRole::Owner) {
        return Err(AuraError::invalid("Cannot modify Owner role"));
    }

    // Promote to Admin
    resident.role = ResidentRole::Admin;
    core.views_mut().set_blocks(blocks);

    Ok(())
}

/// Revoke steward (Admin) role from a resident
///
/// **What it does**: Demotes an Admin to Resident role
/// **Returns**: Unit result
/// **Signal pattern**: Updates blocks view state directly
///
/// Authorization: Only Owner or Admin can revoke steward role.
/// Can only demote Admin, not Owner or Resident.
pub async fn revoke_steward(
    app_core: &Arc<RwLock<AppCore>>,
    target: &str,
) -> Result<(), AuraError> {
    let mut core = app_core.write().await;
    let mut blocks = core.views().get_blocks().clone();

    let block = blocks
        .current_block_mut()
        .ok_or_else(|| AuraError::not_found("No current block selected"))?;

    // Check if actor is authorized (must be Owner or Admin)
    if !block.is_admin() {
        return Err(AuraError::permission_denied(
            "Only stewards can revoke steward role",
        ));
    }

    // Find and update the target resident
    let resident = block
        .resident_mut(target)
        .ok_or_else(|| AuraError::not_found(format!("Resident not found: {}", target)))?;

    // Can only demote Admin, not Owner
    if !matches!(resident.role, ResidentRole::Admin) {
        return Err(AuraError::invalid(
            "Can only revoke Admin role, not Owner or Resident",
        ));
    }

    // Demote to Resident
    resident.role = ResidentRole::Resident;
    core.views_mut().set_blocks(blocks);

    Ok(())
}

/// Check if current user is admin in current block
///
/// **What it does**: Checks admin status in current block
/// **Returns**: Boolean indicating admin status
/// **Signal pattern**: Read-only operation (no emission)
pub async fn is_admin(app_core: &Arc<RwLock<AppCore>>) -> bool {
    let core = app_core.read().await;
    let blocks = core.views().get_blocks();

    blocks
        .current_block()
        .map(|block| block.is_admin())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppConfig;

    #[tokio::test]
    async fn test_is_admin_no_block() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let is_admin_result = is_admin(&app_core).await;
        assert!(!is_admin_result);
    }

    #[tokio::test]
    async fn test_grant_steward_no_block() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let result = grant_steward(&app_core, "user-123").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_revoke_steward_no_block() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let result = revoke_steward(&app_core, "user-123").await;
        assert!(result.is_err());
    }
}
