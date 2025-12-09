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
use std::time::{Duration, Instant};

/// Navigation throttle duration to prevent too-rapid key repeats
pub const NAV_THROTTLE_MS: u64 = 150;

/// Input throttle duration for text input (slightly faster than navigation)
pub const INPUT_THROTTLE_MS: u64 = 50;

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

/// Throttle helper for navigation
pub struct NavThrottle {
    last_nav: Instant,
    duration: Duration,
}

impl Default for NavThrottle {
    fn default() -> Self {
        Self::new()
    }
}

impl NavThrottle {
    /// Create a new navigation throttle
    pub fn new() -> Self {
        Self {
            // Start in the past so first navigation is immediate
            last_nav: Instant::now() - Duration::from_millis(NAV_THROTTLE_MS + 100),
            duration: Duration::from_millis(NAV_THROTTLE_MS),
        }
    }

    /// Check if enough time has passed for another navigation
    pub fn can_navigate(&self) -> bool {
        self.last_nav.elapsed() >= self.duration
    }

    /// Mark that navigation occurred
    pub fn mark(&mut self) {
        self.last_nav = Instant::now();
    }

    /// Check and mark in one call - returns true if navigation is allowed
    pub fn try_navigate(&mut self) -> bool {
        if self.can_navigate() {
            self.mark();
            true
        } else {
            false
        }
    }
}

/// Throttle helper for text input to prevent too-rapid key repeats
pub struct InputThrottle {
    last_input: Instant,
    duration: Duration,
}

impl Default for InputThrottle {
    fn default() -> Self {
        Self::new()
    }
}

impl InputThrottle {
    /// Create a new input throttle
    pub fn new() -> Self {
        Self {
            // Start in the past so first input is immediate
            last_input: Instant::now() - Duration::from_millis(INPUT_THROTTLE_MS + 100),
            duration: Duration::from_millis(INPUT_THROTTLE_MS),
        }
    }

    /// Check if enough time has passed for another input
    pub fn can_input(&self) -> bool {
        self.last_input.elapsed() >= self.duration
    }

    /// Mark that input occurred
    pub fn mark(&mut self) {
        self.last_input = Instant::now();
    }

    /// Check and mark in one call - returns true if input is allowed
    pub fn try_input(&mut self) -> bool {
        if self.can_input() {
            self.mark();
            true
        } else {
            false
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
    let rows = (total + cols - 1) / cols; // ceiling division
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
    fn test_nav_throttle() {
        let mut throttle = NavThrottle::new();
        assert!(throttle.try_navigate()); // First is always allowed
        assert!(!throttle.try_navigate()); // Immediate second is blocked
    }
}
