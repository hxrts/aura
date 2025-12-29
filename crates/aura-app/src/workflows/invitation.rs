//! Invitation Workflow - Portable Business Logic
//!
//! This module contains invitation operations that are portable across
//! all frontends via the RuntimeBridge abstraction.

use crate::runtime_bridge::{InvitationBridgeType, InvitationInfo};
use crate::{views::invitations::InvitationsState, AppCore, INVITATIONS_SIGNAL};
use async_lock::RwLock;
use aura_core::identifiers::AuthorityId;
use aura_core::{effects::reactive::ReactiveEffects, AuraError};
use std::sync::Arc;

#[cfg(feature = "signals")]
async fn yield_once() {
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    struct YieldOnce(bool);

    impl Future for YieldOnce {
        type Output = ();

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            if self.0 {
                Poll::Ready(())
            } else {
                self.0 = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }

    YieldOnce(false).await
}

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
    nickname: Option<String>,
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
        .create_contact_invitation(receiver, nickname, message, ttl_ms)
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
    home_id: String,
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
        .create_channel_invitation(receiver, home_id, message, ttl_ms)
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
pub async fn list_pending_invitations(app_core: &Arc<RwLock<AppCore>>) -> Vec<InvitationInfo> {
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

    core.read(&*INVITATIONS_SIGNAL).await.unwrap_or_default()
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

    #[cfg(feature = "signals")]
    let initial_contact_count = {
        let core = app_core.read().await;
        core.read(&*crate::signal_defs::CONTACTS_SIGNAL)
            .await
            .unwrap_or_default()
            .contacts
            .len()
    };

    #[cfg(feature = "signals")]
    let mut contacts_stream = {
        let core = app_core.read().await;
        core.subscribe(&*crate::signal_defs::CONTACTS_SIGNAL)
    };

    runtime
        .accept_invitation(invitation_id)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to accept invitation: {}", e)))?;

    // Give the runtime fact pipeline a bounded chance to publish CONTACTS_SIGNAL before we refresh
    // derived UI signals like CONNECTION_STATUS_SIGNAL.
    #[cfg(feature = "signals")]
    {
        for _ in 0..4096 {
            // Prefer consuming emissions if available (fast path).
            if let Some(state) = contacts_stream.try_recv() {
                if state.contacts.len() > initial_contact_count {
                    break;
                }
            } else {
                // Fallback: check current state (covers missed emissions).
                let contacts_len = {
                    let core = app_core.read().await;
                    core.read(&*crate::signal_defs::CONTACTS_SIGNAL)
                        .await
                        .unwrap_or_default()
                        .contacts
                        .len()
                };

                if contacts_len > initial_contact_count {
                    break;
                }
            }

            yield_once().await;
        }
    }

    // Best-effort: refresh signals so UI status (e.g. online contact count) updates immediately.
    let _ = super::system::refresh_account(app_core).await;

    Ok(())
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

/// Accept the first pending home/channel invitation
///
/// **What it does**: Finds and accepts the first pending channel invitation
/// **Returns**: Invitation ID that was accepted
/// **Signal pattern**: RuntimeBridge handles signal emission
///
/// This is used by UI to quickly accept a pending home invitation without
/// requiring the user to select a specific invitation ID.
pub async fn accept_pending_home_invitation(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<String, AuraError> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .ok_or_else(|| AuraError::agent("Runtime bridge not available"))?
            .clone()
    };

    // Get pending invitations
    let pending = runtime.list_pending_invitations().await;

    // Find a channel invitation that we received (sender is not us)
    let our_authority = runtime.authority_id();
    let home_invitation = pending.iter().find(|inv| {
        matches!(inv.invitation_type, InvitationBridgeType::Channel { .. })
            && inv.sender_id != our_authority
    });

    match home_invitation {
        Some(inv) => {
            runtime
                .accept_invitation(&inv.invitation_id)
                .await
                .map_err(|e| AuraError::agent(format!("Failed to accept invitation: {}", e)))?;
            Ok(inv.invitation_id.clone())
        }
        None => Err(AuraError::agent("No pending home invitation found")),
    }
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
        assert!(invitations.pending.is_empty());
    }
}
