//! # Message Bubble Component
//!
//! Enhanced message display for chat interfaces.

use iocraft::prelude::*;

use crate::tui::theme::{Borders, Icons, Spacing, Theme};
use crate::tui::types::DeliveryStatus;

/// Props for MessageBubble
#[derive(Default, Props)]
pub struct MessageBubbleProps {
    /// Sender name
    pub sender: String,
    /// Message content
    pub content: String,
    /// Timestamp string
    pub timestamp: String,
    /// Whether this is the current user's message
    pub is_own: bool,
    /// Delivery status for own messages
    pub delivery_status: DeliveryStatus,
    /// Whether this message has been finalized by consensus (A3)
    pub is_finalized: bool,
}

/// An enhanced message bubble with status indicators
#[component]
pub fn MessageBubble(props: &MessageBubbleProps) -> impl Into<AnyElement<'static>> {
    let sender = props.sender.clone();
    let content = props.content.clone();
    let timestamp = props.timestamp.clone();

    let (border_color, align) = if props.is_own {
        // Blue border for local user's messages, right-aligned
        (Theme::ACCENT, AlignItems::FlexEnd)
    } else {
        // Default border for other users' messages, left-aligned
        (Theme::BORDER, AlignItems::FlexStart)
    };

    // Status icon for own messages based on delivery status
    let status_icon = if props.is_own {
        match props.delivery_status {
            DeliveryStatus::Sending => Some((Icons::PENDING, Theme::TEXT_MUTED)),
            DeliveryStatus::Sent => Some((Icons::CHECK, Theme::TEXT_MUTED)),
            DeliveryStatus::Delivered => Some((Icons::CHECK_DOUBLE, Theme::TEXT_MUTED)), // Gray double check
            DeliveryStatus::Read => Some((Icons::CHECK_DOUBLE, Theme::INFO)), // Blue double check
            DeliveryStatus::Failed => Some((Icons::CROSS, Theme::ERROR)),
        }
    } else {
        None
    };

    // Finalization indicator (for consensus-confirmed messages)
    let finalized_icon = if props.is_finalized {
        Some((Icons::FINALIZED, Theme::TEXT_MUTED))
    } else {
        None
    };

    element! {
        View(
            align_items: align,
            margin_bottom: Spacing::XS,
        ) {
            View(
                flex_direction: FlexDirection::Column,
                max_width: 70pct,
                border_style: Borders::PRIMARY,
                border_color: border_color,
                padding_left: Spacing::PANEL_PADDING,
                padding_right: Spacing::PANEL_PADDING,
            ) {
                // Header: sender + timestamp + status indicators
                View(
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::SpaceBetween,
                    gap: Spacing::SM,
                ) {
                    Text(content: sender, weight: Weight::Bold, color: Theme::TEXT_HIGHLIGHT)
                    View(flex_direction: FlexDirection::Row, gap: Spacing::XS) {
                        Text(content: timestamp, color: Theme::TEXT_MUTED)
                        #(status_icon.map(|(icon, color)| element! {
                            Text(content: icon, color: color)
                        }))
                        #(finalized_icon.map(|(icon, color)| element! {
                            Text(content: icon, color: color)
                        }))
                    }
                }
                // Message content
                Text(content: content, wrap: TextWrap::Wrap)
            }
        }
    }
}

/// Props for CompactMessage
#[derive(Default, Props)]
pub struct CompactMessageProps {
    /// Message content
    pub content: String,
    /// Timestamp string
    pub timestamp: String,
    /// Whether this is the current user's message
    pub is_own: bool,
}

/// A compact message (no sender, for use in message groups)
#[component]
pub fn CompactMessage(props: &CompactMessageProps) -> impl Into<AnyElement<'static>> {
    let content = props.content.clone();
    let timestamp = props.timestamp.clone();

    let border_color = if props.is_own {
        // Blue border for local user's messages
        Theme::ACCENT
    } else {
        // Default border for other users' messages
        Theme::BORDER
    };

    element! {
        View(
            flex_direction: FlexDirection::Row,
            max_width: 70pct,
            border_style: Borders::PRIMARY,
            border_color: border_color,
            padding_left: Spacing::PANEL_PADDING,
            padding_right: Spacing::PANEL_PADDING,
            margin_bottom: Spacing::XS,
            gap: Spacing::SM,
        ) {
            View(flex_grow: 1.0) {
                Text(content: content, wrap: TextWrap::Wrap)
            }
            Text(content: timestamp, color: Theme::TEXT_MUTED)
        }
    }
}

/// Props for SystemMessage
#[derive(Default, Props)]
pub struct SystemMessageProps {
    /// Message content
    pub content: String,
    /// Icon to display
    pub icon: String,
}

/// A system/info message (centered, muted)
#[component]
pub fn SystemMessage(props: &SystemMessageProps) -> impl Into<AnyElement<'static>> {
    let icon = if props.icon.is_empty() {
        Icons::INFO.to_string()
    } else {
        props.icon.clone()
    };
    let content = props.content.clone();

    element! {
        View(
            justify_content: JustifyContent::Center,
            flex_direction: FlexDirection::Row,
            gap: Spacing::XS,
            margin_top: Spacing::XS,
            margin_bottom: Spacing::XS,
        ) {
            Text(content: icon, color: Theme::TEXT_MUTED)
            Text(content: content, color: Theme::TEXT_MUTED)
        }
    }
}

/// Props for MessageGroupHeader
#[derive(Default, Props)]
pub struct MessageGroupHeaderProps {
    /// Sender name
    pub sender: String,
    /// Whether these are the current user's messages
    pub is_own: bool,
}

/// A header for a group of messages from the same sender
#[component]
pub fn MessageGroupHeader(props: &MessageGroupHeaderProps) -> impl Into<AnyElement<'static>> {
    let sender = props.sender.clone();
    let align = if props.is_own {
        AlignItems::FlexEnd
    } else {
        AlignItems::FlexStart
    };

    element! {
        View(
            align_items: align,
            margin_bottom: Spacing::XS,
        ) {
            Text(content: sender, weight: Weight::Bold, color: Theme::TEXT_HIGHLIGHT)
        }
    }
}
