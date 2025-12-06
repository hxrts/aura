//! # Theme Constants
//!
//! Centralized color and style definitions for consistent UI.

use iocraft::prelude::*;

/// Theme constants for the Aura TUI
pub struct Theme;

impl Theme {
    // === Primary Colors ===
    pub const PRIMARY: Color = Color::Cyan;
    pub const SECONDARY: Color = Color::Yellow;
    pub const ACCENT: Color = Color::Blue;

    // === Text Colors ===
    pub const TEXT: Color = Color::White;
    pub const TEXT_MUTED: Color = Color::AnsiValue(245); // Light grey - visible on dark backgrounds
    pub const TEXT_HIGHLIGHT: Color = Color::Cyan;

    // === Background Colors ===
    pub const BG_DARK: Color = Color::AnsiValue(236); // Dark grey background for unselected items
    pub const BG_PRIMARY: Color = Color::DarkBlue;
    pub const BG_SELECTED: Color = Color::AnsiValue(24); // Dark blue - distinct but not overpowering
    pub const BG_HOVER: Color = Color::AnsiValue(238); // Slightly lighter than dark
    pub const OVERLAY: Color = Color::AnsiValue(0); // Black for overlay backdrop

    // === List Item Colors (for consistent scrollable lists) ===
    /// Background for selected list items
    pub const LIST_BG_SELECTED: Color = Color::AnsiValue(24); // Dark blue
    /// Background for unselected list items
    pub const LIST_BG_NORMAL: Color = Color::AnsiValue(236); // Dark grey
    /// Text color for selected list items - high contrast on dark blue
    pub const LIST_TEXT_SELECTED: Color = Color::White;
    /// Primary text color for unselected list items
    pub const LIST_TEXT_NORMAL: Color = Color::AnsiValue(252); // Light grey
    /// Secondary/muted text for unselected list items
    pub const LIST_TEXT_MUTED: Color = Color::AnsiValue(245); // Medium grey

    // === Border Colors ===
    pub const BORDER: Color = Color::DarkGrey;
    pub const BORDER_FOCUS: Color = Color::Cyan;
    pub const BORDER_ACTIVE: Color = Color::Blue;

    // === Status Colors ===
    pub const SUCCESS: Color = Color::Green;
    pub const WARNING: Color = Color::Yellow;
    pub const ERROR: Color = Color::Red;
    pub const INFO: Color = Color::Blue;

    // === Message Bubbles ===
    pub const MSG_OWN: Color = Color::DarkBlue;
    pub const MSG_OTHER: Color = Color::AnsiValue(238); // Darker grey for other messages
}

/// Spacing scale for consistent layout
pub struct Spacing;

impl Spacing {
    /// Extra small spacing (1 unit)
    pub const XS: u32 = 1;
    /// Small spacing (2 units)
    pub const SM: u32 = 2;
    /// Medium spacing (3 units)
    pub const MD: u32 = 3;
    /// Large spacing (4 units)
    pub const LG: u32 = 4;
    /// Extra large spacing (6 units)
    pub const XL: u32 = 6;

    // Component-specific spacing
    /// Standard panel padding
    pub const PANEL_PADDING: u32 = 1;
    /// Modal padding
    pub const MODAL_PADDING: u32 = 2;
    /// List item padding
    pub const LIST_ITEM_PADDING: u32 = 1;
    /// Gap between sections
    pub const SECTION_GAP: u32 = 2;
}

/// Unicode icons for status indicators and UI elements (no emoji)
pub struct Icons;

impl Icons {
    // Status indicators
    /// Checkmark for success/completed
    pub const CHECK: &'static str = "\u{2713}"; // ✓
    /// X mark for error/failed
    pub const CROSS: &'static str = "\u{2717}"; // ✗
    /// Warning triangle
    pub const WARNING: &'static str = "\u{26A0}"; // ⚠
    /// Info circle
    pub const INFO: &'static str = "\u{2139}"; // ℹ

    // Online/offline status
    /// Filled circle for online
    pub const ONLINE: &'static str = "\u{25CF}"; // ●
    /// Empty circle for offline
    pub const OFFLINE: &'static str = "\u{25CB}"; // ○
    /// Half circle for pending
    pub const PENDING: &'static str = "\u{25D0}"; // ◐

    // Loading spinner frames (cycle through these)
    /// Spinner frame 1
    pub const SPINNER_1: &'static str = "\u{25D0}"; // ◐
    /// Spinner frame 2
    pub const SPINNER_2: &'static str = "\u{25D3}"; // ◓
    /// Spinner frame 3
    pub const SPINNER_3: &'static str = "\u{25D1}"; // ◑
    /// Spinner frame 4
    pub const SPINNER_4: &'static str = "\u{25D2}"; // ◒

    // Arrows
    /// Right arrow
    pub const ARROW_RIGHT: &'static str = "\u{2192}"; // →
    /// Left arrow
    pub const ARROW_LEFT: &'static str = "\u{2190}"; // ←
    /// Up arrow
    pub const ARROW_UP: &'static str = "\u{2191}"; // ↑
    /// Down arrow
    pub const ARROW_DOWN: &'static str = "\u{2193}"; // ↓
    /// Double right arrow
    pub const ARROW_DOUBLE_RIGHT: &'static str = "\u{00BB}"; // »
    /// Double left arrow
    pub const ARROW_DOUBLE_LEFT: &'static str = "\u{00AB}"; // «

    // Security
    /// Lock icon
    pub const LOCK: &'static str = "\u{1F512}"; // We'll use a text fallback
    /// Key icon
    pub const KEY: &'static str = "\u{26BF}"; // ⚿
    /// Shield
    pub const SHIELD: &'static str = "\u{26E8}"; // ⛨

    // Miscellaneous
    /// Star
    pub const STAR: &'static str = "\u{2605}"; // ★
    /// Star outline
    pub const STAR_OUTLINE: &'static str = "\u{2606}"; // ☆
    /// Heart
    pub const HEART: &'static str = "\u{2665}"; // ♥
    /// Diamond
    pub const DIAMOND: &'static str = "\u{25C6}"; // ◆
    /// Square
    pub const SQUARE: &'static str = "\u{25A0}"; // ■
    /// Triangle right
    pub const TRIANGLE_RIGHT: &'static str = "\u{25B6}"; // ▶
    /// Triangle down
    pub const TRIANGLE_DOWN: &'static str = "\u{25BC}"; // ▼
    /// Ellipsis
    pub const ELLIPSIS: &'static str = "\u{2026}"; // …
    /// Bullet
    pub const BULLET: &'static str = "\u{2022}"; // •
    /// Vertical bar
    pub const VBAR: &'static str = "\u{2502}"; // │
    /// Horizontal bar
    pub const HBAR: &'static str = "\u{2500}"; // ─

    /// Get spinner frames for animation
    pub const SPINNER_FRAMES: [&'static str; 4] = [
        Self::SPINNER_1,
        Self::SPINNER_2,
        Self::SPINNER_3,
        Self::SPINNER_4,
    ];
}
