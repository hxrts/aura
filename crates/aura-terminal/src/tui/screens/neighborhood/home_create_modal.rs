//! Home Creation Modal
//!
//! Modal for creating a new home in the neighborhood.

use iocraft::prelude::*;

use crate::tui::layout::dim;
use crate::tui::state::views::HomeCreateModalState;
use crate::tui::theme::{Borders, Spacing, Theme};

/// Props for HomeCreateModal
#[derive(Default, Props)]
pub struct HomeCreateModalProps {
    /// Modal state
    pub state: HomeCreateModalState,
}

/// Modal for creating a new home
#[component]
pub fn HomeCreateModal(
    props: &HomeCreateModalProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let control_layout_0 = crate::tui::rendered_controls::observe_context_draw(
        &mut hooks,
        crate::tui::rendered_controls::DrawObservationRole::Control {
            id: aura_app::ui::contract::ControlId::Modal(
                aura_app::ui::contract::ModalId::CreateHome,
            ),
            // This component is included only by the modal renderer's actual
            // selected child; observation still requires its completed layout.
            rendered: true,
        },
    );

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
    let header_props = crate::tui::components::ModalHeaderProps::new("Create New Home")
        .with_subtitle("Homes are shared spaces where you and others can communicate.");

    // Input field props
    let name_input =
        crate::tui::components::LabeledInputProps::new("Home Name:", "Enter home name...")
            .with_field(aura_app::ui::contract::FieldId::HomeName)
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
        crate::tui::components::ModalStatus::Loading("Creating home...".to_string())
    } else {
        crate::tui::components::ModalStatus::Idle
    };

    crate::tui::rendered_controls::register_native_view(
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
                crate::tui::rendered_controls::ObservedClipView(
                    width: 100pct,
                    padding: Spacing::MODAL_PADDING,
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.0_f32,
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
        },
        vec![control_layout_0],
    )
}
