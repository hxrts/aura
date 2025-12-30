//! # Modal Overlays Rendering
//!
//! Render functions for all modal overlays in the IoApp component.
//! Modals must render at root level for proper overlay positioning via ModalFrame.
//!
//! This module provides standalone render functions that can be called from IoApp.

use iocraft::prelude::*;

use crate::tui::components::{
    AccountSetupModal, ConfirmModal, ContactSelectModal, HelpModal, ModalFrame, TextInputModal,
};
use crate::tui::props::{
    ChatViewProps, ContactsViewProps, NeighborhoodViewProps, SettingsViewProps,
};
use crate::tui::screens::{
    HomeCreateModal, ChannelInfoModal, ChatCreateModal, DeviceEnrollmentModal,
    GuardianCandidateProps, GuardianSetupKind, GuardianSetupModal, InvitationCodeModal,
    InvitationCreateModal, InvitationImportModal,
};
use crate::tui::types::{Contact, InvitationType};

// =============================================================================
// Global Modal Props
// =============================================================================

/// Props for global modals (not screen-specific)
#[derive(Default, Clone)]
pub struct GlobalModalProps {
    // Account setup modal
    pub account_setup_visible: bool,
    pub account_setup_display_name: String,
    pub account_setup_creating: bool,
    pub account_setup_show_spinner: bool,
    pub account_setup_success: bool,
    pub account_setup_error: Option<String>,

    // Guardian selection modal
    pub guardian_modal_visible: bool,
    pub guardian_modal_title: String,
    pub guardian_modal_contacts: Vec<Contact>,
    pub guardian_modal_selected: usize,
    pub guardian_modal_error: Option<String>,

    pub guardian_modal_selected_ids: Vec<String>,
    pub guardian_modal_multi_select: bool,

    // Generic contact selection modal
    pub contact_modal_visible: bool,
    pub contact_modal_title: String,
    pub contact_modal_contacts: Vec<Contact>,
    pub contact_modal_selected: usize,
    pub contact_modal_error: Option<String>,

    pub contact_modal_selected_ids: Vec<String>,
    pub contact_modal_multi_select: bool,

    // Confirm dialog modal
    pub confirm_visible: bool,
    pub confirm_title: String,
    pub confirm_message: String,

    // Help modal
    pub help_modal_visible: bool,
    pub current_screen_name: String,
}

// =============================================================================
// Global Modal Render Functions
// =============================================================================

pub fn render_account_setup_modal(global: &GlobalModalProps) -> Option<AnyElement<'static>> {
    if global.account_setup_visible {
        Some(
            element! {
                ModalFrame {
                    AccountSetupModal(
                        visible: true,
                        display_name: global.account_setup_display_name.clone(),
                        focused: true,
                        creating: global.account_setup_creating,
                        show_spinner: global.account_setup_show_spinner,
                        success: global.account_setup_success,
                        error: global.account_setup_error.clone().unwrap_or_default(),
                    )
                }
            }
            .into_any(),
        )
    } else {
        None
    }
}

pub fn render_guardian_modal(global: &GlobalModalProps) -> Option<AnyElement<'static>> {
    if global.guardian_modal_visible {
        Some(
            element! {
                ModalFrame {
                    ContactSelectModal(
                        visible: true,
                        title: global.guardian_modal_title.clone(),
                        contacts: global.guardian_modal_contacts.clone(),
                        selected_index: global.guardian_modal_selected,
                        error: global.guardian_modal_error.clone().unwrap_or_default(),
                        selected_ids: global.guardian_modal_selected_ids.clone(),
                        multi_select: global.guardian_modal_multi_select,
                    )
                }
            }
            .into_any(),
        )
    } else {
        None
    }
}

pub fn render_contact_modal(global: &GlobalModalProps) -> Option<AnyElement<'static>> {
    if global.contact_modal_visible {
        Some(
            element! {
                ModalFrame {
                    ContactSelectModal(
                        visible: true,
                        title: global.contact_modal_title.clone(),
                        contacts: global.contact_modal_contacts.clone(),
                        selected_index: global.contact_modal_selected,
                        error: global.contact_modal_error.clone().unwrap_or_default(),
                        selected_ids: global.contact_modal_selected_ids.clone(),
                        multi_select: global.contact_modal_multi_select,
                    )
                }
            }
            .into_any(),
        )
    } else {
        None
    }
}

pub fn render_confirm_modal(global: &GlobalModalProps) -> Option<AnyElement<'static>> {
    if global.confirm_visible {
        Some(
            element! {
                ModalFrame {
                    ConfirmModal(
                        visible: true,
                        title: global.confirm_title.clone(),
                        message: global.confirm_message.clone(),
                        confirm_text: "Confirm".to_string(),
                        cancel_text: "Cancel".to_string(),
                        confirm_focused: true,
                    )
                }
            }
            .into_any(),
        )
    } else {
        None
    }
}

pub fn render_help_modal(global: &GlobalModalProps) -> Option<AnyElement<'static>> {
    if global.help_modal_visible {
        Some(
            element! {
                ModalFrame {
                    HelpModal(visible: true, current_screen: Some(global.current_screen_name.clone()))
                }
            }
            .into_any(),
        )
    } else {
        None
    }
}

// =============================================================================
// Contacts Screen Modal Render Functions
// =============================================================================

pub fn render_nickname_modal(contacts: &ContactsViewProps) -> Option<AnyElement<'static>> {
    if contacts.nickname_modal_visible {
        // Show hint with suggested name if available
        let hint = contacts
            .nickname_modal_suggested_name
            .as_ref()
            .map(|s| format!("Suggestion: {s}"))
            .unwrap_or_default();

        Some(
            element! {
                ModalFrame {
                    TextInputModal(
                        visible: true,
                        focused: true,
                        title: "Edit Nickname".to_string(),
                        value: contacts.nickname_modal_value.clone(),
                        placeholder: "Enter nickname...".to_string(),
                        hint: hint,
                        error: String::new(),
                        submitting: false,
                    )
                }
            }
            .into_any(),
        )
    } else {
        None
    }
}

pub fn render_contacts_import_modal(contacts: &ContactsViewProps) -> Option<AnyElement<'static>> {
    if contacts.import_modal_visible {
        Some(
            element! {
                ModalFrame {
                    InvitationImportModal(
                        visible: true,
                        focused: true,
                        code: contacts.import_modal_code.clone(),
                        error: String::new(),
                        importing: contacts.import_modal_importing,
                    )
                }
            }
            .into_any(),
        )
    } else {
        None
    }
}

pub fn render_contacts_create_modal(contacts: &ContactsViewProps) -> Option<AnyElement<'static>> {
    if contacts.create_modal_visible {
        let invitation_type = match contacts.create_modal_type_index {
            0 => InvitationType::Guardian,
            1 => InvitationType::Contact,
            _ => InvitationType::Channel,
        };
        Some(
            element! {
                ModalFrame {
                    InvitationCreateModal(
                        visible: true,
                        focused: true,
                        focused_field: contacts.create_modal_focused_field,
                        creating: false,
                        error: String::new(),
                        invitation_type: invitation_type,
                        message: contacts.create_modal_message.clone(),
                        ttl_hours: contacts.create_modal_ttl_hours as u32,
                    )
                }
            }
            .into_any(),
        )
    } else {
        None
    }
}

pub fn render_contacts_code_modal(contacts: &ContactsViewProps) -> Option<AnyElement<'static>> {
    if contacts.code_modal_visible {
        Some(
            element! {
                ModalFrame {
                    InvitationCodeModal(
                        visible: true,
                        code: contacts.code_modal_code.clone(),
                        invitation_type: "Invitation".to_string(),
                        copied: contacts.code_modal_copied,
                    )
                }
            }
            .into_any(),
        )
    } else {
        None
    }
}

pub fn render_guardian_setup_modal(contacts: &ContactsViewProps) -> Option<AnyElement<'static>> {
    if contacts.guardian_setup_modal_visible {
        let guardian_contacts: Vec<GuardianCandidateProps> = contacts
            .guardian_setup_modal_contacts
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
                        step: contacts.guardian_setup_modal_step.clone(),
                        contacts: guardian_contacts,
                        selected_indices: contacts.guardian_setup_modal_selected_indices.clone(),
                        focused_index: contacts.guardian_setup_modal_focused_index,
                        threshold_k: contacts.guardian_setup_modal_threshold_k,
                        threshold_n: contacts.guardian_setup_modal_threshold_n,
                        ceremony_responses: contacts.guardian_setup_modal_ceremony_responses.clone(),
                        agreement_mode: contacts.guardian_setup_modal_agreement_mode,
                        reversion_risk: contacts.guardian_setup_modal_reversion_risk,
                        error: contacts.guardian_setup_modal_error.clone(),
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
    if settings.mfa_setup_modal_visible {
        let device_candidates: Vec<GuardianCandidateProps> = settings
            .mfa_setup_modal_contacts
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
                        step: settings.mfa_setup_modal_step.clone(),
                        contacts: device_candidates,
                        selected_indices: settings.mfa_setup_modal_selected_indices.clone(),
                        focused_index: settings.mfa_setup_modal_focused_index,
                        threshold_k: settings.mfa_setup_modal_threshold_k,
                        threshold_n: settings.mfa_setup_modal_threshold_n,
                        ceremony_responses: settings.mfa_setup_modal_ceremony_responses.clone(),
                        agreement_mode: settings.mfa_setup_modal_agreement_mode,
                        reversion_risk: settings.mfa_setup_modal_reversion_risk,
                        error: settings.mfa_setup_modal_error.clone(),
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
    if chat.create_modal_visible {
        Some(
            element! {
                ModalFrame {
                    ChatCreateModal(
                        visible: true,
                        focused: true,
                        step: chat.create_modal_step.clone(),
                        name: chat.create_modal_name.clone(),
                        topic: chat.create_modal_topic.clone(),
                        contacts: chat.create_modal_contacts.clone(),
                        selected_indices: chat.create_modal_selected_indices.clone(),
                        focused_index: chat.create_modal_focused_index,
                        threshold_k: chat.create_modal_threshold_k,
                        threshold_n: chat.create_modal_threshold_n,
                        active_field: chat.create_modal_active_field,
                        members_count: chat.create_modal_member_count,
                        error: chat.create_modal_error.clone(),
                        creating: false,
                        status: chat.create_modal_status.clone(),
                    )
                }
            }
            .into_any(),
        )
    } else {
        None
    }
}

pub fn render_topic_modal(chat: &ChatViewProps) -> Option<AnyElement<'static>> {
    if chat.topic_modal_visible {
        Some(
            element! {
                ModalFrame {
                    TextInputModal(
                        visible: true,
                        title: "Set Channel Topic".to_string(),
                        value: chat.topic_modal_value.clone(),
                        placeholder: "Enter topic...".to_string(),
                        hint: String::new(),
                        error: String::new(),
                    )
                }
            }
            .into_any(),
        )
    } else {
        None
    }
}

pub fn render_channel_info_modal(chat: &ChatViewProps) -> Option<AnyElement<'static>> {
    if chat.info_modal_visible {
        Some(
            element! {
                ModalFrame {
                    ChannelInfoModal(
                        visible: true,
                        channel_name: chat.info_modal_channel_name.clone(),
                        topic: chat.info_modal_topic.clone(),
                        participants: vec!["You".to_string()],
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
// Settings Screen Modal Render Functions
// =============================================================================

pub fn render_display_name_modal(settings: &SettingsViewProps) -> Option<AnyElement<'static>> {
    if settings.display_name_modal_visible {
        Some(
            element! {
                ModalFrame {
                    TextInputModal(
                        visible: true,
                        focused: true,
                        title: "Edit Display Name".to_string(),
                        value: settings.display_name_modal_value.clone(),
                        placeholder: "Enter your display name...".to_string(),
                        hint: String::new(),
                        error: String::new(),
                        submitting: false,
                    )
                }
            }
            .into_any(),
        )
    } else {
        None
    }
}

pub fn render_add_device_modal(settings: &SettingsViewProps) -> Option<AnyElement<'static>> {
    if settings.add_device_modal_visible {
        Some(
            element! {
                ModalFrame {
                    TextInputModal(
                        visible: true,
                        focused: true,
                        title: "Add Device — Step 1 of 3".to_string(),
                        value: settings.add_device_modal_name.clone(),
                        placeholder: "e.g. Mobile, Laptop".to_string(),
                        hint: "This is the device you're inviting (not the current device)."
                            .to_string(),
                        error: String::new(),
                        submitting: false,
                    )
                }
            }
            .into_any(),
        )
    } else {
        None
    }
}

pub fn render_device_import_modal(settings: &SettingsViewProps) -> Option<AnyElement<'static>> {
    if settings.device_import_modal_visible {
        Some(
            element! {
                ModalFrame {
                    TextInputModal(
                        visible: true,
                        focused: true,
                        title: "Import Device Enrollment Code".to_string(),
                        value: settings.device_import_modal_code.clone(),
                        placeholder: "Paste enrollment code...".to_string(),
                        hint: "Used by the new device to join this account".to_string(),
                        error: String::new(),
                        submitting: false,
                    )
                }
            }
            .into_any(),
        )
    } else {
        None
    }
}

pub fn render_device_enrollment_modal(settings: &SettingsViewProps) -> Option<AnyElement<'static>> {
    if settings.device_enrollment_modal_visible {
        Some(
            element! {
                ModalFrame {
                    DeviceEnrollmentModal(
                        visible: true,
                        device_name: settings.device_enrollment_modal_device_name.clone(),
                        enrollment_code: settings.device_enrollment_modal_code.clone(),
                        accepted_count: settings.device_enrollment_modal_accepted_count,
                        total_count: settings.device_enrollment_modal_total_count,
                        threshold: settings.device_enrollment_modal_threshold,
                        is_complete: settings.device_enrollment_modal_is_complete,
                        has_failed: settings.device_enrollment_modal_has_failed,
                        error_message: settings.device_enrollment_modal_error_message.clone(),
                        agreement_mode: settings.device_enrollment_modal_agreement_mode,
                        reversion_risk: settings.device_enrollment_modal_reversion_risk,
                        copied: settings.device_enrollment_modal_copied,
                    )
                }
            }
            .into_any(),
        )
    } else {
        None
    }
}

pub fn render_remove_device_modal(settings: &SettingsViewProps) -> Option<AnyElement<'static>> {
    if settings.confirm_remove_modal_visible {
        Some(
            element! {
                ModalFrame {
                    ConfirmModal(
                        visible: true,
                        title: "Remove Device".to_string(),
                        message: format!(
                            "Are you sure you want to remove \"{}\"?",
                            settings.confirm_remove_modal_device_name
                        ),
                        confirm_text: "Remove".to_string(),
                        cancel_text: "Cancel".to_string(),
                        confirm_focused: settings.confirm_remove_modal_confirm_focused,
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
// Neighborhood Screen Modal Render Functions
// =============================================================================

pub fn render_home_create_modal(
    neighborhood: &NeighborhoodViewProps,
) -> Option<AnyElement<'static>> {
    if neighborhood.home_create_modal_visible {
        Some(
            element! {
                ModalFrame {
                    HomeCreateModal(
                        state: crate::tui::state::views::HomeCreateModalState {
                            name: neighborhood.home_create_modal_name.clone(),
                            description: neighborhood.home_create_modal_description.clone(),
                            active_field: neighborhood.home_create_modal_active_field,
                            error: neighborhood.home_create_modal_error.clone(),
                            creating: neighborhood.home_create_modal_creating,
                        },
                    )
                }
            }
            .into_any(),
        )
    } else {
        None
    }
}
