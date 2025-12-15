//! Footer component with fixed 3-row layout.
//!
//! The footer occupies the bottom 3 rows of the TUI and contains:
//! - Row 1: Top border/separator
//! - Row 2-3: Key hints (fixed 2 rows)

use crate::tui::layout::dim;
use crate::tui::theme::{Spacing, Theme};
use crate::tui::types::KeyHint;
use iocraft::prelude::*;

/// Props for Footer
#[derive(Default, Props)]
pub struct FooterProps {
    /// Key hints to display
    pub hints: Vec<KeyHint>,
    /// Whether the footer is disabled (darkened, indicating hotkeys are inactive)
    pub disabled: bool,
}

/// Fixed 3-row footer for the TUI.
///
/// Layout:
/// ```text
/// ├──────────────────────────────────────────────────────────────────────────────┤ Row 1: Border
/// │ [Esc] back  [Enter] select  [Tab] next  [↑↓] navigate                        │ Row 2: Hints
/// │                                                                              │ Row 3: Hints cont.
/// └──────────────────────────────────────────────────────────────────────────────┘
/// ```
#[component]
pub fn Footer(props: &FooterProps) -> impl Into<AnyElement<'static>> {
    // Format hints into a single line or wrap if needed
    let hints_text: Vec<String> = props
        .hints
        .iter()
        .map(|h| format!("[{}] {}", h.key, h.description))
        .collect();

    // Use darker colors when disabled (insert mode active)
    let border_color = if props.disabled {
        Theme::BG_DARK
    } else {
        Theme::BORDER
    };
    let text_color = if props.disabled {
        Theme::TEXT_DISABLED
    } else {
        Theme::TEXT_MUTED
    };

    element! {
        View(
            width: dim::TOTAL_WIDTH,
            height: dim::FOOTER_HEIGHT,
            flex_direction: FlexDirection::Column,
            overflow: Overflow::Hidden,
        ) {
            // Row 1: Top border (1 row)
            View(
                width: 100pct,
                height: dim::FOOTER_BORDER_HEIGHT,
                border_style: BorderStyle::Single,
                border_edges: Edges::Top,
                border_color: border_color,
            )

            // Row 2-3: Key hints (2 rows)
            View(
                width: 100pct,
                height: dim::KEY_HINTS_HEIGHT,
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                gap: Spacing::MD,
                padding_left: Spacing::SM,
                padding_right: Spacing::SM,
                overflow: Overflow::Hidden,
            ) {
                #(hints_text.iter().map(|hint| {
                    let color = text_color;
                    element! {
                        Text(content: hint.clone(), color: color)
                    }
                }))
            }
        }
    }
}

/// Footer with no hints (empty/minimal)
#[component]
pub fn EmptyFooter() -> impl Into<AnyElement<'static>> {
    element! {
        View(
            width: dim::TOTAL_WIDTH,
            height: dim::FOOTER_HEIGHT,
            flex_direction: FlexDirection::Column,
            overflow: Overflow::Hidden,
        ) {
            // Row 1: Top border
            View(
                width: 100pct,
                height: dim::FOOTER_BORDER_HEIGHT,
                border_style: BorderStyle::Single,
                border_edges: Edges::Top,
                border_color: Theme::BORDER,
            )

            // Row 2-3: Empty space
            View(
                width: 100pct,
                height: dim::KEY_HINTS_HEIGHT,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_footer_dimensions() {
        // Footer should be exactly 3 rows (1 border + 2 hints)
        assert_eq!(dim::FOOTER_HEIGHT, 3);
        assert_eq!(dim::FOOTER_BORDER_HEIGHT, 1);
        assert_eq!(dim::KEY_HINTS_HEIGHT, 2);
    }

    #[test]
    fn test_key_hint_formatting() {
        let hints = vec![KeyHint::new("Esc", "back"), KeyHint::new("Enter", "select")];

        let formatted: Vec<String> = hints
            .iter()
            .map(|h| format!("[{}] {}", h.key, h.description))
            .collect();

        assert_eq!(formatted[0], "[Esc] back");
        assert_eq!(formatted[1], "[Enter] select");
    }
}
