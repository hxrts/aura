//! # Command Dispatcher
//!
//! Maps IRC-style commands to effect system commands with capability checking.
//!
//! ## Capability Checking
//!
//! The dispatcher supports configurable capability checking via `CapabilityPolicy`:
//!
//! - `AllowAll` - Permits all commands (for development/testing)
//! - `DenyNonPublic` - Denies commands requiring capabilities (restrictive default)
//! - `Custom(fn)` - Delegate to a custom capability checker
//!
//! For Biscuit integration, create a custom checker that calls RuntimeBridge:
//!
//! ```ignore
//! let checker = |cap: &CommandCapability| -> bool {
//!     if *cap == CommandCapability::None { return true; }
//!     // Check via RuntimeBridge.has_command_capability(cap.as_str())
//!     runtime.has_command_capability(cap.as_str())
//! };
//! let dispatcher = CommandDispatcher::with_policy(CapabilityPolicy::Custom(Box::new(checker)));
//! ```

use crate::tui::commands::{CommandCapability, IrcCommand};

use super::command_parser::EffectCommand;

/// Policy for checking command capabilities
pub enum CapabilityPolicy {
    /// Allow all commands regardless of capability requirements
    AllowAll,
    /// Deny all commands that require non-None capabilities
    DenyNonPublic,
    /// Use a custom capability checker function
    Custom(Box<dyn Fn(&CommandCapability) -> bool + Send + Sync>),
}

impl Default for CapabilityPolicy {
    fn default() -> Self {
        // Production-safe default: deny commands with non-None capability requirement
        Self::DenyNonPublic
    }
}

impl std::fmt::Debug for CapabilityPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AllowAll => write!(f, "AllowAll"),
            Self::DenyNonPublic => write!(f, "DenyNonPublic"),
            Self::Custom(_) => write!(f, "Custom(...)"),
        }
    }
}

/// Error that can occur during command dispatch
#[derive(Debug, Clone, PartialEq)]
pub enum DispatchError {
    /// User lacks required capability
    PermissionDenied {
        /// Required capability
        required: CommandCapability,
    },
    /// Target user/channel not found
    NotFound {
        /// What was not found
        resource: String,
    },
    /// Invalid command parameter
    InvalidParameter {
        /// Parameter name
        param: String,
        /// Error message
        reason: String,
    },
    /// Command not yet implemented
    NotImplemented {
        /// Command name
        command: String,
    },
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PermissionDenied { required } => {
                write!(
                    f,
                    "Permission denied: requires '{}' capability",
                    required.as_biscuit_capability()
                )
            }
            Self::NotFound { resource } => write!(f, "Not found: {resource}"),
            Self::InvalidParameter { param, reason } => {
                write!(f, "Invalid parameter '{param}': {reason}")
            }
            Self::NotImplemented { command } => {
                write!(f, "Command '{command}' not yet implemented")
            }
        }
    }
}

impl std::error::Error for DispatchError {}

/// Stub Biscuit capability checker for integration with RuntimeBridge.
///
/// This function provides a template for integrating Biscuit token verification
/// into the command dispatcher. In production, this should delegate to
/// RuntimeBridge.has_command_capability().
///
/// # Example Integration
/// ```ignore
/// use crate::tui::effects::dispatcher::{CommandDispatcher, biscuit_capability_stub};
///
/// let runtime_bridge = Arc::new(RwLock::new(runtime_bridge));
/// let checker = move |cap: &CommandCapability| -> bool {
///     if *cap == CommandCapability::None { return true; }
///
///     // Try RuntimeBridge capability check
///     if let Some(bridge) = runtime_bridge.try_read() {
///         bridge.has_command_capability(cap.as_str())
///     } else {
///         // Fallback: use stub or deny
///         biscuit_capability_stub(cap)
///     }
/// };
///
/// let dispatcher = CommandDispatcher::with_biscuit_policy(Box::new(checker));
/// ```
pub fn biscuit_capability_stub(cap: &CommandCapability) -> bool {
    // Stub implementation: Allow a baseline set of capabilities
    // In production, this should check Biscuit tokens via RuntimeBridge
    match cap {
        CommandCapability::None => true,
        CommandCapability::SendDm | CommandCapability::ViewMembers => {
            // Basic messaging capabilities - allowed in stub
            tracing::debug!("Biscuit stub: allowing basic capability {:?}", cap);
            true
        }
        _ => {
            // Advanced capabilities require Biscuit token verification
            tracing::warn!(
                "Biscuit stub: denying advanced capability {:?} (token verification not implemented)",
                cap
            );
            false
        }
    }
}

/// Command dispatcher that maps IRC commands to effect commands
///
/// The dispatcher converts IRC-style commands to effect commands with
/// configurable capability checking.
pub struct CommandDispatcher {
    /// Current channel context
    current_channel: Option<String>,
    /// Capability checking policy
    capability_policy: CapabilityPolicy,
}

impl CommandDispatcher {
    /// Create a new command dispatcher with default policy (DenyNonPublic)
    #[must_use]
    pub fn new() -> Self {
        Self {
            current_channel: None,
            capability_policy: CapabilityPolicy::default(),
        }
    }

    /// Create a dispatcher with a specific capability policy
    #[must_use]
    pub fn with_policy(policy: CapabilityPolicy) -> Self {
        Self {
            current_channel: None,
            capability_policy: policy,
        }
    }

    /// Create a dispatcher with Biscuit-based capability checking.
    ///
    /// This is a convenience constructor for integrating with Biscuit token
    /// verification via RuntimeBridge. Pass a closure that checks capabilities
    /// against Biscuit tokens.
    ///
    /// # Example
    /// ```ignore
    /// let dispatcher = CommandDispatcher::with_biscuit_policy(Box::new(|cap| {
    ///     runtime_bridge.has_command_capability(cap.as_str())
    /// }));
    /// ```
    #[must_use]
    pub fn with_biscuit_policy(
        checker: Box<dyn Fn(&CommandCapability) -> bool + Send + Sync>,
    ) -> Self {
        Self::with_policy(CapabilityPolicy::Custom(checker))
    }

    /// Create a dispatcher with the stub Biscuit checker.
    ///
    /// This uses the built-in `biscuit_capability_stub()` which allows basic
    /// capabilities but denies advanced ones. Useful for development/testing.
    pub fn with_stub_biscuit() -> Self {
        Self::with_biscuit_policy(Box::new(biscuit_capability_stub))
    }

    /// Set the capability policy
    pub fn set_policy(&mut self, policy: CapabilityPolicy) {
        self.capability_policy = policy;
    }

    /// Set the current channel context
    pub fn set_current_channel(&mut self, channel: impl Into<String>) {
        self.current_channel = Some(channel.into());
    }

    /// Clear the current channel context
    pub fn clear_current_channel(&mut self) {
        self.current_channel = None;
    }

    /// Get the current channel context
    #[must_use]
    pub fn current_channel(&self) -> Option<&str> {
        self.current_channel.as_deref()
    }

    /// Check if a command is allowed based on the configured capability policy.
    ///
    /// The check delegates to the configured `CapabilityPolicy`:
    /// - `AllowAll`: Always permits (used when IoContext handles authorization)
    /// - `DenyNonPublic`: Denies commands requiring capabilities
    /// - `Custom(fn)`: Delegates to custom checker (for Biscuit integration)
    pub fn check_capability(&self, command: &IrcCommand) -> Result<(), DispatchError> {
        let capability = command.required_capability();

        // Commands with no capability requirement always pass
        if capability == CommandCapability::None {
            return Ok(());
        }

        // Check based on policy
        let allowed = match &self.capability_policy {
            CapabilityPolicy::AllowAll => true,
            CapabilityPolicy::DenyNonPublic => false,
            CapabilityPolicy::Custom(checker) => checker(&capability),
        };

        if allowed {
            Ok(())
        } else {
            Err(DispatchError::PermissionDenied {
                required: capability,
            })
        }
    }

    /// Dispatch an IRC command to an effect command
    pub fn dispatch(&self, command: IrcCommand) -> Result<EffectCommand, DispatchError> {
        // First check capability
        self.check_capability(&command)?;

        // Then map to effect command
        self.map_command(command)
    }

    /// Map command to effect without capability checking (for testing)
    #[cfg(test)]
    pub fn dispatch_unchecked(&self, command: IrcCommand) -> Result<EffectCommand, DispatchError> {
        self.map_command(command)
    }

    /// Internal mapping from command to effect
    fn map_command(&self, command: IrcCommand) -> Result<EffectCommand, DispatchError> {
        match command {
            IrcCommand::Msg { target, text } => Ok(EffectCommand::SendDirectMessage {
                target,
                content: text,
            }),

            IrcCommand::Me { action } => {
                let channel =
                    self.current_channel
                        .clone()
                        .ok_or_else(|| DispatchError::NotFound {
                            resource: "current channel".to_string(),
                        })?;

                Ok(EffectCommand::SendAction { channel, action })
            }

            IrcCommand::Nick { name } => Ok(EffectCommand::UpdateNickname { name }),

            IrcCommand::Who => {
                let channel =
                    self.current_channel
                        .clone()
                        .ok_or_else(|| DispatchError::NotFound {
                            resource: "current channel".to_string(),
                        })?;

                Ok(EffectCommand::ListParticipants { channel })
            }

            IrcCommand::Whois { target } => Ok(EffectCommand::GetUserInfo { target }),

            IrcCommand::Leave => {
                let channel =
                    self.current_channel
                        .clone()
                        .ok_or_else(|| DispatchError::NotFound {
                            resource: "current channel".to_string(),
                        })?;

                Ok(EffectCommand::LeaveChannel { channel })
            }

            IrcCommand::Help { .. } => {
                // Help is handled locally, not via effect system
                Err(DispatchError::NotImplemented {
                    command: "help".to_string(),
                })
            }

            IrcCommand::Join { channel } => Ok(EffectCommand::JoinChannel { channel }),

            IrcCommand::Kick { target, reason } => {
                let channel =
                    self.current_channel
                        .clone()
                        .ok_or_else(|| DispatchError::NotFound {
                            resource: "current channel".to_string(),
                        })?;

                Ok(EffectCommand::KickUser {
                    channel,
                    target,
                    reason,
                })
            }

            IrcCommand::Ban { target, reason } => Ok(EffectCommand::BanUser { target, reason }),

            IrcCommand::Unban { target } => Ok(EffectCommand::UnbanUser { target }),

            IrcCommand::Mute { target, duration } => Ok(EffectCommand::MuteUser {
                target,
                duration_secs: duration.map(|d| d.as_secs()),
            }),

            IrcCommand::Unmute { target } => Ok(EffectCommand::UnmuteUser { target }),

            IrcCommand::Invite { target } => Ok(EffectCommand::InviteUser { target }),

            IrcCommand::Topic { text } => {
                let channel =
                    self.current_channel
                        .clone()
                        .ok_or_else(|| DispatchError::NotFound {
                            resource: "current channel".to_string(),
                        })?;

                Ok(EffectCommand::SetTopic { channel, text })
            }

            IrcCommand::Pin { message_id } => Ok(EffectCommand::PinMessage { message_id }),

            IrcCommand::Unpin { message_id } => Ok(EffectCommand::UnpinMessage { message_id }),

            IrcCommand::Op { target } => Ok(EffectCommand::GrantSteward { target }),

            IrcCommand::Deop { target } => Ok(EffectCommand::RevokeSteward { target }),

            IrcCommand::Mode { channel, flags } => {
                Ok(EffectCommand::SetChannelMode { channel, flags })
            }
        }
    }
}

impl Default for CommandDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_dispatch_msg() {
        let dispatcher = CommandDispatcher::new();
        let cmd = IrcCommand::Msg {
            target: "alice".to_string(),
            text: "hello".to_string(),
        };

        // Use dispatch_unchecked to test mapping without capability check
        let result = dispatcher.dispatch_unchecked(cmd);
        assert!(result.is_ok());
        match result.unwrap() {
            EffectCommand::SendDirectMessage { target, content } => {
                assert_eq!(target, "alice");
                assert_eq!(content, "hello");
            }
            _ => panic!("Wrong effect command type"),
        }
    }

    #[test]
    fn test_dispatch_nick() {
        let dispatcher = CommandDispatcher::new();
        let cmd = IrcCommand::Nick {
            name: "NewName".to_string(),
        };

        // Use dispatch_unchecked to test mapping without capability check
        let result = dispatcher.dispatch_unchecked(cmd);
        assert!(result.is_ok());
        match result.unwrap() {
            EffectCommand::UpdateNickname { name } => {
                assert_eq!(name, "NewName");
            }
            _ => panic!("Wrong effect command type"),
        }
    }

    #[test]
    fn test_dispatch_me_without_channel() {
        let dispatcher = CommandDispatcher::new();
        let cmd = IrcCommand::Me {
            action: "waves".to_string(),
        };

        // Use dispatch_unchecked to test mapping error (missing channel)
        let result = dispatcher.dispatch_unchecked(cmd);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            DispatchError::NotFound { .. }
        ));
    }

    #[test]
    fn test_dispatch_me_with_channel() {
        let mut dispatcher = CommandDispatcher::new();
        dispatcher.set_current_channel("general");

        let cmd = IrcCommand::Me {
            action: "waves".to_string(),
        };

        // Use dispatch_unchecked to test mapping without capability check
        let result = dispatcher.dispatch_unchecked(cmd);
        assert!(result.is_ok());
        match result.unwrap() {
            EffectCommand::SendAction { channel, action } => {
                assert_eq!(channel, "general");
                assert_eq!(action, "waves");
            }
            _ => panic!("Wrong effect command type"),
        }
    }

    #[test]
    fn test_dispatch_kick() {
        let mut dispatcher = CommandDispatcher::new();
        dispatcher.set_current_channel("general");

        let cmd = IrcCommand::Kick {
            target: "spammer".to_string(),
            reason: Some("flooding".to_string()),
        };

        // Use dispatch_unchecked to test mapping without capability check
        let result = dispatcher.dispatch_unchecked(cmd);
        assert!(result.is_ok());
        match result.unwrap() {
            EffectCommand::KickUser {
                channel,
                target,
                reason,
            } => {
                assert_eq!(channel, "general");
                assert_eq!(target, "spammer");
                assert_eq!(reason, Some("flooding".to_string()));
            }
            _ => panic!("Wrong effect command type"),
        }
    }

    #[test]
    fn test_dispatch_mute_with_duration() {
        let dispatcher = CommandDispatcher::new();
        let cmd = IrcCommand::Mute {
            target: "alice".to_string(),
            duration: Some(Duration::from_secs(300)),
        };

        // Use dispatch_unchecked to test mapping without capability check
        let result = dispatcher.dispatch_unchecked(cmd);
        assert!(result.is_ok());
        match result.unwrap() {
            EffectCommand::MuteUser {
                target,
                duration_secs,
            } => {
                assert_eq!(target, "alice");
                assert_eq!(duration_secs, Some(300));
            }
            _ => panic!("Wrong effect command type"),
        }
    }

    #[test]
    fn test_dispatcher_channel_context() {
        let mut dispatcher = CommandDispatcher::new();

        assert_eq!(dispatcher.current_channel(), None);

        dispatcher.set_current_channel("general");
        assert_eq!(dispatcher.current_channel(), Some("general"));

        dispatcher.clear_current_channel();
        assert_eq!(dispatcher.current_channel(), None);
    }

    // =========================================================================
    // Capability Policy Tests
    // =========================================================================

    #[test]
    fn test_default_policy_denies_non_public() {
        let dispatcher = CommandDispatcher::new();

        // Default policy (DenyNonPublic) should deny commands requiring capabilities
        let cmd = IrcCommand::Msg {
            target: "alice".to_string(),
            text: "hello".to_string(),
        };

        let result = dispatcher.dispatch(cmd);
        assert!(result.is_err());
    }

    #[test]
    fn test_deny_non_public_policy() {
        let dispatcher = CommandDispatcher::with_policy(CapabilityPolicy::DenyNonPublic);

        // Commands requiring capabilities should be denied
        let cmd = IrcCommand::Msg {
            target: "alice".to_string(),
            text: "hello".to_string(),
        };

        let result = dispatcher.dispatch(cmd);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            DispatchError::PermissionDenied { .. }
        ));
    }

    #[test]
    fn test_deny_non_public_allows_help() {
        let dispatcher = CommandDispatcher::with_policy(CapabilityPolicy::DenyNonPublic);

        // Help command has no capability requirement, should pass check
        // (but returns NotImplemented from dispatch, which is fine)
        let cmd = IrcCommand::Help { command: None };

        let result = dispatcher.check_capability(&cmd);
        assert!(result.is_ok());
    }

    #[test]
    fn test_custom_policy() {
        // Custom policy that allows only specific capabilities
        let checker = |cap: &CommandCapability| -> bool {
            matches!(
                cap,
                CommandCapability::SendDm | CommandCapability::ViewMembers
            )
        };

        let dispatcher =
            CommandDispatcher::with_policy(CapabilityPolicy::Custom(Box::new(checker)));

        // SendDm should be allowed
        let msg_cmd = IrcCommand::Msg {
            target: "alice".to_string(),
            text: "hello".to_string(),
        };
        assert!(dispatcher.dispatch(msg_cmd).is_ok());

        // ModerateKick should be denied
        let mut dispatcher_with_channel = CommandDispatcher::with_policy(CapabilityPolicy::Custom(
            Box::new(|cap: &CommandCapability| {
                matches!(
                    cap,
                    CommandCapability::SendDm | CommandCapability::ViewMembers
                )
            }),
        ));
        dispatcher_with_channel.set_current_channel("general");

        let kick_cmd = IrcCommand::Kick {
            target: "spammer".to_string(),
            reason: None,
        };
        let result = dispatcher_with_channel.dispatch(kick_cmd);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            DispatchError::PermissionDenied { .. }
        ));
    }

    #[test]
    fn test_set_policy() {
        let mut dispatcher = CommandDispatcher::new();

        // Start with DenyNonPublic
        let cmd = IrcCommand::Msg {
            target: "alice".to_string(),
            text: "hello".to_string(),
        };
        assert!(dispatcher.dispatch(cmd.clone()).is_err());

        // Change to AllowAll
        dispatcher.set_policy(CapabilityPolicy::AllowAll);
        assert!(dispatcher.dispatch(cmd).is_ok());
    }
}
