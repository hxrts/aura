//! Invitation Workflow - Portable Business Logic
//!
//! This module contains invitation operations that should be portable
//! across all frontends. Currently partially implemented due to
//! RuntimeBridge limitations.
//!
//! ## TODO: RuntimeBridge Extension Needed
//!
//! To fully implement invitation workflows, RuntimeBridge needs these methods:
//! - `create_invitation(receiver, role, ttl) -> Invitation`
//! - `accept_invitation(invitation_id) -> Result<()>`
//! - `decline_invitation(invitation_id) -> Result<()>`
//! - `cancel_invitation(invitation_id) -> Result<()>`
//! - `list_invitations() -> Vec<Invitation>`
//! - `import_invitation(code) -> Invitation`
//!
//! Currently these operations are accessed via `agent.invitations()` in
//! handlers, which breaks the aura-app portability abstraction.

use crate::{
    views::invitations::{Invitation, InvitationsState},
    AppCore, INVITATIONS_SIGNAL,
};
use aura_core::{effects::reactive::ReactiveEffects, identifiers::AuthorityId, AuraError};
use std::sync::Arc;
use tokio::sync::RwLock;

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
// TODO: The following functions need RuntimeBridge extension
// ============================================================================
//
// These operations currently require direct access to InvitationService
// via the agent, which breaks the aura-app portability abstraction.
//
// To make them portable, we need to add corresponding methods to RuntimeBridge
// and implement them in aura-agent.

/// Create an invitation (TODO: Needs RuntimeBridge extension)
///
/// **What it does**: Creates a new invitation for another authority
/// **Returns**: Invitation information
/// **Signal pattern**: Emits INVITATIONS_SIGNAL after creation
///
/// **TODO**: Add `create_invitation` to RuntimeBridge trait.
pub async fn create_invitation(
    _app_core: &Arc<RwLock<AppCore>>,
    _receiver: AuthorityId,
    _role: String,
    _ttl_secs: Option<u64>,
) -> Result<Invitation, AuraError> {
    // TODO: Implement via RuntimeBridge once extended
    Err(AuraError::agent(
        "create_invitation not yet implemented - needs RuntimeBridge extension",
    ))
}

/// Accept an invitation (TODO: Needs RuntimeBridge extension)
///
/// **What it does**: Accepts a received invitation
/// **Returns**: Unit result
/// **Signal pattern**: Emits INVITATIONS_SIGNAL after acceptance
///
/// **TODO**: Add `accept_invitation` to RuntimeBridge trait.
pub async fn accept_invitation(
    _app_core: &Arc<RwLock<AppCore>>,
    _invitation_id: &str,
) -> Result<(), AuraError> {
    // TODO: Implement via RuntimeBridge once extended
    Err(AuraError::agent(
        "accept_invitation not yet implemented - needs RuntimeBridge extension",
    ))
}

/// Decline an invitation (TODO: Needs RuntimeBridge extension)
///
/// **What it does**: Declines a received invitation
/// **Returns**: Unit result
/// **Signal pattern**: Emits INVITATIONS_SIGNAL after declining
///
/// **TODO**: Add `decline_invitation` to RuntimeBridge trait.
pub async fn decline_invitation(
    _app_core: &Arc<RwLock<AppCore>>,
    _invitation_id: &str,
) -> Result<(), AuraError> {
    // TODO: Implement via RuntimeBridge once extended
    Err(AuraError::agent(
        "decline_invitation not yet implemented - needs RuntimeBridge extension",
    ))
}

/// Cancel an invitation (TODO: Needs RuntimeBridge extension)
///
/// **What it does**: Cancels a sent invitation
/// **Returns**: Unit result
/// **Signal pattern**: Emits INVITATIONS_SIGNAL after cancellation
///
/// **TODO**: Add `cancel_invitation` to RuntimeBridge trait.
pub async fn cancel_invitation(
    _app_core: &Arc<RwLock<AppCore>>,
    _invitation_id: &str,
) -> Result<(), AuraError> {
    // TODO: Implement via RuntimeBridge once extended
    Err(AuraError::agent(
        "cancel_invitation not yet implemented - needs RuntimeBridge extension",
    ))
}

/// Import an invitation from a shareable code (TODO: Needs RuntimeBridge extension)
///
/// **What it does**: Validates and imports invitation code into state
/// **Returns**: Unit result
/// **Signal pattern**: Emits INVITATIONS_SIGNAL after import
///
/// **TODO**: Add `import_invitation` to RuntimeBridge trait.
///
/// **Note**: For now, invitation parsing is handled in the terminal layer
/// where aura-agent dependencies are available. This function is a placeholder
/// for the future RuntimeBridge implementation.
pub async fn import_invitation(
    _app_core: &Arc<RwLock<AppCore>>,
    _code: &str,
) -> Result<(), AuraError> {
    // TODO: Implement via RuntimeBridge once extended
    // The parsing logic currently lives in the terminal handler
    // where ShareableInvitation from aura-agent is available
    Err(AuraError::agent(
        "import_invitation not yet implemented - needs RuntimeBridge extension",
    ))
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
