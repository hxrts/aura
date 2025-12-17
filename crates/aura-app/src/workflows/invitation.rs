//! Invitation Workflow - Portable Business Logic
//!
//! This module contains invitation operations that are portable across
//! all frontends via the RuntimeBridge abstraction.

use crate::runtime_bridge::InvitationInfo;
use crate::{views::invitations::InvitationsState, AppCore, INVITATIONS_SIGNAL};
use async_lock::RwLock;
use aura_core::identifiers::AuthorityId;
use aura_core::{effects::reactive::ReactiveEffects, AuraError};
use std::sync::Arc;

// ============================================================================
// Invitation Creation via RuntimeBridge
// ============================================================================

/// Create a contact invitation
///
/// **What it does**: Creates an invitation to become a contact
/// **Returns**: InvitationInfo with the created invitation details
/// **Signal pattern**: RuntimeBridge handles state updates
pub async fn create_contact_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    receiver: AuthorityId,
    petname: Option<String>,
    message: Option<String>,
    ttl_ms: Option<u64>,
) -> Result<InvitationInfo, AuraError> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .ok_or_else(|| AuraError::agent("Runtime bridge not available"))?
            .clone()
    };

    runtime
        .create_contact_invitation(receiver, petname, message, ttl_ms)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to create contact invitation: {}", e)))
}

/// Create a guardian invitation
///
/// **What it does**: Creates an invitation to become a guardian
/// **Returns**: InvitationInfo with the created invitation details
/// **Signal pattern**: RuntimeBridge handles state updates
pub async fn create_guardian_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    receiver: AuthorityId,
    subject: AuthorityId,
    message: Option<String>,
    ttl_ms: Option<u64>,
) -> Result<InvitationInfo, AuraError> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .ok_or_else(|| AuraError::agent("Runtime bridge not available"))?
            .clone()
    };

    runtime
        .create_guardian_invitation(receiver, subject, message, ttl_ms)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to create guardian invitation: {}", e)))
}

/// Create a channel invitation
///
/// **What it does**: Creates an invitation to join a channel
/// **Returns**: InvitationInfo with the created invitation details
/// **Signal pattern**: RuntimeBridge handles state updates
pub async fn create_channel_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    receiver: AuthorityId,
    block_id: String,
    message: Option<String>,
    ttl_ms: Option<u64>,
) -> Result<InvitationInfo, AuraError> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .ok_or_else(|| AuraError::agent("Runtime bridge not available"))?
            .clone()
    };

    runtime
        .create_channel_invitation(receiver, block_id, message, ttl_ms)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to create channel invitation: {}", e)))
}

// ============================================================================
// Invitation Queries via RuntimeBridge
// ============================================================================

/// List pending invitations via RuntimeBridge
///
/// **What it does**: Gets all pending invitations from the RuntimeBridge
/// **Returns**: Vector of InvitationInfo
/// **Signal pattern**: Read-only operation (no emission)
pub async fn list_pending_invitations(
    app_core: &Arc<RwLock<AppCore>>,
) -> Vec<InvitationInfo> {
    let runtime = {
        let core = app_core.read().await;
        match core.runtime() {
            Some(r) => r.clone(),
            None => return Vec::new(),
        }
    };

    runtime.list_pending_invitations().await
}

/// Import and get invitation details from a shareable code
///
/// **What it does**: Parses invitation code and returns the details
/// **Returns**: InvitationInfo with parsed details
/// **Signal pattern**: Read-only until acceptance
pub async fn import_invitation_details(
    app_core: &Arc<RwLock<AppCore>>,
    code: &str,
) -> Result<InvitationInfo, AuraError> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .ok_or_else(|| AuraError::agent("Runtime bridge not available"))?
            .clone()
    };

    runtime
        .import_invitation(code)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to import invitation: {}", e)))
}

// ============================================================================
// Export Operations via RuntimeBridge
// ============================================================================

/// Export an invitation code for sharing
///
/// **What it does**: Generates shareable invitation code
/// **Returns**: Base64-encoded invitation code
/// **Signal pattern**: Read-only operation (no emission)
///
/// This method is implemented via RuntimeBridge.export_invitation().
pub async fn export_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &str,
) -> Result<String, AuraError> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .ok_or_else(|| AuraError::agent("Runtime bridge not available"))?
            .clone()
    };

    runtime
        .export_invitation(invitation_id)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to export invitation: {}", e)))
}

/// Get current invitations state
///
/// **What it does**: Reads invitation state from INVITATIONS_SIGNAL
/// **Returns**: Current invitations (sent and received)
/// **Signal pattern**: Read-only operation (no emission)
pub async fn list_invitations(app_core: &Arc<RwLock<AppCore>>) -> InvitationsState {
    let core = app_core.read().await;

    match core.read(&*INVITATIONS_SIGNAL).await {
        Ok(state) => state,
        Err(_) => InvitationsState::default(),
    }
}

// ============================================================================
// Invitation Operations via RuntimeBridge
// ============================================================================

/// Accept an invitation
///
/// **What it does**: Accepts a received invitation via RuntimeBridge
/// **Returns**: Unit result
/// **Signal pattern**: RuntimeBridge handles signal emission
pub async fn accept_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &str,
) -> Result<(), AuraError> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .ok_or_else(|| AuraError::agent("Runtime bridge not available"))?
            .clone()
    };

    runtime
        .accept_invitation(invitation_id)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to accept invitation: {}", e)))
}

/// Decline an invitation
///
/// **What it does**: Declines a received invitation via RuntimeBridge
/// **Returns**: Unit result
/// **Signal pattern**: RuntimeBridge handles signal emission
pub async fn decline_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &str,
) -> Result<(), AuraError> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .ok_or_else(|| AuraError::agent("Runtime bridge not available"))?
            .clone()
    };

    runtime
        .decline_invitation(invitation_id)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to decline invitation: {}", e)))
}

/// Cancel an invitation
///
/// **What it does**: Cancels a sent invitation via RuntimeBridge
/// **Returns**: Unit result
/// **Signal pattern**: RuntimeBridge handles signal emission
pub async fn cancel_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &str,
) -> Result<(), AuraError> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .ok_or_else(|| AuraError::agent("Runtime bridge not available"))?
            .clone()
    };

    runtime
        .cancel_invitation(invitation_id)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to cancel invitation: {}", e)))
}

/// Import an invitation from a shareable code
///
/// **What it does**: Parses and validates invitation code via RuntimeBridge
/// **Returns**: Unit result
/// **Signal pattern**: RuntimeBridge handles signal emission
///
/// The code parsing and validation is handled by the RuntimeBridge implementation.
pub async fn import_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    code: &str,
) -> Result<(), AuraError> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .ok_or_else(|| AuraError::agent("Runtime bridge not available"))?
            .clone()
    };

    runtime
        .import_invitation(code)
        .await
        .map(|_| ()) // Discard InvitationInfo, just return success
        .map_err(|e| AuraError::agent(format!("Failed to import invitation: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppConfig;

    #[tokio::test]
    async fn test_list_invitations_default() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let invitations = list_invitations(&app_core).await;
        assert!(invitations.sent.is_empty());
        assert!(invitations.received.is_empty());
    }
}
