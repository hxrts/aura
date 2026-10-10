use super::*;

mod chat;
mod contacts;
mod neighborhood;
pub(super) mod notification_actions;
mod notifications;
mod settings;

pub(super) use neighborhood::neighborhood_member_selection_key;

fn screen_tabs(active: ScreenId) -> Vec<(ScreenId, &'static str, bool)> {
    [
        (
            ScreenId::Neighborhood,
            "Neighborhood",
            active == ScreenId::Neighborhood,
        ),
        (ScreenId::Chat, "Chat", active == ScreenId::Chat),
        (ScreenId::Contacts, "Contacts", active == ScreenId::Contacts),
        (
            ScreenId::Notifications,
            "Notifications",
            active == ScreenId::Notifications,
        ),
        (ScreenId::Settings, "Settings", active == ScreenId::Settings),
    ]
    .to_vec()
}

pub(super) fn nav_tab_class(is_active: bool) -> &'static str {
    if is_active {
        "inline-flex h-8 items-center justify-center whitespace-nowrap rounded-sm bg-accent px-3 text-xs font-sans uppercase leading-none tracking-[0.08em] text-foreground"
    } else {
        "inline-flex h-8 items-center justify-center whitespace-nowrap rounded-sm px-3 text-xs font-sans uppercase leading-none tracking-[0.08em] text-muted-foreground hover:bg-accent hover:text-foreground"
    }
}

pub(super) fn nav_tabs(active: ScreenId) -> Vec<(ScreenId, &'static str, bool)> {
    screen_tabs(active)
}

pub(super) fn render_screen_content(
    model: &UiModel,
    neighborhood_runtime: &NeighborhoodRuntimeView,
    chat_runtime: &ChatRuntimeView,
    contacts_runtime: &ContactsRuntimeView,
    settings_runtime: &SettingsRuntimeView,
    notifications_runtime: &NotificationsRuntimeView,
    controller: Arc<UiController>,
    render_tick: Signal<u64>,
    theme: dioxus_shadcn::theme::ThemeContext,
    resolved_scheme: ColorScheme,
) -> Element {
    match model.screen {
        ScreenId::Onboarding => rsx! {
            crate::rendered_control::RenderedRegion {
                control_id: ControlId::Screen(aura_app::ui::contract::ScreenId::Onboarding),
                class: "w-full lg:h-full lg:min-h-0",
                {OnboardingScreen()}
            }
        },
        ScreenId::Neighborhood => rsx! {
            crate::rendered_control::RenderedRegion {
                control_id: ControlId::Screen(aura_app::ui::contract::ScreenId::Neighborhood),
                class: "w-full lg:h-full lg:min-h-0",
                {neighborhood::NeighborhoodScreen(
                    model,
                    neighborhood_runtime,
                    controller,
                    render_tick,
                )}
            }
        },
        ScreenId::Chat => rsx! {
            crate::rendered_control::RenderedRegion {
                control_id: ControlId::Screen(aura_app::ui::contract::ScreenId::Chat),
                class: "w-full lg:h-full lg:min-h-0",
                {chat::ChatScreen(model, chat_runtime, controller, render_tick)}
            }
        },
        ScreenId::Contacts => rsx! {
            crate::rendered_control::RenderedRegion {
                control_id: ControlId::Screen(aura_app::ui::contract::ScreenId::Contacts),
                class: "w-full lg:h-full lg:min-h-0",
                {contacts::ContactsScreen(model, contacts_runtime, controller, render_tick)}
            }
        },
        ScreenId::Notifications => rsx! {
            crate::rendered_control::RenderedRegion {
                control_id: ControlId::Screen(aura_app::ui::contract::ScreenId::Notifications),
                class: "w-full lg:h-full lg:min-h-0",
                {notifications::NotificationsScreen(
                    model,
                    notifications_runtime,
                    controller,
                    render_tick,
                )}
            }
        },
        ScreenId::Settings => rsx! {
            crate::rendered_control::RenderedRegion {
                control_id: ControlId::Screen(aura_app::ui::contract::ScreenId::Settings),
                class: "w-full lg:h-full lg:min-h-0",
                {settings::SettingsScreen(
                    model,
                    settings_runtime,
                    controller,
                    render_tick,
                    theme,
                    resolved_scheme,
                )}
            }
        },
    }
}

#[component]
fn OnboardingScreen() -> Element {
    rsx! {
        div {
            class: "w-full lg:h-full lg:min-h-0"
        }
    }
}
