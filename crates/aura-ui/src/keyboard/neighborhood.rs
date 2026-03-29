use super::set_toast;
use crate::model::{
    AccessDepth, AccessOverrideModalState, ActiveModal, CapabilityConfigModalState, SelectedHome,
    TextModalState, UiModel,
};

pub(super) fn handle_neighborhood_char(model: &mut UiModel, ch: char) {
    match ch {
        'n' => {
            model.modal_hint = "Create New Home".to_string();
            model.active_modal = Some(ActiveModal::CreateHome(TextModalState::default()));
        }
        'a' => {
            model.modal_hint = "Accept Invitation".to_string();
            model.active_modal = Some(ActiveModal::AcceptInvitation(TextModalState::default()));
        }
        'm' => {}
        'v' => {}
        'L' => {}
        'g' | 'H' => {
            if model.selected_home.is_none() {
                model.selected_home = Some(SelectedHome {
                    id: "Neighborhood".to_string(),
                    name: "Neighborhood".to_string(),
                });
            }
            set_toast(model, 'ℹ', "Viewing the neighborhood map");
        }
        'b' => {
            model.access_depth = AccessDepth::Limited;
            model.neighborhood_mode = crate::model::NeighborhoodMode::Map;
            set_toast(model, 'ℹ', "Returned to the neighborhood map");
        }
        'o' if matches!(
            model.neighborhood_mode,
            crate::model::NeighborhoodMode::Detail
        ) =>
        {
            model.modal_hint = "Assign Moderator".to_string();
            model.active_modal = Some(ActiveModal::AssignModerator);
        }
        'x' if matches!(
            model.neighborhood_mode,
            crate::model::NeighborhoodMode::Detail
        ) =>
        {
            model.modal_hint = "Access Override".to_string();
            model.active_modal = Some(ActiveModal::AccessOverride(
                AccessOverrideModalState::default(),
            ));
        }
        'p' if matches!(
            model.neighborhood_mode,
            crate::model::NeighborhoodMode::Detail
        ) =>
        {
            model.modal_hint = "Home Capability Configuration".to_string();
            model.active_modal = Some(ActiveModal::CapabilityConfig(
                CapabilityConfigModalState::default(),
            ));
        }
        'i' if matches!(
            model.neighborhood_mode,
            crate::model::NeighborhoodMode::Detail
        ) =>
        {
            model.input_mode = true;
            model.input_buffer.clear();
        }
        _ => {}
    }
}
