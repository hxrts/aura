//! # Toast Component
//!
//! Notification toast messages

use iocraft::prelude::*;

use crate::tui::layout::dim;
use crate::tui::theme::Theme;

/// Toast severity level
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToastLevel {
    #[default]
    Info,
    Success,
    Warning,
    Error,
    /// Special level for conflict notifications (operations rolled back due to conflicts)
    Conflict,
}

impl ToastLevel {
    pub fn icon(self) -> &'static str {
        match self {
            Self::Info => "ℹ",
            Self::Success => "✓",
            Self::Warning => "⚠",
            Self::Error => "✗",
            Self::Conflict => "⇄", // Two-way arrows for conflict
        }
    }

    /// Alias for icon() - returns the indicator symbol for this level
    pub fn indicator(self) -> &'static str {
        self.icon()
    }

    pub fn color(self) -> Color {
        match self {
            Self::Info => Theme::SECONDARY,
            Self::Success => Theme::SUCCESS,
            Self::Warning => Theme::WARNING,
            Self::Error => Theme::ERROR,
            Self::Conflict => Theme::WARNING, // Use warning color for conflicts
        }
    }

    /// Whether this level represents a conflict that requires user attention
    pub fn is_conflict(self) -> bool {
        matches!(self, Self::Conflict)
    }
}

/// A toast message
#[derive(Clone, Debug, Default)]
pub struct ToastMessage {
    pub id: String,
    pub message: String,
    pub level: ToastLevel,
}

impl ToastMessage {
    pub fn new(id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            message: message.into(),
            level: ToastLevel::Info,
        }
    }

    pub fn with_level(mut self, level: ToastLevel) -> Self {
        self.level = level;
        self
    }

    pub fn info(id: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(id, message).with_level(ToastLevel::Info)
    }

    pub fn success(id: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(id, message).with_level(ToastLevel::Success)
    }

    pub fn warning(id: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(id, message).with_level(ToastLevel::Warning)
    }

    pub fn error(id: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(id, message).with_level(ToastLevel::Error)
    }

    /// Create a conflict notification toast
    ///
    /// Used when an optimistic operation is rolled back due to a conflict
    /// with another concurrent operation.
    pub fn conflict(id: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(id, message).with_level(ToastLevel::Conflict)
    }

    /// Check if this toast is an error level toast
    pub fn is_error(&self) -> bool {
        matches!(self.level, ToastLevel::Error)
    }

    /// Check if this toast is a conflict notification
    pub fn is_conflict(&self) -> bool {
        matches!(self.level, ToastLevel::Conflict)
    }
}

/// Props for Toast
#[derive(Default, Props)]
pub struct ToastProps {
    pub message: String,
    pub level: ToastLevel,
}

/// A single toast notification
#[component]
pub fn Toast(props: &ToastProps) -> impl Into<AnyElement<'static>> {
    let icon = props.level.icon().to_string();
    let color = props.level.color();
    let message = props.message.clone();

    element! {
        View(
            flex_direction: FlexDirection::Row,
            gap: 1,
            padding_left: 1,
            padding_right: 1,
            border_style: BorderStyle::Round,
            border_color: color,
        ) {
            Text(content: icon, color: color)
            Text(content: message, color: Theme::TEXT)
        }
    }
}

// =============================================================================
// Toast Frame - Absolute positioning wrapper
// =============================================================================

/// Props for ToastFrame
#[derive(Default, Props)]
pub struct ToastFrameProps<'a> {
    /// Child content (the toast body)
    pub children: Vec<AnyElement<'a>>,
}

/// Unified toast frame that positions content in the footer overlay region.
///
/// This component handles the absolute positioning so that all toasts
/// appear at exactly the same location: overlaying the footer (rows 28-30).
///
/// Use this as the outermost wrapper for toast components.
/// Children should use `ToastContent` as their root element.
#[component]
pub fn ToastFrame<'a>(props: &mut ToastFrameProps<'a>) -> impl Into<AnyElement<'a>> {
    element! {
        View(
            position: Position::Absolute,
            top: dim::NAV_HEIGHT + dim::MIDDLE_HEIGHT,  // Row 28 (footer start)
            left: 0u16,
            width: dim::TOTAL_WIDTH,
            height: dim::FOOTER_HEIGHT,  // Same height as footer (3 rows)
            overflow: Overflow::Hidden,
        ) {
            #(&mut props.children)
        }
    }
}

// =============================================================================
// Toast Content - Compile-time safe toast body wrapper
// =============================================================================

/// Props for ToastContent - intentionally does NOT include position props
/// to prevent nested absolute positioning bugs at compile time.
///
/// Use this as the root element for all toast body content.
#[derive(Default, Props)]
pub struct ToastContentProps<'a> {
    /// Child content
    pub children: Vec<AnyElement<'a>>,
    /// Flex direction for content layout (defaults to Row)
    pub flex_direction: FlexDirection,
    /// Background color (defaults to BG_MODAL)
    pub background_color: Option<Color>,
    /// Border style
    pub border_style: BorderStyle,
    /// Border color (required - typically based on toast level)
    pub border_color: Option<Color>,
    /// Align items (defaults to Center for toasts)
    pub align_items: Option<AlignItems>,
    /// Gap between items
    pub gap: Option<u16>,
    /// Padding left
    pub padding_left: Option<u16>,
    /// Padding right
    pub padding_right: Option<u16>,
}

/// Toast content wrapper that fills its ToastFrame container.
///
/// **IMPORTANT**: Use this component as the root element for ALL toast bodies.
/// It automatically handles sizing (100% width and height) and prevents
/// the nested absolute positioning bug at compile time by not exposing
/// position props.
///
/// # Example
/// ```ignore
/// element! {
///     ToastFrame {
///         ToastContent(
///             flex_direction: FlexDirection::Row,
///             border_style: BorderStyle::Round,
///             border_color: Some(Theme::SUCCESS),
///             align_items: Some(AlignItems::Center),
///         ) {
///             // Your toast content here
///         }
///     }
/// }
/// ```
#[component]
pub fn ToastContent<'a>(props: &mut ToastContentProps<'a>) -> impl Into<AnyElement<'a>> {
    let bg = props.background_color.unwrap_or(Theme::BG_MODAL);
    let border = props.border_color.unwrap_or(Theme::BORDER_FOCUS);
    let align = props.align_items.unwrap_or(AlignItems::Center);
    let gap = props.gap.unwrap_or(1);
    let pad_left = props.padding_left.unwrap_or(1);
    let pad_right = props.padding_right.unwrap_or(1);

    element! {
        View(
            // Size is always 100% - this is enforced, not configurable
            width: 100pct,
            height: 100pct,
            // Layout props
            flex_direction: props.flex_direction,
            align_items: align,
            gap: gap,
            padding_left: pad_left,
            padding_right: pad_right,
            // Styling props
            background_color: bg,
            border_style: props.border_style,
            border_color: border,
            overflow: Overflow::Hidden,
        ) {
            #(&mut props.children)
        }
    }
}

// =============================================================================
// Toast Container - Main toast display component
// =============================================================================

/// Props for ToastContainer
#[derive(Default, Props)]
pub struct ToastContainerProps {
    pub toasts: Vec<ToastMessage>,
}

/// Toast notification overlay that appears over the footer.
///
/// This component is rendered conditionally in app.rs when toasts are active.
/// It uses ToastFrame for positioning and ToastContent for styling,
/// ensuring compile-time safety against positioning bugs.
#[component]
pub fn ToastContainer(props: &ToastContainerProps) -> impl Into<AnyElement<'static>> {
    let toasts = props.toasts.clone();

    // Show only the most recent toast
    let toast = match toasts.last() {
        Some(t) => t,
        None => {
            // Return empty element - no toasts to show
            return element! { View {} }.into_any();
        }
    };
    let icon = toast.level.icon().to_string();
    let color = toast.level.color();

    // Truncate long messages to fit in footer width
    let max_msg_len = 60; // Leave room for icon and dismiss hint
    let message = if toast.message.len() > max_msg_len {
        format!("{}...", &toast.message[..max_msg_len - 3])
    } else {
        toast.message.clone()
    };

    // Use ToastFrame for positioning, ToastContent for styling
    element! {
        ToastFrame {
            ToastContent(
                flex_direction: FlexDirection::Row,
                border_style: BorderStyle::Round,
                border_color: Some(color),
                align_items: Some(AlignItems::Center),
            ) {
                Text(content: icon, color: color, weight: Weight::Bold, wrap: TextWrap::NoWrap)
                View(flex_grow: 1.0) {
                    Text(content: message, color: Theme::TEXT, wrap: TextWrap::NoWrap)
                }
                Text(content: "[Esc] dismiss", color: Theme::TEXT_MUTED, wrap: TextWrap::NoWrap)
            }
        }
    }
    .into_any()
}

/// Props for StatusBar
#[derive(Default, Props)]
pub struct StatusBarProps {
    pub message: String,
    pub level: ToastLevel,
    pub visible: bool,
}

/// A status bar notification (bottom of screen)
#[component]
pub fn StatusBar(props: &StatusBarProps) -> impl Into<AnyElement<'static>> {
    if !props.visible {
        return element! {
            View {}
        };
    }

    let icon = props.level.icon().to_string();
    let color = props.level.color();
    let message = props.message.clone();

    element! {
        View(
            flex_direction: FlexDirection::Row,
            gap: 1,
            padding_left: 1,
            padding_right: 1,
            border_style: BorderStyle::Round,
            border_color: color,
        ) {
            Text(content: icon, color: color)
            Text(content: message, color: Theme::TEXT)
        }
    }
}
