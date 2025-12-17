//! # ListPanel Component
//!
//! A generic bordered list panel with title, count badge, and scrollable content.
//! Extracted from the common pattern used in ContactList, InvitationList, etc.

use iocraft::prelude::*;

use crate::tui::components::EmptyState;
use crate::tui::theme::{focus_border_color, Spacing, Theme};

/// Props for ListPanel
#[derive(Default, Props)]
pub struct ListPanelProps<'a> {
    /// Panel title (displayed in header)
    pub title: String,
    /// Item count (displayed in parentheses after title)
    pub count: usize,
    /// Whether the panel is focused
    pub focused: bool,
    /// List item elements to display
    pub items: Vec<AnyElement<'a>>,
    /// Message to show when items is empty
    pub empty_message: String,
}

/// A bordered list panel with title, count, and scrollable items
///
/// This component provides the common structure for list panels:
/// - Bordered container with focus-aware border color
/// - Title with item count in header
/// - Scrollable content area with padding
/// - EmptyState component when no items
///
/// ## Example
///
/// ```rust,ignore
/// let items: Vec<AnyElement<'static>> = contacts
///     .iter()
///     .enumerate()
///     .map(|(idx, c)| {
///         element! {
///             View(key: c.id.clone()) {
///                 ContactItem(contact: c.clone(), is_selected: idx == selected)
///             }
///         }.into_any()
///     })
///     .collect();
///
/// element! {
///     ListPanel(
///         title: "Contacts".to_string(),
///         count: contacts.len(),
///         focused: true,
///         items: items,
///         empty_message: "No contacts yet".to_string(),
///     )
/// }
/// ```
#[component]
pub fn ListPanel<'a>(props: &mut ListPanelProps<'a>) -> impl Into<AnyElement<'a>> {
    let title = format!("{} ({})", props.title, props.count);
    let empty_message = if props.empty_message.is_empty() {
        "No items".to_string()
    } else {
        props.empty_message.clone()
    };

    element! {
        View(
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            flex_shrink: 1.0,
            border_style: BorderStyle::Round,
            border_color: focus_border_color(props.focused),
            overflow: Overflow::Hidden,
        ) {
            // Title bar with count
            View(padding_left: Spacing::PANEL_PADDING) {
                Text(content: title, weight: Weight::Bold, color: Theme::PRIMARY)
            }
            // Scrollable content area
            View(
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                padding: Spacing::PANEL_PADDING,
                overflow: Overflow::Scroll,
            ) {
                #(&mut props.items)
                #(if props.items.is_empty() {
                    Some(element! {
                        View {
                            EmptyState(title: empty_message)
                        }
                    })
                } else {
                    None
                })
            }
        }
    }
}
