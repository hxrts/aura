//! # Help Screen
//!
//! Displays help for IRC-style slash commands.
//! Shows all available commands organized by category.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use super::{Screen, ScreenType};
use crate::tui::commands::{all_command_help, CommandCategory, CommandHelp};
use crate::tui::input::InputAction;
use crate::tui::layout::{heights, LayoutPresets, ScreenLayout};
use crate::tui::styles::Styles;

/// Help screen state
pub struct HelpScreen {
    /// All available commands
    commands: Vec<CommandHelp>,
    /// Selected command index
    selected: usize,
    /// List state for rendering
    list_state: ListState,
    /// Current filter category
    filter: Option<CommandCategory>,
    /// Search query
    search: String,
    /// Whether search is active
    searching: bool,
    /// Flag for redraw
    needs_redraw: bool,
}

impl HelpScreen {
    /// Create a new help screen
    pub fn new() -> Self {
        let commands = all_command_help();
        let mut list_state = ListState::default();
        list_state.select(Some(0));

        Self {
            commands,
            selected: 0,
            list_state,
            filter: None,
            search: String::new(),
            searching: false,
            needs_redraw: true,
        }
    }

    /// Get filtered commands
    fn filtered_commands(&self) -> Vec<&CommandHelp> {
        self.commands
            .iter()
            .filter(|cmd| {
                // Apply category filter
                if let Some(filter) = self.filter {
                    if cmd.category != filter {
                        return false;
                    }
                }

                // Apply search filter
                if !self.search.is_empty() {
                    let search_lower = self.search.to_lowercase();
                    let name_matches = cmd.name.to_lowercase().contains(&search_lower);
                    let desc_matches = cmd.description.to_lowercase().contains(&search_lower);
                    let syntax_matches = cmd.syntax.to_lowercase().contains(&search_lower);
                    if !(name_matches || desc_matches || syntax_matches) {
                        return false;
                    }
                }

                true
            })
            .collect()
    }

    /// Get currently selected command
    fn selected_command(&self) -> Option<&CommandHelp> {
        let filtered = self.filtered_commands();
        filtered.get(self.selected).copied()
    }

    /// Move selection up
    fn select_prev(&mut self) {
        let filtered_len = self.filtered_commands().len();
        if filtered_len > 0 && self.selected > 0 {
            self.selected -= 1;
            self.list_state.select(Some(self.selected));
            self.needs_redraw = true;
        }
    }

    /// Move selection down
    fn select_next(&mut self) {
        let filtered_len = self.filtered_commands().len();
        if filtered_len > 0 && self.selected + 1 < filtered_len {
            self.selected += 1;
            self.list_state.select(Some(self.selected));
            self.needs_redraw = true;
        }
    }

    /// Cycle through category filters
    fn cycle_filter(&mut self) {
        self.filter = match self.filter {
            None => Some(CommandCategory::User),
            Some(CommandCategory::User) => Some(CommandCategory::Moderator),
            Some(CommandCategory::Moderator) => Some(CommandCategory::Admin),
            Some(CommandCategory::Admin) => None,
        };
        // Reset selection when filter changes
        self.selected = 0;
        self.list_state.select(Some(0));
        self.needs_redraw = true;
    }

    /// Toggle search mode
    fn toggle_search(&mut self) {
        self.searching = !self.searching;
        if !self.searching {
            self.search.clear();
            self.selected = 0;
            self.list_state.select(Some(0));
        }
        self.needs_redraw = true;
    }

    /// Render the command list
    fn render_list(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let filtered = self.filtered_commands();

        // Group by category
        let mut items: Vec<ListItem> = Vec::new();
        let mut current_category: Option<CommandCategory> = None;

        for (idx, cmd) in filtered.iter().enumerate() {
            // Add category header if changed
            if current_category != Some(cmd.category) {
                if current_category.is_some() {
                    items.push(ListItem::new(Line::from(""))); // Spacer
                }
                items.push(ListItem::new(Line::from(Span::styled(
                    format!("── {} ──", cmd.category.name()),
                    styles.text_highlight().add_modifier(Modifier::BOLD),
                ))));
                current_category = Some(cmd.category);
            }

            // Add command entry
            let is_selected = idx == self.selected;
            let prefix = if is_selected { "▸ " } else { "  " };

            let line = Line::from(vec![
                Span::styled(prefix, styles.text_highlight()),
                Span::styled(format!("/{}", cmd.name), styles.text_info()),
                Span::styled(" - ", styles.text_muted()),
                Span::styled(cmd.description, styles.text()),
            ]);

            items.push(ListItem::new(line));
        }

        let title = match self.filter {
            Some(filter) => format!("Commands ({})", filter.name()),
            None => "Commands (All)".to_string(),
        };

        // Use consistent panel styling from Styles
        let block = styles.panel_focused(title);

        let list = List::new(items).block(block).highlight_style(
            Style::default()
                .bg(styles.palette.surface)
                .add_modifier(Modifier::BOLD),
        );

        let mut state = self.list_state.clone();
        f.render_stateful_widget(list, area, &mut state);
    }

    /// Render the detail panel
    fn render_detail(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        // Use consistent panel styling from Styles
        let block = styles.panel("Command Details");

        let inner = block.inner(area);
        f.render_widget(block, area);

        if let Some(cmd) = self.selected_command() {
            let capability_text = if cmd.capability.as_str().is_empty() {
                "None required".to_string()
            } else {
                cmd.capability.as_str().to_string()
            };

            let lines = vec![
                Line::from(vec![
                    Span::styled("Command: ", styles.text_muted()),
                    Span::styled(
                        format!("/{}", cmd.name),
                        styles.text_info().add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("Syntax: ", styles.text_muted()),
                    Span::styled(cmd.syntax, styles.text()),
                ]),
                Line::from(""),
                Line::from(vec![Span::styled("Description: ", styles.text_muted())]),
                Line::from(Span::styled(cmd.description, styles.text())),
                Line::from(""),
                Line::from(vec![
                    Span::styled("Category: ", styles.text_muted()),
                    Span::styled(cmd.category.name(), styles.text_highlight()),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("Capability: ", styles.text_muted()),
                    Span::styled(capability_text, styles.text_warning()),
                ]),
            ];

            let detail = Paragraph::new(lines).wrap(Wrap { trim: true });
            f.render_widget(detail, inner);
        } else {
            let empty = Paragraph::new("No command selected").style(styles.text_muted());
            f.render_widget(empty, inner);
        }
    }

    /// Render the search/filter bar
    fn render_footer(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        // Use consistent footer panel styling from Styles
        let block = styles.panel_footer();
        let inner = block.inner(area);
        f.render_widget(block, area);

        let mut spans = vec![];

        if self.searching {
            spans.push(Span::styled("Search: ", styles.text_muted()));
            spans.push(Span::styled(&self.search, styles.text()));
            spans.push(Span::styled("█", styles.text_highlight())); // Cursor
        } else {
            spans.push(Span::styled("[/] Search  ", styles.text_muted()));
            spans.push(Span::styled("[Tab] Filter  ", styles.text_muted()));
            spans.push(Span::styled("[↑↓] Navigate  ", styles.text_muted()));
            spans.push(Span::styled("[Esc] Close", styles.text_muted()));
        }

        let footer = Paragraph::new(Line::from(spans)).style(styles.text());
        f.render_widget(footer, inner);
    }
}

impl Default for HelpScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen for HelpScreen {
    fn screen_type(&self) -> ScreenType {
        ScreenType::Help
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<InputAction> {
        if self.searching {
            match key.code {
                KeyCode::Esc => {
                    self.toggle_search();
                    None
                }
                KeyCode::Enter => {
                    self.searching = false;
                    self.needs_redraw = true;
                    None
                }
                KeyCode::Backspace => {
                    self.search.pop();
                    self.selected = 0;
                    self.list_state.select(Some(0));
                    self.needs_redraw = true;
                    None
                }
                KeyCode::Char(c) => {
                    self.search.push(c);
                    self.selected = 0;
                    self.list_state.select(Some(0));
                    self.needs_redraw = true;
                    None
                }
                _ => None,
            }
        } else {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => Some(InputAction::Submit("pop".to_string())),
                KeyCode::Char('j') | KeyCode::Down => {
                    self.select_next();
                    None
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.select_prev();
                    None
                }
                KeyCode::Tab => {
                    self.cycle_filter();
                    None
                }
                KeyCode::Char('/') => {
                    self.toggle_search();
                    None
                }
                KeyCode::Char('?') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                    // Shift+? (question mark) closes help
                    Some(InputAction::Submit("pop".to_string()))
                }
                KeyCode::Home => {
                    self.selected = 0;
                    self.list_state.select(Some(0));
                    self.needs_redraw = true;
                    None
                }
                KeyCode::End => {
                    let filtered_len = self.filtered_commands().len();
                    if filtered_len > 0 {
                        self.selected = filtered_len - 1;
                        self.list_state.select(Some(self.selected));
                        self.needs_redraw = true;
                    }
                    None
                }
                KeyCode::Char('1') => {
                    self.filter = Some(CommandCategory::User);
                    self.selected = 0;
                    self.list_state.select(Some(0));
                    self.needs_redraw = true;
                    None
                }
                KeyCode::Char('2') => {
                    self.filter = Some(CommandCategory::Moderator);
                    self.selected = 0;
                    self.list_state.select(Some(0));
                    self.needs_redraw = true;
                    None
                }
                KeyCode::Char('3') => {
                    self.filter = Some(CommandCategory::Admin);
                    self.selected = 0;
                    self.list_state.select(Some(0));
                    self.needs_redraw = true;
                    None
                }
                KeyCode::Char('0') => {
                    self.filter = None;
                    self.selected = 0;
                    self.list_state.select(Some(0));
                    self.needs_redraw = true;
                    None
                }
                _ => None,
            }
        }
    }

    fn render(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        // Layout using consistent grid system: main content + footer
        let main_chunks = ScreenLayout::new()
            .flexible(10) // Main content (min 10 rows)
            .fixed(heights::COMPACT) // Footer/shortcuts (3 rows)
            .build(area);

        // Split content into list + details using standard LIST_DETAIL split (40/60)
        let content_chunks = LayoutPresets::list_detail(main_chunks[0]);

        self.render_list(f, content_chunks[0], styles);
        self.render_detail(f, content_chunks[1], styles);
        self.render_footer(f, main_chunks[1], styles);
    }

    fn on_enter(&mut self) {
        // Refresh commands and reset state
        self.commands = all_command_help();
        self.selected = 0;
        self.list_state.select(Some(0));
        self.filter = None;
        self.search.clear();
        self.searching = false;
        self.needs_redraw = true;
    }

    fn needs_redraw(&self) -> bool {
        self.needs_redraw
    }

    fn update(&mut self) {
        self.needs_redraw = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_help_screen_new() {
        let screen = HelpScreen::new();
        assert_eq!(screen.screen_type(), ScreenType::Help);
        assert!(!screen.commands.is_empty());
    }

    #[test]
    fn test_filter_by_category() {
        let mut screen = HelpScreen::new();

        // No filter - all commands
        let all_count = screen.filtered_commands().len();
        assert!(all_count > 0);

        // Filter to User commands
        screen.filter = Some(CommandCategory::User);
        let user_count = screen.filtered_commands().len();
        assert!(user_count > 0);
        assert!(user_count < all_count);

        // All filtered commands should be User category
        for cmd in screen.filtered_commands() {
            assert_eq!(cmd.category, CommandCategory::User);
        }
    }

    #[test]
    fn test_search_filter() {
        let mut screen = HelpScreen::new();

        // Search for "kick"
        screen.search = "kick".to_string();
        let filtered = screen.filtered_commands();
        assert!(!filtered.is_empty());

        // Should find the kick command
        assert!(filtered.iter().any(|cmd| cmd.name == "kick"));
    }

    #[test]
    fn test_navigation() {
        let mut screen = HelpScreen::new();

        assert_eq!(screen.selected, 0);

        screen.select_next();
        assert_eq!(screen.selected, 1);

        screen.select_prev();
        assert_eq!(screen.selected, 0);

        // Can't go below 0
        screen.select_prev();
        assert_eq!(screen.selected, 0);
    }

    #[test]
    fn test_cycle_filter() {
        let mut screen = HelpScreen::new();

        assert!(screen.filter.is_none());

        screen.cycle_filter();
        assert_eq!(screen.filter, Some(CommandCategory::User));

        screen.cycle_filter();
        assert_eq!(screen.filter, Some(CommandCategory::Moderator));

        screen.cycle_filter();
        assert_eq!(screen.filter, Some(CommandCategory::Admin));

        screen.cycle_filter();
        assert!(screen.filter.is_none());
    }

    #[test]
    fn test_selected_command() {
        let screen = HelpScreen::new();
        let cmd = screen.selected_command();
        assert!(cmd.is_some());
    }
}
