//! # Modal Overlays Rendering
//!
//! Render functions for all modal overlays in the IoApp component.
//! Modals must render at root level for proper overlay positioning via ModalFrame.
//!
//! This module provides standalone render functions that can be called from IoApp.

use iocraft::prelude::*;

use crate::tui::components::{
    AccountSetupModal, ConfirmModal, ContactSelectModal, DeviceSelectModal, HelpModal, ModalFrame,
    ModalScaffold, TextInputModal,
};
use crate::tui::props::{
    AddDeviceModalViewProps, ChatViewProps, ContactsViewProps, NeighborhoodViewProps,
    SettingsViewProps,
};
use crate::tui::screens::{
    ChannelInfoModal, ChatCreateModal, DeviceEnrollmentModal, GuardianCandidateProps,
    GuardianSetupKind, GuardianSetupModal, HomeCreateModal, InvitationCodeModal,
    InvitationCreateModal, InvitationImportModal,
};
use crate::tui::theme::{Spacing, Theme};
use crate::tui::types::Contact;
use aura_app::ui::types::MAX_NICKNAME_SUGGESTION_LENGTH;

// =============================================================================
// Global Modal Props
// =============================================================================

#[derive(Default, Clone)]
pub struct AccountSetupOverlayProps {
    pub visible: bool,
    pub nickname_suggestion: String,
    pub device_import_code: String,
    pub device_import_manifest: String,
    pub device_import_initiator_verifier: String,
    pub bootstrap_candidates: Vec<String>,
    pub name_focused: bool,
    pub import_code_focused: bool,
    pub import_manifest_focused: bool,
    pub import_initiator_verifier_focused: bool,
    pub creating: bool,
    pub show_spinner: bool,
    pub success: bool,
    pub error: Option<String>,
    pub nickname_error: Option<String>,
}

#[derive(Default, Clone)]
pub struct ContactPickerOverlayProps {
    pub visible: bool,
    pub title: String,
    pub contacts: Vec<Contact>,
    pub selected_index: usize,
    pub error: Option<String>,
    pub selected_ids: Vec<String>,
    pub multi_select: bool,
}

#[derive(Default, Clone)]
pub struct ConfirmOverlayProps {
    pub visible: bool,
    pub title: String,
    pub message: String,
}

#[derive(Default, Clone)]
pub struct HelpOverlayProps {
    pub visible: bool,
    pub current_screen_name: String,
}

/// Props for global modals (not screen-specific)
#[derive(Default, Clone)]
pub struct GlobalModalProps {
    pub account_setup: AccountSetupOverlayProps,
    pub guardian_picker: ContactPickerOverlayProps,
    pub contact_picker: ContactPickerOverlayProps,
    pub confirm: ConfirmOverlayProps,
    pub help: HelpOverlayProps,
}

#[derive(Clone)]
struct TextInputOverlayProps {
    visible: bool,
    focused: bool,
    title: String,
    value: String,
    placeholder: String,
    hint: String,
    error: String,
    submitting: bool,
}

#[derive(Clone)]
struct ContactSelectionOverlayProps {
    visible: bool,
    title: String,
    contacts: Vec<Contact>,
    selected_index: usize,
    error: String,
    selected_ids: Vec<String>,
    multi_select: bool,
}

fn modal_frame(child: AnyElement<'static>) -> AnyElement<'static> {
    element! {
        ModalFrame {
            #(Some(child))
        }
    }
    .into_any()
}

fn render_modal(visible: bool, child: AnyElement<'static>) -> Option<AnyElement<'static>> {
    visible.then(|| modal_frame(child))
}

fn render_text_input_modal(props: TextInputOverlayProps) -> Option<AnyElement<'static>> {
    render_modal(
        props.visible,
        element! {
            TextInputModal(
                visible: true,
                focused: props.focused,
                title: props.title,
                value: props.value,
                placeholder: props.placeholder,
                hint: props.hint,
                error: props.error,
                submitting: props.submitting,
            )
        }
        .into_any(),
    )
}

fn render_contact_selection_modal(
    props: ContactSelectionOverlayProps,
) -> Option<AnyElement<'static>> {
    render_modal(
        props.visible,
        element! {
            ContactSelectModal(
                visible: true,
                title: props.title,
                contacts: props.contacts,
                selected_index: props.selected_index,
                error: props.error,
                selected_ids: props.selected_ids,
                multi_select: props.multi_select,
            )
        }
        .into_any(),
    )
}

// =============================================================================
// Global Modal Render Functions
// =============================================================================

pub fn render_account_setup_modal(global: &GlobalModalProps) -> Option<AnyElement<'static>> {
    render_modal(
        global.account_setup.visible,
        element! {
            AccountSetupModal(
                visible: true,
                nickname_suggestion: global.account_setup.nickname_suggestion.clone(),
                device_import_code: global.account_setup.device_import_code.clone(),
                device_import_manifest:global.account_setup.device_import_manifest.clone(),
                device_import_initiator_verifier:global.account_setup.device_import_initiator_verifier.clone(),
                import_manifest_focused:global.account_setup.import_manifest_focused,
                import_initiator_verifier_focused:global.account_setup.import_initiator_verifier_focused,
                bootstrap_candidates: global.account_setup.bootstrap_candidates.clone(),
                name_focused: global.account_setup.name_focused,
                import_code_focused: global.account_setup.import_code_focused,
                creating: global.account_setup.creating,
                show_spinner: global.account_setup.show_spinner,
                success: global.account_setup.success,
                error: global.account_setup.error.clone().unwrap_or_default(),
                nickname_error: global.account_setup.nickname_error.clone().unwrap_or_default(),
            )
        }
        .into_any(),
    )
}

pub fn render_guardian_modal(global: &GlobalModalProps) -> Option<AnyElement<'static>> {
    render_contact_selection_modal(ContactSelectionOverlayProps {
        visible: global.guardian_picker.visible,
        title: global.guardian_picker.title.clone(),
        contacts: global.guardian_picker.contacts.clone(),
        selected_index: global.guardian_picker.selected_index,
        error: global.guardian_picker.error.clone().unwrap_or_default(),
        selected_ids: global.guardian_picker.selected_ids.clone(),
        multi_select: global.guardian_picker.multi_select,
    })
}

pub fn render_contact_modal(global: &GlobalModalProps) -> Option<AnyElement<'static>> {
    render_contact_selection_modal(ContactSelectionOverlayProps {
        visible: global.contact_picker.visible,
        title: global.contact_picker.title.clone(),
        contacts: global.contact_picker.contacts.clone(),
        selected_index: global.contact_picker.selected_index,
        error: global.contact_picker.error.clone().unwrap_or_default(),
        selected_ids: global.contact_picker.selected_ids.clone(),
        multi_select: global.contact_picker.multi_select,
    })
}

pub fn render_confirm_modal(global: &GlobalModalProps) -> Option<AnyElement<'static>> {
    render_modal(
        global.confirm.visible,
        element! {
            ConfirmModal(
                visible: true,
                title: global.confirm.title.clone(),
                message: global.confirm.message.clone(),
                confirm_text: "Confirm".to_string(),
                cancel_text: "Cancel".to_string(),
                confirm_focused: true,
            )
        }
        .into_any(),
    )
}

pub fn render_help_modal(global: &GlobalModalProps) -> Option<AnyElement<'static>> {
    render_modal(
        global.help.visible,
        element! {
            HelpModal(visible: true, current_screen: Some(global.help.current_screen_name.clone()))
        }
        .into_any(),
    )
}

// =============================================================================
// Contacts Screen Modal Render Functions
// =============================================================================

pub fn render_nickname_modal(contacts: &ContactsViewProps) -> Option<AnyElement<'static>> {
    let modal = &contacts.modals.nickname;
    let hint = modal
        .nickname_suggestion
        .as_ref()
        .map(|suggestion| format!("Suggestion: {suggestion}"))
        .unwrap_or_default();

    render_text_input_modal(TextInputOverlayProps {
        visible: modal.visible,
        focused: true,
        title: "Edit Nickname".to_string(),
        value: modal.value.clone(),
        placeholder: "Enter nickname...".to_string(),
        hint,
        error: String::new(),
        submitting: false,
    })
}

pub fn render_contacts_import_modal(contacts: &ContactsViewProps) -> Option<AnyElement<'static>> {
    let modal = &contacts.modals.import_invitation;
    render_modal(
        modal.visible,
        element! {
            InvitationImportModal(
                visible: true,
                focused: true,
                code: modal.code.clone(),
                error: String::new(),
                importing: modal.importing,
            )
        }
        .into_any(),
    )
}

pub fn render_contacts_create_modal(contacts: &ContactsViewProps) -> Option<AnyElement<'static>> {
    let modal = &contacts.modals.create_invitation;
    render_modal(
        modal.visible,
        element! {
            InvitationCreateModal(
                visible: true,
                focused: true,
                focused_field: modal.focused_field,
                creating: false,
                error: String::new(),
                nickname: modal.nickname.clone(),
                receiver_nickname: modal.receiver_nickname.clone(),
                message: modal.message.clone(),
                ttl_hours: modal.ttl_hours as u32,
            )
        }
        .into_any(),
    )
}

pub fn render_contacts_code_modal(contacts: &ContactsViewProps) -> Option<AnyElement<'static>> {
    let modal = &contacts.modals.code_display;
    render_modal(
        modal.visible,
        element! {
            InvitationCodeModal(
                visible: true,
                code: modal.code.clone(),
                invitation_type: "Invitation".to_string(),
                copied: modal.copied,
            )
        }
        .into_any(),
    )
}

pub fn render_guardian_setup_modal(contacts: &ContactsViewProps) -> Option<AnyElement<'static>> {
    let modal = &contacts.modals.guardian_setup;
    if modal.visible {
        let guardian_contacts: Vec<GuardianCandidateProps> = modal
            .contacts
            .iter()
            .map(|c| GuardianCandidateProps {
                id: c.id.clone(),
                name: c.name.clone(),
                is_current_guardian: c.is_current_guardian,
            })
            .collect();

        Some(
            element! {
                ModalFrame {
                    GuardianSetupModal(
                        visible: true,
                        kind: GuardianSetupKind::Guardian,
                        step: modal.step.clone(),
                        contacts: guardian_contacts,
                        selected_indices: modal.selected_indices.clone(),
                        focused_index: modal.focused_index,
                        threshold_k: modal.threshold_k,
                        threshold_n: modal.threshold_n,
                        ceremony_responses: modal.ceremony_responses.clone(),
                        agreement_mode: modal.agreement_mode,
                        reversion_risk: modal.reversion_risk,
                        error: modal.error.clone(),
                    )
                }
            }
            .into_any(),
        )
    } else {
        None
    }
}

pub fn render_mfa_setup_modal(settings: &SettingsViewProps) -> Option<AnyElement<'static>> {
    let modal = &settings.modals.mfa_setup;
    if modal.visible {
        let device_candidates: Vec<GuardianCandidateProps> = modal
            .contacts
            .iter()
            .map(|c| GuardianCandidateProps {
                id: c.id.clone(),
                name: c.name.clone(),
                is_current_guardian: c.is_current_guardian,
            })
            .collect();

        Some(
            element! {
                ModalFrame {
                    GuardianSetupModal(
                        visible: true,
                        kind: GuardianSetupKind::Mfa,
                        step: modal.step.clone(),
                        contacts: device_candidates,
                        selected_indices: modal.selected_indices.clone(),
                        focused_index: modal.focused_index,
                        threshold_k: modal.threshold_k,
                        threshold_n: modal.threshold_n,
                        ceremony_responses: modal.ceremony_responses.clone(),
                        agreement_mode: modal.agreement_mode,
                        reversion_risk: modal.reversion_risk,
                        error: modal.error.clone(),
                    )
                }
            }
            .into_any(),
        )
    } else {
        None
    }
}

// =============================================================================
// Chat Screen Modal Render Functions
// =============================================================================

pub fn render_chat_create_modal(chat: &ChatViewProps) -> Option<AnyElement<'static>> {
    let modal = &chat.modals.create;
    render_modal(
        modal.visible,
        element! {
            ChatCreateModal(
                visible: true,
                focused: true,
                step: modal.step.clone(),
                name: modal.name.clone(),
                topic: modal.topic.clone(),
                contacts: modal.contacts.clone(),
                selected_indices: modal.selected_indices.clone(),
                focused_index: modal.focused_index,
                threshold_k: modal.threshold_k,
                threshold_n: modal.threshold_n,
                active_field: modal.active_field,
                members_count: modal.member_count,
                error: modal.error.clone(),
                creating: false,
                status: modal.status.clone(),
            )
        }
        .into_any(),
    )
}

pub fn render_topic_modal(chat: &ChatViewProps) -> Option<AnyElement<'static>> {
    let modal = &chat.modals.topic;
    let active_field = modal.active_field;

    let name_display = if modal.name.is_empty() {
        "Enter channel name...".to_string()
    } else {
        modal.name.clone()
    };
    let topic_display = if modal.value.is_empty() {
        "Enter topic...".to_string()
    } else {
        modal.value.clone()
    };

    let name_color = if modal.name.is_empty() {
        Theme::TEXT_MUTED
    } else {
        Theme::TEXT
    };
    let topic_color = if modal.value.is_empty() {
        Theme::TEXT_MUTED
    } else {
        Theme::TEXT
    };

    let name_border = if active_field == 0 {
        Theme::PRIMARY
    } else {
        Theme::BORDER
    };
    let topic_border = if active_field == 1 {
        Theme::PRIMARY
    } else {
        Theme::BORDER
    };

    let header_props = crate::tui::components::ModalHeaderProps::new("Edit Channel".to_string());
    let footer_props = crate::tui::components::ModalFooterProps::new(vec![
        crate::tui::types::KeyHint::new("Esc", "Cancel"),
        crate::tui::types::KeyHint::new("Tab", "Switch field"),
        crate::tui::types::KeyHint::new("Enter", "Save"),
    ]);

    render_modal(
        modal.visible,
        element! {
            ModalScaffold(
                header: header_props,
                footer: footer_props,
                status: crate::tui::components::ModalStatus::Idle,
                border_color: Some(Theme::PRIMARY),
                body_overflow: Overflow::Hidden,
            ) {
                // Name field
                View(margin_bottom: Spacing::XS) {
                    Text(
                        content: "Name",
                        color: if active_field == 0 { Theme::PRIMARY } else { Theme::TEXT_MUTED },
                        weight: Weight::Bold,
                    )
                }
                View(
                    width: 100pct,
                    flex_direction: FlexDirection::Column,
                    border_style: crate::tui::theme::Borders::INPUT,
                    border_color: name_border,
                    padding: Spacing::PANEL_PADDING,
                    margin_bottom: Spacing::SM,
                ) {
                    Text(content: name_display, color: name_color)
                }
                // Topic field
                View(margin_bottom: Spacing::XS) {
                    Text(
                        content: "Description",
                        color: if active_field == 1 { Theme::PRIMARY } else { Theme::TEXT_MUTED },
                        weight: Weight::Bold,
                    )
                }
                View(
                    width: 100pct,
                    flex_direction: FlexDirection::Column,
                    border_style: crate::tui::theme::Borders::INPUT,
                    border_color: topic_border,
                    padding: Spacing::PANEL_PADDING,
                    margin_bottom: Spacing::XS,
                ) {
                    Text(content: topic_display, color: topic_color)
                }
            }
        }
        .into_any(),
    )
}

pub fn render_channel_info_modal(chat: &ChatViewProps) -> Option<AnyElement<'static>> {
    let modal = &chat.modals.info;
    render_modal(
        modal.visible,
        element! {
            ChannelInfoModal(
                visible: true,
                channel_name: modal.channel_name.clone(),
                topic: modal.topic.clone(),
                participants: modal.participants.clone(),
            )
        }
        .into_any(),
    )
}

// =============================================================================
// Settings Screen Modal Render Functions
// =============================================================================

pub fn render_nickname_suggestion_modal(
    settings: &SettingsViewProps,
) -> Option<AnyElement<'static>> {
    let modal = &settings.modals.nickname_suggestion;
    render_text_input_modal(TextInputOverlayProps {
        visible: modal.visible,
        focused: true,
        title: "Edit Nickname".to_string(),
        value: modal.value.clone(),
        placeholder: "Enter what you want to be called...".to_string(),
        hint: format!("Up to {MAX_NICKNAME_SUGGESTION_LENGTH} characters"),
        error: modal.error.clone(),
        submitting: false,
    })
}

pub fn render_add_device_modal(settings: &SettingsViewProps) -> Option<AnyElement<'static>> {
    let modal = &settings.modals.add_device;
    if !modal.visible {
        return None;
    }
    render_modal(true, add_device_modal_body(modal))
}

fn add_device_modal_body(modal: &AddDeviceModalViewProps) -> AnyElement<'static> {
    let name_props =
        crate::tui::components::LabeledInputProps::new("Device name", "e.g. Mobile, Laptop")
            .with_value(modal.name.clone())
            .with_focused(!modal.invitee_focused);
    let invitee_props = crate::tui::components::LabeledInputProps::new(
        "New device's setup code",
        "Export on the new device, then paste here",
    )
    .with_value(modal.setup_code.clone())
    .with_focused(modal.invitee_focused);

    let header_props = crate::tui::components::ModalHeaderProps::new("Add Device — Step 1 of 3")
        .with_subtitle("Transfer the setup code from the new device.");
    let footer_props = crate::tui::components::ModalFooterProps::new(vec![
        crate::tui::types::KeyHint::new("Esc", "Cancel"),
        crate::tui::types::KeyHint::new("Tab", "Switch field"),
        crate::tui::types::KeyHint::new("Enter", "Continue"),
    ]);
    let (status, border_color) = if modal.error.is_empty() {
        (crate::tui::components::ModalStatus::Idle, Theme::PRIMARY)
    } else {
        (
            crate::tui::components::ModalStatus::Error(modal.error.clone()),
            Theme::ERROR,
        )
    };

    element! {
        ModalScaffold(
            header: header_props,
            footer: footer_props,
            status: status,
            border_color: Some(border_color),
            body_overflow: Overflow::Hidden,
        ) {
            View(margin_bottom: Spacing::XS) {
                #(Some(crate::tui::components::labeled_input(&name_props).into()))
            }
            View(margin_bottom: Spacing::XS) {
                #(Some(crate::tui::components::labeled_input(&invitee_props).into()))
            }
        }
    }
    .into_any()
}

pub fn render_device_import_modal(settings: &SettingsViewProps) -> Option<AnyElement<'static>> {
    let modal = &settings.modals.device_import;
    let fields = [
        ("Enrollment code", &modal.code),
        ("Signed manifest", &modal.manifest_code),
        (
            "Initiator verifier (separate transfer)",
            &modal.initiator_verifier_code,
        ),
    ];
    render_modal(modal.visible,element! {
        View(flex_direction:FlexDirection::Column,padding:Spacing::MODAL_PADDING) {
            Text(content:"Import Device Enrollment",weight:Weight::Bold,color:Theme::PRIMARY)
            #(fields.iter().enumerate().map(|(index,(label,value))| {
                element! {
                    View(flex_direction:FlexDirection::Column,margin_top:Spacing::SM) {
                        Text(content:format!("{}{}",if modal.focused_input==index {"> "}else{"  "},label),color:Theme::TEXT_MUTED)
                        Text(content:(*value).clone(),color:Theme::PRIMARY)
                    }
                }.into_any()
            }).collect::<Vec<_>>())
            Text(content:"Tab: next input • Enter: import all three values",color:Theme::TEXT_MUTED)
        }
    }.into_any())
}

pub fn render_device_enrollment_modal(settings: &SettingsViewProps) -> Option<AnyElement<'static>> {
    let modal = &settings.modals.device_enrollment;
    render_modal(
        modal.visible,
        element! {
            DeviceEnrollmentModal(
                visible: true,
                nickname_suggestion: modal.nickname_suggestion.clone(),
                enrollment_code:modal.code.clone(),
                manifest_transfer:modal.manifest_transfer.clone(),
                accepted_count: modal.accepted_count,
                total_count: modal.total_count,
                threshold: modal.threshold,
                is_complete: modal.is_complete,
                has_failed: modal.has_failed,
                error_message: modal.error_message.clone(),
                agreement_mode: modal.agreement_mode,
                reversion_risk: modal.reversion_risk,
                copied: modal.copied,
                is_demo_mode: modal.is_demo_mode,
            )
        }
        .into_any(),
    )
}

pub fn render_device_select_modal(settings: &SettingsViewProps) -> Option<AnyElement<'static>> {
    let modal = &settings.modals.device_select;
    render_modal(
        modal.visible,
        element! {
            DeviceSelectModal(
                visible: true,
                title: "Select Device to Remove".to_string(),
                devices: modal.devices.clone(),
                selected_index: modal.selected_index,
            )
        }
        .into_any(),
    )
}

pub fn render_remove_device_modal(settings: &SettingsViewProps) -> Option<AnyElement<'static>> {
    let modal = &settings.modals.confirm_remove;
    render_modal(
        modal.visible,
        element! {
            ConfirmModal(
                visible: true,
                title: "Remove Device".to_string(),
                message: format!("Are you sure you want to remove \"{}\"?", modal.display_name),
                confirm_text: "Remove".to_string(),
                cancel_text: "Cancel".to_string(),
                confirm_focused: modal.confirm_focused,
            )
        }
        .into_any(),
    )
}

// =============================================================================
// Neighborhood Screen Modal Render Functions
// =============================================================================

pub fn render_home_create_modal(
    neighborhood: &NeighborhoodViewProps,
) -> Option<AnyElement<'static>> {
    let modal = &neighborhood.modals.home_create;
    render_modal(
        modal.visible,
        element! {
            HomeCreateModal(
                state: crate::tui::state::views::HomeCreateModalState {
                    name: modal.name.clone(),
                    description: modal.description.clone(),
                    active_field: modal.active_field,
                    error: modal.error.clone(),
                    creating: modal.creating,
                },
            )
        }
        .into_any(),
    )
}

pub fn render_moderator_assignment_modal(
    neighborhood: &NeighborhoodViewProps,
) -> Option<AnyElement<'static>> {
    let modal = &neighborhood.modals.moderator_assignment;
    if modal.visible {
        let title = if modal.assign {
            "Assign Moderator"
        } else {
            "Revoke Moderator"
        };
        let hint = if modal.contacts.is_empty() {
            "No candidates available".to_string()
        } else if modal.assign {
            "Enter=apply • Tab=toggle revoke • Esc=cancel".to_string()
        } else {
            "Enter=apply • Tab=toggle assign • Esc=cancel".to_string()
        };

        render_contact_selection_modal(ContactSelectionOverlayProps {
            visible: true,
            title: title.to_string(),
            contacts: modal.contacts.clone(),
            selected_index: modal.selected_index,
            error: hint,
            selected_ids: Vec::new(),
            multi_select: false,
        })
    } else {
        None
    }
}

pub fn render_access_override_modal(
    neighborhood: &NeighborhoodViewProps,
) -> Option<AnyElement<'static>> {
    let modal = &neighborhood.modals.access_override;
    if modal.visible {
        let hint = format!(
            "Override: {} • Enter=apply • Tab=toggle • Esc=cancel",
            modal.level.label()
        );
        render_contact_selection_modal(ContactSelectionOverlayProps {
            visible: true,
            title: "Access Override".to_string(),
            contacts: modal.contacts.clone(),
            selected_index: modal.selected_index,
            error: hint,
            selected_ids: Vec::new(),
            multi_select: false,
        })
    } else {
        None
    }
}

pub fn render_capability_config_modal(
    neighborhood: &NeighborhoodViewProps,
) -> Option<AnyElement<'static>> {
    let modal = &neighborhood.modals.capability_config;
    if modal.visible {
        let current_field = match modal.active_field {
            0 => "Full",
            1 => "Partial",
            _ => "Limited",
        };
        let error = modal.error.clone().unwrap_or_default();

        Some(
            element! {
                ModalFrame {
                    View(
                        flex_direction: FlexDirection::Column,
                        border_style: BorderStyle::Round,
                        border_color: crate::tui::theme::Theme::PRIMARY,
                        width: 82,
                        padding_left: 1,
                        padding_right: 1,
                        padding_top: 1,
                        padding_bottom: 1,
                        gap: 1,
                    ) {
                        Text(content: "Home Capability Configuration", weight: Weight::Bold, color: crate::tui::theme::Theme::PRIMARY)
                        Text(content: "Tab=next field • Enter=save • Esc=cancel", color: crate::tui::theme::Theme::TEXT_MUTED)
                        Text(content: format!("Editing: {}", current_field), color: crate::tui::theme::Theme::SECONDARY)
                        Text(content: format!("Full: {}", modal.full_caps), color: crate::tui::theme::Theme::TEXT)
                        Text(content: format!("Partial: {}", modal.partial_caps), color: crate::tui::theme::Theme::TEXT)
                        Text(content: format!("Limited: {}", modal.limited_caps), color: crate::tui::theme::Theme::TEXT)
                        #(if error.is_empty() {
                            None
                        } else {
                            Some(element! { Text(content: error, color: crate::tui::theme::Theme::ERROR) })
                        })
                    }
                }
            }
            .into_any(),
        )
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_device_modal_renders_invitee_field_and_inline_error() {
        let modal = AddDeviceModalViewProps {
            visible: true,
            name: "Laptop".to_string(),
            setup_code: String::new(),
            invitee_focused: true,
            error: "Enter the new device's setup code".to_string(),
        };
        let rendered = add_device_modal_body(&modal).to_string();
        assert!(rendered.contains("Device name"), "{rendered}");
        assert!(rendered.contains("setup code"), "{rendered}");
        assert!(
            rendered.contains("Enter the new device's setup code"),
            "{rendered}"
        );
    }
}

/// Render the retained authority picker through the shared observed row scaffold.
/// Highlighting selects a candidate; the original keyboard command owns switching.
pub fn render_authority_picker_modal(settings: &SettingsViewProps) -> Option<AnyElement<'static>> {
    let modal = &settings.modals.authority_picker;
    render_modal(modal.visible, element! {
        crate::tui::components::SelectionModal(
            visible: true, title: "Select Authority".to_string(), empty_message: "No authorities available".to_string(),
            rows: modal.authorities.iter().map(|(id, label)| crate::tui::components::SelectionModalRow {
                id: id.clone(), label: label.clone(),
            }).collect::<Vec<_>>(),
            selected_index: modal.selected_index,
        )
    }.into_any())
}

#[cfg(test)]
mod authority_picker_tests {
    use super::*;
    use crate::tui::props::extract_settings_view_props;
    use crate::tui::state::{
        transition, ContactSelectModalState, DispatchCommand, QueuedModal, TuiCommand, TuiState,
    };
    use aura_core::effects::terminal::events;
    use aura_core::types::identifiers::AuthorityId;

    fn canvas(state: &TuiState) -> String {
        let props = extract_settings_view_props(state);
        let modal = render_authority_picker_modal(&props).expect("live picker renderer");
        let mut element = element! { View(width: 80, height: 31) { #(Some(modal)) } };
        element.render(None).to_string()
    }
    #[test]
    fn retained_authority_picker_renders_rows_and_navigation_only_selects_candidate() {
        let first = AuthorityId::new_from_entropy([71; 32]);
        let second = AuthorityId::new_from_entropy([72; 32]);
        let mut state = TuiState::new();
        state.modal_queue.enqueue(QueuedModal::AuthorityPicker(
            ContactSelectModalState::single(
                "Select Authority",
                vec![
                    (first.to_string().into(), "First authority".into()),
                    (second.to_string().into(), "Second authority".into()),
                ],
            ),
        ));
        let original_current = state.current_authority_index;
        let initial = canvas(&state);
        assert!(initial.contains("Select Authority"), "{initial}");
        assert!(initial.contains("➤ First authority"), "{initial}");
        assert!(initial.contains("Second authority"), "{initial}");
        let (down, commands) = transition(&state, events::arrow_down());
        assert!(commands.is_empty());
        assert_eq!(down.current_authority_index, original_current);
        let moved = canvas(&down);
        assert!(moved.contains("➤ Second authority"), "{moved}");
        let (up, commands) = transition(&down, events::arrow_up());
        assert!(commands.is_empty());
        assert!(canvas(&up).contains("➤ First authority"));
        let (cancelled, cancelled_commands) = transition(&down, events::escape());
        assert!(cancelled_commands.is_empty());
        assert_eq!(cancelled.current_authority_index, original_current);
        assert!(!cancelled.has_queued_modal());
        assert!(render_authority_picker_modal(&extract_settings_view_props(&cancelled)).is_none());
        let (selected, commands) = transition(&down, events::enter());
        assert_eq!(selected.current_authority_index, original_current);
        assert!(!selected.has_queued_modal());
        assert!(
            matches!(commands.as_slice(), [TuiCommand::Dispatch(DispatchCommand::SwitchAuthority { authority_id })] if *authority_id == second)
        );
        assert!(render_authority_picker_modal(&extract_settings_view_props(&selected)).is_none());
    }
    #[test]
    fn empty_authority_picker_cannot_dispatch_a_target() {
        let mut state = TuiState::new();
        state.modal_queue.enqueue(QueuedModal::AuthorityPicker(
            ContactSelectModalState::single("Select Authority", Vec::new()),
        ));
        let original_current = state.current_authority_index;
        assert!(canvas(&state).contains("No authorities available"));
        let (state, commands) = transition(&state, events::enter());
        assert!(commands.is_empty());
        assert_eq!(state.current_authority_index, original_current);
        assert!(!state.has_queued_modal());
    }
}
