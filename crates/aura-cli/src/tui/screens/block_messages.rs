//! # Block Messages Screen
//!
//! Display and send messages within a block's channels.
//! Implements historical sync and multi-channel support per `work/neighbor.md`.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Wrap,
    },
    Frame,
};

use super::{Screen, ScreenType};
use crate::tui::input::InputAction;
use crate::tui::styles::Styles;

/// Message data for display
#[derive(Debug, Clone)]
pub struct BlockMessage {
    /// Message ID
    pub id: String,
    /// Sender's authority ID
    pub sender_id: String,
    /// Sender's display name (petname)
    pub sender_name: String,
    /// Message content
    pub content: String,
    /// Timestamp (ms since epoch)
    pub timestamp: u64,
    /// Whether the message is from the current user
    pub is_self: bool,
    /// Whether the message is pinned
    pub is_pinned: bool,
    /// Whether this is a system message (join/leave/etc.)
    pub is_system: bool,
}

/// Sync progress for historical messages
#[derive(Debug, Clone, Default)]
pub struct SyncProgress {
    /// Total messages to sync
    pub total: u32,
    /// Messages synced so far
    pub synced: u32,
    /// Whether sync is complete
    pub complete: bool,
    /// Whether sync failed
    pub failed: bool,
    /// Error message if failed
    pub error: Option<String>,
}

impl SyncProgress {
    /// Get sync ratio (0.0 to 1.0)
    pub fn ratio(&self) -> f64 {
        if self.total == 0 {
            return if self.complete { 1.0 } else { 0.0 };
        }
        (self.synced as f64 / self.total as f64).min(1.0)
    }
}

/// Block messages screen state
pub struct BlockMessagesScreen {
    /// Block ID
    block_id: Option<String>,
    /// Block name
    block_name: Option<String>,
    /// Current channel ID
    channel_id: Option<String>,
    /// Current channel name
    channel_name: String,
    /// List of available channels
    channels: Vec<(String, String, u32)>, // (id, name, unread_count)
    /// Messages in the current channel
    messages: Vec<BlockMessage>,
    /// Input buffer for composing messages
    input_buffer: String,
    /// Cursor position in input
    cursor_position: usize,
    /// Selected message index for scrolling
    scroll_position: usize,
    /// Historical sync progress
    sync_progress: Option<SyncProgress>,
    /// Whether input is focused (vs. message list)
    input_focused: bool,
    /// Whether channel sidebar is visible
    sidebar_visible: bool,
    /// Selected channel in sidebar
    selected_channel: usize,
    /// Sidebar list state
    sidebar_state: ListState,
    /// Flag for redraw
    needs_redraw: bool,
}

impl BlockMessagesScreen {
    /// Create a new block messages screen
    pub fn new() -> Self {
        Self {
            block_id: None,
            block_name: None,
            channel_id: None,
            channel_name: "general".to_string(),
            channels: Vec::new(),
            messages: Vec::new(),
            input_buffer: String::new(),
            cursor_position: 0,
            scroll_position: 0,
            sync_progress: None,
            input_focused: true,
            sidebar_visible: false,
            selected_channel: 0,
            sidebar_state: ListState::default(),
            needs_redraw: true,
        }
    }

    /// Set the block for this screen
    pub fn set_block(&mut self, block_id: String, name: Option<String>) {
        self.block_id = Some(block_id);
        self.block_name = name;
        self.needs_redraw = true;
    }

    /// Set the current channel
    pub fn set_channel(&mut self, channel_id: String, name: String) {
        self.channel_id = Some(channel_id);
        self.channel_name = name;
        self.messages.clear();
        self.scroll_position = 0;
        self.needs_redraw = true;
    }

    /// Set available channels
    pub fn set_channels(&mut self, channels: Vec<(String, String, u32)>) {
        self.channels = channels;
        if !self.channels.is_empty() && self.selected_channel >= self.channels.len() {
            self.selected_channel = 0;
        }
        self.sidebar_state.select(Some(self.selected_channel));
        self.needs_redraw = true;
    }

    /// Set messages for the current channel
    pub fn set_messages(&mut self, messages: Vec<BlockMessage>) {
        self.messages = messages;
        // Scroll to bottom for new messages
        if !self.messages.is_empty() {
            self.scroll_position = self.messages.len().saturating_sub(1);
        }
        self.needs_redraw = true;
    }

    /// Add a single message
    pub fn add_message(&mut self, message: BlockMessage) {
        self.messages.push(message);
        // Auto-scroll if near bottom
        if self.scroll_position >= self.messages.len().saturating_sub(5) {
            self.scroll_position = self.messages.len().saturating_sub(1);
        }
        self.needs_redraw = true;
    }

    /// Set sync progress
    pub fn set_sync_progress(&mut self, progress: Option<SyncProgress>) {
        self.sync_progress = progress;
        self.needs_redraw = true;
    }

    /// Get the current input text
    pub fn input(&self) -> &str {
        &self.input_buffer
    }

    /// Clear the input buffer
    pub fn clear_input(&mut self) {
        self.input_buffer.clear();
        self.cursor_position = 0;
        self.needs_redraw = true;
    }

    /// Scroll up in messages
    fn scroll_up(&mut self) {
        if self.scroll_position > 0 {
            self.scroll_position -= 1;
            self.needs_redraw = true;
        }
    }

    /// Scroll down in messages
    fn scroll_down(&mut self) {
        if self.scroll_position + 1 < self.messages.len() {
            self.scroll_position += 1;
            self.needs_redraw = true;
        }
    }

    /// Page up
    fn page_up(&mut self, page_size: usize) {
        self.scroll_position = self.scroll_position.saturating_sub(page_size);
        self.needs_redraw = true;
    }

    /// Page down
    fn page_down(&mut self, page_size: usize) {
        self.scroll_position =
            (self.scroll_position + page_size).min(self.messages.len().saturating_sub(1));
        self.needs_redraw = true;
    }

    /// Handle text input
    fn handle_char(&mut self, c: char) {
        self.input_buffer.insert(self.cursor_position, c);
        self.cursor_position += 1;
        self.needs_redraw = true;
    }

    /// Handle backspace
    fn handle_backspace(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
            self.input_buffer.remove(self.cursor_position);
            self.needs_redraw = true;
        }
    }

    /// Handle delete
    fn handle_delete(&mut self) {
        if self.cursor_position < self.input_buffer.len() {
            self.input_buffer.remove(self.cursor_position);
            self.needs_redraw = true;
        }
    }

    /// Move cursor left
    fn cursor_left(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
            self.needs_redraw = true;
        }
    }

    /// Move cursor right
    fn cursor_right(&mut self) {
        if self.cursor_position < self.input_buffer.len() {
            self.cursor_position += 1;
            self.needs_redraw = true;
        }
    }

    /// Move cursor to start
    fn cursor_home(&mut self) {
        self.cursor_position = 0;
        self.needs_redraw = true;
    }

    /// Move cursor to end
    fn cursor_end(&mut self) {
        self.cursor_position = self.input_buffer.len();
        self.needs_redraw = true;
    }

    /// Toggle sidebar visibility
    fn toggle_sidebar(&mut self) {
        self.sidebar_visible = !self.sidebar_visible;
        self.needs_redraw = true;
    }

    /// Select previous channel in sidebar
    fn select_prev_channel(&mut self) {
        if self.selected_channel > 0 {
            self.selected_channel -= 1;
            self.sidebar_state.select(Some(self.selected_channel));
            self.needs_redraw = true;
        }
    }

    /// Select next channel in sidebar
    fn select_next_channel(&mut self) {
        if self.selected_channel + 1 < self.channels.len() {
            self.selected_channel += 1;
            self.sidebar_state.select(Some(self.selected_channel));
            self.needs_redraw = true;
        }
    }

    /// Render the channel sidebar
    fn render_sidebar(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let block = Block::default()
            .title(" Channels ")
            .borders(Borders::ALL)
            .border_style(if !self.input_focused && self.sidebar_visible {
                styles.border_focused()
            } else {
                styles.border()
            });

        let items: Vec<ListItem> = self
            .channels
            .iter()
            .map(|(_, name, unread)| {
                let unread_text = if *unread > 0 {
                    format!(" ({})", unread)
                } else {
                    String::new()
                };

                let line = Line::from(vec![
                    Span::styled(format!("#{}", name), styles.text()),
                    Span::styled(unread_text, styles.text_warning()),
                ]);

                ListItem::new(line)
            })
            .collect();

        let list = List::new(items).block(block).highlight_style(
            Style::default()
                .add_modifier(Modifier::BOLD)
                .bg(styles.palette.surface),
        );

        let mut state = self.sidebar_state.clone();
        f.render_stateful_widget(list, area, &mut state);
    }

    /// Render the messages area
    fn render_messages(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let title = format!(
            " #{} - {} ",
            self.channel_name,
            self.block_name.as_deref().unwrap_or("Block")
        );

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(if !self.input_focused {
                styles.border_focused()
            } else {
                styles.border()
            });

        let inner = block.inner(area);
        f.render_widget(block, area);

        // Show sync progress if syncing
        if let Some(ref progress) = self.sync_progress {
            if !progress.complete {
                let sync_text = if progress.failed {
                    format!(
                        "Sync failed: {}",
                        progress.error.as_deref().unwrap_or("Unknown error")
                    )
                } else {
                    format!(
                        "Syncing history... {}/{} ({:.0}%)",
                        progress.synced,
                        progress.total,
                        progress.ratio() * 100.0
                    )
                };

                let sync_para = Paragraph::new(sync_text).style(if progress.failed {
                    styles.text_error()
                } else {
                    styles.text_muted()
                });
                f.render_widget(sync_para, inner);
                return;
            }
        }

        if self.messages.is_empty() {
            let empty = Paragraph::new("No messages yet. Be the first to say something!")
                .style(styles.text_muted())
                .wrap(Wrap { trim: true });
            f.render_widget(empty, inner);
            return;
        }

        // Calculate visible messages
        let visible_height = inner.height as usize;
        let start_idx = self
            .scroll_position
            .saturating_sub(visible_height.saturating_sub(1));
        let end_idx = (start_idx + visible_height).min(self.messages.len());

        let visible_messages: Vec<ListItem> = self.messages[start_idx..end_idx]
            .iter()
            .map(|msg| {
                let prefix = if msg.is_pinned {
                    "*"
                } else if msg.is_system {
                    ">"
                } else {
                    " "
                };

                let name_style = if msg.is_self {
                    styles.text_highlight()
                } else if msg.is_system {
                    styles.text_muted()
                } else {
                    styles.text()
                };

                let time = format_time(msg.timestamp);

                let line = Line::from(vec![
                    Span::styled(format!("{} ", prefix), styles.text_muted()),
                    Span::styled(time, styles.text_muted()),
                    Span::styled(" <", styles.text_muted()),
                    Span::styled(&msg.sender_name, name_style),
                    Span::styled("> ", styles.text_muted()),
                    Span::styled(&msg.content, styles.text()),
                ]);

                ListItem::new(line)
            })
            .collect();

        let messages_list = List::new(visible_messages);
        f.render_widget(messages_list, inner);

        // Render scrollbar if needed
        if self.messages.len() > visible_height {
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
            let mut scrollbar_state =
                ScrollbarState::new(self.messages.len()).position(self.scroll_position);
            let margin = ratatui::layout::Margin {
                horizontal: 0,
                vertical: 1,
            };
            f.render_stateful_widget(scrollbar, area.inner(&margin), &mut scrollbar_state);
        }
    }

    /// Render the input area
    fn render_input(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let block = Block::default()
            .title(" Message ")
            .borders(Borders::ALL)
            .border_style(if self.input_focused {
                styles.border_focused()
            } else {
                styles.border()
            });

        let inner = block.inner(area);
        f.render_widget(block, area);

        // Show input with cursor
        let input_text = if self.input_buffer.is_empty() {
            "Type a message... (/help for commands)"
        } else {
            &self.input_buffer
        };

        let input_style = if self.input_buffer.is_empty() {
            styles.text_muted()
        } else {
            styles.text()
        };

        let input = Paragraph::new(input_text).style(input_style);
        f.render_widget(input, inner);

        // Show cursor if input focused
        if self.input_focused {
            let cursor_x = inner.x + self.cursor_position as u16;
            let cursor_y = inner.y;
            if cursor_x < inner.x + inner.width {
                f.set_cursor(cursor_x, cursor_y);
            }
        }
    }
}

impl Default for BlockMessagesScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen for BlockMessagesScreen {
    fn screen_type(&self) -> ScreenType {
        ScreenType::Block
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<InputAction> {
        // Handle sidebar navigation when visible and not input focused
        if self.sidebar_visible && !self.input_focused {
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    self.select_next_channel();
                    return None;
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.select_prev_channel();
                    return None;
                }
                KeyCode::Enter => {
                    if let Some((id, name, _)) = self.channels.get(self.selected_channel).cloned() {
                        return Some(InputAction::Submit(format!(
                            "action:switch_channel:{}:{}",
                            id, name
                        )));
                    }
                    return None;
                }
                KeyCode::Esc => {
                    self.sidebar_visible = false;
                    self.needs_redraw = true;
                    return None;
                }
                _ => {}
            }
        }

        // Handle message scrolling when not input focused
        if !self.input_focused {
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    self.scroll_down();
                    return None;
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.scroll_up();
                    return None;
                }
                KeyCode::PageUp => {
                    self.page_up(10);
                    return None;
                }
                KeyCode::PageDown => {
                    self.page_down(10);
                    return None;
                }
                KeyCode::Char('g') => {
                    self.scroll_position = 0;
                    self.needs_redraw = true;
                    return None;
                }
                KeyCode::Char('G') => {
                    self.scroll_position = self.messages.len().saturating_sub(1);
                    self.needs_redraw = true;
                    return None;
                }
                KeyCode::Char('i') | KeyCode::Enter => {
                    self.input_focused = true;
                    self.needs_redraw = true;
                    return None;
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.toggle_sidebar();
                    return None;
                }
                _ => {}
            }
        }

        // Handle input mode
        if self.input_focused {
            match key.code {
                KeyCode::Esc => {
                    self.input_focused = false;
                    self.needs_redraw = true;
                    return None;
                }
                KeyCode::Enter => {
                    if !self.input_buffer.is_empty() {
                        let content = self.input_buffer.clone();
                        self.clear_input();

                        // Check for slash commands
                        if content.starts_with('/') {
                            return Some(InputAction::Submit(format!(
                                "action:command:{}",
                                content
                            )));
                        }

                        if let Some(ref channel_id) = self.channel_id {
                            return Some(InputAction::Submit(format!(
                                "action:send_block_message:{}:{}",
                                channel_id, content
                            )));
                        }
                    }
                    return None;
                }
                KeyCode::Backspace => {
                    self.handle_backspace();
                    return None;
                }
                KeyCode::Delete => {
                    self.handle_delete();
                    return None;
                }
                KeyCode::Left => {
                    self.cursor_left();
                    return None;
                }
                KeyCode::Right => {
                    self.cursor_right();
                    return None;
                }
                KeyCode::Home => {
                    self.cursor_home();
                    return None;
                }
                KeyCode::End => {
                    self.cursor_end();
                    return None;
                }
                KeyCode::Char(c) => {
                    self.handle_char(c);
                    return None;
                }
                KeyCode::Tab => {
                    self.toggle_sidebar();
                    return None;
                }
                _ => {}
            }
        }

        // Global shortcuts
        match key.code {
            KeyCode::F(1) => Some(InputAction::Submit("navigate:help".to_string())),
            _ => None,
        }
    }

    fn render(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        // Layout: optional sidebar on left, messages in center, input at bottom
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(5), Constraint::Length(3)])
            .split(area);

        if self.sidebar_visible {
            let content_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(20), Constraint::Min(30)])
                .split(main_chunks[0]);

            self.render_sidebar(f, content_chunks[0], styles);
            self.render_messages(f, content_chunks[1], styles);
        } else {
            self.render_messages(f, main_chunks[0], styles);
        }

        self.render_input(f, main_chunks[1], styles);
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

/// Format timestamp as HH:MM
fn format_time(ts: u64) -> String {
    let hours = (ts / 3600000) % 24;
    let minutes = (ts / 60000) % 60;
    format!("{:02}:{:02}", hours, minutes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_messages_screen_new() {
        let screen = BlockMessagesScreen::new();
        assert!(screen.messages.is_empty());
        assert!(screen.input_buffer.is_empty());
        assert!(screen.input_focused);
    }

    #[test]
    fn test_set_messages() {
        let mut screen = BlockMessagesScreen::new();
        let messages = vec![
            BlockMessage {
                id: "msg1".to_string(),
                sender_id: "auth1".to_string(),
                sender_name: "Alice".to_string(),
                content: "Hello!".to_string(),
                timestamp: 1000,
                is_self: false,
                is_pinned: false,
                is_system: false,
            },
            BlockMessage {
                id: "msg2".to_string(),
                sender_id: "auth2".to_string(),
                sender_name: "Bob".to_string(),
                content: "Hi there!".to_string(),
                timestamp: 2000,
                is_self: true,
                is_pinned: false,
                is_system: false,
            },
        ];
        screen.set_messages(messages);
        assert_eq!(screen.messages.len(), 2);
        assert_eq!(screen.scroll_position, 1); // Scrolled to bottom
    }

    #[test]
    fn test_input_handling() {
        let mut screen = BlockMessagesScreen::new();

        screen.handle_char('H');
        screen.handle_char('i');
        assert_eq!(screen.input_buffer, "Hi");
        assert_eq!(screen.cursor_position, 2);

        screen.handle_backspace();
        assert_eq!(screen.input_buffer, "H");
        assert_eq!(screen.cursor_position, 1);

        screen.cursor_left();
        assert_eq!(screen.cursor_position, 0);

        screen.handle_char('O');
        assert_eq!(screen.input_buffer, "OH");
        assert_eq!(screen.cursor_position, 1);
    }

    #[test]
    fn test_sync_progress() {
        let mut progress = SyncProgress::default();
        assert_eq!(progress.ratio(), 0.0);

        progress.total = 100;
        progress.synced = 50;
        assert_eq!(progress.ratio(), 0.5);

        progress.complete = true;
        progress.total = 0;
        assert_eq!(progress.ratio(), 1.0);
    }

    #[test]
    fn test_scrolling() {
        let mut screen = BlockMessagesScreen::new();
        let messages: Vec<BlockMessage> = (0..20)
            .map(|i| BlockMessage {
                id: format!("msg{}", i),
                sender_id: "auth".to_string(),
                sender_name: "User".to_string(),
                content: format!("Message {}", i),
                timestamp: i as u64 * 1000,
                is_self: false,
                is_pinned: false,
                is_system: false,
            })
            .collect();
        screen.set_messages(messages);

        assert_eq!(screen.scroll_position, 19);

        screen.scroll_up();
        assert_eq!(screen.scroll_position, 18);

        screen.scroll_down();
        assert_eq!(screen.scroll_position, 19);

        screen.page_up(5);
        assert_eq!(screen.scroll_position, 14);
    }

    #[test]
    fn test_channel_selection() {
        let mut screen = BlockMessagesScreen::new();
        screen.set_channels(vec![
            ("ch1".to_string(), "general".to_string(), 0),
            ("ch2".to_string(), "governance".to_string(), 3),
            ("ch3".to_string(), "events".to_string(), 1),
        ]);

        assert_eq!(screen.selected_channel, 0);

        screen.select_next_channel();
        assert_eq!(screen.selected_channel, 1);

        screen.select_prev_channel();
        assert_eq!(screen.selected_channel, 0);
    }

    #[test]
    fn test_format_time() {
        assert_eq!(format_time(0), "00:00");
        assert_eq!(format_time(3600000), "01:00");
        assert_eq!(format_time(3661000), "01:01");
        assert_eq!(format_time(86400000), "00:00"); // Wraps at 24h
    }
}
