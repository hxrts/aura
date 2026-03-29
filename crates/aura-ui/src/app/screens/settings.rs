use super::*;

#[allow(non_snake_case)]
pub(super) fn SettingsScreen(
    model: &UiModel,
    runtime: &SettingsRuntimeView,
    controller: Arc<UiController>,
    mut render_tick: Signal<u64>,
    mut theme: dioxus_shadcn::theme::ThemeContext,
    resolved_scheme: ColorScheme,
) -> Element {
    rsx! {
        div {
            class: "grid w-full gap-3 lg:grid-cols-12 lg:h-full lg:min-h-0 lg:[grid-template-rows:minmax(0,1fr)]",
            UiCard {
                title: "Settings".to_string(),
                subtitle: Some("Manage your account".to_string()),
                extra_class: Some("lg:col-span-4".to_string()),
                UiCardBody {
                    extra_class: Some("gap-2".to_string()),
                    for section in SettingsSection::ALL {
                        UiListButton {
                            id: Some(list_item_dom_id(ListId::SettingsSections, section.dom_id())),
                            label: section.title().to_string(),
                            active: section == model.settings_section,
                            extra_class: Some("pt-px pb-0".to_string()),
                            onclick: {
                                let controller = controller.clone();
                                move |_| {
                                    controller.set_settings_section(section);
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                        }
                    }
                }
            }

            UiCard {
                title: model.settings_section.title().to_string(),
                subtitle: Some(model.settings_section.subtitle().to_string()),
                extra_class: Some("lg:col-span-8".to_string()),
                if matches!(model.settings_section, SettingsSection::Profile) {
                    UiCardBody {
                        extra_class: Some("gap-2".to_string()),
                        div {
                            class: "aura-list flex flex-col gap-2",
                            UiListItem {
                                label: format!("Nickname: {}", runtime.nickname),
                                secondary: Some("Suggestion for what contacts should call you".to_string()),
                                active: false,
                            }
                            UiListItem {
                                label: format!("Authority: {}", runtime.authority_id),
                                secondary: Some("local".to_string()),
                                active: false,
                            }
                        }
                        UiCardFooter {
                            extra_class: None,
                            div { class: "flex h-full w-full items-end justify-end gap-2 overflow-x-auto",
                                UiButton {
                                    id: Some(
                                        ControlId::SettingsEditNicknameButton
                                            .web_dom_id()
                                            .required_dom_id(
                                                "ControlId::SettingsEditNicknameButton must define a web DOM id"
                                            )
                                            .to_string(),
                                    ),
                                    label: "Edit Nickname".to_string(),
                                    variant: ButtonVariant::Primary,
                                    onclick: {
                                        let controller = controller.clone();
                                        move |_| {
                                            controller.set_settings_section(SettingsSection::Profile);
                                            controller.send_action_keys("e");
                                            render_tick.set(render_tick() + 1);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if matches!(model.settings_section, SettingsSection::GuardianThreshold) {
                    UiCardBody {
                        extra_class: Some("gap-2".to_string()),
                        div {
                            class: "aura-list flex flex-col gap-2",
                            UiListItem {
                                label: format!("Target threshold: {} of {}", runtime.threshold_k, runtime.threshold_n.max(runtime.guardian_count as u8)),
                                secondary: Some(format!("Configured guardians: {}", runtime.guardian_count)),
                                active: false,
                            }
                            UiListItem {
                                label: format!("Recovery bindings: {}", runtime.guardian_binding_count),
                                secondary: Some("Authorities for which this device can approve recovery".to_string()),
                                active: false,
                            }
                        }
                        UiCardFooter {
                            extra_class: None,
                            div { class: "flex h-full w-full items-end justify-end gap-2 overflow-x-auto",
                                UiButton {
                                    id: Some(
                                        ControlId::SettingsConfigureThresholdButton
                                            .web_dom_id()
                                            .required_dom_id(
                                                "ControlId::SettingsConfigureThresholdButton must define a web DOM id"
                                            )
                                            .to_string(),
                                    ),
                                    label: "Configure Threshold".to_string(),
                                    variant: ButtonVariant::Primary,
                                    onclick: {
                                        let controller = controller.clone();
                                        move |_| {
                                            controller.set_settings_section(SettingsSection::GuardianThreshold);
                                            controller.send_action_keys("t");
                                            render_tick.set(render_tick() + 1);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if matches!(model.settings_section, SettingsSection::RequestRecovery) {
                    UiCardBody {
                        extra_class: Some("gap-2".to_string()),
                        div {
                            class: "aura-list flex flex-col gap-2",
                            UiListItem {
                                label: format!("Last status: {}", runtime.active_recovery_label),
                                secondary: Some(format!("Pending approvals to review: {}", runtime.pending_recovery_requests)),
                                active: false,
                            }
                        }
                        UiCardFooter {
                            extra_class: None,
                            div { class: "flex h-full w-full items-end justify-end gap-2 overflow-x-auto",
                                UiButton {
                                    id: Some(
                                        ControlId::SettingsRequestRecoveryButton
                                            .web_dom_id()
                                            .required_dom_id(
                                                "ControlId::SettingsRequestRecoveryButton must define a web DOM id"
                                            )
                                            .to_string(),
                                    ),
                                    label: "Request Recovery".to_string(),
                                    variant: ButtonVariant::Primary,
                                    onclick: {
                                        let controller = controller.clone();
                                        move |_| {
                                            controller.set_settings_section(SettingsSection::RequestRecovery);
                                            controller.send_action_keys("s");
                                            render_tick.set(render_tick() + 1);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if matches!(model.settings_section, SettingsSection::Devices) {
                    UiCardBody {
                        extra_class: Some("gap-2".to_string()),
                        div {
                            class: "aura-list flex flex-col gap-2",
                            for device in &runtime.devices {
                                UiListItem {
                                    label: if device.is_current {
                                        format!("{} (current)", device.name)
                                    } else {
                                        device.name.clone()
                                    },
                                    secondary: Some(if device.is_current {
                                        "Local device".to_string()
                                    } else {
                                        "Removable secondary device".to_string()
                                    }),
                                    active: false,
                                }
                            }
                        }
                        UiCardFooter {
                            extra_class: None,
                            div { class: "flex h-full w-full items-end justify-end gap-2 overflow-x-auto",
                                UiButton {
                                    id: Some(
                                        ControlId::SettingsAddDeviceButton
                                            .web_dom_id()
                                            .required_dom_id("ControlId::SettingsAddDeviceButton must define a web DOM id")
                                            .to_string(),
                                    ),
                                    label: "Add Device".to_string(),
                                    variant: ButtonVariant::Primary,
                                    onclick: {
                                        let controller = controller.clone();
                                        move |_| {
                                            controller.set_settings_section(SettingsSection::Devices);
                                            controller.send_action_keys("a");
                                            render_tick.set(render_tick() + 1);
                                        }
                                    }
                                }
                                UiButton {
                                    id: Some(
                                        ControlId::SettingsImportDeviceCodeButton
                                            .web_dom_id()
                                            .required_dom_id("ControlId::SettingsImportDeviceCodeButton must define a web DOM id")
                                            .to_string(),
                                    ),
                                    label: "Import Code".to_string(),
                                    variant: ButtonVariant::Secondary,
                                    onclick: {
                                        let controller = controller.clone();
                                        move |_| {
                                            controller.set_settings_section(SettingsSection::Devices);
                                            controller.send_action_keys("i");
                                            render_tick.set(render_tick() + 1);
                                        }
                                    }
                                }
                                UiButton {
                                    id: Some(
                                        ControlId::SettingsRemoveDeviceButton
                                            .web_dom_id()
                                            .required_dom_id("ControlId::SettingsRemoveDeviceButton must define a web DOM id")
                                            .to_string(),
                                    ),
                                    label: "Remove Device".to_string(),
                                    variant: ButtonVariant::Secondary,
                                    onclick: {
                                        let controller = controller.clone();
                                        move |_| {
                                            controller.set_settings_section(SettingsSection::Devices);
                                            controller.send_action_keys("r");
                                            render_tick.set(render_tick() + 1);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if matches!(model.settings_section, SettingsSection::Authority) {
                    UiCardBody {
                        extra_class: Some("gap-2".to_string()),
                        div {
                            class: "aura-list flex flex-col gap-2",
                            UiListItem {
                                label: format!("Authority ID: {}", runtime.authority_id),
                                secondary: Some("Scope: local authority".to_string()),
                                active: false,
                            }
                            for authority in runtime.authorities.clone() {
                                UiListButton {
                                    label: if authority.is_current {
                                        format!("{} (current)", authority.label)
                                    } else {
                                        authority.label.clone()
                                    },
                                    active: authority.is_current,
                                    onclick: {
                                        let controller = controller.clone();
                                        let authority_id = authority.id;
                                        move |_| {
                                            if authority.is_current {
                                                return;
                                            }
                                            let _ = controller.request_authority_switch(authority_id);
                                        }
                                    }
                                }
                            }
                            UiListItem {
                                label: "Multifactor".to_string(),
                                secondary: Some(format!("Policy: {}", runtime.mfa_policy)),
                                active: false,
                            }
                        }
                        UiCardFooter {
                            extra_class: None,
                            div { class: "flex h-full w-full items-end justify-end gap-2 overflow-x-auto",
                                UiButton {
                                    id: Some(
                                        ControlId::SettingsSwitchAuthorityButton
                                            .web_dom_id()
                                            .required_dom_id(
                                                "ControlId::SettingsSwitchAuthorityButton must define a web DOM id"
                                            )
                                            .to_string(),
                                    ),
                                    label: "Switch Authority".to_string(),
                                    variant: ButtonVariant::Primary,
                                    onclick: {
                                        let controller = controller.clone();
                                        move |_| {
                                            controller.set_settings_section(SettingsSection::Authority);
                                            controller.send_action_keys("s");
                                            render_tick.set(render_tick() + 1);
                                        }
                                    }
                                }
                                UiButton {
                                    id: Some(
                                        ControlId::SettingsConfigureMfaButton
                                            .web_dom_id()
                                            .required_dom_id(
                                                "ControlId::SettingsConfigureMfaButton must define a web DOM id"
                                            )
                                            .to_string(),
                                    ),
                                    label: "Configure MFA".to_string(),
                                    variant: ButtonVariant::Secondary,
                                    onclick: {
                                        let controller = controller;
                                        move |_| {
                                            controller.set_settings_section(SettingsSection::Authority);
                                            controller.send_action_keys("m");
                                            render_tick.set(render_tick() + 1);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if matches!(model.settings_section, SettingsSection::Appearance) {
                    UiCardBody {
                        extra_class: Some("gap-2".to_string()),
                        div {
                            class: "aura-list flex flex-col gap-2",
                            UiListItem {
                                label: format!(
                                    "Color mode: {}",
                                    match resolved_scheme {
                                        ColorScheme::Light => "Light",
                                        _ => "Dark",
                                    }
                                ),
                                secondary: Some("Switch the current web theme".to_string()),
                                active: false,
                            }
                        }
                        UiCardFooter {
                            extra_class: None,
                            div { class: "flex h-full w-full items-end justify-end gap-2 overflow-x-auto",
                                UiButton {
                                    id: Some(
                                        ControlId::SettingsToggleThemeButton
                                            .web_dom_id()
                                            .required_dom_id(
                                                "ControlId::SettingsToggleThemeButton must define a web DOM id"
                                            )
                                            .to_string(),
                                    ),
                                    label: match resolved_scheme {
                                        ColorScheme::Light => "Switch to Dark".to_string(),
                                        _ => "Switch to Light".to_string(),
                                    },
                                    variant: ButtonVariant::Primary,
                                    width_class: Some("w-[9.5rem]".to_string()),
                                    onclick: move |_| {
                                        theme.toggle_color_scheme();
                                        render_tick.set(render_tick() + 1);
                                    }
                                }
                            }
                        }
                    }
                }
                if matches!(model.settings_section, SettingsSection::Info) {
                    UiCardBody {
                        extra_class: Some("gap-2".to_string()),
                        div {
                            class: "aura-list flex flex-col gap-2",
                            UiListItem {
                                label: "Storage: IndexedDB".to_string(),
                                secondary: Some(
                                    "Browser-backed local persistence for this device.".to_string()
                                ),
                                active: false,
                            }
                        }
                    }
                }
            }
        }
    }
}
