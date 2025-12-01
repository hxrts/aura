//! # Command Palette Component
//!
//! A VS Code-style command palette for quick access to actions.
//! Triggered via Ctrl+P, provides fuzzy search over available commands.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};

use super::{Component, InputAction, Styles};
use crate::tui::screens::ScreenType;

/// A command that can be executed from the palette
#[derive(Debug, Clone)]
pub struct PaletteCommand {
    /// Display name
    pub name: String,
    /// Short description
    pub description: String,
    /// Keyboard shortcut hint (if any)
    pub shortcut: Option<String>,
    /// Category for grouping
    pub category: CommandPaletteCategory,
    /// Action to execute
    pub action: PaletteAction,
}

impl PaletteCommand {
    /// Create a navigation command
    pub fn navigate(screen: ScreenType) -> Self {
        let shortcut = screen.shortcut().map(|c| c.to_string());
        Self {
            name: format!("Go to {}", screen.title()),
            description: format!("Navigate to {} screen", screen.title()),
            shortcut,
            category: CommandPaletteCategory::Navigation,
            action: PaletteAction::Navigate(screen),
        }
    }

    /// Create an action command
    pub fn action(
        name: impl Into<String>,
        description: impl Into<String>,
        action_id: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            shortcut: None,
            category: CommandPaletteCategory::Action,
            action: PaletteAction::Custom(action_id.into()),
        }
    }

    /// Create a help command
    pub fn help(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            shortcut: None,
            category: CommandPaletteCategory::Help,
            action: PaletteAction::ShowHelp,
        }
    }

    /// Check if command matches search query (fuzzy)
    pub fn matches(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let query_lower = query.to_lowercase();
        let name_lower = self.name.to_lowercase();
        let desc_lower = self.description.to_lowercase();

        // Simple substring match
        name_lower.contains(&query_lower) || desc_lower.contains(&query_lower)
    }

    /// Calculate match score (higher = better match)
    pub fn match_score(&self, query: &str) -> u32 {
        if query.is_empty() {
            return 100;
        }
        let query_lower = query.to_lowercase();
        let name_lower = self.name.to_lowercase();

        // Exact prefix match is best
        if name_lower.starts_with(&query_lower) {
            return 100;
        }

        // Word start match
        if name_lower
            .split_whitespace()
            .any(|w| w.starts_with(&query_lower))
        {
            return 75;
        }

        // Contains match
        if name_lower.contains(&query_lower) {
            return 50;
        }

        // Description match
        if self.description.to_lowercase().contains(&query_lower) {
            return 25;
        }

        0
    }
}

/// Categories for command palette entries
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandPaletteCategory {
    /// Screen navigation
    Navigation,
    /// Actions/operations
    Action,
    /// Help and documentation
    Help,
    /// IRC-style commands
    Command,
}

impl CommandPaletteCategory {
    /// Get category display name
    pub fn name(&self) -> &'static str {
        match self {
            Self::Navigation => "Navigation",
            Self::Action => "Actions",
            Self::Help => "Help",
            Self::Command => "Commands",
        }
    }
}

/// Action to perform when a command is selected
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteAction {
    /// Navigate to a screen
    Navigate(ScreenType),
    /// Execute a custom action by ID
    Custom(String),
    /// Show help
    ShowHelp,
    /// Execute an IRC command
    IrcCommand(String),
}

/// Command palette component
pub struct CommandPalette {
    /// All available commands
    commands: Vec<PaletteCommand>,
    /// Search query
    query: String,
    /// Filtered commands
    filtered: Vec<usize>,
    /// Selected index in filtered list
    selected: usize,
    /// List state for rendering
    list_state: ListState,
    /// Whether the palette is visible
    visible: bool,
    /// Width as percentage of screen
    width_percent: u16,
    /// Max height as percentage of screen
    max_height_percent: u16,
}

impl CommandPalette {
    /// Create a new command palette
    pub fn new() -> Self {
        let commands = Self::default_commands();
        let filtered: Vec<usize> = (0..commands.len()).collect();

        let mut list_state = ListState::default();
        if !filtered.is_empty() {
            list_state.select(Some(0));
        }

        Self {
            commands,
            query: String::new(),
            filtered,
            selected: 0,
            list_state,
            visible: false,
            width_percent: 50,
            max_height_percent: 60,
        }
    }

    /// Generate default commands
    fn default_commands() -> Vec<PaletteCommand> {
        let mut commands = vec![
            // Navigation commands
            PaletteCommand::navigate(ScreenType::Welcome),
            PaletteCommand::navigate(ScreenType::Chat),
            PaletteCommand::navigate(ScreenType::Guardians),
            PaletteCommand::navigate(ScreenType::Recovery),
            PaletteCommand::navigate(ScreenType::Invitations),
            PaletteCommand::navigate(ScreenType::Contacts),
            PaletteCommand::navigate(ScreenType::Neighborhood),
            PaletteCommand::navigate(ScreenType::Block),
            PaletteCommand::navigate(ScreenType::BlockMessages),
            PaletteCommand::navigate(ScreenType::Help),
            // Action commands
            PaletteCommand::action("Quit", "Exit the application", "quit"),
            PaletteCommand::action("Refresh", "Refresh current view", "refresh"),
            // Help commands
            PaletteCommand::help("Show Help", "Display keyboard shortcuts and commands"),
            PaletteCommand {
                name: "About".to_string(),
                description: "Show information about Aura".to_string(),
                shortcut: None,
                category: CommandPaletteCategory::Help,
                action: PaletteAction::Custom("about".to_string()),
            },
        ];

        // Add IRC commands (prefix with /)
        let irc_commands = vec![
            ("/msg", "Send a private message"),
            ("/me", "Send an action/emote"),
            ("/nick", "Change your display name"),
            ("/who", "List participants"),
            ("/whois", "View user info"),
            ("/leave", "Leave current context"),
            ("/help", "Show command help"),
            ("/kick", "Remove a user"),
            ("/ban", "Ban a user"),
            ("/invite", "Invite a user"),
        ];

        for (cmd, desc) in irc_commands {
            commands.push(PaletteCommand {
                name: cmd.to_string(),
                description: desc.to_string(),
                shortcut: None,
                category: CommandPaletteCategory::Command,
                action: PaletteAction::IrcCommand(cmd.to_string()),
            });
        }

        commands
    }

    /// Show the palette
    pub fn show(&mut self) {
        self.visible = true;
        self.query.clear();
        self.update_filter();
    }

    /// Hide the palette
    pub fn hide(&mut self) {
        self.visible = false;
        self.query.clear();
    }

    /// Toggle visibility
    pub fn toggle(&mut self) {
        if self.visible {
            self.hide();
        } else {
            self.show();
        }
    }

    /// Update filtered commands based on query
    fn update_filter(&mut self) {
        self.filtered = self
            .commands
            .iter()
            .enumerate()
            .filter(|(_, cmd)| cmd.matches(&self.query))
            .map(|(i, cmd)| (i, cmd.match_score(&self.query)))
            .collect::<Vec<_>>()
            .into_iter()
            .map(|(i, _score)| i)
            .collect();

        // Sort by score (already filtered)
        self.filtered.sort_by(|&a, &b| {
            let score_a = self.commands[a].match_score(&self.query);
            let score_b = self.commands[b].match_score(&self.query);
            score_b.cmp(&score_a)
        });

        self.selected = 0;
        if self.filtered.is_empty() {
            self.list_state.select(None);
        } else {
            self.list_state.select(Some(0));
        }
    }

    /// Move selection up
    fn select_prev(&mut self) {
        if !self.filtered.is_empty() && self.selected > 0 {
            self.selected -= 1;
            self.list_state.select(Some(self.selected));
        }
    }

    /// Move selection down
    fn select_next(&mut self) {
        if !self.filtered.is_empty() && self.selected + 1 < self.filtered.len() {
            self.selected += 1;
            self.list_state.select(Some(self.selected));
        }
    }

    /// Get the currently selected command
    pub fn selected_command(&self) -> Option<&PaletteCommand> {
        self.filtered
            .get(self.selected)
            .and_then(|&i| self.commands.get(i))
    }

    /// Calculate centered rect
    fn centered_rect(&self, area: Rect) -> Rect {
        let width = (area.width as u32 * self.width_percent as u32 / 100) as u16;

        // Calculate height based on content, but limit to max
        let content_height = 3 + self.filtered.len().min(10) as u16 + 2; // input + items + footer
        let max_height = (area.height as u32 * self.max_height_percent as u32 / 100) as u16;
        let height = content_height.min(max_height);

        let x = area.x + (area.width.saturating_sub(width)) / 2;
        let y = area.y + area.height / 6; // Position towards top

        Rect::new(x, y, width, height)
    }
}

impl Default for CommandPalette {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for CommandPalette {
    fn handle_key(&mut self, key: KeyEvent) -> Option<InputAction> {
        if !self.visible {
            // Check for activation shortcut (Ctrl+P or Ctrl+K)
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                match key.code {
                    KeyCode::Char('p') | KeyCode::Char('k') => {
                        self.show();
                        return Some(InputAction::None);
                    }
                    _ => {}
                }
            }
            return None;
        }

        match key.code {
            KeyCode::Esc => {
                self.hide();
                Some(InputAction::ExitToNormal)
            }
            KeyCode::Enter => {
                if let Some(cmd) = self.selected_command().cloned() {
                    self.hide();
                    match cmd.action {
                        PaletteAction::Navigate(screen) => Some(InputAction::Submit(format!(
                            "navigate:{}",
                            screen.title().to_lowercase()
                        ))),
                        PaletteAction::Custom(id) => {
                            Some(InputAction::Submit(format!("action:{}", id)))
                        }
                        PaletteAction::ShowHelp => {
                            Some(InputAction::Submit("navigate:help".to_string()))
                        }
                        PaletteAction::IrcCommand(cmd) => {
                            Some(InputAction::Submit(format!("irc:{}", cmd)))
                        }
                    }
                } else {
                    Some(InputAction::None)
                }
            }
            KeyCode::Up | KeyCode::BackTab => {
                self.select_prev();
                Some(InputAction::None)
            }
            KeyCode::Down | KeyCode::Tab => {
                self.select_next();
                Some(InputAction::None)
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.update_filter();
                Some(InputAction::None)
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.query.push(c);
                self.update_filter();
                Some(InputAction::None)
            }
            _ => Some(InputAction::None),
        }
    }

    fn render(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        if !self.visible {
            return;
        }

        let palette_area = self.centered_rect(area);

        // Clear background
        f.render_widget(Clear, palette_area);

        // Create layout
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Search input
                Constraint::Min(1),    // Results list
                Constraint::Length(1), // Footer
            ])
            .split(palette_area);

        // Render border block
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(styles.border_focused())
            .title(" Command Palette ");

        f.render_widget(block, palette_area);

        // Render search input
        let input_area = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([Constraint::Length(1)])
            .split(chunks[0])[0];

        let input_text = if self.query.is_empty() {
            Line::from(vec![
                Span::styled("> ", styles.text_highlight()),
                Span::styled("Type to search...", styles.text_muted()),
            ])
        } else {
            Line::from(vec![
                Span::styled("> ", styles.text_highlight()),
                Span::styled(&self.query, styles.text()),
                Span::styled("_", styles.text_highlight()),
            ])
        };

        let input = Paragraph::new(input_text);
        f.render_widget(input, input_area);

        // Render results list
        let list_area = Rect::new(
            chunks[1].x + 1,
            chunks[1].y,
            chunks[1].width.saturating_sub(2),
            chunks[1].height,
        );

        let items: Vec<ListItem> = self
            .filtered
            .iter()
            .take(10) // Show max 10 items
            .map(|&i| {
                let cmd = &self.commands[i];
                let mut spans = vec![Span::styled(&cmd.name, styles.text())];

                if let Some(ref shortcut) = cmd.shortcut {
                    spans.push(Span::raw("  "));
                    spans.push(Span::styled(format!("[{}]", shortcut), styles.text_muted()));
                }

                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(items).highlight_style(
            styles
                .text_highlight()
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::REVERSED),
        );

        let mut state = self.list_state.clone();
        f.render_stateful_widget(list, list_area, &mut state);

        // Render footer
        let footer_area = Rect::new(
            chunks[2].x + 1,
            chunks[2].y,
            chunks[2].width.saturating_sub(2),
            1,
        );

        let footer = Paragraph::new(Line::from(vec![
            Span::styled("[Enter] Select  ", styles.text_muted()),
            Span::styled("[Esc] Close  ", styles.text_muted()),
            Span::styled(
                format!("{}/{}", self.filtered.len(), self.commands.len()),
                styles.text_muted(),
            ),
        ]))
        .alignment(Alignment::Center);

        f.render_widget(footer, footer_area);
    }

    fn is_focused(&self) -> bool {
        self.visible
    }

    fn set_focused(&mut self, focused: bool) {
        if focused {
            self.show();
        } else {
            self.hide();
        }
    }

    fn min_size(&self) -> (u16, u16) {
        (40, 10)
    }

    fn is_visible(&self) -> bool {
        self.visible
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_palette_new() {
        let palette = CommandPalette::new();
        assert!(!palette.visible);
        assert!(!palette.commands.is_empty());
    }

    #[test]
    fn test_command_palette_show_hide() {
        let mut palette = CommandPalette::new();
        assert!(!palette.visible);

        palette.show();
        assert!(palette.visible);

        palette.hide();
        assert!(!palette.visible);

        palette.toggle();
        assert!(palette.visible);
    }

    #[test]
    fn test_command_matches() {
        let cmd = PaletteCommand::navigate(ScreenType::Chat);
        assert!(cmd.matches("chat"));
        assert!(cmd.matches("Chat"));
        assert!(cmd.matches("go"));
        assert!(!cmd.matches("xyz"));
    }

    #[test]
    fn test_filter_updates() {
        let mut palette = CommandPalette::new();
        let initial_count = palette.filtered.len();

        palette.query = "chat".to_string();
        palette.update_filter();

        assert!(palette.filtered.len() < initial_count);
        assert!(!palette.filtered.is_empty());
    }

    #[test]
    fn test_navigation() {
        let mut palette = CommandPalette::new();
        palette.show();

        assert_eq!(palette.selected, 0);

        palette.select_next();
        assert_eq!(palette.selected, 1);

        palette.select_prev();
        assert_eq!(palette.selected, 0);

        // Can't go below 0
        palette.select_prev();
        assert_eq!(palette.selected, 0);
    }

    #[test]
    fn test_selected_command() {
        let palette = CommandPalette::new();
        let cmd = palette.selected_command();
        assert!(cmd.is_some());
    }
}
