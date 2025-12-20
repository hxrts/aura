//! # UI Update Channel
//!
//! This module defines the unified update channel for reactive UI updates.
//!
//! ## Architecture
//!
//! All async callbacks send their results through a single `UiUpdate` channel.
//! The IoApp component awaits on this channel and updates `State<T>` values,
//! which automatically trigger re-renders via iocraft's waker mechanism.
//!
//! This replaces the previous polling-based approach with true reactive updates.
//!
//! ## Error Surfacing
//!
//! - **Domain/runtime failures** emit `ERROR_SIGNAL` (via dispatch/operational handlers) and are
//!   surfaced centrally by the app shell as error toasts (or routed into the account setup modal).
//! - **UI-only failures** (e.g., account file I/O during setup) use `UiUpdate::OperationFailed` and
//!   are handled by the same shell processor.
//!
//! ## Usage
//!
//! ```rust,ignore
//! // In callback:
//! let tx = update_tx.clone();
//! tokio::spawn(async move {
//!     let _ = tx.send(UiUpdate::DisplayNameChanged(name));
//! });
//!
//! // In component:
//! hooks.use_future({
//!     async move {
//!         while let Some(update) = rx.recv().await {
//!             match update {
//!                 UiUpdate::DisplayNameChanged(name) => {
//!                     display_name.set(name); // Triggers re-render
//!                 }
//!                 // ...
//!             }
//!         }
//!     }
//! });
//! ```

use crate::tui::components::ToastMessage;
use crate::tui::types::{Device, MfaPolicy};

/// Channel sender type for UI updates
pub type UiUpdateSender = tokio::sync::mpsc::UnboundedSender<UiUpdate>;

/// Channel receiver type for UI updates
pub type UiUpdateReceiver = tokio::sync::mpsc::UnboundedReceiver<UiUpdate>;

/// Create a new UI update channel pair
pub fn ui_update_channel() -> (UiUpdateSender, UiUpdateReceiver) {
    tokio::sync::mpsc::unbounded_channel()
}

/// All UI updates flow through this enum.
///
/// Each variant represents a state change that should trigger a re-render.
/// The component's update processor matches on these and updates the
/// appropriate `State<T>` values.
#[derive(Debug, Clone)]
pub enum UiUpdate {
    // =========================================================================
    // Settings Updates
    // =========================================================================
    /// Display name was successfully updated
    DisplayNameChanged(String),

    /// MFA policy was successfully updated
    MfaPolicyChanged(MfaPolicy),

    /// Recovery threshold was successfully updated
    ThresholdChanged {
        /// Required number of guardians
        k: u8,
        /// Total number of guardians
        n: u8,
    },

    /// A new device was added
    DeviceAdded(Device),

    /// A device was removed
    DeviceRemoved {
        /// The device ID that was removed
        device_id: String,
    },

    // =========================================================================
    // Toast Notifications
    // =========================================================================
    /// A toast notification should be shown
    ToastAdded(ToastMessage),

    /// A specific toast was dismissed
    ToastDismissed {
        /// The toast ID to dismiss
        toast_id: String,
    },

    /// All toasts should be cleared
    ToastsCleared,

    // =========================================================================
    // Chat/Messages
    // =========================================================================
    /// A message was successfully sent
    MessageSent {
        /// The channel the message was sent to
        channel: String,
        /// The message content (for optimistic update)
        content: String,
    },

    /// Message send was retried
    MessageRetried {
        /// The message ID that was retried
        message_id: String,
    },

    /// A channel was selected
    ChannelSelected(String),

    /// A new channel was created
    ChannelCreated(String),

    /// Channel topic was updated
    TopicSet {
        /// The channel name
        channel: String,
        /// The new topic
        topic: String,
    },

    // =========================================================================
    // Invitations
    // =========================================================================
    /// An invitation was accepted
    InvitationAccepted {
        /// The invitation ID
        invitation_id: String,
    },

    /// An invitation was declined
    InvitationDeclined {
        /// The invitation ID
        invitation_id: String,
    },

    /// A new invitation was created
    InvitationCreated {
        /// The invitation code or ID
        invitation_code: String,
    },

    /// An invitation code was exported (retrieved for sharing)
    InvitationExported {
        /// The exported invitation code
        code: String,
    },

    /// An invitation was imported from a file
    InvitationImported {
        /// The invitation code that was imported
        invitation_code: String,
    },

    // =========================================================================
    // Navigation
    // =========================================================================
    /// Entered a block
    BlockEntered {
        /// The block ID
        block_id: String,
    },

    /// Navigated to home/default view
    NavigatedHome,

    /// Navigated back to street level
    NavigatedToStreet,

    /// Navigated to neighborhood view
    NavigatedToNeighborhood,

    // =========================================================================
    // Recovery
    // =========================================================================
    /// Recovery process was started
    RecoveryStarted,

    /// A guardian was added to the recovery set
    GuardianAdded {
        /// The contact ID of the guardian
        contact_id: String,
    },

    /// A guardian was selected for ceremony
    GuardianSelected {
        /// The contact ID of the guardian
        contact_id: String,
    },

    /// An approval was submitted for a recovery request
    ApprovalSubmitted {
        /// The request ID
        request_id: String,
    },

    /// Guardian ceremony progressed to a new step
    GuardianCeremonyProgress {
        /// Description of the current step
        step: String,
    },

    // =========================================================================
    // Contacts
    // =========================================================================
    /// A contact's nickname was updated
    NicknameUpdated {
        /// The contact ID
        contact_id: String,
        /// The new nickname
        nickname: String,
    },

    /// A direct chat was started with a contact
    ChatStarted {
        /// The contact ID
        contact_id: String,
    },

    /// A LAN peer was invited
    LanPeerInvited {
        /// The peer ID or address
        peer_id: String,
    },

    // =========================================================================
    // Block Operations
    // =========================================================================
    /// A message was sent in a block
    BlockMessageSent {
        /// The block ID
        block_id: String,
        /// The message content
        content: String,
    },

    /// An invite was sent from a block
    BlockInviteSent {
        /// The contact ID that was invited
        contact_id: String,
    },

    /// Steward role was granted
    StewardGranted {
        /// The contact ID
        contact_id: String,
    },

    /// Steward role was revoked
    StewardRevoked {
        /// The contact ID
        contact_id: String,
    },

    // =========================================================================
    // Account
    // =========================================================================
    /// Account was successfully created
    AccountCreated,

    // =========================================================================
    // Sync Status
    // =========================================================================
    /// Sync operation started
    SyncStarted,

    /// Sync operation completed successfully
    SyncCompleted,

    /// Sync operation failed
    SyncFailed {
        /// Error message
        error: String,
    },

    // =========================================================================
    // Error Handling
    // =========================================================================
    /// An async operation failed
    ///
    /// This is used for operations that don't have a specific success variant
    /// or when we want to show an error toast.
    OperationFailed {
        /// Name of the operation that failed
        operation: String,
        /// Error message
        error: String,
    },
}

impl UiUpdate {
    /// Create an operation failed update with toast
    pub fn operation_failed(operation: impl Into<String>, error: impl Into<String>) -> Self {
        Self::OperationFailed {
            operation: operation.into(),
            error: error.into(),
        }
    }

    /// Check if this update represents an error
    pub fn is_error(&self) -> bool {
        matches!(self, Self::OperationFailed { .. } | Self::SyncFailed { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ui_update_channel() {
        let (tx, mut rx) = ui_update_channel();

        tx.send(UiUpdate::DisplayNameChanged("Alice".to_string()))
            .unwrap();
        tx.send(UiUpdate::MfaPolicyChanged(MfaPolicy::AlwaysRequired))
            .unwrap();

        let update1 = rx.try_recv().unwrap();
        assert!(matches!(update1, UiUpdate::DisplayNameChanged(name) if name == "Alice"));

        let update2 = rx.try_recv().unwrap();
        assert!(matches!(
            update2,
            UiUpdate::MfaPolicyChanged(MfaPolicy::AlwaysRequired)
        ));
    }

    #[test]
    fn test_operation_failed() {
        let update = UiUpdate::operation_failed("UpdateNickname", "Network error");
        assert!(update.is_error());

        match update {
            UiUpdate::OperationFailed { operation, error } => {
                assert_eq!(operation, "UpdateNickname");
                assert_eq!(error, "Network error");
            }
            _ => panic!("Expected OperationFailed"),
        }
    }
}
