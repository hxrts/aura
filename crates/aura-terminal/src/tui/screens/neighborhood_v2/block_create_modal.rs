//! Block Creation Modal
//!
//! Modal for creating a new block in the neighborhood.

use iocraft::prelude::*;

use crate::tui::layout::dim;
use crate::tui::state::views::BlockCreateModalState;
use crate::tui::theme::{Borders, Spacing, Theme};

/// Props for BlockCreateModal
#[derive(Default, Props)]
pub struct BlockCreateModalProps {
    /// Modal state
    pub state: BlockCreateModalState,
}

/// Modal for creating a new block
#[component]
pub fn BlockCreateModal(props: &BlockCreateModalProps) -> impl Into<AnyElement<'static>> {
    let state = &props.state;
    let name = state.name.clone();
    let description = state.description.clone();
    let active_field = state.active_field;
    let error = state.error.clone();
    let creating = state.creating;

    // Determine border color based on state
    let border_color = if error.is_some() {
        Theme::ERROR
    } else if creating {
        Theme::WARNING
    } else {
        Theme::PRIMARY
    };

    // Header props
    let header_props = crate::tui::components::ModalHeaderProps::new("Create New Block")
        .with_subtitle("Blocks are shared spaces where you and others can communicate.");

    // Input field props
    let name_input = crate::tui::components::LabeledInputProps::new(
        "Block Name:",
        "Enter block name...",
    )
        .with_value(name)
        .with_focused(active_field == 0);
    let description_input = crate::tui::components::LabeledInputProps::new(
        "Description (optional):",
        "Enter description...",
    )
    .with_value(description)
    .with_focused(active_field == 1);

    // Footer props
    let footer_props = crate::tui::components::ModalFooterProps::new(vec![
        crate::tui::types::KeyHint::new("Esc", "Cancel"),
        crate::tui::types::KeyHint::new("Tab", "Next Field"),
        crate::tui::types::KeyHint::new("Enter", "Create"),
    ]);

    // Status
    let status = if let Some(err) = error {
        crate::tui::components::ModalStatus::Error(err)
    } else if creating {
        crate::tui::components::ModalStatus::Loading("Creating block...".to_string())
    } else {
        crate::tui::components::ModalStatus::Idle
    };

    element! {
        View(
            width: dim::TOTAL_WIDTH,
            height: dim::MIDDLE_HEIGHT,
            flex_direction: FlexDirection::Column,
            background_color: Theme::BG_MODAL,
            border_style: Borders::PRIMARY,
            border_color: border_color,
            overflow: Overflow::Hidden,
        ) {
            // Header
            #(Some(crate::tui::components::modal_header(&header_props).into()))

            // Body
            View(
                width: 100pct,
                padding: Spacing::MODAL_PADDING,
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                overflow: Overflow::Hidden,
            ) {
                // Input fields
                View(flex_direction: FlexDirection::Column, gap: Spacing::SM) {
                    #(Some(crate::tui::components::labeled_input(&name_input).into()))
                    #(Some(crate::tui::components::labeled_input(&description_input).into()))
                }

                // Status message (error/loading)
                #(Some(crate::tui::components::status_message(&status).into()))
            }

            // Footer
            #(Some(crate::tui::components::modal_footer(&footer_props).into()))
        }
    }
}
