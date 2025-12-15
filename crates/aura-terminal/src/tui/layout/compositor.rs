//! Layout compositor for the TUI micro tiling system.
//!
//! The compositor manages fixed regions and coordinates content placement
//! within the 80×31 terminal grid.

use super::dim;
use std::fmt;

/// A rectangle representing a region in the terminal
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    pub const fn new(x: u16, y: u16, width: u16, height: u16) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Bottom edge (y + height)
    pub const fn bottom(&self) -> u16 {
        self.y + self.height
    }

    /// Right edge (x + width)
    pub const fn right(&self) -> u16 {
        self.x + self.width
    }
}

/// Error when terminal is too small for the layout
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutError {
    pub required_width: u16,
    pub required_height: u16,
    pub actual_width: u16,
    pub actual_height: u16,
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Terminal too small: requires {}×{}, got {}×{}",
            self.required_width, self.required_height, self.actual_width, self.actual_height
        )
    }
}

impl std::error::Error for LayoutError {}

/// The main layout compositor - enforces the micro tiling system.
///
/// Calculates fixed regions based on terminal size and provides
/// rectangles for content placement.
#[derive(Clone, Debug)]
pub struct LayoutCompositor {
    /// Fixed nav region (top)
    nav_rect: Rect,
    /// Fixed middle region (screen content or modal)
    middle_rect: Rect,
    /// Fixed footer region (bottom)
    footer_rect: Rect,
    /// Offset for centering in larger terminals
    x_offset: u16,
    y_offset: u16,
    /// Terminal dimensions
    terminal_width: u16,
    terminal_height: u16,
}

impl LayoutCompositor {
    /// Create a new compositor for the given terminal size.
    ///
    /// Returns an error if the terminal is too small.
    pub fn new(terminal_width: u16, terminal_height: u16) -> Result<Self, LayoutError> {
        // Validate minimum size
        if terminal_width < dim::TOTAL_WIDTH || terminal_height < dim::TOTAL_HEIGHT {
            return Err(LayoutError {
                required_width: dim::TOTAL_WIDTH,
                required_height: dim::TOTAL_HEIGHT,
                actual_width: terminal_width,
                actual_height: terminal_height,
            });
        }

        // Center in larger terminals
        let x_offset = (terminal_width - dim::TOTAL_WIDTH) / 2;
        let y_offset = (terminal_height - dim::TOTAL_HEIGHT) / 2;

        Ok(Self {
            nav_rect: Rect::new(x_offset, y_offset, dim::TOTAL_WIDTH, dim::NAV_HEIGHT),
            middle_rect: Rect::new(
                x_offset,
                y_offset + dim::NAV_HEIGHT,
                dim::TOTAL_WIDTH,
                dim::MIDDLE_HEIGHT,
            ),
            footer_rect: Rect::new(
                x_offset,
                y_offset + dim::NAV_HEIGHT + dim::MIDDLE_HEIGHT,
                dim::TOTAL_WIDTH,
                dim::FOOTER_HEIGHT,
            ),
            x_offset,
            y_offset,
            terminal_width,
            terminal_height,
        })
    }

    /// Create a compositor with exact 80×31 dimensions (no centering)
    pub fn exact() -> Self {
        Self {
            nav_rect: Rect::new(0, 0, dim::TOTAL_WIDTH, dim::NAV_HEIGHT),
            middle_rect: Rect::new(0, dim::NAV_HEIGHT, dim::TOTAL_WIDTH, dim::MIDDLE_HEIGHT),
            footer_rect: Rect::new(
                0,
                dim::NAV_HEIGHT + dim::MIDDLE_HEIGHT,
                dim::TOTAL_WIDTH,
                dim::FOOTER_HEIGHT,
            ),
            x_offset: 0,
            y_offset: 0,
            terminal_width: dim::TOTAL_WIDTH,
            terminal_height: dim::TOTAL_HEIGHT,
        }
    }

    /// Get the fixed rect for nav bar
    pub fn nav_rect(&self) -> Rect {
        self.nav_rect
    }

    /// Get the fixed rect for middle content (screen or modal)
    pub fn middle_rect(&self) -> Rect {
        self.middle_rect
    }

    /// Get the fixed rect for footer (key hints or toast)
    pub fn footer_rect(&self) -> Rect {
        self.footer_rect
    }

    /// X offset for centering
    pub fn x_offset(&self) -> u16 {
        self.x_offset
    }

    /// Y offset for centering
    pub fn y_offset(&self) -> u16 {
        self.y_offset
    }

    /// Current terminal width
    pub fn terminal_width(&self) -> u16 {
        self.terminal_width
    }

    /// Current terminal height
    pub fn terminal_height(&self) -> u16 {
        self.terminal_height
    }

    /// Check if terminal is larger than minimum (layout will be centered)
    pub fn is_centered(&self) -> bool {
        self.x_offset > 0 || self.y_offset > 0
    }

    /// Total layout bounds (the 80×31 area)
    pub fn layout_bounds(&self) -> Rect {
        Rect::new(
            self.x_offset,
            self.y_offset,
            dim::TOTAL_WIDTH,
            dim::TOTAL_HEIGHT,
        )
    }
}

impl Default for LayoutCompositor {
    fn default() -> Self {
        Self::exact()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_compositor() {
        let comp = LayoutCompositor::exact();

        assert_eq!(comp.nav_rect(), Rect::new(0, 0, 80, 3));
        assert_eq!(comp.middle_rect(), Rect::new(0, 3, 80, 25));
        assert_eq!(comp.footer_rect(), Rect::new(0, 28, 80, 3));
        assert!(!comp.is_centered());
    }

    #[test]
    fn test_compositor_with_exact_size() {
        let comp = LayoutCompositor::new(80, 31).unwrap();

        assert_eq!(comp.x_offset(), 0);
        assert_eq!(comp.y_offset(), 0);
        assert!(!comp.is_centered());
    }

    #[test]
    fn test_compositor_with_larger_terminal() {
        let comp = LayoutCompositor::new(100, 40).unwrap();

        assert_eq!(comp.x_offset(), 10); // (100 - 80) / 2
        assert_eq!(comp.y_offset(), 4); // (40 - 31) / 2 = 4 (integer division)
        assert!(comp.is_centered());

        // Check regions are offset correctly
        assert_eq!(comp.nav_rect().x, 10);
        assert_eq!(comp.nav_rect().y, 4);
        assert_eq!(comp.middle_rect().x, 10);
        assert_eq!(comp.middle_rect().y, 7); // 4 + 3
        assert_eq!(comp.footer_rect().x, 10);
        assert_eq!(comp.footer_rect().y, 32); // 4 + 3 + 25
    }

    #[test]
    fn test_compositor_too_small() {
        let err = LayoutCompositor::new(70, 31).unwrap_err();
        assert_eq!(err.actual_width, 70);
        assert_eq!(err.required_width, 80);

        let err = LayoutCompositor::new(80, 20).unwrap_err();
        assert_eq!(err.actual_height, 20);
        assert_eq!(err.required_height, 31);
    }

    #[test]
    fn test_layout_bounds() {
        let comp = LayoutCompositor::new(100, 40).unwrap();
        let bounds = comp.layout_bounds();

        assert_eq!(bounds.x, 10);
        assert_eq!(bounds.y, 4);
        assert_eq!(bounds.width, 80);
        assert_eq!(bounds.height, 31);
    }

    #[test]
    fn test_regions_are_contiguous() {
        let comp = LayoutCompositor::exact();

        // Nav bottom == Middle top
        assert_eq!(comp.nav_rect().bottom(), comp.middle_rect().y);

        // Middle bottom == Footer top
        assert_eq!(comp.middle_rect().bottom(), comp.footer_rect().y);

        // Footer bottom == Total height
        assert_eq!(comp.footer_rect().bottom(), dim::TOTAL_HEIGHT);
    }
}
