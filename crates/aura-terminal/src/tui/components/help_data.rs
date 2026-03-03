//! # Help Commands
//!
//! Keyboard shortcuts and command data for the help modal.
//! The actual modal UI is in `components/help_modal.rs`.

use crate::tui::commands::{all_command_help, CommandCategory};
use crate::tui::keymap::{keyboard_help_bindings_for_screen, KeyBinding};

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

fn keyboard_binding_to_help_command(binding: &KeyBinding) -> HelpCommand {
    HelpCommand::new(
        binding.key,
        binding.syntax,
        binding.help_description,
        binding.category,
    )
}

fn neighborhood_depth_descriptions() -> Vec<HelpCommand> {
    vec![
        HelpCommand::new("Limited", "", "View blocks, no interaction", "Neighborhood"),
        HelpCommand::new(
            "Partial",
            "",
            "Limited interaction, request entry",
            "Neighborhood",
        ),
        HelpCommand::new(
            "Full",
            "",
            "Full access to member/channel views",
            "Neighborhood",
        ),
    ]
}

/// Get all keyboard shortcuts organized by category
#[must_use]
pub fn get_help_commands() -> Vec<HelpCommand> {
    let mut commands: Vec<HelpCommand> = keyboard_help_bindings_for_screen(None)
        .iter()
        .map(keyboard_binding_to_help_command)
        .collect();
    commands.extend(neighborhood_depth_descriptions());
    commands
}
