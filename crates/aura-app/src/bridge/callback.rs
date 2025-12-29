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
    HomesState, ChatState, ContactsState, InvitationsState, NeighborhoodState, RecoveryState,
};
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

    /// Notify all observers of chat state change
    pub fn notify_chat(&self, state: &ChatState) {
        for (_, observer) in &self.observers {
            observer.on_chat_changed(state.clone());
        }
    }

    /// Notify all observers of recovery state change
    pub fn notify_recovery(&self, state: &RecoveryState) {
        for (_, observer) in &self.observers {
            observer.on_recovery_changed(state.clone());
        }
    }

    /// Notify all observers of invitations state change
    pub fn notify_invitations(&self, state: &InvitationsState) {
        for (_, observer) in &self.observers {
            observer.on_invitations_changed(state.clone());
        }
    }

    /// Notify all observers of contacts state change
    pub fn notify_contacts(&self, state: &ContactsState) {
        for (_, observer) in &self.observers {
            observer.on_contacts_changed(state.clone());
        }
    }

    /// Notify all observers of homes state change
    pub fn notify_homes(&self, state: &HomesState) {
        for (_, observer) in &self.observers {
            observer.on_homes_changed(state.clone());
        }
    }

    /// Notify all observers of neighborhood state change
    pub fn notify_neighborhood(&self, state: &NeighborhoodState) {
        for (_, observer) in &self.observers {
            observer.on_neighborhood_changed(state.clone());
        }
    }

    /// Notify all observers of an error
    pub fn notify_error(&self, error: CallbackError) {
        for (_, observer) in &self.observers {
            observer.on_error(error.clone());
        }
    }
}
