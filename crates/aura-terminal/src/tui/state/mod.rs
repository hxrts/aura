// Allow patterns that are clearer than their clippy-suggested alternatives
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::field_reassign_with_default)]

//! # TUI State Machine
//!
//! Pure state machine model for the TUI, enabling deterministic testing.
//!
//! The TUI is modeled as:
//! ```text
//! TuiState × TerminalEvent → (TuiState, Vec<TuiCommand>)
//! ```
//!
//! This module provides:
//! - `TuiState`: Complete UI state
//! - `TuiCommand`: Side effects to be executed
//! - `transition()`: Pure state transition function
//!
//! ## Modal and Toast Queue System
//!
//! All modals and toasts go through type-enforced queues:
//! - `ModalQueue`: Ensures exactly 0 or 1 modal is visible at a time (FIFO)
//! - `ToastQueue`: Ensures exactly 0 or 1 toast is visible at a time (FIFO)
//!
//! **IMPORTANT**: Do NOT use `visible: bool` fields on modal structs.
//! All modals MUST be shown via `modal_queue.enqueue()`.

pub mod commands;
pub mod form;
mod handlers;
pub mod ids;
pub mod modal_queue;
mod operations;
pub mod toast;
mod transition;
pub mod views;

// Re-export the sanctioned public TUI state surface.
pub use commands::{
    DispatchCommand, HomeCapabilityConfig, HomeTarget, InvitationKind, ThresholdK, TuiCommand,
};
pub use form::{FormDraft, FormPhase, Validatable, ValidationError};
pub use ids::{AuthorityRef, CeremonyId, ChannelId, ContactId, DeviceId, HomeId, InvitationId};
pub use modal_queue::{
    ChatMemberSelectModalState, ConfirmAction, ContactSelectModalState, ModalQueue, QueuedModal,
};
pub use toast::{QueuedToast, ToastLevel, ToastQueue};
pub use transition::transition;
pub use views::*;
pub use views::{ChatMemberCandidate, CreateChannelModalState, CreateChannelStep};

use crate::tui::screens::{Router, Screen};
use crate::tui::types::AuthorityInfo;
use aura_app::ui::contract::{
    OperationId, OperationInstanceId, OperationSnapshot, OperationState, RuntimeEventId,
    RuntimeEventKind, RuntimeEventSnapshot,
};
use aura_app::ui_contract::{ProjectionRevision, RuntimeFact, SemanticOperationCausality};
use operations::OperationTracker;
use std::collections::HashMap;

/// Complete TUI state
///
/// This struct captures all state needed to render the TUI and process events.
/// It is designed to be:
/// - Clone: Can be copied for state comparison
/// - Debug: Can be inspected for debugging
/// - Serializable: Can be saved for trace replay (via serde)
#[derive(Clone, Debug, Default)]
pub struct TuiState {
    /// Screen navigation state
    pub router: Router,

    // ========================================================================
    // NEW: Queue-Based Modal/Toast System
    // ========================================================================
    /// Modal queue - type-enforced single modal at a time (FIFO)
    /// **USE THIS** instead of scattered `visible: bool` fields
    pub modal_queue: ModalQueue,

    /// Toast queue - type-enforced single toast at a time (FIFO)
    /// **USE THIS** instead of `Vec<Toast>`
    pub toast_queue: ToastQueue,

    /// Counter for generating unique toast IDs
    pub next_toast_id: u64,

    // ========================================================================
    // Screen-Specific State
    // ========================================================================
    /// Chat screen state
    pub chat: ChatViewState,

    /// Contacts screen state
    pub contacts: ContactsViewState,

    /// Notifications screen state
    pub notifications: NotificationsViewState,

    /// Settings screen state
    pub settings: SettingsViewState,

    /// Neighborhood screen state
    pub neighborhood: NeighborhoodViewState,

    /// Help screen state
    pub help: HelpViewState,

    // ========================================================================
    // Global State
    // ========================================================================
    /// Terminal size
    pub terminal_size: (u16, u16),

    /// Whether the TUI should exit
    pub should_exit: bool,

    /// Whether the terminal window has focus
    /// Used to pause animations and show visual indicator when unfocused
    pub window_focused: bool,

    // ========================================================================
    // Authority Context (app-global, affects all screens)
    // ========================================================================
    /// Available authorities for this device (populated by signal)
    pub authorities: Vec<AuthorityInfo>,

    /// Index of the currently active authority in the authorities list
    /// This is app-global context, not screen-specific state.
    pub current_authority_index: usize,

    /// Semantic operation states exported to the harness snapshot.
    operation_states: OperationTracker,

    /// Last exported invite code for harness-visible semantic readiness.
    pub last_exported_invitation_code: Option<String>,

    /// True while the shell is waiting for a startup runtime bootstrap to converge.
    pub pending_runtime_bootstrap: bool,

    /// Long-lived subscriptions that have permanently degraded.
    pub degraded_subscriptions: HashMap<String, String>,

    /// Runtime facts exported from owned TUI transitions.
    pub runtime_facts: Vec<RuntimeFact>,

    /// Latest authoritative runtime-fact revision applied to state.
    pub last_authoritative_runtime_facts_revision: ProjectionRevision,
}

impl TuiState {
    /// Create a new TUI state with default values
    #[must_use]
    pub fn new() -> Self {
        Self {
            terminal_size: (80, 24),
            ..Default::default()
        }
    }

    /// Create a TUI state with specific terminal size
    #[must_use]
    pub fn with_size(width: u16, height: u16) -> Self {
        Self {
            terminal_size: (width, height),
            ..Default::default()
        }
    }

    /// Create a TUI state with the account setup modal visible (via queue)
    #[must_use]
    pub fn with_account_setup() -> Self {
        let mut state = Self::new();
        state.show_account_setup_queued();
        state
    }

    /// Get the current screen
    #[must_use]
    pub fn screen(&self) -> Screen {
        self.router.current()
    }

    // ========================================================================
    // NEW: Queue-Based Modal/Toast Methods
    // ========================================================================

    /// Show a modal via the queue (type-enforced)
    pub fn show_modal(&mut self, modal: QueuedModal) {
        self.modal_queue.enqueue(modal);
    }

    /// Dismiss the current modal (shows next in queue if any)
    pub fn dismiss_modal(&mut self) -> Option<QueuedModal> {
        self.modal_queue.dismiss()
    }

    /// Check if any modal is active (queue-based)
    #[must_use]
    pub fn has_queued_modal(&self) -> bool {
        self.modal_queue.is_active()
    }

    /// Show a toast via the queue (type-enforced)
    pub fn show_toast_queued(&mut self, message: impl Into<String>, level: ToastLevel) {
        let id = self.next_toast_id;
        self.next_toast_id += 1;
        self.toast_queue
            .enqueue(QueuedToast::new(id, message, level));
    }

    /// Show a success toast
    pub fn toast_success(&mut self, message: impl Into<String>) {
        self.show_toast_queued(message, ToastLevel::Success);
    }

    /// Show an error toast
    pub fn toast_error(&mut self, message: impl Into<String>) {
        self.show_toast_queued(message, ToastLevel::Error);
    }

    /// Show an info toast
    pub fn toast_info(&mut self, message: impl Into<String>) {
        self.show_toast_queued(message, ToastLevel::Info);
    }

    /// Show a warning toast
    pub fn toast_warning(&mut self, message: impl Into<String>) {
        self.show_toast_queued(message, ToastLevel::Warning);
    }

    /// Dismiss the current toast (shows next in queue if any)
    pub fn dismiss_toast(&mut self) -> Option<QueuedToast> {
        self.toast_queue.dismiss()
    }

    /// Check if any toast is active (queue-based)
    #[must_use]
    pub fn has_queued_toast(&self) -> bool {
        self.toast_queue.is_active()
    }

    /// Process a tick for the toast queue (auto-dismiss expired toasts)
    pub fn tick_toasts(&mut self) -> bool {
        self.toast_queue.tick()
    }

    /// Show the account setup modal via queue
    pub fn show_account_setup_queued(&mut self) {
        self.modal_queue
            .enqueue(QueuedModal::AccountSetup(AccountSetupModalState::default()));
    }

    pub(crate) fn set_authoritative_operation_state(
        &mut self,
        operation_id: OperationId,
        instance_id: Option<OperationInstanceId>,
        causality: Option<SemanticOperationCausality>,
        state: OperationState,
    ) {
        self.operation_states
            .set_authoritative_state(operation_id, instance_id, causality, state);
    }

    #[must_use]
    pub fn operation_state(&self, operation_id: &OperationId) -> Option<OperationState> {
        self.operation_states.state(operation_id)
    }

    pub fn upsert_runtime_fact(&mut self, fact: RuntimeFact) {
        let key = fact.key();
        self.runtime_facts.retain(|existing| existing.key() != key);
        self.runtime_facts.push(fact);
    }

    pub fn mark_subscription_degraded(
        &mut self,
        signal_id: impl Into<String>,
        reason: impl Into<String>,
    ) -> bool {
        let signal_id = signal_id.into();
        let reason = reason.into();
        let changed = self.degraded_subscriptions.get(&signal_id) != Some(&reason);
        self.degraded_subscriptions.insert(signal_id, reason);
        changed
    }

    #[must_use]
    pub fn degraded_subscription_count(&self) -> usize {
        self.degraded_subscriptions.len()
    }

    pub fn clear_runtime_fact_kind(&mut self, kind: RuntimeEventKind) {
        self.runtime_facts
            .retain(|existing| existing.kind() != kind);
    }

    pub fn apply_runtime_facts_update(
        &mut self,
        revision: ProjectionRevision,
        replace_kinds: Vec<RuntimeEventKind>,
        facts: Vec<RuntimeFact>,
    ) -> bool {
        if revision.is_stale_against(self.last_authoritative_runtime_facts_revision) {
            return false;
        }
        self.last_authoritative_runtime_facts_revision = revision;
        for kind in replace_kinds {
            self.clear_runtime_fact_kind(kind);
        }
        for fact in facts {
            self.upsert_runtime_fact(fact);
        }
        true
    }

    #[must_use]
    pub fn exported_operation_snapshots(&self) -> Vec<OperationSnapshot> {
        self.operation_states.exported_snapshots()
    }

    #[must_use]
    pub fn exported_runtime_events(&self) -> Vec<RuntimeEventSnapshot> {
        let mut facts = self.runtime_facts.clone();
        facts.sort_by_key(|left| left.key());
        facts
            .iter()
            .map(|fact| RuntimeEventSnapshot {
                id: RuntimeEventId(format!("tui-runtime-fact-{}", fact.key())),
                fact: fact.clone(),
            })
            .collect()
    }

    /// Signal that account creation succeeded (queue-based)
    /// Dismisses modal and shows a success toast instead of a success screen
    pub fn account_created_queued(&mut self) {
        // Get the nickname suggestion before dismissing
        let nickname_suggestion =
            if let Some(QueuedModal::AccountSetup(ref state)) = self.modal_queue.current() {
                state.nickname_suggestion.clone()
            } else {
                String::new()
            };

        // Dismiss the modal
        self.modal_queue.dismiss();

        // Show a success toast
        let message = if nickname_suggestion.is_empty() {
            "Account created successfully".to_string()
        } else {
            format!("Welcome, {nickname_suggestion}!")
        };
        self.next_toast_id += 1;
        self.toast_queue
            .enqueue(QueuedToast::success(self.next_toast_id, message));
    }

    /// Signal that account creation failed (queue-based)
    pub fn account_creation_failed_queued(&mut self, error: String) {
        if let Some(QueuedModal::AccountSetup(ref mut state)) = self.modal_queue.current_mut() {
            state.set_error(error);
        }
    }

    /// Check if the current modal is a text input modal (queue-based)
    #[must_use]
    pub fn is_queued_modal_text_input(&self) -> bool {
        match self.modal_queue.current() {
            Some(QueuedModal::AccountSetup(_)) => true,
            Some(QueuedModal::ChatCreate(_)) => true,
            Some(QueuedModal::ChatTopic(_)) => true,
            Some(QueuedModal::ContactsNickname(_)) => true,
            Some(QueuedModal::ContactsImport(_)) => true,
            Some(QueuedModal::SettingsNicknameSuggestion(_)) => true,
            Some(QueuedModal::SettingsAddDevice(_)) => true,
            Some(QueuedModal::NeighborhoodHomeCreate(_)) => true,
            Some(QueuedModal::NeighborhoodCapabilityConfig(_)) => true,
            _ => false,
        }
    }

    /// Check if in insert mode (for text input)
    #[must_use]
    pub fn is_insert_mode(&self) -> bool {
        match self.screen() {
            Screen::Chat => self.chat.insert_mode,
            _ => false,
        }
    }

    // ========================================================================
    // Modal Helper Methods
    // ========================================================================

    /// Check if any modal is active
    #[must_use]
    pub fn has_modal(&self) -> bool {
        self.has_queued_modal()
    }

    /// Get reference to account setup state if it's the active modal
    #[must_use]
    pub fn account_setup_state(&self) -> Option<&AccountSetupModalState> {
        match self.modal_queue.current() {
            Some(QueuedModal::AccountSetup(state)) => Some(state),
            _ => None,
        }
    }

    /// Get mutable reference to account setup state if it's the active modal
    pub fn account_setup_state_mut(&mut self) -> Option<&mut AccountSetupModalState> {
        match self.modal_queue.current_mut() {
            Some(QueuedModal::AccountSetup(state)) => Some(state),
            _ => None,
        }
    }

    /// Signal that account creation succeeded
    pub fn account_created(&mut self) {
        self.account_created_queued();
    }

    /// Signal that account creation failed
    pub fn account_creation_failed(&mut self, error: String) {
        self.account_creation_failed_queued(error);
    }

    // ========================================================================
    // Modal Type Checking (for tests and rendering)
    // ========================================================================

    /// Check if chat create modal is active
    #[must_use]
    pub fn is_chat_create_modal_active(&self) -> bool {
        matches!(self.modal_queue.current(), Some(QueuedModal::ChatCreate(_)))
    }

    /// Get chat create modal state if active
    #[must_use]
    pub fn chat_create_modal_state(&self) -> Option<&CreateChannelModalState> {
        match self.modal_queue.current() {
            Some(QueuedModal::ChatCreate(state)) => Some(state),
            _ => None,
        }
    }

    /// Get mutable chat create modal state if active
    pub fn chat_create_modal_state_mut(&mut self) -> Option<&mut CreateChannelModalState> {
        match self.modal_queue.current_mut() {
            Some(QueuedModal::ChatCreate(state)) => Some(state),
            _ => None,
        }
    }

    /// Check if chat topic modal is active
    #[must_use]
    pub fn is_chat_topic_modal_active(&self) -> bool {
        matches!(self.modal_queue.current(), Some(QueuedModal::ChatTopic(_)))
    }

    /// Get chat topic modal state if active
    #[must_use]
    pub fn chat_topic_modal_state(&self) -> Option<&TopicModalState> {
        match self.modal_queue.current() {
            Some(QueuedModal::ChatTopic(state)) => Some(state),
            _ => None,
        }
    }

    /// Check if chat info modal is active
    #[must_use]
    pub fn is_chat_info_modal_active(&self) -> bool {
        matches!(self.modal_queue.current(), Some(QueuedModal::ChatInfo(_)))
    }

    /// Check if guardian setup modal is active
    #[must_use]
    pub fn is_guardian_setup_modal_active(&self) -> bool {
        matches!(
            self.modal_queue.current(),
            Some(QueuedModal::GuardianSetup(_))
        )
    }

    /// Get guardian setup modal state if active
    #[must_use]
    pub fn guardian_setup_modal_state(&self) -> Option<&GuardianSetupModalState> {
        match self.modal_queue.current() {
            Some(QueuedModal::GuardianSetup(state)) => Some(state),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parity_critical_operation_ids() -> [OperationId; 5] {
        [
            OperationId::invitation_create(),
            OperationId::invitation_accept_contact(),
            OperationId::invitation_accept_channel(),
            OperationId("join_channel".to_string()),
            OperationId::send_message(),
        ]
    }

    fn assert_local_terminal_regression_allocates_new_instance(operation_id: OperationId) {
        let mut state = TuiState::new();
        state
            .operation_states
            .set_state(operation_id.clone(), OperationState::Succeeded);
        let first = state.exported_operation_snapshots();
        let first_instance = first[0].instance_id.clone();

        state
            .operation_states
            .set_state(operation_id, OperationState::Failed);

        let snapshots = state.exported_operation_snapshots();
        assert_eq!(snapshots[0].state, OperationState::Failed);
        assert_ne!(snapshots[0].instance_id, first_instance);
    }

    fn assert_authoritative_terminal_regression_allocates_new_instance(operation_id: OperationId) {
        let mut state = TuiState::new();
        state.set_authoritative_operation_state(
            operation_id.clone(),
            None,
            None,
            OperationState::Succeeded,
        );
        let first = state.exported_operation_snapshots();
        let first_instance = first[0].instance_id.clone();

        state.set_authoritative_operation_state(operation_id, None, None, OperationState::Failed);

        let snapshots = state.exported_operation_snapshots();
        assert_eq!(snapshots[0].state, OperationState::Failed);
        assert_ne!(snapshots[0].instance_id, first_instance);
    }

    fn assert_older_authoritative_instance_cannot_replace_newer_submission(
        operation_id: OperationId,
    ) {
        let mut state = TuiState::new();
        let stale = OperationInstanceId(format!("tui-op-{}-2", operation_id.0));
        let current = OperationInstanceId(format!("tui-op-{}-3", operation_id.0));

        state.set_authoritative_operation_state(
            operation_id.clone(),
            Some(current.clone()),
            None,
            OperationState::Submitting,
        );
        state.set_authoritative_operation_state(
            operation_id,
            Some(stale),
            None,
            OperationState::Succeeded,
        );

        let snapshots = state.exported_operation_snapshots();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].instance_id, current);
        assert_eq!(snapshots[0].state, OperationState::Submitting);
    }

    #[test]
    fn operation_tracker_reissues_instance_id_on_new_submission() {
        let mut state = TuiState::new();
        let operation_id = OperationId::invitation_create();

        state
            .operation_states
            .set_state(operation_id.clone(), OperationState::Submitting);
        let first = state.exported_operation_snapshots();
        assert_eq!(first.len(), 1);
        let first_instance = first[0].instance_id.clone();

        state
            .operation_states
            .set_state(operation_id.clone(), OperationState::Succeeded);
        let second = state.exported_operation_snapshots();
        assert_eq!(second[0].instance_id, first_instance);
        assert_eq!(second[0].state, OperationState::Succeeded);

        state
            .operation_states
            .set_state(operation_id, OperationState::Submitting);
        let third = state.exported_operation_snapshots();
        assert_eq!(third[0].state, OperationState::Submitting);
        assert_ne!(third[0].instance_id, first_instance);
    }

    #[test]
    fn stale_authoritative_runtime_fact_updates_are_rejected() {
        let mut state = TuiState::new();
        let channel = aura_app::ui_contract::ChannelFactKey {
            id: Some("channel-alpha".to_string()),
            name: Some("alpha".to_string()),
        };

        assert!(state.apply_runtime_facts_update(
            ProjectionRevision {
                semantic_seq: 2,
                render_seq: None,
            },
            vec![RuntimeEventKind::ChannelMembershipReady],
            vec![RuntimeFact::ChannelMembershipReady {
                channel: channel.clone(),
                member_count: Some(2),
            }],
        ));
        assert!(!state.apply_runtime_facts_update(
            ProjectionRevision {
                semantic_seq: 1,
                render_seq: None,
            },
            vec![RuntimeEventKind::ChannelMembershipReady],
            vec![RuntimeFact::ChannelMembershipReady {
                channel: channel.clone(),
                member_count: Some(1),
            }],
        ));

        assert_eq!(
            state.runtime_facts,
            vec![RuntimeFact::ChannelMembershipReady {
                channel,
                member_count: Some(2),
            }]
        );
    }

    #[test]
    fn newer_authoritative_runtime_fact_updates_replace_fact_kinds() {
        let mut state = TuiState::new();
        let channel = aura_app::ui_contract::ChannelFactKey {
            id: Some("channel-alpha".to_string()),
            name: Some("alpha".to_string()),
        };

        assert!(state.apply_runtime_facts_update(
            ProjectionRevision {
                semantic_seq: 1,
                render_seq: None,
            },
            vec![RuntimeEventKind::ChannelMembershipReady],
            vec![RuntimeFact::ChannelMembershipReady {
                channel: channel.clone(),
                member_count: Some(1),
            }],
        ));
        assert!(state.apply_runtime_facts_update(
            ProjectionRevision {
                semantic_seq: 2,
                render_seq: None,
            },
            vec![RuntimeEventKind::ChannelMembershipReady],
            vec![RuntimeFact::ChannelMembershipReady {
                channel: channel.clone(),
                member_count: Some(3),
            }],
        ));

        assert_eq!(
            state.runtime_facts,
            vec![RuntimeFact::ChannelMembershipReady {
                channel,
                member_count: Some(3),
            }]
        );
    }

    #[test]
    fn operation_tracker_preserves_instance_id_for_authoritative_updates() {
        let mut state = TuiState::new();
        state.set_authoritative_operation_state(
            OperationId::invitation_accept_contact(),
            None,
            None,
            OperationState::Submitting,
        );
        let first = state.exported_operation_snapshots();
        let first_instance = first[0].instance_id.clone();

        state.set_authoritative_operation_state(
            OperationId::invitation_accept_contact(),
            None,
            None,
            OperationState::Submitting,
        );
        let second = state.exported_operation_snapshots();
        assert_eq!(second[0].instance_id, first_instance);

        state.set_authoritative_operation_state(
            OperationId::invitation_accept_contact(),
            None,
            None,
            OperationState::Succeeded,
        );
        let third = state.exported_operation_snapshots();
        assert_eq!(third[0].instance_id, first_instance);
        assert_eq!(third[0].state, OperationState::Succeeded);
    }

    #[test]
    fn degraded_subscriptions_are_structural_and_deduplicated() {
        let mut state = TuiState::new();

        assert!(state.mark_subscription_degraded("chat", "retry budget exhausted"));
        assert_eq!(state.degraded_subscription_count(), 1);
        assert_eq!(
            state.degraded_subscriptions.get("chat"),
            Some(&"retry budget exhausted".to_string())
        );

        assert!(!state.mark_subscription_degraded("chat", "retry budget exhausted"));
        assert_eq!(state.degraded_subscription_count(), 1);

        assert!(state.mark_subscription_degraded("chat", "signal closed"));
        assert_eq!(state.degraded_subscription_count(), 1);
        assert_eq!(
            state.degraded_subscriptions.get("chat"),
            Some(&"signal closed".to_string())
        );

        assert!(state.mark_subscription_degraded("network", "subscription cancelled"));
        assert_eq!(state.degraded_subscription_count(), 2);
    }

    #[test]
    fn authoritative_submitting_after_terminal_allocates_new_instance() {
        let mut state = TuiState::new();
        let operation_id = OperationId::invitation_create();
        state.set_authoritative_operation_state(
            operation_id.clone(),
            None,
            None,
            OperationState::Submitting,
        );
        let first = state.exported_operation_snapshots();
        let first_instance = first[0].instance_id.clone();

        state.set_authoritative_operation_state(
            operation_id.clone(),
            None,
            None,
            OperationState::Succeeded,
        );
        state.set_authoritative_operation_state(
            operation_id,
            None,
            None,
            OperationState::Submitting,
        );

        let snapshots = state.exported_operation_snapshots();
        assert_eq!(snapshots[0].state, OperationState::Submitting);
        assert_ne!(snapshots[0].instance_id, first_instance);
    }

    #[test]
    fn local_terminal_regression_allocates_new_instance() {
        let mut state = TuiState::new();
        let operation_id = OperationId::invitation_create();
        state
            .operation_states
            .set_state(operation_id.clone(), OperationState::Succeeded);
        let first = state.exported_operation_snapshots();
        let first_instance = first[0].instance_id.clone();

        state
            .operation_states
            .set_state(operation_id, OperationState::Failed);

        let snapshots = state.exported_operation_snapshots();
        assert_eq!(snapshots[0].state, OperationState::Failed);
        assert_ne!(snapshots[0].instance_id, first_instance);
    }

    #[test]
    fn authoritative_terminal_regression_without_instance_allocates_new_instance() {
        let mut state = TuiState::new();
        let operation_id = OperationId::invitation_accept_contact();
        state.set_authoritative_operation_state(
            operation_id.clone(),
            None,
            None,
            OperationState::Succeeded,
        );
        let first = state.exported_operation_snapshots();
        let first_instance = first[0].instance_id.clone();

        state.set_authoritative_operation_state(operation_id, None, None, OperationState::Failed);

        let snapshots = state.exported_operation_snapshots();
        assert_eq!(snapshots[0].state, OperationState::Failed);
        assert_ne!(snapshots[0].instance_id, first_instance);
    }

    #[test]
    fn authoritative_terminal_regression_for_same_instance_is_ignored() {
        let mut state = TuiState::new();
        let operation_id = OperationId::invitation_accept_contact();
        state.set_authoritative_operation_state(
            operation_id.clone(),
            None,
            None,
            OperationState::Succeeded,
        );
        let first = state.exported_operation_snapshots();
        let first_instance = first[0].instance_id.clone();

        state.set_authoritative_operation_state(
            operation_id,
            Some(first_instance.clone()),
            None,
            OperationState::Failed,
        );

        let snapshots = state.exported_operation_snapshots();
        assert_eq!(snapshots[0].state, OperationState::Succeeded);
        assert_eq!(snapshots[0].instance_id, first_instance);
    }

    #[test]
    fn authoritative_update_for_older_instance_does_not_replace_newer_submission() {
        let mut state = TuiState::new();
        let operation_id = OperationId::invitation_accept_contact();

        state.set_authoritative_operation_state(
            operation_id.clone(),
            Some(OperationInstanceId(
                "tui-op-invitation_accept-3".to_string(),
            )),
            None,
            OperationState::Submitting,
        );
        state.set_authoritative_operation_state(
            operation_id,
            Some(OperationInstanceId(
                "tui-op-invitation_accept-2".to_string(),
            )),
            None,
            OperationState::Succeeded,
        );

        let snapshots = state.exported_operation_snapshots();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(
            snapshots[0].instance_id,
            OperationInstanceId("tui-op-invitation_accept-3".to_string())
        );
        assert_eq!(snapshots[0].state, OperationState::Submitting);
    }

    #[test]
    fn authoritative_update_uses_causality_before_instance_suffix_ordering() {
        let mut state = TuiState::new();
        let operation_id = OperationId::invitation_accept_contact();

        state.set_authoritative_operation_state(
            operation_id.clone(),
            Some(OperationInstanceId(
                "tui-op-invitation_accept-2".to_string(),
            )),
            Some(SemanticOperationCausality::new(
                aura_core::OwnerEpoch::new(0),
                aura_core::PublicationSequence::new(5),
            )),
            OperationState::Submitting,
        );
        state.set_authoritative_operation_state(
            operation_id,
            Some(OperationInstanceId(
                "tui-op-invitation_accept-9".to_string(),
            )),
            Some(SemanticOperationCausality::new(
                aura_core::OwnerEpoch::new(0),
                aura_core::PublicationSequence::new(4),
            )),
            OperationState::Succeeded,
        );

        let snapshots = state.exported_operation_snapshots();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(
            snapshots[0].instance_id,
            OperationInstanceId("tui-op-invitation_accept-2".to_string())
        );
        assert_eq!(snapshots[0].state, OperationState::Submitting);
    }

    #[test]
    fn parity_critical_operation_tracker_invariants_hold() {
        for operation_id in parity_critical_operation_ids() {
            assert_local_terminal_regression_allocates_new_instance(operation_id.clone());
            assert_authoritative_terminal_regression_allocates_new_instance(operation_id.clone());
            assert_older_authoritative_instance_cannot_replace_newer_submission(operation_id);
        }
    }
}
