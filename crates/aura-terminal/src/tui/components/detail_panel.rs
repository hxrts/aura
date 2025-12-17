//! # DetailPanel Component
//!
//! A generic bordered detail panel with title and scrollable content area.
//! Extracted from the common pattern used in ContactDetail, InvitationDetail, etc.

use iocraft::prelude::*;

use crate::tui::theme::{focus_border_color, Spacing, Theme};

/// Props for DetailPanel
#[derive(Default, Props)]
pub struct DetailPanelProps<'a> {
    /// Panel title (displayed in header)
    pub title: String,
    /// Whether the panel is focused
    pub focused: bool,
    /// Content elements to display
    pub content: Vec<AnyElement<'a>>,
    /// Message to show when content is empty
    pub empty_message: String,
}

/// A bordered detail panel with title and scrollable content
///
/// This component provides the common structure for detail panels:
/// - Bordered container with focus-aware border color
/// - Bold title in the header
/// - Scrollable content area with padding
/// - Optional empty state message
///
/// ## Example
///
/// ```rust,ignore
/// element! {
///     DetailPanel(
///         title: "Details".to_string(),
///         focused: true,
///         content: vec![
///             element! { KeyValue(label: "Name".to_string(), value: name.clone()) }.into_any(),
///             element! { KeyValue(label: "Status".to_string(), value: status) }.into_any(),
///         ],
///         empty_message: "Select an item to view details".to_string(),
///     )
/// }
/// ```
#[component]
pub fn DetailPanel<'a>(props: &mut DetailPanelProps<'a>) -> impl Into<AnyElement<'a>> {
    let title = props.title.clone();
    let empty_message = if props.empty_message.is_empty() {
        "No content".to_string()
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
            // Title bar
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
                #(&mut props.content)
                #(if props.content.is_empty() {
                    Some(element! {
                        Text(content: empty_message, color: Theme::TEXT_MUTED)
                    })
                } else {
                    None
                })
            }
        }
    }
}
