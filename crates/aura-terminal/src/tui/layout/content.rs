//! Bounded content types that guarantee fit within specific regions.
//!
//! These types use phantom types and const generics to encode
//! maximum dimensions, enabling compile-time and runtime overflow detection.

use super::regions::{Footer, LayoutRegion, Middle, Nav};
use iocraft::prelude::*;
use std::marker::PhantomData;

/// Content guaranteed to fit within a specific region.
/// Const generics encode maximum dimensions.
pub struct BoundedContent<R: LayoutRegion> {
    element: AnyElement<'static>,
    /// Actual height used (for debugging/validation)
    height_used: u16,
    _region: PhantomData<R>,
}

impl<R: LayoutRegion> BoundedContent<R> {
    /// Create bounded content from an element.
    ///
    /// In debug builds, panics if the specified height exceeds region bounds.
    pub fn new(element: AnyElement<'static>, height_used: u16) -> Self {
        debug_assert!(
            height_used <= R::HEIGHT,
            "OVERFLOW in {} region: content uses {} rows but max is {}",
            R::NAME,
            height_used,
            R::HEIGHT
        );
        Self {
            element,
            height_used,
            _region: PhantomData,
        }
    }

    /// Create bounded content that fills the entire region.
    pub fn full(element: AnyElement<'static>) -> Self {
        Self {
            element,
            height_used: R::HEIGHT,
            _region: PhantomData,
        }
    }

    /// Maximum width for this content
    pub const fn max_width() -> u16 {
        R::WIDTH
    }

    /// Maximum height for this content
    pub const fn max_height() -> u16 {
        R::HEIGHT
    }

    /// Actual height used by this content
    pub fn height_used(&self) -> u16 {
        self.height_used
    }

    /// Remaining height available in this region
    pub fn remaining_height(&self) -> u16 {
        R::HEIGHT.saturating_sub(self.height_used)
    }

    /// Consume the bounded content and return the inner element
    pub fn into_element(self) -> AnyElement<'static> {
        self.element
    }

    /// Get a reference to the inner element
    pub fn element(&self) -> &AnyElement<'static> {
        &self.element
    }
}

// Allow conversion from BoundedContent to AnyElement
impl<R: LayoutRegion> From<BoundedContent<R>> for AnyElement<'static> {
    fn from(content: BoundedContent<R>) -> Self {
        content.element
    }
}

/// Type aliases for each region
pub type NavContent = BoundedContent<Nav>;
pub type MiddleContent = BoundedContent<Middle>;
pub type FooterContent = BoundedContent<Footer>;

/// Modal content is same size as middle (overlays the middle region)
pub type ModalContent = MiddleContent;

/// Toast content is same size as footer (overlays the footer region)
pub type ToastContent = FooterContent;

/// Empty content for a region (transparent/no rendering)
pub fn empty_content<R: LayoutRegion>() -> BoundedContent<R> {
    BoundedContent::new(
        element! {
            View(width: R::WIDTH, height: R::HEIGHT)
        }
        .into_any(),
        0,
    )
}

/// Create nav content from an element
pub fn nav_content(element: AnyElement<'static>, height: u16) -> NavContent {
    BoundedContent::new(element, height)
}

/// Create middle content from an element
pub fn middle_content(element: AnyElement<'static>, height: u16) -> MiddleContent {
    BoundedContent::new(element, height)
}

/// Create footer content from an element
pub fn footer_content(element: AnyElement<'static>, height: u16) -> FooterContent {
    BoundedContent::new(element, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_max_dimensions() {
        assert_eq!(NavContent::max_width(), 80);
        assert_eq!(NavContent::max_height(), 3);

        assert_eq!(MiddleContent::max_width(), 80);
        assert_eq!(MiddleContent::max_height(), 25);

        assert_eq!(FooterContent::max_width(), 80);
        assert_eq!(FooterContent::max_height(), 3);
    }

    #[test]
    fn test_height_tracking() {
        let element = element! { View(width: 80, height: 10) }.into_any();
        let content = MiddleContent::new(element, 10);

        assert_eq!(content.height_used(), 10);
        assert_eq!(content.remaining_height(), 15); // 25 - 10
    }

    #[test]
    fn test_full_content() {
        let element = element! { View(width: 80, height: 25) }.into_any();
        let content = MiddleContent::full(element);

        assert_eq!(content.height_used(), 25);
        assert_eq!(content.remaining_height(), 0);
    }

    #[test]
    #[should_panic(expected = "OVERFLOW")]
    #[cfg(debug_assertions)]
    fn test_overflow_detection() {
        let element = element! { View(width: 80, height: 30) }.into_any();
        // This should panic in debug mode - height 30 > MIDDLE_HEIGHT 25
        let _ = MiddleContent::new(element, 30);
    }
}
