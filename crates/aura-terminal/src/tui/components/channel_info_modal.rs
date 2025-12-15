//! # Channel Info Modal
//!
//! Display modal showing channel information and settings.

use iocraft::prelude::*;

use crate::tui::theme::Theme;

/// Props for ChannelInfoModal
#[derive(Default, Props)]
pub struct ChannelInfoModalProps {
    /// Whether the modal is visible
    pub visible: bool,
    /// Channel name
    pub channel_name: String,
    /// Channel topic (if any)
    pub topic: String,
    /// List of participant names
    pub participants: Vec<String>,
}

/// Modal displaying channel information
#[component]
pub fn ChannelInfoModal(props: &ChannelInfoModalProps) -> impl Into<AnyElement<'static>> {
    if !props.visible {
        return element! {
            View {}
        };
    }

    let channel_name = props.channel_name.clone();
    let topic = if props.topic.is_empty() {
        "No topic set".to_string()
    } else {
        props.topic.clone()
    };
    let participants = props.participants.clone();
    let participant_count = participants.len();

    element! {
        View(
            position: Position::Absolute,
            width: 100pct,
            height: 100pct,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,

        ) {
            View(
                width: Percent(60.0),
                flex_direction: FlexDirection::Column,
                background_color: Theme::BG_MODAL,
                border_style: BorderStyle::Round,
                border_color: Theme::PRIMARY,
            ) {
                // Header
                View(
                    padding: 2,
                    border_style: BorderStyle::Single,
                    border_edges: Edges::Bottom,
                    border_color: Theme::BORDER,
                ) {
                    Text(
                        content: format!("Channel: #{}", channel_name),
                        weight: Weight::Bold,
                        color: Theme::PRIMARY,
                    )
                }

                // Body
                View(padding: 2, flex_direction: FlexDirection::Column, gap: 1) {
                    // Topic section
                    View(flex_direction: FlexDirection::Column) {
                        Text(
                            content: "Topic:",
                            weight: Weight::Bold,
                            color: Theme::TEXT_MUTED,
                        )
                        Text(
                            content: topic,
                            color: Theme::TEXT,
                        )
                    }

                    // Participants section
                    View(flex_direction: FlexDirection::Column, margin_top: 1) {
                        Text(
                            content: format!("Participants ({}):", participant_count),
                            weight: Weight::Bold,
                            color: Theme::TEXT_MUTED,
                        )
                        Text(
                            content: if participants.is_empty() {
                                "No participants".to_string()
                            } else {
                                participants.join(", ")
                            },
                            color: Theme::TEXT,
                        )
                    }
                }

                // Footer with hint
                View(
                    padding: 2,
                    border_style: BorderStyle::Single,
                    border_edges: Edges::Top,
                    border_color: Theme::BORDER,
                ) {
                    Text(
                        content: "Press Esc to close • t to edit topic",
                        color: Theme::TEXT_MUTED,
                    )
                }
            }
        }
    }
}
