//! # Key Hints Component
//!
//! Displays keyboard shortcuts at the bottom of the screen.
//!
//! Layout:
//! - Top row(s): Screen-specific hints in a 4-column grid (max 4 per row)
//! - Bottom row: Global navigation hints (Tab/S-Tab, arrows, q Quit, ? Help)

use iocraft::prelude::*;

use crate::tui::theme::Theme;
use crate::tui::types::KeyHint;

/// Props for KeyHintsBar
#[derive(Default, Props)]
pub struct KeyHintsBarProps {
    /// Screen-specific hints (varies by screen)
    pub screen_hints: Vec<KeyHint>,
}

/// A single hint item component with fixed width for 4-column grid layout
#[component]
fn GridHintItem(props: &GridHintItemProps) -> impl Into<AnyElement<'static>> {
    let key = props.key_name.clone();
    let desc = props.description.clone();

    element! {
        View(flex_direction: FlexDirection::Row, width: 25pct) {
            Text(content: key, weight: Weight::Bold)
            Text(content: " ")
            Text(content: desc, color: Theme::TEXT_MUTED)
        }
    }
}

#[derive(Default, Props)]
struct GridHintItemProps {
    key_name: String,
    description: String,
}

/// A single hint item component for the global row (fixed width for 4-column grid)
#[component]
fn GlobalHintItem(props: &GlobalHintItemProps) -> impl Into<AnyElement<'static>> {
    let key = props.key_name.clone();
    let desc = props.description.clone();

    element! {
        View(flex_direction: FlexDirection::Row, width: 25pct) {
            Text(content: key, weight: Weight::Bold)
            Text(content: " ")
            Text(content: desc, color: Theme::TEXT_MUTED)
        }
    }
}

#[derive(Default, Props)]
struct GlobalHintItemProps {
    key_name: String,
    description: String,
}

/// A bar showing keyboard shortcut hints
///
/// Layout:
/// - Screen-specific hints in 4-column grid rows (max 4 per row)
/// - Global navigation hints always at bottom in fixed order
#[component]
pub fn KeyHintsBar(props: &KeyHintsBarProps) -> impl Into<AnyElement<'static>> {
    // Build screen hints rows (4 columns each, max 4 per row)
    let screen_rows: Vec<Vec<&KeyHint>> = props
        .screen_hints
        .chunks(4)
        .map(|chunk| chunk.iter().collect())
        .collect();

    // Build global hints (always in this fixed order)
    let global_hints = [
        KeyHint::new("Tab/S-Tab", "Screen"),
        KeyHint::new("↑↓←→", "Navigate"),
        KeyHint::new("q", "Quit"),
        KeyHint::new("?", "Help"),
    ];

    element! {
        View(
            flex_direction: FlexDirection::Column,
            width: 100pct,
            flex_shrink: 0.0,  // Don't shrink - hints bar takes priority
            border_style: BorderStyle::Single,
            border_edges: Edges::Top,
            border_color: Theme::BORDER,
        ) {
            // Screen-specific hints (4-column grid, variable rows)
            #(screen_rows.iter().map(|row| element! {
                View(flex_direction: FlexDirection::Row, width: 100pct, padding_left: 1, padding_right: 1) {
                    #(row.iter().map(|hint| element! {
                        GridHintItem(
                            key_name: hint.key.clone(),
                            description: hint.description.clone(),
                        )
                    }))
                }
            }))

            // Global navigation hints (always at bottom, fixed order)
            View(
                flex_direction: FlexDirection::Row,
                width: 100pct,
                padding_left: 1,
                padding_right: 1,
            ) {
                #(global_hints.iter().map(|hint| element! {
                    GlobalHintItem(
                        key_name: hint.key.clone(),
                        description: hint.description.clone(),
                    )
                }))
            }
        }
    }
}
