use super::*;

#[derive(Debug, Clone)]
pub struct RenderedHarnessSnapshot {
    pub screen: String,
    pub authoritative_screen: String,
    pub normalized_screen: String,
    pub raw_screen: String,
}

impl UiModel {
    #[must_use]
    pub fn semantic_snapshot(&self) -> UiSnapshot {
        let mut lists = Vec::new();
        let mut selections = Vec::new();

        let navigation_items = [
            ScreenId::Neighborhood,
            ScreenId::Chat,
            ScreenId::Contacts,
            ScreenId::Notifications,
            ScreenId::Settings,
        ]
        .into_iter()
        .map(|screen| ListItemSnapshot {
            id: screen.help_label().to_ascii_lowercase(),
            selected: self.screen == screen,
            confirmation: ConfirmationState::Confirmed,
            is_current: false,
        })
        .collect::<Vec<_>>();
        lists.push(ListSnapshot {
            id: ListId::Navigation,
            items: navigation_items,
        });
        selections.push(SelectionSnapshot {
            list: ListId::Navigation,
            item_id: self.screen.help_label().to_ascii_lowercase(),
        });

        let channel_items = self
            .channels
            .iter()
            .map(|channel| ListItemSnapshot {
                id: channel.id.clone(),
                selected: channel.selected,
                confirmation: ConfirmationState::Confirmed,
                is_current: false,
            })
            .collect::<Vec<_>>();
        if !channel_items.is_empty() {
            lists.push(ListSnapshot {
                id: ListId::Channels,
                items: channel_items,
            });
        }
        if let Some(channel) = self.selected_channel_id() {
            selections.push(SelectionSnapshot {
                list: ListId::Channels,
                item_id: channel.to_string(),
            });
        }

        let contact_items = self
            .contacts
            .iter()
            .map(|contact| ListItemSnapshot {
                id: contact.authority_id.to_string(),
                selected: contact.selected,
                confirmation: contact.confirmation,
                is_current: false,
            })
            .collect::<Vec<_>>();
        if !contact_items.is_empty() {
            lists.push(ListSnapshot {
                id: ListId::Contacts,
                items: contact_items,
            });
        }
        if let Some(contact_id) = self.selected_contact_authority_id() {
            selections.push(SelectionSnapshot {
                list: ListId::Contacts,
                item_id: contact_id.to_string(),
            });
        }

        let authority_items = self
            .authorities
            .iter()
            .map(|authority| ListItemSnapshot {
                id: authority.id.to_string(),
                selected: authority.selected,
                confirmation: ConfirmationState::Confirmed,
                is_current: false,
            })
            .collect::<Vec<_>>();
        if !authority_items.is_empty() {
            lists.push(ListSnapshot {
                id: ListId::Authorities,
                items: authority_items,
            });
        }
        if let Some(authority_id) = self.selected_authority_id {
            selections.push(SelectionSnapshot {
                list: ListId::Authorities,
                item_id: authority_id.to_string(),
            });
        }

        let notification_items = self
            .notification_ids
            .iter()
            .map(|notification| ListItemSnapshot {
                id: notification.0.clone(),
                selected: self.selected_notification_id.as_ref() == Some(notification),
                confirmation: ConfirmationState::Confirmed,
                is_current: false,
            })
            .collect::<Vec<_>>();
        if !notification_items.is_empty() {
            lists.push(ListSnapshot {
                id: ListId::Notifications,
                items: notification_items,
            });
        }
        if let Some(notification_id) = &self.selected_notification_id {
            selections.push(SelectionSnapshot {
                list: ListId::Notifications,
                item_id: notification_id.0.clone(),
            });
        }

        let settings_items = SettingsSection::ALL
            .into_iter()
            .map(|section| ListItemSnapshot {
                id: section.dom_id().to_string(),
                selected: self.settings_section == section,
                confirmation: ConfirmationState::Confirmed,
                is_current: false,
            })
            .collect::<Vec<_>>();
        lists.push(ListSnapshot {
            id: ListId::SettingsSections,
            items: settings_items,
        });
        selections.push(SelectionSnapshot {
            list: ListId::SettingsSections,
            item_id: self.settings_section.dom_id().to_string(),
        });

        let mut toasts = Vec::new();
        if let Some(toast) = &self.toast {
            let kind = match toast.icon {
                '✓' => ToastKind::Success,
                'ℹ' => ToastKind::Info,
                _ => ToastKind::Error,
            };
            toasts.push(ToastSnapshot {
                id: ToastId(format!("toast-{}", self.toast_key)),
                kind,
                message: toast.message.clone(),
            });
        }

        let messages = self
            .messages
            .iter()
            .enumerate()
            .map(|(idx, content)| MessageSnapshot {
                id: format!("local-message-{idx}"),
                content: content.clone(),
            })
            .collect::<Vec<_>>();
        let open_modal = self.modal_state().map(ModalState::contract_id);
        let focused_control = if self.modal_field_id().is_some() {
            Some(ControlId::ModalInput)
        } else if let Some(open_modal) = open_modal {
            Some(ControlId::Modal(open_modal))
        } else if self.account_ready {
            Some(ControlId::Screen(Self::canonical_ready_screen(self.screen)))
        } else {
            Some(ControlId::OnboardingRoot)
        };
        let screen = if self.account_ready {
            Self::canonical_ready_screen(self.screen)
        } else {
            ScreenId::Onboarding
        };

        UiSnapshot {
            screen,
            focused_control,
            open_modal,
            readiness: readiness_owner::account_gate_readiness(self.account_ready),
            revision: self.semantic_revision,
            quiescence: QuiescenceSnapshot::derive(
                readiness_owner::account_gate_readiness(self.account_ready),
                open_modal,
                &self.operations,
            ),
            selections,
            lists,
            messages,
            operations: self.operations.clone(),
            toasts,
            runtime_events: self.runtime_events.clone(),
        }
    }
}

impl UiController {
    pub fn snapshot(&self) -> RenderedHarnessSnapshot {
        let screen = self
            .model
            .try_read()
            .ok()
            .map(|model| render_canonical_snapshot(&model))
            .unwrap_or_else(|| {
                let snapshot = UiSnapshot::loading(ScreenId::Neighborhood);
                format!(
                    "[harness-snapshot-busy]\nscreen={:?}\nreadiness={:?}\nopen_modal={:?}\nfocused_control={:?}",
                    snapshot.screen, snapshot.readiness, snapshot.open_modal, snapshot.focused_control
                )
            });
        let normalized_screen = screen
            .replace('\r', "")
            .lines()
            .map(|line| line.trim_end())
            .collect::<Vec<_>>()
            .join("\n");

        RenderedHarnessSnapshot {
            screen: normalized_screen.clone(),
            authoritative_screen: normalized_screen.clone(),
            normalized_screen,
            raw_screen: screen,
        }
    }

    pub fn ui_snapshot(&self) -> UiSnapshot {
        self.model
            .read()
            .ok()
            .map(|model| model.semantic_snapshot())
            .unwrap_or_else(|| UiSnapshot::loading(ScreenId::Neighborhood))
    }

    pub fn semantic_model_snapshot(&self) -> UiSnapshot {
        let snapshot = self
            .model
            .read()
            .ok()
            .map(|model| model.semantic_snapshot())
            .unwrap_or_else(|| UiSnapshot::loading(ScreenId::Neighborhood));
        snapshot
            .validate_invariants()
            .unwrap_or_else(|error| panic!("invalid semantic model snapshot export: {error}"));
        snapshot
    }

    pub fn publish_ui_snapshot(&self, snapshot: UiSnapshot) {
        snapshot
            .validate_invariants()
            .unwrap_or_else(|error| panic!("invalid published UI snapshot: {error}"));
        if let Ok(mut last_snapshot) = self.last_published_ui_snapshot.lock() {
            if last_snapshot.as_ref() == Some(&snapshot) {
                return;
            }
            *last_snapshot = Some(snapshot.clone());
        }
        if let Ok(slot) = self.ui_snapshot_sink.lock() {
            let sink = slot.as_ref().cloned();
            drop(slot);
            if let Some(sink) = sink {
                sink(snapshot);
            }
        }
    }

    pub fn reset_published_ui_snapshot(&self) {
        if let Ok(mut last_snapshot) = self.last_published_ui_snapshot.lock() {
            *last_snapshot = None;
        }
    }
}
