//! Content builder with overflow prevention.
//!
//! The ContentBuilder tracks height usage and prevents overflow
//! by panicking (in debug) or silently truncating (in release)
//! when content exceeds region bounds.

use super::content::BoundedContent;
use super::dim;
use super::regions::{Footer, LayoutRegion, Middle, Nav};
use iocraft::prelude::*;

/// Content builder that tracks height and prevents overflow.
///
/// Use this to construct content for a region piece by piece,
/// with runtime validation that the total doesn't exceed bounds.
pub struct ContentBuilder<R: LayoutRegion> {
    elements: Vec<AnyElement<'static>>,
    height_used: u16,
    _region: std::marker::PhantomData<R>,
}

impl<R: LayoutRegion> Default for ContentBuilder<R> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R: LayoutRegion> ContentBuilder<R> {
    /// Create a new content builder for a region
    #[must_use]
    pub fn new() -> Self {
        Self {
            elements: Vec::new(),
            height_used: 0,
            _region: std::marker::PhantomData,
        }
    }

    /// Maximum height available for this region
    #[must_use]
    pub const fn max_height() -> u16 {
        R::HEIGHT
    }

    /// Add an element with known height.
    ///
    /// In debug builds, panics if this would cause overflow.
    /// In release builds, silently ignores elements that would overflow.
    #[must_use]
    pub fn add(mut self, element: AnyElement<'static>, height: u16) -> Self {
        let new_height = self.height_used + height;

        debug_assert!(
            new_height <= R::HEIGHT,
            "OVERFLOW in {} region: adding {} rows would exceed {} (already used: {})",
            R::NAME,
            height,
            R::HEIGHT,
            self.height_used
        );

        // In release mode, silently skip elements that would overflow
        if new_height <= R::HEIGHT {
            self.height_used = new_height;
            self.elements.push(element);
        }

        self
    }

    /// Add an element that should fill remaining space.
    ///
    /// The element will be given flex_grow: 1.0 to fill remaining height.
    #[must_use]
    pub fn fill(mut self, element: AnyElement<'static>) -> Self {
        let remaining = self.remaining();
        if remaining > 0 {
            self.elements.push(element);
            self.height_used = R::HEIGHT;
        }
        self
    }

    /// Remaining height available in this region
    #[must_use]
    pub fn remaining(&self) -> u16 {
        R::HEIGHT.saturating_sub(self.height_used)
    }

    /// Height already used
    #[must_use]
    pub fn height_used(&self) -> u16 {
        self.height_used
    }

    /// Check if there's room for more content
    #[must_use]
    pub fn has_remaining(&self) -> bool {
        self.remaining() > 0
    }

    /// Build into bounded content.
    ///
    /// Wraps all elements in a vertical View with fixed dimensions.
    #[must_use]
    pub fn build(self) -> BoundedContent<R> {
        let height_used = self.height_used;

        // Combine elements into vertical stack with fixed dimensions
        let combined = element! {
            View(
                width: dim::TOTAL_WIDTH,
                height: R::HEIGHT,
                flex_direction: FlexDirection::Column,
                overflow: Overflow::Hidden,
            ) {
                #(self.elements)
            }
        };

        BoundedContent::new(combined.into_any(), height_used)
    }
}

/// Type alias for nav content builder
pub type NavBuilder = ContentBuilder<Nav>;

/// Type alias for middle content builder (screen content)
pub type MiddleBuilder = ContentBuilder<Middle>;

/// Type alias for footer content builder
pub type FooterBuilder = ContentBuilder<Footer>;

/// Type alias for modal content builder (same as middle)
pub type ModalBuilder = MiddleBuilder;

/// Type alias for toast content builder (same as footer)
pub type ToastBuilder = FooterBuilder;

// Convenience constructors
impl NavBuilder {
    /// Create a nav bar builder (3 rows max)
    #[must_use]
    pub fn nav() -> Self {
        Self::new()
    }
}

impl MiddleBuilder {
    /// Create a middle content builder (25 rows max)
    #[must_use]
    pub fn middle() -> Self {
        Self::new()
    }

    /// Create a modal content builder (25 rows max, same as middle)
    #[must_use]
    pub fn modal() -> Self {
        Self::new()
    }
}

impl FooterBuilder {
    /// Create a footer builder (3 rows max)
    #[must_use]
    pub fn footer() -> Self {
        Self::new()
    }

    /// Create a toast builder (3 rows max, same as footer)
    #[must_use]
    pub fn toast() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::layout::dim;

    fn dummy_element(height: u16) -> AnyElement<'static> {
        element! {
            View(width: 80, height: height)
        }
        .into_any()
    }

    #[test]
    fn test_builder_tracking() {
        let builder = MiddleBuilder::new()
            .add(dummy_element(5), 5)
            .add(dummy_element(10), 10);

        assert_eq!(builder.height_used(), 15);
        assert_eq!(builder.remaining(), MiddleBuilder::max_height() - 15);
        assert!(builder.has_remaining());
    }

    #[test]
    fn test_builder_full() {
        let builder = MiddleBuilder::new().add(dummy_element(10), 10).add(
            dummy_element(MiddleBuilder::max_height() - 10),
            MiddleBuilder::max_height() - 10,
        );

        assert_eq!(builder.height_used(), MiddleBuilder::max_height());
        assert_eq!(builder.remaining(), 0);
        assert!(!builder.has_remaining());
    }

    #[test]
    fn test_nav_builder() {
        let builder = NavBuilder::nav().add(dummy_element(2), 2);

        assert_eq!(builder.height_used(), 2);
        assert_eq!(
            builder.remaining(),
            NavBuilder::max_height().saturating_sub(2)
        );
        assert_eq!(NavBuilder::max_height(), dim::NAV_HEIGHT);
    }

    #[test]
    fn test_footer_builder() {
        let builder = FooterBuilder::footer().add(dummy_element(1), 1);

        assert_eq!(builder.height_used(), 1);
        assert_eq!(
            builder.remaining(),
            FooterBuilder::max_height().saturating_sub(1)
        );
        assert_eq!(FooterBuilder::max_height(), dim::FOOTER_HEIGHT);
    }

    #[test]
    fn test_build_produces_bounded_content() {
        let content = MiddleBuilder::new()
            .add(dummy_element(10), 10)
            .add(dummy_element(5), 5)
            .build();

        assert_eq!(content.height_used(), 15);
        assert_eq!(content.remaining_height(), MiddleBuilder::max_height() - 15);
    }

    #[test]
    #[should_panic(expected = "OVERFLOW")]
    #[cfg(debug_assertions)]
    fn test_overflow_panics_in_debug() {
        MiddleBuilder::new()
            .add(dummy_element(20), 20)
            .add(dummy_element(10), 10); // This should panic: 20 + 10 > max height
    }
}
