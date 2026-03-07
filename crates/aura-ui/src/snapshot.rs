//! Canonical text snapshot rendering for UI state comparison.
//!
//! Renders the UI model as a deterministic text representation suitable for
//! snapshot testing and harness assertions across different rendering backends.

use crate::model::{
    AccessOverrideLevel, ActiveModal, AddDeviceWizardStep, CapabilityTier,
    CreateChannelDetailsField, CreateChannelWizardStep, ModalState, NeighborhoodMode,
    SettingsSection, ThresholdWizardStep, UiModel, UiScreen,
};

const PANEL_WIDTH: usize = 38;
const CONTENT_ROWS: usize = 20;
pub fn render_canonical_snapshot(model: &UiModel) -> String {
    let mut lines = Vec::with_capacity(CONTENT_ROWS + 4);
    let authority_label = format!("Authority: {} (local)", model.authority_id);
    let mut authority_written = false;

    lines.push("Neighborhood Chat Contacts Notifications Settings".to_string());
    lines.push("┌────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐".to_string());

    for row_idx in 0..CONTENT_ROWS {
        let (mut left, mut center, mut right) = panel_row(model, row_idx);
        apply_modal_overlay(model, row_idx, &mut left, &mut center, &mut right);
        if !authority_written && right.is_empty() {
            right = authority_label.clone();
            authority_written = true;
        }
        lines.push(format_panel_row(&left, &center, &right));
    }

    if !authority_written {
        lines.push(format!("│ {authority_label:<114} │"));
    }

    if let Some(toast) = &model.toast {
        lines.push(format!(
            "│ {:<114} │",
            format!("{} {} [Esc] dismiss", toast.icon, toast.message)
        ));
    }

    lines.join("\n")
}

fn panel_row(model: &UiModel, row_idx: usize) -> (String, String, String) {
    match model.screen {
        UiScreen::Neighborhood => neighborhood_row(model, row_idx),
        UiScreen::Chat => chat_row(model, row_idx),
        UiScreen::Contacts => contacts_row(model, row_idx),
        UiScreen::Notifications => notifications_row(model, row_idx),
        UiScreen::Settings => settings_row(model, row_idx),
    }
}

fn neighborhood_row(model: &UiModel, row_idx: usize) -> (String, String, String) {
    match model.neighborhood_mode {
        NeighborhoodMode::Map => {
            if row_idx == 0 {
                return (
                    "Neighborhood".to_string(),
                    "Map".to_string(),
                    "Welcome to Aura".to_string(),
                );
            }
            if row_idx == 1 {
                return (
                    "➤ Homes".to_string(),
                    String::new(),
                    model
                        .selected_home
                        .as_ref()
                        .map(|home| format!("Selected home: {}", home.name))
                        .unwrap_or_else(|| "Selected home: none".to_string()),
                );
            }
            if row_idx == 2 {
                return (
                    format!("Can enter: {}", model.access_depth.label()),
                    String::new(),
                    String::new(),
                );
            }
            if row_idx == 3 {
                return (
                    "Members & Participants".to_string(),
                    String::new(),
                    String::new(),
                );
            }
            if row_idx == 4 {
                return ("Member".to_string(), String::new(), String::new());
            }
            if row_idx == 5 {
                return (
                    String::new(),
                    String::new(),
                    format!("Access: {}", model.access_depth.label()),
                );
            }
            if row_idx == 6 {
                return (
                    String::new(),
                    String::new(),
                    format!("{} M:Off P:0", model.access_depth.compact()),
                );
            }
            if row_idx == 7 {
                return (
                    String::new(),
                    String::new(),
                    model.access_depth.compact().to_string(),
                );
            }
            (String::new(), String::new(), String::new())
        }
        NeighborhoodMode::Detail => {
            if row_idx == 0 {
                return (
                    "Neighborhood".to_string(),
                    "Details".to_string(),
                    "Welcome to Aura".to_string(),
                );
            }
            if row_idx == 1 {
                return (
                    "Members/Participants:".to_string(),
                    String::new(),
                    model
                        .selected_home
                        .as_ref()
                        .map(|home| format!("Selected home: {}", home.name))
                        .unwrap_or_else(|| "Selected home: none".to_string()),
                );
            }
            if row_idx == 2 {
                return ("Member".to_string(), String::new(), String::new());
            }
            if row_idx == 3 {
                return (
                    String::new(),
                    String::new(),
                    format!("Access: {}", model.access_depth.label()),
                );
            }
            if row_idx == 4 {
                return (
                    String::new(),
                    String::new(),
                    format!("{} M:Off P:0", model.access_depth.compact()),
                );
            }
            (String::new(), String::new(), String::new())
        }
    }
}

fn chat_row(model: &UiModel, row_idx: usize) -> (String, String, String) {
    if row_idx == 0 {
        let channel = model.selected_channel_name().unwrap_or("general");
        let topic = model.selected_channel_topic();
        return (
            "Channels".to_string(),
            format!("Channel: #{channel}"),
            format!("Topic: {topic}"),
        );
    }

    if row_idx > 0 && row_idx <= model.channels.len() {
        let channel = &model.channels[row_idx - 1];
        let prefix = if channel.selected { "➤ " } else { "" };
        return (
            format!("{prefix}# {}", channel.name),
            String::new(),
            String::new(),
        );
    }

    let message_offset = row_idx.saturating_sub(4);
    if let Some(message) = model.messages.get(message_offset) {
        return (String::new(), String::new(), message.clone());
    }

    if row_idx == 4 && model.messages.is_empty() {
        return (String::new(), String::new(), "No messages yet".to_string());
    }

    if row_idx == CONTENT_ROWS - 1 {
        let mode = if model.input_mode { "insert" } else { "normal" };
        let value = if model.input_mode {
            model.input_buffer.clone()
        } else {
            String::new()
        };
        return (format!("mode: {mode}"), value, String::new());
    }

    (String::new(), String::new(), String::new())
}

fn contacts_row(model: &UiModel, row_idx: usize) -> (String, String, String) {
    if row_idx == 0 {
        return (
            format!("Contacts ({})", model.contacts.len()),
            String::new(),
            if model.contact_details {
                "Details".to_string()
            } else {
                "Select a contact".to_string()
            },
        );
    }

    if row_idx > 0 && row_idx <= model.contacts.len() {
        let contact = &model.contacts[row_idx - 1];
        let prefix = if contact.selected { "➤ " } else { "" };
        return (
            format!("{prefix}○ {}", contact.name),
            String::new(),
            if model.contact_details
                && model.selected_contact_index() == Some(row_idx.saturating_sub(1))
            {
                format!("Nickname: {}", contact.name)
            } else {
                String::new()
            },
        );
    }

    if row_idx == model.contacts.len().saturating_add(2) {
        return (
            format!("Last scan: {}", model.last_scan),
            String::new(),
            String::new(),
        );
    }

    (String::new(), String::new(), String::new())
}

fn notifications_row(model: &UiModel, row_idx: usize) -> (String, String, String) {
    if row_idx == 0 {
        return (
            "Notifications".to_string(),
            String::new(),
            "No notifications".to_string(),
        );
    }
    if row_idx == 1 {
        return (
            String::new(),
            String::new(),
            "Select a notification".to_string(),
        );
    }
    if let Some(entry) = model.notifications.get(row_idx.saturating_sub(2)) {
        return (String::new(), String::new(), entry.clone());
    }
    (String::new(), String::new(), String::new())
}

fn settings_row(model: &UiModel, row_idx: usize) -> (String, String, String) {
    if row_idx == 0 {
        return (
            "Settings".to_string(),
            String::new(),
            "Storage: IndexedDB".to_string(),
        );
    }

    if row_idx > 0 && row_idx <= SettingsSection::ALL.len() {
        let idx = row_idx - 1;
        let section = SettingsSection::from_index(idx);
        let prefix = if section == model.settings_section {
            "➤ "
        } else {
            ""
        };
        let right = if matches!(section, SettingsSection::Profile) {
            format!("Nickname: {}", model.profile_nickname)
        } else if matches!(section, SettingsSection::Devices) {
            let device_count = if model.has_secondary_device { 2 } else { 1 };
            format!("Devices: {device_count}")
        } else if matches!(section, SettingsSection::Authority) {
            format!("Authority: {} (local)", model.authority_id)
        } else {
            String::new()
        };
        return (format!("{prefix}{}", section.title()), String::new(), right);
    }

    (String::new(), String::new(), String::new())
}

fn apply_modal_overlay(
    model: &UiModel,
    row_idx: usize,
    _left: &mut String,
    center: &mut String,
    right: &mut String,
) {
    let Some(modal) = model.modal_state() else {
        return;
    };

    match modal {
        ModalState::Help => {
            if row_idx == 0 {
                *center = model.modal_hint.clone();
            } else if row_idx == 1 {
                *center = "Use ? for TUI help".to_string();
            }
        }
        ModalState::CreateInvitation => {
            if row_idx == 0 {
                *center = "Invite Contacts".to_string();
            } else if row_idx == 1 {
                *center = if model.modal_hint.is_empty() {
                    "Press Enter to create invitation".to_string()
                } else {
                    model.modal_hint.clone()
                };
            }
        }
        ModalState::AcceptInvitation => {
            if row_idx == 0 {
                *center = model.modal_hint.clone();
            } else if row_idx == 1 {
                *center = model.modal_text_value().unwrap_or_default();
            }
        }
        ModalState::CreateHome => {
            if row_idx == 0 {
                *center = "Create New Home".to_string();
            } else if row_idx == 1 {
                *center = model.modal_text_value().unwrap_or_default();
            }
        }
        ModalState::CreateChannel => {
            if let Some(state) = model.create_channel_modal() {
                if row_idx == 0 {
                    *center = if model.modal_hint.is_empty() {
                        "New Chat Group".to_string()
                    } else {
                        model.modal_hint.clone()
                    };
                } else if row_idx == 1 {
                    *center = match state.step {
                        CreateChannelWizardStep::Details => match state.active_field {
                            CreateChannelDetailsField::Name => state.name.clone(),
                            CreateChannelDetailsField::Topic => state.topic.clone(),
                        },
                        CreateChannelWizardStep::Members => {
                            format!("selected: {}", state.selected_members.len())
                        }
                        CreateChannelWizardStep::Threshold => state.threshold.to_string(),
                    };
                }
            }
        }
        ModalState::SetChannelTopic => {
            if row_idx == 0 {
                *center = "Set Channel Topic".to_string();
            } else if row_idx == 1 {
                *center = model.modal_text_value().unwrap_or_default();
            }
        }
        ModalState::ChannelInfo => {
            if row_idx == 0 {
                *center = model.modal_hint.clone();
            }
        }
        ModalState::EditNickname => {
            if row_idx == 0 {
                *center = "Edit Nickname".to_string();
            } else if row_idx == 1 {
                *center = model.modal_text_value().unwrap_or_default();
            }
        }
        ModalState::RemoveContact => {
            if row_idx == 0 {
                *center = "Remove Contact".to_string();
            }
        }
        ModalState::GuardianSetup => {
            if let Some(state) = model.guardian_setup_modal() {
                match state.step {
                    ThresholdWizardStep::Selection => {
                        if row_idx == 0 {
                            *center = "Guardian Setup — Step 1 of 3".to_string();
                        } else if row_idx == 1 {
                            *center = format!("selected: {}", state.selected_indices.len());
                        }
                    }
                    ThresholdWizardStep::Threshold => {
                        if row_idx == 0 {
                            *center = "Guardian Setup — Step 2 of 3".to_string();
                        } else if row_idx == 1 {
                            *center = state.threshold_input.clone();
                        }
                    }
                    ThresholdWizardStep::Ceremony => {
                        if row_idx == 0 {
                            *center = "Guardian Setup — Step 3 of 3".to_string();
                        } else if row_idx == 1 {
                            *center = format!(
                                "{} of {} approvals",
                                state.threshold_k, state.selected_count
                            );
                        }
                    }
                }
            }
        }
        ModalState::RequestRecovery => {
            if row_idx == 0 {
                *center = "Request Recovery".to_string();
            } else if row_idx == 1 {
                *center = "Notify guardians to begin recovery".to_string();
            }
        }
        ModalState::AddDeviceStep1 => {
            if let Some(state) = model.add_device_modal() {
                match state.step {
                    AddDeviceWizardStep::Name => {
                        if row_idx == 0 {
                            *center = "Add Device — Step 1 of 3".to_string();
                        } else if row_idx == 1 {
                            *center = state.name_input.clone();
                        }
                    }
                    AddDeviceWizardStep::ShareCode => {
                        if row_idx == 0 {
                            *center = "Add Device — Step 2 of 3".to_string();
                        } else if row_idx == 1 {
                            *center = format!("Code: {}", state.enrollment_code);
                        }
                    }
                    AddDeviceWizardStep::Confirm => {
                        if row_idx == 0 {
                            *center = "Add Device — Step 3 of 3".to_string();
                        } else if row_idx == 1 {
                            *center = format!("Invite '{}'", state.device_name);
                        }
                    }
                }
            }
        }
        ModalState::ImportDeviceEnrollmentCode => {
            let code = model.modal_text_value().unwrap_or_default();
            if row_idx == 0 {
                *center = "Import Device Enrollment Code".to_string();
            } else if row_idx == 1 {
                *center = code;
            }
        }
        ModalState::SelectDeviceToRemove => {
            if row_idx == 0 {
                *center = "Select Device to Remove".to_string();
            } else if row_idx == 1 {
                *center = model
                    .secondary_device_name()
                    .or_else(|| {
                        model
                            .selected_device_modal()
                            .map(|state| state.candidate_name.as_str())
                    })
                    .unwrap_or("Secondary device")
                    .to_string();
            }
        }
        ModalState::ConfirmRemoveDevice => {
            if row_idx == 0 {
                *center = "Confirm Device Removal".to_string();
            } else if row_idx == 1 {
                *center = model
                    .secondary_device_name()
                    .or_else(|| {
                        model
                            .selected_device_modal()
                            .map(|state| state.candidate_name.as_str())
                    })
                    .unwrap_or("Secondary device")
                    .to_string();
            }
        }
        ModalState::MfaSetup => {
            if let Some(state) = model.mfa_setup_modal() {
                match state.step {
                    ThresholdWizardStep::Selection => {
                        if row_idx == 0 {
                            *center = "Multifactor Setup — Step 1 of 3".to_string();
                        } else if row_idx == 1 {
                            *center = format!("selected: {}", state.selected_indices.len());
                        }
                    }
                    ThresholdWizardStep::Threshold => {
                        if row_idx == 0 {
                            *center = "Multifactor Setup — Step 2 of 3".to_string();
                        } else if row_idx == 1 {
                            *center = state.threshold_input.clone();
                        }
                    }
                    ThresholdWizardStep::Ceremony => {
                        if row_idx == 0 {
                            *center = "Multifactor Setup — Step 3 of 3".to_string();
                        } else if row_idx == 1 {
                            *center = format!(
                                "{} of {} signatures",
                                state.threshold_k, state.selected_count
                            );
                        }
                    }
                }
            }
        }
        ModalState::AssignModerator => {
            if row_idx == 0 {
                *center = "Assign Moderator".to_string();
            } else if row_idx == 1 {
                *right = "only members can be designated as moderators".to_string();
            }
        }
        ModalState::SwitchAuthority => {
            if row_idx == 0 {
                *center = "Switch Authority".to_string();
            } else if row_idx > 0 {
                let authority_idx = row_idx - 1;
                if let Some(authority) = model.authorities.get(authority_idx) {
                    let prefix = if model.selected_authority_index() == Some(authority_idx) {
                        ">"
                    } else {
                        " "
                    };
                    let current = if authority.is_current {
                        " (current)"
                    } else {
                        ""
                    };
                    *right = format!("{prefix} {}{current}", authority.label);
                }
            }
        }
        ModalState::AccessOverride => {
            let access_level = match model.active_modal.as_ref() {
                Some(ActiveModal::AccessOverride(state)) => state.level.label(),
                _ => AccessOverrideLevel::Limited.label(),
            };
            if row_idx == 0 {
                *center = "Access Override".to_string();
            } else if row_idx == 1 {
                *right = format!("Access: {access_level}");
            } else if row_idx == 2 {
                *right = "Use Tab to toggle".to_string();
            }
        }
        ModalState::CapabilityConfig => {
            let active = match model.active_modal.as_ref() {
                Some(ActiveModal::CapabilityConfig(state)) => state.active_tier.label(),
                _ => CapabilityTier::Full.label(),
            };
            if row_idx == 0 {
                *center = "Home Capability Configuration".to_string();
            } else if row_idx == 1 {
                *right = format!("Editing: {active}");
            } else if row_idx == 2 {
                *right = "Use Tab to switch".to_string();
            }
        }
    }
}

fn format_panel_row(left: &str, center: &str, right: &str) -> String {
    format!("│ {left:<PANEL_WIDTH$} │ {center:<PANEL_WIDTH$} │ {right:<PANEL_WIDTH$} │")
}
