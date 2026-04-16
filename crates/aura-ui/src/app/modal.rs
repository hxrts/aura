use super::*;

pub(in crate::app) fn active_modal_title(model: &UiModel) -> Option<String> {
    let modal = model.modal_state()?;
    if !model.modal_hint.trim().is_empty() {
        return Some(model.modal_hint.trim().to_string());
    }
    Some(
        match modal {
            ModalState::Help => "Help",
            ModalState::CreateInvitation => "Invite Contacts",
            ModalState::AcceptContactInvitation => "Accept Contact Invitation",
            ModalState::AcceptChannelInvitation => "Accept Channel Invitation",
            ModalState::CreateHome => "Create New Home",
            ModalState::CreateChannel => "New Chat Group",
            ModalState::ChannelInfo => "Channel Info",
            ModalState::EditNickname => "Edit Nickname",
            ModalState::RemoveContact => "Remove Contact",
            ModalState::GuardianSetup => "Guardian Setup",
            ModalState::RequestRecovery => "Request Recovery",
            ModalState::AddDeviceStep1 => "Add Device",
            ModalState::ImportDeviceEnrollmentCode => "Import Device Enrollment Code",
            ModalState::SelectDeviceToRemove => "Select Device to Remove",
            ModalState::ConfirmRemoveDevice => "Confirm Device Removal",
            ModalState::MfaSetup => "Multifactor Setup",
            ModalState::AssignModerator => "Assign Moderator",
            ModalState::SwitchAuthority => "Switch Authority",
            ModalState::AccessOverride => "Access Override",
            ModalState::CapabilityConfig => "Home Capability Configuration",
            ModalState::EditChannelInfo => "Edit Channel",
        }
        .to_string(),
    )
}

pub(in crate::app) fn modal_view(
    model: &UiModel,
    chat_runtime: &ChatRuntimeView,
) -> Option<ModalView> {
    let modal = model.modal_state()?;
    let title = active_modal_title(model).unwrap_or_else(|| "Modal".to_string());
    let mut details = Vec::new();
    let mut keybind_rows = Vec::new();
    let mut inputs = Vec::new();
    let mut values = Vec::new();

    match modal {
        ModalState::Help => {
            let (help_details, help_keybind_rows) = help_modal_content(model.screen);
            details = help_details;
            keybind_rows = help_keybind_rows;
        }
        ModalState::CreateInvitation => {
            details.push("Create a shareable contact invite code.".to_string());
            details.push("Share it out of band. The recipient authority is learned when they import and accept it.".to_string());
            let modal_state = model.create_invitation_modal().cloned().unwrap_or_default();
            inputs.push(ModalInputView {
                label: "Message (optional)".to_string(),
                field_id: FieldId::InvitationMessage,
                value: modal_state.message,
            });
            inputs.push(ModalInputView {
                label: "TTL (Hours)".to_string(),
                field_id: FieldId::InvitationTtl,
                value: modal_state.ttl_hours.to_string(),
            });
            if let Some(code) = model.last_invite_code.as_ref() {
                details.push("The invite code was copied to your clipboard.".to_string());
                details
                    .push("Share it out of band, or copy it again from this dialog.".to_string());
                keybind_rows.push((
                    ControlId::ModalCopyButton
                        .activation_key()
                        .unwrap_or("c")
                        .to_string(),
                    "Copy the invite code again".to_string(),
                ));
                values.push(ModalValueView {
                    label: "Invite Code".to_string(),
                    value: code.clone(),
                });
            }
        }
        ModalState::AcceptContactInvitation => {
            details.push("Paste a contact invite code, then press Enter.".to_string());
            inputs.push(ModalInputView {
                label: "Invite Code".to_string(),
                field_id: FieldId::InvitationCode,
                value: model.modal_text_value().unwrap_or_default(),
            });
        }
        ModalState::AcceptChannelInvitation => {
            details.push("Paste a channel invite code, then press Enter.".to_string());
            inputs.push(ModalInputView {
                label: "Invite Code".to_string(),
                field_id: FieldId::InvitationCode,
                value: model.modal_text_value().unwrap_or_default(),
            });
        }
        ModalState::CreateHome => {
            details.push("Enter a new home name and press Enter.".to_string());
            inputs.push(ModalInputView {
                label: "Home Name".to_string(),
                field_id: FieldId::HomeName,
                value: model.modal_text_value().unwrap_or_default(),
            });
        }
        ModalState::CreateChannel => {
            if let Some(state) = model.create_channel_modal() {
                match state.step {
                    CreateChannelWizardStep::Details => {
                        let active = match state.active_field {
                            CreateChannelDetailsField::Name => "Group Name",
                            CreateChannelDetailsField::Topic => "Topic (optional)",
                        };
                        details.push("Step 1 of 3: Configure group details.".to_string());
                        details.push(format!("Active field: {active} (Tab to switch)"));
                        inputs.push(ModalInputView {
                            label: "Group Name".to_string(),
                            field_id: FieldId::CreateChannelName,
                            value: state.name.clone(),
                        });
                        inputs.push(ModalInputView {
                            label: "Topic (optional)".to_string(),
                            field_id: FieldId::CreateChannelTopic,
                            value: state.topic.clone(),
                        });
                    }
                    CreateChannelWizardStep::Members => {
                        details.push("Step 2 of 3: Select members to invite.".to_string());
                        if model.contacts.is_empty() {
                            details.push("No contacts available.".to_string());
                        }
                    }
                    CreateChannelWizardStep::Threshold => {
                        let participant_total = state.selected_members.len().saturating_add(1);
                        details.push("Step 3 of 3: Set threshold.".to_string());
                        details.push(format!("Participants (including you): {participant_total}"));
                        details.push("Use ↑/↓ to adjust, Enter to create.".to_string());
                        inputs.push(ModalInputView {
                            label: "Threshold".to_string(),
                            field_id: FieldId::ThresholdInput,
                            value: model.modal_text_value().unwrap_or_default(),
                        });
                    }
                }
            }
        }
        ModalState::ChannelInfo => {
            let active_channel = if chat_runtime.active_channel.is_empty() {
                model
                    .selected_channel_name()
                    .unwrap_or(NOTE_TO_SELF_CHANNEL_NAME)
                    .to_string()
            } else {
                chat_runtime.active_channel.clone()
            };
            if let Some(channel) = chat_runtime
                .channels
                .iter()
                .find(|channel| channel.name.eq_ignore_ascii_case(&active_channel))
            {
                details.push(format!("Channel: #{}", channel.name));
                details.push(format!(
                    "Type: {}",
                    if channel.is_dm {
                        "Direct Message"
                    } else {
                        "Group Channel"
                    }
                ));
                details.push(format!(
                    "Topic: {}",
                    if channel.topic.trim().is_empty() {
                        "No topic set".to_string()
                    } else {
                        channel.topic.clone()
                    }
                ));
                details.push(format!("Unread messages: {}", channel.unread_count));
                details.push(format!("Visible messages: {}", chat_runtime.messages.len()));
                if channel.member_count > 0 {
                    details.push(format!("Known members: {}", channel.member_count));
                }
                if let Some(last_message) = &channel.last_message {
                    details.push(format!("Latest message: {last_message}"));
                }
            } else {
                details.push("No channel selected.".to_string());
            }
        }
        ModalState::EditNickname => {
            details.push("Update your nickname suggestion and press Enter.".to_string());
            inputs.push(ModalInputView {
                label: "Nickname".to_string(),
                field_id: FieldId::Nickname,
                value: model.modal_text_value().unwrap_or_default(),
            });
        }
        ModalState::RemoveContact => {
            details.push("Remove the selected contact from this authority.".to_string());
            details.push("Press Enter to confirm.".to_string());
        }
        ModalState::GuardianSetup => {
            if let Some(state) = model.guardian_setup_modal() {
                match state.step {
                    ThresholdWizardStep::Selection => {
                        details.push("Step 1 of 3: Select guardians.".to_string());
                        if model.contacts.is_empty() {
                            details.push("No contacts available.".to_string());
                        }
                        // Selection list is rendered via selectable_items (checkbox component)
                    }
                    ThresholdWizardStep::Threshold => {
                        details.push("Step 2 of 3: Choose threshold.".to_string());
                        details.push(format!("Selected guardians: {}", state.selected_count));
                        details.push("Enter k (approvals required).".to_string());
                        inputs.push(ModalInputView {
                            label: "Threshold (k)".to_string(),
                            field_id: FieldId::ThresholdInput,
                            value: model.modal_text_value().unwrap_or_default(),
                        });
                    }
                    ThresholdWizardStep::Ceremony => {
                        details.push("Step 3 of 3: Ready to start ceremony.".to_string());
                        details.push(format!(
                            "Will start guardian setup with {} of {} approvals.",
                            state.threshold_k, state.selected_count
                        ));
                        details.push("Press Enter to start.".to_string());
                    }
                }
            }
        }
        ModalState::RequestRecovery => {
            details.push("Request guardian-assisted recovery for this authority.".to_string());
            details.push("Press Enter to notify your configured guardians.".to_string());
        }
        ModalState::AddDeviceStep1 => {
            if let Some(state) = model.add_device_modal() {
                match state.step {
                    AddDeviceWizardStep::Name => {
                        details
                            .push("Step 1 of 3: Name the device you want to invite.".to_string());
                        details.push("This is the new device, not the current one.".to_string());
                        details.push(
                            "Press Enter to generate an out-of-band enrollment code.".to_string(),
                        );
                        if !state.name_input.trim().is_empty() {
                            details.push(format!("Draft name: {}", state.name_input));
                        }
                        inputs.push(ModalInputView {
                            label: "Device Name".to_string(),
                            field_id: FieldId::DeviceName,
                            value: model.modal_text_value().unwrap_or_default(),
                        });
                    }
                    AddDeviceWizardStep::ShareCode => {
                        details.push(
                            "Step 2 of 3: Share this code out-of-band with that device."
                                .to_string(),
                        );
                        details.push(format!("Enrollment Code: {}", state.enrollment_code));
                        details.push("Press c to copy, then press Enter when shared.".to_string());
                        if let Some(ceremony_id) = state.ceremony_id.as_ref() {
                            details.push(format!("Ceremony: {ceremony_id}"));
                        }
                    }
                    AddDeviceWizardStep::Confirm => {
                        details.push(
                            "Step 3 of 3: Waiting for the new device to import the code."
                                .to_string(),
                        );
                        details.push(format!(
                            "Device '{}': {} of {} confirmations ({})",
                            state.device_name,
                            state.accepted_count,
                            state.total_count.max(1),
                            state.threshold.max(1)
                        ));
                        if let Some(error) = &state.error_message {
                            details.push(format!("Error: {error}"));
                        } else if state.has_failed {
                            details.push("The enrollment ceremony failed.".to_string());
                        } else if state.is_complete {
                            details.push("Enrollment ceremony complete. The new device is now part of this authority.".to_string());
                        } else {
                            details.push("Leave this dialog open to monitor progress, or press Esc to cancel the ceremony.".to_string());
                        }
                    }
                }
            }
        }
        ModalState::ImportDeviceEnrollmentCode => {
            details.push("Import a device enrollment code and press Enter.".to_string());
            inputs.push(ModalInputView {
                label: "Enrollment Code".to_string(),
                field_id: FieldId::DeviceImportCode,
                value: model.modal_text_value().unwrap_or_default(),
            });
        }
        ModalState::SelectDeviceToRemove => {
            details.push("Select the device to remove.".to_string());
            details.push(format!(
                "Selected: {}",
                model
                    .secondary_device_name()
                    .or_else(|| model
                        .selected_device_modal()
                        .map(|state| state.candidate_name.as_str()))
                    .unwrap_or("Secondary device")
            ));
            details.push("Press Enter to continue.".to_string());
        }
        ModalState::ConfirmRemoveDevice => {
            details.push(format!(
                "Remove \"{}\" from this authority?",
                model
                    .secondary_device_name()
                    .or_else(|| model
                        .selected_device_modal()
                        .map(|state| state.candidate_name.as_str()))
                    .unwrap_or("Secondary device")
            ));
            details.push("Press Enter to confirm removal.".to_string());
        }
        ModalState::MfaSetup => {
            if let Some(state) = model.mfa_setup_modal() {
                match state.step {
                    ThresholdWizardStep::Selection => {
                        details.push("Step 1 of 3: Select devices for MFA signing.".to_string());
                        // Selection list is rendered via selectable_items (checkbox component)
                    }
                    ThresholdWizardStep::Threshold => {
                        details.push("Step 2 of 3: Configure signing threshold.".to_string());
                        details.push(format!("Selected devices: {}", state.selected_count));
                        details.push("Enter required signatures (k).".to_string());
                        inputs.push(ModalInputView {
                            label: "Threshold (k)".to_string(),
                            field_id: FieldId::ThresholdInput,
                            value: model.modal_text_value().unwrap_or_default(),
                        });
                    }
                    ThresholdWizardStep::Ceremony => {
                        details.push("Step 3 of 3: Ready to start MFA ceremony.".to_string());
                        details.push(format!(
                            "Will start MFA with {} of {} signatures.",
                            state.threshold_k, state.selected_count
                        ));
                        details.push("Press Enter to start.".to_string());
                    }
                }
            }
        }
        ModalState::AssignModerator => {
            details.push("Apply moderator role changes in the currently entered home.".to_string());
            details.push("Select a member in the Home panel first. Only members can be designated as moderators.".to_string());
        }
        ModalState::SwitchAuthority => {
            details.push("Switch to another authority stored on this device.".to_string());
            details.push("Use ↑/↓ to choose, then press Enter to reload into it.".to_string());
            if model.authorities.is_empty() {
                details.push("No authorities available.".to_string());
            } else {
                for (idx, authority) in model.authorities.iter().enumerate() {
                    let focused = if model.selected_authority_index() == Some(idx) {
                        ">"
                    } else {
                        " "
                    };
                    let current = if authority.is_current {
                        " (current)"
                    } else {
                        ""
                    };
                    details.push(format!("{focused} {}{current}", authority.label));
                }
            }
        }
        ModalState::AccessOverride => {
            let selected_contact = model
                .selected_contact_name()
                .unwrap_or("No contact selected");
            let level = match model.active_modal.as_ref() {
                Some(ActiveModal::AccessOverride(state)) => state.level.label(),
                _ => AccessOverrideLevel::Limited.label(),
            };
            details.push("Apply a per-home access override for the selected contact.".to_string());
            details.push(format!("Selected contact: {selected_contact}"));
            details.push(format!("Access level: {level}"));
            details.push("Use ↑/↓ to select a contact. Tab toggles Limited/Partial.".to_string());
            details.push("Press Enter to apply the override to the current home.".to_string());
        }
        ModalState::CapabilityConfig => {
            let (active, full_caps, partial_caps, limited_caps) = model
                .capability_config_modal()
                .map(|state| {
                    (
                        state.active_tier.label(),
                        state.full_caps.as_str(),
                        state.partial_caps.as_str(),
                        state.limited_caps.as_str(),
                    )
                })
                .unwrap_or((
                    CapabilityTier::Full.label(),
                    DEFAULT_CAPABILITY_FULL,
                    DEFAULT_CAPABILITY_PARTIAL,
                    DEFAULT_CAPABILITY_LIMITED,
                ));
            details.push("Configure per-home capabilities for each access level.".to_string());
            details.push("Tab switches fields. Enter saves to the current home.".to_string());
            details.push(format!("Editing: {active}"));
            details.push(format!("Full: {full_caps}"));
            details.push(format!("Partial: {partial_caps}"));
            details.push(format!("Limited: {limited_caps}"));
            let field_id = match model
                .capability_config_modal()
                .map(|state| state.active_tier)
            {
                Some(CapabilityTier::Partial) => FieldId::CapabilityPartial,
                Some(CapabilityTier::Limited) => FieldId::CapabilityLimited,
                _ => FieldId::CapabilityFull,
            };
            inputs.push(ModalInputView {
                label: format!("{active} Capabilities"),
                field_id,
                value: model.modal_text_value().unwrap_or_default(),
            });
        }
        ModalState::EditChannelInfo => {
            details.push("Edit the channel name and topic.".to_string());
            let (name_val, topic_val) = model
                .edit_channel_info()
                .map(|state| (state.name.clone(), state.topic.clone()))
                .unwrap_or_default();
            inputs.push(ModalInputView {
                label: "Channel Name".to_string(),
                field_id: FieldId::CreateChannelName,
                value: name_val,
            });
            inputs.push(ModalInputView {
                label: "Channel Topic".to_string(),
                field_id: FieldId::CreateChannelTopic,
                value: topic_val,
            });
        }
    }

    let enter_label = match modal {
        ModalState::Help | ModalState::ChannelInfo => "Close".to_string(),
        ModalState::CreateInvitation => {
            if model.last_invite_code.is_some() {
                "Close".to_string()
            } else {
                "Create".to_string()
            }
        }
        ModalState::CreateChannel => match model.create_channel_modal().map(|state| state.step) {
            Some(CreateChannelWizardStep::Threshold) => "Create".to_string(),
            _ => "Next".to_string(),
        },
        ModalState::AddDeviceStep1 => match model.add_device_modal().map(|state| state.step) {
            Some(AddDeviceWizardStep::ShareCode) => "Next".to_string(),
            Some(AddDeviceWizardStep::Confirm) => {
                if model
                    .add_device_modal()
                    .map(|state| state.is_complete || state.has_failed)
                    .unwrap_or(false)
                {
                    "Close".to_string()
                } else {
                    "Refresh".to_string()
                }
            }
            _ => "Generate Code".to_string(),
        },
        ModalState::GuardianSetup => match model.guardian_setup_modal().map(|state| state.step) {
            Some(ThresholdWizardStep::Ceremony) => "Start".to_string(),
            _ => "Next".to_string(),
        },
        ModalState::MfaSetup => match model.mfa_setup_modal().map(|state| state.step) {
            Some(ThresholdWizardStep::Ceremony) => "Start".to_string(),
            _ => "Next".to_string(),
        },
        ModalState::SwitchAuthority => "Switch".to_string(),
        ModalState::AccessOverride => "Apply".to_string(),
        ModalState::CapabilityConfig | ModalState::EditChannelInfo => "Save".to_string(),
        _ => "Confirm".to_string(),
    };

    let footer_shortcuts = if matches!(modal, ModalState::AcceptContactInvitation) {
        model
            .demo_contact_shortcuts()
            .map(|shortcuts| {
                vec![
                    ("Alice".to_string(), shortcuts.alice_invite_code.clone()),
                    ("Carol".to_string(), shortcuts.carol_invite_code.clone()),
                ]
            })
            .unwrap_or_default()
    } else {
        vec![]
    };

    let footer_actions =
        if matches!(modal, ModalState::CreateInvitation) && model.last_invite_code.is_some() {
            vec![ModalFooterActionView {
                control_id: Some(ControlId::ModalCopyButton),
                label: "Copy Code".to_string(),
            }]
        } else {
            vec![]
        };

    let selectable_items = match modal {
        ModalState::CreateChannel => model
            .create_channel_modal()
            .filter(|state| matches!(state.step, CreateChannelWizardStep::Members))
            .map(|state| {
                model
                    .contacts
                    .iter()
                    .enumerate()
                    .map(|(idx, contact)| SelectableItem {
                        index: idx,
                        label: contact.name.clone(),
                        selected: state.selected_members.contains(&idx),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        ModalState::GuardianSetup => model
            .guardian_setup_modal()
            .filter(|state| matches!(state.step, ThresholdWizardStep::Selection))
            .map(|state| {
                model
                    .contacts
                    .iter()
                    .enumerate()
                    .map(|(idx, contact)| SelectableItem {
                        index: idx,
                        label: contact.name.clone(),
                        selected: state.selected_indices.contains(&idx),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        ModalState::MfaSetup => model
            .mfa_setup_modal()
            .filter(|state| matches!(state.step, ThresholdWizardStep::Selection))
            .map(|state| {
                let devices: Vec<String> = if model.has_secondary_device {
                    vec![
                        "This Device".to_string(),
                        model
                            .secondary_device_name()
                            .unwrap_or("Secondary Device")
                            .to_string(),
                    ]
                } else {
                    vec!["This Device".to_string()]
                };
                devices
                    .into_iter()
                    .enumerate()
                    .map(|(idx, name)| SelectableItem {
                        index: idx,
                        label: name,
                        selected: state.selected_indices.contains(&idx),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        _ => vec![],
    };

    Some(ModalView {
        modal_id: modal.contract_id(),
        title,
        details,
        keybind_rows,
        inputs,
        values,
        selectable_items,
        enter_label,
        footer_shortcuts,
        footer_actions,
    })
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use crate::model::CreateInvitationModalState;

    #[test]
    fn create_invitation_modal_shows_generated_code_and_copy_action() {
        let mut model = UiModel::new("authority-local".to_string());
        model.last_invite_code = Some("INVITE-123".to_string());
        model.active_modal = Some(ActiveModal::CreateInvitation(
            CreateInvitationModalState::default(),
        ));

        let Some(modal) = modal_view(&model, &ChatRuntimeView::default()) else {
            panic!("create invitation modal should render");
        };

        assert_eq!(modal.enter_label, "Close");
        assert_eq!(modal.values.len(), 1);
        assert_eq!(modal.values[0].label, "Invite Code");
        assert_eq!(modal.values[0].value, "INVITE-123");
        assert_eq!(modal.footer_actions.len(), 1);
        assert_eq!(
            modal.footer_actions[0].control_id,
            Some(ControlId::ModalCopyButton)
        );
    }

    #[test]
    fn create_invitation_modal_without_generated_code_keeps_create_state() {
        let mut model = UiModel::new("authority-local".to_string());
        model.active_modal = Some(ActiveModal::CreateInvitation(
            CreateInvitationModalState::default(),
        ));

        let Some(modal) = modal_view(&model, &ChatRuntimeView::default()) else {
            panic!("create invitation modal should render");
        };

        assert_eq!(modal.enter_label, "Create");
        assert!(modal.values.is_empty());
        assert!(modal.footer_actions.is_empty());
    }
}

fn help_modal_content(screen: ScreenId) -> (Vec<String>, Vec<(String, String)>) {
    let details = match screen {
        ScreenId::Onboarding => vec![
            "Onboarding reference".to_string(),
            "Create or import a local account before entering the main application.".to_string(),
        ],
        ScreenId::Neighborhood => vec![
            "Neighborhood reference".to_string(),
            "Browse homes and neighborhood detail views.".to_string(),
        ],
        ScreenId::Chat => vec![
            "Chat reference".to_string(),
            "Navigate channels, compose messages, and manage channel metadata.".to_string(),
        ],
        ScreenId::Contacts => vec![
            "Contacts reference".to_string(),
            "Manage invitations, nicknames, guardians, and direct-message handoff.".to_string(),
        ],
        ScreenId::Notifications => vec![
            "Notifications reference".to_string(),
            "Review pending notices and move through the notification feed.".to_string(),
        ],
        ScreenId::Settings => vec![
            "Settings reference".to_string(),
            "Adjust profile, recovery, devices, authority, and appearance.".to_string(),
        ],
    };

    let keybind_rows = match screen {
        ScreenId::Onboarding => vec![
            (
                "type".to_string(),
                "Enter account name or import code".to_string(),
            ),
            (
                "enter".to_string(),
                "Submit the active onboarding form".to_string(),
            ),
        ],
        ScreenId::Neighborhood => vec![
            ("1-5".to_string(), "Switch screens".to_string()),
            ("tab / shift+tab".to_string(), "Cycle screens".to_string()),
            ("enter".to_string(), "Toggle map/detail view".to_string()),
            ("a".to_string(), "Accept home invitation".to_string()),
            ("n".to_string(), "Create home".to_string()),
            ("esc".to_string(), "Close modal / back out".to_string()),
        ],
        ScreenId::Chat => vec![
            ("1-5".to_string(), "Switch screens".to_string()),
            ("tab / shift+tab".to_string(), "Cycle screens".to_string()),
            (
                "up / down".to_string(),
                "Move channel selection".to_string(),
            ),
            ("i".to_string(), "Enter message input".to_string()),
            ("n".to_string(), "Create channel".to_string()),
            ("e".to_string(), "Edit channel info".to_string()),
            ("o".to_string(), "Open channel info".to_string()),
            ("esc".to_string(), "Close modal / exit input".to_string()),
        ],
        ScreenId::Contacts => vec![
            ("1-5".to_string(), "Switch screens".to_string()),
            ("tab / shift+tab".to_string(), "Cycle screens".to_string()),
            (
                "up / down".to_string(),
                "Move contact selection".to_string(),
            ),
            (
                "left / right".to_string(),
                "Toggle contact detail pane".to_string(),
            ),
            ("n".to_string(), "Create invitation".to_string()),
            ("a".to_string(), "Accept invitation".to_string()),
            (
                "i".to_string(),
                "Invite selected contact to current channel".to_string(),
            ),
            ("e".to_string(), "Edit nickname".to_string()),
            ("g".to_string(), "Configure guardians".to_string()),
            ("c".to_string(), "Open DM for selected contact".to_string()),
            ("r".to_string(), "Remove contact".to_string()),
        ],
        ScreenId::Notifications => vec![
            ("1-5".to_string(), "Switch screens".to_string()),
            ("tab / shift+tab".to_string(), "Cycle screens".to_string()),
            (
                "up / down".to_string(),
                "Move notification selection".to_string(),
            ),
            ("a".to_string(), "Accept channel invitation".to_string()),
            (
                "click actions".to_string(),
                "Accept, decline, export, or approve from the detail pane".to_string(),
            ),
            ("esc".to_string(), "Close modal".to_string()),
        ],
        ScreenId::Settings => vec![
            ("1-5".to_string(), "Switch screens".to_string()),
            ("tab / shift+tab".to_string(), "Cycle screens".to_string()),
            (
                "up / down".to_string(),
                "Move settings selection".to_string(),
            ),
            (
                "enter".to_string(),
                "Open selected settings action".to_string(),
            ),
            ("e".to_string(), "Edit profile nickname".to_string()),
            ("t".to_string(), "Guardian threshold setup".to_string()),
            ("s".to_string(), "Request recovery".to_string()),
            ("a".to_string(), "Add device".to_string()),
            ("i".to_string(), "Import enrollment code".to_string()),
        ],
    };

    (details, keybind_rows)
}

pub(in crate::app) fn modal_accepts_text(model: &UiModel, modal: ModalState) -> bool {
    let _ = modal;
    model.modal_accepts_text()
}
