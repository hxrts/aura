//! # Message Panel Component
//!
//! Shared message list rendering for chat-like screens.

use iocraft::prelude::*;

use crate::tui::components::MessageBubble;
use crate::tui::theme::{Spacing, Theme};
use crate::tui::types::Message;

/// Props for MessagePanel
#[derive(Default, Props)]
pub struct MessagePanelProps {
    pub messages: Vec<Message>,
    pub title: Option<String>,
    pub empty_message: Option<String>,
}

/// A shared, scrollable message list panel
#[component]
pub fn MessagePanel(props: &MessagePanelProps) -> impl Into<AnyElement<'static>> {
    let messages = props.messages.clone();
    let title = props.title.clone().filter(|value| !value.is_empty());
    let empty_message = props
        .empty_message
        .clone()
        .filter(|value| !value.is_empty());

    let panel_padding = Spacing::PANEL_PADDING;
    let padding_top = if title.is_some() { 0 } else { panel_padding };

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
            #(title.map(|title| element! {
                View(padding_bottom: 1) {
                    Text(content: title, weight: Weight::Bold, color: Theme::PRIMARY)
                }
            }))
            View(
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                overflow: Overflow::Scroll,
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
                    messages
                        .iter()
                        .map(|msg| {
                            let id = msg.id.clone();
                            let sender = msg.sender.clone();
                            let content = msg.content.clone();
                            let ts = msg.timestamp.clone();
                            let status = msg.delivery_status;
                            let is_own = msg.is_own;
                            element! {
                                MessageBubble(
                                    key: id,
                                    sender: sender,
                                    content: content,
                                    timestamp: ts,
                                    is_own: is_own,
                                    delivery_status: status,
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
