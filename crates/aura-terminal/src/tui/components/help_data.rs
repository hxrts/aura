//! # Help Commands
//!
//! Keyboard shortcuts and command data for the help modal.
//! The actual modal UI is in `components/help_modal.rs`.

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
/// 1. Navigation (always first - applies to all screens)
/// 2. Current screen commands (most relevant)
/// 3. Other screen commands (excluded for Neighborhood to reduce clutter)
pub fn get_help_commands_for_screen(current_screen: Option<&str>) -> Vec<HelpCommand> {
    let all_commands = get_help_commands();

    match current_screen {
        Some(screen) => {
            let mut result = Vec::new();

            // Always include navigation first
            result.extend(
                all_commands
                    .iter()
                    .filter(|c| c.category == "Navigation")
                    .cloned(),
            );

            // Then commands for the current screen
            result.extend(
                all_commands
                    .iter()
                    .filter(|c| c.category == screen)
                    .cloned(),
            );

            // For Neighborhood, only show Navigation + Neighborhood commands
            // (other screen hotkeys are not relevant when navigating blocks)
            if screen != "Neighborhood" {
                // For other screens, include remaining commands
                result.extend(
                    all_commands
                        .iter()
                        .filter(|c| c.category != "Navigation" && c.category != screen)
                        .cloned(),
                );
            }

            result
        }
        None => all_commands,
    }
}

/// Get all keyboard shortcuts organized by category
pub fn get_help_commands() -> Vec<HelpCommand> {
    vec![
        // Global navigation
        HelpCommand::new("1-5", "1, 2, 3, 4, 5", "Switch screens", "Navigation"),
        HelpCommand::new("?", "?", "Show/hide help", "Navigation"),
        HelpCommand::new("q", "q", "Quit", "Navigation"),
        HelpCommand::new("Esc", "Esc", "Cancel/close modal", "Navigation"),
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
        // Neighborhood screen
        HelpCommand::new("Enter", "Enter", "Enter selected block", "Neighborhood"),
        HelpCommand::new("Esc", "Esc", "Return to map view", "Neighborhood"),
        HelpCommand::new("a", "a", "Accept invitation code", "Neighborhood"),
        HelpCommand::new(
            "i",
            "i",
            "Enter insert mode (Interior only)",
            "Neighborhood",
        ),
        HelpCommand::new("d", "d", "Cycle traversal depth", "Neighborhood"),
        HelpCommand::new("g", "g", "Go to home block", "Neighborhood"),
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
        HelpCommand::new("Space", "Space", "Toggle option/edit field", "Settings"),
        HelpCommand::new("Enter", "Enter", "Confirm selection", "Settings"),
        // Notifications screen
        HelpCommand::new("j/k", "j, k", "Move through notifications", "Notifications"),
        HelpCommand::new("h/l", "h, l", "Switch panels", "Notifications"),
    ]
}
