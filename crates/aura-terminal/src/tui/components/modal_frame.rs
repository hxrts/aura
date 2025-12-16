//! # Modal Frame
//!
//! Unified wrapper component for all modal dialogs.
//!
//! ALL modals MUST use this frame to ensure consistent positioning.
//! The frame positions the modal to exactly fill the middle panel region (80×25).
//!
//! ## Usage
//!
//! Modals should be rendered at ROOT level in app.rs, wrapped in ModalFrame:
//!
//! ```ignore
//! #(if modal_visible {
//!     Some(element! {
//!         ModalFrame {
//!             MyModalContent(...)
//!         }
//!     })
//! } else {
//!     None
//! })
//! ```
//!
//! ## Important
//!
//! - ALL modals must be rendered at root level in app.rs
//! - Do NOT render modals inside screen components
//! - This ensures consistent positioning across all modals

use iocraft::prelude::*;

use crate::tui::layout::dim;

/// Props for ModalFrame
#[derive(Default, Props)]
pub struct ModalFrameProps<'a> {
    /// Child content (the modal body)
    pub children: Vec<AnyElement<'a>>,
}

/// Unified modal frame that positions content in the middle panel region.
///
/// This component handles the absolute positioning so that all modals
/// appear at exactly the same location: filling the middle panel (rows 3-27).
///
/// Use this as the outermost wrapper for ALL modal components.
#[component]
pub fn ModalFrame<'a>(props: &mut ModalFrameProps<'a>) -> impl Into<AnyElement<'a>> {
    element! {
        View(
            position: Position::Absolute,
            top: dim::NAV_HEIGHT,
            left: 0u16,
            width: dim::TOTAL_WIDTH,
            height: dim::MIDDLE_HEIGHT,
            overflow: Overflow::Hidden,
        ) {
            #(&mut props.children)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::layout::dim;

    #[test]
    fn test_modal_frame_dimensions() {
        // Modal frame should position at nav height and fill middle region
        assert_eq!(dim::NAV_HEIGHT, 3);
        assert_eq!(dim::TOTAL_WIDTH, 80);
        assert_eq!(dim::MIDDLE_HEIGHT, 25);
    }
}
