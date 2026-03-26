use super::*;

mod chat;
mod contacts;
mod neighborhood;
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

pub(super) fn nav_button_id(screen: ScreenId) -> &'static str {
    match screen {
        ScreenId::Onboarding => ControlId::OnboardingRoot
            .web_dom_id()
            .required_dom_id("OnboardingRoot must define a web DOM id"),
        ScreenId::Neighborhood => ControlId::NavNeighborhood
            .web_dom_id()
            .required_dom_id("NavNeighborhood must define a web DOM id"),
        ScreenId::Chat => ControlId::NavChat
            .web_dom_id()
            .required_dom_id("NavChat must define a web DOM id"),
        ScreenId::Contacts => ControlId::NavContacts
            .web_dom_id()
            .required_dom_id("NavContacts must define a web DOM id"),
        ScreenId::Notifications => ControlId::NavNotifications
            .web_dom_id()
            .required_dom_id("NavNotifications must define a web DOM id"),
        ScreenId::Settings => ControlId::NavSettings
            .web_dom_id()
            .required_dom_id("NavSettings must define a web DOM id"),
    }
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
            div {
                id: ControlId::Screen(aura_app::ui::contract::ScreenId::Onboarding)
                    .web_dom_id()
                    .required_dom_id("Screen(Onboarding) must define a web DOM id"),
                class: "w-full lg:h-full lg:min-h-0",
                {OnboardingScreen()}
            }
        },
        ScreenId::Neighborhood => rsx! {
            div {
                id: ControlId::Screen(aura_app::ui::contract::ScreenId::Neighborhood)
                    .web_dom_id()
                    .required_dom_id("Screen(Neighborhood) must define a web DOM id"),
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
            div {
                id: ControlId::Screen(aura_app::ui::contract::ScreenId::Chat)
                    .web_dom_id()
                    .required_dom_id("Screen(Chat) must define a web DOM id"),
                class: "w-full lg:h-full lg:min-h-0",
                {chat::ChatScreen(model, chat_runtime, controller, render_tick)}
            }
        },
        ScreenId::Contacts => rsx! {
            div {
                id: ControlId::Screen(aura_app::ui::contract::ScreenId::Contacts)
                    .web_dom_id()
                    .required_dom_id("Screen(Contacts) must define a web DOM id"),
                class: "w-full lg:h-full lg:min-h-0",
                {contacts::ContactsScreen(model, contacts_runtime, controller, render_tick)}
            }
        },
        ScreenId::Notifications => rsx! {
            div {
                id: ControlId::Screen(aura_app::ui::contract::ScreenId::Notifications)
                    .web_dom_id()
                    .required_dom_id("Screen(Notifications) must define a web DOM id"),
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
            div {
                id: ControlId::Screen(aura_app::ui::contract::ScreenId::Settings)
                    .web_dom_id()
                    .required_dom_id("Screen(Settings) must define a web DOM id"),
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
