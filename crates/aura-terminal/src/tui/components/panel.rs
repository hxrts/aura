//! # Panel Component
//!
//! A bordered container with optional title and badge.

use iocraft::prelude::*;

use crate::tui::theme::{focus_border_color, Spacing, Theme};

/// Props for Panel
#[derive(Default, Props)]
pub struct PanelProps {
    /// Panel title (displayed in header)
    pub title: String,
    /// Optional badge text (displayed next to title)
    pub badge: Option<String>,
    /// Badge color (defaults to INFO)
    pub badge_color: Option<Color>,
    /// Whether the panel is focused
    pub focused: bool,
    /// Optional background color
    pub background: Option<Color>,
    /// Content text to display
    pub content: String,
    /// Flex grow factor
    pub flex_grow: Option<f32>,
    /// Width (as percentage)
    pub width: Option<Size>,
    /// Height (as percentage)
    pub height: Option<Size>,
}

/// A bordered container with optional title and badge
///
/// Note: For complex nested content, compose View elements manually.
/// This component is for simple titled panels with text content.
#[component]
pub fn Panel(props: &PanelProps) -> impl Into<AnyElement<'static>> {
    let title = props.title.clone();
    let has_title = !title.is_empty();
    let badge = props.badge.clone();
    let badge_color = props.badge_color.unwrap_or(Theme::INFO);
    let content = props.content.clone();

    element! {
        View(
            flex_direction: FlexDirection::Column,
            border_style: BorderStyle::Round,
            border_color: focus_border_color(props.focused),
            background_color: props.background.unwrap_or(Color::Reset),
            flex_grow: props.flex_grow.unwrap_or(0.0),
            width: props.width.unwrap_or(Size::Auto),
            height: props.height.unwrap_or(Size::Auto),
        ) {
            // Title bar (only if title is present)
            #(if has_title {
                Some(element! {
                    View(
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        border_style: BorderStyle::Single,
                        border_edges: Edges::Bottom,
                        border_color: Theme::BORDER,
                        padding_left: Spacing::PANEL_PADDING,
                        padding_right: Spacing::PANEL_PADDING,
                        gap: Spacing::SM,
                    ) {
                        Text(content: title, weight: Weight::Bold, color: Theme::PRIMARY)
                        #(badge.map(|b| element! {
                            View(
                                padding_left: 1,
                                padding_right: 1,
                                border_style: BorderStyle::Round,
                                border_color: badge_color,
                            ) {
                                Text(content: b, color: badge_color, weight: Weight::Bold)
                            }
                        }))
                    }
                })
            } else {
                None
            })
            // Content area
            View(
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                padding: Spacing::PANEL_PADDING,
            ) {
                Text(content: content)
            }
        }
    }
}

/// Helper to create a panel layout without content (for use as a container pattern)
/// Returns style properties to apply to your own View
pub struct PanelStyle;

impl PanelStyle {
    /// Get border color based on focus state
    ///
    /// Delegates to `theme::focus_border_color` for consistency.
    #[inline]
    pub fn border_color(focused: bool) -> Color {
        focus_border_color(focused)
    }
}
