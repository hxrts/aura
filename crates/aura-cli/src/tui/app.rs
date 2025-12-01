//! # Unified Reactive TUI Application
//!
//! Main application using the reactive view system with synchronous
//! screen updates for testability and consistency.
//!
//! The TUI is backend-agnostic: the same code works for production
//! (real network) and demo mode (simulated backend). The only difference
//! is which EffectBridge implementation is injected via TuiContext.

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Tabs, Wrap},
    Frame, Terminal,
};
use std::io;
use tokio::sync::mpsc;

use super::context::TuiContext;
use super::demo::Tip;
use aura_core::AuthorityId;

/// Minimal demo events for coordinating the human-agent demo.
/// These are intentionally scoped to the current demo flow.
#[derive(Debug, Clone)]
pub enum DemoEvent {
    /// Advance to the next demo phase (triggered by Enter/Space)
    AdvancePhase,
    /// Send a chat message from Bob during the demo
    SendMessage(String),
    /// Begin the recovery process after device loss
    InitiateRecovery,
    /// A guardian approved the recovery (authority ID provided)
    GuardianApproval(AuthorityId),
    /// Restore Bob's messages after successful recovery
    RestoreMessages,
    /// Reset the demo state back to setup
    Reset,
}
use super::input::{InputAction, InputMode};
use super::screens::{
    BlockMessagesScreen, BlockScreen, ChatScreen, ContactsScreen, GuardiansScreen,
    InvitationsScreen, NeighborhoodScreen, RecoveryScreen, Screen, ScreenManager, ScreenType,
    WelcomeScreen,
};
use super::styles::Styles;

/// Main TUI application using the reactive system
pub struct TuiApp {
    /// Reactive context (Views, QueryExecutor, EffectBridge)
    ctx: TuiContext,
    /// Screen manager for navigation
    screen_manager: ScreenManager,
    /// Screen instances
    screens: Screens,
    /// Style configuration
    styles: Styles,
    /// Current input mode
    input_mode: InputMode,
    /// Input buffer for text editing
    input_buffer: String,
    /// Whether to show help overlay
    show_help: bool,
    /// Should quit application
    should_quit: bool,
    /// Demo event sender for orchestration
    demo_tx: Option<mpsc::UnboundedSender<DemoEvent>>,
    /// Cached tip for rendering (updated from async context)
    cached_tip: Option<Tip>,
}

/// Container for all screen instances
struct Screens {
    welcome: WelcomeScreen,
    chat: ChatScreen,
    guardians: GuardiansScreen,
    recovery: RecoveryScreen,
    invitations: InvitationsScreen,
    contacts: ContactsScreen,
    neighborhood: NeighborhoodScreen,
    block: BlockScreen,
    block_messages: BlockMessagesScreen,
}

impl Default for Screens {
    fn default() -> Self {
        Self {
            welcome: WelcomeScreen::new(),
            chat: ChatScreen::new(),
            guardians: GuardiansScreen::new(),
            recovery: RecoveryScreen::new(),
            invitations: InvitationsScreen::new(),
            contacts: ContactsScreen::new(),
            neighborhood: NeighborhoodScreen::new(),
            block: BlockScreen::new(),
            block_messages: BlockMessagesScreen::new(),
        }
    }
}

impl Default for TuiApp {
    fn default() -> Self {
        Self::new()
    }
}

impl TuiApp {
    /// Create a new TUI application
    pub fn new() -> Self {
        Self {
            ctx: TuiContext::with_defaults(),
            screen_manager: ScreenManager::new(ScreenType::Welcome),
            screens: Screens::default(),
            styles: Styles::default(),
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            show_help: false,
            should_quit: false,
            demo_tx: None,
            cached_tip: None,
        }
    }

    /// Create with a custom context
    pub fn with_context(ctx: TuiContext) -> Self {
        Self {
            ctx,
            screen_manager: ScreenManager::new(ScreenType::Welcome),
            screens: Screens::default(),
            demo_tx: None,
            styles: Styles::default(),
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            show_help: false,
            should_quit: false,
            cached_tip: None,
        }
    }

    /// Get the TUI context
    pub fn context(&self) -> &TuiContext {
        &self.ctx
    }

    /// Get mutable TUI context
    pub fn context_mut(&mut self) -> &mut TuiContext {
        &mut self.ctx
    }

    /// Set the demo event sender for orchestration (deprecated)
    ///
    /// This is used by the human-agent demo for coordination between
    /// the TUI and automated guardian agents. New implementations should
    /// use SimulatedBridge instead.
    #[deprecated(
        since = "0.1.0",
        note = "Use SimulatedBridge for new demo implementations"
    )]
    #[allow(deprecated)]
    pub fn set_demo_sender(&mut self, sender: mpsc::UnboundedSender<DemoEvent>) {
        self.demo_tx = Some(sender);
    }

    /// Run the TUI application
    pub async fn run(&mut self) -> io::Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let res = self.run_loop(&mut terminal).await;

        // Restore terminal
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        if let Err(err) = res {
            eprintln!("TUI error: {:?}", err);
        }

        Ok(())
    }

    /// Main application loop
    async fn run_loop<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> io::Result<()> {
        loop {
            // Sync data from Views to Screens (synchronous)
            self.sync_screens();

            // Update cached tip if in demo mode
            self.update_cached_tip().await;

            // Render
            terminal.draw(|f| self.render(f))?;

            // Handle input with timeout
            if let Ok(true) = event::poll(std::time::Duration::from_millis(50)) {
                if let Event::Key(key) = event::read()? {
                    self.handle_key(key).await;
                }
            }

            // Update screens
            self.update_screens();

            if self.should_quit {
                break;
            }
        }
        Ok(())
    }

    /// Synchronously sync data from reactive Views to Screens
    fn sync_screens(&mut self) {
        // Sync chat screen
        if let Some(channels) = self.ctx.chat_view().cached_channels() {
            self.screens.chat.set_channels(channels);
        }
        if let Some(messages) = self.ctx.chat_view().cached_messages() {
            self.screens.chat.set_messages(messages);
        }

        // Sync guardians screen
        if let Some(guardians) = self.ctx.guardians_view().cached_guardians() {
            self.screens.guardians.set_guardians(guardians);
        }
        if let Some(threshold) = self.ctx.guardians_view().cached_threshold() {
            self.screens
                .guardians
                .set_threshold(threshold.threshold, threshold.total);
        }

        // Sync recovery screen
        if let Some(status) = self.ctx.recovery_view().cached_status() {
            self.screens.recovery.set_status(status);
        }

        // Sync invitations screen
        if let Some(invitations) = self.ctx.invitations_view().cached_invitations() {
            self.screens.invitations.set_invitations(invitations);
        }
    }

    /// Update all screens (called on tick)
    fn update_screens(&mut self) {
        self.screens.welcome.update();
        self.screens.chat.update();
        self.screens.guardians.update();
        self.screens.recovery.update();
        self.screens.invitations.update();
        self.screens.contacts.update();
        self.screens.neighborhood.update();
        self.screens.block.update();
        self.screens.block_messages.update();
    }

    /// Handle key input
    async fn handle_key(&mut self, key: KeyEvent) {
        // Global keys first
        match key.code {
            KeyCode::Char('q') if self.input_mode == InputMode::Normal => {
                self.should_quit = true;
                return;
            }
            KeyCode::Char('?') if self.input_mode == InputMode::Normal => {
                self.show_help = !self.show_help;
                return;
            }
            KeyCode::Esc if self.show_help => {
                self.show_help = false;
                return;
            }
            _ => {}
        }

        // Help overlay captures all input when shown
        if self.show_help {
            if key.code == KeyCode::Char('?') || key.code == KeyCode::Esc {
                self.show_help = false;
            }
            return;
        }

        // Mode-specific handling
        match self.input_mode {
            InputMode::Normal => self.handle_normal_key(key).await,
            InputMode::Editing => self.handle_edit_key(key).await,
            InputMode::Command => self.handle_command_key(key).await,
        }
    }

    /// Handle keys in normal mode
    async fn handle_normal_key(&mut self, key: KeyEvent) {
        // Navigation between screens
        match key.code {
            KeyCode::Tab => {
                self.cycle_screen();
                return;
            }
            KeyCode::Char('1') => {
                self.screen_manager.navigate(ScreenType::Welcome);
                return;
            }
            KeyCode::Char('2') => {
                self.screen_manager.navigate(ScreenType::Chat);
                return;
            }
            KeyCode::Char('3') => {
                self.screen_manager.navigate(ScreenType::Guardians);
                return;
            }
            KeyCode::Char('4') => {
                self.screen_manager.navigate(ScreenType::Recovery);
                return;
            }
            KeyCode::Char('5') => {
                self.screen_manager.navigate(ScreenType::Invitations);
                return;
            }
            KeyCode::Char('6') => {
                self.screen_manager.navigate(ScreenType::Contacts);
                return;
            }
            KeyCode::Char('7') => {
                self.screen_manager.navigate(ScreenType::Neighborhood);
                return;
            }
            KeyCode::Char('8') => {
                self.screen_manager.navigate(ScreenType::Block);
                return;
            }
            KeyCode::Char('9') => {
                self.screen_manager.navigate(ScreenType::BlockMessages);
                return;
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                // Advance demo phase when in demo mode
                if let Some(tx) = &self.demo_tx {
                    let _ = tx.send(DemoEvent::AdvancePhase);
                }
                // Move off the welcome screen into chat to show progress
                self.screens.welcome.advance();
                self.screen_manager.navigate(ScreenType::Chat);
                return;
            }
            KeyCode::Char('i') if self.screen_manager.current() == ScreenType::Chat => {
                self.input_mode = InputMode::Editing;
                return;
            }
            _ => {}
        }

        // Delegate to current screen
        let action = self.current_screen_mut().handle_key(key);
        self.process_action(action).await;
    }

    /// Handle keys in editing mode
    async fn handle_edit_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.input_buffer.clear();
            }
            KeyCode::Enter => {
                if !self.input_buffer.is_empty() {
                    let content = std::mem::take(&mut self.input_buffer);
                    self.send_message(content).await;
                }
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Char(c) => {
                self.input_buffer.push(c);
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
            }
            _ => {}
        }
    }

    /// Handle keys in command mode
    async fn handle_command_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.input_buffer.clear();
            }
            KeyCode::Enter => {
                let cmd = std::mem::take(&mut self.input_buffer);
                self.execute_command(&cmd).await;
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Char(c) => {
                self.input_buffer.push(c);
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
            }
            _ => {}
        }
    }

    /// Process an input action from a screen
    async fn process_action(&mut self, action: Option<InputAction>) {
        if let Some(action) = action {
            match action {
                InputAction::SwitchScreen => {
                    self.cycle_screen();
                }
                InputAction::Submit(content) => {
                    // Handle action strings from screens
                    if content.starts_with("action:") {
                        self.handle_screen_action(&content).await;
                    }
                }
                InputAction::ExitToNormal => {
                    self.input_mode = InputMode::Normal;
                    self.input_buffer.clear();
                }
                InputAction::Command(cmd) => {
                    self.execute_irc_command(cmd).await;
                }
                InputAction::Error(msg) => {
                    // Currently surfaced via stderr until toast UI is wired
                    eprintln!("Error: {}", msg);
                }
                InputAction::Quit => {
                    self.should_quit = true;
                }
                _ => {}
            }
        }
    }

    /// Handle action strings from screens
    async fn handle_screen_action(&mut self, action: &str) {
        use super::effects::EffectCommand;

        match action {
            "action:start_recovery" => {
                let _ = self
                    .ctx
                    .bridge()
                    .dispatch(EffectCommand::StartRecovery)
                    .await;
            }
            "action:cancel_recovery" => {
                let _ = self
                    .ctx
                    .bridge()
                    .dispatch(EffectCommand::CancelRecovery)
                    .await;
            }
            "action:complete_recovery" => {
                let _ = self
                    .ctx
                    .bridge()
                    .dispatch(EffectCommand::CompleteRecovery)
                    .await;
            }
            _ if action.starts_with("action:accept_invitation:") => {
                if let Some(id) = action.strip_prefix("action:accept_invitation:") {
                    let _ = self
                        .ctx
                        .bridge()
                        .dispatch(EffectCommand::AcceptInvitation {
                            invitation_id: id.to_string(),
                        })
                        .await;
                }
            }
            _ if action.starts_with("action:decline_invitation:") => {
                if let Some(id) = action.strip_prefix("action:decline_invitation:") {
                    let _ = self
                        .ctx
                        .bridge()
                        .dispatch(EffectCommand::DeclineInvitation {
                            invitation_id: id.to_string(),
                        })
                        .await;
                }
            }
            _ => {}
        }
    }

    /// Execute an IRC command
    ///
    /// Maps IrcCommand variants to EffectCommand for dispatch via the effect bridge.
    /// User commands that don't require network I/O are handled locally.
    async fn execute_irc_command(&mut self, cmd: super::commands::IrcCommand) {
        use super::commands::IrcCommand;
        use super::effects::EffectCommand;

        match cmd {
            // === User Commands ===
            IrcCommand::Msg { target, text } => {
                let _ = self
                    .ctx
                    .bridge()
                    .dispatch(EffectCommand::SendDirectMessage {
                        target,
                        content: text,
                    })
                    .await;
            }
            IrcCommand::Me { action } => {
                let _ = self
                    .ctx
                    .bridge()
                    .dispatch(EffectCommand::SendAction {
                        channel: "general".to_string(),
                        action,
                    })
                    .await;
            }
            IrcCommand::Nick { name } => {
                let _ = self
                    .ctx
                    .bridge()
                    .dispatch(EffectCommand::UpdateNickname { name })
                    .await;
            }
            IrcCommand::Who => {
                let _ = self
                    .ctx
                    .bridge()
                    .dispatch(EffectCommand::ListParticipants {
                        channel: "general".to_string(),
                    })
                    .await;
            }
            IrcCommand::Whois { target } => {
                let _ = self
                    .ctx
                    .bridge()
                    .dispatch(EffectCommand::GetUserInfo { target })
                    .await;
            }
            IrcCommand::Leave => {
                self.should_quit = true;
            }
            IrcCommand::Help { .. } => {
                self.show_help = true;
            }
            IrcCommand::Join { channel } => {
                let _ = self
                    .ctx
                    .bridge()
                    .dispatch(EffectCommand::JoinChannel { channel })
                    .await;
            }

            // === Moderator Commands ===
            IrcCommand::Kick { target, reason } => {
                let _ = self
                    .ctx
                    .bridge()
                    .dispatch(EffectCommand::KickUser {
                        channel: "general".to_string(),
                        target,
                        reason,
                    })
                    .await;
            }
            IrcCommand::Ban { target, reason } => {
                let _ = self
                    .ctx
                    .bridge()
                    .dispatch(EffectCommand::BanUser { target, reason })
                    .await;
            }
            IrcCommand::Unban { target } => {
                let _ = self
                    .ctx
                    .bridge()
                    .dispatch(EffectCommand::UnbanUser { target })
                    .await;
            }
            IrcCommand::Mute { target, duration } => {
                let _ = self
                    .ctx
                    .bridge()
                    .dispatch(EffectCommand::MuteUser {
                        target,
                        duration_secs: duration.map(|d| d.as_secs()),
                    })
                    .await;
            }
            IrcCommand::Unmute { target } => {
                let _ = self
                    .ctx
                    .bridge()
                    .dispatch(EffectCommand::UnmuteUser { target })
                    .await;
            }
            IrcCommand::Invite { target } => {
                let _ = self
                    .ctx
                    .bridge()
                    .dispatch(EffectCommand::InviteUser { target })
                    .await;
            }
            IrcCommand::Topic { text } => {
                let _ = self
                    .ctx
                    .bridge()
                    .dispatch(EffectCommand::SetTopic {
                        channel: "general".to_string(),
                        text,
                    })
                    .await;
            }
            IrcCommand::Pin { message_id } => {
                let _ = self
                    .ctx
                    .bridge()
                    .dispatch(EffectCommand::PinMessage { message_id })
                    .await;
            }
            IrcCommand::Unpin { message_id } => {
                let _ = self
                    .ctx
                    .bridge()
                    .dispatch(EffectCommand::UnpinMessage { message_id })
                    .await;
            }

            // === Admin Commands ===
            IrcCommand::Op { target } => {
                let _ = self
                    .ctx
                    .bridge()
                    .dispatch(EffectCommand::GrantSteward { target })
                    .await;
            }
            IrcCommand::Deop { target } => {
                let _ = self
                    .ctx
                    .bridge()
                    .dispatch(EffectCommand::RevokeSteward { target })
                    .await;
            }
            IrcCommand::Mode { channel, flags } => {
                let _ = self
                    .ctx
                    .bridge()
                    .dispatch(EffectCommand::SetChannelMode { channel, flags })
                    .await;
            }
        }
    }

    /// Execute a text command
    async fn execute_command(&mut self, cmd: &str) {
        let trimmed = cmd.trim();
        if trimmed.is_empty() {
            return;
        }
        // Use default channel; context wiring for channel selection is not yet implemented
        let channel = "general".to_string();
        let irc_cmd = super::commands::IrcCommand::Msg {
            target: channel,
            text: trimmed.to_string(),
        };
        self.execute_irc_command(irc_cmd).await;
    }

    /// Send a message via the effect bridge
    async fn send_message(&mut self, content: String) {
        use super::effects::EffectCommand;
        // Uses default "general" channel until context wiring is available
        let _ = self
            .ctx
            .bridge()
            .dispatch(EffectCommand::SendMessage {
                channel: "general".to_string(),
                content,
            })
            .await;
    }

    /// Cycle to next screen
    fn cycle_screen(&mut self) {
        let next = match self.screen_manager.current() {
            ScreenType::Welcome => ScreenType::Chat,
            ScreenType::Chat => ScreenType::Guardians,
            ScreenType::Guardians => ScreenType::Recovery,
            ScreenType::Recovery => ScreenType::Invitations,
            ScreenType::Invitations => ScreenType::Contacts,
            ScreenType::Contacts => ScreenType::Neighborhood,
            ScreenType::Neighborhood => ScreenType::Block,
            ScreenType::Block => ScreenType::BlockMessages,
            ScreenType::BlockMessages => ScreenType::Welcome,
            _ => ScreenType::Welcome,
        };
        self.screen_manager.navigate(next);
    }

    /// Update cached tip from async context
    async fn update_cached_tip(&mut self) {
        if self.ctx.has_tip_provider() {
            self.cached_tip = self.ctx.current_tip(self.screen_manager.current()).await;
        }
    }

    /// Get mutable reference to current screen
    fn current_screen_mut(&mut self) -> &mut dyn Screen {
        match self.screen_manager.current() {
            ScreenType::Welcome => &mut self.screens.welcome,
            ScreenType::Chat => &mut self.screens.chat,
            ScreenType::Guardians => &mut self.screens.guardians,
            ScreenType::Recovery => &mut self.screens.recovery,
            ScreenType::Invitations => &mut self.screens.invitations,
            ScreenType::Contacts => &mut self.screens.contacts,
            ScreenType::Neighborhood => &mut self.screens.neighborhood,
            ScreenType::Block => &mut self.screens.block,
            ScreenType::BlockMessages => &mut self.screens.block_messages,
            _ => &mut self.screens.welcome,
        }
    }

    /// Get reference to current screen
    fn current_screen(&self) -> &dyn Screen {
        match self.screen_manager.current() {
            ScreenType::Welcome => &self.screens.welcome,
            ScreenType::Chat => &self.screens.chat,
            ScreenType::Guardians => &self.screens.guardians,
            ScreenType::Recovery => &self.screens.recovery,
            ScreenType::Invitations => &self.screens.invitations,
            ScreenType::Contacts => &self.screens.contacts,
            ScreenType::Neighborhood => &self.screens.neighborhood,
            ScreenType::Block => &self.screens.block,
            ScreenType::BlockMessages => &self.screens.block_messages,
            _ => &self.screens.welcome,
        }
    }

    /// Render the UI
    fn render(&self, f: &mut Frame<'_>) {
        // Conditional layout based on whether we have a tip to show
        let has_tip = self.cached_tip.is_some();

        let chunks = if has_tip {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(2), // Tab bar
                    Constraint::Min(1),    // Main content
                    Constraint::Length(2), // Tip bar
                    Constraint::Length(3), // Status bar
                ])
                .split(f.size())
        } else {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(2), // Tab bar
                    Constraint::Min(1),    // Main content
                    Constraint::Length(3), // Status bar
                ])
                .split(f.size())
        };

        // Tab bar
        self.render_tabs(f, chunks[0]);

        // Main screen content
        let panel = self
            .styles
            .panel(self.screen_manager.current().title())
            .border_style(self.styles.border());
        let inner = panel.inner(chunks[1]);
        f.render_widget(panel, chunks[1]);
        self.current_screen().render(f, inner, &self.styles);

        // Tip bar (if present)
        if has_tip {
            if let Some(ref tip) = self.cached_tip {
                self.render_tip_bar(f, tip, chunks[2]);
            }
            // Status bar with input
            self.render_status_bar(f, chunks[3]);
        } else {
            // Status bar with input
            self.render_status_bar(f, chunks[2]);
        }

        // Help overlay
        if self.show_help {
            self.render_help_overlay(f);
        }
    }

    /// Render the tip bar for demo mode
    fn render_tip_bar(&self, f: &mut Frame<'_>, tip: &Tip, area: Rect) {
        let content = if let Some(ref hint) = tip.action_hint {
            format!(" {} [{}]", tip.message, hint)
        } else {
            format!(" {}", tip.message)
        };

        let tip_line = Line::from(vec![
            Span::styled(" TIP ", Style::default().fg(Color::Black).bg(Color::Yellow)),
            Span::styled(content, Style::default().fg(Color::Yellow)),
        ]);

        let paragraph = Paragraph::new(tip_line);
        f.render_widget(paragraph, area);
    }

    /// Render tab bar
    fn render_tabs(&self, f: &mut Frame<'_>, area: Rect) {
        let titles = vec![
            "1:Home",
            "2:Chat",
            "3:Guardians",
            "4:Recovery",
            "5:Invitations",
            "6:Contacts",
            "7:Neighborhood",
            "8:Block",
            "9:Block Msgs",
        ];
        let selected = match self.screen_manager.current() {
            ScreenType::Welcome => 0,
            ScreenType::Chat => 1,
            ScreenType::Guardians => 2,
            ScreenType::Recovery => 3,
            ScreenType::Invitations => 4,
            ScreenType::Contacts => 5,
            ScreenType::Neighborhood => 6,
            ScreenType::Block => 7,
            ScreenType::BlockMessages => 8,
            _ => 0,
        };

        let tabs = Tabs::new(titles)
            .select(selected)
            .style(self.styles.text_muted())
            .highlight_style(
                Style::default()
                    .fg(self.styles.palette.primary)
                    .add_modifier(Modifier::BOLD),
            )
            .divider("|");

        f.render_widget(tabs, area);
    }

    /// Render status bar
    fn render_status_bar(&self, f: &mut Frame<'_>, area: Rect) {
        let mode_text = match self.input_mode {
            InputMode::Normal => "NORMAL",
            InputMode::Editing => "INSERT",
            InputMode::Command => "COMMAND",
        };

        let mode_style = match self.input_mode {
            InputMode::Normal => Style::default().fg(Color::Black).bg(Color::Blue),
            InputMode::Editing => Style::default().fg(Color::Black).bg(Color::Green),
            InputMode::Command => Style::default().fg(Color::Black).bg(Color::Yellow),
        };

        let content = if self.input_mode != InputMode::Normal {
            Line::from(vec![
                Span::styled(format!(" {} ", mode_text), mode_style),
                Span::raw(" > "),
                Span::raw(&self.input_buffer),
                Span::styled("_", Style::default().add_modifier(Modifier::SLOW_BLINK)),
            ])
        } else {
            Line::from(vec![
                Span::styled(format!(" {} ", mode_text), mode_style),
                Span::raw(" | "),
                Span::raw("Tab: Switch | ?: Help | q: Quit | i: Insert"),
            ])
        };

        let status = Paragraph::new(content).block(Block::default().borders(Borders::ALL));
        f.render_widget(status, area);
    }

    /// Render help overlay
    fn render_help_overlay(&self, f: &mut Frame<'_>) {
        let area = centered_rect(60, 70, f.size());

        let help_text = vec![
            Line::from(Span::styled(
                "Aura TUI Help",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("Navigation:"),
            Line::from("  1-5       Jump to screen"),
            Line::from("  Tab       Cycle screens"),
            Line::from("  j/k       Move up/down in lists"),
            Line::from(""),
            Line::from("Chat:"),
            Line::from("  i         Enter insert mode"),
            Line::from("  Enter     Send message"),
            Line::from("  Esc       Exit insert mode"),
            Line::from(""),
            Line::from("Recovery:"),
            Line::from("  s         Start recovery"),
            Line::from("  Enter     Complete (when ready)"),
            Line::from(""),
            Line::from("General:"),
            Line::from("  ?         Toggle help"),
            Line::from("  q         Quit"),
            Line::from(""),
            Line::from("Press ? or Esc to close"),
        ];

        f.render_widget(Clear, area);
        let help = Paragraph::new(help_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Help ")
                    .border_style(self.styles.border_focused()),
            )
            .wrap(Wrap { trim: true });

        f.render_widget(help, area);
    }
}

/// Helper to create a centered rect
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_app_creation() {
        let app = TuiApp::new();
        assert_eq!(app.screen_manager.current(), ScreenType::Welcome);
        assert!(!app.should_quit);
    }

    #[tokio::test]
    async fn test_screen_cycling() {
        let mut app = TuiApp::new();
        assert_eq!(app.screen_manager.current(), ScreenType::Welcome);

        app.cycle_screen();
        assert_eq!(app.screen_manager.current(), ScreenType::Chat);

        app.cycle_screen();
        assert_eq!(app.screen_manager.current(), ScreenType::Guardians);

        app.cycle_screen();
        assert_eq!(app.screen_manager.current(), ScreenType::Recovery);

        app.cycle_screen();
        assert_eq!(app.screen_manager.current(), ScreenType::Invitations);

        app.cycle_screen();
        assert_eq!(app.screen_manager.current(), ScreenType::Contacts);

        app.cycle_screen();
        assert_eq!(app.screen_manager.current(), ScreenType::Neighborhood);

        app.cycle_screen();
        assert_eq!(app.screen_manager.current(), ScreenType::Block);

        app.cycle_screen();
        assert_eq!(app.screen_manager.current(), ScreenType::BlockMessages);

        app.cycle_screen();
        assert_eq!(app.screen_manager.current(), ScreenType::Welcome);
    }
}
