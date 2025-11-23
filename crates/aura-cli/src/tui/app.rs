//! # Main TUI Application
//!
//! Core application logic for Bob's demo TUI interface.

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::Backend,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Margin},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, List, ListItem, Paragraph, Wrap},
    Frame, Terminal,
};
use std::io::{self, Stdout};
use tokio::sync::mpsc;

use super::state::{AppState, DemoPhase};
use crate::tui::screens::ScreenType;

/// Main TUI application for Bob's demo
pub struct TuiApp {
    /// Current application state
    state: AppState,
    /// Current screen
    current_screen: ScreenType,
    /// Input mode
    input_mode: InputMode,
    /// Current input buffer
    input_buffer: String,
    /// Should quit application
    should_quit: bool,
    /// Demo automation channel
    demo_tx: Option<mpsc::UnboundedSender<DemoEvent>>,
    /// Whether to show help
    show_help: bool,
}

/// Input mode for the TUI
#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    Normal,
    Editing,
}

/// Events that can be sent to the demo orchestration
#[derive(Debug, Clone)]
pub enum DemoEvent {
    AdvancePhase,
    SendMessage(String),
    InitiateRecovery,
    GuardianApproval(aura_core::AuthorityId),
    RestoreMessages,
    Reset,
}

impl Default for TuiApp {
    fn default() -> Self {
        Self::new()
    }
}

impl TuiApp {
    /// Create new TUI application
    pub fn new() -> Self {
        Self {
            state: AppState::new(),
            current_screen: ScreenType::Welcome,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            should_quit: false,
            demo_tx: None,
            show_help: false,
        }
    }

    /// Set the demo event sender
    pub fn set_demo_sender(&mut self, tx: mpsc::UnboundedSender<DemoEvent>) {
        self.demo_tx = Some(tx);
    }

    /// Run the TUI application
    pub async fn run(&mut self) -> io::Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let res = self.run_app(&mut terminal).await;

        // Restore terminal
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        if let Err(err) = res {
            println!("{:?}", err)
        }

        Ok(())
    }

    /// Main application loop
    async fn run_app<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> io::Result<()> {
        loop {
            terminal.draw(|f| self.ui(f))?;

            if let Ok(true) = event::poll(std::time::Duration::from_millis(100)) {
                if let Event::Key(key) = event::read()? {
                    match self.input_mode {
                        InputMode::Normal => {
                            if self.handle_normal_input(key.code).await {
                                break;
                            }
                        }
                        InputMode::Editing => {
                            if self.handle_edit_input(key.code).await {
                                break;
                            }
                        }
                    }
                }
            }

            if self.should_quit {
                break;
            }
        }
        Ok(())
    }

    /// Handle input in normal mode
    async fn handle_normal_input(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Char('q') => {
                self.should_quit = true;
                return true;
            }
            KeyCode::Char('h') => self.show_help = !self.show_help,
            KeyCode::Char('n') | KeyCode::Right => self.advance_phase().await,
            KeyCode::Char('p') | KeyCode::Left => self.previous_phase(),
            KeyCode::Char('r') => self.reset_demo().await,
            KeyCode::Char('i') => {
                if self.state.current_phase == DemoPhase::GroupChat {
                    self.input_mode = InputMode::Editing;
                }
            }
            KeyCode::Char('s') => {
                if self.state.current_phase == DemoPhase::DataLoss {
                    self.initiate_recovery().await;
                }
            }
            KeyCode::Char('a') => self.alice_approves().await,
            KeyCode::Char('c') => self.charlie_approves().await,
            KeyCode::Tab => self.switch_screen(),
            _ => {}
        }
        false
    }

    /// Handle input in editing mode
    async fn handle_edit_input(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Enter => {
                if !self.input_buffer.is_empty() {
                    self.send_message(self.input_buffer.clone()).await;
                    self.input_buffer.clear();
                }
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Char(c) => {
                self.input_buffer.push(c);
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
            }
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.input_buffer.clear();
            }
            _ => {}
        }
        false
    }

    /// Advance to next phase
    async fn advance_phase(&mut self) {
        if let Some(tx) = &self.demo_tx {
            let _ = tx.send(DemoEvent::AdvancePhase);
        }
        self.state.advance_phase();
        self.update_screen();
    }

    /// Go to previous phase
    fn previous_phase(&mut self) {
        use DemoPhase::*;
        self.state.current_phase = match self.state.current_phase {
            Welcome => Welcome,
            BobOnboarding => Welcome,
            GuardianSetup => BobOnboarding,
            GroupChat => GuardianSetup,
            DataLoss => GroupChat,
            Recovery => DataLoss,
            Restoration => Recovery,
            Completed => Restoration,
        };
        self.update_screen();
    }

    /// Reset demo to beginning
    async fn reset_demo(&mut self) {
        if let Some(tx) = &self.demo_tx {
            let _ = tx.send(DemoEvent::Reset);
        }
        self.state = AppState::new();
        self.current_screen = ScreenType::Welcome;
    }

    /// Send a message in group chat
    async fn send_message(&mut self, content: String) {
        if let Some(tx) = &self.demo_tx {
            let _ = tx.send(DemoEvent::SendMessage(content.clone()));
        }
        self.state.add_status(format!("Bob: {}", content));
    }

    /// Initiate recovery process
    async fn initiate_recovery(&mut self) {
        if let Some(tx) = &self.demo_tx {
            let _ = tx.send(DemoEvent::InitiateRecovery);
        }
        self.state.initiate_recovery();
    }

    /// Alice approves recovery
    async fn alice_approves(&mut self) {
        if let Some(alice_id) = self.state.alice_authority {
            if let Some(tx) = &self.demo_tx {
                let _ = tx.send(DemoEvent::GuardianApproval(alice_id));
            }
            self.state.guardian_approves_recovery(alice_id);
        }
    }

    /// Charlie approves recovery
    async fn charlie_approves(&mut self) {
        if let Some(charlie_id) = self.state.charlie_authority {
            if let Some(tx) = &self.demo_tx {
                let _ = tx.send(DemoEvent::GuardianApproval(charlie_id));
            }
            self.state.guardian_approves_recovery(charlie_id);
        }
    }

    /// Switch between screens
    fn switch_screen(&mut self) {
        use ScreenType::*;
        self.current_screen = match self.current_screen {
            Welcome => Demo,
            Demo => Guardian,
            Guardian => Technical,
            Technical => Welcome,
        };
    }

    /// Update screen based on current phase
    fn update_screen(&mut self) {
        use DemoPhase::*;
        self.current_screen = match self.state.current_phase {
            Welcome => ScreenType::Welcome,
            BobOnboarding | GuardianSetup | GroupChat | DataLoss | Recovery | Restoration
            | Completed => ScreenType::Demo,
        };
    }

    /// Render the UI
    fn ui(&self, f: &mut Frame<'_>) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([Constraint::Min(1), Constraint::Length(3)].as_ref())
            .split(f.size());

        // Main content area
        self.render_main_content(f, chunks[0]);

        // Status bar
        self.render_status_bar(f, chunks[1]);

        // Help overlay
        if self.show_help {
            self.render_help_overlay(f);
        }
    }

    /// Render main content based on current screen
    fn render_main_content(&self, f: &mut Frame<'_>, area: ratatui::layout::Rect) {
        match self.current_screen {
            ScreenType::Welcome => self.render_welcome_screen(f, area),
            ScreenType::Demo => self.render_demo_screen(f, area),
            ScreenType::Guardian => self.render_guardian_screen(f, area),
            ScreenType::Technical => self.render_technical_screen(f, area),
        }
    }

    /// Render welcome screen
    fn render_welcome_screen(&self, f: &mut Frame<'_>, area: ratatui::layout::Rect) {
        let welcome_text = vec![
            Line::from(vec![
                Span::styled("Welcome to ", Style::default()),
                Span::styled(
                    "Aura",
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" - Threshold Identity Demo", Style::default()),
            ]),
            Line::from(""),
            Line::from("This demo shows Bob's journey through:"),
            Line::from("• Identity creation with social guardians"),
            Line::from("• Secure group messaging"),
            Line::from("• Complete device loss"),
            Line::from("• Guardian-assisted recovery"),
            Line::from("• Message history restoration"),
            Line::from(""),
            Line::from("Press 'n' to begin Bob's journey..."),
            Line::from("Press 'h' for help, 'q' to quit"),
        ];

        let welcome = Paragraph::new(welcome_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Bob's Recovery Demo"),
            )
            .wrap(Wrap { trim: true });

        f.render_widget(welcome, area);
    }

    /// Render main demo screen
    fn render_demo_screen(&self, f: &mut Frame<'_>, area: ratatui::layout::Rect) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)].as_ref())
            .split(area);

        // Main demo content
        self.render_phase_content(f, chunks[0]);

        // Side panel with status
        self.render_side_panel(f, chunks[1]);
    }

    /// Render current phase content
    fn render_phase_content(&self, f: &mut Frame<'_>, area: ratatui::layout::Rect) {
        let title = self.state.phase_description();

        let content = match self.state.current_phase {
            DemoPhase::Welcome => vec![Line::from("Welcome screen")],
            DemoPhase::BobOnboarding => vec![
                Line::from("🔐 Creating Bob's threshold identity..."),
                Line::from("• Generating cryptographic keys"),
                Line::from("• Setting up 2-of-3 guardian threshold"),
                Line::from("• Registering with Aura network"),
            ],
            DemoPhase::GuardianSetup => vec![
                Line::from("👥 Setting up guardians..."),
                Line::from("• Alice: Guardian #1"),
                Line::from("• Charlie: Guardian #2"),
                Line::from("• Threshold: 2-of-3 guardians required"),
            ],
            DemoPhase::GroupChat => {
                let mut lines = vec![Line::from("💬 Group Chat Active"), Line::from("")];

                // Show recent messages
                for message in self.state.message_history.iter().rev().take(10).rev() {
                    lines.push(Line::from(format!(
                        "  {}: {}",
                        message.sender_id, message.content
                    )));
                }

                if self.input_mode == InputMode::Editing {
                    lines.push(Line::from(""));
                    lines.push(Line::from(format!("> {}_", self.input_buffer)));
                } else {
                    lines.push(Line::from(""));
                    lines.push(Line::from("Press 'i' to send a message"));
                }

                lines
            }
            DemoPhase::DataLoss => vec![
                Line::from("⚠️  Device Data Loss Simulation"),
                Line::from(""),
                Line::from("Oh no! Bob's device has failed completely:"),
                Line::from("• All local data lost"),
                Line::from("• Private keys gone"),
                Line::from("• Message history deleted"),
                Line::from(""),
                Line::from("Press 's' to start guardian recovery"),
            ],
            DemoPhase::Recovery => {
                let mut lines = vec![
                    Line::from("🔄 Guardian Recovery Process"),
                    Line::from(""),
                    Line::from(format!("Status: {}", self.state.recovery_status())),
                    Line::from(""),
                ];

                for guardian in self.state.guardian_states.values() {
                    let status = if guardian.approved_recovery {
                        "✅"
                    } else {
                        "⏳"
                    };
                    lines.push(Line::from(format!(
                        "{} {}: {}",
                        status,
                        guardian.name,
                        if guardian.approved_recovery {
                            "Approved"
                        } else {
                            "Pending"
                        }
                    )));
                }

                if self.state.recovery_progress.approvals < self.state.recovery_progress.threshold {
                    lines.push(Line::from(""));
                    lines.push(Line::from("Press 'a' for Alice to approve"));
                    lines.push(Line::from("Press 'c' for Charlie to approve"));
                }

                lines
            }
            DemoPhase::Restoration => vec![
                Line::from("✅ Data Restoration Complete"),
                Line::from(""),
                Line::from(format!(
                    "Restored {} messages",
                    self.state.message_history.len()
                )),
                Line::from("Guardian recovery successful!"),
                Line::from("Bob can continue chatting..."),
            ],
            DemoPhase::Completed => vec![
                Line::from("🎉 Demo Complete!"),
                Line::from(""),
                Line::from("Bob's journey summary:"),
                Line::from("✅ Identity created with guardians"),
                Line::from("✅ Secure group messaging"),
                Line::from("✅ Complete device failure"),
                Line::from("✅ Guardian-assisted recovery"),
                Line::from("✅ Full data restoration"),
            ],
        };

        let block = Block::default().borders(Borders::ALL).title(title);

        let paragraph = Paragraph::new(content)
            .block(block)
            .wrap(Wrap { trim: true });

        f.render_widget(paragraph, area);
    }

    /// Render side panel with status and progress
    fn render_side_panel(&self, f: &mut Frame<'_>, area: ratatui::layout::Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(6), Constraint::Min(1)].as_ref())
            .split(area);

        // Progress gauge
        self.render_progress_gauge(f, chunks[0]);

        // Status messages
        self.render_status_messages(f, chunks[1]);
    }

    /// Render progress gauge
    fn render_progress_gauge(&self, f: &mut Frame<'_>, area: ratatui::layout::Rect) {
        let progress = match self.state.current_phase {
            DemoPhase::Welcome => 0,
            DemoPhase::BobOnboarding => 15,
            DemoPhase::GuardianSetup => 30,
            DemoPhase::GroupChat => 45,
            DemoPhase::DataLoss => 60,
            DemoPhase::Recovery => 75,
            DemoPhase::Restoration => 90,
            DemoPhase::Completed => 100,
        };

        let gauge = Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Demo Progress"),
            )
            .gauge_style(Style::default().fg(Color::Blue))
            .percent(progress);

        f.render_widget(gauge, area);
    }

    /// Render status messages
    fn render_status_messages(&self, f: &mut Frame<'_>, area: ratatui::layout::Rect) {
        let items: Vec<ListItem> = self
            .state
            .status_messages
            .iter()
            .rev()
            .take(20)
            .map(|msg| ListItem::new(msg.as_str()))
            .collect();

        let list = List::new(items).block(Block::default().borders(Borders::ALL).title("Status"));

        f.render_widget(list, area);
    }

    /// Render guardian screen (placeholder for now)
    fn render_guardian_screen(&self, f: &mut Frame<'_>, area: ratatui::layout::Rect) {
        let text = vec![
            Line::from("Guardian Interface"),
            Line::from("(Alice/Charlie perspective)"),
            Line::from(""),
            Line::from("Coming soon..."),
        ];

        let paragraph = Paragraph::new(text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Guardian View"),
            )
            .wrap(Wrap { trim: true });

        f.render_widget(paragraph, area);
    }

    /// Render technical details screen (placeholder for now)
    fn render_technical_screen(&self, f: &mut Frame<'_>, area: ratatui::layout::Rect) {
        let text = vec![
            Line::from("Technical Details"),
            Line::from("• FROST threshold signatures"),
            Line::from("• AMP messaging protocol"),
            Line::from("• Guardian coordination"),
            Line::from("• Recovery protocol"),
        ];

        let paragraph = Paragraph::new(text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Technical View"),
            )
            .wrap(Wrap { trim: true });

        f.render_widget(paragraph, area);
    }

    /// Render status bar
    fn render_status_bar(&self, f: &mut Frame<'_>, area: ratatui::layout::Rect) {
        let mode_text = match self.input_mode {
            InputMode::Normal => "NORMAL",
            InputMode::Editing => "EDIT",
        };

        let text = vec![Line::from(vec![
            Span::styled(
                format!(" {} ", mode_text),
                Style::default().fg(Color::Black).bg(Color::Blue),
            ),
            Span::raw(" | "),
            Span::raw(format!("Screen: {:?}", self.current_screen)),
            Span::raw(" | "),
            Span::raw("Tab: Switch | h: Help | q: Quit"),
        ])];

        let status = Paragraph::new(text).block(Block::default().borders(Borders::ALL));

        f.render_widget(status, area);
    }

    /// Render help overlay
    fn render_help_overlay(&self, f: &mut Frame<'_>) {
        let area = centered_rect(60, 70, f.size());

        let help_text = vec![
            Line::from("Aura Demo Controls"),
            Line::from(""),
            Line::from("Navigation:"),
            Line::from("  n / →     Next phase"),
            Line::from("  p / ←     Previous phase"),
            Line::from("  Tab       Switch screen"),
            Line::from("  r         Reset demo"),
            Line::from(""),
            Line::from("Group Chat (when active):"),
            Line::from("  i         Send message"),
            Line::from(""),
            Line::from("Recovery (when active):"),
            Line::from("  s         Start recovery"),
            Line::from("  a         Alice approves"),
            Line::from("  c         Charlie approves"),
            Line::from(""),
            Line::from("General:"),
            Line::from("  h         Toggle help"),
            Line::from("  q         Quit"),
            Line::from(""),
            Line::from("Press 'h' to close help"),
        ];

        f.render_widget(Clear, area);
        let help = Paragraph::new(help_text)
            .block(Block::default().borders(Borders::ALL).title("Help"))
            .wrap(Wrap { trim: true });

        f.render_widget(help, area);
    }

    /// Get mutable reference to app state
    pub fn state_mut(&mut self) -> &mut AppState {
        &mut self.state
    }

    /// Get reference to app state
    pub fn state(&self) -> &AppState {
        &self.state
    }
}

/// Helper function to create a centered rect
fn centered_rect(
    percent_x: u16,
    percent_y: u16,
    r: ratatui::layout::Rect,
) -> ratatui::layout::Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
