//! # TUI Layout System
//!
//! Consistent grid system and layout primitives for the Aura TUI.
//! Provides standardized dimensions, spacing, and layout builders
//! to ensure visual consistency across all screens.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

// ============================================================================
// Grid Constants
// ============================================================================

/// Base grid unit (1 terminal cell)
pub const UNIT: u16 = 1;

/// Standard spacing values
pub mod spacing {
    use super::UNIT;

    /// No spacing
    pub const NONE: u16 = 0;
    /// Tight spacing (1 unit)
    pub const TIGHT: u16 = UNIT;
    /// Standard spacing (2 units)
    pub const STANDARD: u16 = UNIT * 2;
    /// Relaxed spacing (3 units)
    pub const RELAXED: u16 = UNIT * 3;
    /// Section spacing (4 units)
    pub const SECTION: u16 = UNIT * 4;
}

/// Standard panel heights
pub mod heights {
    use super::UNIT;

    /// Compact header/footer (3 lines)
    pub const COMPACT: u16 = UNIT * 3;
    /// Standard panel height (5 lines)
    pub const STANDARD: u16 = UNIT * 5;
    /// Medium panel height (8 lines)
    pub const MEDIUM: u16 = UNIT * 8;
    /// Large panel height (12 lines)
    pub const LARGE: u16 = UNIT * 12;
    /// Input field height (3 lines)
    pub const INPUT: u16 = UNIT * 3;
    /// Status bar height (1 line)
    pub const STATUS_BAR: u16 = UNIT;
    /// Title bar height (3 lines)
    pub const TITLE_BAR: u16 = UNIT * 3;
    /// Action bar with buttons (3 lines)
    pub const ACTION_BAR: u16 = UNIT * 3;
}

/// Standard widths for sidebars and panels
pub mod widths {
    /// Narrow sidebar (20 columns)
    pub const SIDEBAR_NARROW: u16 = 20;
    /// Standard sidebar (28 columns)
    pub const SIDEBAR_STANDARD: u16 = 28;
    /// Wide sidebar (36 columns)
    pub const SIDEBAR_WIDE: u16 = 36;
    /// Minimum content width
    pub const CONTENT_MIN: u16 = 40;
}

/// Standard percentage-based splits
pub mod splits {
    /// One-third split
    pub const THIRD: u16 = 33;
    /// Two-thirds split
    pub const TWO_THIRDS: u16 = 67;
    /// Half split
    pub const HALF: u16 = 50;
    /// Quarter split
    pub const QUARTER: u16 = 25;
    /// Three-quarters split
    pub const THREE_QUARTERS: u16 = 75;
    /// Sidebar/content ratio (30/70)
    pub const SIDEBAR: u16 = 30;
    /// Content/sidebar ratio (70/30)
    pub const CONTENT: u16 = 70;
}

// ============================================================================
// Layout Margin/Padding
// ============================================================================

/// Margin configuration for layout areas
#[derive(Debug, Clone, Copy, Default)]
pub struct Margin {
    /// Top margin in terminal cells
    pub top: u16,
    /// Right margin in terminal cells
    pub right: u16,
    /// Bottom margin in terminal cells
    pub bottom: u16,
    /// Left margin in terminal cells
    pub left: u16,
}

impl Margin {
    /// No margin
    pub const fn none() -> Self {
        Self {
            top: 0,
            right: 0,
            bottom: 0,
            left: 0,
        }
    }

    /// Uniform margin on all sides
    pub const fn uniform(size: u16) -> Self {
        Self {
            top: size,
            right: size,
            bottom: size,
            left: size,
        }
    }

    /// Symmetric margin (vertical, horizontal)
    pub const fn symmetric(vertical: u16, horizontal: u16) -> Self {
        Self {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
        }
    }

    /// Standard content margin (1 unit all around)
    pub const fn standard() -> Self {
        Self::uniform(spacing::TIGHT)
    }

    /// Apply margin to a rect, returning the inner area
    pub fn apply(&self, area: Rect) -> Rect {
        let x = area.x.saturating_add(self.left);
        let y = area.y.saturating_add(self.top);
        let width = area
            .width
            .saturating_sub(self.left)
            .saturating_sub(self.right);
        let height = area
            .height
            .saturating_sub(self.top)
            .saturating_sub(self.bottom);
        Rect::new(x, y, width, height)
    }
}

// ============================================================================
// Layout Presets
// ============================================================================

/// Common layout patterns used across screens
pub struct LayoutPresets;

impl LayoutPresets {
    /// Main content area with optional status bar at bottom
    /// Returns: [content, status_bar]
    pub fn with_status_bar(area: Rect) -> Vec<Rect> {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(heights::STATUS_BAR)])
            .split(area)
            .to_vec()
    }

    /// Header + content layout
    /// Returns: [header, content]
    pub fn header_content(area: Rect, header_height: u16) -> Vec<Rect> {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(header_height), Constraint::Min(1)])
            .split(area)
            .to_vec()
    }

    /// Header + content + footer layout
    /// Returns: [header, content, footer]
    pub fn header_content_footer(
        area: Rect,
        header_height: u16,
        footer_height: u16,
    ) -> Vec<Rect> {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(header_height),
                Constraint::Min(1),
                Constraint::Length(footer_height),
            ])
            .split(area)
            .to_vec()
    }

    /// Sidebar + content horizontal split
    /// Returns: [sidebar, content]
    pub fn sidebar_content(area: Rect, sidebar_width: u16) -> Vec<Rect> {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(sidebar_width),
                Constraint::Min(widths::CONTENT_MIN),
            ])
            .split(area)
            .to_vec()
    }

    /// Content + sidebar horizontal split (sidebar on right)
    /// Returns: [content, sidebar]
    pub fn content_sidebar(area: Rect, sidebar_width: u16) -> Vec<Rect> {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(widths::CONTENT_MIN),
                Constraint::Length(sidebar_width),
            ])
            .split(area)
            .to_vec()
    }

    /// Two-column percentage split
    /// Returns: [left, right]
    pub fn two_columns(area: Rect, left_percent: u16) -> Vec<Rect> {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(left_percent),
                Constraint::Percentage(100 - left_percent),
            ])
            .split(area)
            .to_vec()
    }

    /// Three-column equal split
    /// Returns: [left, center, right]
    pub fn three_columns_equal(area: Rect) -> Vec<Rect> {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(splits::THIRD),
                Constraint::Percentage(splits::THIRD),
                Constraint::Percentage(splits::THIRD),
            ])
            .split(area)
            .to_vec()
    }

    /// Content area with input field at bottom
    /// Returns: [content, input]
    pub fn content_with_input(area: Rect) -> Vec<Rect> {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(heights::INPUT)])
            .split(area)
            .to_vec()
    }

    /// Standard screen layout: header + main content + input + status
    /// Returns: [header, content, input, status]
    pub fn standard_screen(area: Rect) -> Vec<Rect> {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(heights::TITLE_BAR),
                Constraint::Min(1),
                Constraint::Length(heights::INPUT),
                Constraint::Length(heights::STATUS_BAR),
            ])
            .split(area)
            .to_vec()
    }

    /// Dashboard layout: header + two equal rows + footer
    /// Returns: [header, top_row, bottom_row, footer]
    pub fn dashboard(area: Rect) -> Vec<Rect> {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(heights::TITLE_BAR),
                Constraint::Percentage(splits::HALF),
                Constraint::Percentage(splits::HALF),
                Constraint::Length(heights::ACTION_BAR),
            ])
            .split(area)
            .to_vec()
    }

    /// Form layout with multiple fixed-height sections
    /// Returns areas for each section
    pub fn form_sections(area: Rect, section_heights: &[u16]) -> Vec<Rect> {
        let constraints: Vec<Constraint> = section_heights
            .iter()
            .map(|&h| Constraint::Length(h))
            .collect();

        Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area)
            .to_vec()
    }
}

// ============================================================================
// Screen Layout Builder
// ============================================================================

/// Builder for constructing complex screen layouts
#[derive(Debug, Clone)]
pub struct ScreenLayout {
    margin: Margin,
    sections: Vec<LayoutSection>,
}

#[derive(Debug, Clone)]
enum LayoutSection {
    Fixed(u16),
    Flexible(u16), // min height
    Percentage(u16),
}

impl ScreenLayout {
    /// Create a new screen layout builder
    pub fn new() -> Self {
        Self {
            margin: Margin::none(),
            sections: Vec::new(),
        }
    }

    /// Set the outer margin
    pub fn margin(mut self, margin: Margin) -> Self {
        self.margin = margin;
        self
    }

    /// Add a fixed-height section
    pub fn fixed(mut self, height: u16) -> Self {
        self.sections.push(LayoutSection::Fixed(height));
        self
    }

    /// Add a flexible section that fills remaining space
    pub fn flexible(mut self, min_height: u16) -> Self {
        self.sections.push(LayoutSection::Flexible(min_height));
        self
    }

    /// Add a percentage-based section
    pub fn percentage(mut self, percent: u16) -> Self {
        self.sections.push(LayoutSection::Percentage(percent));
        self
    }

    /// Build the layout and return the areas
    pub fn build(&self, area: Rect) -> Vec<Rect> {
        let inner = self.margin.apply(area);

        let constraints: Vec<Constraint> = self
            .sections
            .iter()
            .map(|s| match s {
                LayoutSection::Fixed(h) => Constraint::Length(*h),
                LayoutSection::Flexible(min) => Constraint::Min(*min),
                LayoutSection::Percentage(p) => Constraint::Percentage(*p),
            })
            .collect();

        Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(inner)
            .to_vec()
    }
}

impl Default for ScreenLayout {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Panel Layout Helper
// ============================================================================

/// Helper for creating consistent panel layouts within a container
pub struct PanelLayout;

impl PanelLayout {
    /// Create a centered panel with maximum width
    pub fn centered(area: Rect, max_width: u16) -> Rect {
        if area.width <= max_width {
            return area;
        }
        let padding = (area.width - max_width) / 2;
        Rect::new(area.x + padding, area.y, max_width, area.height)
    }

    /// Create a panel with consistent inner padding
    pub fn with_padding(area: Rect, padding: u16) -> Rect {
        Margin::uniform(padding).apply(area)
    }

    /// Split area into a grid of cells
    pub fn grid(area: Rect, rows: u16, cols: u16) -> Vec<Vec<Rect>> {
        let row_constraints: Vec<Constraint> =
            (0..rows).map(|_| Constraint::Ratio(1, rows as u32)).collect();
        let col_constraints: Vec<Constraint> =
            (0..cols).map(|_| Constraint::Ratio(1, cols as u32)).collect();

        let row_areas = Layout::default()
            .direction(Direction::Vertical)
            .constraints(row_constraints)
            .split(area);

        row_areas
            .iter()
            .map(|row| {
                Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints(col_constraints.clone())
                    .split(*row)
                    .to_vec()
            })
            .collect()
    }

    /// Create evenly spaced horizontal sections
    pub fn horizontal_split(area: Rect, count: u16) -> Vec<Rect> {
        let constraints: Vec<Constraint> = (0..count)
            .map(|_| Constraint::Ratio(1, count as u32))
            .collect();

        Layout::default()
            .direction(Direction::Horizontal)
            .constraints(constraints)
            .split(area)
            .to_vec()
    }

    /// Create evenly spaced vertical sections
    pub fn vertical_split(area: Rect, count: u16) -> Vec<Rect> {
        let constraints: Vec<Constraint> = (0..count)
            .map(|_| Constraint::Ratio(1, count as u32))
            .collect();

        Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area)
            .to_vec()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn test_area() -> Rect {
        Rect::new(0, 0, 100, 50)
    }

    #[test]
    fn test_margin_apply() {
        let area = test_area();
        let margin = Margin::uniform(2);
        let inner = margin.apply(area);

        assert_eq!(inner.x, 2);
        assert_eq!(inner.y, 2);
        assert_eq!(inner.width, 96);
        assert_eq!(inner.height, 46);
    }

    #[test]
    fn test_sidebar_content() {
        let area = test_area();
        let parts = LayoutPresets::sidebar_content(area, widths::SIDEBAR_STANDARD);

        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].width, widths::SIDEBAR_STANDARD);
    }

    #[test]
    fn test_header_content_footer() {
        let area = test_area();
        let parts =
            LayoutPresets::header_content_footer(area, heights::TITLE_BAR, heights::ACTION_BAR);

        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].height, heights::TITLE_BAR);
        assert_eq!(parts[2].height, heights::ACTION_BAR);
    }

    #[test]
    fn test_screen_layout_builder() {
        let area = test_area();
        let layout = ScreenLayout::new()
            .margin(Margin::uniform(1))
            .fixed(heights::TITLE_BAR)
            .flexible(10)
            .fixed(heights::INPUT)
            .build(area);

        assert_eq!(layout.len(), 3);
        assert_eq!(layout[0].height, heights::TITLE_BAR);
        assert_eq!(layout[2].height, heights::INPUT);
    }

    #[test]
    fn test_panel_grid() {
        let area = test_area();
        let grid = PanelLayout::grid(area, 2, 3);

        assert_eq!(grid.len(), 2);
        assert_eq!(grid[0].len(), 3);
    }

    #[test]
    fn test_centered_panel() {
        let area = test_area();
        let centered = PanelLayout::centered(area, 60);

        assert_eq!(centered.width, 60);
        assert_eq!(centered.x, 20); // (100 - 60) / 2
    }
}
