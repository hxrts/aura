//! # Toast Notification Helper
//!
//! Manages toast notifications for the TUI.
//!
//! Note: The fullscreen iocraft shell renders user-visible toasts via the
//! type-enforced `ToastQueue` in `tui/state_machine.rs`. `ToastHelper` is kept
//! for non-shell contexts (e.g., tests, helper APIs) and as a staging area where
//! needed.

use std::sync::Arc;

use async_lock::RwLock;

use crate::tui::components::ToastMessage;

/// Helper for managing toast notifications
#[derive(Clone)]
pub struct ToastHelper {
    /// Toast notifications for displaying errors/info in the UI
    toasts: Arc<RwLock<Vec<ToastMessage>>>,
}

impl ToastHelper {
    /// Create a new toast helper
    #[must_use]
    pub fn new() -> Self {
        Self {
            toasts: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Add a toast notification
    pub async fn add(&self, toast: ToastMessage) {
        // Limit toasts to prevent unbounded growth
        const MAX_TOASTS: usize = 5;

        let mut toasts = self.toasts.write().await;
        toasts.push(toast);

        // Keep only the most recent MAX_TOASTS
        let len = toasts.len();
        if len > MAX_TOASTS {
            toasts.drain(0..(len - MAX_TOASTS));
        }
    }

    /// Add an error toast
    pub async fn error(&self, id: impl Into<String>, message: impl Into<String>) {
        self.add(ToastMessage::error(id, message)).await;
    }

    /// Add a success toast
    pub async fn success(&self, id: impl Into<String>, message: impl Into<String>) {
        self.add(ToastMessage::success(id, message)).await;
    }

    /// Add an info toast
    pub async fn info(&self, id: impl Into<String>, message: impl Into<String>) {
        self.add(ToastMessage::info(id, message)).await;
    }

    /// Get all current toasts
    pub async fn get_all(&self) -> Vec<ToastMessage> {
        self.toasts.read().await.clone()
    }

    /// Clear a specific toast by ID
    pub async fn clear(&self, id: &str) {
        self.toasts.write().await.retain(|t| t.id != id);
    }

    /// Clear all toasts
    pub async fn clear_all(&self) {
        self.toasts.write().await.clear();
    }
}

impl Default for ToastHelper {
    fn default() -> Self {
        Self::new()
    }
}
