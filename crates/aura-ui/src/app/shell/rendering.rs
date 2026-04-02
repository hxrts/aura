use super::*;

#[component]
pub fn AuraUiRoot(controller: Arc<UiController>) -> Element {
    rsx! {
        ThemeProvider {
            theme: themes::neutral(),
            color_scheme: ColorScheme::Dark,
            style {
                r#"
                [data-slot="toaster"] {{
                    z-index: 2147483647 !important;
                    isolation: isolate !important;
                }}

                [data-slot="toast"] {{
                    z-index: 2147483647 !important;
                    min-height: 5rem !important;
                    padding-top: 1.25rem !important;
                    padding-bottom: 1.25rem !important;
                }}

                button:focus-visible {{
                    outline: none;
                }}
                "#
            }
            div {
                id: ControlId::ToastRegion
                    .web_dom_id()
                    .required_dom_id("ToastRegion must define a web DOM id"),
                style: "--normal-bg: var(--popover); --normal-text: var(--popover-foreground); --normal-border: var(--border); position: relative; z-index: 2147483647;",
                ToastProvider {
                    default_duration: Duration::from_secs(5),
                    max_toasts: 8,
                    position: ToastPosition::BottomLeft,
                    AuraUiShell { controller }
                }
            }
        }
    }
}

#[component]
fn AuraUiShell(controller: Arc<UiController>) -> Element {
    let mut render_tick = use_signal(|| 0_u64);
    let render_tick_value = render_tick();
    let mut last_toast_key = use_signal(|| None::<String>);
    let mut last_chat_selection_key = use_signal(|| None::<String>);
    let runtime_bridge_started = use_signal(|| false);
    let neighborhood_runtime = use_signal(NeighborhoodRuntimeView::default);
    let chat_runtime = use_signal(ChatRuntimeView::default);
    let contacts_runtime = use_signal(ContactsRuntimeView::default);
    let settings_runtime = use_signal(SettingsRuntimeView::default);
    let notifications_runtime = use_signal(NotificationsRuntimeView::default);
    let toasts = use_toast();
    let theme = use_theme();

    let Some(model) = controller.ui_model() else {
        return rsx! {
            main {
                class: "min-h-screen bg-background text-foreground grid place-items-center",
                p { "UI state unavailable" }
            }
        };
    };

    let controller_for_toast = controller.clone();

    use_effect(move || {
        let _ = render_tick();
        let Some(current_model) = controller_for_toast.ui_model() else {
            return;
        };
        let next_key = current_model.toast.as_ref().map(|toast| {
            format!(
                "{}::{}::{}",
                current_model.toast_key, toast.icon, toast.message
            )
        });

        if last_toast_key() == next_key {
            return;
        }

        if let Some(toast) = &current_model.toast {
            let opts = Some(ToastOptions {
                description: None,
                duration: Some(Duration::from_secs(5)),
                permanent: false,
                action: None,
                on_dismiss: None,
            });

            match toast.icon {
                'Y' | 'y' | '+' | '✓' => toasts.success(toast.message.clone(), opts),
                'X' | 'x' | '-' | '!' | '✗' => toasts.error(toast.message.clone(), opts),
                _ => toasts.info(toast.message.clone(), opts),
            };
        }

        last_toast_key.set(next_key);
    });

    let controller_for_chat_selection = controller.clone();
    let mut chat_for_selection_change = chat_runtime;
    use_effect(move || {
        let _ = render_tick();
        let Some(current_model) = controller_for_chat_selection.ui_model() else {
            return;
        };
        let selected_channel_key = current_model.selected_channel_id().map(str::to_string);
        if last_chat_selection_key() == selected_channel_key {
            return;
        }

        last_chat_selection_key.set(selected_channel_key);

        let controller_for_reload = controller_for_chat_selection.clone();
        spawn_ui(async move {
            chat_for_selection_change
                .set(load_chat_runtime_view(controller_for_reload.clone()).await);
            controller_for_reload.request_rerender();
        });
    });

    use_runtime_bridge_subscriptions(
        controller.clone(),
        runtime_bridge_started,
        neighborhood_runtime,
        chat_runtime,
        contacts_runtime,
        settings_runtime,
        notifications_runtime,
    );

    let resolved_scheme = theme.resolved_scheme();
    let shell_state = ShellRenderState::from_runtime(
        &model,
        neighborhood_runtime,
        chat_runtime,
        contacts_runtime,
        settings_runtime,
        notifications_runtime,
    );
    let shell_header_exit_input_controller = controller.clone();
    let shell_footer_exit_input_controller = controller.clone();
    let keydown_runtime_snapshot = shell_state.runtime.neighborhood.clone();
    let keydown_chat_runtime = shell_state.runtime.chat.clone();
    let keydown_contacts_runtime = shell_state.runtime.contacts.clone();
    let keydown_settings_runtime = shell_state.runtime.settings.clone();
    let keydown_model = model.clone();
    let modal_runtime_snapshot = shell_state.runtime.neighborhood.clone();
    let modal_chat_runtime = shell_state.runtime.chat.clone();
    let modal_contacts_runtime = shell_state.runtime.contacts.clone();
    let modal_settings_runtime = shell_state.runtime.settings.clone();
    let modal_model = model.clone();
    let keydown_selected_member_key = shell_state.selected_member_key.clone();
    let modal_selected_member_key = shell_state.selected_member_key.clone();
    let modal = shell_state.modal.clone();
    let modal_state = shell_state.modal_state;
    let add_device_modal_state = shell_state.add_device_modal_state.clone();
    let cancel_add_device_ceremony_id = shell_state.cancel_add_device_ceremony_id.clone();
    let rerender = schedule_update();
    controller.set_rerender_callback(rerender.clone());
    let keydown_rerender = rerender.clone();
    let cancel_rerender = rerender.clone();
    let dedicated_primary_rerender = rerender.clone();
    let generic_confirm_rerender = rerender.clone();
    shell_state
        .runtime
        .publish_semantic_snapshot(controller.as_ref(), &model);
    let keydown_controller = controller.clone();
    rsx! {
        main {
            id: ControlId::AppRoot
                .web_dom_id()
                .required_dom_id("AppRoot must define a web DOM id"),
            "data-render-tick": "{render_tick_value}",
            class: "relative flex min-h-screen flex-col overflow-y-auto bg-background text-foreground font-sans outline-none lg:h-[100dvh] lg:min-h-[100dvh] lg:overflow-hidden",
            tabindex: 0,
            autofocus: true,
            onmounted: move |mounted| {
                spawn_ui(async move {
                    let _ = mounted.data().set_focus(true).await;
                });
            },
            onkeydown: move |event| {
                if should_skip_global_key(keydown_controller.as_ref(), event.data().as_ref()) {
                    return;
                }
                if let Key::Character(text) = event.data().key() {
                    if handle_runtime_character_shortcut(
                        keydown_controller.clone(),
                        &model,
                        &keydown_runtime_snapshot,
                        &text,
                        keydown_rerender.clone(),
                    ) {
                        event.prevent_default();
                        return;
                    }
                }
                if matches!(event.data().key(), Key::Enter)
                    && matches!(model.screen, ScreenId::Chat)
                    && model.input_mode
                    && submit_runtime_chat_input(
                        keydown_controller.clone(),
                        shell_state.runtime.chat.active_channel.clone(),
                        model.input_buffer.clone(),
                        keydown_rerender.clone(),
                    )
                {
                    event.prevent_default();
                    return;
                }
                if matches!(event.data().key(), Key::Enter)
                    && submit_runtime_modal_action(
                                keydown_controller.clone(),
                                modal_state,
                                add_device_modal_state
                                    .as_ref()
                                    .map(|state| state.step)
                                    .unwrap_or(AddDeviceWizardStep::Name),
                                add_device_modal_state
                                    .as_ref()
                                    .and_then(|state| state.ceremony_id.clone()),
                                add_device_modal_state
                                    .as_ref()
                                    .map(|state| state.is_complete)
                                    .unwrap_or(false),
                                add_device_modal_state
                                    .as_ref()
                                    .map(|state| state.has_failed)
                                    .unwrap_or(false),
                                model.modal_text_value().unwrap_or_default(),
                        keydown_runtime_snapshot.clone(),
                        keydown_chat_runtime.clone(),
                        keydown_contacts_runtime.clone(),
                        keydown_settings_runtime.clone(),
                        selected_home_id_for_modal(&keydown_runtime_snapshot, &keydown_model),
                        keydown_selected_member_key.clone(),
                        keydown_rerender.clone(),
                    )
                {
                    event.prevent_default();
                    return;
                }
                if handle_keydown(keydown_controller.as_ref(), event.data().as_ref()) {
                    event.prevent_default();
                    render_tick.set(render_tick() + 1);
                }
            },
            nav {
                id: ControlId::NavRoot
                    .web_dom_id()
                    .required_dom_id("NavRoot must define a web DOM id"),
                class: "bg-background/95 backdrop-blur supports-[backdrop-filter]:bg-background/80",
                onclick: move |_| {
                    if shell_state.should_exit_insert_mode {
                        shell_header_exit_input_controller.exit_input_mode();
                        render_tick.set(render_tick() + 1);
                    }
                },
                div {
                    class: "relative flex items-end px-4 pt-6 pb-0 sm:px-6",
                    div {
                        class: "absolute bottom-0 left-4 z-10 flex items-center justify-start gap-3 sm:left-6",
                        button {
                            r#type: "button",
                            id: "aura-nav-brand",
                            class: "inline-flex h-8 items-center justify-center whitespace-nowrap px-6 text-xs font-sans font-bold uppercase leading-none tracking-[0.12em] text-foreground cursor-pointer hover:text-muted-foreground transition-colors",
                            onclick: {
                                move |_| {
                                    controller.set_screen(ScreenId::Neighborhood);
                                    render_tick.set(render_tick() + 1);
                                }
                            },
                            "AURA"
                        }
                    }
                    div {
                        class: "w-full min-w-0 overflow-x-auto px-16 [::-webkit-scrollbar]:hidden sm:px-24",
                        div {
                            class: "mx-auto flex h-8 min-w-max items-center justify-center gap-2",
                            for (screen, label, is_active) in nav_tabs(model.screen) {
                                button {
                                    r#type: "button",
                                    id: nav_button_id(screen),
                                    class: nav_tab_class(is_active),
                                    onclick: {
                                        let controller = controller.clone();
                                        move |_| {
                                            let before_log = format!(
                                                "nav_click start screen={}",
                                                screen.help_label()
                                            );
                                            controller.push_log(&before_log);
                                            harness_log(&before_log);
                                            controller.set_screen(screen);
                                            let after_log = format!(
                                                "nav_click done screen={}",
                                                screen.help_label()
                                            );
                                            controller.push_log(&after_log);
                                            harness_log(&after_log);
                                            render_tick.set(render_tick() + 1);
                                        }
                                    },
                                    "{label}"
                                }
                            }
                        }
                    }
                }
            }
            div {
                class: "flex-1 px-4 py-4 sm:px-6 sm:py-5 lg:min-h-0 lg:overflow-hidden",
                {render_screen_content(
                    &model,
                    &shell_state.runtime.neighborhood,
                    &shell_state.runtime.chat,
                    &shell_state.runtime.contacts,
                    &shell_state.runtime.settings,
                    &shell_state.runtime.notifications,
                    controller.clone(),
                    render_tick,
                    theme,
                    resolved_scheme,
                )}
            }

            div {
                id: ControlId::ModalRegion
                    .web_dom_id()
                    .required_dom_id("ModalRegion must define a web DOM id"),
                class: "contents",
                if let Some(modal) = modal {
                    if let Some(add_device_state) = model.add_device_modal() {
                        if !matches!(add_device_state.step, AddDeviceWizardStep::Name) {
                            UiDeviceEnrollmentModal {
                            modal_id: ModalId::AddDevice,
                            title: if matches!(add_device_state.step, AddDeviceWizardStep::ShareCode) {
                                "Add Device — Step 2 of 3".to_string()
                            } else {
                                "Add Device — Step 3 of 3".to_string()
                            },
                            enrollment_code: add_device_state.enrollment_code.clone(),
                            ceremony_id: add_device_state
                                .ceremony_id
                                .as_ref()
                                .map(ToString::to_string),
                            device_name: add_device_state.device_name.clone(),
                            accepted_count: add_device_state.accepted_count,
                            total_count: add_device_state.total_count,
                            threshold: add_device_state.threshold,
                            is_complete: add_device_state.is_complete,
                            has_failed: add_device_state.has_failed,
                            error_message: add_device_state.error_message.clone(),
                            copied: add_device_state.code_copied,
                            primary_label: if matches!(add_device_state.step, AddDeviceWizardStep::ShareCode) {
                                "Next".to_string()
                            } else if add_device_state.is_complete || add_device_state.has_failed {
                                "Close".to_string()
                            } else {
                                "Refresh".to_string()
                            },
                            on_cancel: {
                                let controller = controller.clone();
                                let add_device_modal_state = add_device_modal_state.clone();
                                move |_| {
                                    if !cancel_runtime_modal_action(
                                        controller.clone(),
                                        modal_state,
                                        add_device_modal_state
                                            .as_ref()
                                            .map(|state| state.step)
                                            .unwrap_or(AddDeviceWizardStep::Name),
                                        cancel_add_device_ceremony_id.clone(),
                                        add_device_modal_state
                                            .as_ref()
                                            .map(|state| state.is_complete)
                                            .unwrap_or(false),
                                        add_device_modal_state
                                            .as_ref()
                                            .map(|state| state.has_failed)
                                            .unwrap_or(false),
                                        cancel_rerender.clone(),
                                    ) {
                                        controller.send_key_named("esc", 1);
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                            },
                            on_copy: {
                                let controller = controller.clone();
                                let enrollment_code = add_device_state.enrollment_code.clone();
                                move |_| {
                                    controller.write_clipboard(&enrollment_code);
                                    controller.mark_add_device_code_copied();
                                    controller.info_toast("Copied to clipboard");
                                    render_tick.set(render_tick() + 1);
                                }
                            },
                            on_primary: {
                                let controller = controller.clone();
                                let add_device_modal_state = add_device_modal_state.clone();
                                move |_| {
                                    if !submit_runtime_modal_action(
                                        controller.clone(),
                                        modal_state,
                                        add_device_modal_state
                                            .as_ref()
                                            .map(|state| state.step)
                                            .unwrap_or(AddDeviceWizardStep::Name),
                                        add_device_modal_state
                                            .as_ref()
                                            .and_then(|state| state.ceremony_id.clone()),
                                        add_device_modal_state
                                            .as_ref()
                                            .map(|state| state.is_complete)
                                            .unwrap_or(false),
                                        add_device_modal_state
                                            .as_ref()
                                            .map(|state| state.has_failed)
                                            .unwrap_or(false),
                                        modal_model.modal_text_value().unwrap_or_default(),
                                        modal_runtime_snapshot.clone(),
                                        modal_chat_runtime.clone(),
                                        modal_contacts_runtime.clone(),
                                        modal_settings_runtime.clone(),
                                        selected_home_id_for_modal(&modal_runtime_snapshot, &modal_model),
                                        modal_selected_member_key.clone(),
                                        dedicated_primary_rerender.clone(),
                                    ) {
                                        controller.send_key_named("enter", 1);
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                            }
                        }
                    } else {
                        UiModal {
                            modal,
                            on_cancel: {
                                let controller = controller.clone();
                                let add_device_modal_state = add_device_modal_state.clone();
                                move |_| {
                                    if !cancel_runtime_modal_action(
                                        controller.clone(),
                                        modal_state,
                                        add_device_modal_state
                                            .as_ref()
                                            .map(|state| state.step)
                                            .unwrap_or(AddDeviceWizardStep::Name),
                                        cancel_add_device_ceremony_id.clone(),
                                        add_device_modal_state
                                            .as_ref()
                                            .map(|state| state.is_complete)
                                            .unwrap_or(false),
                                        add_device_modal_state
                                            .as_ref()
                                            .map(|state| state.has_failed)
                                            .unwrap_or(false),
                                        cancel_rerender.clone(),
                                    ) {
                                        controller.send_key_named("esc", 1);
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                            },
                            on_confirm: {
                                let controller = controller.clone();
                                let add_device_modal_state = add_device_modal_state.clone();
                                move |_| {
                                    if !submit_runtime_modal_action(
                                        controller.clone(),
                                        modal_state,
                                        add_device_modal_state
                                            .as_ref()
                                            .map(|state| state.step)
                                            .unwrap_or(AddDeviceWizardStep::Name),
                                        add_device_modal_state
                                            .as_ref()
                                            .and_then(|state| state.ceremony_id.clone()),
                                        add_device_modal_state
                                            .as_ref()
                                            .map(|state| state.is_complete)
                                            .unwrap_or(false),
                                        add_device_modal_state
                                            .as_ref()
                                            .map(|state| state.has_failed)
                                            .unwrap_or(false),
                                        modal_model.modal_text_value().unwrap_or_default(),
                                        modal_runtime_snapshot.clone(),
                                        modal_chat_runtime.clone(),
                                        modal_contacts_runtime.clone(),
                                        modal_settings_runtime.clone(),
                                        selected_home_id_for_modal(&modal_runtime_snapshot, &modal_model),
                                        modal_selected_member_key.clone(),
                                        generic_confirm_rerender.clone(),
                                    ) {
                                        controller.send_key_named("enter", 1);
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                            },
                            on_input_change: {
                                let controller = controller.clone();
                                move |(field_id, value): (FieldId, String)| {
                                    controller.set_modal_field_value(field_id, &value);
                                    render_tick.set(render_tick() + 1);
                                }
                            },
                            on_input_focus: {
                                let controller = controller.clone();
                                move |field_id: FieldId| {
                                    controller.set_modal_active_field(field_id);
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                        }
                        }
                    } else if matches!(modal_state, Some(ModalState::SwitchAuthority)) {
                        UiAuthorityPickerModal {
                        modal_id: ModalId::SwitchAuthority,
                        title: active_modal_title(&model)
                            .unwrap_or_else(|| "Switch Authority".to_string()),
                        current_label: shell_state
                            .runtime
                            .settings
                            .authorities
                            .iter()
                            .find(|authority| authority.is_current)
                            .map(|authority| authority.label.clone())
                            .unwrap_or_else(|| "Current Authority".to_string()),
                        current_id: shell_state.runtime.settings.authority_id.clone(),
                        mfa_policy: shell_state.runtime.settings.mfa_policy.clone(),
                        authorities: model
                            .authorities
                            .iter()
                            .map(|authority| AuthorityPickerItem {
                                id: authority.id,
                                label: authority.label.clone(),
                                is_current: authority.is_current,
                                is_selected: authority.selected,
                            })
                            .collect(),
                        on_cancel: {
                            let controller = controller.clone();
                            move |_| {
                                controller.send_key_named("esc", 1);
                                render_tick.set(render_tick() + 1);
                            }
                        },
                        on_select: {
                            let controller = controller.clone();
                            move |index| {
                                controller.set_selected_authority_index(index);
                                render_tick.set(render_tick() + 1);
                            }
                        },
                        on_confirm: {
                            let controller = controller.clone();
                            let add_device_modal_state = add_device_modal_state.clone();
                            move |_| {
                                if !submit_runtime_modal_action(
                                    controller.clone(),
                                    modal_state,
                                    add_device_modal_state
                                        .as_ref()
                                        .map(|state| state.step)
                                        .unwrap_or(AddDeviceWizardStep::Name),
                                    add_device_modal_state
                                        .as_ref()
                                        .and_then(|state| state.ceremony_id.clone()),
                                    add_device_modal_state
                                        .as_ref()
                                        .map(|state| state.is_complete)
                                        .unwrap_or(false),
                                    add_device_modal_state
                                        .as_ref()
                                        .map(|state| state.has_failed)
                                        .unwrap_or(false),
                                    modal_model.modal_text_value().unwrap_or_default(),
                                    modal_runtime_snapshot.clone(),
                                    modal_chat_runtime.clone(),
                                    modal_contacts_runtime.clone(),
                                    modal_settings_runtime.clone(),
                                    selected_home_id_for_modal(&modal_runtime_snapshot, &modal_model),
                                    modal_selected_member_key.clone(),
                                    dedicated_primary_rerender.clone(),
                                ) {
                                    controller.send_key_named("enter", 1);
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                        }
                        }
                    } else {
                        UiModal {
                        modal,
                        on_cancel: {
                            let controller = controller.clone();
                            let add_device_modal_state = add_device_modal_state.clone();
                            move |_| {
                                if !cancel_runtime_modal_action(
                                    controller.clone(),
                                    modal_state,
                                    add_device_modal_state
                                        .as_ref()
                                        .map(|state| state.step)
                                        .unwrap_or(AddDeviceWizardStep::Name),
                                    cancel_add_device_ceremony_id.clone(),
                                    add_device_modal_state
                                        .as_ref()
                                        .map(|state| state.is_complete)
                                        .unwrap_or(false),
                                    add_device_modal_state
                                        .as_ref()
                                        .map(|state| state.has_failed)
                                        .unwrap_or(false),
                                    cancel_rerender.clone(),
                                ) {
                                    controller.send_key_named("esc", 1);
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                        },
                        on_confirm: {
                            let controller = controller.clone();
                            let add_device_modal_state = add_device_modal_state.clone();
                            move |_| {
                                if !submit_runtime_modal_action(
                                    controller.clone(),
                                    modal_state,
                                    add_device_modal_state
                                        .as_ref()
                                        .map(|state| state.step)
                                        .unwrap_or(AddDeviceWizardStep::Name),
                                    add_device_modal_state
                                        .as_ref()
                                        .and_then(|state| state.ceremony_id.clone()),
                                    add_device_modal_state
                                        .as_ref()
                                        .map(|state| state.is_complete)
                                        .unwrap_or(false),
                                    add_device_modal_state
                                        .as_ref()
                                        .map(|state| state.has_failed)
                                        .unwrap_or(false),
                                    modal_model.modal_text_value().unwrap_or_default(),
                                    modal_runtime_snapshot.clone(),
                                    modal_chat_runtime.clone(),
                                    modal_contacts_runtime.clone(),
                                    modal_settings_runtime.clone(),
                                    selected_home_id_for_modal(&modal_runtime_snapshot, &modal_model),
                                    modal_selected_member_key.clone(),
                                    generic_confirm_rerender.clone(),
                                ) {
                                    controller.send_key_named("enter", 1);
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                        },
                        on_input_change: {
                            let controller = controller.clone();
                            move |(field_id, value): (FieldId, String)| {
                                controller.set_modal_field_value(field_id, &value);
                                render_tick.set(render_tick() + 1);
                            }
                        },
                        on_input_focus: {
                            let controller = controller.clone();
                            move |field_id: FieldId| {
                                controller.set_modal_active_field(field_id);
                                render_tick.set(render_tick() + 1);
                            }
                        }
                        }
                    }
                }
            }

            div {
                onclick: move |_| {
                    if shell_state.should_exit_insert_mode {
                        shell_footer_exit_input_controller.exit_input_mode();
                        render_tick.set(render_tick() + 1);
                    }
                },
                UiFooter {
                    left: String::new(),
                    network_status: shell_state.footer.network_status,
                    peer_count: shell_state.footer.peer_count,
                    online_count: shell_state.footer.online_count,
                }
            }
        }
    }
}
