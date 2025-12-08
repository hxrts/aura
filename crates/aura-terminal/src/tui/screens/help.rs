//! # Help Screen
//!
//! Displays help for IRC-style slash commands

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
#[allow(dead_code)]
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

    element! {
        View(
            flex_direction: FlexDirection::Column,
            width: 100pct,
            height: 100pct,
            flex_grow: 1.0,
            flex_shrink: 1.0,
            overflow: Overflow::Hidden,
        ) {
            // Command list
            View(
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                padding: Spacing::XS,
                overflow: Overflow::Hidden,
            ) {
                #(commands.iter().enumerate().map(|(idx, cmd)| {
                    let is_selected = idx == current_selected;
                    let key = cmd.name.clone();
                    element! {
                        View(key: key) {
                            CommandItem(
                                name: cmd.name.clone(),
                                syntax: cmd.syntax.clone(),
                                description: cmd.description.clone(),
                                is_selected: is_selected,
                            )
                        }
                    }
                }))
            }
        }
    }
}

/// Run the help screen with sample data
pub async fn run_help_screen() -> std::io::Result<()> {
    let commands = vec![
        HelpCommand::new(
            "/msg",
            "/msg <user> <message>",
            "Send a direct message",
            "User",
        ),
        HelpCommand::new("/join", "/join <channel>", "Join a channel", "User"),
        HelpCommand::new(
            "/leave",
            "/leave [channel]",
            "Leave current or specified channel",
            "User",
        ),
        HelpCommand::new("/nick", "/nick <name>", "Change your display name", "User"),
        HelpCommand::new("/me", "/me <action>", "Send an action message", "User"),
        HelpCommand::new(
            "/kick",
            "/kick <user> [reason]",
            "Kick a user from the block",
            "Moderator",
        ),
        HelpCommand::new(
            "/ban",
            "/ban <user> [duration]",
            "Ban a user from the block",
            "Moderator",
        ),
        HelpCommand::new(
            "/mute",
            "/mute <user> [duration]",
            "Mute a user",
            "Moderator",
        ),
        HelpCommand::new(
            "/admin",
            "/admin <action>",
            "Administrative commands",
            "Admin",
        ),
        HelpCommand::new(
            "/config",
            "/config <setting> <value>",
            "Configure block settings",
            "Admin",
        ),
    ];

    element! {
        HelpScreen(commands: commands, selected_index: 0usize)
    }
    .fullscreen()
    .await
}
