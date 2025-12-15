//! Toast layout trait for TUI toasts.
//!
//! Toasts implement this trait to produce content that fits
//! within the fixed footer region (80×3), overlaying the key hints.

use super::content::ToastContent;
use super::dim;

/// Toast severity level
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToastLevel {
    /// Informational message
    #[default]
    Info,
    /// Success message
    Success,
    /// Warning message
    Warning,
    /// Error message
    Error,
}

/// Context passed to toast rendering
#[derive(Clone, Debug, Default)]
pub struct ToastContext {
    /// Toast level for styling
    pub level: ToastLevel,
    /// Toast width (same as footer region)
    pub width: u16,
    /// Toast height (same as footer region)
    pub height: u16,
}

impl ToastContext {
    /// Create a new toast context with standard dimensions
    pub fn new(level: ToastLevel) -> Self {
        Self {
            level,
            width: dim::TOTAL_WIDTH,
            height: dim::FOOTER_HEIGHT,
        }
    }

    /// Create an info toast context
    pub fn info() -> Self {
        Self::new(ToastLevel::Info)
    }

    /// Create a success toast context
    pub fn success() -> Self {
        Self::new(ToastLevel::Success)
    }

    /// Create a warning toast context
    pub fn warning() -> Self {
        Self::new(ToastLevel::Warning)
    }

    /// Create an error toast context
    pub fn error() -> Self {
        Self::new(ToastLevel::Error)
    }
}

/// Trait for toast content.
///
/// Toasts overlay the footer region exactly (80 × 3).
pub trait ToastLayout {
    /// Render the toast content.
    ///
    /// Return type guarantees content fits within 80 × 3.
    fn render(&self, ctx: &ToastContext) -> ToastContent;

    /// Whether this toast can be dismissed with Esc
    fn is_dismissable(&self) -> bool {
        true
    }

    /// Auto-dismiss timeout in milliseconds (None = no auto-dismiss)
    fn auto_dismiss_ms(&self) -> Option<u64> {
        Some(3000)
    }

    /// Toast severity level
    fn level(&self) -> ToastLevel {
        ToastLevel::Info
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toast_context_dimensions() {
        let ctx = ToastContext::info();
        assert_eq!(ctx.width, 80);
        assert_eq!(ctx.height, 3);
        assert_eq!(ctx.level, ToastLevel::Info);
    }

    #[test]
    fn test_toast_levels() {
        assert_eq!(ToastContext::success().level, ToastLevel::Success);
        assert_eq!(ToastContext::warning().level, ToastLevel::Warning);
        assert_eq!(ToastContext::error().level, ToastLevel::Error);
    }
}
