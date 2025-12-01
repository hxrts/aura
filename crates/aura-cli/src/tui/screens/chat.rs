//! # Chat Screen
//!
//! Full-screen chat interface with channels, messages, and input.
//! Connects to ChatView for reactive data updates.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use super::{Screen, ScreenType};
use crate::tui::input::InputAction;
use crate::tui::layout::{widths, LayoutPresets};
use crate::tui::reactive::{Channel, Message};
use crate::tui::styles::Styles;

/// Chat screen state
pub struct ChatScreen {
    /// List of channels
    channels: Vec<Channel>,
    /// Currently selected channel index
    selected_channel: Option<usize>,
    /// Messages in current channel
    messages: Vec<Message>,
    /// Message scroll offset
    message_scroll: usize,
    /// Channel list state for rendering
    channel_list_state: ListState,
    /// Whether channel pane is focused
    channel_focused: bool,
    /// Input buffer for composing messages
    input_buffer: String,
    /// Whether input is active
    input_active: bool,
    /// Cursor position in input
    cursor_pos: usize,
    /// Flag for redraw
    needs_redraw: bool,
}

impl ChatScreen {
    /// Create a new chat screen
    pub fn new() -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));

        Self {
            channels: Vec::new(),
            selected_channel: Some(0),
            messages: Vec::new(),
            message_scroll: 0,
            channel_list_state: list_state,
            channel_focused: true,
            input_buffer: String::new(),
            input_active: false,
            cursor_pos: 0,
            needs_redraw: true,
        }
    }

    /// Set channels from view data
    pub fn set_channels(&mut self, channels: Vec<Channel>) {
        self.channels = channels;
        if self.selected_channel.is_none() && !self.channels.is_empty() {
            self.selected_channel = Some(0);
            self.channel_list_state.select(Some(0));
        }
        self.needs_redraw = true;
    }

    /// Set messages for current channel
    pub fn set_messages(&mut self, messages: Vec<Message>) {
        self.messages = messages;
        // Auto-scroll to bottom for new messages
        if !self.messages.is_empty() {
            self.message_scroll = self.messages.len().saturating_sub(1);
        }
        self.needs_redraw = true;
    }

    /// Get the currently selected channel
    pub fn selected_channel(&self) -> Option<&Channel> {
        self.selected_channel.and_then(|idx| self.channels.get(idx))
    }

    /// Get the input buffer content
    pub fn input_content(&self) -> &str {
        &self.input_buffer
    }

    /// Clear the input buffer
    pub fn clear_input(&mut self) {
        self.input_buffer.clear();
        self.cursor_pos = 0;
        self.needs_redraw = true;
    }

    /// Move channel selection up
    fn select_prev_channel(&mut self) {
        if let Some(selected) = self.selected_channel {
            if selected > 0 {
                self.selected_channel = Some(selected - 1);
                self.channel_list_state.select(Some(selected - 1));
                self.needs_redraw = true;
            }
        }
    }

    /// Move channel selection down
    fn select_next_channel(&mut self) {
        if let Some(selected) = self.selected_channel {
            if selected + 1 < self.channels.len() {
                self.selected_channel = Some(selected + 1);
                self.channel_list_state.select(Some(selected + 1));
                self.needs_redraw = true;
            }
        }
    }

    /// Scroll messages up
    fn scroll_up(&mut self) {
        if self.message_scroll > 0 {
            self.message_scroll -= 1;
            self.needs_redraw = true;
        }
    }

    /// Scroll messages down
    fn scroll_down(&mut self) {
        if self.message_scroll + 1 < self.messages.len() {
            self.message_scroll += 1;
            self.needs_redraw = true;
        }
    }

    /// Handle input in compose mode
    fn handle_input_key(&mut self, key: KeyEvent) -> Option<InputAction> {
        match key.code {
            KeyCode::Esc => {
                self.input_active = false;
                self.needs_redraw = true;
                None
            }
            KeyCode::Enter => {
                if !self.input_buffer.is_empty() {
                    let content = self.input_buffer.clone();
                    self.clear_input();
                    self.input_active = false;

                    // Check if input is an IRC command
                    if crate::tui::commands::is_command(&content) {
                        match crate::tui::commands::parse_command(&content) {
                            Ok(cmd) => Some(InputAction::Command(cmd)),
                            Err(e) => Some(InputAction::Error(e.to_string())),
                        }
                    } else {
                        // Regular message
                        Some(InputAction::Submit(content))
                    }
                } else {
                    self.input_active = false;
                    self.needs_redraw = true;
                    None
                }
            }
            KeyCode::Char(c) => {
                self.input_buffer.insert(self.cursor_pos, c);
                self.cursor_pos += 1;
                self.needs_redraw = true;
                None
            }
            KeyCode::Backspace => {
                if self.cursor_pos > 0 {
                    self.cursor_pos -= 1;
                    self.input_buffer.remove(self.cursor_pos);
                    self.needs_redraw = true;
                }
                None
            }
            KeyCode::Delete => {
                if self.cursor_pos < self.input_buffer.len() {
                    self.input_buffer.remove(self.cursor_pos);
                    self.needs_redraw = true;
                }
                None
            }
            KeyCode::Left => {
                if self.cursor_pos > 0 {
                    self.cursor_pos -= 1;
                    self.needs_redraw = true;
                }
                None
            }
            KeyCode::Right => {
                if self.cursor_pos < self.input_buffer.len() {
                    self.cursor_pos += 1;
                    self.needs_redraw = true;
                }
                None
            }
            KeyCode::Home => {
                self.cursor_pos = 0;
                self.needs_redraw = true;
                None
            }
            KeyCode::End => {
                self.cursor_pos = self.input_buffer.len();
                self.needs_redraw = true;
                None
            }
            _ => None,
        }
    }

    /// Render the channel list
    fn render_channels(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let items: Vec<ListItem> = self
            .channels
            .iter()
            .map(|channel| {
                let prefix = if channel.is_dm { "◯" } else { "◈" };

                let unread_indicator = if channel.unread_count > 0 {
                    format!(" ({})", channel.unread_count)
                } else {
                    String::new()
                };

                let line = Line::from(vec![
                    Span::styled(format!("{} ", prefix), styles.text_muted()),
                    Span::styled(&channel.name, styles.text()),
                    Span::styled(unread_indicator, styles.text_highlight()),
                ]);

                ListItem::new(line)
            })
            .collect();

        // Use consistent panel styling from Styles
        let block = if self.channel_focused {
            styles.panel_focused("Channels")
        } else {
            styles.panel_sidebar("Channels")
        };

        let list = List::new(items)
            .block(block)
            .highlight_style(styles.list_item_selected());

        let mut state = self.channel_list_state.clone();
        f.render_stateful_widget(list, area, &mut state);
    }

    /// Render the message area
    fn render_messages(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let title = self
            .selected_channel()
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "Messages".to_string());

        // Use consistent panel styling from Styles
        let block = if !self.channel_focused && !self.input_active {
            styles.panel_focused(title)
        } else {
            styles.panel(title)
        };

        let inner = block.inner(area);
        f.render_widget(block, area);

        if self.messages.is_empty() {
            let empty = Paragraph::new("No messages yet")
                .style(styles.text_muted())
                .wrap(Wrap { trim: true });
            f.render_widget(empty, inner);
            return;
        }

        // Calculate visible messages
        let visible_height = inner.height as usize;
        let start = self.message_scroll.saturating_sub(visible_height / 2);
        let end = (start + visible_height).min(self.messages.len());

        let message_lines: Vec<Line> = self.messages[start..end]
            .iter()
            .flat_map(|msg| {
                vec![
                    Line::from(vec![
                        Span::styled(&msg.sender_name, styles.text_highlight()),
                        Span::styled(
                            format!("  {}", format_timestamp(msg.timestamp)),
                            styles.text_muted(),
                        ),
                    ]),
                    Line::from(Span::styled(&msg.content, styles.text())),
                    Line::from(""),
                ]
            })
            .collect();

        let messages = Paragraph::new(message_lines).wrap(Wrap { trim: true });
        f.render_widget(messages, inner);
    }

    /// Render the input area
    fn render_input(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        // Use consistent input panel styling from Styles
        let block = styles.panel_input("Compose", self.input_active);

        let inner = block.inner(area);
        f.render_widget(block, area);

        if self.input_active {
            // Show input with cursor
            let before_cursor = &self.input_buffer[..self.cursor_pos];
            let cursor_char = self
                .input_buffer
                .chars()
                .nth(self.cursor_pos)
                .map(|c| c.to_string())
                .unwrap_or_else(|| " ".to_string());
            let after_cursor = if self.cursor_pos < self.input_buffer.len() {
                &self.input_buffer[self.cursor_pos + 1..]
            } else {
                ""
            };

            let text = Line::from(vec![
                Span::raw(before_cursor),
                Span::styled(
                    cursor_char,
                    Style::default()
                        .bg(styles.palette.text_primary)
                        .fg(styles.palette.background),
                ),
                Span::raw(after_cursor),
            ]);

            let input = Paragraph::new(text);
            f.render_widget(input, inner);
        } else {
            let hint = Paragraph::new("Press 'i' to compose a message").style(styles.text_muted());
            f.render_widget(hint, inner);
        }
    }
}

impl Default for ChatScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen for ChatScreen {
    fn screen_type(&self) -> ScreenType {
        ScreenType::Chat
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<InputAction> {
        // Handle input mode separately
        if self.input_active {
            return self.handle_input_key(key);
        }

        match key.code {
            // Navigation
            KeyCode::Char('j') | KeyCode::Down => {
                if self.channel_focused {
                    self.select_next_channel();
                } else {
                    self.scroll_down();
                }
                None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.channel_focused {
                    self.select_prev_channel();
                } else {
                    self.scroll_up();
                }
                None
            }
            KeyCode::Tab => {
                self.channel_focused = !self.channel_focused;
                self.needs_redraw = true;
                None
            }
            KeyCode::Enter => {
                if self.channel_focused {
                    // Switch to messages pane
                    self.channel_focused = false;
                    self.needs_redraw = true;
                }
                None
            }
            // Compose
            KeyCode::Char('i') => {
                self.input_active = true;
                self.needs_redraw = true;
                None
            }
            // Page navigation
            KeyCode::PageUp => {
                self.message_scroll = self.message_scroll.saturating_sub(10);
                self.needs_redraw = true;
                None
            }
            KeyCode::PageDown => {
                self.message_scroll =
                    (self.message_scroll + 10).min(self.messages.len().saturating_sub(1));
                self.needs_redraw = true;
                None
            }
            KeyCode::Home if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.message_scroll = 0;
                self.needs_redraw = true;
                None
            }
            KeyCode::End if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.message_scroll = self.messages.len().saturating_sub(1);
                self.needs_redraw = true;
                None
            }
            // Pass through for global handling
            _ => None,
        }
    }

    fn render(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        // Main layout: sidebar + content using consistent widths
        let main_chunks = LayoutPresets::sidebar_content(area, widths::SIDEBAR_STANDARD);

        // Content area: messages + input using consistent heights
        let content_chunks = LayoutPresets::content_with_input(main_chunks[1]);

        self.render_channels(f, main_chunks[0], styles);
        self.render_messages(f, content_chunks[0], styles);
        self.render_input(f, content_chunks[1], styles);
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

/// Format a timestamp for display
fn format_timestamp(ts: u64) -> String {
    // Simple formatting suitable for offline TUI; swap for chrono/formatting helpers if needed
    let hours = (ts / 3600000) % 24;
    let minutes = (ts / 60000) % 60;
    format!("{:02}:{:02}", hours, minutes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_screen_new() {
        let screen = ChatScreen::new();
        assert_eq!(screen.screen_type(), ScreenType::Chat);
        assert!(screen.channels.is_empty());
        assert!(screen.messages.is_empty());
    }

    #[test]
    fn test_set_channels() {
        let mut screen = ChatScreen::new();
        let channels = vec![
            Channel {
                id: "1".to_string(),
                name: "general".to_string(),
                topic: None,
                unread_count: 5,
                is_dm: false,
                member_count: 10,
                last_activity: 1000,
            },
            Channel {
                id: "2".to_string(),
                name: "alice".to_string(),
                topic: None,
                unread_count: 0,
                is_dm: true,
                member_count: 2,
                last_activity: 0,
            },
        ];
        screen.set_channels(channels);
        assert_eq!(screen.channels.len(), 2);
        assert_eq!(screen.selected_channel, Some(0));
    }

    #[test]
    fn test_channel_navigation() {
        let mut screen = ChatScreen::new();
        let channels = vec![
            Channel {
                id: "1".to_string(),
                name: "ch1".to_string(),
                topic: None,
                unread_count: 0,
                is_dm: false,
                member_count: 1,
                last_activity: 0,
            },
            Channel {
                id: "2".to_string(),
                name: "ch2".to_string(),
                topic: None,
                unread_count: 0,
                is_dm: false,
                member_count: 1,
                last_activity: 0,
            },
        ];
        screen.set_channels(channels);

        assert_eq!(screen.selected_channel, Some(0));
        screen.select_next_channel();
        assert_eq!(screen.selected_channel, Some(1));
        screen.select_next_channel(); // Should not go past end
        assert_eq!(screen.selected_channel, Some(1));
        screen.select_prev_channel();
        assert_eq!(screen.selected_channel, Some(0));
    }

    #[test]
    fn test_input_handling() {
        let mut screen = ChatScreen::new();
        screen.input_active = true;

        // Type some text
        screen.handle_input_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));
        screen.handle_input_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        assert_eq!(screen.input_buffer, "hi");
        assert_eq!(screen.cursor_pos, 2);

        // Backspace
        screen.handle_input_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(screen.input_buffer, "h");
        assert_eq!(screen.cursor_pos, 1);
    }
}
