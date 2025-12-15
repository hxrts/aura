//! Fixed layout dimensions for all TUI screens.
//!
//! This module defines compile-time validated constants for the micro tiling system.
//! All screens, modals, and overlays use these fixed dimensions.

/// Fixed layout dimensions for all TUI screens
pub mod dim {
    /// Total terminal width (characters)
    pub const TOTAL_WIDTH: u16 = 80;

    /// Total terminal height (rows)
    pub const TOTAL_HEIGHT: u16 = 31;

    /// Navigation bar height (top)
    pub const NAV_HEIGHT: u16 = 3;

    /// Footer height (bottom)
    pub const FOOTER_HEIGHT: u16 = 3;

    /// Middle content area height (computed)
    pub const MIDDLE_HEIGHT: u16 = TOTAL_HEIGHT - NAV_HEIGHT - FOOTER_HEIGHT; // 25

    /// Key hints bar height (fixed, within footer)
    pub const KEY_HINTS_HEIGHT: u16 = 2;

    /// Footer border/separator height
    pub const FOOTER_BORDER_HEIGHT: u16 = 1;

    // Compile-time validation
    const _: () = assert!(NAV_HEIGHT + MIDDLE_HEIGHT + FOOTER_HEIGHT == TOTAL_HEIGHT);
    const _: () = assert!(MIDDLE_HEIGHT > 0);
    const _: () = assert!(TOTAL_WIDTH >= 40); // Minimum usable width
    const _: () = assert!(MIDDLE_HEIGHT == 25); // Explicit check for expected value
    const _: () = assert!(KEY_HINTS_HEIGHT + FOOTER_BORDER_HEIGHT == FOOTER_HEIGHT);
}

pub use dim::*;

#[cfg(test)]
mod tests {
    use super::dim::*;

    #[test]
    fn test_dimensions_add_up() {
        assert_eq!(NAV_HEIGHT + MIDDLE_HEIGHT + FOOTER_HEIGHT, TOTAL_HEIGHT);
    }

    #[test]
    fn test_total_dimensions() {
        assert_eq!(TOTAL_WIDTH, 80);
        assert_eq!(TOTAL_HEIGHT, 31);
    }

    #[test]
    fn test_region_heights() {
        assert_eq!(NAV_HEIGHT, 3);
        assert_eq!(MIDDLE_HEIGHT, 25);
        assert_eq!(FOOTER_HEIGHT, 3);
    }

    #[test]
    fn test_footer_subdivision() {
        assert_eq!(KEY_HINTS_HEIGHT, 2);
        assert_eq!(FOOTER_BORDER_HEIGHT, 1);
        assert_eq!(KEY_HINTS_HEIGHT + FOOTER_BORDER_HEIGHT, FOOTER_HEIGHT);
    }
}
