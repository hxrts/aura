//! # Message Panel Component
//!
//! Shared message list rendering for chat-like screens.
//!
//! ## Features
//!
//! - **Breadcrumb title**: Displays a breadcrumb path (e.g., "Home › Channel › # general")
//! - **Programmatic scrolling**: Accepts `scroll_offset` for external scroll control
//! - **Scroll indicators**: Shows ▲/▼ indicators when content is scrollable
//!
//! ## Usage
//!
//! Both chat and neighborhood screens use this component:
//! - Chat: title = "Messages"
//! - Neighborhood: title = breadcrumb path

use iocraft::prelude::*;

use crate::tui::components::MessageBubble;
use crate::tui::theme::{Spacing, Theme};
use crate::tui::types::Message;

/// Visible message window height (messages that fit on screen)
const VISIBLE_MESSAGE_ROWS: usize = 18;

/// Props for MessagePanel
#[derive(Default, Props)]
pub struct MessagePanelProps {
    /// Messages to display
    pub messages: Vec<Message>,
    /// Title or breadcrumb path (e.g., "Home › Interior › # general")
    pub title: Option<String>,
    /// Message to show when there are no messages
    pub empty_message: Option<String>,
    /// Scroll offset (0 = bottom, higher = scrolled up)
    /// The offset represents how many messages up from the bottom we've scrolled
    pub scroll_offset: usize,
    /// Total message count (used for scroll calculations)
    pub message_count: usize,
}

/// A shared, scrollable message list panel with breadcrumb support
#[component]
pub fn MessagePanel(props: &MessagePanelProps) -> impl Into<AnyElement<'static>> {
    let messages = props.messages.clone();
    let title = props.title.clone().filter(|value| !value.is_empty());
    let empty_message = props
        .empty_message
        .clone()
        .filter(|value| !value.is_empty());
    let scroll_offset = props.scroll_offset;
    let message_count = props.message_count;

    let panel_padding = Spacing::PANEL_PADDING;
    let padding_top = if title.is_some() { 0 } else { panel_padding };

    // Calculate scroll indicators
    let can_scroll_up = scroll_offset > 0;
    let can_scroll_down = message_count > VISIBLE_MESSAGE_ROWS
        && scroll_offset < message_count.saturating_sub(VISIBLE_MESSAGE_ROWS);

    // Build scroll indicator suffix for title
    let scroll_indicator = if can_scroll_up && can_scroll_down {
        " ▲▼"
    } else if can_scroll_up {
        " ▲"
    } else if can_scroll_down {
        " ▼"
    } else {
        ""
    };

    element! {
        View(
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            border_style: BorderStyle::Round,
            border_color: Theme::BORDER,
            padding_left: panel_padding,
            padding_right: panel_padding,
            padding_bottom: panel_padding,
            padding_top: padding_top,
            overflow: Overflow::Hidden,
        ) {
            #(title.map(|title| {
                let indicator = scroll_indicator.to_string();
                element! {
                    View(padding_bottom: 1, flex_direction: FlexDirection::Row) {
                        Text(content: title, weight: Weight::Bold, color: Theme::PRIMARY)
                        Text(content: indicator, color: Theme::TEXT_MUTED)
                    }
                }
            }))
            View(
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                overflow: Overflow::Hidden,
            ) {
                #(if messages.is_empty() {
                    empty_message
                        .map(|text| {
                            vec![element! {
                                View { Text(content: text, color: Theme::TEXT_MUTED) }
                            }
                            .into_any()]
                        })
                        .unwrap_or_default()
                } else {
                    // Calculate visible range based on scroll offset
                    // scroll_offset = 0 means we show the latest messages (bottom)
                    // Higher offset means we're scrolled up, showing older messages
                    let total = messages.len();
                    let end_idx = total.saturating_sub(scroll_offset);
                    let start_idx = end_idx.saturating_sub(VISIBLE_MESSAGE_ROWS);

                    messages[start_idx..end_idx]
                        .iter()
                        .map(|msg| {
                            let id = msg.id.clone();
                            let sender = msg.sender.clone();
                            let content = msg.content.clone();
                            let ts = msg.timestamp.clone();
                            let status = msg.delivery_status;
                            let is_own = msg.is_own;
                            let is_finalized = msg.is_finalized;
                            element! {
                                MessageBubble(
                                    key: id,
                                    sender: sender,
                                    content: content,
                                    timestamp: ts,
                                    is_own: is_own,
                                    delivery_status: status,
                                    is_finalized: is_finalized,
                                )
                            }
                            .into_any()
                        })
                        .collect()
                })
            }
        }
    }
}
