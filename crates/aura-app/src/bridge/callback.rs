//! # Callback-based Reactive Bridge
//!
//! This module provides a callback-based API for platforms that don't
//! support Rust signals natively (iOS via UniFFI, Android via UniFFI).
//!
//! ## Usage (Swift)
//!
//! ```swift
//! class MyObserver: StateObserver {
//!     func onChatChanged(state: ChatState) {
//!         // Update UI
//!     }
//!     // ... other callbacks
//! }
//!
//! app.subscribe(observer: MyObserver())
//! ```

use crate::core::IntentError;
use crate::errors::AppError;
use crate::views::{
    ChatState, ContactsState, HomesState, InvitationsState, NeighborhoodState, RecoveryState,
};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;

/// Observer trait for receiving state updates.
///
/// Implement this trait in Swift/Kotlin to receive state change notifications.
/// The callbacks are called on a background thread - dispatch to main thread
/// for UI updates.
#[cfg_attr(feature = "uniffi", uniffi::export(callback_interface))]
pub trait StateObserver: Send + Sync {
    /// Called when chat state changes
    fn on_chat_changed(&self, state: ChatState);

    /// Called when recovery state changes
    fn on_recovery_changed(&self, state: RecoveryState);

    /// Called when invitations state changes
    fn on_invitations_changed(&self, state: InvitationsState);

    /// Called when contacts state changes
    fn on_contacts_changed(&self, state: ContactsState);

    /// Called when homes state changes
    fn on_homes_changed(&self, state: HomesState);

    /// Called when neighborhood state changes
    fn on_neighborhood_changed(&self, state: NeighborhoodState);

    /// Called when an error occurs
    fn on_error(&self, error: CallbackError);
}

/// Error type for callback notifications (FFI-compatible)
///
/// This is a simplified error struct for UniFFI bindings to mobile platforms.
/// It provides a flat structure that can be easily represented in Swift/Kotlin.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct CallbackError {
    /// Error code
    pub code: String,
    /// Human-readable message
    pub message: String,
    /// Whether this error is recoverable
    pub recoverable: bool,
}

impl From<IntentError> for CallbackError {
    fn from(err: IntentError) -> Self {
        Self {
            code: match &err {
                IntentError::Unauthorized { .. } => "unauthorized",
                IntentError::ValidationFailed { .. } => "validation_failed",
                IntentError::JournalError { .. } => "journal_error",
                IntentError::InternalError { .. } => "internal_error",
                IntentError::ContextNotFound { .. } => "context_not_found",
                IntentError::NetworkError { .. } => "network_error",
                IntentError::StorageError { .. } => "storage_error",
                IntentError::NoAgent { .. } => "no_agent",
                IntentError::ServiceError { .. } => "service_error",
            }
            .to_string(),
            message: err.to_string(),
            recoverable: matches!(
                &err,
                IntentError::NetworkError { .. }
                    | IntentError::ValidationFailed { .. }
                    | IntentError::ServiceError { .. }
            ),
        }
    }
}

impl From<AppError> for CallbackError {
    fn from(err: AppError) -> Self {
        Self {
            code: err.code().to_string(),
            message: err.to_string(),
            recoverable: err.is_recoverable(),
        }
    }
}

/// Registry for managing observer subscriptions
#[derive(Default)]
pub struct ObserverRegistry {
    observers: Vec<(u64, Arc<dyn StateObserver>)>,
    next_id: u64,
}

impl ObserverRegistry {
    const MAX_OBSERVERS: usize = 64;
    /// Create a new registry
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an observer and return its subscription ID
    pub fn add(&mut self, observer: Arc<dyn StateObserver>) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        if self.observers.len() >= Self::MAX_OBSERVERS {
            self.observers.remove(0);
        }
        self.observers.push((id, observer));
        id
    }

    /// Remove an observer by ID
    pub fn remove(&mut self, id: u64) {
        self.observers.retain(|(obs_id, _)| *obs_id != id);
    }

    fn log_observer_panic(id: u64, callback: &str) {
        #[cfg(feature = "instrumented")]
        tracing::error!(
            observer_id = id,
            callback,
            "state observer panicked during notification"
        );
        #[cfg(not(feature = "instrumented"))]
        {
            let _ = (id, callback);
        }
    }

    fn notify_with<T, F>(&self, callback: &str, value: &T, mut notify: F)
    where
        T: Clone,
        F: FnMut(&dyn StateObserver, T),
    {
        for (id, observer) in &self.observers {
            let result = catch_unwind(AssertUnwindSafe(|| {
                notify(observer.as_ref(), value.clone())
            }));
            if result.is_err() {
                Self::log_observer_panic(*id, callback);
            }
        }
    }

    /// Notify all observers of chat state change.
    pub fn notify_chat(&self, state: &ChatState) {
        self.notify_with("on_chat_changed", state, |observer, state| {
            observer.on_chat_changed(state);
        });
    }

    /// Notify all observers of recovery state change
    pub fn notify_recovery(&self, state: &RecoveryState) {
        self.notify_with("on_recovery_changed", state, |observer, state| {
            observer.on_recovery_changed(state);
        });
    }

    /// Notify all observers of invitations state change
    pub fn notify_invitations(&self, state: &InvitationsState) {
        self.notify_with("on_invitations_changed", state, |observer, state| {
            observer.on_invitations_changed(state);
        });
    }

    /// Notify all observers of contacts state change
    pub fn notify_contacts(&self, state: &ContactsState) {
        self.notify_with("on_contacts_changed", state, |observer, state| {
            observer.on_contacts_changed(state);
        });
    }

    /// Notify all observers of homes state change
    pub fn notify_homes(&self, state: &HomesState) {
        self.notify_with("on_homes_changed", state, |observer, state| {
            observer.on_homes_changed(state);
        });
    }

    /// Notify all observers of neighborhood state change
    pub fn notify_neighborhood(&self, state: &NeighborhoodState) {
        self.notify_with("on_neighborhood_changed", state, |observer, state| {
            observer.on_neighborhood_changed(state);
        });
    }

    /// Notify all observers of an error
    pub fn notify_error(&self, error: CallbackError) {
        self.notify_with("on_error", &error, |observer, error| {
            observer.on_error(error);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    struct PanicObserver;

    impl StateObserver for PanicObserver {
        fn on_chat_changed(&self, _state: ChatState) {
            panic!("observer panic");
        }

        fn on_recovery_changed(&self, _state: RecoveryState) {}
        fn on_invitations_changed(&self, _state: InvitationsState) {}
        fn on_contacts_changed(&self, _state: ContactsState) {}
        fn on_homes_changed(&self, _state: HomesState) {}
        fn on_neighborhood_changed(&self, _state: NeighborhoodState) {}
        fn on_error(&self, _error: CallbackError) {}
    }

    struct RecordingObserver {
        notifications: Arc<Mutex<Vec<&'static str>>>,
    }

    impl StateObserver for RecordingObserver {
        fn on_chat_changed(&self, _state: ChatState) {
            self.notifications.lock().expect("lock").push("chat");
        }

        fn on_recovery_changed(&self, _state: RecoveryState) {}
        fn on_invitations_changed(&self, _state: InvitationsState) {}
        fn on_contacts_changed(&self, _state: ContactsState) {}
        fn on_homes_changed(&self, _state: HomesState) {}
        fn on_neighborhood_changed(&self, _state: NeighborhoodState) {}
        fn on_error(&self, _error: CallbackError) {}
    }

    #[test]
    fn panicking_observer_does_not_block_remaining_observers() {
        let mut registry = ObserverRegistry::new();
        let notifications = Arc::new(Mutex::new(Vec::new()));
        registry.add(Arc::new(PanicObserver));
        registry.add(Arc::new(RecordingObserver {
            notifications: Arc::clone(&notifications),
        }));

        registry.notify_chat(&ChatState::default());

        assert_eq!(notifications.lock().expect("lock").as_slice(), ["chat"]);
    }
}
