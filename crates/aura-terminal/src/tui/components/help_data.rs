//! # Help Commands
//!
//! Keyboard shortcuts and command data for the help modal.
//! The actual modal UI is in `components/help_modal.rs`.

use crate::tui::commands::{all_command_help, CommandCategory};

/// A command help item
#[derive(Clone, Debug)]
pub struct HelpCommand {
    pub name: String,
    pub syntax: String,
    pub description: String,
    pub category: String,
}

impl HelpCommand {
    pub fn new(
        name: impl Into<String>,
        syntax: impl Into<String>,
        description: impl Into<String>,
        category: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            syntax: syntax.into(),
            description: description.into(),
            category: category.into(),
        }
    }
}

/// Get help commands filtered for a specific screen context
///
/// When a current_screen is provided, returns commands in this order:
/// 1. Navigation (global hotkeys - applies to all screens)
/// 2. Current screen commands (screen-specific hotkeys)
/// 3. For Chat screen: slash commands organized by category
///
/// Commands from other screens are NOT included to keep help focused and relevant.
#[must_use]
pub fn get_help_commands_for_screen(current_screen: Option<&str>) -> Vec<HelpCommand> {
    let all_commands = get_help_commands();

    match current_screen {
        Some(screen) => {
            let mut result = Vec::new();

            // Always include navigation first (global hotkeys)
            result.extend(
                all_commands
                    .iter()
                    .filter(|c| c.category == "Navigation")
                    .cloned(),
            );

            // Then commands for the current screen only
            result.extend(
                all_commands
                    .iter()
                    .filter(|c| c.category == screen)
                    .cloned(),
            );

            // For Chat screen, add slash commands
            if screen == "Chat" {
                result.extend(get_slash_commands());
            }

            result
        }
        None => all_commands,
    }
}

/// Get slash commands for the Chat screen, organized by category
fn get_slash_commands() -> Vec<HelpCommand> {
    let mut commands = Vec::new();

    // Add commands by category: User, Moderator, Admin
    for category in CommandCategory::all() {
        let category_name = match category {
            CommandCategory::User => "Slash Commands",
            CommandCategory::Moderator => "Moderator Commands",
            CommandCategory::Admin => "Admin Commands",
        };

        for cmd in all_command_help() {
            if cmd.category == *category {
                commands.push(HelpCommand::new(
                    format!("/{}", cmd.name),
                    cmd.syntax,
                    cmd.description,
                    category_name,
                ));
            }
        }
    }

    commands
}

/// Get all keyboard shortcuts organized by category
#[must_use]
pub fn get_help_commands() -> Vec<HelpCommand> {
    vec![
        // Global navigation
        HelpCommand::new("1-5", "1, 2, 3, 4, 5", "Switch screens", "Navigation"),
        HelpCommand::new("Tab", "Tab", "Next screen", "Navigation"),
        HelpCommand::new("S-Tab", "Shift+Tab", "Previous screen", "Navigation"),
        HelpCommand::new("?", "?", "Show/hide help", "Navigation"),
        HelpCommand::new("q", "q", "Quit", "Navigation"),
        HelpCommand::new("Esc", "Esc", "Cancel/close modal/toast", "Navigation"),
        HelpCommand::new("y", "y", "Copy error to clipboard", "Navigation"),
        HelpCommand::new("j/k", "j, k", "Move down/up in lists", "Navigation"),
        HelpCommand::new("h/l", "h, l", "Switch panels (left/right)", "Navigation"),
        // Chat screen
        HelpCommand::new("i", "i", "Enter insert mode (type message)", "Chat"),
        HelpCommand::new("n", "n", "Create new channel", "Chat"),
        HelpCommand::new("o", "o", "Open channel info", "Chat"),
        HelpCommand::new("t", "t", "Set channel topic", "Chat"),
        HelpCommand::new("r", "r", "Retry failed message", "Chat"),
        HelpCommand::new(
            "Tab",
            "Tab",
            "Switch between channels/messages/input",
            "Chat",
        ),
        // Contacts screen
        HelpCommand::new("e", "e", "Edit contact nickname", "Contacts"),
        HelpCommand::new("g", "g", "Open guardian setup", "Contacts"),
        HelpCommand::new("c", "c", "Start chat with contact", "Contacts"),
        HelpCommand::new("a", "a", "Accept invitation code", "Contacts"),
        HelpCommand::new("n", "n", "Create invitation code", "Contacts"),
        HelpCommand::new("r", "r", "Remove contact", "Contacts"),
        // Neighborhood screen
        HelpCommand::new("Enter", "Enter", "Enter selected home", "Neighborhood"),
        HelpCommand::new("Esc", "Esc", "Return to map view", "Neighborhood"),
        HelpCommand::new("a", "a", "Accept invitation code", "Neighborhood"),
        HelpCommand::new(
            "i",
            "i",
            "Enter insert mode (Interior only)",
            "Neighborhood",
        ),
        HelpCommand::new("d", "d", "Cycle traversal depth", "Neighborhood"),
        HelpCommand::new("g", "g", "Go to primary home", "Neighborhood"),
        // Traversal depth descriptions
        HelpCommand::new("Street", "", "View blocks, no interaction", "Neighborhood"),
        HelpCommand::new(
            "Frontage",
            "",
            "Limited interaction, request entry",
            "Neighborhood",
        ),
        HelpCommand::new(
            "Interior",
            "",
            "Full access, can send messages",
            "Neighborhood",
        ),
        // Settings screen
        HelpCommand::new("h/l", "h, l", "Switch panels", "Settings"),
        HelpCommand::new("j/k", "j, k", "Navigate sections/sub-sections", "Settings"),
        HelpCommand::new("Space", "Space", "Toggle option/edit field", "Settings"),
        HelpCommand::new("Enter", "Enter", "Confirm selection", "Settings"),
        HelpCommand::new("s", "s", "Switch authority (if multiple)", "Settings"),
        HelpCommand::new("m", "m", "Configure multifactor auth", "Settings"),
        // Notifications screen
        HelpCommand::new("j/k", "j, k", "Move through notifications", "Notifications"),
        HelpCommand::new("h/l", "h, l", "Switch panels", "Notifications"),
    ]
}
