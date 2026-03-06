//! Dioxus-based web UI application root and screen components.
//!
//! Provides the main application shell, screen routing, keyboard handling,
//! and toast notifications for the Aura web interface.

use crate::components::{
    ButtonVariant, ModalView, PillTone, UiButton, UiCard, UiFooter, UiListItem, UiModal, UiPill,
};
use crate::model::{ModalState, NeighborhoodMode, UiController, UiModel, UiScreen};
use dioxus::events::KeyboardData;
use dioxus::prelude::*;
use dioxus_shadcn::components::button::{
    Button as ShadButton, ButtonSize as ShadButtonSize, ButtonVariant as ShadButtonVariant,
};
use dioxus_shadcn::components::empty::{
    Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyTitle,
};
use dioxus_shadcn::components::scroll_area::{ScrollArea, ScrollAreaViewport};
use dioxus_shadcn::components::toast::{use_toast, ToastOptions, ToastProvider};
use dioxus_shadcn::theme::{themes, use_theme, ColorScheme, ThemeProvider};
use std::sync::Arc;
use std::time::Duration;

const SETTINGS_ROWS: [&str; 5] = [
    "Profile",
    "Guardian Threshold",
    "Request Recovery",
    "Devices",
    "Authority",
];

#[component]
pub fn AuraUiRoot(controller: Arc<UiController>) -> Element {
    rsx! {
        ThemeProvider {
            theme: themes::neutral(),
            color_scheme: ColorScheme::Dark,
            ToastProvider {
                default_duration: Duration::from_secs(5),
                max_toasts: 8,
                AuraUiShell { controller }
            }
        }
    }
}

#[component]
fn AuraUiShell(controller: Arc<UiController>) -> Element {
    let mut render_tick = use_signal(|| 0_u64);
    let _render_tick_value = render_tick();
    let mut last_toast_key = use_signal(|| None::<String>);
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

    let modal = modal_view(&model);
    let toast_snapshot = model.toast.clone();

    use_effect(move || {
        let next_key = toast_snapshot
            .as_ref()
            .map(|toast| format!("{}::{}::{}", model.toast_key, toast.icon, toast.message));

        if last_toast_key() == next_key {
            return;
        }

        if let Some(toast) = &toast_snapshot {
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
    let resolved_scheme = theme.resolved_scheme();
    let mut theme_for_toggle = theme.clone();

    rsx! {
        main {
            class: "relative min-h-screen bg-background text-foreground font-mono outline-none flex flex-col",
            tabindex: 0,
            autofocus: true,
            onmounted: move |mounted| {
                spawn(async move {
                    let _ = mounted.data().set_focus(true).await;
                });
            },
            onkeydown: move |event| {
                if should_skip_global_key(controller.as_ref(), event.data().as_ref()) {
                    return;
                }
                if handle_keydown(controller.as_ref(), event.data().as_ref()) {
                    event.prevent_default();
                    render_tick.set(render_tick() + 1);
                }
            },
            nav {
                class: "border-b border-border bg-background/95 backdrop-blur supports-[backdrop-filter]:bg-background/80",
                div {
                    class: "flex flex-wrap items-center justify-between gap-3 px-4 py-3 sm:px-6",
                    div {
                        class: "flex flex-wrap items-center gap-2",
                        span { class: "mr-2 text-xs font-bold uppercase tracking-[0.12em] text-white", "AURA" }
                        for (screen, label, is_active) in screen_tabs(model.screen) {
                            button {
                                r#type: "button",
                                class: nav_tab_class(is_active),
                                onclick: {
                                    let controller = controller.clone();
                                    move |_| {
                                        controller.set_screen(screen);
                                        render_tick.set(render_tick() + 1);
                                    }
                                },
                                "{label}"
                            }
                        }
                    }
                    ShadButton {
                        variant: ShadButtonVariant::Ghost,
                        size: ShadButtonSize::Icon,
                        is_icon_button: true,
                        aria_label: Some(match resolved_scheme {
                            ColorScheme::Dark => "Switch to light mode".to_string(),
                            _ => "Switch to dark mode".to_string(),
                        }),
                        on_click: move |_| theme_for_toggle.toggle_color_scheme(),
                        span { class: "text-lg leading-none", "◑" }
                    }
                }
            }
            div {
                class: "flex-1 min-h-0 overflow-y-auto px-4 py-4 sm:px-6 sm:py-5",
                {render_screen_content(&model, controller.clone(), render_tick)}
            }

            if let Some(modal) = modal {
                UiModal {
                    modal,
                    on_cancel: {
                        let controller = controller.clone();
                        move |_| {
                            controller.send_key_named("esc", 1);
                            render_tick.set(render_tick() + 1);
                        }
                    },
                    on_confirm: {
                        let controller = controller.clone();
                        move |_| {
                            controller.send_key_named("enter", 1);
                            render_tick.set(render_tick() + 1);
                        }
                    },
                    on_input_change: {
                        let controller = controller.clone();
                        move |value: String| {
                            controller.set_modal_buffer(&value);
                            render_tick.set(render_tick() + 1);
                        }
                    }
                }
            }

            UiFooter {
                left: String::new(),
                right: format!(
                    "screen: {} | authority: {} | toast: {}",
                    screen_label(model.screen),
                    model.authority_id,
                    model.toast.is_some()
                ),
            }
        }
    }
}

fn neighborhood_screen(
    model: &UiModel,
    controller: Arc<UiController>,
    mut render_tick: Signal<u64>,
) -> Element {
    let mode = match model.neighborhood_mode {
        NeighborhoodMode::Map => "Map",
        NeighborhoodMode::Detail => "Details",
    };
    let selected_home = model
        .selected_home
        .clone()
        .unwrap_or_else(|| "none".to_string());
    let access_label = model.access_depth.label().to_string();
    let access_tone = match model.access_depth.label() {
        "Full" => PillTone::Success,
        "Partial" => PillTone::Info,
        _ => PillTone::Neutral,
    };

    rsx! {
        div {
            class: "grid gap-3 lg:grid-cols-12",
            UiCard {
                title: "Neighborhood".to_string(),
                subtitle: Some(format!("Mode: {mode}")),
                extra_class: Some("lg:col-span-3".to_string()),
                div { class: "flex flex-wrap gap-2",
                    UiPill { label: format!("Access: {access_label}"), tone: access_tone }
                    UiPill { label: "Scope: local".to_string(), tone: PillTone::Neutral }
                }
                UiListItem {
                    label: format!("Selected Home: {selected_home}"),
                    secondary: Some("Press n to create a home".to_string()),
                    active: true,
                }
                UiListItem {
                    label: "Members & Participants".to_string(),
                    secondary: Some("Member".to_string()),
                    active: false,
                }
                div { class: "flex gap-2 pt-1",
                    UiButton {
                        label: "New Home".to_string(),
                        variant: ButtonVariant::Primary,
                        on_click: move |_| {
                            controller.send_action_keys("n");
                            render_tick.set(render_tick() + 1);
                        }
                    }
                }
            }

            UiCard {
                title: "Map".to_string(),
                subtitle: Some("Topology overview".to_string()),
                extra_class: Some("lg:col-span-5".to_string()),
                div {
                    class: "rounded-lg border border-dashed border-border bg-background p-4 min-h-[15rem]",
                    p { class: "m-0 text-sm text-foreground", "Neighborhood map rendering area" }
                    p { class: "m-0 mt-2 text-xs text-muted-foreground", "Map mode mirrors the runtime state; interaction remains keyboard-first." }
                }
            }

            UiCard {
                title: "Authority".to_string(),
                subtitle: Some("Current local identity".to_string()),
                extra_class: Some("lg:col-span-4".to_string()),
                UiListItem {
                    label: model.authority_id.clone(),
                    secondary: Some("local".to_string()),
                    active: true,
                }
                UiListItem {
                    label: format!("Depth: {}", model.access_depth.compact()),
                    secondary: Some("M:Off P:0".to_string()),
                    active: false,
                }
            }
        }
    }
}

fn chat_screen(
    model: &UiModel,
    controller: Arc<UiController>,
    mut render_tick: Signal<u64>,
) -> Element {
    let active_channel = model.selected_channel_name().unwrap_or("general");
    let topic = model.selected_channel_topic().to_string();
    let mode = if model.input_mode { "insert" } else { "normal" };
    let new_group_controller = controller.clone();
    let start_typing_controller = controller.clone();

    rsx! {
        div {
            class: "grid gap-3 lg:grid-cols-12",
            UiCard {
                title: "Channels".to_string(),
                subtitle: Some(format!("Current: #{active_channel}")),
                extra_class: Some("lg:col-span-3".to_string()),
                for channel in &model.channels {
                    UiListItem {
                        label: format!("# {}", channel.name),
                        secondary: if channel.topic.is_empty() { None } else { Some(channel.topic.clone()) },
                        active: channel.selected,
                    }
                }
                div { class: "flex gap-2 pt-1",
                    UiButton {
                        label: "New Group".to_string(),
                        variant: ButtonVariant::Primary,
                        on_click: move |_| {
                            new_group_controller.send_action_keys("n");
                            render_tick.set(render_tick() + 1);
                        }
                    }
                }
            }

            UiCard {
                title: "Conversation".to_string(),
                subtitle: Some(format!("Topic: {topic}")),
                extra_class: Some("lg:col-span-9".to_string()),
                ScrollArea {
                    max_height: Some("22rem".to_string()),
                    class: Some("pr-1".to_string()),
                    ScrollAreaViewport {
                        class: Some("space-y-2".to_string()),
                        if model.messages.is_empty() {
                            Empty {
                                class: Some("min-h-[14rem] border-border bg-background".to_string()),
                                EmptyHeader {
                                    EmptyTitle { "No messages yet" }
                                    EmptyDescription { "Send one from input mode." }
                                }
                                EmptyContent {
                                    UiButton {
                                        label: "Start Typing".to_string(),
                                        variant: ButtonVariant::Primary,
                                        on_click: move |_| {
                                            start_typing_controller.send_action_keys("i");
                                            render_tick.set(render_tick() + 1);
                                        }
                                    }
                                }
                            }
                        } else {
                            for message in model.messages.iter().take(16) {
                                UiListItem {
                                    label: message.clone(),
                                    secondary: None,
                                    active: false,
                                }
                            }
                        }
                    }
                }
                div {
                    class: "mt-2 rounded-lg border border-border bg-background px-3 py-2",
                    p { class: "m-0 text-xs uppercase tracking-[0.08em] text-muted-foreground", "Composer" }
                    p { class: "m-0 mt-1 text-sm text-foreground", "Mode: {mode}" }
                    p { class: "m-0 mt-1 text-sm text-muted-foreground whitespace-pre-wrap break-words", "{model.input_buffer}" }
                }
            }
        }
    }
}

fn contacts_screen(
    model: &UiModel,
    controller: Arc<UiController>,
    mut render_tick: Signal<u64>,
) -> Element {
    let selected_name = model
        .selected_contact_name()
        .map_or_else(|| "none".to_string(), |name| name.to_string());
    let empty_invite_controller = controller.clone();
    let invite_controller = controller.clone();

    rsx! {
        div {
            class: "grid gap-3 lg:grid-cols-12",
            UiCard {
                title: format!("Contacts ({})", model.contacts.len()),
                subtitle: Some("Social graph neighbors".to_string()),
                extra_class: Some("lg:col-span-4".to_string()),
                if model.contacts.is_empty() {
                    Empty {
                        class: Some("border-border bg-background".to_string()),
                        EmptyHeader {
                            EmptyTitle { "No contacts yet" }
                            EmptyDescription { "Use the invitation flow to add contacts." }
                        }
                        EmptyContent {
                            UiButton {
                                label: "Invite".to_string(),
                                variant: ButtonVariant::Primary,
                                on_click: move |_| {
                                    empty_invite_controller.send_action_keys("n");
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                        }
                    }
                } else {
                    for (idx, contact) in model.contacts.iter().enumerate() {
                        UiListItem {
                            label: contact.name.clone(),
                            secondary: if model.contact_details && model.selected_contact_index == idx {
                                Some(format!("Nickname: {}", contact.name))
                            } else {
                                None
                            },
                            active: contact.selected,
                        }
                    }
                }
                div { class: "flex gap-2 pt-1",
                    UiButton {
                        label: "Invite".to_string(),
                        variant: ButtonVariant::Primary,
                        on_click: move |_| {
                            invite_controller.send_action_keys("n");
                            render_tick.set(render_tick() + 1);
                        }
                    }
                }
            }

            UiCard {
                title: "Details".to_string(),
                subtitle: Some(format!("Selected: {selected_name}")),
                extra_class: Some("lg:col-span-8".to_string()),
                UiListItem {
                    label: format!("Last scan: {}", model.last_scan),
                    secondary: Some("Contact metadata and trust state".to_string()),
                    active: false,
                }
            }
        }
    }
}

fn notifications_screen(
    model: &UiModel,
    _controller: Arc<UiController>,
    _render_tick: Signal<u64>,
) -> Element {
    rsx! {
        div {
            class: "grid gap-3",
            UiCard {
                title: "Notifications".to_string(),
                subtitle: Some("Runtime events".to_string()),
                extra_class: None,
                if model.notifications.is_empty() {
                    Empty {
                        class: Some("border-border bg-background".to_string()),
                        EmptyHeader {
                            EmptyTitle { "No notifications" }
                            EmptyDescription { "Runtime events will appear here." }
                        }
                    }
                } else {
                    ScrollArea {
                        max_height: Some("24rem".to_string()),
                        ScrollAreaViewport {
                            class: Some("space-y-2".to_string()),
                            for (idx, entry) in model.notifications.iter().enumerate().take(24) {
                                UiListItem {
                                    label: entry.clone(),
                                    secondary: None,
                                    active: idx == model.selected_notification_index,
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn settings_screen(
    model: &UiModel,
    controller: Arc<UiController>,
    mut render_tick: Signal<u64>,
) -> Element {
    rsx! {
        div {
            class: "grid gap-3 lg:grid-cols-12",
            UiCard {
                title: "Settings".to_string(),
                subtitle: Some("Storage: IndexedDB".to_string()),
                extra_class: Some("lg:col-span-4".to_string()),
                for (idx, section) in SETTINGS_ROWS.iter().enumerate() {
                    button {
                        r#type: "button",
                        class: if idx == model.settings_index {
                            "w-full text-left rounded-lg border border-primary/40 bg-primary/10 px-2.5 py-2 text-sm text-foreground"
                        } else {
                            "w-full text-left rounded-lg border border-border bg-background/60 px-2.5 py-2 text-sm text-muted-foreground hover:text-foreground"
                        },
                        onclick: {
                            let controller = controller.clone();
                            move |_| {
                                controller.set_settings_index(idx);
                                render_tick.set(render_tick() + 1);
                            }
                        },
                        "{section}"
                    }
                }
            }

            UiCard {
                title: settings_panel_title(model.settings_index),
                subtitle: Some(settings_panel_subtitle(model.settings_index)),
                extra_class: Some("lg:col-span-8".to_string()),
                if model.settings_index == 0 {
                    UiListItem {
                        label: format!("Nickname: {}", model.profile_nickname),
                        secondary: Some("Update display name for this authority".to_string()),
                        active: false,
                    }
                    UiListItem {
                        label: format!("Authority: {}", model.authority_id),
                        secondary: Some("local".to_string()),
                        active: false,
                    }
                    div {
                        class: "flex flex-wrap gap-2 pt-1",
                        UiButton {
                            label: "Edit Nickname".to_string(),
                            variant: ButtonVariant::Secondary,
                            on_click: {
                                let controller = controller.clone();
                                move |_| {
                                    controller.set_settings_index(0);
                                    controller.send_action_keys("e");
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                        }
                    }
                }
                if model.settings_index == 1 {
                    UiListItem {
                        label: "Guardian Setup".to_string(),
                        secondary: Some("Configure guardian threshold and policy".to_string()),
                        active: false,
                    }
                    UiListItem {
                        label: "Target threshold: 2 of N".to_string(),
                        secondary: Some("Adjust in ceremony flow".to_string()),
                        active: false,
                    }
                    div {
                        class: "flex flex-wrap gap-2 pt-1",
                        UiButton {
                            label: "Configure Threshold".to_string(),
                            variant: ButtonVariant::Secondary,
                            on_click: {
                                let controller = controller.clone();
                                move |_| {
                                    controller.set_settings_index(1);
                                    controller.send_action_keys("t");
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                        }
                    }
                }
                if model.settings_index == 2 {
                    UiListItem {
                        label: "Recovery request".to_string(),
                        secondary: Some("Start guardian-assisted recovery flow".to_string()),
                        active: false,
                    }
                    UiListItem {
                        label: "Last status: idle".to_string(),
                        secondary: Some("No active recovery session".to_string()),
                        active: false,
                    }
                    div {
                        class: "flex flex-wrap gap-2 pt-1",
                        UiButton {
                            label: "Request Recovery".to_string(),
                            variant: ButtonVariant::Secondary,
                            on_click: {
                                let controller = controller.clone();
                                move |_| {
                                    controller.set_settings_index(2);
                                    controller.send_action_keys("s");
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                        }
                    }
                }
                if model.settings_index == 3 {
                    UiListItem {
                        label: "Add device".to_string(),
                        secondary: Some("Start device enrollment flow".to_string()),
                        active: false,
                    }
                    UiListItem {
                        label: "Import enrollment code".to_string(),
                        secondary: Some("Import an existing enrollment code".to_string()),
                        active: false,
                    }
                    UiListItem {
                        label: "Remove current device".to_string(),
                        secondary: Some("Current device removal is blocked".to_string()),
                        active: false,
                    }
                    div {
                        class: "flex flex-wrap gap-2 pt-1",
                        UiButton {
                            label: "Add Device".to_string(),
                            variant: ButtonVariant::Secondary,
                            on_click: {
                                let controller = controller.clone();
                                move |_| {
                                    controller.set_settings_index(3);
                                    controller.send_action_keys("a");
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                        }
                        UiButton {
                            label: "Import Code".to_string(),
                            variant: ButtonVariant::Secondary,
                            on_click: {
                                let controller = controller.clone();
                                move |_| {
                                    controller.set_settings_index(3);
                                    controller.send_action_keys("i");
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                        }
                        UiButton {
                            label: "Remove Device".to_string(),
                            variant: ButtonVariant::Secondary,
                            on_click: {
                                let controller = controller.clone();
                                move |_| {
                                    controller.set_settings_index(3);
                                    controller.send_action_keys("r");
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                        }
                    }
                }
                if model.settings_index == 4 {
                    UiListItem {
                        label: format!("Authority ID: {}", model.authority_id),
                        secondary: Some("Scope: local authority".to_string()),
                        active: false,
                    }
                    UiListItem {
                        label: "Switch authority".to_string(),
                        secondary: Some("Requires more than one available authority".to_string()),
                        active: false,
                    }
                    UiListItem {
                        label: "Multifactor".to_string(),
                        secondary: Some("Configure MFA ceremony for this authority".to_string()),
                        active: false,
                    }
                    div {
                        class: "flex flex-wrap gap-2 pt-1",
                        UiButton {
                            label: "Switch Authority".to_string(),
                            variant: ButtonVariant::Secondary,
                            on_click: {
                                let controller = controller.clone();
                                move |_| {
                                    controller.set_settings_index(4);
                                    controller.send_action_keys("s");
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                        }
                        UiButton {
                            label: "Configure MFA".to_string(),
                            variant: ButtonVariant::Secondary,
                            on_click: {
                                let controller = controller.clone();
                                move |_| {
                                    controller.set_settings_index(4);
                                    controller.send_action_keys("m");
                                    render_tick.set(render_tick() + 1);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn settings_panel_title(index: usize) -> String {
    SETTINGS_ROWS
        .get(index)
        .copied()
        .unwrap_or("Settings")
        .to_string()
}

fn settings_panel_subtitle(index: usize) -> String {
    match index {
        0 => "Current values".to_string(),
        1 => "Guardian policy".to_string(),
        2 => "Recovery operations".to_string(),
        3 => "Device management".to_string(),
        4 => "Authority scope".to_string(),
        _ => "Settings details".to_string(),
    }
}

fn screen_tabs(active: UiScreen) -> Vec<(UiScreen, &'static str, bool)> {
    [
        (
            UiScreen::Neighborhood,
            "Neighborhood",
            active == UiScreen::Neighborhood,
        ),
        (UiScreen::Chat, "Chat", active == UiScreen::Chat),
        (UiScreen::Contacts, "Contacts", active == UiScreen::Contacts),
        (
            UiScreen::Notifications,
            "Notifications",
            active == UiScreen::Notifications,
        ),
        (UiScreen::Settings, "Settings", active == UiScreen::Settings),
    ]
    .to_vec()
}

fn screen_label(screen: UiScreen) -> &'static str {
    match screen {
        UiScreen::Neighborhood => "neighborhood",
        UiScreen::Chat => "chat",
        UiScreen::Contacts => "contacts",
        UiScreen::Notifications => "notifications",
        UiScreen::Settings => "settings",
    }
}

fn nav_tab_class(is_active: bool) -> &'static str {
    if is_active {
        "rounded-md bg-accent px-3 py-1.5 text-xs uppercase tracking-[0.08em] text-foreground"
    } else {
        "rounded-md px-3 py-1.5 text-xs uppercase tracking-[0.08em] text-muted-foreground hover:bg-accent hover:text-foreground"
    }
}

fn render_screen_content(
    model: &UiModel,
    controller: Arc<UiController>,
    render_tick: Signal<u64>,
) -> Element {
    match model.screen {
        UiScreen::Neighborhood => neighborhood_screen(model, controller, render_tick),
        UiScreen::Chat => chat_screen(model, controller, render_tick),
        UiScreen::Contacts => contacts_screen(model, controller, render_tick),
        UiScreen::Notifications => notifications_screen(model, controller, render_tick),
        UiScreen::Settings => settings_screen(model, controller, render_tick),
    }
}

fn active_modal_title(model: &UiModel) -> Option<String> {
    let modal = model.modal?;
    if !model.modal_hint.trim().is_empty() {
        return Some(model.modal_hint.trim().to_string());
    }
    Some(
        match modal {
            ModalState::Help => "Help",
            ModalState::CreateInvitation => "Invite Contacts",
            ModalState::AcceptInvitation => "Accept Invitation",
            ModalState::CreateHome => "Create New Home",
            ModalState::CreateChannel => "New Chat Group",
            ModalState::SetChannelTopic => "Set Channel Topic",
            ModalState::ChannelInfo => "Channel Info",
            ModalState::EditNickname => "Edit Nickname",
            ModalState::RemoveContact => "Remove Contact",
            ModalState::GuardianSetup => "Guardian Setup",
            ModalState::RequestRecovery => "Request Recovery",
            ModalState::AddDeviceStep1 => "Add Device",
            ModalState::ImportDeviceEnrollmentCode => "Import Device Enrollment Code",
            ModalState::AssignModerator => "Assign Moderator",
            ModalState::AccessOverride => "Access Override",
            ModalState::CapabilityConfig => "Home Capability Configuration",
        }
        .to_string(),
    )
}

fn modal_view(model: &UiModel) -> Option<ModalView> {
    let modal = model.modal?;
    let title = active_modal_title(model).unwrap_or_else(|| "Modal".to_string());
    let mut details = Vec::new();
    let mut keybind_rows = Vec::new();
    let mut input_label = None;

    match modal {
        ModalState::Help => {
            let (help_details, help_keybind_rows) = help_modal_content(model.screen);
            details = help_details;
            keybind_rows = help_keybind_rows;
        }
        ModalState::CreateInvitation => {
            details.push("Create an invitation code for a contact.".to_string());
            details.push("Press Enter to generate and copy the code.".to_string());
        }
        ModalState::AcceptInvitation => {
            details.push("Paste an invitation code, then press Enter.".to_string());
            input_label = Some("Invitation Code".to_string());
        }
        ModalState::CreateHome => {
            details.push("Enter a new home name and press Enter.".to_string());
            input_label = Some("Home Name".to_string());
        }
        ModalState::CreateChannel => {
            details.push("Enter a new channel name and press Enter.".to_string());
            input_label = Some("Channel Name".to_string());
        }
        ModalState::SetChannelTopic => {
            details.push("Set a topic for the selected channel.".to_string());
            input_label = Some("Channel Topic".to_string());
        }
        ModalState::ChannelInfo => {
            details.push("Channel details view.".to_string());
        }
        ModalState::EditNickname => {
            details.push("Update the selected nickname and press Enter.".to_string());
            input_label = Some("Nickname".to_string());
        }
        ModalState::RemoveContact => {
            details.push("Remove the selected contact from this authority.".to_string());
            details.push("Press Enter to confirm.".to_string());
        }
        ModalState::GuardianSetup => {
            details.push("Guardian setup wizard".to_string());
            details.push("1. Select guardians".to_string());
            details.push("2. Configure threshold".to_string());
            details.push("3. Confirm ceremony".to_string());
        }
        ModalState::RequestRecovery => {
            details.push("Request guardian-assisted recovery for this authority.".to_string());
            details.push("Press Enter to notify your configured guardians.".to_string());
        }
        ModalState::AddDeviceStep1 => {
            details.push("Add Device Wizard".to_string());
            details.push("Step 1 of 3: Generate enrollment invitation".to_string());
            details.push("Press Enter to continue.".to_string());
        }
        ModalState::ImportDeviceEnrollmentCode => {
            details.push("Import a device enrollment code and press Enter.".to_string());
            input_label = Some("Enrollment Code".to_string());
        }
        ModalState::AssignModerator => {
            details.push("Assign moderator role for selected home context.".to_string());
            details.push("Only members can be designated as moderators.".to_string());
        }
        ModalState::AccessOverride => {
            details.push("Override access depth for selected authority.".to_string());
            details.push("Preview changes before applying.".to_string());
        }
        ModalState::CapabilityConfig => {
            details.push("Configure home capability profile.".to_string());
            details.push("Press Enter to apply policy changes.".to_string());
        }
    }

    let input_value = if modal_accepts_text(modal) {
        Some(model.modal_buffer.clone())
    } else {
        None
    };

    let enter_label = match modal {
        ModalState::Help | ModalState::ChannelInfo => "Close".to_string(),
        _ => "Confirm".to_string(),
    };

    Some(ModalView {
        title,
        details,
        keybind_rows,
        input_label,
        input_value,
        enter_label,
    })
}

fn help_modal_content(screen: UiScreen) -> (Vec<String>, Vec<(String, String)>) {
    let details = match screen {
        UiScreen::Neighborhood => vec![
            "Neighborhood reference".to_string(),
            "Browse homes, access depth, and neighborhood detail views.".to_string(),
        ],
        UiScreen::Chat => vec![
            "Chat reference".to_string(),
            "Navigate channels, compose messages, and manage channel metadata.".to_string(),
        ],
        UiScreen::Contacts => vec![
            "Contacts reference".to_string(),
            "Manage invitations, nicknames, guardians, and direct-message handoff.".to_string(),
        ],
        UiScreen::Notifications => vec![
            "Notifications reference".to_string(),
            "Review pending notices and move through the notification feed.".to_string(),
        ],
        UiScreen::Settings => vec![
            "Settings reference".to_string(),
            "Adjust profile, recovery, devices, and authority-level configuration.".to_string(),
        ],
    };

    let keybind_rows = match screen {
        UiScreen::Neighborhood => vec![
            ("1-5".to_string(), "Switch screens".to_string()),
            ("tab / shift+tab".to_string(), "Cycle screens".to_string()),
            ("enter".to_string(), "Toggle map/detail view".to_string()),
            ("a".to_string(), "Accept home invitation".to_string()),
            ("n".to_string(), "Create home".to_string()),
            ("d".to_string(), "Cycle access depth".to_string()),
            ("esc".to_string(), "Close modal / back out".to_string()),
        ],
        UiScreen::Chat => vec![
            ("1-5".to_string(), "Switch screens".to_string()),
            ("tab / shift+tab".to_string(), "Cycle screens".to_string()),
            (
                "up / down".to_string(),
                "Move channel selection".to_string(),
            ),
            ("i".to_string(), "Enter message input".to_string()),
            ("n".to_string(), "Create channel".to_string()),
            ("t".to_string(), "Set channel topic".to_string()),
            ("o".to_string(), "Open channel info".to_string()),
            ("esc".to_string(), "Close modal / exit input".to_string()),
        ],
        UiScreen::Contacts => vec![
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
            ("e".to_string(), "Edit nickname".to_string()),
            ("g".to_string(), "Configure guardians".to_string()),
            ("c".to_string(), "Open DM for selected contact".to_string()),
            ("r".to_string(), "Remove contact".to_string()),
        ],
        UiScreen::Notifications => vec![
            ("1-5".to_string(), "Switch screens".to_string()),
            ("tab / shift+tab".to_string(), "Cycle screens".to_string()),
            (
                "up / down".to_string(),
                "Move notification selection".to_string(),
            ),
            ("enter".to_string(), "No-op placeholder".to_string()),
            ("esc".to_string(), "Close modal".to_string()),
        ],
        UiScreen::Settings => vec![
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

fn modal_accepts_text(modal: ModalState) -> bool {
    matches!(
        modal,
        ModalState::CreateInvitation
            | ModalState::AcceptInvitation
            | ModalState::CreateHome
            | ModalState::CreateChannel
            | ModalState::SetChannelTopic
            | ModalState::EditNickname
            | ModalState::ImportDeviceEnrollmentCode
    )
}

fn handle_keydown(controller: &UiController, event: &KeyboardData) -> bool {
    match event.key() {
        Key::Enter => {
            controller.send_key_named("enter", 1);
            true
        }
        Key::Escape => {
            controller.send_key_named("esc", 1);
            true
        }
        Key::Tab => {
            if event.modifiers().contains(Modifiers::SHIFT) {
                controller.send_key_named("backtab", 1);
            } else {
                controller.send_key_named("tab", 1);
            }
            true
        }
        Key::ArrowUp => {
            controller.send_key_named("up", 1);
            true
        }
        Key::ArrowDown => {
            controller.send_key_named("down", 1);
            true
        }
        Key::ArrowLeft => {
            controller.send_key_named("left", 1);
            true
        }
        Key::ArrowRight => {
            controller.send_key_named("right", 1);
            true
        }
        Key::Backspace => {
            controller.send_key_named("backspace", 1);
            true
        }
        Key::Character(text) => {
            if text.is_empty() {
                return false;
            }
            controller.send_keys(&text);
            true
        }
        _ => false,
    }
}

fn should_skip_global_key(controller: &UiController, event: &KeyboardData) -> bool {
    let Some(model) = controller.ui_model() else {
        return false;
    };
    let Some(modal) = model.modal else {
        return false;
    };
    if !modal_accepts_text(modal) {
        return false;
    }
    !matches!(event.key(), Key::Enter | Key::Escape)
}
