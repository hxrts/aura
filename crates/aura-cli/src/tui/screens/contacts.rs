//! # Contacts Screen
//!
//! Manage petnames and contact suggestions.
//! See `work/neighbor.md` for the petname system design.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use super::{Screen, ScreenType};
use crate::tui::input::InputAction;
use crate::tui::styles::Styles;

/// Policy for handling incoming contact suggestions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SuggestionPolicy {
    /// Automatically accept and update petname
    #[default]
    AutoAccept,
    /// Prompt before updating petname
    PromptFirst,
    /// Ignore suggestions entirely (manual petnames only)
    Ignore,
}

impl SuggestionPolicy {
    /// Get display name
    pub fn name(&self) -> &'static str {
        match self {
            Self::AutoAccept => "Auto-Accept",
            Self::PromptFirst => "Prompt First",
            Self::Ignore => "Ignore",
        }
    }

    /// Get description
    pub fn description(&self) -> &'static str {
        match self {
            Self::AutoAccept => "Automatically update petnames from suggestions",
            Self::PromptFirst => "Ask before updating petnames",
            Self::Ignore => "Never update petnames from suggestions",
        }
    }

    /// Cycle to next policy
    pub fn next(&self) -> Self {
        match self {
            Self::AutoAccept => Self::PromptFirst,
            Self::PromptFirst => Self::Ignore,
            Self::Ignore => Self::AutoAccept,
        }
    }
}

/// Contact suggestion metadata shared when connecting
#[derive(Debug, Clone, Default)]
pub struct ContactSuggestion {
    /// Suggested display name
    pub display_name: Option<String>,
    /// Suggested avatar (emoji or URL)
    pub avatar: Option<String>,
    /// Suggested pronouns
    pub pronouns: Option<String>,
    /// Optional status/bio
    pub status: Option<String>,
}

impl ContactSuggestion {
    /// Create a new contact suggestion
    pub fn new() -> Self {
        Self::default()
    }

    /// Set display name
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = Some(name.into());
        self
    }

    /// Check if suggestion has any content
    pub fn is_empty(&self) -> bool {
        self.display_name.is_none()
            && self.avatar.is_none()
            && self.pronouns.is_none()
            && self.status.is_none()
    }
}

/// A contact with petname and optional suggestion
#[derive(Debug, Clone)]
pub struct Contact {
    /// Authority ID (cryptographic identity)
    pub authority_id: String,
    /// Local petname (what you call them)
    pub petname: String,
    /// Their last contact suggestion
    pub suggestion: Option<ContactSuggestion>,
    /// Whether there's a pending suggestion update
    pub pending_suggestion: Option<ContactSuggestion>,
    /// When contact was added
    pub added_at: u64,
    /// When last interacted
    pub last_seen: Option<u64>,
    /// Whether contact is online (if known)
    pub is_online: Option<bool>,
}

impl Contact {
    /// Get display name (petname takes priority)
    pub fn display_name(&self) -> &str {
        &self.petname
    }

    /// Get suggested name if different from petname
    pub fn suggested_name(&self) -> Option<&str> {
        self.suggestion
            .as_ref()
            .and_then(|s| s.display_name.as_deref())
            .filter(|name| *name != self.petname)
    }
}

/// Contacts screen state
pub struct ContactsScreen {
    /// List of contacts
    contacts: Vec<Contact>,
    /// Selected contact index
    selected: Option<usize>,
    /// List state for scrolling
    list_state: ListState,
    /// Current suggestion policy
    policy: SuggestionPolicy,
    /// User's own contact suggestion (what they share)
    my_suggestion: ContactSuggestion,
    /// Whether editing mode is active
    editing: Option<EditingField>,
    /// Edit buffer
    edit_buffer: String,
    /// Filter string
    filter: String,
    /// Whether filter input is focused (reserved for future UI)
    _filter_active: bool,
    /// Show pending suggestions panel
    show_pending: bool,
    /// Focus: 0=list, 1=details, 2=pending
    focused_panel: usize,
    /// Flag for redraw
    needs_redraw: bool,
}

/// Field being edited
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditingField {
    /// Editing petname for selected contact
    Petname,
    /// Editing filter
    Filter,
    /// Editing own display name
    MyDisplayName,
    /// Editing own status
    MyStatus,
}

impl ContactsScreen {
    /// Create a new contacts screen
    pub fn new() -> Self {
        Self {
            contacts: Vec::new(),
            selected: None,
            list_state: ListState::default(),
            policy: SuggestionPolicy::default(),
            my_suggestion: ContactSuggestion::default(),
            editing: None,
            edit_buffer: String::new(),
            filter: String::new(),
            _filter_active: false,
            show_pending: false,
            focused_panel: 0,
            needs_redraw: true,
        }
    }

    /// Set contacts list
    pub fn set_contacts(&mut self, contacts: Vec<Contact>) {
        self.contacts = contacts;
        if self.selected.is_none() && !self.contacts.is_empty() {
            self.selected = Some(0);
            self.list_state.select(Some(0));
        }
        self.needs_redraw = true;
    }

    /// Set suggestion policy
    pub fn set_policy(&mut self, policy: SuggestionPolicy) {
        self.policy = policy;
        self.needs_redraw = true;
    }

    /// Set own contact suggestion
    pub fn set_my_suggestion(&mut self, suggestion: ContactSuggestion) {
        self.my_suggestion = suggestion;
        self.needs_redraw = true;
    }

    /// Get selected contact
    pub fn selected_contact(&self) -> Option<&Contact> {
        self.selected
            .and_then(|idx| self.filtered_contacts().get(idx).copied())
    }

    /// Get filtered contacts
    fn filtered_contacts(&self) -> Vec<&Contact> {
        if self.filter.is_empty() {
            self.contacts.iter().collect()
        } else {
            let filter_lower = self.filter.to_lowercase();
            self.contacts
                .iter()
                .filter(|c| {
                    c.petname.to_lowercase().contains(&filter_lower)
                        || c.authority_id.to_lowercase().contains(&filter_lower)
                })
                .collect()
        }
    }

    /// Get contacts with pending suggestions
    fn pending_suggestions(&self) -> Vec<&Contact> {
        self.contacts
            .iter()
            .filter(|c| c.pending_suggestion.is_some())
            .collect()
    }

    /// Move selection up
    fn select_prev(&mut self) {
        let filtered = self.filtered_contacts();
        if let Some(selected) = self.selected {
            if selected > 0 {
                self.selected = Some(selected - 1);
                self.list_state.select(Some(selected - 1));
                self.needs_redraw = true;
            }
        } else if !filtered.is_empty() {
            self.selected = Some(0);
            self.list_state.select(Some(0));
            self.needs_redraw = true;
        }
    }

    /// Move selection down
    fn select_next(&mut self) {
        let filtered = self.filtered_contacts();
        if let Some(selected) = self.selected {
            if selected + 1 < filtered.len() {
                self.selected = Some(selected + 1);
                self.list_state.select(Some(selected + 1));
                self.needs_redraw = true;
            }
        } else if !filtered.is_empty() {
            self.selected = Some(0);
            self.list_state.select(Some(0));
            self.needs_redraw = true;
        }
    }

    /// Start editing a field
    fn start_editing(&mut self, field: EditingField) {
        self.edit_buffer = match field {
            EditingField::Petname => self
                .selected_contact()
                .map(|c| c.petname.clone())
                .unwrap_or_default(),
            EditingField::Filter => self.filter.clone(),
            EditingField::MyDisplayName => {
                self.my_suggestion.display_name.clone().unwrap_or_default()
            }
            EditingField::MyStatus => self.my_suggestion.status.clone().unwrap_or_default(),
        };
        self.editing = Some(field);
        self.needs_redraw = true;
    }

    /// Finish editing
    fn finish_editing(&mut self) -> Option<InputAction> {
        if let Some(field) = self.editing.take() {
            let result = match field {
                EditingField::Petname => {
                    if let Some(contact) = self.selected_contact() {
                        Some(InputAction::Submit(format!(
                            "action:set_petname:{}:{}",
                            contact.authority_id, self.edit_buffer
                        )))
                    } else {
                        None
                    }
                }
                EditingField::Filter => {
                    self.filter = self.edit_buffer.clone();
                    self.selected = if self.filtered_contacts().is_empty() {
                        None
                    } else {
                        Some(0)
                    };
                    self.list_state.select(self.selected);
                    None
                }
                EditingField::MyDisplayName => {
                    self.my_suggestion.display_name = if self.edit_buffer.is_empty() {
                        None
                    } else {
                        Some(self.edit_buffer.clone())
                    };
                    Some(InputAction::Submit(format!(
                        "action:update_my_suggestion:name:{}",
                        self.edit_buffer
                    )))
                }
                EditingField::MyStatus => {
                    self.my_suggestion.status = if self.edit_buffer.is_empty() {
                        None
                    } else {
                        Some(self.edit_buffer.clone())
                    };
                    Some(InputAction::Submit(format!(
                        "action:update_my_suggestion:status:{}",
                        self.edit_buffer
                    )))
                }
            };
            self.edit_buffer.clear();
            self.needs_redraw = true;
            return result;
        }
        None
    }

    /// Cancel editing
    fn cancel_editing(&mut self) {
        self.editing = None;
        self.edit_buffer.clear();
        self.needs_redraw = true;
    }

    /// Handle character input during editing
    fn handle_edit_char(&mut self, c: char) {
        self.edit_buffer.push(c);
        self.needs_redraw = true;
    }

    /// Handle backspace during editing
    fn handle_edit_backspace(&mut self) {
        self.edit_buffer.pop();
        self.needs_redraw = true;
    }

    /// Toggle pending panel
    fn toggle_pending(&mut self) {
        self.show_pending = !self.show_pending;
        self.needs_redraw = true;
    }

    /// Cycle suggestion policy
    fn cycle_policy(&mut self) -> Option<InputAction> {
        self.policy = self.policy.next();
        self.needs_redraw = true;
        Some(InputAction::Submit(format!(
            "action:set_suggestion_policy:{}",
            match self.policy {
                SuggestionPolicy::AutoAccept => "auto",
                SuggestionPolicy::PromptFirst => "prompt",
                SuggestionPolicy::Ignore => "ignore",
            }
        )))
    }

    /// Next panel
    fn next_panel(&mut self) {
        let max_panels = if self.show_pending { 3 } else { 2 };
        self.focused_panel = (self.focused_panel + 1) % max_panels;
        self.needs_redraw = true;
    }

    /// Render the contacts list
    fn render_list(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let block = Block::default()
            .title(" Contacts ")
            .borders(Borders::ALL)
            .border_style(if self.focused_panel == 0 {
                styles.border_focused()
            } else {
                styles.border()
            });

        let filtered = self.filtered_contacts();
        let items: Vec<ListItem> = filtered
            .iter()
            .map(|contact| {
                let (icon, icon_style) = if contact.is_online == Some(true) {
                    ("*", styles.text_success())
                } else if contact.pending_suggestion.is_some() {
                    ("!", styles.text_warning())
                } else {
                    (" ", styles.text_muted())
                };

                let name_style = if contact.pending_suggestion.is_some() {
                    styles.text_highlight()
                } else {
                    styles.text()
                };

                let line = Line::from(vec![
                    Span::styled(icon, icon_style),
                    Span::styled(" ", styles.text()),
                    Span::styled(&contact.petname, name_style),
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

    /// Render contact details
    fn render_details(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let block = Block::default()
            .title(" Details ")
            .borders(Borders::ALL)
            .border_style(if self.focused_panel == 1 {
                styles.border_focused()
            } else {
                styles.border()
            });

        let inner = block.inner(area);
        f.render_widget(block, area);

        if let Some(contact) = self.selected_contact() {
            let mut lines = vec![
                Line::from(vec![
                    Span::styled("Petname: ", styles.text_muted()),
                    Span::styled(&contact.petname, styles.text()),
                ]),
                Line::from(vec![
                    Span::styled("Authority: ", styles.text_muted()),
                    Span::styled(
                        &contact.authority_id[..contact.authority_id.len().min(16)],
                        styles.text_muted(),
                    ),
                    Span::styled("...", styles.text_muted()),
                ]),
            ];

            if let Some(suggested) = contact.suggested_name() {
                lines.push(Line::from(vec![
                    Span::styled("Suggested: ", styles.text_muted()),
                    Span::styled(suggested, styles.text_highlight()),
                ]));
            }

            if let Some(suggestion) = &contact.suggestion {
                if let Some(status) = &suggestion.status {
                    lines.push(Line::from(vec![
                        Span::styled("Status: ", styles.text_muted()),
                        Span::styled(status, styles.text()),
                    ]));
                }
            }

            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("Online: ", styles.text_muted()),
                match contact.is_online {
                    Some(true) => Span::styled("Yes", styles.text_success()),
                    Some(false) => Span::styled("No", styles.text_muted()),
                    None => Span::styled("Unknown", styles.text_muted()),
                },
            ]));

            if let Some(pending) = &contact.pending_suggestion {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![Span::styled(
                    "Pending Update:",
                    styles.text_warning(),
                )]));
                if let Some(name) = &pending.display_name {
                    lines.push(Line::from(vec![
                        Span::styled("  Name: ", styles.text_muted()),
                        Span::styled(name, styles.text_warning()),
                    ]));
                }
            }

            let para = Paragraph::new(lines);
            f.render_widget(para, inner);
        } else {
            let empty = Paragraph::new("No contact selected").style(styles.text_muted());
            f.render_widget(empty, inner);
        }
    }

    /// Render pending suggestions panel
    fn render_pending(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let block = Block::default()
            .title(" Pending Suggestions ")
            .borders(Borders::ALL)
            .border_style(if self.focused_panel == 2 {
                styles.border_focused()
            } else {
                styles.border()
            });

        let inner = block.inner(area);
        f.render_widget(block, area);

        let pending = self.pending_suggestions();
        if pending.is_empty() {
            let empty = Paragraph::new("No pending suggestions").style(styles.text_muted());
            f.render_widget(empty, inner);
        } else {
            let lines: Vec<Line> = pending
                .iter()
                .take(10)
                .map(|c| {
                    let new_name = c
                        .pending_suggestion
                        .as_ref()
                        .and_then(|s| s.display_name.as_deref())
                        .unwrap_or("?");

                    Line::from(vec![
                        Span::styled(&c.petname, styles.text()),
                        Span::styled(" -> ", styles.text_muted()),
                        Span::styled(new_name, styles.text_warning()),
                    ])
                })
                .collect();

            let para = Paragraph::new(lines);
            f.render_widget(para, inner);
        }
    }

    /// Render my suggestion settings
    fn render_my_suggestion(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let block = Block::default()
            .title(" My Contact Card ")
            .borders(Borders::ALL)
            .border_style(styles.border());

        let inner = block.inner(area);
        f.render_widget(block, area);

        let name = self
            .my_suggestion
            .display_name
            .as_deref()
            .unwrap_or("(not set)");
        let status = self
            .my_suggestion
            .status
            .as_deref()
            .unwrap_or("(no status)");

        let lines = vec![
            Line::from(vec![
                Span::styled("Name: ", styles.text_muted()),
                Span::styled(name, styles.text()),
            ]),
            Line::from(vec![
                Span::styled("Status: ", styles.text_muted()),
                Span::styled(status, styles.text()),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Policy: ", styles.text_muted()),
                Span::styled(self.policy.name(), styles.text_highlight()),
            ]),
        ];

        let para = Paragraph::new(lines);
        f.render_widget(para, inner);
    }

    /// Render actions bar
    fn render_actions(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let actions = if self.editing.is_some() {
            vec![
                Span::styled("[Enter] ", styles.text_highlight()),
                Span::styled("Save  ", styles.text()),
                Span::styled("[Esc] ", styles.text_muted()),
                Span::styled("Cancel", styles.text()),
            ]
        } else {
            let mut actions = vec![
                Span::styled("[E] ", styles.text_highlight()),
                Span::styled("Edit  ", styles.text()),
                Span::styled("[/] ", styles.text_muted()),
                Span::styled("Filter  ", styles.text()),
                Span::styled("[P] ", styles.text_muted()),
                Span::styled("Policy  ", styles.text()),
            ];

            let pending_count = self.pending_suggestions().len();
            if pending_count > 0 {
                actions.push(Span::styled("[U] ", styles.text_warning()));
                actions.push(Span::styled(
                    format!("Pending ({})  ", pending_count),
                    styles.text(),
                ));
            }

            actions
        };

        let action_line = Line::from(actions);
        let para = Paragraph::new(action_line).wrap(Wrap { trim: true });

        let block = Block::default()
            .title(" Actions ")
            .borders(Borders::ALL)
            .border_style(styles.border());

        f.render_widget(para.block(block), area);
    }

    /// Render edit popup
    fn render_edit_popup(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        if let Some(field) = &self.editing {
            let title = match field {
                EditingField::Petname => "Edit Petname",
                EditingField::Filter => "Filter Contacts",
                EditingField::MyDisplayName => "Edit Display Name",
                EditingField::MyStatus => "Edit Status",
            };

            // Center popup
            let popup_width = 50.min(area.width.saturating_sub(4));
            let popup_height = 5;
            let popup_x = (area.width.saturating_sub(popup_width)) / 2;
            let popup_y = (area.height.saturating_sub(popup_height)) / 2;

            let popup_area = Rect::new(
                area.x + popup_x,
                area.y + popup_y,
                popup_width,
                popup_height,
            );

            f.render_widget(Clear, popup_area);

            let block = Block::default()
                .title(format!(" {} ", title))
                .borders(Borders::ALL)
                .border_style(styles.border_focused());

            let inner = block.inner(popup_area);
            f.render_widget(block, popup_area);

            let input_text = format!("{}_", &self.edit_buffer);
            let input = Paragraph::new(input_text).style(styles.text());
            f.render_widget(input, inner);
        }
    }
}

impl Default for ContactsScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen for ContactsScreen {
    fn screen_type(&self) -> ScreenType {
        ScreenType::Contacts
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<InputAction> {
        // Handle editing mode
        if self.editing.is_some() {
            match key.code {
                KeyCode::Enter => return self.finish_editing(),
                KeyCode::Esc => {
                    self.cancel_editing();
                    return None;
                }
                KeyCode::Backspace => {
                    self.handle_edit_backspace();
                    return None;
                }
                KeyCode::Char(c) => {
                    self.handle_edit_char(c);
                    return None;
                }
                _ => return None,
            }
        }

        // Normal mode
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
            KeyCode::Char('e') | KeyCode::Char('E') => {
                if self.selected_contact().is_some() {
                    self.start_editing(EditingField::Petname);
                }
                None
            }
            KeyCode::Char('/') => {
                self.start_editing(EditingField::Filter);
                None
            }
            KeyCode::Char('p') | KeyCode::Char('P') => self.cycle_policy(),
            KeyCode::Char('u') | KeyCode::Char('U') => {
                self.toggle_pending();
                None
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                self.start_editing(EditingField::MyDisplayName);
                None
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.start_editing(EditingField::MyStatus);
                None
            }
            KeyCode::Enter => {
                // Accept pending suggestion for selected contact
                if let Some(contact) = self.selected_contact() {
                    if contact.pending_suggestion.is_some() {
                        return Some(InputAction::Submit(format!(
                            "action:accept_suggestion:{}",
                            contact.authority_id
                        )));
                    }
                }
                None
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
                // Reject pending suggestion
                if let Some(contact) = self.selected_contact() {
                    if contact.pending_suggestion.is_some() {
                        return Some(InputAction::Submit(format!(
                            "action:reject_suggestion:{}",
                            contact.authority_id
                        )));
                    }
                }
                None
            }
            KeyCode::Esc => {
                if !self.filter.is_empty() {
                    self.filter.clear();
                    self.selected = if self.contacts.is_empty() {
                        None
                    } else {
                        Some(0)
                    };
                    self.list_state.select(self.selected);
                    self.needs_redraw = true;
                }
                None
            }
            _ => None,
        }
    }

    fn render(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        // Main layout
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(6), // My suggestion
                Constraint::Min(10),   // Content
                Constraint::Length(3), // Actions
            ])
            .split(area);

        self.render_my_suggestion(f, main_chunks[0], styles);

        // Content layout
        if self.show_pending {
            let content_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(35),
                    Constraint::Percentage(35),
                    Constraint::Percentage(30),
                ])
                .split(main_chunks[1]);

            self.render_list(f, content_chunks[0], styles);
            self.render_details(f, content_chunks[1], styles);
            self.render_pending(f, content_chunks[2], styles);
        } else {
            let content_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
                .split(main_chunks[1]);

            self.render_list(f, content_chunks[0], styles);
            self.render_details(f, content_chunks[1], styles);
        }

        self.render_actions(f, main_chunks[2], styles);

        // Edit popup (rendered last to be on top)
        self.render_edit_popup(f, area, styles);
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
    fn test_contacts_screen_new() {
        let screen = ContactsScreen::new();
        assert!(screen.contacts.is_empty());
        assert_eq!(screen.policy, SuggestionPolicy::AutoAccept);
    }

    #[test]
    fn test_suggestion_policy_cycle() {
        let policy = SuggestionPolicy::AutoAccept;
        assert_eq!(policy.next(), SuggestionPolicy::PromptFirst);
        assert_eq!(policy.next().next(), SuggestionPolicy::Ignore);
        assert_eq!(policy.next().next().next(), SuggestionPolicy::AutoAccept);
    }

    #[test]
    fn test_contact_display_name() {
        let contact = Contact {
            authority_id: "auth123".to_string(),
            petname: "Alice".to_string(),
            suggestion: Some(ContactSuggestion {
                display_name: Some("Alice Smith".to_string()),
                ..Default::default()
            }),
            pending_suggestion: None,
            added_at: 0,
            last_seen: None,
            is_online: None,
        };

        assert_eq!(contact.display_name(), "Alice");
        assert_eq!(contact.suggested_name(), Some("Alice Smith"));
    }

    #[test]
    fn test_contact_suggestion_empty() {
        let suggestion = ContactSuggestion::default();
        assert!(suggestion.is_empty());

        let suggestion = ContactSuggestion::new().with_name("Test");
        assert!(!suggestion.is_empty());
    }

    #[test]
    fn test_set_contacts() {
        let mut screen = ContactsScreen::new();
        assert!(screen.selected.is_none());

        let contacts = vec![
            Contact {
                authority_id: "a1".to_string(),
                petname: "Alice".to_string(),
                suggestion: None,
                pending_suggestion: None,
                added_at: 0,
                last_seen: None,
                is_online: None,
            },
            Contact {
                authority_id: "a2".to_string(),
                petname: "Bob".to_string(),
                suggestion: None,
                pending_suggestion: None,
                added_at: 0,
                last_seen: None,
                is_online: None,
            },
        ];

        screen.set_contacts(contacts);
        assert_eq!(screen.contacts.len(), 2);
        assert_eq!(screen.selected, Some(0));
    }

    #[test]
    fn test_filter_contacts() {
        let mut screen = ContactsScreen::new();
        screen.set_contacts(vec![
            Contact {
                authority_id: "a1".to_string(),
                petname: "Alice".to_string(),
                suggestion: None,
                pending_suggestion: None,
                added_at: 0,
                last_seen: None,
                is_online: None,
            },
            Contact {
                authority_id: "a2".to_string(),
                petname: "Bob".to_string(),
                suggestion: None,
                pending_suggestion: None,
                added_at: 0,
                last_seen: None,
                is_online: None,
            },
        ]);

        assert_eq!(screen.filtered_contacts().len(), 2);

        screen.filter = "ali".to_string();
        assert_eq!(screen.filtered_contacts().len(), 1);
        assert_eq!(screen.filtered_contacts()[0].petname, "Alice");
    }

    #[test]
    fn test_pending_suggestions() {
        let mut screen = ContactsScreen::new();
        screen.set_contacts(vec![
            Contact {
                authority_id: "a1".to_string(),
                petname: "Alice".to_string(),
                suggestion: None,
                pending_suggestion: Some(ContactSuggestion {
                    display_name: Some("Alice New".to_string()),
                    ..Default::default()
                }),
                added_at: 0,
                last_seen: None,
                is_online: None,
            },
            Contact {
                authority_id: "a2".to_string(),
                petname: "Bob".to_string(),
                suggestion: None,
                pending_suggestion: None,
                added_at: 0,
                last_seen: None,
                is_online: None,
            },
        ]);

        let pending = screen.pending_suggestions();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].petname, "Alice");
    }
}
