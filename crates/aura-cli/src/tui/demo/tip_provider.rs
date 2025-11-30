//! # Tip Provider
//!
//! Provides contextual tips and hints during demo mode.
//!
//! The tip system shows helpful hints to users during the demo,
//! guiding them through the features being demonstrated.

use crate::tui::screens::ScreenType;

/// Context information for determining which tip to show
#[derive(Debug, Clone)]
pub struct TipContext {
    /// Current screen
    pub screen: ScreenType,
    /// Whether recovery is active
    pub recovery_active: bool,
    /// Number of guardian approvals received
    pub approvals_received: u8,
    /// Required threshold for recovery
    pub threshold: u8,
    /// Whether user has sent a message
    pub has_sent_message: bool,
}

impl TipContext {
    /// Create a new tip context
    pub fn new(screen: ScreenType) -> Self {
        Self {
            screen,
            recovery_active: false,
            approvals_received: 0,
            threshold: 2,
            has_sent_message: false,
        }
    }

    /// Update recovery state
    pub fn with_recovery(mut self, active: bool, approvals: u8, threshold: u8) -> Self {
        self.recovery_active = active;
        self.approvals_received = approvals;
        self.threshold = threshold;
        self
    }
}

/// A tip to display to the user
#[derive(Debug, Clone)]
pub struct Tip {
    /// The tip message
    pub message: String,
    /// Optional action hint (e.g., "Press Enter to continue")
    pub action_hint: Option<String>,
    /// Priority (higher = more important)
    pub priority: u8,
}

impl Tip {
    /// Create a new tip
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            action_hint: None,
            priority: 0,
        }
    }

    /// Add an action hint
    pub fn with_action(mut self, hint: impl Into<String>) -> Self {
        self.action_hint = Some(hint.into());
        self
    }

    /// Set priority
    pub fn with_priority(mut self, priority: u8) -> Self {
        self.priority = priority;
        self
    }
}

/// Trait for providing contextual tips
pub trait TipProvider: Send + Sync {
    /// Get the current tip based on context
    fn current_tip(&self, context: &TipContext) -> Option<Tip>;

    /// Mark a tip as seen/dismissed
    fn dismiss_tip(&mut self, tip_id: &str);

    /// Check if tips are enabled
    fn tips_enabled(&self) -> bool;

    /// Enable or disable tips
    fn set_tips_enabled(&mut self, enabled: bool);
}

/// Demo tip provider implementation
pub struct DemoTipProvider {
    /// Whether tips are enabled
    enabled: bool,
    /// Set of dismissed tip IDs
    dismissed: std::collections::HashSet<String>,
}

impl DemoTipProvider {
    /// Create a new demo tip provider
    pub fn new() -> Self {
        Self {
            enabled: true,
            dismissed: std::collections::HashSet::new(),
        }
    }
}

impl Default for DemoTipProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl TipProvider for DemoTipProvider {
    fn current_tip(&self, context: &TipContext) -> Option<Tip> {
        if !self.enabled {
            return None;
        }

        // Generate tip based on context
        match context.screen {
            ScreenType::Chat => {
                if !context.has_sent_message && !self.dismissed.contains("chat_intro") {
                    Some(
                        Tip::new("Welcome to the chat! Type a message and press Enter to send.")
                            .with_action("Type /help for commands")
                            .with_priority(10),
                    )
                } else {
                    None
                }
            }
            ScreenType::Recovery => {
                if context.recovery_active {
                    if context.approvals_received == 0
                        && !self.dismissed.contains("recovery_waiting")
                    {
                        Some(
                            Tip::new("Recovery initiated! Waiting for guardian approvals...")
                                .with_action("Guardians will be notified automatically")
                                .with_priority(20),
                        )
                    } else if context.approvals_received > 0
                        && context.approvals_received < context.threshold
                    {
                        Some(
                            Tip::new(format!(
                                "Progress: {}/{} guardian approvals received",
                                context.approvals_received, context.threshold
                            ))
                            .with_priority(15),
                        )
                    } else if context.approvals_received >= context.threshold {
                        Some(
                            Tip::new("Threshold met! Recovery can now complete.")
                                .with_action("Press Enter to complete recovery")
                                .with_priority(25),
                        )
                    } else {
                        None
                    }
                } else if !self.dismissed.contains("recovery_intro") {
                    Some(
                        Tip::new(
                            "This is the recovery screen. You can initiate account recovery here.",
                        )
                        .with_action("Press 's' to start recovery")
                        .with_priority(10),
                    )
                } else {
                    None
                }
            }
            ScreenType::Guardians => {
                if !self.dismissed.contains("guardians_intro") {
                    Some(
                        Tip::new(
                            "Your guardians help protect your account through social recovery.",
                        )
                        .with_action("Press 'i' to invite a new guardian")
                        .with_priority(10),
                    )
                } else {
                    None
                }
            }
            ScreenType::Invitations => {
                if !self.dismissed.contains("invitations_intro") {
                    Some(
                        Tip::new("Manage your pending invitations here.")
                            .with_action("Press Enter to accept, Backspace to decline")
                            .with_priority(10),
                    )
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn dismiss_tip(&mut self, tip_id: &str) {
        self.dismissed.insert(tip_id.to_string());
    }

    fn tips_enabled(&self) -> bool {
        self.enabled
    }

    fn set_tips_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tip_creation() {
        let tip = Tip::new("Test tip")
            .with_action("Press Enter")
            .with_priority(5);

        assert_eq!(tip.message, "Test tip");
        assert_eq!(tip.action_hint, Some("Press Enter".to_string()));
        assert_eq!(tip.priority, 5);
    }

    #[test]
    fn test_demo_tip_provider() {
        let mut provider = DemoTipProvider::new();

        let context = TipContext::new(ScreenType::Chat);
        let tip = provider.current_tip(&context);
        assert!(tip.is_some());

        // Disable tips
        provider.set_tips_enabled(false);
        let tip = provider.current_tip(&context);
        assert!(tip.is_none());
    }

    #[test]
    fn test_dismiss_tip() {
        let mut provider = DemoTipProvider::new();

        provider.dismiss_tip("chat_intro");

        let context = TipContext::new(ScreenType::Chat);
        let tip = provider.current_tip(&context);
        assert!(tip.is_none());
    }
}
