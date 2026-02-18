//! # Message Panel Component
//!
//! Shared message list rendering for chat-like screens.
//!
//! ## Features
//!
//! - **Breadcrumb title**: Displays a breadcrumb path (e.g., "Home › Channel › # general")
//! - **Programmatic scrolling**: Accepts `scroll_offset` for external scroll control
//! - **Scroll indicators**: Shows ▲/▼ indicators when content is scrollable
//! - **Scrollbar**: Visual scrollbar on the right side when content is scrollable
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

/// Default visible message window height (messages that fit on screen)
/// Used when `visible_rows` prop is not specified
const DEFAULT_VISIBLE_ROWS: usize = 18;

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
    /// Number of visible rows (defaults to 18 if not specified)
    /// This should be set based on the actual panel height
    pub visible_rows: Option<usize>,
}

/// Build the scrollbar string for display
/// Returns a string of characters representing the scrollbar track with thumb
fn build_scrollbar(
    visible_rows: usize,
    total_items: usize,
    scroll_offset: usize,
    track_height: usize,
) -> String {
    if total_items <= visible_rows || track_height == 0 {
        // No scrollbar needed - return empty track
        return " ".repeat(track_height);
    }

    let max_scroll = total_items.saturating_sub(visible_rows);
    let clamped_offset = scroll_offset.min(max_scroll);

    // Calculate thumb size (minimum 1 row)
    let thumb_size = ((visible_rows as f64 / total_items as f64) * track_height as f64)
        .ceil()
        .max(1.0) as usize;

    // Calculate thumb position (inverted because scroll_offset 0 = bottom)
    // When at bottom (offset=0), thumb should be at bottom of track
    // When at top (offset=max), thumb should be at top of track
    let scroll_ratio = if max_scroll > 0 {
        clamped_offset as f64 / max_scroll as f64
    } else {
        0.0
    };
    // Invert: 0 at bottom, 1 at top
    let thumb_top =
        ((1.0 - scroll_ratio) * (track_height.saturating_sub(thumb_size)) as f64).round() as usize;

    // Build scrollbar string
    let mut scrollbar = String::with_capacity(track_height);
    for i in 0..track_height {
        if i >= thumb_top && i < thumb_top + thumb_size {
            scrollbar.push('█'); // Thumb
        } else {
            scrollbar.push('│'); // Track
        }
    }
    scrollbar
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
    // Use actual message count from the messages vec, not the prop
    // (the prop may be stale or computed incorrectly across channels)
    let actual_message_count = messages.len();

    // Use provided visible_rows or fall back to default
    let visible_rows = props.visible_rows.unwrap_or(DEFAULT_VISIBLE_ROWS);

    let panel_padding = Spacing::PANEL_PADDING;
    let padding_top = if title.is_some() { 0 } else { panel_padding };

    // Clamp scroll offset to valid range for indicator calculation
    let max_scroll = actual_message_count.saturating_sub(visible_rows.min(actual_message_count));
    let clamped_offset = scroll_offset.min(max_scroll);

    // Calculate scroll indicators using clamped offset
    let can_scroll_up = clamped_offset > 0;
    let can_scroll_down = actual_message_count > visible_rows && clamped_offset < max_scroll;

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

    // Determine if we need a scrollbar
    let needs_scrollbar = actual_message_count > visible_rows;

    // Build scrollbar content (one character per row, displayed vertically)
    let scrollbar_track_height = visible_rows.min(actual_message_count);
    let scrollbar_chars = build_scrollbar(
        visible_rows,
        actual_message_count,
        scroll_offset,
        scrollbar_track_height,
    );

    element! {
        View(
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            border_style: BorderStyle::Round,
            border_color: Theme::BORDER,
            padding_left: panel_padding,
            padding_right: 0, // No right padding - scrollbar goes there
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
                flex_direction: FlexDirection::Row,
                flex_grow: 1.0,
                overflow: Overflow::Hidden,
            ) {
                // Messages area
                View(
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.0,
                    overflow: Overflow::Hidden,
                    gap: 0,
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
                        let end_idx = actual_message_count.saturating_sub(clamped_offset);
                        let start_idx = end_idx.saturating_sub(visible_rows);

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
                // Scrollbar area (1 character wide)
                #(if needs_scrollbar {
                    Some(element! {
                        View(
                            flex_direction: FlexDirection::Column,
                            width: 1,
                            padding_left: 0,
                            padding_right: panel_padding,
                        ) {
                            // Display scrollbar vertically (one char per line)
                            #(scrollbar_chars.chars().map(|ch| {
                                element! {
                                    Text(content: ch.to_string(), color: Theme::TEXT_MUTED)
                                }.into_any()
                            }).collect::<Vec<_>>())
                        }
                    })
                } else {
                    // Empty spacer when no scrollbar needed
                    Some(element! {
                        View(width: panel_padding, padding_right: panel_padding) {}
                    })
                })
            }
        }
    }
}
