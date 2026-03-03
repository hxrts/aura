//! Shared keybinding registry for footer hints and help content.

use crate::tui::screens::Screen;
use crate::tui::types::KeyHint;

/// One keyboard binding entry used by footer/help presenters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyBinding {
    /// Key shown to users (`Esc`, `Tab`, `j/k`, etc.).
    pub key: &'static str,
    /// Syntax shown in help details.
    pub syntax: &'static str,
    /// Short action label used in compact footer hints.
    pub footer_action: &'static str,
    /// Longer action description used in help modal.
    pub help_description: &'static str,
    /// Help section/category.
    pub category: &'static str,
    /// Whether this binding should appear in the compact footer hints.
    pub include_in_footer: bool,
}

impl KeyBinding {
    const fn new(
        key: &'static str,
        syntax: &'static str,
        footer_action: &'static str,
        help_description: &'static str,
        category: &'static str,
        include_in_footer: bool,
    ) -> Self {
        Self {
            key,
            syntax,
            footer_action,
            help_description,
            category,
            include_in_footer,
        }
    }

    fn to_hint(self) -> KeyHint {
        KeyHint::new(self.key, self.footer_action)
    }
}

const GLOBAL_BINDINGS: &[KeyBinding] = &[
    KeyBinding::new(
        "1-5",
        "1, 2, 3, 4, 5",
        "Screens",
        "Switch screens",
        "Navigation",
        false,
    ),
    KeyBinding::new(
        "↑↓←→",
        "↑↓←→",
        "Nav",
        "Move focus/lists",
        "Navigation",
        true,
    ),
    KeyBinding::new("Tab", "Tab", "Next", "Next screen", "Navigation", true),
    KeyBinding::new(
        "S-Tab",
        "Shift+Tab",
        "Prev",
        "Previous screen",
        "Navigation",
        false,
    ),
    KeyBinding::new("?", "?", "Help", "Show/hide help", "Navigation", true),
    KeyBinding::new("q", "q", "Quit", "Quit", "Navigation", true),
    KeyBinding::new(
        "Esc",
        "Esc",
        "Cancel",
        "Cancel/close modal/toast",
        "Navigation",
        false,
    ),
    KeyBinding::new(
        "y",
        "y",
        "Copy",
        "Copy error to clipboard",
        "Navigation",
        false,
    ),
    KeyBinding::new(
        "j/k",
        "j, k",
        "List",
        "Move down/up in lists",
        "Navigation",
        false,
    ),
    KeyBinding::new(
        "h/l",
        "h, l",
        "Panels",
        "Switch panels (left/right)",
        "Navigation",
        false,
    ),
];

const CHAT_BINDINGS: &[KeyBinding] = &[
    KeyBinding::new(
        "i",
        "i",
        "Insert",
        "Enter insert mode (type message)",
        "Chat",
        true,
    ),
    KeyBinding::new("n", "n", "New", "Create new channel", "Chat", true),
    KeyBinding::new("o", "o", "Info", "Open channel info", "Chat", true),
    KeyBinding::new("t", "t", "Topic", "Set channel topic", "Chat", true),
    KeyBinding::new("r", "r", "Retry", "Retry failed message", "Chat", true),
    KeyBinding::new(
        "Tab",
        "Tab",
        "Focus",
        "Switch between channels/messages/input",
        "Chat",
        false,
    ),
];

const CONTACTS_BINDINGS: &[KeyBinding] = &[
    KeyBinding::new("e", "e", "Edit", "Edit contact nickname", "Contacts", true),
    KeyBinding::new(
        "g",
        "g",
        "Guardian",
        "Open guardian setup",
        "Contacts",
        false,
    ),
    KeyBinding::new(
        "c",
        "c",
        "Chat",
        "Start chat with contact",
        "Contacts",
        true,
    ),
    KeyBinding::new(
        "a",
        "a",
        "Accept",
        "Accept invitation code",
        "Contacts",
        true,
    ),
    KeyBinding::new(
        "n",
        "n",
        "Invite",
        "Create invitation code",
        "Contacts",
        true,
    ),
    KeyBinding::new(
        "p",
        "p",
        "Peers",
        "Toggle LAN peers list",
        "Contacts",
        false,
    ),
    KeyBinding::new("d", "d", "Rescan", "Rescan LAN peers", "Contacts", true),
    KeyBinding::new("r", "r", "Remove", "Remove contact", "Contacts", false),
];

const NEIGHBORHOOD_BINDINGS: &[KeyBinding] = &[
    KeyBinding::new(
        "Enter",
        "Enter",
        "Enter",
        "Enter selected home",
        "Neighborhood",
        true,
    ),
    KeyBinding::new(
        "Esc",
        "Esc",
        "Map",
        "Return to map view",
        "Neighborhood",
        true,
    ),
    KeyBinding::new(
        "a",
        "a",
        "Accept",
        "Accept invitation code",
        "Neighborhood",
        true,
    ),
    KeyBinding::new(
        "d",
        "d",
        "Depth",
        "Cycle traversal depth",
        "Neighborhood",
        true,
    ),
    KeyBinding::new("n", "n", "New", "Create home", "Neighborhood", true),
    KeyBinding::new(
        "m",
        "m",
        "Neighborhood",
        "Create/select neighborhood",
        "Neighborhood",
        false,
    ),
    KeyBinding::new(
        "v",
        "v",
        "Join",
        "Add selected home as member",
        "Neighborhood",
        false,
    ),
    KeyBinding::new(
        "L",
        "Shift+l",
        "Link",
        "Link direct one-hop link",
        "Neighborhood",
        false,
    ),
    KeyBinding::new(
        "o",
        "o",
        "Moderator",
        "Open assign moderator modal",
        "Neighborhood",
        false,
    ),
    KeyBinding::new(
        "x",
        "x",
        "Override",
        "Open access override modal",
        "Neighborhood",
        false,
    ),
    KeyBinding::new(
        "p",
        "p",
        "Caps",
        "Open capability config modal",
        "Neighborhood",
        false,
    ),
    KeyBinding::new(
        "g",
        "g",
        "Home",
        "Go to primary home",
        "Neighborhood",
        false,
    ),
];

const SETTINGS_BINDINGS: &[KeyBinding] = &[
    KeyBinding::new("h/l", "h, l", "Panels", "Switch panels", "Settings", false),
    KeyBinding::new(
        "j/k",
        "j, k",
        "Navigate",
        "Navigate sections/sub-sections",
        "Settings",
        false,
    ),
    KeyBinding::new(
        "Enter",
        "Enter",
        "Select",
        "Confirm selection",
        "Settings",
        true,
    ),
    KeyBinding::new(
        "Space",
        "Space",
        "Toggle",
        "Toggle option/edit field",
        "Settings",
        true,
    ),
    KeyBinding::new(
        "s",
        "s",
        "Authority",
        "Switch authority (if multiple)",
        "Settings",
        false,
    ),
    KeyBinding::new(
        "m",
        "m",
        "MFA",
        "Configure multifactor auth",
        "Settings",
        false,
    ),
];

const NOTIFICATIONS_BINDINGS: &[KeyBinding] = &[
    KeyBinding::new(
        "j/k",
        "j, k",
        "Move",
        "Move through notifications",
        "Notifications",
        true,
    ),
    KeyBinding::new(
        "h/l",
        "h, l",
        "Focus",
        "Switch panels",
        "Notifications",
        true,
    ),
];

fn bindings_for_screen(screen: Screen) -> &'static [KeyBinding] {
    match screen {
        Screen::Neighborhood => NEIGHBORHOOD_BINDINGS,
        Screen::Chat => CHAT_BINDINGS,
        Screen::Contacts => CONTACTS_BINDINGS,
        Screen::Notifications => NOTIFICATIONS_BINDINGS,
        Screen::Settings => SETTINGS_BINDINGS,
    }
}

fn screen_from_name(name: &str) -> Option<Screen> {
    Screen::all()
        .iter()
        .copied()
        .find(|screen| screen.name() == name)
}

/// Compact global footer hints.
#[must_use]
pub fn global_footer_hints() -> Vec<KeyHint> {
    GLOBAL_BINDINGS
        .iter()
        .filter(|binding| binding.include_in_footer)
        .map(|binding| binding.to_hint())
        .collect()
}

/// Compact footer hints for a specific screen.
#[must_use]
pub fn screen_footer_hints(screen: Screen) -> Vec<KeyHint> {
    bindings_for_screen(screen)
        .iter()
        .filter(|binding| binding.include_in_footer)
        .map(|binding| binding.to_hint())
        .collect()
}

/// Keyboard help bindings for the current screen context.
#[must_use]
pub fn keyboard_help_bindings_for_screen(current_screen: Option<&str>) -> Vec<KeyBinding> {
    let mut result = Vec::new();
    result.extend_from_slice(GLOBAL_BINDINGS);

    match current_screen.and_then(screen_from_name) {
        Some(screen) => {
            result.extend_from_slice(bindings_for_screen(screen));
        }
        None if current_screen.is_none() => {
            for screen in Screen::all() {
                result.extend_from_slice(bindings_for_screen(*screen));
            }
        }
        None => {}
    }

    result
}
