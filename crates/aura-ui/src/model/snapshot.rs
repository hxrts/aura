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

        // Onboarding renders outside the app shell, which owns Navigation.
        // Export its list and selection only when that shell can mount.
        if self.account_ready() {
            let navigation_items = [
                ScreenId::Neighborhood,
                ScreenId::Chat,
                ScreenId::Contacts,
                ScreenId::Notifications,
                ScreenId::Settings,
            ]
            .into_iter()
            .map(|item| ListItemSnapshot {
                id: item.help_label().to_ascii_lowercase(),
                selected: self.screen == item,
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
        }

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

        let mut seen_notifications = std::collections::HashSet::new();
        let notification_items = self
            .notification_ids
            .iter()
            // A repeated id would export several selected items for one selection.
            .filter(|notification| seen_notifications.insert(notification.0.clone()))
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
                delivery_status: None,
            })
            .collect::<Vec<_>>();
        let open_modal = self.modal_state().map(ModalState::contract_id);
        let focused_control = if let Some(field) = self.modal_field_id() {
            Some(ControlId::Field(field))
        } else if let Some(open_modal) = open_modal {
            Some(ControlId::Modal(open_modal))
        } else if self.account_ready() {
            Some(ControlId::Screen(self.screen))
        } else {
            Some(ControlId::OnboardingRoot)
        };
        let screen = self.screen;

        UiSnapshot {
            screen,
            focused_control,
            open_modal,
            readiness: readiness_owner::account_gate_readiness(self.account_ready()),
            revision: self.semantic_revision,
            projection_source_revisions: aura_app::core::ProjectionSourceRevisions::default(),
            home_modes: Vec::new(),
            rendered_controls: Vec::new(),
            rendered_control_frame: aura_app::ui::contract::RenderedControlFrame::Unavailable,
            quiescence: QuiescenceSnapshot::derive(
                readiness_owner::account_gate_readiness(self.account_ready()),
                open_modal,
                &self.operations,
            ),
            selections,
            lists,
            messages,
            operations: self.operations.clone(),
            toasts,
            runtime_events: self.runtime_events.clone(),
            subscription_health: self.subscription_health.clone(),
            supervised_task_failures: Vec::new(),
            message_drops: Vec::new(),
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
        self.semantic_model_snapshot()
    }

    fn observe_app_snapshot(&self, snapshot: &mut UiSnapshot) {
        if let Some(core) = self.app_core.try_read() {
            let app_snapshot = core.snapshot();
            snapshot.projection_source_revisions = app_snapshot.projection_source_revisions;
            snapshot.home_modes = aura_app::ui::contract::observed_home_modes(&app_snapshot.homes);
        } else {
            *snapshot = UiSnapshot::loading(snapshot.screen);
        }
    }

    pub fn semantic_model_snapshot(&self) -> UiSnapshot {
        let mut snapshot = self
            .model
            .read()
            .ok()
            .map(|model| model.semantic_snapshot())
            .unwrap_or_else(|| UiSnapshot::loading(ScreenId::Neighborhood));
        self.observe_app_snapshot(&mut snapshot);
        snapshot
            .validate_invariants()
            .unwrap_or_else(|error| panic!("invalid semantic model snapshot export: {error}"));
        snapshot
    }

    pub fn publish_ui_snapshot(&self, snapshot: UiSnapshot) {
        self.publish_ui_snapshot_with_capture(snapshot, None);
    }

    fn publish_ui_snapshot_with_capture(
        &self,
        mut snapshot: UiSnapshot,
        receipt: Option<&RendererCaptureReceipt>,
    ) {
        let rendered_source = aura_app::ui::contract::ControlRenderSource::from_snapshot(&snapshot);
        let observations_match = snapshot.rendered_controls.iter().all(|observation| {
            observation.source == rendered_source
                && receipt.is_none_or(|receipt| observation.capture_revision == receipt.revision)
        });
        self.observe_app_snapshot(&mut snapshot);
        let source = aura_app::ui::contract::ControlRenderSource::from_snapshot(&snapshot);
        snapshot
            .rendered_controls
            .retain(|observation| observation.source == source);
        if let Some(receipt) = receipt.filter(|_| rendered_source == source && observations_match) {
            snapshot.rendered_control_frame =
                aura_app::ui::contract::RenderedControlFrame::Committed {
                    capture_revision: receipt.revision,
                    layout_revision: receipt.revision,
                    source: source.clone(),
                };
        } else {
            snapshot.rendered_control_frame =
                aura_app::ui::contract::RenderedControlFrame::Unavailable;
            if receipt.is_some() {
                snapshot.rendered_controls.clear();
            }
        }
        snapshot
            .validate_invariants()
            .unwrap_or_else(|error| panic!("invalid published UI snapshot: {error}"));
        let should_drain = if let Ok(mut publication) = self.last_published_ui_snapshot.lock() {
            if let Some(receipt) = receipt {
                if !publication
                    .active_renderer
                    .as_ref()
                    .is_some_and(|active| Arc::ptr_eq(&active.identity, &receipt.identity))
                {
                    return;
                }
                snapshot
                    .rendered_controls
                    .retain(|observation| observation.capture_revision == receipt.revision);
            }
            if let aura_app::ui::contract::RenderedControlFrame::Committed {
                layout_revision, ..
            } = &mut snapshot.rendered_control_frame
            {
                if receipt.is_some() {
                    let Some(next) = publication.next_layout_revision.checked_add(1) else {
                        return;
                    };
                    publication.next_layout_revision = next;
                    *layout_revision = next;
                }
            }
            let last_snapshot = &mut publication.snapshot;
            if receipt.is_none() && snapshot.rendered_controls.is_empty() {
                if let Some(previous) = last_snapshot.as_ref() {
                    if matches!(&previous.rendered_control_frame, aura_app::ui::contract::RenderedControlFrame::Committed { source: previous_source, .. } if previous_source == &source)
                    {
                        snapshot.rendered_control_frame = previous.rendered_control_frame.clone();
                    }
                    snapshot.rendered_controls = previous
                        .rendered_controls
                        .iter()
                        .filter(|observation| observation.source == source)
                        .cloned()
                        .collect();
                }
            }
            if last_snapshot.as_ref() == Some(&snapshot) {
                return;
            }
            *last_snapshot = Some(snapshot.clone());
            publication.pending_delivery = Some(snapshot);
            let should_drain = !publication.delivering;
            publication.delivering = true;
            should_drain
        } else {
            false
        };
        if should_drain {
            self.drain_ui_snapshot_delivery();
        }
    }

    fn drain_ui_snapshot_delivery(&self) {
        loop {
            let snapshot = {
                let Ok(mut publication) = self.last_published_ui_snapshot.lock() else {
                    return;
                };
                match publication.pending_delivery.take() {
                    Some(snapshot) => snapshot,
                    None => {
                        publication.delivering = false;
                        return;
                    }
                }
            };
            self.emit_published_ui_snapshot(snapshot);
        }
    }

    /// Whether the publication owner is currently delivering to an external
    /// observer. A reentrant layout read must withhold fresh publication here.
    pub fn is_delivering_ui_snapshot(&self) -> bool {
        self.last_published_ui_snapshot
            .lock()
            .map_or(true, |publication| publication.delivering)
    }

    fn emit_published_ui_snapshot(&self, snapshot: UiSnapshot) {
        if let Ok(slot) = self.ui_snapshot_sink.lock() {
            let sink = slot.as_ref().cloned();
            drop(slot);
            if let Some(sink) = sink {
                sink(snapshot);
            }
        }
    }

    /// Publish actual renderer evidence through the existing snapshot owner.
    /// The final AppCore observation invalidates any mismatched source.
    pub fn begin_renderer_capture(&self) -> Option<RendererCaptureReceipt> {
        let mut publication = self.last_published_ui_snapshot.lock().ok()?;
        publication.next_renderer_revision = publication.next_renderer_revision.checked_add(1)?;
        let receipt = RendererCaptureReceipt {
            identity: Arc::new(()),
            revision: publication.next_renderer_revision,
        };
        publication.active_renderer = Some(receipt.clone());
        let invalidated = publication.snapshot.as_mut().map(|snapshot| {
            snapshot.rendered_control_frame =
                aura_app::ui::contract::RenderedControlFrame::Unavailable;
            for observation in &mut snapshot.rendered_controls {
                observation.state = aura_app::ui::contract::ControlLayoutState::Unavailable;
                observation.capture_revision = receipt.revision;
            }
            snapshot.clone()
        });
        let should_drain = if let Some(snapshot) = invalidated {
            publication.pending_delivery = Some(snapshot);
            let should_drain = !publication.delivering;
            publication.delivering = true;
            should_drain
        } else {
            false
        };
        drop(publication);
        if should_drain {
            self.drain_ui_snapshot_delivery();
        }
        Some(receipt)
    }

    /// Publish the complete rendered-control multiset for the exact active receipt.
    pub fn invalidate_rendered_control_layout(&self, receipt: &RendererCaptureReceipt) {
        let should_drain = if let Ok(mut publication) = self.last_published_ui_snapshot.lock() {
            if !publication
                .active_renderer
                .as_ref()
                .is_some_and(|active| Arc::ptr_eq(&active.identity, &receipt.identity))
            {
                return;
            }
            let Some(snapshot) = publication.snapshot.as_mut() else {
                return;
            };
            snapshot.rendered_control_frame =
                aura_app::ui::contract::RenderedControlFrame::Unavailable;
            for observation in &mut snapshot.rendered_controls {
                observation.state = aura_app::ui::contract::ControlLayoutState::Unavailable;
            }
            publication.pending_delivery = Some(snapshot.clone());
            let should_drain = !publication.delivering;
            publication.delivering = true;
            should_drain
        } else {
            false
        };
        if should_drain {
            self.drain_ui_snapshot_delivery();
        }
    }

    /// Publish the complete rendered-control multiset for the exact active receipt.
    pub fn publish_rendered_control_snapshot(
        &self,
        receipt: &RendererCaptureReceipt,
        mut snapshot: UiSnapshot,
        observations: Vec<aura_app::ui::contract::RenderedControlObservation>,
    ) {
        snapshot.rendered_controls = observations;
        self.publish_ui_snapshot_with_capture(snapshot, Some(receipt));
    }

    pub fn reset_published_ui_snapshot(&self) {
        if let Ok(mut publication) = self.last_published_ui_snapshot.lock() {
            publication.snapshot = None;
            publication.active_renderer = None;
            publication.pending_delivery = None;
        }
    }
}

#[cfg(test)]
mod notification_snapshot_tests {
    use super::super::{NotificationSelectionId, UiModel};

    #[test]
    fn repeated_notification_ids_export_a_valid_snapshot() {
        let mut model = UiModel::new("authority-test".to_string());
        let id = NotificationSelectionId("inv-repeated".to_string());
        model.notification_ids = vec![id.clone(), id.clone()];
        model.selected_notification_id = Some(id);

        let snapshot = model.semantic_snapshot();
        snapshot
            .validate_invariants()
            .unwrap_or_else(|error| panic!("snapshot must stay valid: {error}"));
    }
}

#[cfg(test)]
mod onboarding_navigation_snapshot_tests {
    use super::*;

    fn mounted_ui_root() -> dioxus::prelude::Element {
        use dioxus::prelude::*;
        let controller = use_context::<Arc<UiController>>();
        rsx! { crate::AuraUiRoot { controller } }
    }

    #[test]
    fn onboarding_exports_no_navigation_selection_until_account_is_ready() {
        let mut model = UiModel::new("authority-test".to_string());
        model.set_account_setup_state(false, "", None);
        model.set_screen(ScreenId::Onboarding);

        let onboarding = model.semantic_snapshot();
        assert_eq!(onboarding.screen, ScreenId::Onboarding);
        assert!(onboarding
            .lists
            .iter()
            .all(|list| list.id != ListId::Navigation));
        assert!(onboarding
            .selections
            .iter()
            .all(|selection| selection.list != ListId::Navigation));
        onboarding
            .validate_invariants()
            .expect("valid onboarding projection");

        model.set_account_setup_state(true, "", None);
        model.set_screen(ScreenId::Chat);
        let ready = model.semantic_snapshot();
        let navigation = ready
            .lists
            .iter()
            .find(|list| list.id == ListId::Navigation)
            .expect("ready shell has navigation");
        let selected = navigation
            .items
            .iter()
            .find(|item| item.selected)
            .expect("ready navigation has selected row");
        assert_eq!(
            selected.id,
            ScreenId::Chat.help_label().to_ascii_lowercase()
        );
        assert_eq!(
            ready.selected_item_id(ListId::Navigation),
            Some(selected.id.as_str())
        );
        ready.validate_invariants().expect("valid ready projection");

        model.set_account_setup_state(false, "", None);
        assert_eq!(model.screen, ScreenId::Onboarding);
        let signed_out = model.semantic_snapshot();
        assert_eq!(signed_out.screen, model.screen);
        assert!(signed_out.selected_item_id(ListId::Navigation).is_none());
        signed_out
            .validate_invariants()
            .expect("valid onboarding after ready Chat");

        model.set_account_setup_state(true, "", None);
        assert_eq!(model.screen, ScreenId::Neighborhood);
        let re_ready = model.semantic_snapshot();
        assert_eq!(re_ready.screen, model.screen);
        assert_eq!(
            re_ready.selected_item_id(ListId::Navigation),
            Some(
                ScreenId::Neighborhood
                    .help_label()
                    .to_ascii_lowercase()
                    .as_str()
            )
        );
        re_ready
            .validate_invariants()
            .expect("valid ready navigation after onboarding");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn mounted_onboarding_and_ready_navigation_publish_valid_projections() {
        let app = Arc::new(async_lock::RwLock::new(
            aura_app::AppCore::new(aura_app::AppConfig::default()).expect("fixture app"),
        ));
        aura_app::AppCore::init_signals_with_hooks(&app)
            .await
            .expect("fixture signals");
        let controller = Arc::new(UiController::new(
            app,
            Arc::new(crate::MemoryClipboard::default()),
        ));
        let published = Arc::new(std::sync::Mutex::new(Vec::<UiSnapshot>::new()));
        controller.set_ui_snapshot_sink(Arc::new({
            let published = published.clone();
            move |snapshot| published.lock().expect("snapshot sink").push(snapshot)
        }));
        controller.set_account_setup_state(false, "", None);
        controller.set_screen(ScreenId::Onboarding);
        published.lock().expect("snapshot sink").clear();

        let mut dom = dioxus::dioxus_core::VirtualDom::new(mounted_ui_root);
        dom.provide_root_context(controller.clone());
        dom.rebuild_in_place();
        dom.render_immediate_to_vec();
        let onboarding = published
            .lock()
            .expect("snapshot sink")
            .last()
            .cloned()
            .expect("mounted onboarding publication");
        assert_eq!(onboarding.screen, ScreenId::Onboarding);
        assert!(onboarding.selected_item_id(ListId::Navigation).is_none());
        onboarding
            .validate_invariants()
            .expect("mounted onboarding valid");

        published.lock().expect("snapshot sink").clear();
        controller.set_account_setup_state(true, "", None);
        controller.set_screen(ScreenId::Chat);
        dom.render_immediate_to_vec();
        let ready = published
            .lock()
            .expect("snapshot sink")
            .last()
            .cloned()
            .expect("mounted ready publication");
        assert_eq!(ready.screen, ScreenId::Chat);
        assert_eq!(
            ready.selected_item_id(ListId::Navigation),
            Some(ScreenId::Chat.help_label().to_ascii_lowercase().as_str())
        );
        ready.validate_invariants().expect("mounted ready valid");

        published.lock().expect("snapshot sink").clear();
        controller.set_account_setup_state(false, "", None);
        dom.render_immediate_to_vec();
        let signed_out = published
            .lock()
            .expect("snapshot sink")
            .last()
            .cloned()
            .expect("mounted onboarding after ready Chat");
        assert_eq!(signed_out.screen, ScreenId::Onboarding);
        assert!(signed_out.selected_item_id(ListId::Navigation).is_none());
        signed_out
            .validate_invariants()
            .expect("mounted signed-out projection valid");
    }
}

#[cfg(test)]
mod rendered_publication_tests {
    use super::*;
    use aura_app::ui::contract::{
        ControlId, ControlLayoutState, ControlRenderSource, RenderedControlObservation,
    };
    fn controller() -> UiController {
        UiController::new(
            Arc::new(async_lock::RwLock::new(
                aura_app::AppCore::new(aura_app::AppConfig::default()).expect("fixture app"),
            )),
            Arc::new(crate::MemoryClipboard::default()),
        )
    }
    fn observation(
        snapshot: &UiSnapshot,
        receipt: &RendererCaptureReceipt,
        state: ControlLayoutState,
    ) -> RenderedControlObservation {
        RenderedControlObservation {
            control: ControlId::OnboardingRoot,
            capture_revision: receipt.revision(),
            source: ControlRenderSource::from_snapshot(snapshot),
            state,
        }
    }
    #[test]
    fn previous_mount_disposal_cannot_overwrite_new_mount_with_identical_source() {
        let controller = controller();
        let old = controller.begin_renderer_capture().expect("old mount");
        let current = controller.begin_renderer_capture().expect("new mount");
        let snapshot = controller.semantic_model_snapshot();
        controller.publish_rendered_control_snapshot(
            &current,
            snapshot.clone(),
            vec![observation(
                &snapshot,
                &current,
                ControlLayoutState::Visible,
            )],
        );
        controller.publish_rendered_control_snapshot(
            &old,
            snapshot.clone(),
            vec![observation(&snapshot, &old, ControlLayoutState::Disposed)],
        );
        let published = controller.last_published_ui_snapshot.lock().expect("owner");
        let actual = &published
            .snapshot
            .as_ref()
            .expect("published")
            .rendered_controls;
        assert_eq!(actual.len(), 1);
        assert_eq!(actual[0].capture_revision, current.revision());
        assert_eq!(actual[0].state, ControlLayoutState::Visible);
    }
    #[test]
    fn duplicate_rendered_instances_survive_publication_and_same_source_hidden_supersedes() {
        let controller = controller();
        let receipt = controller.begin_renderer_capture().expect("capture");
        let snapshot = controller.semantic_model_snapshot();
        let visible = observation(&snapshot, &receipt, ControlLayoutState::Visible);
        controller.publish_rendered_control_snapshot(
            &receipt,
            snapshot.clone(),
            vec![visible.clone(), visible],
        );
        assert_eq!(
            controller
                .last_published_ui_snapshot
                .lock()
                .expect("owner")
                .snapshot
                .as_ref()
                .expect("published")
                .rendered_controls
                .len(),
            2
        );
        let next = controller
            .begin_renderer_capture()
            .expect("local hidden render");
        controller.publish_rendered_control_snapshot(
            &next,
            snapshot.clone(),
            vec![observation(&snapshot, &next, ControlLayoutState::Hidden)],
        );
        let published = controller.last_published_ui_snapshot.lock().expect("owner");
        let actual = &published
            .snapshot
            .as_ref()
            .expect("published")
            .rendered_controls;
        assert_eq!(actual.len(), 1);
        assert_eq!(actual[0].state, ControlLayoutState::Hidden);
    }
    #[test]
    fn reentrant_sink_can_reset_begin_read_and_publish_without_deadlock() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let controller = Arc::new(controller());
        let weak = Arc::downgrade(&controller);
        let entered = Arc::new(AtomicBool::new(false));
        let delivered = Arc::new(Mutex::new(Vec::new()));
        let captured = delivered.clone();
        controller.set_ui_snapshot_sink(Arc::new(move |snapshot| {
            captured.lock().expect("events").push(snapshot);
            if !entered.swap(true, Ordering::SeqCst) {
                let controller = weak.upgrade().expect("live controller");
                controller.reset_published_ui_snapshot();
                let receipt = controller
                    .begin_renderer_capture()
                    .expect("reentrant capture");
                let snapshot = controller.semantic_model_snapshot();
                controller.publish_rendered_control_snapshot(
                    &receipt,
                    snapshot.clone(),
                    vec![observation(&snapshot, &receipt, ControlLayoutState::Hidden)],
                );
            }
        }));
        let receipt = controller
            .begin_renderer_capture()
            .expect("initial capture");
        let snapshot = controller.semantic_model_snapshot();
        controller.publish_rendered_control_snapshot(
            &receipt,
            snapshot.clone(),
            vec![observation(
                &snapshot,
                &receipt,
                ControlLayoutState::Visible,
            )],
        );
        controller.publish_rendered_control_snapshot(
            &receipt,
            snapshot.clone(),
            vec![observation(
                &snapshot,
                &receipt,
                ControlLayoutState::Disposed,
            )],
        );
        let delivered = delivered.lock().expect("events");
        assert_eq!(delivered.len(), 2);
        assert!(
            delivered[1].rendered_controls[0].capture_revision
                > delivered[0].rendered_controls[0].capture_revision
        );
        assert_eq!(
            delivered[1].rendered_controls[0].state,
            ControlLayoutState::Hidden
        );
    }
    #[test]
    fn final_app_source_rejects_foreign_observation_without_reusing_previous_positive() {
        let controller = controller();
        let receipt = controller.begin_renderer_capture().expect("capture");
        let snapshot = controller.semantic_model_snapshot();
        let mut stale = observation(&snapshot, &receipt, ControlLayoutState::Visible);
        stale.source.projections.homes = Some(u64::MAX);
        controller.publish_rendered_control_snapshot(&receipt, snapshot, vec![stale]);
        assert!(controller
            .last_published_ui_snapshot
            .lock()
            .expect("owner")
            .snapshot
            .as_ref()
            .expect("published")
            .rendered_controls
            .is_empty());
    }
}

#[cfg(test)]
mod modal_focus_tests {
    use super::*;
    #[test]
    fn actual_modal_snapshot_retains_each_exact_active_field() {
        for field in [
            FieldId::Nickname,
            FieldId::InvitationReceiverNickname,
            FieldId::InvitationMessage,
            FieldId::InvitationTtl,
        ] {
            let mut model = UiModel::new("local".to_string());
            model.active_modal = Some(ActiveModal::CreateInvitation(CreateInvitationModalState {
                active_field: field,
                ..Default::default()
            }));
            assert_eq!(
                model.semantic_snapshot().focused_control,
                Some(ControlId::Field(field))
            );
        }
    }
}
