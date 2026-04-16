//! # UI Update Channel
//!
//! This module defines the unified update channel for reactive UI updates.
//!
//! ## Architecture
//!
//! Async callback results send their UI state changes through `UiUpdate`.
//! The IoApp component awaits on this channel and updates `State<T>` values,
//! which automatically trigger re-renders via iocraft's waker mechanism.
//!
//! Typed harness semantic commands use a separate command channel so their
//! application/ack path stays independent from the broader async UI update stream.
//!
//! This replaces the previous polling-based approach with true reactive updates.
//!
//! ## Error Surfacing
//!
//! - **Domain/runtime failures** emit `ERROR_SIGNAL` (via dispatch/operational handlers) and are
//!   surfaced centrally by the app shell as error toasts (or routed into the account setup modal).
//! - **UI-only failures** (e.g., account file I/O during setup) use typed
//!   `UiUpdate::OperationFailed` payloads and are handled by the same shell processor.
//!
//! ## Usage
//!
//! ```rust,ignore
//! // In a callback owned by an existing task supervisor:
//! let tx = update_tx.clone();
//! task_owner.spawn(async move {
//!     let _ = tx.try_send(UiUpdate::NicknameSuggestionChanged(name));
//! });
//!
//! // In component:
//! hooks.use_future({
//!     async move {
//!         while let Some(update) = rx.recv().await {
//!             match update {
//!                 UiUpdate::NicknameSuggestionChanged(name) => {
//!                     nickname_suggestion.set(name); // Triggers re-render
//!                 }
//!                 // ...
//!             }
//!         }
//!     }
//! });
//! ```

use crate::error::TerminalError;
use crate::tui::components::ToastMessage;
use crate::tui::tasks::UiTaskOwner;
use crate::tui::types::{AuthorityInfo, Device, MfaPolicy};
pub use aura_app::frontend_primitives::FrontendUiOperation as UiOperation;
use aura_app::ui::contract::HarnessUiCommand;
use aura_app::ui_contract::{
    ChannelBindingWitness, OperationId, OperationInstanceId, ProjectionRevision, RuntimeEventKind,
    RuntimeFact, SemanticOperationStatus,
};
use aura_core::types::Epoch;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use tokio::sync::Notify;

/// Channel sender type for UI updates
pub type UiUpdateSender = tokio::sync::mpsc::Sender<UiUpdate>;

/// Channel receiver type for UI updates
pub type UiUpdateReceiver = tokio::sync::mpsc::Receiver<UiUpdate>;

/// Channel sender type for typed harness UI commands.
pub type HarnessCommandSender = tokio::sync::mpsc::Sender<HarnessCommandSubmission>;

/// Channel receiver type for typed harness UI commands.
pub type HarnessCommandReceiver = tokio::sync::mpsc::Receiver<HarnessCommandSubmission>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiUpdatePublication {
    OrderedRequired,
    RequiredUnordered,
    LossyObserved,
}

pub async fn publish_ui_update(
    tx: &UiUpdateSender,
    update: UiUpdate,
    publication: UiUpdatePublication,
) -> bool {
    match publication {
        UiUpdatePublication::OrderedRequired | UiUpdatePublication::RequiredUnordered => {
            if tx.send(update).await.is_err() {
                tracing::debug!("UI update channel closed during shutdown");
                return false;
            }
            true
        }
        UiUpdatePublication::LossyObserved => tx.try_send(update).is_ok(),
    }
}

pub fn spawn_ui_update(
    tasks: &Arc<UiTaskOwner>,
    tx: &UiUpdateSender,
    update: UiUpdate,
    publication: UiUpdatePublication,
) {
    let tx = tx.clone();
    match publication {
        UiUpdatePublication::LossyObserved => {
            let _ = tx.try_send(update);
        }
        UiUpdatePublication::OrderedRequired | UiUpdatePublication::RequiredUnordered => {
            tasks.spawn(async move {
                let _ = publish_ui_update(&tx, update, publication).await;
            });
        }
    }
}

pub struct OrderedUiUpdateGate {
    next_ticket: AtomicU64,
    serving_ticket: AtomicU64,
    notify: Notify,
}

impl OrderedUiUpdateGate {
    #[must_use]
    pub fn new() -> Self {
        Self {
            next_ticket: AtomicU64::new(0),
            serving_ticket: AtomicU64::new(0),
            notify: Notify::new(),
        }
    }

    fn issue_ticket(&self) -> u64 {
        self.next_ticket.fetch_add(1, Ordering::AcqRel)
    }

    async fn wait_turn(&self, ticket: u64) {
        loop {
            if self.serving_ticket.load(Ordering::Acquire) == ticket {
                return;
            }
            let notified = self.notify.notified();
            if self.serving_ticket.load(Ordering::Acquire) == ticket {
                return;
            }
            notified.await;
        }
    }

    fn finish_turn(&self) {
        self.serving_ticket.fetch_add(1, Ordering::AcqRel);
        self.notify.notify_waiters();
    }
}

impl Default for OrderedUiUpdateGate {
    fn default() -> Self {
        Self::new()
    }
}

pub fn spawn_ordered_ui_updates(
    tasks: &Arc<UiTaskOwner>,
    tx: &UiUpdateSender,
    ordered_gate: &Arc<OrderedUiUpdateGate>,
    updates: Vec<UiUpdate>,
) {
    if updates.is_empty() {
        return;
    }

    let tx = tx.clone();
    let ordered_gate = Arc::clone(ordered_gate);
    let ticket = ordered_gate.issue_ticket();
    tasks.spawn(async move {
        ordered_gate.wait_turn(ticket).await;
        for update in updates {
            let _ = publish_ui_update(&tx, update, UiUpdatePublication::OrderedRequired).await;
        }
        ordered_gate.finish_turn();
    });
}

/// Send a UI update, trying non-blocking first and falling back to async.
/// Returns `true` if delivered, `false` if the channel is closed.
pub async fn send_ui_update_required(tx: &UiUpdateSender, update: UiUpdate) -> bool {
    publish_ui_update(tx, update, UiUpdatePublication::RequiredUnordered).await
}

fn required_ui_update_tasks() -> &'static UiTaskOwner {
    static REQUIRED_UI_UPDATE_TASKS: OnceLock<UiTaskOwner> = OnceLock::new();
    REQUIRED_UI_UPDATE_TASKS.get_or_init(UiTaskOwner::new)
}

/// Send a required UI update from a synchronous callback context.
pub fn send_ui_update_required_blocking(tx: &UiUpdateSender, update: UiUpdate) -> bool {
    match tx.try_send(update) {
        Ok(()) => true,
        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => false,
        Err(tokio::sync::mpsc::error::TrySendError::Full(update)) => {
            let tx = tx.clone();
            required_ui_update_tasks().spawn(async move {
                let _ =
                    publish_ui_update(&tx, update, UiUpdatePublication::RequiredUnordered).await;
            });
            true
        }
    }
}

/// Send a UI update without blocking. Returns `true` if sent.
pub fn send_ui_update_lossy(tx: &UiUpdateSender, update: UiUpdate) -> bool {
    tx.try_send(update).is_ok()
}

/// Create a new UI update channel pair
#[must_use]
pub fn ui_update_channel() -> (UiUpdateSender, UiUpdateReceiver) {
    tokio::sync::mpsc::channel(1024)
}

/// Create a new harness command channel pair.
#[must_use]
pub fn harness_command_channel() -> (HarnessCommandSender, HarnessCommandReceiver) {
    tokio::sync::mpsc::channel(128)
}

#[derive(Clone, Debug)]
pub struct HarnessCommandSubmission {
    pub submission_id: String,
    pub command: HarnessUiCommand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiOperationFailure {
    pub operation: UiOperation,
    pub error: TerminalError,
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
    /// Nickname suggestion was successfully updated
    NicknameSuggestionChanged(String),

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

    /// Authorities list and current selection were updated from authoritative settings state.
    AuthoritiesUpdated {
        authorities: Vec<AuthorityInfo>,
        current_index: usize,
    },

    /// Pending startup runtime bootstrap finished converging.
    RuntimeBootstrapFinalized,

    /// A locally observed runtime fact should be persisted into TUI state.
    RuntimeFactObserved(RuntimeFact),

    /// Device enrollment ("add device") ceremony started.
    DeviceEnrollmentStarted {
        ceremony_id: String,
        nickname_suggestion: String,
        enrollment_code: String,
        pending_epoch: Epoch,
        device_id: String,
    },

    /// Generic key-rotation ceremony status update (device enrollment, guardian rotation, etc.).
    KeyRotationCeremonyStatus {
        ceremony_id: String,
        kind: aura_app::ui::types::CeremonyKind,
        accepted_count: u16,
        total_count: u16,
        threshold: u16,
        is_complete: bool,
        has_failed: bool,
        accepted_participants: Vec<aura_core::threshold::ParticipantIdentity>,
        error_message: Option<String>,
        pending_epoch: Option<Epoch>,
        agreement_mode: aura_core::threshold::AgreementMode,
        reversion_risk: bool,
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

    /// A channel was selected via an authoritative workflow/signal witness.
    ChannelSelected(ChannelBindingWitness),

    /// A new channel was created
    ChannelCreated {
        /// Exact instance id for the local terminal operation that produced this creation.
        operation_instance_id: Option<OperationInstanceId>,
        /// Canonical channel identity returned by the workflow/runtime.
        channel_id: String,
        /// Authoritative context identity, if the workflow resolved one.
        context_id: Option<String>,
        /// Display name used for user feedback.
        name: String,
    },

    /// Chat state changed (channel/message counts + selection)
    ChatStateUpdated {
        /// Total number of channels
        channel_count: usize,
        /// Total number of messages in selected channel
        message_count: usize,
        /// Selected channel index (if known)
        selected_index: Option<usize>,
    },

    /// Channel topic was updated
    TopicSet {
        /// The channel name
        channel: String,
        /// The new topic
        topic: String,
    },

    /// Neighborhood state changed (message count for current home/channel)
    NeighborhoodStateUpdated {
        /// Number of messages in the current home's selected channel
        message_count: usize,
    },

    /// Channel info participants were updated
    ChannelInfoParticipants {
        /// Channel ID for the modal
        channel_id: String,
        /// Participants to display
        participants: Vec<String>,
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
        /// The invite code or ID
        invitation_code: String,
    },

    /// An invite code was exported (retrieved for sharing)
    InvitationExported {
        /// The exported invite code
        code: String,
        /// Operation that exported the code, when the export belongs to an exact semantic flow
        operation_id: Option<OperationId>,
        /// Exact instance that produced the code, when the export belongs to an exact semantic flow
        instance_id: Option<OperationInstanceId>,
    },

    /// An invitation was imported from a file
    InvitationImported {
        /// The invite code that was imported
        invitation_code: String,
    },

    // =========================================================================
    // Navigation
    // =========================================================================
    /// Entered a home
    HomeEntered {
        /// The home ID
        home_id: String,
    },

    /// Navigated to home/default view
    NavigatedHome,

    /// Navigated back to limited level
    NavigatedToLimited,

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

    /// Guardian ceremony status update (for in-progress ceremony UI)
    GuardianCeremonyStatus {
        /// Ceremony identifier
        ceremony_id: String,
        /// List of guardian IDs who have accepted
        accepted_guardians: Vec<String>,
        /// Total number of guardians
        total_count: u16,
        /// Threshold required for completion
        threshold: u16,
        /// Whether the ceremony is complete
        is_complete: bool,
        /// Whether the ceremony has failed
        has_failed: bool,
        /// Optional error message if failed
        error_message: Option<String>,
        /// Pending epoch for key rotation (if created)
        pending_epoch: Option<Epoch>,
        /// Agreement mode (A1/A2/A3)
        agreement_mode: aura_core::threshold::AgreementMode,
        /// Whether reversion is still possible
        reversion_risk: bool,
    },

    // =========================================================================
    // Contacts
    // =========================================================================
    /// Contact list count changed (for keyboard navigation)
    ContactCountChanged(usize),

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
    /// LAN peer count changed (for keyboard navigation)
    LanPeersCountChanged(usize),

    // =========================================================================
    // Notifications
    // =========================================================================
    /// Notifications count changed (for keyboard navigation)
    NotificationsCountChanged(usize),

    /// A long-lived subscription exhausted its retry budget and degraded permanently.
    SubscriptionDegraded { signal_id: String, reason: String },

    /// Replace the authoritative runtime facts for specific fact kinds.
    RuntimeFactsUpdated {
        revision: ProjectionRevision,
        replace_kinds: Vec<RuntimeEventKind>,
        facts: Vec<RuntimeFact>,
    },

    /// Apply an authoritative semantic operation status emitted by `aura-app`.
    AuthoritativeOperationStatus {
        operation_id: OperationId,
        instance_id: Option<OperationInstanceId>,
        causality: Option<aura_app::ui_contract::SemanticOperationCausality>,
        status: SemanticOperationStatus,
    },

    // =========================================================================
    // Home Operations
    // =========================================================================
    /// An invite was sent from a home
    HomeInviteSent {
        /// The contact ID that was invited
        contact_id: String,
    },

    /// Moderator role was granted
    ModeratorGranted {
        /// The contact ID
        contact_id: String,
    },

    /// Moderator role was revoked
    ModeratorRevoked {
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
    OperationFailed { failure: UiOperationFailure },
}

impl UiUpdate {
    /// Create an operation failed update with toast
    pub fn operation_failed(operation: UiOperation, error: impl Into<TerminalError>) -> Self {
        Self::OperationFailed {
            failure: UiOperationFailure {
                operation,
                error: error.into(),
            },
        }
    }

    /// Check if this update represents an error
    #[must_use]
    pub fn is_error(&self) -> bool {
        matches!(self, Self::OperationFailed { .. } | Self::SyncFailed { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{pin_mut, poll};
    use std::sync::Arc;

    #[test]
    fn test_ui_update_channel() {
        let (tx, mut rx) = ui_update_channel();

        tx.try_send(UiUpdate::NicknameSuggestionChanged("Alice".to_string()))
            .unwrap();
        tx.try_send(UiUpdate::MfaPolicyChanged(MfaPolicy::AlwaysRequired))
            .unwrap();

        let update1 = rx.try_recv().unwrap();
        assert!(matches!(update1, UiUpdate::NicknameSuggestionChanged(name) if name == "Alice"));

        let update2 = rx.try_recv().unwrap();
        assert!(matches!(
            update2,
            UiUpdate::MfaPolicyChanged(MfaPolicy::AlwaysRequired)
        ));
    }

    #[test]
    fn test_harness_command_channel() {
        let (tx, mut rx) = harness_command_channel();

        tx.try_send(HarnessCommandSubmission {
            submission_id: "submission-1".to_string(),
            command: HarnessUiCommand::NavigateScreen {
                screen: aura_app::ui::contract::ScreenId::Settings,
            },
        })
        .unwrap();

        let submission = rx.try_recv().unwrap();
        assert_eq!(submission.submission_id, "submission-1");
        assert!(matches!(
            submission.command,
            HarnessUiCommand::NavigateScreen {
                screen: aura_app::ui::contract::ScreenId::Settings,
            }
        ));
    }

    #[test]
    fn test_operation_failed() {
        let update = UiUpdate::operation_failed(
            UiOperation::CreateAccount,
            TerminalError::Network("Network error".to_string()),
        );
        assert!(update.is_error());

        match update {
            UiUpdate::OperationFailed { failure } => {
                assert_eq!(failure.operation, UiOperation::CreateAccount);
                assert_eq!(
                    failure.error,
                    TerminalError::Network("Network error".to_string())
                );
            }
            _ => panic!("Expected OperationFailed"),
        }
    }

    #[tokio::test]
    async fn required_publication_waits_for_backpressure() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        tx.send(UiUpdate::SyncStarted).await.unwrap();

        let publish = publish_ui_update(
            &tx,
            UiUpdate::SyncCompleted,
            UiUpdatePublication::RequiredUnordered,
        );
        pin_mut!(publish);
        assert!(poll!(publish.as_mut()).is_pending());

        assert!(matches!(rx.recv().await, Some(UiUpdate::SyncStarted)));
        assert!(publish.await);
        assert!(matches!(rx.recv().await, Some(UiUpdate::SyncCompleted)));
    }

    #[tokio::test]
    async fn ordered_publication_preserves_sequence_under_backpressure() {
        let tasks = Arc::new(UiTaskOwner::new());
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let ordered_gate = Arc::new(OrderedUiUpdateGate::new());

        spawn_ordered_ui_updates(
            &tasks,
            &tx,
            &ordered_gate,
            vec![UiUpdate::SyncStarted, UiUpdate::SyncCompleted],
        );

        assert!(matches!(rx.recv().await, Some(UiUpdate::SyncStarted)));
        assert!(matches!(rx.recv().await, Some(UiUpdate::SyncCompleted)));
        tasks.shutdown();
    }

    #[tokio::test]
    async fn ordered_publication_preserves_sequence_across_batches() {
        let tasks = Arc::new(UiTaskOwner::new());
        let (tx, mut rx) = tokio::sync::mpsc::channel(4);
        let ordered_gate = Arc::new(OrderedUiUpdateGate::new());

        spawn_ordered_ui_updates(&tasks, &tx, &ordered_gate, vec![UiUpdate::SyncStarted]);
        spawn_ordered_ui_updates(&tasks, &tx, &ordered_gate, vec![UiUpdate::SyncCompleted]);

        assert!(matches!(rx.recv().await, Some(UiUpdate::SyncStarted)));
        assert!(matches!(rx.recv().await, Some(UiUpdate::SyncCompleted)));
        tasks.shutdown();
    }

    #[tokio::test]
    async fn lossy_observed_publication_drops_on_backpressure() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        tx.send(UiUpdate::SyncStarted).await.unwrap();

        assert!(
            !publish_ui_update(
                &tx,
                UiUpdate::SyncCompleted,
                UiUpdatePublication::LossyObserved,
            )
            .await
        );

        assert!(matches!(rx.recv().await, Some(UiUpdate::SyncStarted)));
        assert!(rx.try_recv().is_err());
    }
}
