//! # Welcome Screen
//!
//! Onboarding and welcome screen for new users.
//! Guides through initial setup and account creation.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame,
};

use super::{Screen, ScreenType};
use crate::tui::input::InputAction;
use crate::tui::layout::{LayoutPresets, ScreenLayout};
use crate::tui::styles::Styles;

/// Welcome screen state
pub struct WelcomeScreen {
    /// Current onboarding step
    step: OnboardingStep,
    /// Whether account exists
    has_account: bool,
    /// Account name (if provided)
    account_name: Option<String>,
    /// Flag for redraw
    needs_redraw: bool,
}

/// Onboarding steps
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnboardingStep {
    /// Initial welcome
    Welcome,
    /// Account creation
    CreateAccount,
    /// Guardian setup explanation
    GuardianSetup,
    /// Recovery setup explanation
    RecoverySetup,
    /// Completion
    Complete,
}

impl WelcomeScreen {
    /// Create a new welcome screen
    pub fn new() -> Self {
        Self {
            step: OnboardingStep::Welcome,
            has_account: false,
            account_name: None,
            needs_redraw: true,
        }
    }

    /// Create a welcome screen for an existing account
    pub fn with_account(name: String) -> Self {
        Self {
            step: OnboardingStep::Complete,
            has_account: true,
            account_name: Some(name),
            needs_redraw: true,
        }
    }

    /// Set whether an account exists
    pub fn set_has_account(&mut self, has: bool, name: Option<String>) {
        self.has_account = has;
        self.account_name = name;
        if has {
            self.step = OnboardingStep::Complete;
        }
        self.needs_redraw = true;
    }

    /// Move to next onboarding step
    fn next_step(&mut self) {
        self.step = match self.step {
            OnboardingStep::Welcome => OnboardingStep::CreateAccount,
            OnboardingStep::CreateAccount => OnboardingStep::GuardianSetup,
            OnboardingStep::GuardianSetup => OnboardingStep::RecoverySetup,
            OnboardingStep::RecoverySetup => OnboardingStep::Complete,
            OnboardingStep::Complete => OnboardingStep::Complete,
        };
        self.needs_redraw = true;
    }

    /// Public advance method for external navigation (e.g., Enter on welcome)
    pub fn advance(&mut self) {
        self.next_step();
    }

    /// Move to previous onboarding step
    fn prev_step(&mut self) {
        self.step = match self.step {
            OnboardingStep::Welcome => OnboardingStep::Welcome,
            OnboardingStep::CreateAccount => OnboardingStep::Welcome,
            OnboardingStep::GuardianSetup => OnboardingStep::CreateAccount,
            OnboardingStep::RecoverySetup => OnboardingStep::GuardianSetup,
            OnboardingStep::Complete => OnboardingStep::RecoverySetup,
        };
        self.needs_redraw = true;
    }

    /// Render the welcome step
    fn render_welcome(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let logo = r#"
╔═════════════════════════╗
║                         ║
║     ▄▀█ █░█ █▀█ ▄▀█     ║
║     █▀█ █▄█ █▀▄ █▀█     ║
║                         ║
╚═════════════════════════╝
"#;

        let content = vec![
            Line::from(""),
            Line::from(Span::styled(
                "Welcome to Aura",
                styles.text().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Aura is a threshold identity and encrypted storage platform",
                styles.text_muted(),
            )),
            Line::from(Span::styled(
                "built on relational security principles.",
                styles.text_muted(),
            )),
            Line::from(""),
            Line::from(Span::styled("With Aura, you can:", styles.text_muted())),
            Line::from(""),
            Line::from(vec![
                Span::styled("  • ", styles.text_highlight()),
                Span::styled("Create threshold-protected accounts", styles.text_muted()),
            ]),
            Line::from(vec![
                Span::styled("  • ", styles.text_highlight()),
                Span::styled(
                    "Designate trusted guardians for recovery",
                    styles.text_muted(),
                ),
            ]),
            Line::from(vec![
                Span::styled("  • ", styles.text_highlight()),
                Span::styled(
                    "Securely communicate with end-to-end encryption",
                    styles.text_muted(),
                ),
            ]),
            Line::from(vec![
                Span::styled("  • ", styles.text_highlight()),
                Span::styled(
                    "Recover your account with guardian assistance",
                    styles.text_muted(),
                ),
            ]),
            Line::from(""),
            Line::from(""),
            Line::from(Span::styled(
                "Press Enter to get started",
                styles.text_highlight().add_modifier(Modifier::BOLD),
            )),
        ];

        // Use consistent grid system: logo + content layout
        let chunks = ScreenLayout::new()
            .fixed(14) // Logo (fixed height)
            .flexible(10) // Content (min 10 rows)
            .build(area);

        let logo_para = Paragraph::new(logo)
            .style(styles.text_highlight())
            .alignment(Alignment::Center);
        f.render_widget(logo_para, chunks[0]);

        let content_para = Paragraph::new(content)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });
        f.render_widget(content_para, chunks[1]);
    }

    /// Render the create account step
    fn render_create_account(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let content = vec![
            Line::from(Span::styled(
                "Create Your Account",
                styles.text().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Your Aura account uses threshold cryptography to protect your identity.",
                styles.text_muted(),
            )),
            Line::from(""),
            Line::from(Span::styled("How it works:", styles.text_muted())),
            Line::from(""),
            Line::from(vec![
                Span::styled("  1. ", styles.text_highlight()),
                Span::styled(
                    "A master key is generated for your account",
                    styles.text_muted(),
                ),
            ]),
            Line::from(vec![
                Span::styled("  2. ", styles.text_highlight()),
                Span::styled(
                    "The key is split into shares using threshold cryptography",
                    styles.text_muted(),
                ),
            ]),
            Line::from(vec![
                Span::styled("  3. ", styles.text_highlight()),
                Span::styled(
                    "Shares are distributed to you and your guardians",
                    styles.text_muted(),
                ),
            ]),
            Line::from(vec![
                Span::styled("  4. ", styles.text_highlight()),
                Span::styled(
                    "Any 2-of-3 (or configured threshold) can reconstruct the key",
                    styles.text_muted(),
                ),
            ]),
            Line::from(""),
            Line::from(""),
            Line::from(vec![
                Span::styled("[Enter] ", styles.text_highlight()),
                Span::styled("Continue", styles.text_muted()),
                Span::styled("    [Esc] ", styles.text_muted()),
                Span::styled("Back", styles.text_muted()),
            ]),
        ];

        // Use consistent panel styling from Styles
        let para = Paragraph::new(content)
            .block(styles.panel("Account Setup"))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });

        f.render_widget(para, area);
    }

    /// Render the guardian setup step
    fn render_guardian_setup(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let content = vec![
            Line::from(Span::styled(
                "Guardian Setup",
                styles.text().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Guardians are trusted contacts who help protect your account.",
                styles.text_muted(),
            )),
            Line::from(""),
            Line::from(Span::styled("Choose guardians who:", styles.text_muted())),
            Line::from(""),
            Line::from(vec![
                Span::styled("  • ", styles.text_highlight()),
                Span::styled(
                    "You trust to help recover your account",
                    styles.text_muted(),
                ),
            ]),
            Line::from(vec![
                Span::styled("  • ", styles.text_highlight()),
                Span::styled("Are reliable and reachable", styles.text_muted()),
            ]),
            Line::from(vec![
                Span::styled("  • ", styles.text_highlight()),
                Span::styled("Won't collude against you", styles.text_muted()),
            ]),
            Line::from(vec![
                Span::styled("  • ", styles.text_highlight()),
                Span::styled(
                    "Have different risk profiles (e.g., family + friend)",
                    styles.text_muted(),
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Recommended: Start with a 2-of-3 threshold setup.",
                styles.text_warning(),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("[Enter] ", styles.text_highlight()),
                Span::styled("Continue", styles.text_muted()),
                Span::styled("    [Esc] ", styles.text_muted()),
                Span::styled("Back", styles.text_muted()),
            ]),
        ];

        // Use consistent panel styling from Styles
        let para = Paragraph::new(content)
            .block(styles.panel("Guardian Setup"))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });

        f.render_widget(para, area);
    }

    /// Render the recovery setup step
    fn render_recovery_setup(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let content = vec![
            Line::from(Span::styled(
                "Recovery Process",
                styles.text().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "If you lose access to your account, guardians can help you recover.",
                styles.text_muted(),
            )),
            Line::from(""),
            Line::from(Span::styled("The recovery process:", styles.text_muted())),
            Line::from(""),
            Line::from(vec![
                Span::styled("  1. ", styles.text_highlight()),
                Span::styled("Initiate recovery from any device", styles.text_muted()),
            ]),
            Line::from(vec![
                Span::styled("  2. ", styles.text_highlight()),
                Span::styled(
                    "Contact your guardians to approve the request",
                    styles.text_muted(),
                ),
            ]),
            Line::from(vec![
                Span::styled("  3. ", styles.text_highlight()),
                Span::styled(
                    "Once threshold is met, your key is reconstructed",
                    styles.text_muted(),
                ),
            ]),
            Line::from(vec![
                Span::styled("  4. ", styles.text_highlight()),
                Span::styled(
                    "Full access is restored to your account",
                    styles.text_muted(),
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Your guardians can never access your account without your consent.",
                styles.text_success(),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("[Enter] ", styles.text_highlight()),
                Span::styled("Continue", styles.text_muted()),
                Span::styled("    [Esc] ", styles.text_muted()),
                Span::styled("Back", styles.text_muted()),
            ]),
        ];

        // Use consistent panel styling from Styles
        let para = Paragraph::new(content)
            .block(styles.panel("Recovery Setup"))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });

        f.render_widget(para, area);
    }

    /// Render the complete step
    fn render_complete(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let account_info = self
            .account_name
            .as_ref()
            .map(|n| format!("Account: {}", n))
            .unwrap_or_else(|| "No account configured".to_string());

        let content = if self.has_account {
            vec![
                Line::from(Span::styled(
                    "Welcome Back!",
                    styles.text().add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(&account_info, styles.text_highlight())),
                Line::from(""),
                Line::from(""),
                Line::from(Span::styled("Quick Navigation:", styles.text_muted())),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  [c] ", styles.text_highlight()),
                    Span::styled("Chat", styles.text_muted()),
                ]),
                Line::from(vec![
                    Span::styled("  [g] ", styles.text_highlight()),
                    Span::styled("Guardians", styles.text_muted()),
                ]),
                Line::from(vec![
                    Span::styled("  [r] ", styles.text_highlight()),
                    Span::styled("Recovery", styles.text_muted()),
                ]),
                Line::from(vec![
                    Span::styled("  [i] ", styles.text_highlight()),
                    Span::styled("Invitations", styles.text_muted()),
                ]),
                Line::from(vec![
                    Span::styled("  [s] ", styles.text_highlight()),
                    Span::styled("Settings", styles.text_muted()),
                ]),
                #[cfg(feature = "development")]
                Line::from(vec![
                    Span::styled("  [d] ", styles.text_warning()),
                    Span::styled("Demo", styles.text_muted()),
                ]),
                Line::from(vec![
                    Span::styled("  [?] ", styles.text_highlight()),
                    Span::styled("Help", styles.text_muted()),
                ]),
            ]
        } else {
            vec![
                Line::from(Span::styled(
                    "Ready to Get Started!",
                    styles.text().add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "You're all set to create your Aura account.",
                    styles.text_muted(),
                )),
                Line::from(""),
                Line::from(""),
                Line::from(vec![
                    Span::styled("[Enter] ", styles.text_success()),
                    Span::styled("Create Account", styles.text()),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("[Esc] ", styles.text_muted()),
                    Span::styled("Go Back", styles.text_muted()),
                ]),
            ]
        };

        // Use consistent panel styling from Styles
        let para = Paragraph::new(content)
            .block(styles.panel("Aura"))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });

        f.render_widget(para, area);
    }
}

impl Default for WelcomeScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen for WelcomeScreen {
    fn screen_type(&self) -> ScreenType {
        ScreenType::Welcome
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<InputAction> {
        match key.code {
            KeyCode::Enter => {
                if self.step == OnboardingStep::Complete && !self.has_account {
                    return Some(InputAction::Submit("action:create_account".to_string()));
                }
                self.next_step();
                None
            }
            KeyCode::Esc => {
                if self.step != OnboardingStep::Welcome {
                    self.prev_step();
                }
                None
            }
            // Navigation shortcuts when onboarding is complete
            KeyCode::Char('c') if self.has_account => {
                Some(InputAction::Submit("navigate:chat".to_string()))
            }
            KeyCode::Char('g') if self.has_account => {
                Some(InputAction::Submit("navigate:guardians".to_string()))
            }
            KeyCode::Char('r') if self.has_account => {
                Some(InputAction::Submit("navigate:recovery".to_string()))
            }
            KeyCode::Char('i') if self.has_account => {
                Some(InputAction::Submit("navigate:invitations".to_string()))
            }
            KeyCode::Char('s') if self.has_account => {
                Some(InputAction::Submit("navigate:settings".to_string()))
            }
            #[cfg(feature = "development")]
            KeyCode::Char('d') if self.has_account => {
                Some(InputAction::Submit("navigate:demo".to_string()))
            }
            _ => None,
        }
    }

    fn render(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        // Center the content using consistent layout system
        // Horizontal centering: 15% margin on each side, 70% content
        let centered = LayoutPresets::two_columns(area, 15);
        let center_area = LayoutPresets::two_columns(centered[1], 82)[0]; // 70/(70+15) ≈ 82%

        // Vertical centering: 10% padding top/bottom
        let vertical = ScreenLayout::new()
            .percentage(10)
            .percentage(80)
            .percentage(10)
            .build(center_area);

        match self.step {
            OnboardingStep::Welcome => self.render_welcome(f, vertical[1], styles),
            OnboardingStep::CreateAccount => self.render_create_account(f, vertical[1], styles),
            OnboardingStep::GuardianSetup => self.render_guardian_setup(f, vertical[1], styles),
            OnboardingStep::RecoverySetup => self.render_recovery_setup(f, vertical[1], styles),
            OnboardingStep::Complete => self.render_complete(f, vertical[1], styles),
        }
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
    fn test_welcome_screen_new() {
        let screen = WelcomeScreen::new();
        assert_eq!(screen.screen_type(), ScreenType::Welcome);
        assert_eq!(screen.step, OnboardingStep::Welcome);
        assert!(!screen.has_account);
    }

    #[test]
    fn test_with_account() {
        let screen = WelcomeScreen::with_account("Alice".to_string());
        assert_eq!(screen.step, OnboardingStep::Complete);
        assert!(screen.has_account);
        assert_eq!(screen.account_name, Some("Alice".to_string()));
    }

    #[test]
    fn test_step_navigation() {
        let mut screen = WelcomeScreen::new();

        assert_eq!(screen.step, OnboardingStep::Welcome);
        screen.next_step();
        assert_eq!(screen.step, OnboardingStep::CreateAccount);
        screen.next_step();
        assert_eq!(screen.step, OnboardingStep::GuardianSetup);
        screen.next_step();
        assert_eq!(screen.step, OnboardingStep::RecoverySetup);
        screen.next_step();
        assert_eq!(screen.step, OnboardingStep::Complete);

        // Can't go past complete
        screen.next_step();
        assert_eq!(screen.step, OnboardingStep::Complete);

        // Navigate back
        screen.prev_step();
        assert_eq!(screen.step, OnboardingStep::RecoverySetup);
        screen.prev_step();
        assert_eq!(screen.step, OnboardingStep::GuardianSetup);
    }

    #[test]
    fn test_set_has_account() {
        let mut screen = WelcomeScreen::new();
        assert_eq!(screen.step, OnboardingStep::Welcome);

        screen.set_has_account(true, Some("Bob".to_string()));
        assert!(screen.has_account);
        assert_eq!(screen.step, OnboardingStep::Complete);
        assert_eq!(screen.account_name, Some("Bob".to_string()));
    }
}
