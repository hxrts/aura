//! Modal layout trait for TUI modals.
//!
//! All modals implement this trait to produce content that fits
//! within the fixed middle region (80×25), overlaying the screen content.

use super::content::ModalContent;
use super::dim;
use super::screen_trait::ScreenContext;

/// Context passed to modal rendering
#[derive(Clone, Debug, Default)]
pub struct ModalContext {
    /// Screen context for the underlying screen
    pub screen: ScreenContext,
    /// Whether the modal is currently focused
    pub is_focused: bool,
    /// Modal width (same as middle region)
    pub width: u16,
    /// Modal height (same as middle region)
    pub height: u16,
}

impl ModalContext {
    /// Create a new modal context with standard dimensions
    pub fn new() -> Self {
        Self {
            screen: ScreenContext::unfocused(),
            is_focused: true,
            width: dim::TOTAL_WIDTH,
            height: dim::MIDDLE_HEIGHT,
        }
    }
}

/// Trait for modal content.
///
/// Modals overlay the middle region exactly (80 × 25).
pub trait ModalLayout {
    /// Render the modal content.
    ///
    /// Return type guarantees content fits within 80 × 25.
    fn render(&self, ctx: &ModalContext) -> ModalContent;

    /// Whether this modal can be dismissed with Esc
    fn is_dismissable(&self) -> bool {
        true
    }

    /// Title shown at the top of the modal
    fn title(&self) -> &str;

    /// Key hints specific to this modal
    fn key_hints(&self) -> Vec<super::screen_trait::KeyHint> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modal_context_dimensions() {
        let ctx = ModalContext::new();
        assert_eq!(ctx.width, 80);
        assert_eq!(ctx.height, 25);
        assert!(ctx.is_focused);
        assert!(!ctx.screen.is_focused);
    }
}
