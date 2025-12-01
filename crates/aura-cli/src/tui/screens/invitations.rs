//! # Invitations Screen
//!
//! Display and manage guardian invitations.
//! Shows sent and received invitations with status tracking.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState, Paragraph, Tabs, Wrap},
    Frame,
};

use super::{Screen, ScreenType};
use crate::tui::input::InputAction;
use crate::tui::layout::{heights, LayoutPresets, ScreenLayout};
use crate::tui::reactive::{Invitation, InvitationDirection, InvitationStatus, InvitationType};
use crate::tui::styles::Styles;

/// Invitations screen state
pub struct InvitationsScreen {
    /// All invitations
    invitations: Vec<Invitation>,
    /// Current filter (sent/received/all)
    filter: InvitationFilter,
    /// Selected invitation index
    selected: Option<usize>,
    /// List state for rendering
    list_state: ListState,
    /// Whether detail panel is focused
    detail_focused: bool,
    /// Flag for redraw
    needs_redraw: bool,
}

/// Filter for invitation display
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvitationFilter {
    /// Show all invitations
    All,
    /// Show only sent invitations
    Sent,
    /// Show only received invitations
    Received,
}

impl InvitationsScreen {
    /// Create a new invitations screen
    pub fn new() -> Self {
        Self {
            invitations: Vec::new(),
            filter: InvitationFilter::All,
            selected: None,
            list_state: ListState::default(),
            detail_focused: false,
            needs_redraw: true,
        }
    }

    /// Set invitations from view data
    pub fn set_invitations(&mut self, invitations: Vec<Invitation>) {
        self.invitations = invitations;
        if self.selected.is_none() && !self.filtered_invitations().is_empty() {
            self.selected = Some(0);
            self.list_state.select(Some(0));
        }
        self.needs_redraw = true;
    }

    /// Get filtered invitations based on current filter
    fn filtered_invitations(&self) -> Vec<&Invitation> {
        self.invitations
            .iter()
            .filter(|inv| match self.filter {
                InvitationFilter::All => true,
                InvitationFilter::Sent => inv.direction == InvitationDirection::Outbound,
                InvitationFilter::Received => inv.direction == InvitationDirection::Inbound,
            })
            .collect()
    }

    /// Get currently selected invitation
    pub fn selected_invitation(&self) -> Option<&Invitation> {
        let filtered = self.filtered_invitations();
        self.selected.and_then(|idx| filtered.get(idx).copied())
    }

    /// Move selection up
    fn select_prev(&mut self) {
        if let Some(selected) = self.selected {
            if selected > 0 {
                self.selected = Some(selected - 1);
                self.list_state.select(Some(selected - 1));
                self.needs_redraw = true;
            }
        }
    }

    /// Move selection down
    fn select_next(&mut self) {
        let filtered_len = self.filtered_invitations().len();
        if let Some(selected) = self.selected {
            if selected + 1 < filtered_len {
                self.selected = Some(selected + 1);
                self.list_state.select(Some(selected + 1));
                self.needs_redraw = true;
            }
        }
    }

    /// Switch to next filter
    fn next_filter(&mut self) {
        self.filter = match self.filter {
            InvitationFilter::All => InvitationFilter::Sent,
            InvitationFilter::Sent => InvitationFilter::Received,
            InvitationFilter::Received => InvitationFilter::All,
        };
        // Reset selection for new filter
        let filtered = self.filtered_invitations();
        if filtered.is_empty() {
            self.selected = None;
            self.list_state.select(None);
        } else {
            self.selected = Some(0);
            self.list_state.select(Some(0));
        }
        self.needs_redraw = true;
    }

    /// Render the filter tabs
    fn render_tabs(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let titles = vec!["All", "Sent", "Received"];
        let selected_idx = match self.filter {
            InvitationFilter::All => 0,
            InvitationFilter::Sent => 1,
            InvitationFilter::Received => 2,
        };

        // Use consistent tab styling from Styles
        let tabs = Tabs::new(titles)
            .block(styles.panel_tabs())
            .select(selected_idx)
            .style(styles.text_muted())
            .highlight_style(
                Style::default()
                    .fg(styles.palette.primary)
                    .add_modifier(Modifier::BOLD),
            );

        f.render_widget(tabs, area);
    }

    /// Render the invitation list
    fn render_list(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let filtered = self.filtered_invitations();

        let items: Vec<ListItem> = filtered
            .iter()
            .map(|invitation| {
                let direction_icon = match invitation.direction {
                    InvitationDirection::Outbound => "→",
                    InvitationDirection::Inbound => "←",
                };

                let status_style = match invitation.status {
                    InvitationStatus::Pending => styles.text_warning(),
                    InvitationStatus::Accepted => styles.text_success(),
                    InvitationStatus::Declined => styles.text_error(),
                    InvitationStatus::Expired => styles.text_muted(),
                    InvitationStatus::Cancelled => styles.text_muted(),
                };

                let type_icon = match invitation.invitation_type {
                    InvitationType::Guardian => "◆",
                    InvitationType::Contact => "◯",
                    InvitationType::Channel => "◈",
                };

                let line = Line::from(vec![
                    Span::styled(format!("{} ", type_icon), styles.text_highlight()),
                    Span::styled(format!("{} ", direction_icon), styles.text_muted()),
                    Span::styled(&invitation.other_party_name, styles.text()),
                    Span::styled(
                        format!(" [{}]", format_status(&invitation.status)),
                        status_style,
                    ),
                ]);

                ListItem::new(line)
            })
            .collect();

        // Use consistent panel styling from Styles
        let block = if !self.detail_focused {
            styles.panel_focused(format!("Invitations ({})", filtered.len()))
        } else {
            styles.panel_compact(format!("Invitations ({})", filtered.len()))
        };

        let list = List::new(items)
            .block(block)
            .highlight_style(styles.list_item_selected());

        let mut state = self.list_state.clone();
        f.render_stateful_widget(list, area, &mut state);
    }

    /// Render the detail panel
    fn render_detail(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        // Use consistent panel styling from Styles with focus state
        let block = if self.detail_focused {
            styles.panel_focused("Details")
        } else {
            styles.panel("Details")
        };

        let inner = block.inner(area);
        f.render_widget(block, area);

        if let Some(invitation) = self.selected_invitation() {
            let direction_text = match invitation.direction {
                InvitationDirection::Outbound => "Sent to",
                InvitationDirection::Inbound => "Received from",
            };

            let type_text = match invitation.invitation_type {
                InvitationType::Guardian => "Guardian Invitation",
                InvitationType::Contact => "Contact Invitation",
                InvitationType::Channel => "Channel Invitation",
            };

            let status_style = match invitation.status {
                InvitationStatus::Pending => styles.text_warning(),
                InvitationStatus::Accepted => styles.text_success(),
                InvitationStatus::Declined => styles.text_error(),
                InvitationStatus::Expired => styles.text_muted(),
                InvitationStatus::Cancelled => styles.text_muted(),
            };

            let mut lines = vec![
                Line::from(vec![
                    Span::styled("Type: ", styles.text_muted()),
                    Span::styled(type_text, styles.text()),
                ]),
                Line::from(vec![
                    Span::styled(format!("{}: ", direction_text), styles.text_muted()),
                    Span::styled(&invitation.other_party_name, styles.text()),
                ]),
                Line::from(vec![
                    Span::styled("Status: ", styles.text_muted()),
                    Span::styled(format_status(&invitation.status), status_style),
                ]),
                Line::from(""),
            ];

            if let Some(ref message) = invitation.message {
                lines.push(Line::from(vec![Span::styled(
                    "Message: ",
                    styles.text_muted(),
                )]));
                lines.push(Line::from(vec![Span::styled(message, styles.text_muted())]));
                lines.push(Line::from(""));
            }

            lines.push(Line::from(vec![
                Span::styled("Created: ", styles.text_muted()),
                Span::styled(format_timestamp(invitation.created_at), styles.text_muted()),
            ]));

            if let Some(expires) = invitation.expires_at {
                lines.push(Line::from(vec![
                    Span::styled("Expires: ", styles.text_muted()),
                    Span::styled(format_timestamp(expires), styles.text_muted()),
                ]));
            }

            let detail = Paragraph::new(lines).wrap(Wrap { trim: true });
            f.render_widget(detail, inner);
        } else {
            let empty =
                Paragraph::new("Select an invitation to view details").style(styles.text_muted());
            f.render_widget(empty, inner);
        }
    }

    /// Render action hints
    fn render_actions(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        // Use consistent footer panel styling from Styles
        let block = styles.panel_footer();
        let inner = block.inner(area);
        f.render_widget(block, area);

        let mut actions = Vec::new();

        if let Some(invitation) = self.selected_invitation() {
            match (&invitation.direction, &invitation.status) {
                (InvitationDirection::Inbound, InvitationStatus::Pending) => {
                    actions.push(Span::styled("[a]ccept  ", styles.text_success()));
                    actions.push(Span::styled("[d]ecline  ", styles.text_error()));
                }
                (InvitationDirection::Outbound, InvitationStatus::Pending) => {
                    actions.push(Span::styled("[c]ancel  ", styles.text_warning()));
                }
                _ => {}
            }
        }

        actions.push(Span::styled("[n]ew Invitation  ", styles.text_highlight()));
        actions.push(Span::styled("[Tab] Filter  ", styles.text_muted()));

        let action_line = Line::from(actions);
        let actions_para = Paragraph::new(action_line);
        f.render_widget(actions_para, inner);
    }
}

impl Default for InvitationsScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen for InvitationsScreen {
    fn screen_type(&self) -> ScreenType {
        ScreenType::Invitations
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
                if !self.detail_focused {
                    self.next_filter();
                } else {
                    self.detail_focused = false;
                    self.needs_redraw = true;
                }
                None
            }
            KeyCode::Enter => {
                self.detail_focused = !self.detail_focused;
                self.needs_redraw = true;
                None
            }
            KeyCode::Char('a') | KeyCode::Char('A') => {
                if let Some(inv) = self.selected_invitation() {
                    if inv.direction == InvitationDirection::Inbound
                        && inv.status == InvitationStatus::Pending
                    {
                        return Some(InputAction::Submit(format!(
                            "action:accept_invitation:{}",
                            inv.id
                        )));
                    }
                }
                None
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
                if let Some(inv) = self.selected_invitation() {
                    if inv.direction == InvitationDirection::Inbound
                        && inv.status == InvitationStatus::Pending
                    {
                        return Some(InputAction::Submit(format!(
                            "action:decline_invitation:{}",
                            inv.id
                        )));
                    }
                }
                None
            }
            KeyCode::Char('c') | KeyCode::Char('C') => {
                if let Some(inv) = self.selected_invitation() {
                    if inv.direction == InvitationDirection::Outbound
                        && inv.status == InvitationStatus::Pending
                    {
                        return Some(InputAction::Submit(format!(
                            "action:cancel_invitation:{}",
                            inv.id
                        )));
                    }
                }
                None
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                Some(InputAction::Submit("action:new_invitation".to_string()))
            }
            _ => None,
        }
    }

    fn render(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        // Layout using consistent grid system: tabs, content, actions
        let chunks = ScreenLayout::new()
            .fixed(2) // Tabs (2 rows)
            .flexible(10) // Content (min 10 rows)
            .fixed(heights::COMPACT) // Actions footer (3 rows)
            .build(area);

        // Split content into list + details using standard LIST_DETAIL split (40/60)
        let content_chunks = LayoutPresets::list_detail(chunks[1]);

        self.render_tabs(f, chunks[0], styles);
        self.render_list(f, content_chunks[0], styles);
        self.render_detail(f, content_chunks[1], styles);
        self.render_actions(f, chunks[2], styles);
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

/// Format invitation status for display
fn format_status(status: &InvitationStatus) -> &'static str {
    match status {
        InvitationStatus::Pending => "Pending",
        InvitationStatus::Accepted => "Accepted",
        InvitationStatus::Declined => "Declined",
        InvitationStatus::Expired => "Expired",
        InvitationStatus::Cancelled => "Cancelled",
    }
}

/// Format a timestamp for display
fn format_timestamp(ts: u64) -> String {
    let hours = (ts / 3600000) % 24;
    let minutes = (ts / 60000) % 60;
    format!("{:02}:{:02}", hours, minutes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_invitation(
        id: &str,
        direction: InvitationDirection,
        status: InvitationStatus,
    ) -> Invitation {
        Invitation {
            id: id.to_string(),
            direction,
            other_party_id: format!("peer_{}", id),
            other_party_name: format!("Peer {}", id),
            invitation_type: InvitationType::Guardian,
            status,
            created_at: 1000,
            expires_at: Some(2000),
            message: Some("Test message".to_string()),
        }
    }

    #[test]
    fn test_invitations_screen_new() {
        let screen = InvitationsScreen::new();
        assert_eq!(screen.screen_type(), ScreenType::Invitations);
        assert!(screen.invitations.is_empty());
        assert_eq!(screen.filter, InvitationFilter::All);
    }

    #[test]
    fn test_set_invitations() {
        let mut screen = InvitationsScreen::new();
        let invitations = vec![
            create_test_invitation(
                "1",
                InvitationDirection::Outbound,
                InvitationStatus::Pending,
            ),
            create_test_invitation("2", InvitationDirection::Inbound, InvitationStatus::Pending),
        ];
        screen.set_invitations(invitations);
        assert_eq!(screen.invitations.len(), 2);
        assert_eq!(screen.selected, Some(0));
    }

    #[test]
    fn test_filter_invitations() {
        let mut screen = InvitationsScreen::new();
        let invitations = vec![
            create_test_invitation(
                "1",
                InvitationDirection::Outbound,
                InvitationStatus::Pending,
            ),
            create_test_invitation("2", InvitationDirection::Inbound, InvitationStatus::Pending),
            create_test_invitation(
                "3",
                InvitationDirection::Outbound,
                InvitationStatus::Accepted,
            ),
        ];
        screen.set_invitations(invitations);

        // All filter
        assert_eq!(screen.filtered_invitations().len(), 3);

        // Sent filter
        screen.filter = InvitationFilter::Sent;
        assert_eq!(screen.filtered_invitations().len(), 2);

        // Received filter
        screen.filter = InvitationFilter::Received;
        assert_eq!(screen.filtered_invitations().len(), 1);
    }

    #[test]
    fn test_navigation() {
        let mut screen = InvitationsScreen::new();
        let invitations = vec![
            create_test_invitation(
                "1",
                InvitationDirection::Outbound,
                InvitationStatus::Pending,
            ),
            create_test_invitation("2", InvitationDirection::Inbound, InvitationStatus::Pending),
        ];
        screen.set_invitations(invitations);

        assert_eq!(screen.selected, Some(0));
        screen.select_next();
        assert_eq!(screen.selected, Some(1));
        screen.select_prev();
        assert_eq!(screen.selected, Some(0));
    }

    #[test]
    fn test_filter_cycling() {
        let mut screen = InvitationsScreen::new();
        assert_eq!(screen.filter, InvitationFilter::All);

        screen.next_filter();
        assert_eq!(screen.filter, InvitationFilter::Sent);

        screen.next_filter();
        assert_eq!(screen.filter, InvitationFilter::Received);

        screen.next_filter();
        assert_eq!(screen.filter, InvitationFilter::All);
    }
}
