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

    /// Navigation bar height (top) - 2 rows: tabs + border
    pub const NAV_HEIGHT: u16 = 2;

    /// Footer height (bottom)
    pub const FOOTER_HEIGHT: u16 = 3;

    /// Middle content area height (computed)
    pub const MIDDLE_HEIGHT: u16 = TOTAL_HEIGHT - NAV_HEIGHT - FOOTER_HEIGHT; // 26

    /// Key hints bar height (fixed, within footer)
    pub const KEY_HINTS_HEIGHT: u16 = 2;

    /// Footer border/separator height
    pub const FOOTER_BORDER_HEIGHT: u16 = 1;

    // === Two-Panel Layout (Settings, Contacts, etc.) ===

    /// Left panel width for two-panel layouts (list/menu side)
    pub const TWO_PANEL_LEFT_WIDTH: u16 = 28;

    /// Gap between panels
    pub const TWO_PANEL_GAP: u16 = 1;

    /// Right panel width for two-panel layouts (detail side)
    /// Computed as: TOTAL_WIDTH - LEFT_WIDTH - GAP = 80 - 28 - 1 = 51
    pub const TWO_PANEL_RIGHT_WIDTH: u16 = TOTAL_WIDTH - TWO_PANEL_LEFT_WIDTH - TWO_PANEL_GAP;

    // === Message Panel Layout ===

    /// Message panel height (includes borders, title, etc.)
    pub const MESSAGE_PANEL_HEIGHT: u16 = 22;

    /// Message panel border overhead (top + bottom)
    pub const MESSAGE_PANEL_BORDER: u16 = 2;

    /// Message panel title row (includes padding)
    pub const MESSAGE_PANEL_TITLE: u16 = 2;

    /// Message panel internal padding (bottom only, top is covered by title)
    pub const MESSAGE_PANEL_PADDING: u16 = 1;

    /// Visible message rows in the message panel
    /// Computed: panel height - borders - title - padding = 22 - 2 - 2 - 1 = 17
    pub const VISIBLE_MESSAGE_ROWS: u16 =
        MESSAGE_PANEL_HEIGHT - MESSAGE_PANEL_BORDER - MESSAGE_PANEL_TITLE - MESSAGE_PANEL_PADDING;

    // Compile-time validation
    const _: () = assert!(NAV_HEIGHT + MIDDLE_HEIGHT + FOOTER_HEIGHT == TOTAL_HEIGHT);
    const _: () = assert!(MIDDLE_HEIGHT > 0);
    const _: () = assert!(TOTAL_WIDTH >= 40); // Minimum usable width
    const _: () = assert!(MIDDLE_HEIGHT == 26); // Explicit check for expected value
    const _: () = assert!(KEY_HINTS_HEIGHT + FOOTER_BORDER_HEIGHT == FOOTER_HEIGHT);
    const _: () =
        assert!(TWO_PANEL_LEFT_WIDTH + TWO_PANEL_GAP + TWO_PANEL_RIGHT_WIDTH == TOTAL_WIDTH);
    const _: () = assert!(TWO_PANEL_RIGHT_WIDTH == 51); // Explicit check
    const _: () = assert!(VISIBLE_MESSAGE_ROWS == 17); // Explicit check for message rows
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
        assert_eq!(NAV_HEIGHT, 2);
        assert_eq!(MIDDLE_HEIGHT, 26);
        assert_eq!(FOOTER_HEIGHT, 3);
    }

    #[test]
    fn test_footer_subdivision() {
        assert_eq!(KEY_HINTS_HEIGHT, 2);
        assert_eq!(FOOTER_BORDER_HEIGHT, 1);
        assert_eq!(KEY_HINTS_HEIGHT + FOOTER_BORDER_HEIGHT, FOOTER_HEIGHT);
    }
}
