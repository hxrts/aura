//! # Toast Notification Component
//!
//! Ephemeral notifications that appear briefly and auto-dismiss.
//! Supports multiple severity levels and stacking.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crossterm::event::KeyEvent;
use ratatui::{
    layout::{Alignment, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use super::{Component, InputAction, Styles};
use crate::tui::styles::ToastLevel;

/// Unique identifier for a toast
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ToastId(u64);

impl ToastId {
    /// Create a new toast ID
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

/// A toast notification
#[derive(Debug, Clone)]
pub struct Toast {
    /// Unique identifier
    pub id: ToastId,
    /// Message content
    pub message: String,
    /// Severity level
    pub level: ToastLevel,
    /// When the toast was created
    pub created_at: Instant,
    /// How long to display (None = until dismissed)
    pub duration: Option<Duration>,
    /// Whether the toast can be dismissed by user
    pub dismissable: bool,
}

impl Toast {
    /// Create a new toast notification
    pub fn new(message: impl Into<String>, level: ToastLevel) -> Self {
        Self {
            id: ToastId::new(),
            message: message.into(),
            level,
            created_at: Instant::now(),
            duration: Some(Duration::from_secs(5)),
            dismissable: true,
        }
    }

    /// Create an info toast
    pub fn info(message: impl Into<String>) -> Self {
        Self::new(message, ToastLevel::Info)
    }

    /// Create a success toast
    pub fn success(message: impl Into<String>) -> Self {
        Self::new(message, ToastLevel::Success)
    }

    /// Create a warning toast
    pub fn warning(message: impl Into<String>) -> Self {
        Self::new(message, ToastLevel::Warning)
    }

    /// Create an error toast
    pub fn error(message: impl Into<String>) -> Self {
        Self::new(message, ToastLevel::Error)
    }

    /// Set the duration
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }

    /// Make the toast persist until dismissed
    pub fn persistent(mut self) -> Self {
        self.duration = None;
        self
    }

    /// Make the toast non-dismissable
    pub fn non_dismissable(mut self) -> Self {
        self.dismissable = false;
        self
    }

    /// Check if the toast has expired
    pub fn is_expired(&self) -> bool {
        match self.duration {
            Some(d) => self.created_at.elapsed() >= d,
            None => false,
        }
    }

    /// Get the icon for this toast level
    fn icon(&self) -> &'static str {
        match self.level {
            ToastLevel::Info => "i",
            ToastLevel::Success => "*",
            ToastLevel::Warning => "!",
            ToastLevel::Error => "x",
        }
    }
}

/// Manager for multiple toast notifications
#[derive(Debug, Default)]
pub struct ToastManager {
    /// Queue of active toasts
    toasts: VecDeque<Toast>,
    /// Maximum number of visible toasts
    max_visible: usize,
    /// Whether the toast area is focused (for dismissal)
    focused: bool,
}

impl ToastManager {
    /// Create a new toast manager
    pub fn new() -> Self {
        Self {
            toasts: VecDeque::new(),
            max_visible: 5,
            focused: false,
        }
    }

    /// Set maximum visible toasts
    pub fn with_max_visible(mut self, max: usize) -> Self {
        self.max_visible = max;
        self
    }

    /// Add a new toast
    pub fn push(&mut self, toast: Toast) {
        self.toasts.push_back(toast);
        // Trim old toasts if we exceed the limit
        while self.toasts.len() > self.max_visible * 2 {
            self.toasts.pop_front();
        }
    }

    /// Add an info toast
    pub fn info(&mut self, message: impl Into<String>) {
        self.push(Toast::info(message));
    }

    /// Add a success toast
    pub fn success(&mut self, message: impl Into<String>) {
        self.push(Toast::success(message));
    }

    /// Add a warning toast
    pub fn warning(&mut self, message: impl Into<String>) {
        self.push(Toast::warning(message));
    }

    /// Add an error toast
    pub fn error(&mut self, message: impl Into<String>) {
        self.push(Toast::error(message));
    }

    /// Dismiss a toast by ID
    pub fn dismiss(&mut self, id: ToastId) {
        self.toasts.retain(|t| t.id != id);
    }

    /// Dismiss the oldest toast
    pub fn dismiss_oldest(&mut self) {
        if let Some(front) = self.toasts.front() {
            if front.dismissable {
                self.toasts.pop_front();
            }
        }
    }

    /// Remove expired toasts
    pub fn cleanup(&mut self) {
        self.toasts.retain(|t| !t.is_expired());
    }

    /// Get visible toasts
    pub fn visible(&self) -> impl Iterator<Item = &Toast> {
        self.toasts.iter().rev().take(self.max_visible)
    }

    /// Check if there are any toasts
    pub fn is_empty(&self) -> bool {
        self.toasts.is_empty()
    }

    /// Get the number of active toasts
    pub fn len(&self) -> usize {
        self.toasts.len()
    }
}

impl Component for ToastManager {
    fn handle_key(&mut self, key: KeyEvent) -> Option<InputAction> {
        use crossterm::event::KeyCode;

        if !self.focused || self.is_empty() {
            return None;
        }

        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                self.dismiss_oldest();
                Some(InputAction::None)
            }
            _ => None,
        }
    }

    fn render(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        // Clean up expired toasts would need mutable access
        // In practice, call cleanup() before render()

        if self.is_empty() {
            return;
        }

        // Calculate toast area (bottom-right corner)
        let toast_width = area.width.min(50);
        let toast_height = 3;
        let gap = 1;

        let mut y_offset = area.height;

        for toast in self.visible() {
            if y_offset < toast_height + gap {
                break;
            }
            y_offset -= toast_height + gap;

            let toast_area = Rect {
                x: area.x + area.width - toast_width,
                y: area.y + y_offset,
                width: toast_width,
                height: toast_height,
            };

            // Clear background
            f.render_widget(Clear, toast_area);

            // Render toast
            let style = styles.toast(toast.level);
            let icon = toast.icon();
            let dismiss_hint = if toast.dismissable { " [Esc]" } else { "" };

            let content = vec![Line::from(vec![
                Span::styled(format!("[{}] ", icon), style),
                Span::raw(&toast.message),
                Span::styled(dismiss_hint, styles.text_muted()),
            ])];

            let border_style = if self.focused {
                styles.border_focused()
            } else {
                styles.border()
            };

            let paragraph = Paragraph::new(content)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(border_style),
                )
                .alignment(Alignment::Left)
                .wrap(Wrap { trim: true });

            f.render_widget(paragraph, toast_area);
        }
    }

    fn is_focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn min_size(&self) -> (u16, u16) {
        (30, 3)
    }

    fn is_visible(&self) -> bool {
        !self.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toast_creation() {
        let toast = Toast::info("Hello");
        assert_eq!(toast.message, "Hello");
        assert_eq!(toast.level, ToastLevel::Info);
        assert!(toast.dismissable);
    }

    #[test]
    fn test_toast_builders() {
        let info = Toast::info("Info");
        assert_eq!(info.level, ToastLevel::Info);

        let success = Toast::success("Success");
        assert_eq!(success.level, ToastLevel::Success);

        let warning = Toast::warning("Warning");
        assert_eq!(warning.level, ToastLevel::Warning);

        let error = Toast::error("Error");
        assert_eq!(error.level, ToastLevel::Error);
    }

    #[test]
    fn test_toast_persistent() {
        let toast = Toast::info("Test").persistent();
        assert!(toast.duration.is_none());
        assert!(!toast.is_expired());
    }

    #[test]
    fn test_toast_manager() {
        let mut manager = ToastManager::new();
        assert!(manager.is_empty());

        manager.info("First");
        manager.warning("Second");
        assert_eq!(manager.len(), 2);
        assert!(!manager.is_empty());

        manager.dismiss_oldest();
        assert_eq!(manager.len(), 1);
    }

    #[test]
    fn test_toast_manager_visible() {
        let mut manager = ToastManager::new().with_max_visible(2);

        manager.info("1");
        manager.info("2");
        manager.info("3");

        let visible: Vec<_> = manager.visible().collect();
        assert_eq!(visible.len(), 2);
    }
}
