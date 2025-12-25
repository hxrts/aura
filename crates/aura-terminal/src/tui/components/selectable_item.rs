//! # SelectableItem Component
//!
//! Provides consistent selection styling across list items.
//! Used by Settings, Contacts, and other screens with selectable lists.

use iocraft::prelude::*;

use crate::tui::theme::{Icons, Theme};

/// Props for SimpleSelectableItem (text-only variant)
#[derive(Default, Props)]
pub struct SimpleSelectableItemProps {
    /// The text label to display
    pub label: String,
    /// Whether this item is currently selected
    pub selected: bool,
}

/// A simple text-only selectable item
///
/// This component provides consistent selection styling:
/// - Selection indicator (● colored circle when selected, space otherwise)
/// - Background color based on selection state
/// - Text color based on selection state
/// - Consistent padding
///
/// For more complex content (with icons, badges, etc.), implement the same
/// pattern directly in your component using `Theme::LIST_BG_SELECTED`,
/// `Theme::LIST_TEXT_SELECTED`, etc.
///
/// ## Example
///
/// ```rust,ignore
/// element! {
///     SimpleSelectableItem(
///         label: "Profile".to_string(),
///         selected: current_section == Section::Profile,
///     )
/// }
/// ```
#[component]
pub fn SimpleSelectableItem(props: &SimpleSelectableItemProps) -> impl Into<AnyElement<'static>> {
    let bg = if props.selected {
        Theme::LIST_BG_SELECTED
    } else {
        Theme::LIST_BG_NORMAL
    };
    let fg = if props.selected {
        Theme::LIST_TEXT_SELECTED
    } else {
        Theme::LIST_TEXT_NORMAL
    };

    element! {
        View(
            flex_direction: FlexDirection::Row,
            background_color: bg,
            padding_left: 1,
            padding_right: 1,
        ) {
            // Selection indicator: colored circle when selected, space otherwise
            #(if props.selected {
                Some(element! {
                    Text(content: format!("{} ", Icons::ONLINE), color: Theme::PRIMARY)
                })
            } else {
                Some(element! {
                    Text(content: "  ", color: fg)
                })
            })
            Text(content: props.label.clone(), color: fg)
        }
    }
}
