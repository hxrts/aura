//! # Navigation Module
//!
//! Provides consistent arrow key navigation across all TUI screens.
//!
//! ## Focus Grid Model
//!
//! Screens define a grid of focusable regions. Navigation works as:
//! - **Up/Down (k/j)**: Move vertically within current column, or navigate items in a list
//! - **Left/Right (h/l)**: Move horizontally between columns/panels
//!
//! ## Common Layouts
//!
//! 1. **Single panel**: Only vertical navigation (Help)
//! 2. **Two panels**: Left/Right switches panels, Up/Down navigates within (Contacts, Invitations, Settings)
//! 3. **Three panels**: Horizontal cycle through panels (Chat)
//! 4. **Grid**: 2D navigation (Neighborhood)

use crate::tui::components::ListNavigation;
use iocraft::prelude::*;

/// Check if a navigation key was pressed (not released)
pub fn is_nav_key_press(event: &TerminalEvent) -> Option<NavKey> {
    match event {
        TerminalEvent::Key(KeyEvent { code, kind, .. }) if *kind != KeyEventKind::Release => {
            match code {
                KeyCode::Up | KeyCode::Char('k') => Some(NavKey::Up),
                KeyCode::Down | KeyCode::Char('j') => Some(NavKey::Down),
                KeyCode::Left | KeyCode::Char('h') => Some(NavKey::Left),
                KeyCode::Right | KeyCode::Char('l') => Some(NavKey::Right),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Navigation key directions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavKey {
    Up,
    Down,
    Left,
    Right,
}

impl NavKey {
    /// Check if this is a vertical navigation key
    pub fn is_vertical(&self) -> bool {
        matches!(self, NavKey::Up | NavKey::Down)
    }

    /// Check if this is a horizontal navigation key
    pub fn is_horizontal(&self) -> bool {
        matches!(self, NavKey::Left | NavKey::Right)
    }

    /// Convert to ListNavigation for list navigation (vertical keys only)
    pub fn to_list_nav(&self) -> Option<ListNavigation> {
        match self {
            NavKey::Up => Some(ListNavigation::Up),
            NavKey::Down => Some(ListNavigation::Down),
            _ => None,
        }
    }
}

/// Focus state for two-panel layouts (list + detail)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TwoPanelFocus {
    #[default]
    List,
    Detail,
}

impl TwoPanelFocus {
    /// Toggle between list and detail
    pub fn toggle(&self) -> Self {
        match self {
            TwoPanelFocus::List => TwoPanelFocus::Detail,
            TwoPanelFocus::Detail => TwoPanelFocus::List,
        }
    }

    /// Move focus based on navigation key (with wrap-around)
    pub fn navigate(&self, key: NavKey) -> Self {
        match key {
            // Left/Right both toggle (wrap around between two panels)
            NavKey::Left | NavKey::Right => self.toggle(),
            _ => *self,
        }
    }
}

/// Focus state for three-panel layouts (list + content + input)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThreePanelFocus {
    #[default]
    Left,
    Center,
    Right,
}

impl ThreePanelFocus {
    /// Cycle to next panel (left -> center -> right -> left)
    pub fn next(&self) -> Self {
        match self {
            ThreePanelFocus::Left => ThreePanelFocus::Center,
            ThreePanelFocus::Center => ThreePanelFocus::Right,
            ThreePanelFocus::Right => ThreePanelFocus::Left,
        }
    }

    /// Cycle to previous panel
    pub fn prev(&self) -> Self {
        match self {
            ThreePanelFocus::Left => ThreePanelFocus::Right,
            ThreePanelFocus::Center => ThreePanelFocus::Left,
            ThreePanelFocus::Right => ThreePanelFocus::Center,
        }
    }

    /// Move focus based on navigation key
    pub fn navigate(&self, key: NavKey) -> Self {
        match key {
            NavKey::Left => self.prev(),
            NavKey::Right => self.next(),
            _ => *self,
        }
    }
}

/// Navigate within a list (vertical navigation) with wrap-around
pub fn navigate_list(current: usize, count: usize, key: NavKey) -> usize {
    if count == 0 {
        return 0;
    }
    match key {
        NavKey::Up => {
            if current == 0 {
                count - 1 // Wrap to last item
            } else {
                current - 1
            }
        }
        NavKey::Down => {
            if current >= count - 1 {
                0 // Wrap to first item
            } else {
                current + 1
            }
        }
        _ => current,
    }
}

/// Navigate within a 2D grid with wrap-around
pub fn navigate_grid(current: usize, cols: usize, total: usize, key: NavKey) -> usize {
    if total == 0 || cols == 0 {
        return 0;
    }
    let rows = total.div_ceil(cols); // ceiling division
    let current_row = current / cols;
    let current_col = current % cols;

    match key {
        NavKey::Up => {
            if current_row == 0 {
                // Wrap to last row, same column (or last item if column doesn't exist)
                let target = (rows - 1) * cols + current_col;
                target.min(total - 1)
            } else {
                current - cols
            }
        }
        NavKey::Down => {
            let next = current + cols;
            if next >= total {
                // Wrap to first row, same column
                current_col.min(total - 1)
            } else {
                next
            }
        }
        NavKey::Left => {
            if current == 0 {
                total - 1 // Wrap to last item
            } else {
                current - 1
            }
        }
        NavKey::Right => {
            if current >= total - 1 {
                0 // Wrap to first item
            } else {
                current + 1
            }
        }
    }
}

// ============================================================================
// Unified Navigation Types
// ============================================================================

/// Unified navigation state for list-based screens
///
/// Tracks both the current index and item count, enabling wrap-around navigation.
/// Update the count when data changes, and navigation will automatically wrap.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ListNav {
    /// Current selected index
    pub index: usize,
    /// Total number of items (updated from data layer)
    pub count: usize,
}

impl ListNav {
    /// Create a new list navigation state
    pub fn new() -> Self {
        Self::default()
    }

    /// Navigate using a NavKey (Up/Down move within list with wrap-around)
    pub fn navigate(&mut self, key: NavKey) {
        self.index = navigate_list(self.index, self.count, key);
    }

    /// Navigate up with wrap-around
    pub fn up(&mut self) {
        self.navigate(NavKey::Up);
    }

    /// Navigate down with wrap-around
    pub fn down(&mut self) {
        self.navigate(NavKey::Down);
    }

    /// Update the item count (call when data changes)
    ///
    /// Clamps the current index if count shrunk.
    pub fn set_count(&mut self, count: usize) {
        self.count = count;
        // Clamp index if count shrunk
        if count > 0 && self.index >= count {
            self.index = count - 1;
        } else if count == 0 {
            self.index = 0;
        }
    }

    /// Get the current index, clamped to valid range
    pub fn current(&self) -> usize {
        if self.count == 0 {
            0
        } else {
            self.index.min(self.count - 1)
        }
    }

    /// Check if there are any items
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Select a specific index (clamped to valid range)
    pub fn select(&mut self, index: usize) {
        if self.count > 0 {
            self.index = index.min(self.count - 1);
        } else {
            self.index = 0;
        }
    }

    /// Reset to first item
    pub fn reset(&mut self) {
        self.index = 0;
    }
}

/// Unified navigation state for grid-based screens
///
/// Tracks position in a 2D grid with wrap-around in all directions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GridNav {
    /// Current index in flattened grid
    pub index: usize,
    /// Number of columns in the grid
    pub cols: usize,
    /// Total number of items
    pub count: usize,
}

impl GridNav {
    /// Create a new grid navigation state
    pub fn new(cols: usize) -> Self {
        Self {
            index: 0,
            cols,
            count: 0,
        }
    }

    /// Navigate using a NavKey (all directions work with wrap-around)
    pub fn navigate(&mut self, key: NavKey) {
        self.index = navigate_grid(self.index, self.cols, self.count, key);
    }

    /// Navigate up with wrap-around
    pub fn up(&mut self) {
        self.navigate(NavKey::Up);
    }

    /// Navigate down with wrap-around
    pub fn down(&mut self) {
        self.navigate(NavKey::Down);
    }

    /// Navigate left with wrap-around
    pub fn left(&mut self) {
        self.navigate(NavKey::Left);
    }

    /// Navigate right with wrap-around
    pub fn right(&mut self) {
        self.navigate(NavKey::Right);
    }

    /// Get current row
    pub fn row(&self) -> usize {
        if self.cols == 0 {
            0
        } else {
            self.index / self.cols
        }
    }

    /// Get current column
    pub fn col(&self) -> usize {
        if self.cols == 0 {
            0
        } else {
            self.index % self.cols
        }
    }

    /// Update the item count (call when data changes)
    pub fn set_count(&mut self, count: usize) {
        self.count = count;
        // Clamp index if count shrunk
        if count > 0 && self.index >= count {
            self.index = count - 1;
        } else if count == 0 {
            self.index = 0;
        }
    }

    /// Update the column count
    pub fn set_cols(&mut self, cols: usize) {
        self.cols = cols;
    }

    /// Get the current index
    pub fn current(&self) -> usize {
        if self.count == 0 {
            0
        } else {
            self.index.min(self.count - 1)
        }
    }

    /// Select a specific index
    pub fn select(&mut self, index: usize) {
        if self.count > 0 {
            self.index = index.min(self.count - 1);
        } else {
            self.index = 0;
        }
    }

    /// Reset to first item
    pub fn reset(&mut self) {
        self.index = 0;
    }
}

/// Unified panel navigation for multi-panel layouts
///
/// Handles horizontal navigation between panels with wrap-around.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PanelNav {
    /// Current panel index
    pub index: usize,
    /// Total number of panels
    pub count: usize,
}

impl PanelNav {
    /// Create a new panel navigation with specified panel count
    pub fn new(count: usize) -> Self {
        Self { index: 0, count }
    }

    /// Navigate using a NavKey (Left/Right move between panels with wrap-around)
    pub fn navigate(&mut self, key: NavKey) {
        if self.count == 0 {
            return;
        }
        match key {
            NavKey::Left => {
                if self.index == 0 {
                    self.index = self.count - 1;
                } else {
                    self.index -= 1;
                }
            }
            NavKey::Right => {
                self.index = (self.index + 1) % self.count;
            }
            _ => {}
        }
    }

    /// Navigate left with wrap-around
    pub fn left(&mut self) {
        self.navigate(NavKey::Left);
    }

    /// Navigate right with wrap-around
    pub fn right(&mut self) {
        self.navigate(NavKey::Right);
    }

    /// Get current panel index
    pub fn current(&self) -> usize {
        self.index
    }

    /// Check if at first panel
    pub fn is_first(&self) -> bool {
        self.index == 0
    }

    /// Check if at last panel
    pub fn is_last(&self) -> bool {
        self.count > 0 && self.index == self.count - 1
    }

    /// Select a specific panel
    pub fn select(&mut self, index: usize) {
        if self.count > 0 {
            self.index = index.min(self.count - 1);
        }
    }
}

/// Composite navigation for screens with panels containing lists
///
/// Combines panel navigation (horizontal) with list navigation (vertical).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScreenNav {
    /// Panel/focus navigation
    pub panel: PanelNav,
    /// List navigation for each panel (indexed by panel index)
    pub lists: Vec<ListNav>,
}

impl ScreenNav {
    /// Create a new screen navigation with specified panel count
    pub fn new(panel_count: usize) -> Self {
        Self {
            panel: PanelNav::new(panel_count),
            lists: vec![ListNav::new(); panel_count],
        }
    }

    /// Navigate using a NavKey
    ///
    /// - Left/Right: move between panels
    /// - Up/Down: move within current panel's list
    pub fn navigate(&mut self, key: NavKey) {
        match key {
            NavKey::Left | NavKey::Right => {
                self.panel.navigate(key);
            }
            NavKey::Up | NavKey::Down => {
                if let Some(list) = self.lists.get_mut(self.panel.index) {
                    list.navigate(key);
                }
            }
        }
    }

    /// Get current panel index
    pub fn current_panel(&self) -> usize {
        self.panel.current()
    }

    /// Get current list selection in current panel
    pub fn current_selection(&self) -> usize {
        self.lists
            .get(self.panel.index)
            .map(|l| l.current())
            .unwrap_or(0)
    }

    /// Get list navigation for current panel
    pub fn current_list(&self) -> Option<&ListNav> {
        self.lists.get(self.panel.index)
    }

    /// Get mutable list navigation for current panel
    pub fn current_list_mut(&mut self) -> Option<&mut ListNav> {
        self.lists.get_mut(self.panel.index)
    }

    /// Update list count for a specific panel
    pub fn set_list_count(&mut self, panel: usize, count: usize) {
        if let Some(list) = self.lists.get_mut(panel) {
            list.set_count(count);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_two_panel_focus_toggle() {
        let focus = TwoPanelFocus::List;
        assert_eq!(focus.toggle(), TwoPanelFocus::Detail);
        assert_eq!(focus.toggle().toggle(), TwoPanelFocus::List);
    }

    #[test]
    fn test_two_panel_focus_navigate() {
        let focus = TwoPanelFocus::List;
        // Left and Right both toggle (wrap around)
        assert_eq!(focus.navigate(NavKey::Right), TwoPanelFocus::Detail);
        assert_eq!(focus.navigate(NavKey::Left), TwoPanelFocus::Detail);
        // Detail wraps back to List
        let detail = TwoPanelFocus::Detail;
        assert_eq!(detail.navigate(NavKey::Right), TwoPanelFocus::List);
        assert_eq!(detail.navigate(NavKey::Left), TwoPanelFocus::List);
        // Vertical keys don't change panel focus
        assert_eq!(focus.navigate(NavKey::Up), TwoPanelFocus::List);
        assert_eq!(focus.navigate(NavKey::Down), TwoPanelFocus::List);
    }

    #[test]
    fn test_three_panel_focus_cycle() {
        let focus = ThreePanelFocus::Left;
        assert_eq!(focus.next(), ThreePanelFocus::Center);
        assert_eq!(focus.next().next(), ThreePanelFocus::Right);
        assert_eq!(focus.next().next().next(), ThreePanelFocus::Left);
    }

    #[test]
    fn test_navigate_list() {
        // Empty list
        assert_eq!(navigate_list(0, 0, NavKey::Down), 0);

        // Single item - wraps to itself
        assert_eq!(navigate_list(0, 1, NavKey::Up), 0);
        assert_eq!(navigate_list(0, 1, NavKey::Down), 0);

        // Multiple items - normal navigation
        assert_eq!(navigate_list(0, 5, NavKey::Down), 1);
        assert_eq!(navigate_list(2, 5, NavKey::Up), 1);

        // Wrap-around behavior
        assert_eq!(navigate_list(4, 5, NavKey::Down), 0); // Wraps to first
        assert_eq!(navigate_list(0, 5, NavKey::Up), 4); // Wraps to last
    }

    #[test]
    fn test_navigate_grid() {
        // 2x2 grid (indices 0,1 on row 0; 2,3 on row 1)
        assert_eq!(navigate_grid(0, 2, 4, NavKey::Right), 1);
        assert_eq!(navigate_grid(1, 2, 4, NavKey::Right), 2); // Continues to next row
        assert_eq!(navigate_grid(0, 2, 4, NavKey::Down), 2);
        assert_eq!(navigate_grid(2, 2, 4, NavKey::Up), 0);
        assert_eq!(navigate_grid(1, 2, 4, NavKey::Left), 0);

        // Wrap-around in 2x2 grid
        assert_eq!(navigate_grid(3, 2, 4, NavKey::Right), 0); // Wraps to first
        assert_eq!(navigate_grid(0, 2, 4, NavKey::Left), 3); // Wraps to last
        assert_eq!(navigate_grid(0, 2, 4, NavKey::Up), 2); // Wraps to bottom row
        assert_eq!(navigate_grid(2, 2, 4, NavKey::Down), 0); // Wraps to top row

        // 3x2 grid (6 items)
        assert_eq!(navigate_grid(0, 3, 6, NavKey::Right), 1);
        assert_eq!(navigate_grid(1, 3, 6, NavKey::Right), 2);
        assert_eq!(navigate_grid(2, 3, 6, NavKey::Right), 3); // Continues to next row
        assert_eq!(navigate_grid(0, 3, 6, NavKey::Down), 3);
        assert_eq!(navigate_grid(5, 3, 6, NavKey::Right), 0); // Wraps to first
    }

    #[test]
    fn test_list_nav_wrap_around() {
        let mut nav = ListNav::new();
        nav.set_count(5);

        // Start at 0
        assert_eq!(nav.current(), 0);

        // Go up from 0 wraps to last
        nav.up();
        assert_eq!(nav.current(), 4);

        // Go down from last wraps to first
        nav.down();
        assert_eq!(nav.current(), 0);

        // Normal down navigation
        nav.down();
        assert_eq!(nav.current(), 1);
        nav.down();
        assert_eq!(nav.current(), 2);

        // Normal up navigation
        nav.up();
        assert_eq!(nav.current(), 1);
    }

    #[test]
    fn test_list_nav_count_clamp() {
        let mut nav = ListNav::new();
        nav.set_count(10);
        nav.select(8);
        assert_eq!(nav.current(), 8);

        // Shrink count - should clamp
        nav.set_count(5);
        assert_eq!(nav.current(), 4);

        // Shrink to zero
        nav.set_count(0);
        assert_eq!(nav.current(), 0);
    }

    #[test]
    fn test_grid_nav_all_directions() {
        let mut nav = GridNav::new(3);
        nav.set_count(9); // 3x3 grid

        // Start at 0,0
        assert_eq!(nav.row(), 0);
        assert_eq!(nav.col(), 0);

        // Right
        nav.right();
        assert_eq!(nav.col(), 1);
        assert_eq!(nav.row(), 0);

        // Down
        nav.down();
        assert_eq!(nav.col(), 1);
        assert_eq!(nav.row(), 1);

        // Left
        nav.left();
        assert_eq!(nav.col(), 0);
        assert_eq!(nav.row(), 1);

        // Up
        nav.up();
        assert_eq!(nav.col(), 0);
        assert_eq!(nav.row(), 0);
    }

    #[test]
    fn test_grid_nav_wrap_around() {
        let mut nav = GridNav::new(3);
        nav.set_count(9); // 3x3 grid

        // Wrap left from 0,0 to last item
        nav.left();
        assert_eq!(nav.current(), 8);

        // Wrap right from last to first
        nav.right();
        assert_eq!(nav.current(), 0);

        // Wrap up from row 0 to last row
        nav.up();
        assert_eq!(nav.row(), 2);
        assert_eq!(nav.col(), 0);

        // Wrap down from last row to first row
        nav.down();
        assert_eq!(nav.row(), 0);
        assert_eq!(nav.col(), 0);
    }

    #[test]
    fn test_panel_nav_wrap_around() {
        let mut nav = PanelNav::new(3);

        // Start at 0
        assert_eq!(nav.current(), 0);

        // Wrap left from 0 to last
        nav.left();
        assert_eq!(nav.current(), 2);

        // Wrap right from last to first
        nav.right();
        assert_eq!(nav.current(), 0);

        // Normal navigation
        nav.right();
        assert_eq!(nav.current(), 1);
        nav.right();
        assert_eq!(nav.current(), 2);
        nav.left();
        assert_eq!(nav.current(), 1);
    }

    #[test]
    fn test_screen_nav_composite() {
        let mut nav = ScreenNav::new(2); // 2 panels
        nav.set_list_count(0, 5); // Panel 0 has 5 items
        nav.set_list_count(1, 3); // Panel 1 has 3 items

        // Start at panel 0, item 0
        assert_eq!(nav.current_panel(), 0);
        assert_eq!(nav.current_selection(), 0);

        // Navigate down in panel 0
        nav.navigate(NavKey::Down);
        assert_eq!(nav.current_selection(), 1);

        // Navigate right to panel 1
        nav.navigate(NavKey::Right);
        assert_eq!(nav.current_panel(), 1);
        assert_eq!(nav.current_selection(), 0); // Panel 1 starts at 0

        // Navigate down in panel 1
        nav.navigate(NavKey::Down);
        nav.navigate(NavKey::Down);
        assert_eq!(nav.current_selection(), 2);

        // Wrap down in panel 1 (only 3 items)
        nav.navigate(NavKey::Down);
        assert_eq!(nav.current_selection(), 0);

        // Navigate left wraps to panel 0
        nav.navigate(NavKey::Left);
        assert_eq!(nav.current_panel(), 0);
        assert_eq!(nav.current_selection(), 1); // Preserves panel 0's selection
    }
}
