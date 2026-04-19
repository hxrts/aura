use super::wizard::open_guardian_setup_wizard;
use crate::model::{ActiveModal, CreateInvitationModalState, ScreenId, TextModalState, UiModel};
pub(super) fn handle_contacts_char(model: &mut UiModel, ch: char) {
    match ch {
        'n' => {
            model.modal_hint = "Invite Contacts".to_string();
            model.clear_current_invitation_code();
            model.active_modal = Some(ActiveModal::CreateInvitation(CreateInvitationModalState {
                nickname: model.profile_nickname.trim().to_string(),
                receiver_nickname: String::new(),
                message: String::new(),
                ttl_hours: 24,
                active_field: aura_app::ui::contract::FieldId::Nickname,
                generated_code: None,
            }));
        }
        'a' => {
            model.modal_hint = "Accept Invitation".to_string();
            model.active_modal = Some(ActiveModal::AcceptContactInvitation(
                TextModalState::default(),
            ));
        }
        'e' => {
            model.modal_hint = "Edit Nickname".to_string();
            model.active_modal = Some(ActiveModal::EditNickname(TextModalState {
                value: model
                    .selected_contact_name()
                    .unwrap_or_default()
                    .to_string(),
            }));
        }
        'g' => {
            open_guardian_setup_wizard(model);
        }
        'c' => {
            if let Some(contact) = model.selected_contact_name().map(str::to_string) {
                model.set_screen(ScreenId::Chat);
                let channel_id = super::chat::ensure_named_channel(
                    model,
                    &format!("DM: {contact}"),
                    String::new(),
                );
                model.select_channel_id(Some(&channel_id));
            }
        }
        'd' => {}
        'p' => {}
        'r' => {
            model.modal_hint = "Remove Contact".to_string();
            model.active_modal = Some(ActiveModal::RemoveContact);
        }
        _ => {}
    }
}
