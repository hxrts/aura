//! # Device Enrollment Modal
//!
//! Shows the out-of-band enrollment code and ceremony progress for Settings → Add device.

use iocraft::prelude::*;

use crate::tui::layout::dim;
use crate::tui::theme::{Borders, Icons, Spacing, Theme};

#[derive(Default, Props)]
pub struct DeviceEnrollmentModalProps {
    pub visible: bool,
    pub device_name: String,
    pub enrollment_code: String,
    pub accepted_count: u16,
    pub total_count: u16,
    pub threshold: u16,
    pub is_complete: bool,
    pub has_failed: bool,
    pub error_message: String,
}

#[component]
pub fn DeviceEnrollmentModal(props: &DeviceEnrollmentModalProps) -> impl Into<AnyElement<'static>> {
    if !props.visible {
        return element! { View {} };
    }

    // Format long codes into multiple lines for readability.
    let formatted_code = if props.enrollment_code.len() > 40 {
        props
            .enrollment_code
            .chars()
            .collect::<Vec<_>>()
            .chunks(40)
            .map(|c| c.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        props.enrollment_code.clone()
    };

    let (status_icon, status_color, status_text) = if props.has_failed {
        (Icons::CROSS, Theme::ERROR, "Enrollment failed".to_string())
    } else if props.is_complete {
        (
            Icons::CHECK,
            Theme::SUCCESS,
            "Enrollment complete".to_string(),
        )
    } else {
        (
            Icons::PENDING,
            Theme::WARNING,
            "Waiting for acceptance…".to_string(),
        )
    };

    let footer_hint = if props.has_failed || props.is_complete {
        "Close"
    } else {
        "Cancel"
    };

    element! {
        View(
            width: dim::TOTAL_WIDTH,
            height: dim::MIDDLE_HEIGHT,
            flex_direction: FlexDirection::Column,
            background_color: Theme::BG_MODAL,
            border_style: Borders::PRIMARY,
            border_color: Theme::PRIMARY,
            overflow: Overflow::Hidden,
        ) {
            // Header
            View(
                width: 100pct,
                padding_left: Spacing::PANEL_PADDING,
                padding_right: Spacing::PANEL_PADDING,
                padding_top: Spacing::XS,
                padding_bottom: Spacing::XS,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                border_style: BorderStyle::Single,
                border_edges: Edges::Bottom,
                border_color: Theme::BORDER,
            ) {
                Text(
                    content: format!("Enroll device: {}", props.device_name),
                    weight: Weight::Bold,
                    color: Theme::TEXT,
                )
                View(flex_direction: FlexDirection::Row, gap: 1) {
                    Text(content: status_icon.to_string(), color: status_color)
                    Text(content: status_text, color: status_color, weight: Weight::Bold)
                    Text(content: " — ", color: Theme::TEXT_MUTED)
                    Text(
                        content: format!(
                            "{}/{} accepted (need {})",
                            props.accepted_count, props.total_count, props.threshold
                        ),
                        color: Theme::TEXT_MUTED,
                    )
                }
            }

            // Body
            View(
                width: 100pct,
                padding_left: Spacing::MODAL_PADDING,
                padding_right: Spacing::MODAL_PADDING,
                padding_top: Spacing::XS,
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                overflow: Overflow::Hidden,
            ) {
                Text(
                    content: "Import this code on the new device:",
                    color: Theme::TEXT,
                )
                View(
                    width: 100pct,
                    flex_direction: FlexDirection::Column,
                    border_style: Borders::INPUT,
                    border_color: Theme::PRIMARY,
                    padding_left: Spacing::PANEL_PADDING,
                    padding_right: Spacing::PANEL_PADDING,
                ) {
                    Text(
                        content: formatted_code,
                        color: Theme::PRIMARY,
                        wrap: TextWrap::Wrap,
                    )
                }
                #(if props.has_failed && !props.error_message.is_empty() {
                    Some(element! {
                        View(margin_top: Spacing::XS) {
                            Text(content: props.error_message.clone(), color: Theme::ERROR)
                        }
                    })
                } else {
                    None
                })
            }

            // Footer
            View(
                width: 100pct,
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::Center,
                padding_left: Spacing::PANEL_PADDING,
                padding_right: Spacing::PANEL_PADDING,
                padding_top: Spacing::XS,
                padding_bottom: Spacing::XS,
                border_style: BorderStyle::Single,
                border_edges: Edges::Top,
                border_color: Theme::BORDER,
            ) {
                View(flex_direction: FlexDirection::Row, gap: Spacing::XS) {
                    Text(content: "Esc", weight: Weight::Bold, color: Theme::SECONDARY)
                    Text(content: footer_hint.to_string(), color: Theme::TEXT_MUTED)
                }
            }
        }
    }
}
