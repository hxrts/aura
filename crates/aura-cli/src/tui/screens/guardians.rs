//! # Guardians Screen
//!
//! Display and manage guardian relationships.
//! Shows threshold configuration, guardian status, and key share info.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use super::{Screen, ScreenType};
use crate::tui::input::InputAction;
use crate::tui::reactive::{Guardian, GuardianStatus};
use crate::tui::styles::Styles;

/// Guardians screen state
pub struct GuardiansScreen {
    /// List of guardians
    guardians: Vec<Guardian>,
    /// Selected guardian index
    selected: Option<usize>,
    /// List state for rendering
    list_state: ListState,
    /// Threshold configuration
    threshold: Option<ThresholdInfo>,
    /// Whether detail panel is focused
    detail_focused: bool,
    /// Flag for redraw
    needs_redraw: bool,
}

/// Threshold configuration display info
#[derive(Clone)]
pub struct ThresholdInfo {
    /// Required number of guardians
    pub required: u32,
    /// Total number of guardians
    pub total: u32,
    /// Whether threshold is currently met
    pub met: bool,
}

impl GuardiansScreen {
    /// Create a new guardians screen
    pub fn new() -> Self {
        Self {
            guardians: Vec::new(),
            selected: None,
            list_state: ListState::default(),
            threshold: None,
            detail_focused: false,
            needs_redraw: true,
        }
    }

    /// Set guardians from view data
    pub fn set_guardians(&mut self, guardians: Vec<Guardian>) {
        self.guardians = guardians;
        if self.selected.is_none() && !self.guardians.is_empty() {
            self.selected = Some(0);
            self.list_state.select(Some(0));
        }
        self.needs_redraw = true;
    }

    /// Set threshold configuration
    pub fn set_threshold(&mut self, required: u32, total: u32) {
        let active_count = self
            .guardians
            .iter()
            .filter(|g| g.status == GuardianStatus::Active)
            .count() as u32;

        self.threshold = Some(ThresholdInfo {
            required,
            total,
            met: active_count >= required,
        });
        self.needs_redraw = true;
    }

    /// Get currently selected guardian
    pub fn selected_guardian(&self) -> Option<&Guardian> {
        self.selected.and_then(|idx| self.guardians.get(idx))
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
        if let Some(selected) = self.selected {
            if selected + 1 < self.guardians.len() {
                self.selected = Some(selected + 1);
                self.list_state.select(Some(selected + 1));
                self.needs_redraw = true;
            }
        }
    }

    /// Render the guardian list
    fn render_list(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let items: Vec<ListItem> = self
            .guardians
            .iter()
            .map(|guardian| {
                let status_icon = match guardian.status {
                    GuardianStatus::Active => ("●", styles.text_success()),
                    GuardianStatus::Pending => ("○", styles.text_warning()),
                    GuardianStatus::Offline => ("◌", styles.text_muted()),
                    GuardianStatus::Declined => ("✗", styles.text_error()),
                    GuardianStatus::Removed => ("✗", styles.text_muted()),
                };

                let share_indicator = if guardian.share_index.is_some() {
                    Span::styled(" [share]", styles.text_highlight())
                } else {
                    Span::styled("", styles.text_muted())
                };

                let line = Line::from(vec![
                    Span::styled(format!("{} ", status_icon.0), status_icon.1),
                    Span::styled(&guardian.name, styles.text()),
                    share_indicator,
                ]);

                ListItem::new(line)
            })
            .collect();

        let block = Block::default()
            .title(" Guardians ")
            .borders(Borders::ALL)
            .border_style(if !self.detail_focused {
                styles.border_focused()
            } else {
                styles.border()
            });

        let list = List::new(items).block(block).highlight_style(
            Style::default()
                .add_modifier(Modifier::BOLD)
                .bg(styles.palette.surface),
        );

        let mut state = self.list_state.clone();
        f.render_stateful_widget(list, area, &mut state);
    }

    /// Render the detail panel
    fn render_detail(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let block = Block::default()
            .title(" Details ")
            .borders(Borders::ALL)
            .border_style(if self.detail_focused {
                styles.border_focused()
            } else {
                styles.border()
            });

        let inner = block.inner(area);
        f.render_widget(block, area);

        if let Some(guardian) = self.selected_guardian() {
            let status_line = match guardian.status {
                GuardianStatus::Active => ("Active", styles.text_success()),
                GuardianStatus::Pending => ("Pending", styles.text_warning()),
                GuardianStatus::Offline => ("Offline", styles.text_muted()),
                GuardianStatus::Declined => ("Declined", styles.text_error()),
                GuardianStatus::Removed => ("Removed", styles.text_muted()),
            };

            let mut lines = vec![
                Line::from(vec![
                    Span::styled("Name: ", styles.text_muted()),
                    Span::styled(&guardian.name, styles.text()),
                ]),
                Line::from(vec![
                    Span::styled("Status: ", styles.text_muted()),
                    Span::styled(status_line.0, status_line.1),
                ]),
                Line::from(vec![
                    Span::styled("ID: ", styles.text_muted()),
                    Span::styled(&guardian.authority_id, styles.text_muted()),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("Has Share: ", styles.text_muted()),
                    Span::styled(
                        if guardian.share_index.is_some() {
                            "Yes"
                        } else {
                            "No"
                        },
                        if guardian.share_index.is_some() {
                            styles.text_success()
                        } else {
                            styles.text_muted()
                        },
                    ),
                ]),
                Line::from(""),
            ];

            if let Some(ts) = guardian.last_seen {
                lines.push(Line::from(vec![
                    Span::styled("Last Seen: ", styles.text_muted()),
                    Span::styled(format_timestamp(ts), styles.text_muted()),
                ]));
            }

            let detail = Paragraph::new(lines).wrap(Wrap { trim: true });
            f.render_widget(detail, inner);
        } else {
            let empty =
                Paragraph::new("Select a guardian to view details").style(styles.text_muted());
            f.render_widget(empty, inner);
        }
    }

    /// Render threshold status
    fn render_threshold(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let block = Block::default()
            .title(" Threshold ")
            .borders(Borders::ALL)
            .border_style(styles.border());

        let inner = block.inner(area);
        f.render_widget(block, area);

        if let Some(ref threshold) = self.threshold {
            let status_style = if threshold.met {
                styles.text_success()
            } else {
                styles.text_warning()
            };

            let lines = vec![
                Line::from(vec![
                    Span::styled("Required: ", styles.text_muted()),
                    Span::styled(
                        format!("{} of {}", threshold.required, threshold.total),
                        styles.text(),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Status: ", styles.text_muted()),
                    Span::styled(
                        if threshold.met {
                            "✓ Met"
                        } else {
                            "✗ Not Met"
                        },
                        status_style,
                    ),
                ]),
                Line::from(""),
                Line::from(vec![Span::styled(
                    format!(
                        "{} active guardian{}",
                        self.guardians
                            .iter()
                            .filter(|g| g.status == GuardianStatus::Active)
                            .count(),
                        if self
                            .guardians
                            .iter()
                            .filter(|g| g.status == GuardianStatus::Active)
                            .count()
                            == 1
                        {
                            ""
                        } else {
                            "s"
                        }
                    ),
                    styles.text_muted(),
                )]),
            ];

            let threshold_info = Paragraph::new(lines);
            f.render_widget(threshold_info, inner);
        } else {
            let empty = Paragraph::new("No threshold configured").style(styles.text_muted());
            f.render_widget(empty, inner);
        }
    }
}

impl Default for GuardiansScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen for GuardiansScreen {
    fn screen_type(&self) -> ScreenType {
        ScreenType::Guardians
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
                self.detail_focused = !self.detail_focused;
                self.needs_redraw = true;
                None
            }
            KeyCode::Enter => {
                // Could trigger action on guardian
                None
            }
            _ => None,
        }
    }

    fn render(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        // Layout: list + details side by side, threshold at bottom
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(10),   // Main content
                Constraint::Length(6), // Threshold status
            ])
            .split(area);

        let content_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(40), // Guardian list
                Constraint::Percentage(60), // Details
            ])
            .split(main_chunks[0]);

        self.render_list(f, content_chunks[0], styles);
        self.render_detail(f, content_chunks[1], styles);
        self.render_threshold(f, main_chunks[1], styles);
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

    #[test]
    fn test_guardians_screen_new() {
        let screen = GuardiansScreen::new();
        assert_eq!(screen.screen_type(), ScreenType::Guardians);
        assert!(screen.guardians.is_empty());
    }

    #[test]
    fn test_set_guardians() {
        let mut screen = GuardiansScreen::new();
        let guardians = vec![
            Guardian {
                authority_id: "g1".to_string(),
                name: "Alice".to_string(),
                status: GuardianStatus::Active,
                added_at: 1000,
                last_seen: Some(2000),
                share_index: Some(1),
            },
            Guardian {
                authority_id: "g2".to_string(),
                name: "Bob".to_string(),
                status: GuardianStatus::Pending,
                added_at: 1500,
                last_seen: None,
                share_index: None,
            },
        ];
        screen.set_guardians(guardians);
        assert_eq!(screen.guardians.len(), 2);
        assert_eq!(screen.selected, Some(0));
    }

    #[test]
    fn test_threshold_info() {
        let mut screen = GuardiansScreen::new();
        let guardians = vec![
            Guardian {
                authority_id: "g1".to_string(),
                name: "Alice".to_string(),
                status: GuardianStatus::Active,
                added_at: 0,
                last_seen: None,
                share_index: Some(1),
            },
            Guardian {
                authority_id: "g2".to_string(),
                name: "Bob".to_string(),
                status: GuardianStatus::Active,
                added_at: 0,
                last_seen: None,
                share_index: Some(2),
            },
        ];
        screen.set_guardians(guardians);
        screen.set_threshold(2, 3);

        assert!(screen.threshold.is_some());
        let threshold = screen.threshold.as_ref().unwrap();
        assert_eq!(threshold.required, 2);
        assert_eq!(threshold.total, 3);
        assert!(threshold.met);
    }

    #[test]
    fn test_navigation() {
        let mut screen = GuardiansScreen::new();
        let guardians = vec![
            Guardian {
                authority_id: "g1".to_string(),
                name: "Alice".to_string(),
                status: GuardianStatus::Active,
                added_at: 0,
                last_seen: None,
                share_index: None,
            },
            Guardian {
                authority_id: "g2".to_string(),
                name: "Bob".to_string(),
                status: GuardianStatus::Pending,
                added_at: 0,
                last_seen: None,
                share_index: None,
            },
        ];
        screen.set_guardians(guardians);

        assert_eq!(screen.selected, Some(0));
        screen.select_next();
        assert_eq!(screen.selected, Some(1));
        screen.select_prev();
        assert_eq!(screen.selected, Some(0));
    }
}
