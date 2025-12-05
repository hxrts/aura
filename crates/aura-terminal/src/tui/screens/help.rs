//! # Help Screen
//!
//! Displays help for IRC-style slash commands

use iocraft::prelude::*;
use std::time::{Duration, Instant};

use crate::tui::components::KeyHintsBar;
use crate::tui::theme::Theme;
use crate::tui::types::KeyHint;

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
    let bg = if props.is_selected {
        Theme::BG_SELECTED
    } else {
        Theme::BG_DARK
    };

    let name = props.name.clone();
    let syntax = props.syntax.clone();
    let description = props.description.clone();

    element! {
        View(
            flex_direction: FlexDirection::Column,
            background_color: bg,
            padding: 1,
            margin_bottom: 1,
        ) {
            View(flex_direction: FlexDirection::Row, gap: 2) {
                Text(content: name, weight: Weight::Bold, color: Theme::PRIMARY)
                Text(content: syntax, color: Theme::TEXT_MUTED)
            }
            Text(content: description, color: Theme::TEXT)
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
    let selected = hooks.use_state(|| props.selected_index);

    let hints = vec![
        KeyHint::new("↑↓", "Navigate"),
        KeyHint::new("/", "Search"),
        KeyHint::new("Tab", "Filter"),
        KeyHint::new("Esc", "Close"),
    ];

    let commands = props.commands.clone();
    let current_selected = selected.get();

    // Throttle for navigation keys - persists across renders using use_ref
    let mut nav_throttle = hooks.use_ref(|| Instant::now() - Duration::from_millis(200));
    let throttle_duration = Duration::from_millis(150);

    hooks.use_terminal_events({
        let mut selected = selected.clone();
        let command_count = commands.len();
        move |event| {
            if let TerminalEvent::Key(KeyEvent { code, .. }) = event {
                match code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        let should_move = nav_throttle.read().elapsed() >= throttle_duration;
                        if should_move {
                            let current = selected.get();
                            if current > 0 {
                                selected.set(current - 1);
                            }
                            nav_throttle.set(Instant::now());
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        let should_move = nav_throttle.read().elapsed() >= throttle_duration;
                        if should_move {
                            let current = selected.get();
                            if current + 1 < command_count {
                                selected.set(current + 1);
                            }
                            nav_throttle.set(Instant::now());
                        }
                    }
                    _ => {}
                }
            }
        }
    });

    element! {
        View(
            flex_direction: FlexDirection::Column,
            width: 100pct,
            height: 100pct,
        ) {
            // Header
            View(
                padding: 1,
                border_style: BorderStyle::Single,
                border_edges: Edges::Bottom,
                border_color: Theme::BORDER,
            ) {
                Text(content: "Help", weight: Weight::Bold, color: Theme::PRIMARY)
            }

            // Command list
            View(
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                padding: 1,
                overflow: Overflow::Scroll,
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

            // Key hints
            KeyHintsBar(hints: hints)
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
