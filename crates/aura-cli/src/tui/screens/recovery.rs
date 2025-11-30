//! # Recovery Screen
//!
//! Display and manage the account recovery process.
//! Shows recovery status, guardian approvals, and progress.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Wrap},
    Frame,
};

use super::{Screen, ScreenType};
use crate::tui::input::InputAction;
use crate::tui::reactive::{RecoveryState, RecoveryStatus};
use crate::tui::styles::Styles;

/// Recovery screen state
pub struct RecoveryScreen {
    /// Current recovery status
    status: Option<RecoveryStatus>,
    /// Flag for redraw
    needs_redraw: bool,
    /// Whether action panel is focused
    action_focused: bool,
}

impl RecoveryScreen {
    /// Create a new recovery screen
    pub fn new() -> Self {
        Self {
            status: None,
            needs_redraw: true,
            action_focused: false,
        }
    }

    /// Set recovery status from view data
    pub fn set_status(&mut self, status: RecoveryStatus) {
        self.status = Some(status);
        self.needs_redraw = true;
    }

    /// Get current state
    pub fn state(&self) -> RecoveryState {
        self.status
            .as_ref()
            .map(|s| s.state)
            .unwrap_or(RecoveryState::None)
    }

    /// Check if recovery can be started
    pub fn can_start(&self) -> bool {
        matches!(self.state(), RecoveryState::None)
    }

    /// Check if recovery can be cancelled
    pub fn can_cancel(&self) -> bool {
        matches!(
            self.state(),
            RecoveryState::Initiated | RecoveryState::ThresholdMet
        )
    }

    /// Check if recovery can be completed
    pub fn can_complete(&self) -> bool {
        matches!(self.state(), RecoveryState::ThresholdMet)
    }

    /// Render the status header
    fn render_status(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let block = Block::default()
            .title(" Recovery Status ")
            .borders(Borders::ALL)
            .border_style(styles.border());

        let inner = block.inner(area);
        f.render_widget(block, area);

        let (state_text, state_style) = match self.state() {
            RecoveryState::None => ("Not Started", styles.text_muted()),
            RecoveryState::Initiated => ("Awaiting Guardian Approvals", styles.text_warning()),
            RecoveryState::ThresholdMet => {
                ("Threshold Met - Ready to Complete", styles.text_success())
            }
            RecoveryState::InProgress => ("Reconstructing Keys...", styles.text_highlight()),
            RecoveryState::Completed => ("Recovery Completed!", styles.text_success()),
            RecoveryState::Failed => ("Recovery Failed", styles.text_error()),
            RecoveryState::Cancelled => ("Recovery Cancelled", styles.text_muted()),
        };

        let mut lines = vec![Line::from(vec![
            Span::styled("State: ", styles.text_muted()),
            Span::styled(state_text, state_style),
        ])];

        if let Some(ref status) = self.status {
            if let Some(ref session_id) = status.session_id {
                lines.push(Line::from(vec![
                    Span::styled("Session: ", styles.text_muted()),
                    Span::styled(&session_id[..session_id.len().min(16)], styles.text_muted()),
                ]));
            }
        }

        let status_para = Paragraph::new(lines).alignment(Alignment::Center);
        f.render_widget(status_para, inner);
    }

    /// Render the progress gauge
    fn render_progress(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let block = Block::default()
            .title(" Progress ")
            .borders(Borders::ALL)
            .border_style(styles.border());

        let inner = block.inner(area);
        f.render_widget(block, area);

        if let Some(ref status) = self.status {
            let progress = if status.threshold > 0 {
                (status.approvals_received as f64 / status.threshold as f64).min(1.0)
            } else {
                0.0
            };

            let label = format!(
                "{} / {} approvals",
                status.approvals_received, status.threshold
            );

            let gauge = Gauge::default().ratio(progress).label(label).gauge_style(
                Style::default()
                    .fg(styles.palette.primary)
                    .add_modifier(Modifier::BOLD),
            );

            f.render_widget(gauge, inner);
        } else {
            let empty = Paragraph::new("No active recovery")
                .style(styles.text_muted())
                .alignment(Alignment::Center);
            f.render_widget(empty, inner);
        }
    }

    /// Render the guardian approvals list
    fn render_approvals(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let block = Block::default()
            .title(" Guardian Approvals ")
            .borders(Borders::ALL)
            .border_style(if !self.action_focused {
                styles.border_focused()
            } else {
                styles.border()
            });

        if let Some(ref status) = self.status {
            let items: Vec<ListItem> = status
                .approvals
                .iter()
                .map(|approval| {
                    let (icon, icon_style) = if approval.approved {
                        ("✓", styles.text_success())
                    } else {
                        ("○", styles.text_muted())
                    };

                    let timestamp = approval
                        .timestamp
                        .map(|ts| format!(" at {}", format_timestamp(ts)))
                        .unwrap_or_default();

                    let line = Line::from(vec![
                        Span::styled(format!("{} ", icon), icon_style),
                        Span::styled(&approval.guardian_name, styles.text()),
                        Span::styled(timestamp, styles.text_muted()),
                    ]);

                    ListItem::new(line)
                })
                .collect();

            let list = List::new(items).block(block);
            f.render_widget(list, area);
        } else {
            let inner = block.inner(area);
            f.render_widget(block, area);
            let empty = Paragraph::new("No recovery in progress").style(styles.text_muted());
            f.render_widget(empty, inner);
        }
    }

    /// Render action buttons
    fn render_actions(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let block = Block::default()
            .title(" Actions ")
            .borders(Borders::ALL)
            .border_style(if self.action_focused {
                styles.border_focused()
            } else {
                styles.border()
            });

        let inner = block.inner(area);
        f.render_widget(block, area);

        let mut actions = Vec::new();

        if self.can_start() {
            actions.push(Span::styled("[S] ", styles.text_highlight()));
            actions.push(Span::styled("Start Recovery  ", styles.text()));
        }

        if self.can_cancel() {
            actions.push(Span::styled("[X] ", styles.text_warning()));
            actions.push(Span::styled("Cancel Recovery  ", styles.text()));
        }

        if self.can_complete() {
            actions.push(Span::styled("[Enter] ", styles.text_success()));
            actions.push(Span::styled("Complete Recovery  ", styles.text()));
        }

        if actions.is_empty() {
            actions.push(Span::styled("No actions available", styles.text_muted()));
        }

        let action_line = Line::from(actions);
        let actions_para = Paragraph::new(action_line).wrap(Wrap { trim: true });
        f.render_widget(actions_para, inner);
    }

    /// Render help text
    fn render_help(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let help_text = match self.state() {
            RecoveryState::None => {
                "Press 'S' to begin the account recovery process. You will need approval from your guardians."
            }
            RecoveryState::Initiated => {
                "Waiting for guardian approvals. Contact your guardians and ask them to approve the recovery request."
            }
            RecoveryState::ThresholdMet => {
                "Sufficient approvals received! Press Enter to complete the recovery and restore your account."
            }
            RecoveryState::InProgress => {
                "Reconstructing your keys from guardian shares. Please wait..."
            }
            RecoveryState::Completed => {
                "Recovery complete! Your account has been restored."
            }
            RecoveryState::Failed => {
                "Recovery failed. Press 'S' to try again."
            }
            _ => "Press '?' for help.",
        };

        let help = Paragraph::new(help_text)
            .style(styles.text_muted())
            .wrap(Wrap { trim: true });

        f.render_widget(help, area);
    }
}

impl Default for RecoveryScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen for RecoveryScreen {
    fn screen_type(&self) -> ScreenType {
        ScreenType::Recovery
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<InputAction> {
        match key.code {
            KeyCode::Char('s') | KeyCode::Char('S') if self.can_start() => {
                Some(InputAction::Submit("action:start_recovery".to_string()))
            }
            KeyCode::Char('x') | KeyCode::Char('X') if self.can_cancel() => {
                Some(InputAction::Submit("action:cancel_recovery".to_string()))
            }
            KeyCode::Enter if self.can_complete() => {
                Some(InputAction::Submit("action:complete_recovery".to_string()))
            }
            KeyCode::Tab => {
                self.action_focused = !self.action_focused;
                self.needs_redraw = true;
                None
            }
            _ => None,
        }
    }

    fn render(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        // Layout: status at top, progress, approvals, actions, help at bottom
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(5), // Status
                Constraint::Length(3), // Progress
                Constraint::Min(8),    // Approvals
                Constraint::Length(5), // Actions
                Constraint::Length(3), // Help
            ])
            .split(area);

        self.render_status(f, chunks[0], styles);
        self.render_progress(f, chunks[1], styles);
        self.render_approvals(f, chunks[2], styles);
        self.render_actions(f, chunks[3], styles);
        self.render_help(f, chunks[4], styles);
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
    let hours = (ts / 3600000) % 24;
    let minutes = (ts / 60000) % 60;
    format!("{:02}:{:02}", hours, minutes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::reactive::GuardianApproval;

    #[test]
    fn test_recovery_screen_new() {
        let screen = RecoveryScreen::new();
        assert_eq!(screen.screen_type(), ScreenType::Recovery);
        assert!(screen.status.is_none());
        assert!(screen.can_start());
    }

    #[test]
    fn test_state_transitions() {
        let mut screen = RecoveryScreen::new();

        // Initial state
        assert!(screen.can_start());
        assert!(!screen.can_cancel());
        assert!(!screen.can_complete());

        // Set initiated state
        screen.set_status(RecoveryStatus {
            state: RecoveryState::Initiated,
            session_id: Some("test-session".to_string()),
            approvals_received: 1,
            threshold: 2,
            total_guardians: 3,
            approvals: vec![
                GuardianApproval {
                    guardian_id: "g1".to_string(),
                    guardian_name: "Alice".to_string(),
                    approved: true,
                    timestamp: Some(1000),
                },
                GuardianApproval {
                    guardian_id: "g2".to_string(),
                    guardian_name: "Bob".to_string(),
                    approved: false,
                    timestamp: None,
                },
            ],
            started_at: Some(500),
            expires_at: None,
            error: None,
        });

        assert!(!screen.can_start());
        assert!(screen.can_cancel());
        assert!(!screen.can_complete());

        // Set threshold met
        screen.set_status(RecoveryStatus {
            state: RecoveryState::ThresholdMet,
            session_id: Some("test-session".to_string()),
            approvals_received: 2,
            threshold: 2,
            total_guardians: 3,
            approvals: vec![
                GuardianApproval {
                    guardian_id: "g1".to_string(),
                    guardian_name: "Alice".to_string(),
                    approved: true,
                    timestamp: Some(1000),
                },
                GuardianApproval {
                    guardian_id: "g2".to_string(),
                    guardian_name: "Bob".to_string(),
                    approved: true,
                    timestamp: Some(1500),
                },
            ],
            started_at: Some(500),
            expires_at: None,
            error: None,
        });

        assert!(!screen.can_start());
        assert!(screen.can_cancel());
        assert!(screen.can_complete());
    }

    #[test]
    fn test_completed_state() {
        let mut screen = RecoveryScreen::new();
        screen.set_status(RecoveryStatus {
            state: RecoveryState::Completed,
            session_id: Some("test-session".to_string()),
            approvals_received: 2,
            threshold: 2,
            total_guardians: 3,
            approvals: vec![],
            started_at: Some(500),
            expires_at: None,
            error: None,
        });

        assert!(!screen.can_start());
        assert!(!screen.can_cancel());
        assert!(!screen.can_complete());
    }
}
