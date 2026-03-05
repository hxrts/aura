use crate::model::{ModalState, NeighborhoodMode, UiController, UiModel, UiScreen};
use dioxus::events::KeyboardData;
use dioxus::prelude::*;
use std::sync::Arc;

const SETTINGS_ROWS: [&str; 5] = [
    "Profile",
    "Guardian Threshold",
    "Request Recovery",
    "Devices",
    "Authority",
];

const MIN_ROWS: usize = 20;

#[derive(Clone)]
struct PanelRow {
    left: String,
    center: String,
    right: String,
}

#[component]
pub fn AuraUiRoot(controller: Arc<UiController>) -> Element {
    let mut render_tick = use_signal(|| 0_u64);
    let _render_tick_value = render_tick();

    let model = controller.ui_model();
    let Some(model) = model else {
        return rsx! {
            main {
                class: "min-h-screen bg-slate-950 text-slate-100 grid place-items-center",
                p { "UI state unavailable" }
            }
        };
    };

    let tabs = screen_tabs(model.screen);
    let rows = build_panel_rows(&model);
    let modal_title = active_modal_title(&model);
    let line_count = rows.len();

    rsx! {
        main {
            class: "relative min-h-screen p-3 sm:p-6 grid place-items-center outline-none text-slate-100 font-mono bg-slate-950",
            tabindex: 0,
            autofocus: true,
            onmounted: move |mounted| {
                spawn(async move {
                    let _ = mounted.data().set_focus(true).await;
                });
            },
            onkeydown: move |event| {
                if handle_keydown(controller.as_ref(), event.data().as_ref()) {
                    event.prevent_default();
                    render_tick.set(render_tick() + 1);
                }
            },
            div {
                class: "pointer-events-none absolute inset-0 opacity-80",
                div {
                    class: "absolute left-[-8rem] top-[-7rem] h-[28rem] w-[28rem] rounded-full blur-3xl bg-cyan-500/20"
                }
                div {
                    class: "absolute right-[-9rem] top-[-8rem] h-[32rem] w-[32rem] rounded-full blur-3xl bg-emerald-500/20"
                }
            }
            section {
                class: "relative w-full max-w-[1300px] min-h-[86vh] sm:min-h-[82vh] overflow-hidden rounded-2xl border border-slate-600/40 bg-slate-950/80 shadow-[0_24px_72px_rgba(0,0,0,0.58)] backdrop-blur",
                header {
                    class: "px-4 py-3 border-b border-slate-600/40 flex items-center justify-between gap-3 bg-gradient-to-b from-slate-900 to-slate-900/40",
                    h1 { class: "m-0 text-[0.96rem] tracking-[0.09em] uppercase", "Aura Runtime Shell" }
                    span { class: "text-cyan-300 text-xs tracking-[0.04em] uppercase border border-cyan-400/30 bg-slate-900/80 rounded-full px-2.5 py-1 whitespace-nowrap", "dioxus 0.7 | aura-ui" }
                }
                div {
                    class: "p-3 sm:p-4 overflow-hidden",
                    div {
                        class: "w-full h-full min-h-[60vh] max-h-[72vh] sm:max-h-[74vh] overflow-auto rounded-xl border border-slate-500/30 bg-slate-950 text-blue-100",
                        div {
                            class: "px-3 py-2.5 border-b border-slate-700/50 bg-slate-900/40 flex flex-wrap gap-2",
                            for (label, active) in tabs {
                                {
                                    let cls = if active {
                                        "inline-flex items-center rounded-md border border-cyan-400/40 bg-cyan-500/15 px-2 py-0.5 text-[0.68rem] uppercase tracking-[0.08em] text-cyan-200"
                                    } else {
                                        "inline-flex items-center rounded-md border border-slate-600/50 bg-slate-900/50 px-2 py-0.5 text-[0.68rem] uppercase tracking-[0.08em] text-slate-300"
                                    };
                                    rsx! { span { class: "{cls}", "{label}" } }
                                }
                            }
                        }
                        if let Some(title) = modal_title {
                            div {
                                class: "px-3 py-1.5 border-b border-amber-500/40 bg-amber-500/10 text-amber-200 text-xs tracking-[0.04em] uppercase",
                                "Modal: {title}"
                            }
                        }
                        div {
                            class: "text-[0.74rem] sm:text-[0.84rem] leading-[1.35]",
                            for row in rows {
                                div {
                                    class: "grid grid-cols-3 border-b border-slate-800/80",
                                    div { class: "px-3 py-1.5 border-r border-slate-800/70 whitespace-pre-wrap break-words", "{row.left}" }
                                    div { class: "px-3 py-1.5 border-r border-slate-800/70 whitespace-pre-wrap break-words", "{row.center}" }
                                    div { class: "px-3 py-1.5 whitespace-pre-wrap break-words", "{row.right}" }
                                }
                            }
                        }
                        if let Some(toast) = &model.toast {
                            div {
                                class: "px-3 py-1.5 border-t border-emerald-500/30 bg-emerald-950/20 text-emerald-200 whitespace-pre-wrap break-words",
                                "{toast.icon} {toast.message} [Esc] dismiss"
                            }
                        }
                    }
                }
                footer {
                    class: "border-t border-slate-600/40 text-slate-400 text-xs tracking-[0.02em] flex justify-between gap-3 px-4 py-2.5 flex-wrap",
                    span { class: "text-slate-300", "Keys: 1-5 tab shift+tab arrows enter esc ? i n a c d r" }
                    span { "rows: {line_count} | toast: {model.toast.is_some()}" }
                }
            }
        }
    }
}

fn screen_tabs(active: UiScreen) -> Vec<(&'static str, bool)> {
    [
        ("Neighborhood", active == UiScreen::Neighborhood),
        ("Chat", active == UiScreen::Chat),
        ("Contacts", active == UiScreen::Contacts),
        ("Notifications", active == UiScreen::Notifications),
        ("Settings", active == UiScreen::Settings),
    ]
    .to_vec()
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

fn build_panel_rows(model: &UiModel) -> Vec<PanelRow> {
    let mut rows = match model.screen {
        UiScreen::Neighborhood => neighborhood_rows(model),
        UiScreen::Chat => chat_rows(model),
        UiScreen::Contacts => contacts_rows(model),
        UiScreen::Notifications => notifications_rows(model),
        UiScreen::Settings => settings_rows(model),
    };

    if rows.len() < MIN_ROWS {
        rows.extend((rows.len()..MIN_ROWS).map(|_| PanelRow {
            left: String::new(),
            center: String::new(),
            right: String::new(),
        }));
    }
    rows
}

fn neighborhood_rows(model: &UiModel) -> Vec<PanelRow> {
    let mut rows = Vec::new();
    let mode_label = match model.neighborhood_mode {
        NeighborhoodMode::Map => "Map",
        NeighborhoodMode::Detail => "Details",
    };

    rows.push(PanelRow {
        left: "Neighborhood".to_string(),
        center: mode_label.to_string(),
        right: "Welcome to Aura".to_string(),
    });
    rows.push(PanelRow {
        left: "➤ Homes".to_string(),
        center: String::new(),
        right: model
            .selected_home
            .as_ref()
            .map(|home| format!("Selected home: {home}"))
            .unwrap_or_else(|| "Selected home: none".to_string()),
    });
    rows.push(PanelRow {
        left: format!("Can enter: {}", model.access_depth.label()),
        center: String::new(),
        right: format!("Authority: {} (local)", model.authority_id),
    });
    rows.push(PanelRow {
        left: "Members & Participants".to_string(),
        center: String::new(),
        right: String::new(),
    });
    rows.push(PanelRow {
        left: "Member".to_string(),
        center: String::new(),
        right: String::new(),
    });
    rows.push(PanelRow {
        left: String::new(),
        center: String::new(),
        right: format!("Access: {}", model.access_depth.label()),
    });
    rows.push(PanelRow {
        left: String::new(),
        center: String::new(),
        right: format!("{} M:Off P:0", model.access_depth.compact()),
    });
    rows.push(PanelRow {
        left: String::new(),
        center: String::new(),
        right: model.access_depth.compact().to_string(),
    });
    rows
}

fn chat_rows(model: &UiModel) -> Vec<PanelRow> {
    let mut rows = Vec::new();
    let channel = model.selected_channel_name().unwrap_or("general");

    rows.push(PanelRow {
        left: "Channels".to_string(),
        center: format!("Channel: #{channel}"),
        right: format!("Topic: {}", model.selected_channel_topic()),
    });

    for channel in &model.channels {
        let prefix = if channel.selected { "➤ " } else { "" };
        rows.push(PanelRow {
            left: format!("{prefix}# {}", channel.name),
            center: String::new(),
            right: String::new(),
        });
    }

    if model.messages.is_empty() {
        rows.push(PanelRow {
            left: String::new(),
            center: String::new(),
            right: "No messages yet".to_string(),
        });
    } else {
        for message in model.messages.iter().take(12) {
            rows.push(PanelRow {
                left: String::new(),
                center: String::new(),
                right: message.clone(),
            });
        }
    }

    let mode = if model.input_mode { "insert" } else { "normal" };
    rows.push(PanelRow {
        left: format!("mode: {mode}"),
        center: if model.input_mode {
            model.input_buffer.clone()
        } else {
            String::new()
        },
        right: String::new(),
    });
    rows
}

fn contacts_rows(model: &UiModel) -> Vec<PanelRow> {
    let mut rows = Vec::new();
    rows.push(PanelRow {
        left: format!("Contacts ({})", model.contacts.len()),
        center: String::new(),
        right: if model.contact_details {
            "Details".to_string()
        } else {
            "Select a contact".to_string()
        },
    });

    for (idx, contact) in model.contacts.iter().enumerate() {
        let prefix = if contact.selected { "➤ " } else { "" };
        let details = if model.contact_details && model.selected_contact_index == idx {
            format!("Nickname: {}", contact.name)
        } else {
            String::new()
        };
        rows.push(PanelRow {
            left: format!("{prefix}○ {}", contact.name),
            center: String::new(),
            right: details,
        });
    }

    rows.push(PanelRow {
        left: format!("Last scan: {}", model.last_scan),
        center: String::new(),
        right: String::new(),
    });
    rows
}

fn notifications_rows(model: &UiModel) -> Vec<PanelRow> {
    let mut rows = Vec::new();
    rows.push(PanelRow {
        left: "Notifications".to_string(),
        center: String::new(),
        right: "No notifications".to_string(),
    });
    rows.push(PanelRow {
        left: String::new(),
        center: String::new(),
        right: "Select a notification".to_string(),
    });
    for entry in model.notifications.iter().take(16) {
        rows.push(PanelRow {
            left: String::new(),
            center: String::new(),
            right: entry.clone(),
        });
    }
    rows
}

fn settings_rows(model: &UiModel) -> Vec<PanelRow> {
    let mut rows = Vec::new();
    rows.push(PanelRow {
        left: "Settings".to_string(),
        center: String::new(),
        right: "Storage: IndexedDB".to_string(),
    });

    for (idx, section) in SETTINGS_ROWS.iter().enumerate() {
        let prefix = if idx == model.settings_index { "➤ " } else { "" };
        let right = if *section == "Profile" {
            format!("Nickname: {}", model.profile_nickname)
        } else if *section == "Authority" {
            format!("Authority: {} (local)", model.authority_id)
        } else {
            String::new()
        };
        rows.push(PanelRow {
            left: format!("{prefix}{section}"),
            center: String::new(),
            right,
        });
    }

    rows
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
