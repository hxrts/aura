//! # Card Component
//!
//! A selectable container for list items.

use iocraft::prelude::*;

use crate::tui::theme::{Spacing, Theme};

/// Card style helper for applying card styling to custom layouts
pub struct CardStyle;

impl CardStyle {
    /// Get background and border colors based on selection/focus state
    /// Background is always transparent (Color::Reset) - use borders for selection indication
    pub fn colors(selected: bool, focused: bool) -> (Color, Color) {
        let border = match (selected, focused) {
            (true, true) => Theme::PRIMARY,
            (true, false) => Theme::SECONDARY,
            (false, true) => Theme::BORDER_FOCUS,
            (false, false) => Theme::BORDER,
        };
        (Color::Reset, border)
    }

    /// Get text color based on selection state
    pub fn text_color(selected: bool) -> Color {
        if selected {
            Theme::LIST_TEXT_SELECTED
        } else {
            Theme::LIST_TEXT_NORMAL
        }
    }
}

/// Props for CardHeader
#[derive(Default, Props)]
pub struct CardHeaderProps {
    /// Title text
    pub title: String,
    /// Optional subtitle
    pub subtitle: String,
    /// Optional right-side content (e.g., status indicator)
    pub trailing: String,
    /// Trailing content color
    pub trailing_color: Option<Color>,
    /// Whether the card is selected
    pub selected: bool,
    /// Whether the card is focused
    pub focused: bool,
    /// Whether to show a border
    pub bordered: bool,
}

/// A styled card header with title, subtitle, and trailing text
#[component]
pub fn CardHeader(props: &CardHeaderProps) -> impl Into<AnyElement<'static>> {
    let title = props.title.clone();
    let subtitle = props.subtitle.clone();
    let trailing = props.trailing.clone();
    let has_subtitle = !subtitle.is_empty();
    let has_trailing = !trailing.is_empty();

    let (bg, border_color) = CardStyle::colors(props.selected, props.focused);
    let text_color = CardStyle::text_color(props.selected);
    let subtitle_color = if props.selected {
        Theme::LIST_TEXT_SELECTED
    } else {
        Theme::LIST_TEXT_MUTED
    };
    let trailing_color = props.trailing_color.unwrap_or(subtitle_color);

    element! {
        View(
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceBetween,
            background_color: bg,
            border_style: if props.bordered { BorderStyle::Round } else { BorderStyle::None },
            border_color: border_color,
            padding: Spacing::LIST_ITEM_PADDING,
            margin_bottom: Spacing::XS,
        ) {
            View(flex_direction: FlexDirection::Column, flex_shrink: 1.0) {
                Text(content: title, weight: Weight::Bold, color: text_color)
                #(if has_subtitle {
                    Some(element! {
                        Text(content: subtitle, color: subtitle_color)
                    })
                } else {
                    None
                })
            }
            #(if has_trailing {
                Some(element! {
                    Text(content: trailing, color: trailing_color)
                })
            } else {
                None
            })
        }
    }
}

/// Props for SimpleCard
#[derive(Default, Props)]
pub struct SimpleCardProps {
    /// Card content text
    pub content: String,
    /// Whether the card is selected
    pub selected: bool,
    /// Whether the card is focused
    pub focused: bool,
    /// Whether to show a border
    pub bordered: bool,
}

/// A simple card with just content text
#[component]
pub fn SimpleCard(props: &SimpleCardProps) -> impl Into<AnyElement<'static>> {
    let content = props.content.clone();
    let (bg, border_color) = CardStyle::colors(props.selected, props.focused);

    element! {
        View(
            flex_direction: FlexDirection::Column,
            background_color: bg,
            border_style: if props.bordered { BorderStyle::Round } else { BorderStyle::None },
            border_color: border_color,
            padding: Spacing::LIST_ITEM_PADDING,
            margin_bottom: Spacing::XS,
        ) {
            Text(content: content)
        }
    }
}

/// Props for CardFooter
#[derive(Default, Props)]
pub struct CardFooterProps {
    /// Footer text (muted)
    pub text: String,
}

/// Footer section for a card (to be used inside a View)
#[component]
pub fn CardFooter(props: &CardFooterProps) -> impl Into<AnyElement<'static>> {
    let text = props.text.clone();

    element! {
        View(
            margin_top: Spacing::XS,
            border_style: BorderStyle::Single,
            border_edges: Edges::Top,
            border_color: Theme::BORDER,
            padding_top: Spacing::XS,
        ) {
            Text(content: text, color: Theme::TEXT_MUTED)
        }
    }
}
