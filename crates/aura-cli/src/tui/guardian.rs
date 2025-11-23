//! # Guardian TUI Interface
//!
//! Alice's guardian interface for demo coordination.

use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use super::state::{AppState, GuardianState};
use aura_core::AuthorityId;

/// Guardian interface for Alice/Charlie during demo
pub struct GuardianInterface {
    /// Guardian authority ID
    guardian_id: AuthorityId,
    /// Guardian name
    guardian_name: String,
    /// Whether this guardian is active/focused
    active: bool,
}

impl GuardianInterface {
    /// Create new guardian interface
    pub fn new(guardian_id: AuthorityId, guardian_name: String) -> Self {
        Self {
            guardian_id,
            guardian_name,
            active: false,
        }
    }

    /// Set active status
    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    /// Render the guardian interface
    pub fn render(
        &self,
        f: &mut Frame<'_>,
        area: ratatui::layout::Rect,
        app_state: &AppState,
    ) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Header
                Constraint::Min(1),    // Main content
                Constraint::Length(5), // Actions
            ])
            .split(area);

        // Header
        self.render_header(f, chunks[0]);

        // Main content based on demo phase
        self.render_main_content(f, chunks[1], app_state);

        // Actions
        self.render_actions(f, chunks[2], app_state);
    }

    /// Render guardian header
    fn render_header(&self, f: &mut Frame<'_>, area: ratatui::layout::Rect) {
        let title = format!("{} - Guardian Interface", self.guardian_name);
        let status = if self.active { "ACTIVE" } else { "STANDBY" };
        let status_color = if self.active {
            Color::Green
        } else {
            Color::Yellow
        };

        let header_text = vec![Line::from(vec![
            Span::raw(title),
            Span::raw("  ["),
            Span::styled(
                status,
                Style::default()
                    .fg(status_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("]"),
        ])];

        let header = Paragraph::new(header_text).block(Block::default().borders(Borders::ALL));

        f.render_widget(header, area);
    }

    /// Render main content
    fn render_main_content(
        &self,
        f: &mut Frame<'_>,
        area: ratatui::layout::Rect,
        app_state: &AppState,
    ) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        // Recovery status
        self.render_recovery_status(f, chunks[0], app_state);

        // Bob's status
        self.render_bob_status(f, chunks[1], app_state);
    }

    /// Render recovery status panel
    fn render_recovery_status(
        &self,
        f: &mut Frame<'_>,
        area: ratatui::layout::Rect,
        app_state: &AppState,
    ) {
        let mut content = vec![Line::from("Recovery Status:"), Line::from("")];

        if app_state.recovery_progress.initiated {
            content.push(Line::from(format!(
                "Status: {}",
                app_state.recovery_status()
            )));
            content.push(Line::from(""));

            // Show guardian approvals
            for guardian in app_state.guardian_states.values() {
                let status_icon = if guardian.approved_recovery {
                    "[OK]"
                } else {
                    "[WAIT]"
                };
                let status_text = if guardian.approved_recovery {
                    "Approved"
                } else {
                    "Pending"
                };

                let style = if guardian.approved_recovery {
                    Style::default().fg(Color::Green)
                } else if guardian.authority_id == self.guardian_id {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                content.push(Line::from(vec![
                    Span::raw(format!("{} {}: ", status_icon, guardian.name)),
                    Span::styled(status_text, style),
                ]));
            }

            // Progress bar
            if app_state.recovery_progress.threshold > 0 {
                content.push(Line::from(""));
                let progress = (app_state.recovery_progress.approvals * 100)
                    / app_state.recovery_progress.threshold;
                content.push(Line::from(format!("Progress: {}%", progress)));
            }
        } else {
            content.push(Line::from("No active recovery"));
        }

        let recovery_panel = Paragraph::new(content)
            .block(Block::default().borders(Borders::ALL).title("Recovery"))
            .wrap(Wrap { trim: true });

        f.render_widget(recovery_panel, area);
    }

    /// Render Bob's status panel
    fn render_bob_status(
        &self,
        f: &mut Frame<'_>,
        area: ratatui::layout::Rect,
        app_state: &AppState,
    ) {
        let mut content = vec![
            Line::from("Bob's Current Status:"),
            Line::from(""),
            Line::from(format!("Phase: {}", app_state.phase_description())),
            Line::from(""),
        ];

        match app_state.current_phase {
            super::state::DemoPhase::GroupChat => {
                content.push(Line::from("Bob is chatting normally"));
                if let Some(last_msg) = app_state.message_history.last() {
                    content.push(Line::from(format!(
                        "Last message: \"{}\"",
                        last_msg.content
                    )));
                }
            }
            super::state::DemoPhase::DataLoss => {
                content.push(Line::from("Bob lost his device!"));
                content.push(Line::from("Bob needs guardian help"));
            }
            super::state::DemoPhase::Recovery => {
                content.push(Line::from("Bob is recovering"));
                content.push(Line::from("Waiting for guardian approvals"));
            }
            super::state::DemoPhase::Restoration => {
                content.push(Line::from("Bob's data restored"));
                content.push(Line::from("Bob can chat again"));
            }
            _ => {
                content.push(Line::from("Setting up demo..."));
            }
        }

        let bob_panel = Paragraph::new(content)
            .block(Block::default().borders(Borders::ALL).title("Bob's Status"))
            .wrap(Wrap { trim: true });

        f.render_widget(bob_panel, area);
    }

    /// Render available actions
    fn render_actions(
        &self,
        f: &mut Frame<'_>,
        area: ratatui::layout::Rect,
        app_state: &AppState,
    ) {
        let mut actions = vec![];

        // Add context-specific actions
        if app_state.recovery_progress.initiated {
            if let Some(guardian_state) = app_state.guardian_states.get(&self.guardian_id) {
                if !guardian_state.approved_recovery {
                    actions.push("SPACE: Approve Recovery");
                }
            }
        }

        // General actions
        actions.push("Tab: Switch View");
        actions.push("q: Quit");

        let action_text: Vec<Line> = actions
            .into_iter()
            .map(|action| Line::from(action))
            .collect();

        let actions_panel = Paragraph::new(action_text)
            .block(Block::default().borders(Borders::ALL).title("Actions"))
            .wrap(Wrap { trim: true });

        f.render_widget(actions_panel, area);
    }

    /// Check if this guardian can approve recovery
    pub fn can_approve_recovery(&self, app_state: &AppState) -> bool {
        if !app_state.recovery_progress.initiated {
            return false;
        }

        if let Some(guardian_state) = app_state.guardian_states.get(&self.guardian_id) {
            !guardian_state.approved_recovery
        } else {
            false
        }
    }

    /// Get guardian ID
    pub fn guardian_id(&self) -> AuthorityId {
        self.guardian_id
    }

    /// Get guardian name
    pub fn guardian_name(&self) -> &str {
        &self.guardian_name
    }
}
