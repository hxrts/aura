//! # Block Screen
//!
//! Display and manage block membership, residents, and storage.
//! Implements the urban social topology from `work/neighbor.md`.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use super::{Screen, ScreenType};
use crate::tui::input::InputAction;
use crate::tui::styles::Styles;

/// Resident display data
#[derive(Debug, Clone)]
pub struct Resident {
    /// Authority ID of the resident
    pub authority_id: String,
    /// Display name (petname)
    pub name: String,
    /// Whether this resident is a steward
    pub is_steward: bool,
    /// When the resident joined
    pub joined_at: u64,
    /// Storage allocated by this resident
    pub storage_allocated: u64,
    /// Last seen timestamp
    pub last_seen: Option<u64>,
    /// Whether this is the current user
    pub is_self: bool,
}

/// Message channel within the block
#[derive(Debug, Clone)]
pub struct MessageChannel {
    /// Channel ID
    pub id: String,
    /// Channel name
    pub name: String,
    /// Unread message count
    pub unread_count: u32,
    /// Whether this channel is the default
    pub is_default: bool,
}

/// Block storage budget display
#[derive(Debug, Clone, Default)]
pub struct BlockStorageBudgetView {
    /// Total storage limit (10 MB)
    pub total_limit: u64,
    /// Storage used by residents
    pub resident_spent: u64,
    /// Storage used by pinned content
    pub pinned_spent: u64,
    /// Storage donated to neighborhoods
    pub neighborhood_donated: u64,
}

impl BlockStorageBudgetView {
    /// Default storage limit: 10 MB
    pub const DEFAULT_TOTAL: u64 = 10 * 1024 * 1024;
    /// Default resident allocation: 200 KB
    pub const RESIDENT_ALLOCATION: u64 = 200 * 1024;
    /// Default neighborhood donation: 1 MB per neighborhood
    pub const NEIGHBORHOOD_DONATION: u64 = 1024 * 1024;

    /// Calculate remaining public-good space
    pub fn remaining(&self) -> u64 {
        self.total_limit
            .saturating_sub(self.resident_spent + self.pinned_spent + self.neighborhood_donated)
    }

    /// Get usage as a fraction (0.0 to 1.0)
    pub fn usage_ratio(&self) -> f64 {
        if self.total_limit == 0 {
            return 0.0;
        }
        let used = self.resident_spent + self.pinned_spent + self.neighborhood_donated;
        (used as f64 / self.total_limit as f64).min(1.0)
    }

    /// Format bytes as human-readable string
    pub fn format_bytes(bytes: u64) -> String {
        if bytes >= 1024 * 1024 {
            format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
        } else if bytes >= 1024 {
            format!("{:.1} KB", bytes as f64 / 1024.0)
        } else {
            format!("{} B", bytes)
        }
    }
}

/// Block screen state
pub struct BlockScreen {
    /// Block ID being displayed
    block_id: Option<String>,
    /// Block name (if set)
    block_name: Option<String>,
    /// List of residents
    residents: Vec<Resident>,
    /// Selected resident index
    selected_resident: Option<usize>,
    /// List state for rendering
    list_state: ListState,
    /// Message channels in this block
    channels: Vec<MessageChannel>,
    /// Selected channel index
    selected_channel: usize,
    /// Storage budget
    storage: BlockStorageBudgetView,
    /// Number of neighborhoods this block belongs to
    neighborhood_count: u8,
    /// Whether the current user is a resident
    is_resident: bool,
    /// Whether the current user is a steward
    is_steward: bool,
    /// Which panel is focused (0=residents, 1=channels, 2=storage)
    focused_panel: usize,
    /// Flag for redraw
    needs_redraw: bool,
}

impl BlockScreen {
    /// Create a new block screen
    pub fn new() -> Self {
        Self {
            block_id: None,
            block_name: None,
            residents: Vec::new(),
            selected_resident: None,
            list_state: ListState::default(),
            channels: Vec::new(),
            selected_channel: 0,
            storage: BlockStorageBudgetView {
                total_limit: BlockStorageBudgetView::DEFAULT_TOTAL,
                ..Default::default()
            },
            neighborhood_count: 0,
            is_resident: false,
            is_steward: false,
            focused_panel: 0,
            needs_redraw: true,
        }
    }

    /// Set the block being displayed
    pub fn set_block(&mut self, block_id: String, name: Option<String>) {
        self.block_id = Some(block_id);
        self.block_name = name;
        self.needs_redraw = true;
    }

    /// Set the residents list
    pub fn set_residents(&mut self, residents: Vec<Resident>) {
        self.residents = residents;
        if self.selected_resident.is_none() && !self.residents.is_empty() {
            self.selected_resident = Some(0);
            self.list_state.select(Some(0));
        }
        self.needs_redraw = true;
    }

    /// Set message channels
    pub fn set_channels(&mut self, channels: Vec<MessageChannel>) {
        self.channels = channels;
        if !self.channels.is_empty() && self.selected_channel >= self.channels.len() {
            self.selected_channel = 0;
        }
        self.needs_redraw = true;
    }

    /// Set storage budget
    pub fn set_storage(&mut self, storage: BlockStorageBudgetView) {
        self.storage = storage;
        self.needs_redraw = true;
    }

    /// Set neighborhood count
    pub fn set_neighborhood_count(&mut self, count: u8) {
        self.neighborhood_count = count;
        self.storage.neighborhood_donated =
            count as u64 * BlockStorageBudgetView::NEIGHBORHOOD_DONATION;
        self.needs_redraw = true;
    }

    /// Set whether current user is a resident
    pub fn set_is_resident(&mut self, is_resident: bool) {
        self.is_resident = is_resident;
        self.needs_redraw = true;
    }

    /// Set whether current user is a steward
    pub fn set_is_steward(&mut self, is_steward: bool) {
        self.is_steward = is_steward;
        self.needs_redraw = true;
    }

    /// Get selected resident
    pub fn selected_resident(&self) -> Option<&Resident> {
        self.selected_resident
            .and_then(|idx| self.residents.get(idx))
    }

    /// Get selected channel
    pub fn selected_channel(&self) -> Option<&MessageChannel> {
        self.channels.get(self.selected_channel)
    }

    /// Move selection up in current panel
    fn select_prev(&mut self) {
        match self.focused_panel {
            0 => {
                if let Some(selected) = self.selected_resident {
                    if selected > 0 {
                        self.selected_resident = Some(selected - 1);
                        self.list_state.select(Some(selected - 1));
                        self.needs_redraw = true;
                    }
                }
            }
            1 => {
                if self.selected_channel > 0 {
                    self.selected_channel -= 1;
                    self.needs_redraw = true;
                }
            }
            _ => {}
        }
    }

    /// Move selection down in current panel
    fn select_next(&mut self) {
        match self.focused_panel {
            0 => {
                if let Some(selected) = self.selected_resident {
                    if selected + 1 < self.residents.len() {
                        self.selected_resident = Some(selected + 1);
                        self.list_state.select(Some(selected + 1));
                        self.needs_redraw = true;
                    }
                }
            }
            1 => {
                if self.selected_channel + 1 < self.channels.len() {
                    self.selected_channel += 1;
                    self.needs_redraw = true;
                }
            }
            _ => {}
        }
    }

    /// Switch to next panel
    fn next_panel(&mut self) {
        self.focused_panel = (self.focused_panel + 1) % 3;
        self.needs_redraw = true;
    }

    /// Render the block header
    fn render_header(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let block = Block::default()
            .title(format!(
                " {} ",
                self.block_name
                    .as_deref()
                    .unwrap_or_else(|| self.block_id.as_deref().unwrap_or("Block"))
            ))
            .borders(Borders::ALL)
            .border_style(styles.border());

        let inner = block.inner(area);
        f.render_widget(block, area);

        let status = if self.is_steward {
            ("Steward", styles.text_success())
        } else if self.is_resident {
            ("Resident", styles.text_highlight())
        } else {
            ("Visitor", styles.text_muted())
        };

        let lines = vec![
            Line::from(vec![
                Span::styled("Status: ", styles.text_muted()),
                Span::styled(status.0, status.1),
            ]),
            Line::from(vec![
                Span::styled("Residents: ", styles.text_muted()),
                Span::styled(format!("{}/8", self.residents.len()), styles.text()),
            ]),
            Line::from(vec![
                Span::styled("Neighborhoods: ", styles.text_muted()),
                Span::styled(format!("{}/4", self.neighborhood_count), styles.text()),
            ]),
        ];

        let header = Paragraph::new(lines).alignment(Alignment::Center);
        f.render_widget(header, inner);
    }

    /// Render the residents list
    fn render_residents(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let block = Block::default()
            .title(" Residents ")
            .borders(Borders::ALL)
            .border_style(if self.focused_panel == 0 {
                styles.border_focused()
            } else {
                styles.border()
            });

        let items: Vec<ListItem> = self
            .residents
            .iter()
            .map(|resident| {
                let (icon, icon_style) = if resident.is_steward {
                    ("*", styles.text_success())
                } else if resident.is_self {
                    (">", styles.text_highlight())
                } else {
                    (" ", styles.text())
                };

                let online_indicator = if resident.last_seen.is_some() {
                    Span::styled(" ", styles.text_success())
                } else {
                    Span::styled(" ", styles.text_muted())
                };

                let line = Line::from(vec![
                    Span::styled(format!("{} ", icon), icon_style),
                    Span::styled(&resident.name, styles.text()),
                    online_indicator,
                ]);

                ListItem::new(line)
            })
            .collect();

        let list = List::new(items).block(block).highlight_style(
            Style::default()
                .add_modifier(Modifier::BOLD)
                .bg(styles.palette.surface),
        );

        let mut state = self.list_state.clone();
        f.render_stateful_widget(list, area, &mut state);
    }

    /// Render the channels list
    fn render_channels(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let block = Block::default()
            .title(" Channels ")
            .borders(Borders::ALL)
            .border_style(if self.focused_panel == 1 {
                styles.border_focused()
            } else {
                styles.border()
            });

        let inner = block.inner(area);
        f.render_widget(block, area);

        if self.channels.is_empty() {
            let empty = Paragraph::new("No channels").style(styles.text_muted());
            f.render_widget(empty, inner);
            return;
        }

        let mut lines = Vec::new();
        for (i, channel) in self.channels.iter().enumerate() {
            let selected = i == self.selected_channel && self.focused_panel == 1;
            let prefix = if selected { "> " } else { "  " };

            let unread = if channel.unread_count > 0 {
                format!(" ({})", channel.unread_count)
            } else {
                String::new()
            };

            let style = if selected {
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .bg(styles.palette.surface)
            } else {
                Style::default()
            };

            lines.push(Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(format!("#{}", channel.name), style),
                Span::styled(unread, styles.text_warning()),
            ]));
        }

        let channels = Paragraph::new(lines);
        f.render_widget(channels, inner);
    }

    /// Render the storage budget
    fn render_storage(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let block = Block::default()
            .title(" Storage Budget ")
            .borders(Borders::ALL)
            .border_style(if self.focused_panel == 2 {
                styles.border_focused()
            } else {
                styles.border()
            });

        let inner = block.inner(area);
        f.render_widget(block, area);

        // Split into gauge and breakdown
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(1)])
            .split(inner);

        // Usage gauge
        let usage = self.storage.usage_ratio();
        let remaining = self.storage.remaining();
        let gauge = Gauge::default()
            .ratio(usage)
            .label(format!(
                "{} remaining",
                BlockStorageBudgetView::format_bytes(remaining)
            ))
            .gauge_style(
                Style::default()
                    .fg(if usage > 0.9 {
                        styles.palette.error
                    } else if usage > 0.7 {
                        styles.palette.warning
                    } else {
                        styles.palette.primary
                    })
                    .add_modifier(Modifier::BOLD),
            );
        f.render_widget(gauge, chunks[0]);

        // Breakdown
        let breakdown = vec![
            Line::from(vec![
                Span::styled("Residents:     ", styles.text_muted()),
                Span::styled(
                    BlockStorageBudgetView::format_bytes(self.storage.resident_spent),
                    styles.text(),
                ),
            ]),
            Line::from(vec![
                Span::styled("Pinned:        ", styles.text_muted()),
                Span::styled(
                    BlockStorageBudgetView::format_bytes(self.storage.pinned_spent),
                    styles.text(),
                ),
            ]),
            Line::from(vec![
                Span::styled("Neighborhoods: ", styles.text_muted()),
                Span::styled(
                    BlockStorageBudgetView::format_bytes(self.storage.neighborhood_donated),
                    styles.text(),
                ),
            ]),
        ];

        let breakdown_para = Paragraph::new(breakdown);
        f.render_widget(breakdown_para, chunks[1]);
    }

    /// Render actions based on current state
    fn render_actions(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let block = Block::default()
            .title(" Actions ")
            .borders(Borders::ALL)
            .border_style(styles.border());

        let inner = block.inner(area);
        f.render_widget(block, area);

        let mut actions = Vec::new();

        if !self.is_resident {
            actions.push(Span::styled("[J] ", styles.text_highlight()));
            actions.push(Span::styled("Join Block  ", styles.text()));
        } else {
            actions.push(Span::styled("[L] ", styles.text_warning()));
            actions.push(Span::styled("Leave Block  ", styles.text()));
        }

        if self.is_resident {
            actions.push(Span::styled("[Enter] ", styles.text_highlight()));
            actions.push(Span::styled("Open Channel  ", styles.text()));
        }

        if self.is_steward {
            actions.push(Span::styled("[I] ", styles.text_success()));
            actions.push(Span::styled("Invite  ", styles.text()));
            actions.push(Span::styled("[M] ", styles.text_success()));
            actions.push(Span::styled("Moderate  ", styles.text()));
        }

        if actions.is_empty() {
            actions.push(Span::styled("No actions available", styles.text_muted()));
        }

        let action_line = Line::from(actions);
        let actions_para = Paragraph::new(action_line).wrap(Wrap { trim: true });
        f.render_widget(actions_para, inner);
    }
}

impl Default for BlockScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen for BlockScreen {
    fn screen_type(&self) -> ScreenType {
        ScreenType::Block
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<InputAction> {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.select_next();
                None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.select_prev();
                None
            }
            KeyCode::Tab => {
                self.next_panel();
                None
            }
            KeyCode::Char('J') if !self.is_resident => {
                if let Some(ref block_id) = self.block_id {
                    return Some(InputAction::Submit(format!(
                        "action:join_block:{}",
                        block_id
                    )));
                }
                None
            }
            KeyCode::Char('L') if self.is_resident => {
                if let Some(ref block_id) = self.block_id {
                    return Some(InputAction::Submit(format!(
                        "action:leave_block:{}",
                        block_id
                    )));
                }
                None
            }
            KeyCode::Char('I') | KeyCode::Char('i') if self.is_steward => {
                if let Some(ref block_id) = self.block_id {
                    return Some(InputAction::Submit(format!(
                        "action:invite_to_block:{}",
                        block_id
                    )));
                }
                None
            }
            KeyCode::Char('M') | KeyCode::Char('m') if self.is_steward => {
                if let Some(ref block_id) = self.block_id {
                    return Some(InputAction::Submit(format!(
                        "action:moderate_block:{}",
                        block_id
                    )));
                }
                None
            }
            KeyCode::Enter if self.is_resident && self.focused_panel == 1 => {
                if let Some(channel) = self.selected_channel() {
                    return Some(InputAction::Submit(format!(
                        "action:open_channel:{}",
                        channel.id
                    )));
                }
                None
            }
            KeyCode::Enter if self.focused_panel == 0 => {
                if let Some(resident) = self.selected_resident() {
                    return Some(InputAction::Submit(format!(
                        "action:view_resident:{}",
                        resident.authority_id
                    )));
                }
                None
            }
            _ => None,
        }
    }

    fn render(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        // Layout: header, main content (residents + channels + storage), actions
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(6), // Header
                Constraint::Min(10),   // Main content
                Constraint::Length(3), // Actions
            ])
            .split(area);

        self.render_header(f, main_chunks[0], styles);

        // Main content: residents on left, channels and storage on right
        let content_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(main_chunks[1]);

        self.render_residents(f, content_chunks[0], styles);

        // Right side: channels and storage stacked
        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(content_chunks[1]);

        self.render_channels(f, right_chunks[0], styles);
        self.render_storage(f, right_chunks[1], styles);

        self.render_actions(f, main_chunks[2], styles);
    }

    fn on_enter(&mut self) {
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
    fn test_block_screen_new() {
        let screen = BlockScreen::new();
        assert_eq!(screen.screen_type(), ScreenType::Block);
        assert!(screen.block_id.is_none());
        assert!(screen.residents.is_empty());
    }

    #[test]
    fn test_set_block() {
        let mut screen = BlockScreen::new();
        screen.set_block("block123".to_string(), Some("My Block".to_string()));
        assert_eq!(screen.block_id, Some("block123".to_string()));
        assert_eq!(screen.block_name, Some("My Block".to_string()));
    }

    #[test]
    fn test_set_residents() {
        let mut screen = BlockScreen::new();
        let residents = vec![
            Resident {
                authority_id: "auth1".to_string(),
                name: "Alice".to_string(),
                is_steward: true,
                joined_at: 1000,
                storage_allocated: 204800,
                last_seen: Some(2000),
                is_self: false,
            },
            Resident {
                authority_id: "auth2".to_string(),
                name: "Bob".to_string(),
                is_steward: false,
                joined_at: 1500,
                storage_allocated: 204800,
                last_seen: None,
                is_self: true,
            },
        ];
        screen.set_residents(residents);
        assert_eq!(screen.residents.len(), 2);
        assert_eq!(screen.selected_resident, Some(0));
    }

    #[test]
    fn test_storage_budget() {
        let mut budget = BlockStorageBudgetView::default();
        budget.total_limit = BlockStorageBudgetView::DEFAULT_TOTAL;
        budget.resident_spent = 8 * BlockStorageBudgetView::RESIDENT_ALLOCATION;
        budget.neighborhood_donated = 4 * BlockStorageBudgetView::NEIGHBORHOOD_DONATION;

        // 10 MB - 1.6 MB (residents) - 4 MB (neighborhoods) = ~4.4 MB
        let remaining = budget.remaining();
        assert!(remaining > 4 * 1024 * 1024);
        assert!(remaining < 5 * 1024 * 1024);
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(BlockStorageBudgetView::format_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(BlockStorageBudgetView::format_bytes(512 * 1024), "512.0 KB");
        assert_eq!(BlockStorageBudgetView::format_bytes(100), "100 B");
    }

    #[test]
    fn test_navigation() {
        let mut screen = BlockScreen::new();
        let residents = vec![
            Resident {
                authority_id: "auth1".to_string(),
                name: "Alice".to_string(),
                is_steward: false,
                joined_at: 0,
                storage_allocated: 0,
                last_seen: None,
                is_self: false,
            },
            Resident {
                authority_id: "auth2".to_string(),
                name: "Bob".to_string(),
                is_steward: false,
                joined_at: 0,
                storage_allocated: 0,
                last_seen: None,
                is_self: false,
            },
        ];
        screen.set_residents(residents);

        assert_eq!(screen.selected_resident, Some(0));
        screen.select_next();
        assert_eq!(screen.selected_resident, Some(1));
        screen.select_prev();
        assert_eq!(screen.selected_resident, Some(0));
    }

    #[test]
    fn test_panel_focus() {
        let mut screen = BlockScreen::new();
        assert_eq!(screen.focused_panel, 0);
        screen.next_panel();
        assert_eq!(screen.focused_panel, 1);
        screen.next_panel();
        assert_eq!(screen.focused_panel, 2);
        screen.next_panel();
        assert_eq!(screen.focused_panel, 0);
    }
}
