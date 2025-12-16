//! # Help Modal
//!
//! Modal overlay showing keyboard shortcuts and screen navigation hints.
//! Context-sensitive: shows relevant commands for the current screen first.

use iocraft::prelude::*;

use crate::tui::layout::dim;
use crate::tui::theme::Theme;

use super::help_data::{get_help_commands_for_screen, HelpCommand};

/// Props for HelpModal
#[derive(Default, Props)]
pub struct HelpModalProps {
    /// Whether the modal is visible
    pub visible: bool,
    /// Current screen name for context-sensitive help (e.g., "Chat", "Block")
    pub current_screen: Option<String>,
}

/// Group commands by category for display
fn group_commands_by_category(commands: &[HelpCommand]) -> Vec<(String, Vec<&HelpCommand>)> {
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

/// Help modal showing keyboard shortcuts
/// Context-sensitive: prioritizes commands for the current screen
#[component]
pub fn HelpModal(props: &HelpModalProps) -> impl Into<AnyElement<'static>> {
    if !props.visible {
        return element! {
            View {}
        };
    }

    // Get context-sensitive commands
    let current_screen = props.current_screen.as_deref();
    let commands = get_help_commands_for_screen(current_screen);
    let groups = group_commands_by_category(&commands);

    // Build header with context info
    let header_text = if let Some(screen) = current_screen {
        format!("Help - {}", screen)
    } else {
        "Keyboard Shortcuts".to_string()
    };

    // Build grouped command elements with 2-column grid layout
    let category_elements: Vec<AnyElement<'static>> = groups
        .into_iter()
        .take(4)
        .map(|(category, cmds)| {
            let cat_name = category.clone();

            // Build command items as 50% width grid cells
            let cmd_elements: Vec<AnyElement<'static>> = cmds
                .into_iter()
                .map(|cmd| {
                    let key = cmd.name.clone();
                    let desc = cmd.description.clone();
                    element! {
                        View(flex_direction: FlexDirection::Row, width: 50pct, padding_right: 1) {
                            View(width: 10) {
                                Text(content: key, weight: Weight::Bold, color: Theme::SECONDARY)
                            }
                            Text(content: desc, color: Theme::TEXT)
                        }
                    }
                    .into_any()
                })
                .collect();

            element! {
                View(flex_direction: FlexDirection::Column, margin_bottom: 1) {
                    // Category header
                    View(margin_bottom: 0) {
                        Text(content: cat_name, weight: Weight::Bold, color: Theme::PRIMARY)
                    }
                    // Commands in wrapping 2-column grid
                    View(flex_direction: FlexDirection::Row, flex_wrap: FlexWrap::Wrap) {
                        #(cmd_elements)
                    }
                }
            }
            .into_any()
        })
        .collect();

    element! {
        View(
            position: Position::Absolute,
            top: dim::NAV_HEIGHT,
            left: 0u16,
            width: dim::TOTAL_WIDTH,
            height: dim::MIDDLE_HEIGHT,
            flex_direction: FlexDirection::Column,
            background_color: Theme::BG_MODAL,
            border_style: BorderStyle::Round,
            border_color: Theme::PRIMARY,
            overflow: Overflow::Hidden,
        ) {
            // Header
            View(
                width: 100pct,
                padding: 1,
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::Center,
                border_style: BorderStyle::Single,
                border_edges: Edges::Bottom,
                border_color: Theme::BORDER,
            ) {
                Text(
                    content: header_text,
                    weight: Weight::Bold,
                    color: Theme::PRIMARY,
                )
            }

            // Body - display grouped commands in grid
            View(
                width: 100pct,
                padding: 2,
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                flex_shrink: 1.0,
                overflow: Overflow::Hidden,
            ) {
                #(category_elements)
            }

            // Footer
            View(
                width: 100pct,
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::Center,
                padding: 1,
                border_style: BorderStyle::Single,
                border_edges: Edges::Top,
                border_color: Theme::BORDER,
            ) {
                Text(content: "Esc", weight: Weight::Bold, color: Theme::SECONDARY)
                Text(content: " / ", color: Theme::TEXT_MUTED)
                Text(content: "?", weight: Weight::Bold, color: Theme::SECONDARY)
                Text(content: " close", color: Theme::TEXT_MUTED)
            }
        }
    }
}

/// State for help modal
#[derive(Clone, Debug, Default)]
pub struct HelpModalState {
    /// Whether the modal is visible
    pub visible: bool,
}

impl HelpModalState {
    /// Create a new state
    pub fn new() -> Self {
        Self::default()
    }

    /// Show the modal
    pub fn show(&mut self) {
        self.visible = true;
    }

    /// Hide the modal
    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// Toggle visibility
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_help_modal_state() {
        let mut state = HelpModalState::new();
        assert!(!state.visible);

        state.show();
        assert!(state.visible);

        state.hide();
        assert!(!state.visible);

        state.toggle();
        assert!(state.visible);

        state.toggle();
        assert!(!state.visible);
    }
}
