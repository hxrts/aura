//! # Custom Hooks for iocraft
//!
//! Bridges reactive state with iocraft's component system using the unified
//! `ReactiveEffects` system from aura-core.
//!
//! ## Overview
//!
//! These hooks allow iocraft components to subscribe to application signals
//! and automatically re-render when data changes.
//!
//! ## Push-Based Signal Subscription
//!
//! iocraft's `use_future` hook owns a supervised subscription for the
//! component lifetime. The helper reports typed health to the shell and
//! updates iocraft's `State<T>` when a signal emits a new value.
//!
//! ```ignore
//! use iocraft::prelude::*;
//! use aura_app::ui::signals::CHAT_SIGNAL;
//! use crate::tui::hooks::{subscribe_signal_with_retry, AppCoreContext};
//!
//! #[component]
//! fn ChatScreen(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
//!     // Get the reporting context from the shell.
//!     let ctx = hooks.use_context::<AppCoreContext>();
//!
//!     // Initialize state from current value
//!     let chat_state = hooks.use_state(|| Default::default());
//!
//!     // Subscribe to signal updates via use_future
//!     hooks.use_future({
//!         let mut chat_state = chat_state.clone();
//!         let subscription = ctx.for_subscription_scope("chat");
//!         async move {
//!             subscribe_signal_with_retry(subscription, &*CHAT_SIGNAL, move |value| {
//!                 chat_state.set(value);
//!             }).await;
//!         }
//!     });
//!
//!     element! {
//!         Text(content: format!("Messages: {}", chat_state.read().messages.len()))
//!     }
//! }
//! ```
//!
//! ## Snapshot Utilities
//!
//! For components that don't need live updates, snapshot functions provide
//! point-in-time reads of reactive state.

use std::sync::Arc;
use std::time::Duration;

use async_lock::Mutex;
use aura_app::harness_mode_enabled;
use aura_app::ui::prelude::*;
use aura_core::effects::reactive::{ReactiveEffects, ReactiveError, Signal};
use aura_core::{
    execute_with_retry_budget, ExponentialBackoffPolicy, RetryBudgetPolicy, RetryRunError,
    TimeoutExecutionProfile,
};
use aura_effects::time::PhysicalTimeHandler;
use parking_lot::RwLock;

use crate::error::TerminalResult;
use crate::tui::context::{InitializedAppCore, IoContext};
use crate::tui::tasks::UiTaskOwner;
use crate::tui::updates::{
    spawn_ordered_ui_updates, OrderedUiUpdateGate, UiUpdate, UiUpdateSender,
};
use aura_app::ui_contract::SubscriptionFailureCode;

#[derive(Debug, Clone)]
pub enum AppSnapshotAvailability {
    Available(Box<aura_app::ui::types::StateSnapshot>),
    Contended,
}

// =============================================================================
// AppCore Context for iocraft
// =============================================================================

/// Context type for sharing AppCore with iocraft components.
///
/// This enables components to access AppCore via `hooks.use_context::<AppCoreContext>()`.
/// Components can then use `use_future` to subscribe to signals for reactive updates
/// via the unified `ReactiveEffects` system.
///
/// ## Example
///
/// ```ignore
/// use crate::tui::hooks::{subscribe_signal_with_retry, AppCoreContext};
/// use aura_app::ui::signals::CHAT_SIGNAL;
///
/// #[component]
/// fn MyComponent(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
///     let ctx = hooks.use_context::<AppCoreContext>();
///
///     // Initialize state from current value
///     let messages = hooks.use_state(|| Vec::new());
///
///     // Subscribe through the component-owned health reporter.
///     hooks.use_future({
///         let mut messages = messages.clone();
///         let subscription = ctx.for_subscription_scope("chat");
///         async move {
///             subscribe_signal_with_retry(subscription, &*CHAT_SIGNAL, move |state| {
///                 messages.set(state.messages.clone());
///             }).await;
///         }
///     });
///
///     element! { ... }
/// }
/// ```
#[derive(Clone)]
pub struct AppCoreContext {
    /// The shared AppCore instance (signals initialized)
    pub app_core: InitializedAppCore,

    /// The IoContext for effect dispatch
    io_context: Arc<IoContext>,
    subscription_updates: Arc<RwLock<Option<UiUpdateSender>>>,
    subscription_health_gate: Arc<OrderedUiUpdateGate>,
    subscription_scope: &'static str,
}

impl AppCoreContext {
    /// Create a new AppCoreContext
    #[must_use]
    pub fn new(app_core: InitializedAppCore, io_context: Arc<IoContext>) -> Self {
        Self {
            app_core,
            io_context,
            subscription_updates: Arc::new(RwLock::new(None)),
            subscription_health_gate: Arc::new(OrderedUiUpdateGate::new()),
            subscription_scope: "shell",
        }
    }

    /// Give a mounted screen an independent health identity for shared signals.
    #[must_use]
    pub fn for_subscription_scope(&self, scope: &'static str) -> Self {
        let mut scoped = self.clone();
        scoped.subscription_scope = scope;
        scoped
    }

    /// Connect component subscription health to the shell's owned update loop.
    pub fn set_subscription_update_sender(&self, sender: Option<UiUpdateSender>) {
        *self.subscription_updates.write() = sender;
    }

    fn report_subscription_health(&self, signal_id: String, health: SubscriptionHealth) {
        if let Some(sender) = self.subscription_updates.read().clone() {
            spawn_ordered_ui_updates(
                &self.tasks(),
                &sender,
                &self.subscription_health_gate,
                vec![subscription_health_update(
                    format!("{}/{signal_id}", self.subscription_scope),
                    health,
                )],
            );
        }
    }

    /// Get a snapshot of the current state
    ///
    /// This is useful for initializing iocraft State<T> values.
    #[must_use]
    pub fn snapshot(&self) -> AppSnapshotAvailability {
        match self.app_core.raw().try_read() {
            Some(guard) => AppSnapshotAvailability::Available(Box::new(guard.snapshot())),
            None => AppSnapshotAvailability::Contended,
        }
    }

    /// Dispatch an effect command through IoContext
    pub async fn dispatch(&self, cmd: crate::tui::effects::EffectCommand) -> TerminalResult<()> {
        self.io_context.dispatch(cmd).await
    }

    pub async fn dispatch_and_wait(
        &self,
        cmd: crate::tui::effects::EffectCommand,
    ) -> TerminalResult<()> {
        self.io_context.dispatch_and_wait(cmd).await
    }

    pub async fn export_invitation_code(&self, invitation_id: &str) -> TerminalResult<String> {
        self.io_context.export_invitation_code(invitation_id).await
    }

    pub async fn remember_key_rotation_ceremony(
        &self,
        handle: aura_app::ui::workflows::ceremonies::CeremonyHandle,
    ) {
        self.io_context.remember_key_rotation_ceremony(handle).await;
    }

    pub async fn key_rotation_ceremony_status_handle(
        &self,
        ceremony_id: &str,
    ) -> TerminalResult<aura_app::ui::workflows::ceremonies::CeremonyStatusHandle> {
        self.io_context
            .key_rotation_ceremony_status_handle(ceremony_id)
            .await
    }

    pub async fn take_key_rotation_ceremony_handle(
        &self,
        ceremony_id: &str,
    ) -> TerminalResult<aura_app::ui::workflows::ceremonies::CeremonyHandle> {
        self.io_context
            .take_key_rotation_ceremony_handle(ceremony_id)
            .await
    }

    pub async fn forget_key_rotation_ceremony(&self, ceremony_id: &str) {
        self.io_context
            .forget_key_rotation_ceremony(ceremony_id)
            .await;
    }

    pub async fn add_error_toast(&self, id: impl Into<String>, message: impl Into<String>) {
        self.io_context.add_error_toast(id, message).await;
    }

    pub async fn add_success_toast(&self, id: impl Into<String>, message: impl Into<String>) {
        self.io_context.add_success_toast(id, message).await;
    }

    pub async fn add_info_toast(&self, id: impl Into<String>, message: impl Into<String>) {
        self.io_context.add_info_toast(id, message).await;
    }

    pub fn request_authority_switch(
        &self,
        authority_id: aura_core::types::identifiers::AuthorityId,
        nickname_suggestion: Option<String>,
    ) {
        self.io_context
            .request_authority_switch(authority_id, nickname_suggestion);
    }

    #[must_use]
    pub fn tasks(&self) -> Arc<UiTaskOwner> {
        self.io_context.tasks()
    }

    #[must_use]
    pub fn io_context(&self) -> Arc<IoContext> {
        self.io_context.clone()
    }

    #[must_use]
    pub fn bootstrap_runtime_handoff_committed(&self) -> bool {
        self.io_context.bootstrap_runtime_handoff_committed()
    }

    pub fn mark_bootstrap_runtime_handoff_committed(&self) -> TerminalResult<()> {
        self.io_context.mark_bootstrap_runtime_handoff_committed()
    }
}

// =============================================================================
// Signal Subscription Helpers
// =============================================================================

/// Subscribe to a reactive signal and keep the subscription alive.
///
/// This is the default TUI subscription primitive. It avoids a class of
/// "silent non-updating" UIs by ensuring that:
/// - attachment and recovery publish typed health through the shell update loop,
/// - subscription failures emit `ERROR_SIGNAL` for diagnostics, and
/// - subscriptions retry with a bounded backoff policy.
///
/// **Behavior**:
/// - Attaches first, then reads the current value for catch-up.
/// - Forwards stream values to `on_value`.
/// - On error, reports health and retries until the owner budget ends.
///
/// Maximum outer retry attempts before giving up on a signal subscription.
/// At 2s max backoff this is ~6+ minutes of retrying before the loop exits.
#[cfg(not(test))]
const MAX_SUBSCRIPTION_RETRIES: u32 = 200;
#[cfg(test)]
const MAX_SUBSCRIPTION_RETRIES: u32 = 1;
const SUBSCRIPTION_INITIAL_BACKOFF: Duration = Duration::from_millis(50);
const SUBSCRIPTION_MAX_BACKOFF: Duration = Duration::from_secs(2);

fn subscription_timeout_profile() -> TimeoutExecutionProfile {
    if harness_mode_enabled() {
        TimeoutExecutionProfile::harness()
    } else {
        TimeoutExecutionProfile::production()
    }
}

#[allow(clippy::expect_used)]
fn subscription_retry_policy() -> RetryBudgetPolicy {
    let profile = subscription_timeout_profile();
    let base = RetryBudgetPolicy::new(
        MAX_SUBSCRIPTION_RETRIES,
        ExponentialBackoffPolicy::new(
            SUBSCRIPTION_INITIAL_BACKOFF,
            SUBSCRIPTION_MAX_BACKOFF,
            profile.jitter(),
        )
        .expect("subscription backoff policy must be valid"),
    );
    profile
        .apply_retry_policy(&base)
        .expect("subscription retry policy must scale")
}

pub async fn subscribe_signal_with_retry<T, F>(
    app_ctx: AppCoreContext,
    signal: &'static Signal<T>,
    on_value: F,
) where
    T: Clone + Send + Sync + 'static,
    F: FnMut(T) + Send + 'static,
{
    let app_core = app_ctx.app_core.clone();
    let signal_id = signal.id().to_string();
    subscribe_signal_with_retry_report(app_core, signal, on_value, move |health| {
        app_ctx.report_subscription_health(signal_id.clone(), health);
    })
    .await;
}

/// Health transitions for a component-owned reactive subscription.
#[derive(Debug, Clone)]
pub enum SubscriptionHealth {
    /// A fresh snapshot was delivered to the component.
    Ready,
    /// The current attachment failed and the owner is retrying.
    Retrying(ReactiveError),
    /// The bounded retry policy ended without a live subscription.
    Degraded(RetryRunError<ReactiveError>),
}

fn subscription_failure_code(error: &ReactiveError) -> SubscriptionFailureCode {
    match error {
        ReactiveError::SignalNotFound { .. } => SubscriptionFailureCode::RegistrationFailed,
        ReactiveError::SubscriptionClosed { .. } => SubscriptionFailureCode::StreamClosed,
        _ => SubscriptionFailureCode::SnapshotReadFailed,
    }
}

pub(crate) fn subscription_health_update(
    signal_id: String,
    health: SubscriptionHealth,
) -> UiUpdate {
    match health {
        SubscriptionHealth::Ready => UiUpdate::SubscriptionRecovered { signal_id },
        SubscriptionHealth::Retrying(error) => UiUpdate::SubscriptionRetrying {
            signal_id,
            reason_code: subscription_failure_code(&error),
        },
        SubscriptionHealth::Degraded(error) => {
            let reason_code = match &error {
                RetryRunError::AttemptsExhausted { last_error, .. } => {
                    subscription_failure_code(last_error)
                }
                RetryRunError::Timeout(_) => SubscriptionFailureCode::SnapshotReadFailed,
            };
            UiUpdate::SubscriptionDegraded {
                signal_id,
                reason: error.to_string(),
                reason_code,
            }
        }
    }
}

pub async fn subscribe_signal_with_retry_report<T, F, G>(
    app_core: InitializedAppCore,
    signal: &'static Signal<T>,
    on_value: F,
    on_health: G,
) where
    T: Clone + Send + Sync + 'static,
    F: FnMut(T) + Send + 'static,
    G: Fn(SubscriptionHealth) + Send + 'static,
{
    let reactive: ReactiveHandler = {
        let core = app_core.raw().read().await;
        core.reactive().clone()
    };

    let last_emitted = Arc::new(Mutex::new(None::<String>));
    let on_value = Arc::new(Mutex::new(on_value));
    let on_health = Arc::new(on_health);
    let time = PhysicalTimeHandler::new();
    let retry_policy = subscription_retry_policy();

    let result: Result<(), RetryRunError<ReactiveError>> =
        execute_with_retry_budget(&time, &retry_policy, |_attempt| async {
            if !reactive.is_registered(signal.id()) {
                let error = ReactiveError::SignalNotFound {
                    id: signal.id().to_string(),
                };
                (on_health)(SubscriptionHealth::Retrying(error.clone()));
                maybe_emit_reactive_error(&reactive, &last_emitted, error.to_string()).await;
                return Err(error);
            }

            let mut stream = match reactive.subscribe_attached(signal).await {
                Ok(stream) => stream,
                Err(error) => {
                    (on_health)(SubscriptionHealth::Retrying(error.clone()));
                    maybe_emit_reactive_error(&reactive, &last_emitted, error.to_string()).await;
                    return Err(error);
                }
            };

            match reactive.read(signal).await {
                Ok(value) => {
                    let mut on_value = on_value.lock().await;
                    (*on_value)(value);
                    (on_health)(SubscriptionHealth::Ready);
                }
                Err(e) => {
                    (on_health)(SubscriptionHealth::Retrying(e.clone()));
                    maybe_emit_reactive_error(&reactive, &last_emitted, e.to_string()).await;
                    return Err(e);
                }
            }

            loop {
                match stream.recv().await {
                    Ok(value) => {
                        let mut on_value = on_value.lock().await;
                        (*on_value)(value);
                    }
                    Err(e) => {
                        (on_health)(SubscriptionHealth::Retrying(e.clone()));
                        maybe_emit_reactive_error(&reactive, &last_emitted, e.to_string()).await;
                        return Err(e);
                    }
                }
            }
        })
        .await;

    if let Err(error) = result {
        tracing::warn!(signal = %signal.id(), %error, "Signal subscription abandoned");
        (on_health)(SubscriptionHealth::Degraded(error));
    }
}

async fn maybe_emit_reactive_error(
    reactive: &ReactiveHandler,
    last_emitted: &Arc<Mutex<Option<String>>>,
    message: String,
) {
    let mut last_emitted = last_emitted.lock().await;
    if last_emitted.as_deref() == Some(&message) {
        return;
    }

    *last_emitted = Some(message.clone());
    let _ = reactive
        .emit(
            &*ERROR_SIGNAL,
            Some(AppError::internal("tui:reactive", message)),
        )
        .await;
}

/// Trait for types that can be used with reactive hooks
pub trait ReactiveValue: Clone + Send + Sync + 'static {}
impl<T: Clone + Send + Sync + 'static> ReactiveValue for T {}

/// Snapshot of a ReactiveState for use in iocraft components
///
/// Returns the current value. For real-time push-based updates, use `use_future`
/// with signal subscription (see module documentation).
#[must_use]
pub fn snapshot_state<T: Clone>(state: &ReactiveState<T>) -> T {
    state.get()
}

/// Snapshot of a ReactiveVec for use in iocraft components
///
/// Returns a cloned vector of all current items.
#[must_use]
pub fn snapshot_vec<T: Clone>(vec: &ReactiveVec<T>) -> Vec<T> {
    vec.get_cloned()
}

/// Helper to check if a ReactiveVec is empty
#[must_use]
pub fn is_vec_empty<T: Clone>(vec: &ReactiveVec<T>) -> bool {
    vec.is_empty()
}

/// Helper to get the length of a ReactiveVec
#[must_use]
pub fn vec_len<T: Clone>(vec: &ReactiveVec<T>) -> usize {
    vec.len()
}

// =============================================================================
// Props Helpers
// =============================================================================

/// Trait for props that contain reactive data
///
/// Implement this trait to enable automatic snapshot extraction in components.
pub trait HasReactiveData {
    /// Type of the snapshot data
    type Snapshot;

    /// Create a snapshot of all reactive data for rendering
    fn snapshot(&self) -> Self::Snapshot;
}

// =============================================================================
// View Snapshot Types
// =============================================================================
//
// These snapshot structs are populated from AppCore's ViewState. The old
// View classes (ChatView, GuardiansView, etc.) have been removed - screens
// now subscribe directly to AppCore signals for reactive updates.

/// Snapshot of chat-related data for rendering
#[derive(Debug, Clone)]
pub struct ChatSnapshot {
    /// Current channels list
    pub channels: Vec<aura_app::ui::types::chat::Channel>,
    /// Currently selected channel ID
    pub selected_channel: Option<String>,
    /// Messages for the selected channel
    pub messages: Vec<aura_app::ui::types::chat::Message>,
}

impl Default for ChatSnapshot {
    fn default() -> Self {
        Self {
            channels: Vec::new(),
            selected_channel: None,
            messages: Vec::new(),
        }
    }
}

impl ChatSnapshot {
    /// Get the number of channels
    #[must_use]
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    /// Check if there are no channels
    #[must_use]
    pub fn channels_is_empty(&self) -> bool {
        self.channels.is_empty()
    }

    /// Iterate over all channels
    pub fn all_channels(&self) -> impl Iterator<Item = &aura_app::ui::types::chat::Channel> {
        self.channels.iter()
    }
}

/// Snapshot of guardian-related data for rendering
#[derive(Debug, Clone)]
pub struct GuardiansSnapshot {
    /// Guardian list
    pub guardians: Vec<aura_app::ui::types::recovery::Guardian>,
    /// Threshold configuration
    pub threshold: Option<aura_core::threshold::ThresholdConfig>,
}

impl Default for GuardiansSnapshot {
    fn default() -> Self {
        Self {
            guardians: Vec::new(),
            threshold: None,
        }
    }
}

/// Snapshot of recovery-related data for rendering
#[derive(Debug, Clone)]
pub struct RecoverySnapshot {
    /// Recovery state
    pub status: aura_app::ui::types::recovery::RecoveryState,
    /// Progress percentage (0-100)
    pub progress_percent: u32,
    /// Whether recovery is in progress
    pub is_in_progress: bool,
}

impl Default for RecoverySnapshot {
    fn default() -> Self {
        Self {
            status: aura_app::ui::types::recovery::RecoveryState::default(),
            progress_percent: 0,
            is_in_progress: false,
        }
    }
}

/// Snapshot of invitation-related data for rendering
#[derive(Debug, Clone)]
pub struct InvitationsSnapshot {
    /// All invitations
    pub invitations: Vec<aura_app::ui::types::invitations::Invitation>,
    /// Count of pending invitations
    pub pending_count: usize,
}

impl Default for InvitationsSnapshot {
    fn default() -> Self {
        Self {
            invitations: Vec::new(),
            pending_count: 0,
        }
    }
}

/// Snapshot of home-related data for rendering
#[derive(Debug, Clone)]
pub struct HomeSnapshot {
    /// Home state (contains id, name, members, storage, etc.)
    pub home_state: Option<aura_app::ui::types::home::HomeState>,
    /// Whether user is a member
    pub is_member: bool,
    /// Whether user is a moderator
    pub is_moderator: bool,
}

impl Default for HomeSnapshot {
    fn default() -> Self {
        Self {
            home_state: None,
            is_member: false,
            is_moderator: false,
        }
    }
}

impl HomeSnapshot {
    /// Get members list from home state
    #[must_use]
    pub fn members(&self) -> &[aura_app::ui::types::home::HomeMember] {
        self.home_state
            .as_ref()
            .map(|b| b.members.as_slice())
            .unwrap_or(&[])
    }

    /// Get storage info from home state
    #[must_use]
    pub fn storage(&self) -> aura_app::ui::types::HomeFlowBudget {
        self.home_state
            .as_ref()
            .map(|b| b.storage.clone())
            .unwrap_or_default()
    }
}

/// Snapshot of contacts-related data for rendering
#[derive(Debug, Clone)]
pub struct ContactsSnapshot {
    /// Contacts list
    pub contacts: Vec<aura_app::ui::types::contacts::Contact>,
}

impl Default for ContactsSnapshot {
    fn default() -> Self {
        Self {
            contacts: Vec::new(),
        }
    }
}

impl ContactsSnapshot {
    /// Get total number of contacts
    #[must_use]
    pub fn contact_count(&self) -> usize {
        self.contacts.len()
    }

    /// Check if there are no contacts
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.contacts.is_empty()
    }
}

/// Snapshot of neighborhood-related data for rendering
#[derive(Debug, Clone)]
pub struct NeighborhoodSnapshot {
    /// Neighborhood ID
    pub neighborhood_id: Option<String>,
    /// Neighborhood name
    pub neighborhood_name: Option<String>,
    /// Homes in neighborhood
    pub homes: Vec<aura_app::ui::types::neighborhood::NeighborHome>,
    /// Current traversal position
    pub position: aura_app::ui::types::neighborhood::TraversalPosition,
}

impl Default for NeighborhoodSnapshot {
    fn default() -> Self {
        Self {
            neighborhood_id: None,
            neighborhood_name: None,
            homes: Vec::new(),
            position: aura_app::ui::types::neighborhood::TraversalPosition::default(),
        }
    }
}

/// Snapshot of device-related data for rendering
#[derive(Debug, Clone)]
pub struct DevicesSnapshot {
    /// List of registered devices
    pub devices: Vec<crate::tui::types::Device>,
    /// ID of the current device (for highlighting)
    pub current_device_id: Option<String>,
}

impl Default for DevicesSnapshot {
    fn default() -> Self {
        Self {
            devices: Vec::new(),
            current_device_id: None,
        }
    }
}

// Note: The old View-based snapshot functions have been removed.
// Snapshots are now created directly from AppCore's ViewState in IoContext.
// See context.rs for the snapshot_* implementations.

// =============================================================================
// Callback Context for iocraft
// =============================================================================

use crate::tui::callbacks::CallbackRegistry;

/// Context type for sharing callbacks with iocraft components.
///
/// This enables components to access domain-specific callbacks via
/// `hooks.use_context::<CallbackContext>()` instead of passing them
/// through props at every level.
///
/// ## Example
///
/// ```ignore
/// use crate::tui::hooks::CallbackContext;
///
/// #[component]
/// fn ChatScreen(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
///     let callbacks = hooks.use_context::<CallbackContext>();
///
///     // Access chat-specific callbacks
///     let on_send = callbacks.registry.chat.on_send.clone();
///
///     element! { ... }
/// }
/// ```
#[derive(Clone)]
pub struct CallbackContext {
    /// The callback registry containing all domain callbacks
    pub registry: CallbackRegistry,
}

impl CallbackContext {
    /// Create a new CallbackContext with the given registry
    #[must_use]
    pub fn new(registry: CallbackRegistry) -> Self {
        Self { registry }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, LazyLock};

    use async_lock::RwLock;
    use aura_app::ui::types::AppConfig;

    static UNREGISTERED_TEST_SIGNAL: LazyLock<Signal<u64>> =
        LazyLock::new(|| Signal::new("test:unregistered"));

    #[test]
    fn test_snapshot_state() {
        let state = ReactiveState::new(42);
        assert_eq!(snapshot_state(&state), 42);

        state.set(100);
        assert_eq!(snapshot_state(&state), 100);
    }

    #[test]
    fn test_snapshot_vec() {
        let vec = ReactiveVec::new();
        vec.push(1);
        vec.push(2);
        vec.push(3);

        let snapshot = snapshot_vec(&vec);
        assert_eq!(snapshot, vec![1, 2, 3]);
    }

    #[test]
    fn test_vec_helpers() {
        let vec: ReactiveVec<i32> = ReactiveVec::new();
        assert!(is_vec_empty(&vec));
        assert_eq!(vec_len(&vec), 0);

        vec.push(1);
        assert!(!is_vec_empty(&vec));
        assert_eq!(vec_len(&vec), 1);
    }

    #[test]
    fn test_chat_snapshot_default() {
        let snapshot = ChatSnapshot::default();

        assert!(snapshot.channels.is_empty());
        assert!(snapshot.selected_channel.is_none());
        assert!(snapshot.messages.is_empty());
    }

    #[test]
    fn test_snapshot_defaults() {
        // All snapshot types should have sensible defaults
        let chat = ChatSnapshot::default();
        assert!(chat.channels.is_empty());

        let guardians = GuardiansSnapshot::default();
        assert!(guardians.guardians.is_empty());

        let recovery = RecoverySnapshot::default();
        assert!(!recovery.is_in_progress);

        let invitations = InvitationsSnapshot::default();
        assert!(invitations.invitations.is_empty());

        let home_snapshot = HomeSnapshot::default();
        assert!(home_snapshot.home_state.is_none());

        let contacts = ContactsSnapshot::default();
        assert!(contacts.contacts.is_empty());

        let neighborhood = NeighborhoodSnapshot::default();
        assert!(neighborhood.homes.is_empty());
    }

    #[tokio::test]
    async fn subscribe_signal_with_retry_report_invokes_terminal_failure_for_unregistered_signal() {
        let app_core = Arc::new(RwLock::new(
            AppCore::new(AppConfig::default())
                .unwrap_or_else(|error| panic!("Failed to create test AppCore: {error}")),
        ));
        let app_core = InitializedAppCore::new(app_core)
            .await
            .unwrap_or_else(|error| panic!("Failed to init signals: {error}"));

        let health = Arc::new(std::sync::Mutex::new(Vec::new()));
        let reported = health.clone();
        subscribe_signal_with_retry_report(
            app_core,
            &UNREGISTERED_TEST_SIGNAL,
            |_| {},
            move |state| {
                reported
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(state);
            },
        )
        .await;

        let health = health
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(matches!(
            health.first(),
            Some(SubscriptionHealth::Retrying(ReactiveError::SignalNotFound { id }))
                if id == "test:unregistered"
        ));
        assert!(
            matches!(
                health.last(),
                Some(SubscriptionHealth::Degraded(RetryRunError::AttemptsExhausted {
                    last_error: ReactiveError::SignalNotFound { id },
                    ..
                }))
                    if id == "test:unregistered"
            ) || matches!(
                health.last(),
                Some(SubscriptionHealth::Degraded(RetryRunError::Timeout(_)))
            )
        );
    }
}
