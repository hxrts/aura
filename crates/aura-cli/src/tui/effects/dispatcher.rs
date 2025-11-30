//! # Command Dispatcher
//!
//! Maps IRC-style commands to effect system commands with capability checking.

use crate::tui::commands::{CommandCapability, IrcCommand};

use super::bridge::EffectCommand;

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
                    required.as_str()
                )
            }
            Self::NotFound { resource } => write!(f, "Not found: {}", resource),
            Self::InvalidParameter { param, reason } => {
                write!(f, "Invalid parameter '{}': {}", param, reason)
            }
            Self::NotImplemented { command } => {
                write!(f, "Command '{}' not yet implemented", command)
            }
        }
    }
}

impl std::error::Error for DispatchError {}

/// Command dispatcher that maps IRC commands to effect commands
pub struct CommandDispatcher {
    /// Current channel context
    current_channel: Option<String>,
}

impl CommandDispatcher {
    /// Create a new command dispatcher
    pub fn new() -> Self {
        Self {
            current_channel: None,
        }
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
    pub fn current_channel(&self) -> Option<&str> {
        self.current_channel.as_deref()
    }

    /// Check if a command is allowed (capability check - stub for now)
    pub fn check_capability(&self, command: &IrcCommand) -> Result<(), DispatchError> {
        // Stub: in production this should evaluate Biscuit capabilities
        let _capability = command.required_capability();

        // In production, this would:
        // 1. Get current user's Biscuit token
        // 2. Build an authorizer with the required capability
        // 3. Evaluate and return Ok/Err based on result

        Ok(())
    }

    /// Dispatch an IRC command to an effect command
    pub fn dispatch(&self, command: IrcCommand) -> Result<EffectCommand, DispatchError> {
        // First check capability
        self.check_capability(&command)?;

        // Then map to effect command
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

        let result = dispatcher.dispatch(cmd);
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

        let result = dispatcher.dispatch(cmd);
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

        let result = dispatcher.dispatch(cmd);
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

        let result = dispatcher.dispatch(cmd);
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

        let result = dispatcher.dispatch(cmd);
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

        let result = dispatcher.dispatch(cmd);
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
}
