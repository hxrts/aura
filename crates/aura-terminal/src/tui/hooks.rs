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
//! iocraft's `use_future` hook enables true push-based reactive updates by
//! spawning async tasks that subscribe to `ReactiveEffects` signals. When a
//! signal emits a new value, the task updates iocraft's `State<T>`, which
//! triggers a re-render.
//!
//! ```ignore
//! use iocraft::prelude::*;
//! use aura_app::ui::signals::CHAT_SIGNAL;
//! use aura_core::effects::reactive::ReactiveEffects;
//!
//! #[component]
//! fn ChatScreen(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
//!     // Get AppCore from context
//!     let ctx = hooks.use_context::<AppCoreContext>();
//!
//!     // Initialize state from current value
//!     let chat_state = hooks.use_state(|| Default::default());
//!
//!     // Subscribe to signal updates via use_future
//!     hooks.use_future({
//!         let mut chat_state = chat_state.clone();
//!         let app_core = ctx.app_core.clone();
//!         async move {
//!             // Get subscription via ReactiveEffects
//!             let mut stream = {
//!                 let core = app_core.raw().read().await;
//!                 core.subscribe(&*CHAT_SIGNAL)
//!             };
//!
//!             // Process updates until component unmounts
//!             while let Ok(new_value) = stream.recv().await {
//!                 chat_state.set(new_value);
//!             }
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

use crate::error::TerminalResult;
use crate::tui::context::{InitializedAppCore, IoContext};
use crate::tui::tasks::UiTaskOwner;

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
/// use crate::tui::hooks::AppCoreContext;
/// use aura_app::ui::signals::CHAT_SIGNAL;
/// use aura_core::effects::reactive::ReactiveEffects;
///
/// #[component]
/// fn MyComponent(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
///     let ctx = hooks.use_context::<AppCoreContext>();
///
///     // Initialize state from current value
///     let messages = hooks.use_state(|| Vec::new());
///
///     // Subscribe to signal updates via ReactiveEffects
///     hooks.use_future({
///         let mut messages = messages.clone();
///         let app_core = ctx.app_core.clone();
///         async move {
///             let mut stream = {
///                 let core = app_core.raw().read().await;
///                 core.subscribe(&*CHAT_SIGNAL)
///             };
///             while let Ok(state) = stream.recv().await {
///                 messages.set(state.messages.clone());
///             }
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
}

impl AppCoreContext {
    /// Create a new AppCoreContext
    #[must_use]
    pub fn new(app_core: InitializedAppCore, io_context: Arc<IoContext>) -> Self {
        Self {
            app_core,
            io_context,
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
/// - subscription failures emit `ERROR_SIGNAL` (best-effort), and
/// - subscriptions retry with backoff instead of terminating permanently.
///
/// **Behavior**:
/// - Reads the current value first (catch-up).
/// - Subscribes and forwards values to `on_value`.
/// - On any error, emits `ERROR_SIGNAL` and retries.
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
    app_core: InitializedAppCore,
    signal: &'static Signal<T>,
    on_value: F,
) where
    T: Clone + Send + Sync + 'static,
    F: FnMut(T) + Send + 'static,
{
    subscribe_signal_with_retry_report(app_core, signal, on_value, |_| {}).await;
}

pub async fn subscribe_signal_with_retry_report<T, F, G>(
    app_core: InitializedAppCore,
    signal: &'static Signal<T>,
    on_value: F,
    on_terminal_failure: G,
) where
    T: Clone + Send + Sync + 'static,
    F: FnMut(T) + Send + 'static,
    G: Fn(String) + Send + 'static,
{
    let reactive: ReactiveHandler = {
        let core = app_core.raw().read().await;
        core.reactive().clone()
    };

    let last_emitted = Arc::new(Mutex::new(None::<String>));
    let on_value = Arc::new(Mutex::new(on_value));
    let on_terminal_failure = Arc::new(on_terminal_failure);
    let time = PhysicalTimeHandler::new();
    let retry_policy = subscription_retry_policy();

    let result = execute_with_retry_budget(&time, &retry_policy, |_attempt| async {
        if !reactive.is_registered(signal.id()) {
            let message = format!("Reactive signal not registered: {}", signal.id());
            maybe_emit_reactive_error(&reactive, &last_emitted, message.clone()).await;
            return Err(message);
        }

        match reactive.read(signal).await {
            Ok(value) => {
                let mut on_value = on_value.lock().await;
                (*on_value)(value);
            }
            Err(e) => {
                let message = format!(
                    "Reactive read failed ({}): {}",
                    signal.id(),
                    format_reactive_error(&e)
                );
                maybe_emit_reactive_error(&reactive, &last_emitted, message.clone()).await;
                return Err(message);
            }
        }

        let mut stream = reactive
            .subscribe(signal)
            .map_err(|error| format_reactive_error(&error))?;
        loop {
            match stream.recv().await {
                Ok(value) => {
                    let mut on_value = on_value.lock().await;
                    (*on_value)(value);
                }
                Err(e) => {
                    let message = format!(
                        "Reactive subscription failed ({}): {}",
                        signal.id(),
                        format_reactive_error(&e)
                    );
                    maybe_emit_reactive_error(&reactive, &last_emitted, message.clone()).await;
                    return Err(message);
                }
            }
        }
    })
    .await;

    match result {
        Ok(()) => {}
        Err(RetryRunError::AttemptsExhausted {
            attempts_used,
            last_error,
        }) => {
            (*on_terminal_failure)(format!(
                "attempts exhausted after {attempts_used} retries: {last_error}"
            ));
            tracing::warn!(
                signal = %signal.id(),
                attempts_used,
                last_error,
                "Signal subscription abandoned after max retries"
            );
        }
        Err(RetryRunError::Timeout(error)) => {
            (*on_terminal_failure)(format!("retry budget handling timed out: {error}"));
            tracing::warn!(
                signal = %signal.id(),
                error = %error,
                "Signal subscription abandoned because retry budget handling timed out"
            );
        }
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

fn format_reactive_error(err: &ReactiveError) -> String {
    match err {
        ReactiveError::SignalNotFound { id } => format!("signal not found: {id}"),
        ReactiveError::TypeMismatch {
            id,
            expected,
            actual,
        } => format!("type mismatch ({id}): expected {expected}, got {actual}"),
        ReactiveError::SubscriptionClosed { id } => format!("subscription closed: {id}"),
        ReactiveError::EmissionFailed { id, reason } => {
            format!("emission failed ({id}): {reason}")
        }
        ReactiveError::CycleDetected { path } => format!("cycle detected: {path}"),
        ReactiveError::HandlerUnavailable => "handler unavailable".to_string(),
        ReactiveError::Internal { reason } => format!("internal error: {reason}"),
    }
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
    use async_lock::Mutex;
    use std::sync::{Arc, LazyLock};

    use async_lock::RwLock;
    use aura_app::ui::types::AppConfig;
    use tokio::sync::oneshot;

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

        let (tx, rx) = oneshot::channel();
        let sender = Arc::new(Mutex::new(Some(tx)));
        subscribe_signal_with_retry_report(
            app_core,
            &UNREGISTERED_TEST_SIGNAL,
            |_| {},
            move |reason| {
                if let Some(tx) = sender.lock_blocking().take() {
                    let _ = tx.send(reason);
                }
            },
        )
        .await;

        let reason = rx.await.unwrap_or_else(|error| {
            panic!("terminal failure callback should receive a reason: {error}")
        });
        assert!(
            reason.contains("attempts exhausted")
                || reason.contains("retry budget handling timed out"),
            "unexpected terminal failure reason: {reason}"
        );
        assert!(
            reason.contains("Reactive signal not registered: test:unregistered"),
            "terminal failure reason should preserve the signal registration error: {reason}"
        );
    }
}
