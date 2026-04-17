//! # Invitation Create Modal
//!
//! Modal for creating new invitations from the TUI.
//!
//! ## Field-Focus Navigation
//!
//! The modal uses a simple field-focus model:
//! - ↑/↓: Navigate between Nickname, Invitee Nickname, Message, and TTL fields
//! - ←/→: Change value (TTL field only)
//! - Typing: Edit text fields when focused
//! - Enter: Create invitation
//! - Esc: Cancel

use iocraft::prelude::*;
use std::sync::Arc;

use crate::tui::components::{modal_header, status_message, ModalHeaderProps, ModalStatus};
use crate::tui::layout::dim;
use crate::tui::state::CreateInvitationField;
use crate::tui::theme::{Borders, Spacing, Theme};

/// Callback type for invitation creation
pub type CreateInvitationCallback =
    Arc<dyn Fn(Option<String>, Option<String>, Option<String>, Option<u64>) + Send + Sync>;

/// Callback type for modal cancellation
pub type CancelCallback = Arc<dyn Fn() + Send + Sync>;

/// Props for InvitationCreateModal
#[derive(Default, Props)]
pub struct InvitationCreateModalProps {
    /// Whether the modal is visible
    pub visible: bool,
    /// Whether the modal itself is focused (vs other UI elements)
    pub focused: bool,
    /// Which field is currently focused
    pub focused_field: CreateInvitationField,
    /// Whether creation is in progress
    pub creating: bool,
    /// Error message if creation failed
    pub error: String,
    /// Optional nickname for the invitation
    pub nickname: String,
    /// Optional sender-local nickname for the invitee
    pub receiver_nickname: String,
    /// Optional message for the invitation
    pub message: String,
    /// TTL in hours (0 = no expiry)
    pub ttl_hours: u32,
    /// Callback for creating the invitation
    pub on_create: Option<CreateInvitationCallback>,
    /// Callback for canceling
    pub on_cancel: Option<CancelCallback>,
}

/// Modal for creating new invitations
#[component]
pub fn InvitationCreateModal(props: &InvitationCreateModalProps) -> impl Into<AnyElement<'static>> {
    if !props.visible {
        return element! {
            View {}
        };
    }

    let creating = props.creating;
    let has_error = !props.error.is_empty();
    let error = props.error.clone();
    let nickname = props.nickname.clone();
    let receiver_nickname = props.receiver_nickname.clone();
    let message = props.message.clone();
    let ttl_hours = props.ttl_hours;
    let focused_field = props.focused_field;

    let can_submit = !creating;

    // Field focus colors
    let nickname_focused = focused_field == CreateInvitationField::Nickname;
    let receiver_nickname_focused = focused_field == CreateInvitationField::ReceiverNickname;
    let message_focused = focused_field == CreateInvitationField::Message;
    let ttl_focused = focused_field == CreateInvitationField::Ttl;

    let nickname_border = if nickname_focused {
        Theme::BORDER_FOCUS
    } else {
        Theme::BORDER
    };
    let message_border = if message_focused {
        Theme::BORDER_FOCUS
    } else {
        Theme::BORDER
    };
    let receiver_nickname_border = if receiver_nickname_focused {
        Theme::BORDER_FOCUS
    } else {
        Theme::BORDER
    };
    let ttl_border = if ttl_focused {
        Theme::BORDER_FOCUS
    } else {
        Theme::BORDER
    };

    // Focus indicator
    let nickname_pointer = if nickname_focused { "▸ " } else { "  " };
    let receiver_nickname_pointer = if receiver_nickname_focused {
        "▸ "
    } else {
        "  "
    };
    let message_pointer = if message_focused { "▸ " } else { "  " };
    let ttl_pointer = if ttl_focused { "▸ " } else { "  " };

    let nickname_display = if nickname_focused {
        if nickname.is_empty() {
            "│".to_string()
        } else if nickname.len() > 15 {
            format!("{}...│", &nickname[..12])
        } else {
            format!("{nickname}│")
        }
    } else if nickname.is_empty() {
        "(optional)".to_string()
    } else if nickname.len() > 16 {
        format!("{}...", &nickname[..13])
    } else {
        nickname.clone()
    };

    let nickname_color = if nickname_focused {
        Theme::TEXT
    } else if nickname.is_empty() {
        Theme::TEXT_MUTED
    } else {
        Theme::TEXT
    };

    let receiver_nickname_display = if receiver_nickname_focused {
        if receiver_nickname.is_empty() {
            "│".to_string()
        } else if receiver_nickname.len() > 15 {
            format!("{}...│", &receiver_nickname[..12])
        } else {
            format!("{receiver_nickname}│")
        }
    } else if receiver_nickname.is_empty() {
        "(optional)".to_string()
    } else if receiver_nickname.len() > 16 {
        format!("{}...", &receiver_nickname[..13])
    } else {
        receiver_nickname.clone()
    };

    let receiver_nickname_color = if receiver_nickname_focused {
        Theme::TEXT
    } else if receiver_nickname.is_empty() {
        Theme::TEXT_MUTED
    } else {
        Theme::TEXT
    };

    // Message display (truncated to fit input width, show cursor when focused)
    let message_display = if message_focused {
        if message.is_empty() {
            "│".to_string() // Cursor only
        } else if message.len() > 11 {
            format!("{}...│", &message[..8])
        } else {
            format!("{message}│")
        }
    } else if message.is_empty() {
        "(optional)".to_string()
    } else if message.len() > 12 {
        format!("{}...", &message[..9])
    } else {
        message.clone()
    };

    let message_color = if message_focused {
        Theme::TEXT
    } else if message.is_empty() {
        Theme::TEXT_MUTED
    } else {
        Theme::TEXT
    };

    // TTL display with arrows when focused
    let ttl_display = {
        let base = if ttl_hours == 1 {
            "1 hour".to_string()
        } else if ttl_hours < 24 {
            format!("{ttl_hours} hours")
        } else if ttl_hours == 24 {
            "1 day".to_string()
        } else if ttl_hours == 168 {
            "1 week".to_string()
        } else {
            format!("{} days", ttl_hours / 24)
        };
        if ttl_focused {
            format!("◀ {base} ▶")
        } else {
            base
        }
    };

    // Header props
    let header_props = ModalHeaderProps::new("Create Contact Invitation")
        .with_subtitle("Add someone as a contact for messaging");

    // Status for error
    let status = if has_error {
        ModalStatus::Error(error)
    } else {
        ModalStatus::Idle
    };

    element! {
        View(
            width: dim::TOTAL_WIDTH,
            height: dim::MIDDLE_HEIGHT,
            flex_direction: FlexDirection::Column,
            background_color: Theme::BG_MODAL,
            border_style: Borders::PRIMARY,
            border_color: if props.focused { Theme::BORDER_FOCUS } else { Theme::BORDER },
            overflow: Overflow::Hidden,
        ) {
            // Header
            #(Some(modal_header(&header_props).into()))

            // Form content - fills available space
            View(
                width: 100pct,
                padding_left: Spacing::MODAL_PADDING,
                padding_right: Spacing::MODAL_PADDING,
                padding_bottom: Spacing::XS,
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                overflow: Overflow::Hidden,
            ) {
                // Optional nickname
                View(flex_direction: FlexDirection::Column, margin_bottom: Spacing::XS) {
                    View(flex_direction: FlexDirection::Row) {
                        Text(content: nickname_pointer.to_string(), color: Theme::PRIMARY, weight: Weight::Bold)
                        Text(content: "Nickname", color: if nickname_focused { Theme::TEXT } else { Theme::TEXT_MUTED })
                        Text(content: " - ", color: Theme::TEXT_MUTED)
                        Text(content: "What the recipient should call you", color: Theme::TEXT_MUTED)
                    }
                    View(
                        margin_left: 2,
                        width: 26,
                        border_style: Borders::INPUT,
                        border_color: nickname_border,
                        padding_left: Spacing::PANEL_PADDING,
                        padding_right: Spacing::PANEL_PADDING,
                    ) {
                        Text(content: nickname_display, color: nickname_color)
                    }
                }

                // Optional invitee nickname
                View(flex_direction: FlexDirection::Column, margin_bottom: Spacing::XS) {
                    View(flex_direction: FlexDirection::Row) {
                        Text(content: receiver_nickname_pointer.to_string(), color: Theme::PRIMARY, weight: Weight::Bold)
                        Text(content: "Their Nickname", color: if receiver_nickname_focused { Theme::TEXT } else { Theme::TEXT_MUTED })
                        Text(content: " - ", color: Theme::TEXT_MUTED)
                        Text(content: "How this pending invite should be labeled for you", color: Theme::TEXT_MUTED)
                    }
                    View(
                        margin_left: 2,
                        width: 26,
                        border_style: Borders::INPUT,
                        border_color: receiver_nickname_border,
                        padding_left: Spacing::PANEL_PADDING,
                        padding_right: Spacing::PANEL_PADDING,
                    ) {
                        Text(content: receiver_nickname_display, color: receiver_nickname_color)
                    }
                }

                // Optional message
                View(flex_direction: FlexDirection::Column, margin_bottom: Spacing::XS) {
                    View(flex_direction: FlexDirection::Row) {
                        Text(content: message_pointer.to_string(), color: Theme::PRIMARY, weight: Weight::Bold)
                        Text(content: "Message", color: if message_focused { Theme::TEXT } else { Theme::TEXT_MUTED })
                        Text(content: " - ", color: Theme::TEXT_MUTED)
                        Text(content: "Personal note included with the invitation", color: Theme::TEXT_MUTED)
                    }
                    View(
                        margin_left: 2,
                        width: 26,
                        border_style: Borders::INPUT,
                        border_color: message_border,
                        padding_left: Spacing::PANEL_PADDING,
                        padding_right: Spacing::PANEL_PADDING,
                    ) {
                        Text(content: message_display, color: message_color)
                    }
                }

                // TTL selector
                View(flex_direction: FlexDirection::Column, margin_bottom: Spacing::XS) {
                    View(flex_direction: FlexDirection::Row) {
                        Text(content: ttl_pointer.to_string(), color: Theme::PRIMARY, weight: Weight::Bold)
                        Text(content: "Expiry", color: if ttl_focused { Theme::TEXT } else { Theme::TEXT_MUTED })
                        Text(content: " - ", color: Theme::TEXT_MUTED)
                        Text(content: "How long the invite code remains valid", color: Theme::TEXT_MUTED)
                    }
                    View(
                        margin_left: 2,
                        width: 24,
                        border_style: Borders::INPUT,
                        border_color: ttl_border,
                        padding_left: Spacing::PANEL_PADDING,
                        padding_right: Spacing::PANEL_PADDING,
                    ) {
                        Text(content: ttl_display, color: if ttl_focused { Theme::PRIMARY } else { Theme::TEXT })
                    }
                }

                // Error message
                #(Some(status_message(&status).into()))
            }

            // Footer with hints
            View(
                width: 100pct,
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                padding: Spacing::PANEL_PADDING,
                gap: Spacing::LG,
                border_style: BorderStyle::Single,
                border_edges: Edges::Top,
                border_color: Theme::BORDER,
            ) {
                View(flex_direction: FlexDirection::Row, gap: Spacing::XS) {
                    Text(content: "↑/↓", weight: Weight::Bold, color: Theme::SECONDARY)
                    Text(content: "Navigate", color: Theme::TEXT_MUTED)
                }
                View(flex_direction: FlexDirection::Row, gap: Spacing::XS) {
                    Text(content: "←/→", weight: Weight::Bold, color: Theme::SECONDARY)
                    Text(content: "TTL", color: Theme::TEXT_MUTED)
                }
                View(flex_direction: FlexDirection::Row, gap: Spacing::XS) {
                    Text(content: "Esc", weight: Weight::Bold, color: Theme::SECONDARY)
                    Text(content: "Cancel", color: Theme::TEXT_MUTED)
                }
                View(flex_direction: FlexDirection::Row, gap: Spacing::XS) {
                    Text(content: "Enter", weight: Weight::Bold, color: if can_submit { Theme::PRIMARY } else { Theme::TEXT_MUTED })
                    Text(content: "Generate", color: if can_submit { Theme::PRIMARY } else { Theme::TEXT_MUTED })
                }
            }
        }
    }
}
