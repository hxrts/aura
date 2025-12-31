//! # Styled Text Component
//!
//! Text with semantic styling for consistent UI.

use iocraft::prelude::*;

use crate::tui::theme::Theme;

/// Semantic text styles
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextStyle {
    /// Normal text
    #[default]
    Normal,
    /// Muted/secondary text
    Muted,
    /// Primary accent color
    Primary,
    /// Secondary accent color
    Secondary,
    /// Success/positive
    Success,
    /// Warning/caution
    Warning,
    /// Error/danger
    Error,
    /// Informational
    Info,
    /// Highlighted/selected
    Highlight,
    /// Label/header text
    Label,
    /// Code/monospace
    Code,
}

impl TextStyle {
    /// Get the color for this style
    #[must_use]
    pub fn color(&self) -> Color {
        match self {
            TextStyle::Normal => Theme::TEXT,
            TextStyle::Muted => Theme::TEXT_MUTED,
            TextStyle::Primary => Theme::PRIMARY,
            TextStyle::Secondary => Theme::SECONDARY,
            TextStyle::Success => Theme::SUCCESS,
            TextStyle::Warning => Theme::WARNING,
            TextStyle::Error => Theme::ERROR,
            TextStyle::Info => Theme::INFO,
            TextStyle::Highlight => Theme::TEXT_HIGHLIGHT,
            TextStyle::Label => Theme::PRIMARY,
            TextStyle::Code => Theme::SECONDARY,
        }
    }

    /// Get the weight for this style
    #[must_use]
    pub fn weight(&self) -> Weight {
        match self {
            TextStyle::Label => Weight::Bold,
            TextStyle::Primary => Weight::Bold,
            TextStyle::Error => Weight::Bold,
            TextStyle::Warning => Weight::Bold,
            _ => Weight::Normal,
        }
    }
}

/// Props for StyledText
#[derive(Default, Props)]
pub struct StyledTextProps {
    /// Text content
    pub content: String,
    /// Semantic style
    pub style: TextStyle,
    /// Override color (takes precedence over style)
    pub color: Option<Color>,
    /// Override weight (takes precedence over style)
    pub weight: Option<Weight>,
}

/// Text with semantic styling
#[component]
pub fn StyledText(props: &StyledTextProps) -> impl Into<AnyElement<'static>> {
    let content = props.content.clone();
    let color = props.color.unwrap_or_else(|| props.style.color());
    let weight = props.weight.unwrap_or_else(|| props.style.weight());

    element! {
        Text(content: content, color: color, weight: weight)
    }
}

/// Props for KeyValue display
#[derive(Default, Props)]
pub struct KeyValueProps {
    /// Label text (the "key" part)
    pub label: String,
    /// Value text
    pub value: String,
    /// Separator (default ": ")
    pub separator: String,
}

/// Key-value pair display (e.g., "Status: Active")
#[component]
pub fn KeyValue(props: &KeyValueProps) -> impl Into<AnyElement<'static>> {
    let label = props.label.clone();
    let value = props.value.clone();
    let sep = if props.separator.is_empty() {
        ": ".to_string()
    } else {
        props.separator.clone()
    };

    element! {
        View(flex_direction: FlexDirection::Row) {
            Text(content: label, color: Theme::TEXT_MUTED)
            Text(content: sep, color: Theme::TEXT_MUTED)
            Text(content: value, color: Theme::TEXT)
        }
    }
}

/// Props for Badge
#[derive(Default, Props)]
pub struct BadgeProps {
    /// Badge text
    pub text: String,
    /// Badge style
    pub style: TextStyle,
}

/// A small badge/tag with colored border
#[component]
pub fn Badge(props: &BadgeProps) -> impl Into<AnyElement<'static>> {
    let text = props.text.clone();
    let color = props.style.color();

    element! {
        View(
            padding_left: 1,
            padding_right: 1,
            border_style: BorderStyle::Round,
            border_color: color,
        ) {
            Text(content: text, color: color, weight: Weight::Bold)
        }
    }
}

/// Props for Heading
#[derive(Default, Props)]
pub struct HeadingProps {
    /// Heading text
    pub text: String,
    /// Heading level (1 = largest)
    pub level: u8,
}

/// A heading with appropriate styling
#[component]
pub fn Heading(props: &HeadingProps) -> impl Into<AnyElement<'static>> {
    let text = props.text.clone();
    let color = match props.level {
        1 => Theme::PRIMARY,
        2 => Theme::SECONDARY,
        _ => Theme::TEXT,
    };

    element! {
        View(
            margin_bottom: if props.level <= 2 { 1 } else { 0 },
        ) {
            Text(content: text, color: color, weight: Weight::Bold)
        }
    }
}

/// Props for Divider
#[derive(Default, Props)]
pub struct DividerProps {
    /// Optional label in the divider
    pub label: String,
}

/// A horizontal divider line
#[component]
pub fn Divider(props: &DividerProps) -> impl Into<AnyElement<'static>> {
    let label = props.label.clone();
    let has_label = !label.is_empty();

    element! {
        View(
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            margin_top: 1,
            margin_bottom: 1,
        ) {
            View(
                flex_grow: 1.0,
                border_style: BorderStyle::Single,
                border_edges: Edges::Bottom,
                border_color: Theme::BORDER,
                height: 1u32,
            )
            #(if has_label {
                Some(element! {
                    View(padding_left: 1, padding_right: 1) {
                        Text(content: label, color: Theme::TEXT_MUTED)
                    }
                })
            } else {
                None
            })
            #(if has_label {
                Some(element! {
                    View(
                        flex_grow: 1.0,
                        border_style: BorderStyle::Single,
                        border_edges: Edges::Bottom,
                        border_color: Theme::BORDER,
                        height: 1u32,
                    )
                })
            } else {
                None
            })
        }
    }
}
