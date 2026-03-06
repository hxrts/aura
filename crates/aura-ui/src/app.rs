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
use dioxus_shadcn::components::empty::{
    Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyTitle,
};
use dioxus_shadcn::components::scroll_area::{ScrollArea, ScrollAreaViewport};
use dioxus_shadcn::components::toast::{use_toast, ToastOptions, ToastProvider};
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
        ToastProvider {
            default_duration: Duration::from_secs(5),
            max_toasts: 8,
            AuraUiShell { controller }
        }
    }
}

#[component]
fn AuraUiShell(controller: Arc<UiController>) -> Element {
    let mut render_tick = use_signal(|| 0_u64);
    let _render_tick_value = render_tick();
    let mut last_toast_key = use_signal(|| None::<String>);
    let toasts = use_toast();

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
            .map(|toast| format!("{}::{}", toast.icon, toast.message));

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

    rsx! {
        main {
            class: "relative min-h-screen bg-background text-foreground font-mono p-3 sm:p-6 grid place-items-center outline-none",
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
            section {
                class: "relative w-full max-w-[1300px] min-h-[86vh] sm:min-h-[82vh] rounded-2xl border border-border bg-card shadow-2xl overflow-hidden",
                nav {
                    class: "border-b border-border bg-card/95 backdrop-blur px-3 py-2.5 sm:px-4",
                    div {
                        class: "flex flex-wrap items-center gap-2",
                        span { class: "mr-2 text-xs uppercase tracking-[0.12em] text-muted-foreground", "Aura" }
                        for (screen, label, is_active) in screen_tabs(model.screen) {
                            button {
                                r#type: "button",
                                class: if is_active {
                                    "rounded-md border border-primary/40 bg-primary/15 px-3 py-1.5 text-xs uppercase tracking-[0.08em] text-foreground"
                                } else {
                                    "rounded-md border border-transparent bg-transparent px-3 py-1.5 text-xs uppercase tracking-[0.08em] text-muted-foreground hover:border-border hover:text-foreground"
                                },
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
                }
                div {
                    class: "p-3 sm:p-4",
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
                    left: "Keys: 1-5 tab shift+tab arrows enter esc ? i n a c d r".to_string(),
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
                        label: "Help".to_string(),
                        variant: ButtonVariant::Secondary,
                        on_click: {
                            let controller = controller.clone();
                            move |_| {
                                controller.send_keys("?");
                                render_tick.set(render_tick() + 1);
                            }
                        }
                    }
                    UiButton {
                        label: "New Home".to_string(),
                        variant: ButtonVariant::Primary,
                        on_click: move |_| {
                            controller.send_keys("n");
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
                            new_group_controller.send_keys("n");
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
                                            start_typing_controller.send_keys("i");
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
                                    empty_invite_controller.send_keys("i");
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
                            invite_controller.send_keys("i");
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

fn settings_screen(model: &UiModel) -> Element {
    rsx! {
        div {
            class: "grid gap-3 lg:grid-cols-12",
            UiCard {
                title: "Settings".to_string(),
                subtitle: Some("Storage: IndexedDB".to_string()),
                extra_class: Some("lg:col-span-4".to_string()),
                for (idx, section) in SETTINGS_ROWS.iter().enumerate() {
                    UiListItem {
                        label: (*section).to_string(),
                        secondary: None,
                        active: idx == model.settings_index,
                    }
                }
            }

            UiCard {
                title: "Profile".to_string(),
                subtitle: Some("Current values".to_string()),
                extra_class: Some("lg:col-span-8".to_string()),
                UiListItem {
                    label: format!("Nickname: {}", model.profile_nickname),
                    secondary: None,
                    active: false,
                }
                UiListItem {
                    label: format!("Authority: {}", model.authority_id),
                    secondary: Some("local".to_string()),
                    active: false,
                }
            }
        }
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
        UiScreen::Settings => settings_screen(model),
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
    let mut input_label = None;

    match modal {
        ModalState::Help => {
            details.push("Use 1-5 or click tabs to switch screens.".to_string());
            details.push("Use Enter to confirm and Esc to cancel/close.".to_string());
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
        input_label,
        input_value,
        enter_label,
    })
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
