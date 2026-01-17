//! # Device Select Modal
//!
//! Modal for selecting a device from a list (used for device removal).
//! The current device is shown but greyed out and not selectable.

use iocraft::prelude::*;

use super::modal::ModalContent;
use super::{modal_footer, modal_header, ModalFooterProps, ModalHeaderProps};
use crate::tui::theme::{Borders, Spacing, Theme};
use crate::tui::types::{Device, KeyHint};

/// Props for DeviceSelectModal
#[derive(Default, Props)]
pub struct DeviceSelectModalProps {
    /// Whether the modal is visible
    pub visible: bool,
    /// Modal title
    pub title: String,
    /// Available devices to select from
    pub devices: Vec<Device>,
    /// Currently selected index (skips current device)
    pub selected_index: usize,
}

/// Modal for selecting a device to remove
#[component]
pub fn DeviceSelectModal(props: &DeviceSelectModalProps) -> impl Into<AnyElement<'static>> {
    if !props.visible {
        return element! {
            View {}
        }
        .into_any();
    }

    let title = props.title.clone();
    let devices = props.devices.clone();
    let selected_index = props.selected_index;

    // Header props
    let header_props = ModalHeaderProps::new(title);

    // Footer props
    let footer_hints = vec![
        KeyHint::new("Esc", "Cancel"),
        KeyHint::new("↑↓", "Navigate"),
        KeyHint::new("Enter", "Select"),
    ];
    let footer_props = ModalFooterProps::new(footer_hints);

    // Count selectable devices (non-current)
    let selectable_count = devices.iter().filter(|d| !d.is_current).count();

    element! {
        ModalContent(
            flex_direction: FlexDirection::Column,
            border_style: Borders::PRIMARY,
            border_color: Some(Theme::PRIMARY),
        ) {
            // Header
            #(Some(modal_header(&header_props).into()))

            // Body - device list
            View(
                width: 100pct,
                padding: Spacing::MODAL_PADDING,
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                overflow: Overflow::Scroll,
            ) {
                #(if devices.is_empty() {
                    vec![element! {
                        View {
                            Text(content: "No devices available", color: Theme::TEXT_MUTED)
                        }
                    }]
                } else if selectable_count == 0 {
                    vec![element! {
                        View {
                            Text(content: "No other devices to remove", color: Theme::TEXT_MUTED)
                        }
                    }]
                } else {
                    // Track the selectable index (excludes current device)
                    let mut selectable_idx = 0usize;
                    devices.iter().map(|device| {
                        let is_current = device.is_current;

                        // Only non-current devices can be selected
                        let is_selected = if is_current {
                            false
                        } else {
                            let result = selectable_idx == selected_index;
                            selectable_idx += 1;
                            result
                        };

                        // Styling based on state
                        let (bg, text_color, pointer_color) = if is_current {
                            // Current device: greyed out
                            (Theme::LIST_BG_NORMAL, Theme::TEXT_MUTED, Theme::TEXT_MUTED)
                        } else if is_selected {
                            // Selected
                            (Theme::LIST_BG_SELECTED, Theme::LIST_TEXT_SELECTED, Theme::LIST_TEXT_SELECTED)
                        } else {
                            // Normal
                            (Theme::LIST_BG_NORMAL, Theme::LIST_TEXT_NORMAL, Theme::PRIMARY)
                        };

                        let name = device.name.clone();
                        let id = device.id.clone();
                        let pointer = if is_selected { "➤ " } else { "  " };

                        // Add "(current)" suffix for current device
                        let display_name = if is_current {
                            format!("{name} (current)")
                        } else {
                            name
                        };

                        element! {
                            View(
                                key: id,
                                flex_direction: FlexDirection::Row,
                                background_color: bg,
                                padding_left: Spacing::XS,
                            ) {
                                Text(content: pointer.to_string(), color: pointer_color)
                                Text(content: display_name, color: text_color)
                            }
                        }
                    }).collect()
                })
            }

            // Footer
            #(Some(modal_footer(&footer_props).into()))
        }
    }
    .into_any()
}
