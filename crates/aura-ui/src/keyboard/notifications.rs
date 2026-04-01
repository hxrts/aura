use crate::model::{ActiveModal, TextModalState, UiModel};

pub(super) fn handle_notifications_char(model: &mut UiModel, ch: char) {
    if ch == 'a' {
        model.modal_hint = "Accept Channel Invitation".to_string();
        model.active_modal = Some(ActiveModal::AcceptChannelInvitation(
            TextModalState::default(),
        ));
    }
}
