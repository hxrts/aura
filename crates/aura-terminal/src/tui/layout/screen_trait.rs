//! Screen layout trait for TUI screens.
//!
//! All TUI screens implement this trait to produce content
//! that fits within the fixed middle region (80×25).

use super::content::MiddleContent;
use super::dim;

/// A key hint displayed in the footer
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyHint {
    /// Key binding (e.g., "Esc", "Enter", "Tab")
    pub key: &'static str,
    /// Action description (e.g., "back", "select", "next")
    pub action: &'static str,
}

impl KeyHint {
    /// Create a new key hint
    #[must_use]
    pub const fn new(key: &'static str, action: &'static str) -> Self {
        Self { key, action }
    }
}

/// Context passed to screen rendering
#[derive(Clone, Debug, Default)]
pub struct ScreenContext {
    /// Whether the screen is currently focused
    pub is_focused: bool,
    /// Terminal width (should be TOTAL_WIDTH)
    pub width: u16,
    /// Available height for content (should be MIDDLE_HEIGHT)
    pub height: u16,
}

impl ScreenContext {
    /// Create a new screen context with standard dimensions
    #[must_use]
    pub fn new() -> Self {
        Self {
            is_focused: true,
            width: dim::TOTAL_WIDTH,
            height: dim::MIDDLE_HEIGHT,
        }
    }

    /// Create context for unfocused screen
    #[must_use]
    pub fn unfocused() -> Self {
        Self {
            is_focused: false,
            width: dim::TOTAL_WIDTH,
            height: dim::MIDDLE_HEIGHT,
        }
    }
}

/// Trait for all TUI screens.
///
/// Screens only produce middle content - nav and footer are handled by the compositor.
/// The middle region is exactly 80 columns × 25 rows.
pub trait ScreenLayout {
    /// Render content for the middle region.
    ///
    /// Return type guarantees content fits within 80 × 25.
    fn render_middle(&self, ctx: &ScreenContext) -> MiddleContent;

    /// Navigation bar title for this screen
    fn nav_title(&self) -> &'static str;

    /// Key hints for the footer (screen-specific).
    ///
    /// These are rendered in the 2-row key hints area within the footer.
    fn key_hints(&self) -> Vec<KeyHint>;

    /// Whether this screen can be navigated away from with Esc
    fn is_escapable(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_hint_creation() {
        let hint = KeyHint::new("Esc", "back");
        assert_eq!(hint.key, "Esc");
        assert_eq!(hint.action, "back");
    }

    #[test]
    fn test_screen_context_default() {
        let ctx = ScreenContext::new();
        assert!(ctx.is_focused);
        assert_eq!(ctx.width, dim::TOTAL_WIDTH);
        assert_eq!(ctx.height, dim::MIDDLE_HEIGHT);
    }

    #[test]
    fn test_screen_context_unfocused() {
        let ctx = ScreenContext::unfocused();
        assert!(!ctx.is_focused);
        assert_eq!(ctx.width, dim::TOTAL_WIDTH);
        assert_eq!(ctx.height, dim::MIDDLE_HEIGHT);
    }
}
