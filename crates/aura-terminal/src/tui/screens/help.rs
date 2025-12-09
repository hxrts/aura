//! # Help Screen
//!
//! Displays keyboard shortcuts organized by screen category

use iocraft::prelude::*;

use crate::tui::navigation::{is_nav_key_press, navigate_list, NavThrottle};
use crate::tui::theme::{Spacing, Theme};

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

/// Props for CommandItem
#[derive(Default, Props)]
pub struct CommandItemProps {
    pub name: String,
    pub syntax: String,
    pub description: String,
    pub is_selected: bool,
}

/// A single command item in the list
#[component]
pub fn CommandItem(props: &CommandItemProps) -> impl Into<AnyElement<'static>> {
    // Use consistent list item colors
    let bg = if props.is_selected {
        Theme::LIST_BG_SELECTED
    } else {
        Theme::LIST_BG_NORMAL
    };

    let name_color = if props.is_selected {
        Theme::LIST_TEXT_SELECTED
    } else {
        Theme::PRIMARY
    };

    let syntax_color = if props.is_selected {
        Theme::LIST_TEXT_SELECTED
    } else {
        Theme::LIST_TEXT_MUTED
    };

    let desc_color = if props.is_selected {
        Theme::LIST_TEXT_SELECTED
    } else {
        Theme::LIST_TEXT_NORMAL
    };

    let name = props.name.clone();
    let syntax = props.syntax.clone();
    let description = props.description.clone();

    element! {
        View(
            flex_direction: FlexDirection::Column,
            background_color: bg,
            padding: Spacing::XS,
            margin_bottom: 0,
        ) {
            View(flex_direction: FlexDirection::Row, gap: Spacing::SM) {
                Text(content: name, weight: Weight::Bold, color: name_color)
                Text(content: syntax, color: syntax_color)
            }
            Text(content: description, color: desc_color)
        }
    }
}

/// Props for CategoryHeader
#[derive(Default, Props)]
pub struct CategoryHeaderProps {
    pub title: String,
}

/// Category header in the command list
#[component]
pub fn CategoryHeader(props: &CategoryHeaderProps) -> impl Into<AnyElement<'static>> {
    let title = props.title.clone();

    element! {
        View(
            padding_top: 1,
            padding_bottom: 1,
            border_style: BorderStyle::Single,
            border_edges: Edges::Bottom,
            border_color: Theme::BORDER,
        ) {
            Text(content: title, weight: Weight::Bold, color: Theme::SECONDARY)
        }
    }
}

/// Props for HelpScreen
#[derive(Default, Props)]
pub struct HelpScreenProps {
    pub commands: Vec<HelpCommand>,
    pub selected_index: usize,
}

/// Group commands by category, preserving order
fn group_by_category(commands: &[HelpCommand]) -> Vec<(String, Vec<&HelpCommand>)> {
    let mut groups: Vec<(String, Vec<&HelpCommand>)> = Vec::new();
    let mut current_category: Option<String> = None;

    for cmd in commands {
        if current_category.as_ref() != Some(&cmd.category) {
            groups.push((cmd.category.clone(), Vec::new()));
            current_category = Some(cmd.category.clone());
        }
        if let Some((_, ref mut cmds)) = groups.last_mut() {
            cmds.push(cmd);
        }
    }
    groups
}

/// The help screen component
#[component]
pub fn HelpScreen(props: &HelpScreenProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let mut selected = hooks.use_state(|| props.selected_index);

    let commands = props.commands.clone();
    let current_selected = selected.get();
    let command_count = commands.len();

    // Throttle for navigation keys - persists across renders using use_ref
    let mut nav_throttle = hooks.use_ref(NavThrottle::new);

    hooks.use_terminal_events({
        move |event| {
            // Handle navigation keys (vertical only for single-panel list)
            if let Some(nav_key) = is_nav_key_press(&event) {
                if nav_key.is_vertical() && nav_throttle.write().try_navigate() && command_count > 0
                {
                    let new_idx = navigate_list(selected.get(), command_count, nav_key);
                    selected.set(new_idx);
                }
            }
        }
    });

    // Group commands by category for display
    let groups = group_by_category(&commands);

    // Build flat list with command indices for selection tracking
    let mut flat_idx = 0usize;
    let grouped_elements: Vec<AnyElement<'static>> = groups
        .into_iter()
        .flat_map(|(category, cmds)| {
            let mut elements: Vec<AnyElement<'static>> = Vec::new();

            // Category header
            let header_key = format!("header-{}", category);
            elements.push(
                element! {
                    View(key: header_key) {
                        CategoryHeader(title: category)
                    }
                }
                .into_any(),
            );

            // Commands in this category
            for cmd in cmds {
                let is_selected = flat_idx == current_selected;
                let item_key = format!("{}-{}", cmd.category, cmd.name);
                elements.push(
                    element! {
                        View(key: item_key) {
                            CommandItem(
                                name: cmd.name.clone(),
                                syntax: cmd.syntax.clone(),
                                description: cmd.description.clone(),
                                is_selected: is_selected,
                            )
                        }
                    }
                    .into_any(),
                );
                flat_idx += 1;
            }

            elements
        })
        .collect();

    element! {
        View(
            flex_direction: FlexDirection::Column,
            width: 100pct,
            height: 100pct,
            flex_grow: 1.0,
            flex_shrink: 1.0,
            overflow: Overflow::Hidden,
        ) {
            // Command list with category headers
            View(
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                padding: Spacing::XS,
                overflow: Overflow::Hidden,
            ) {
                #(grouped_elements)
            }
        }
    }
}

/// Get help commands filtered for a specific screen context
///
/// When a current_screen is provided, returns commands in this order:
/// 1. Navigation (always first - applies to all screens)
/// 2. Current screen commands (most relevant)
/// 3. Other screen commands
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

            // Then all other commands
            result.extend(
                all_commands
                    .iter()
                    .filter(|c| c.category != "Navigation" && c.category != screen)
                    .cloned(),
            );

            result
        }
        None => all_commands,
    }
}

/// Get all keyboard shortcuts organized by category
pub fn get_help_commands() -> Vec<HelpCommand> {
    vec![
        // Global navigation
        HelpCommand::new(
            "1-7",
            "1, 2, 3, 4, 5, 6, 7",
            "Switch screens (Block, Chat, Contacts, Neighborhood, Invitations, Settings, Recovery)",
            "Navigation",
        ),
        HelpCommand::new("?", "?", "Show/hide help", "Navigation"),
        HelpCommand::new("q", "q", "Quit application", "Navigation"),
        HelpCommand::new("Esc", "Esc", "Cancel/close modal", "Navigation"),
        HelpCommand::new("j/k", "j, k", "Move down/up in lists", "Navigation"),
        HelpCommand::new("h/l", "h, l", "Switch panels (left/right)", "Navigation"),
        // Block screen
        HelpCommand::new("i", "i", "Enter insert mode (type message)", "Block"),
        HelpCommand::new("v", "v", "Send block invitation", "Block"),
        HelpCommand::new("n", "n", "Navigate to neighborhood", "Block"),
        HelpCommand::new("g", "g", "Grant steward role", "Block"),
        HelpCommand::new("R", "Shift+R", "Revoke steward role", "Block"),
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
        HelpCommand::new("e", "e", "Edit contact petname", "Contacts"),
        HelpCommand::new("g", "g", "Toggle guardian status", "Contacts"),
        HelpCommand::new("c", "c", "Start chat with contact", "Contacts"),
        HelpCommand::new("i", "i", "Invite selected peer", "Contacts"),
        // Neighborhood screen
        HelpCommand::new("Enter", "Enter", "Enter selected block", "Neighborhood"),
        HelpCommand::new("g", "g", "Go to home block", "Neighborhood"),
        HelpCommand::new("b", "b", "Back to street view", "Neighborhood"),
        // Invitations screen
        HelpCommand::new("n", "n", "Create new invitation", "Invitations"),
        HelpCommand::new("i", "i", "Import invitation code", "Invitations"),
        HelpCommand::new("e", "e", "Export invitation code", "Invitations"),
        HelpCommand::new("f", "f", "Filter invitations", "Invitations"),
        // Settings screen
        HelpCommand::new("h/l", "h, l", "Switch panels", "Settings"),
        HelpCommand::new("Space", "Space", "Toggle option/edit field", "Settings"),
        HelpCommand::new("Enter", "Enter", "Confirm selection", "Settings"),
        // Recovery screen
        HelpCommand::new("a", "a", "Add guardian to recovery", "Recovery"),
        HelpCommand::new("s", "s", "Start recovery process", "Recovery"),
        HelpCommand::new("h/l", "h, l", "Switch tabs", "Recovery"),
    ]
}

/// Run the help screen with sample data
pub async fn run_help_screen() -> std::io::Result<()> {
    let commands = get_help_commands();

    element! {
        HelpScreen(commands: commands, selected_index: 0usize)
    }
    .fullscreen()
    .await
}
