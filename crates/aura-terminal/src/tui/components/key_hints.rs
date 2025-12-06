//! # Key Hints Component
//!
//! Displays keyboard shortcuts at the bottom of the screen.

use iocraft::prelude::*;

use crate::tui::theme::Theme;
use crate::tui::types::KeyHint;

/// Props for KeyHintsBar
#[derive(Default, Props)]
pub struct KeyHintsBarProps {
    /// The key hints to display
    pub hints: Vec<KeyHint>,
}

/// A bar showing keyboard shortcut hints
#[component]
pub fn KeyHintsBar(props: &KeyHintsBarProps) -> impl Into<AnyElement<'static>> {
    // Split hints into rows of max 4 items each
    let rows: Vec<Vec<&KeyHint>> = props.hints.chunks(4).map(|chunk| chunk.iter().collect()).collect();

    element! {
        View(
            flex_direction: FlexDirection::Column,
            width: 100pct,
            overflow: Overflow::Hidden,
        ) {
            #(rows.iter().map(|row| element! {
                View(
                    flex_direction: FlexDirection::Row,
                    gap: 3,
                    padding_left: 1,
                    padding_right: 1,
                ) {
                    #(row.iter().map(|hint| element! {
                        View(flex_direction: FlexDirection::Row, gap: 1) {
                            Text(content: hint.key.clone(), weight: Weight::Bold)
                            Text(content: hint.description.clone(), color: Theme::TEXT_MUTED)
                        }
                    }))
                }
            }))
        }
    }
}
