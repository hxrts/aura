//! Intent Effect Traits
//!
//! This module defines the effect trait for intent dispatch - the mechanism
//! by which user actions are processed through the system.
//!
//! # Effect Classification
//!
//! - **Category**: Application Effect
//! - **Implementation**: `aura-app` (Layer 6)
//! - **Usage**: All UI layers needing to dispatch user actions
//!
//! # Design
//!
//! The `IntentEffects` trait is generic over:
//! - `I`: The intent type (e.g., `aura_app::Intent`)
//! - `E`: The error type (e.g., `aura_app::IntentError`)
//!
//! This allows the trait to be defined at the core layer while the specific
//! intent variants are defined at the application layer where domain logic lives.
//!
//! # Flow
//!
//! ```text
//! Intent → Authorize (Biscuit) → Journal → Reduce → View → Sync
//!          └─────────────────────────────────────────────────┘
//!                        IntentEffects::dispatch()
//! ```
//!
//! When an intent is dispatched, the handler:
//! 1. Validates the intent
//! 2. Checks authorization (Biscuit tokens)
//! 3. Checks flow budget
//! 4. Creates a journal fact
//! 5. Runs the reducer to update state
//! 6. Notifies subscribers via reactive signals

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::sync::Arc;

// ─────────────────────────────────────────────────────────────────────────────
// Error Types
// ─────────────────────────────────────────────────────────────────────────────

/// Base error type for intent dispatch.
///
/// This provides common error variants that any intent system should support.
/// Concrete implementations can wrap this or define their own more specific errors.
#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize)]
pub enum IntentDispatchError {
    /// The intent was not authorized
    #[error("Unauthorized: {reason}")]
    Unauthorized { reason: String },

    /// The intent failed validation
    #[error("Validation failed: {reason}")]
    ValidationFailed { reason: String },

    /// Flow budget exceeded
    #[error("Flow budget exceeded: {reason}")]
    FlowBudgetExceeded { reason: String },

    /// Journal error during fact recording
    #[error("Journal error: {reason}")]
    JournalError { reason: String },

    /// Reactive system error during state update
    #[error("Reactive error: {reason}")]
    ReactiveError { reason: String },

    /// Internal error during dispatch
    #[error("Internal error: {reason}")]
    InternalError { reason: String },
}

impl IntentDispatchError {
    /// Create an unauthorized error
    pub fn unauthorized(reason: impl Into<String>) -> Self {
        Self::Unauthorized {
            reason: reason.into(),
        }
    }

    /// Create a validation error
    pub fn validation_failed(reason: impl Into<String>) -> Self {
        Self::ValidationFailed {
            reason: reason.into(),
        }
    }

    /// Create a flow budget error
    pub fn flow_budget_exceeded(reason: impl Into<String>) -> Self {
        Self::FlowBudgetExceeded {
            reason: reason.into(),
        }
    }

    /// Create a journal error
    pub fn journal_error(reason: impl Into<String>) -> Self {
        Self::JournalError {
            reason: reason.into(),
        }
    }

    /// Create a reactive error
    pub fn reactive_error(reason: impl Into<String>) -> Self {
        Self::ReactiveError {
            reason: reason.into(),
        }
    }

    /// Create an internal error
    pub fn internal_error(reason: impl Into<String>) -> Self {
        Self::InternalError {
            reason: reason.into(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Intent Metadata
// ─────────────────────────────────────────────────────────────────────────────

/// Metadata about an intent for authorization and auditing.
///
/// This trait allows the effect system to introspect intents without
/// knowing their concrete type.
pub trait IntentMetadata {
    /// Get a human-readable description of the intent
    fn description(&self) -> &str;

    /// Check if this intent should be recorded in the journal
    ///
    /// Pure queries (like navigation) typically shouldn't be journaled.
    fn should_journal(&self) -> bool;

    /// Get the authorization level required for this intent
    fn authorization_level(&self) -> AuthorizationLevel {
        AuthorizationLevel::Basic
    }
}

/// Authorization levels for intent dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum AuthorizationLevel {
    /// No authorization required (e.g., navigation)
    Public,
    /// Basic user authorization (e.g., sending messages)
    Basic,
    /// Elevated authorization for sensitive operations (e.g., recovery)
    Sensitive,
    /// Administrator-level authorization (e.g., banning users)
    Admin,
}

impl AuthorizationLevel {
    /// Get a human-readable description
    pub fn description(&self) -> &'static str {
        match self {
            Self::Public => "public access",
            Self::Basic => "basic user access",
            Self::Sensitive => "sensitive operations",
            Self::Admin => "administrator privileges",
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Intent Effects Trait
// ─────────────────────────────────────────────────────────────────────────────

/// Effect trait for dispatching intents.
///
/// This trait defines the interface for processing user actions through
/// the system. Implementations compose authorization, journaling, state
/// updates, and reactive notifications.
///
/// # Type Parameters
///
/// - `I`: The intent type (must implement `IntentMetadata`)
/// - `E`: The error type (must be convertible from `IntentDispatchError`)
///
/// # Example
///
/// ```ignore
/// // Dispatch an intent
/// let result = effects.dispatch(Intent::SendMessage {
///     channel_id: channel,
///     content: "Hello!".to_string(),
///     reply_to: None,
/// }).await;
///
/// // Dispatch with explicit error handling
/// match effects.dispatch(intent).await {
///     Ok(()) => println!("Intent processed"),
///     Err(e) => eprintln!("Failed: {}", e),
/// }
/// ```
#[async_trait]
pub trait IntentEffects<I, E>: Send + Sync
where
    I: IntentMetadata + Send + Sync + 'static,
    E: From<IntentDispatchError> + Send + 'static,
{
    /// Dispatch an intent for processing.
    ///
    /// This method composes multiple effects:
    /// 1. Authorization check
    /// 2. Flow budget check
    /// 3. Journal fact creation (if `should_journal()`)
    /// 4. State reduction
    /// 5. Reactive signal emission
    ///
    /// # Errors
    ///
    /// Returns an error if any step in the dispatch pipeline fails.
    async fn dispatch(&self, intent: I) -> Result<(), E>;

    /// Dispatch an intent and wait for sync confirmation.
    ///
    /// Like `dispatch()`, but also waits for the change to be synced
    /// to other devices/participants (if applicable).
    ///
    /// # Errors
    ///
    /// Returns an error if dispatch or sync fails.
    async fn dispatch_and_sync(&self, intent: I) -> Result<(), E> {
        // Default implementation just dispatches without sync
        self.dispatch(intent).await
    }

    /// Check if an intent would be authorized without dispatching.
    ///
    /// This is useful for UI hints (e.g., graying out unauthorized actions).
    async fn can_dispatch(&self, intent: &I) -> bool;
}

/// Simplified intent effects trait with a fixed error type.
///
/// This is a convenience trait for handlers that use `IntentDispatchError` directly.
#[async_trait]
pub trait SimpleIntentEffects<I>: IntentEffects<I, IntentDispatchError>
where
    I: IntentMetadata + Send + Sync + 'static,
{
}

// Blanket implementation
impl<T, I> SimpleIntentEffects<I> for T
where
    T: IntentEffects<I, IntentDispatchError>,
    I: IntentMetadata + Send + Sync + 'static,
{
}

// ─────────────────────────────────────────────────────────────────────────────
// Blanket Implementations
// ─────────────────────────────────────────────────────────────────────────────

/// Blanket implementation for Arc<T> where T: IntentEffects
#[async_trait]
impl<T, I, E> IntentEffects<I, E> for Arc<T>
where
    T: IntentEffects<I, E> + ?Sized,
    I: IntentMetadata + Send + Sync + 'static,
    E: From<IntentDispatchError> + Send + 'static,
{
    async fn dispatch(&self, intent: I) -> Result<(), E> {
        (**self).dispatch(intent).await
    }

    async fn dispatch_and_sync(&self, intent: I) -> Result<(), E> {
        (**self).dispatch_and_sync(intent).await
    }

    async fn can_dispatch(&self, intent: &I) -> bool {
        (**self).can_dispatch(intent).await
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_authorization_level_ordering() {
        assert!(AuthorizationLevel::Public < AuthorizationLevel::Basic);
        assert!(AuthorizationLevel::Basic < AuthorizationLevel::Sensitive);
        assert!(AuthorizationLevel::Sensitive < AuthorizationLevel::Admin);
    }

    #[test]
    fn test_intent_dispatch_error_display() {
        let err = IntentDispatchError::unauthorized("missing token");
        assert!(err.to_string().contains("missing token"));

        let err = IntentDispatchError::flow_budget_exceeded("rate limited");
        assert!(err.to_string().contains("rate limited"));
    }

    #[test]
    fn test_authorization_level_description() {
        assert_eq!(AuthorizationLevel::Public.description(), "public access");
        assert_eq!(
            AuthorizationLevel::Admin.description(),
            "administrator privileges"
        );
    }
}
