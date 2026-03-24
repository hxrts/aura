//! # Toast Notification Helper
//!
//! Manages toast notifications for the TUI.
//!
//! Note: The fullscreen iocraft shell renders user-visible toasts via the
//! type-enforced `ToastQueue` in `tui/state_machine.rs`. `ToastHelper` is kept
//! for non-shell contexts (e.g., tests, helper APIs) and as a staging area where
//! needed.

use std::sync::Arc;

use std::collections::VecDeque;

use async_lock::RwLock;

use crate::tui::components::ToastMessage;

const MAX_RENDER_TOASTS: usize = 5;

#[derive(Clone, Debug, Default)]
struct ToastRenderBuffer {
    messages: VecDeque<ToastMessage>,
}

impl ToastRenderBuffer {
    fn push(&mut self, toast: ToastMessage) {
        self.messages.push_back(toast);
        while self.messages.len() > MAX_RENDER_TOASTS {
            let _ = self.messages.pop_front();
        }
    }

    fn snapshot(&self) -> Vec<ToastMessage> {
        self.messages.iter().cloned().collect()
    }

    fn clear(&mut self, id: &str) {
        self.messages.retain(|toast| toast.id != id);
    }

    fn clear_all(&mut self) {
        self.messages.clear();
    }
}

/// Helper for managing toast notifications
#[derive(Clone)]
pub struct ToastHelper {
    /// Observed render payloads for non-shell contexts.
    ///
    /// Fullscreen shell queue ownership stays in `tui::state::ToastQueue`; this
    /// helper only stores bounded render payloads for helper and test surfaces.
    render_buffer: Arc<RwLock<ToastRenderBuffer>>,
}

impl ToastHelper {
    /// Create a new toast helper
    #[must_use]
    pub fn new() -> Self {
        Self {
            render_buffer: Arc::new(RwLock::new(ToastRenderBuffer::default())),
        }
    }

    /// Add a toast notification
    pub async fn add(&self, toast: ToastMessage) {
        self.render_buffer.write().await.push(toast);
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
        self.render_buffer.read().await.snapshot()
    }

    /// Clear a specific toast by ID
    pub async fn clear(&self, id: &str) {
        self.render_buffer.write().await.clear(id);
    }

    /// Clear all toasts
    pub async fn clear_all(&self) {
        self.render_buffer.write().await.clear_all();
    }
}

impl Default for ToastHelper {
    fn default() -> Self {
        Self::new()
    }
}
