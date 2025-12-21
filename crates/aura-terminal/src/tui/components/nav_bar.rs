//! Nav bar component with fixed 2-row layout.
//!
//! The nav bar occupies the top 2 rows of the TUI and contains:
//! - Row 1: Screen tabs (6 items in fixed-width columns, left-justified)
//! - Row 2: Bottom border/separator

use crate::tui::layout::dim;
use crate::tui::screens::Screen;
use crate::tui::theme::{Spacing, Theme};
use iocraft::prelude::*;

/// Width of each nav column (6 columns across 80 chars)
const NAV_COL_WIDTH: u16 = dim::TOTAL_WIDTH / 6; // 13 chars each

/// Props for NavBar
#[derive(Default, Props)]
pub struct NavBarProps {
    /// Currently active screen
    pub active_screen: Screen,
}

/// Fixed 2-row nav bar for the TUI.
///
/// Layout:
/// ```text
/// │ Block   Chat   Contacts   Neighborhood   Recovery   Settings │ Row 1: Tabs
/// ├──────────────────────────────────────────────────────────────┤ Row 2: Border
/// ```
#[component]
pub fn NavBar(props: &NavBarProps) -> impl Into<AnyElement<'static>> {
    let active = props.active_screen;

    element! {
        View(
            width: dim::TOTAL_WIDTH,
            height: dim::NAV_HEIGHT,
            flex_direction: FlexDirection::Column,
            overflow: Overflow::Hidden,
        ) {
            // Row 1: Screen tabs in fixed-width columns, left-justified
            View(
                width: 100pct,
                height: 1,
                flex_direction: FlexDirection::Row,
                padding_left: Spacing::SM,
            ) {
                #(Screen::all().iter().map(|&screen| {
                    let is_active = screen == active;
                    let color = if is_active { Theme::PRIMARY } else { Theme::TEXT_MUTED };
                    let weight = if is_active { Weight::Bold } else { Weight::Normal };
                    let title = screen.name().to_string();
                    element! {
                        View(
                            width: NAV_COL_WIDTH,
                            height: 1,
                        ) {
                            Text(content: title, color: color, weight: weight)
                        }
                    }
                }))
            }

            // Row 2: Bottom border (1 row)
            View(
                width: 100pct,
                height: 1,
                border_style: BorderStyle::Single,
                border_edges: Edges::Top,
                border_color: Theme::BORDER,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nav_bar_dimensions() {
        // Nav bar should be exactly 2 rows (tabs + border)
        assert_eq!(dim::NAV_HEIGHT, 2);
    }
}
