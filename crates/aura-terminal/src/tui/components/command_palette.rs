//! # Command Palette Component
//!
//! Fuzzy command search overlay

use iocraft::prelude::*;

use crate::tui::theme::{Borders, Layout, Spacing, Theme};

/// A command in the palette
#[derive(Clone, Debug, Default)]
pub struct PaletteCommand {
    pub id: String,
    pub name: String,
    pub description: String,
    pub shortcut: Option<String>,
}

impl PaletteCommand {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            shortcut: None,
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    pub fn with_shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }
}

/// Props for CommandItem
#[derive(Default, Props)]
pub struct CommandItemProps {
    pub command: PaletteCommand,
    pub is_selected: bool,
}

/// A single command item in the palette
#[component]
pub fn CommandItem(props: &CommandItemProps) -> impl Into<AnyElement<'static>> {
    let cmd = &props.command;
    let pointer = if props.is_selected { "▸ " } else { "  " };
    let name_color = if props.is_selected {
        Theme::PRIMARY
    } else {
        Theme::TEXT
    };

    let name = cmd.name.clone();
    let description = cmd.description.clone();
    let shortcut = cmd.shortcut.clone().unwrap_or_default();

    element! {
        View(
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceBetween,
            padding_left: Spacing::LIST_ITEM_PADDING,
            padding_right: Spacing::LIST_ITEM_PADDING,
        ) {
            Text(content: pointer, color: Theme::PRIMARY)
            View(flex_direction: FlexDirection::Row, gap: Spacing::SM) {
                Text(content: name, color: name_color)
                Text(content: description, color: Theme::TEXT_MUTED)
            }
            #(if !shortcut.is_empty() {
                vec![element! {
                    View {
                        Text(content: shortcut, color: Theme::SECONDARY)
                    }
                }]
            } else {
                vec![element! { View {} }]
            })
        }
    }
}

/// Props for CommandPalette
#[derive(Default, Props)]
pub struct CommandPaletteProps {
    /// Whether the palette is visible
    pub visible: bool,
    /// Current search query
    pub query: String,
    /// Filtered commands to display
    pub commands: Vec<PaletteCommand>,
    /// Currently selected index
    pub selected_index: usize,
    /// Placeholder text for search input
    pub placeholder: String,
}

/// A command palette overlay
#[component]
pub fn CommandPalette(props: &CommandPaletteProps) -> impl Into<AnyElement<'static>> {
    if !props.visible {
        return element! {
            View {}
        };
    }

    let query = props.query.clone();
    let commands = props.commands.clone();
    let selected = props.selected_index;
    let placeholder = if props.placeholder.is_empty() {
        "Type to search...".to_string()
    } else {
        props.placeholder.clone()
    };

    let display_text = if query.is_empty() { placeholder } else { query };
    let text_color = if props.query.is_empty() {
        Theme::TEXT_MUTED
    } else {
        Theme::TEXT
    };

    element! {
        View(
            position: Position::Absolute,
            width: 100pct,
            height: 100pct,
            justify_content: JustifyContent::FlexStart,
            align_items: AlignItems::Center,
            padding_top: Layout::OVERLAY_TOP_PADDING,

        ) {
            View(
                width: Percent(Layout::COMMAND_PALETTE_WIDTH_PCT),
                max_height: Percent(Layout::COMMAND_PALETTE_MAX_HEIGHT_PCT),
                flex_direction: FlexDirection::Column,
                background_color: Theme::BG_MODAL,
                border_style: Borders::PRIMARY,
                border_color: Theme::BORDER_FOCUS,
            ) {
                // Search input
                View(
                    padding: Spacing::PANEL_PADDING,
                    border_style: BorderStyle::Single,
                    border_edges: Edges::Bottom,
                    border_color: Theme::BORDER,
                ) {
                    View(flex_direction: FlexDirection::Row, gap: Spacing::XS) {
                        Text(content: ">", color: Theme::PRIMARY)
                        Text(content: display_text, color: text_color)
                    }
                }
                // Command list
                View(
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.0,
                    overflow: Overflow::Scroll,
                    padding: Spacing::PANEL_PADDING,
                ) {
                    #(if commands.is_empty() {
                        vec![element! {
                            View {
                                Text(content: "No commands found", color: Theme::TEXT_MUTED)
                            }
                        }]
                    } else {
                        commands.iter().enumerate().map(|(idx, cmd)| {
                            let is_selected = idx == selected;
                            element! {
                                View {
                                    CommandItem(command: cmd.clone(), is_selected: is_selected)
                                }
                            }
                        }).collect()
                    })
                }
                // Hints
                View(
                    padding: Spacing::PANEL_PADDING,
                    border_style: BorderStyle::Single,
                    border_edges: Edges::Top,
                    border_color: Theme::BORDER,
                    flex_direction: FlexDirection::Row,
                    gap: Spacing::MD,
                ) {
                    Text(content: "↑↓ Navigate", color: Theme::TEXT_MUTED)
                    Text(content: "Enter Select", color: Theme::TEXT_MUTED)
                    Text(content: "Esc Close", color: Theme::TEXT_MUTED)
                }
            }
        }
    }
}
