//! # Command Dispatcher
//!
//! Maps IRC-style commands to effect system commands.
//!
//! Authorization belongs to the runtime-backed dispatch path. This dispatcher is
//! a pure mapping helper for chat commands plus current-channel context.

use crate::tui::commands::ChatCommand;

use super::command_parser::EffectCommand;

/// Error that can occur during command dispatch
#[derive(Debug, Clone, PartialEq)]
pub enum DispatchError {
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
    /// Command is handled locally by the UI instead of the effect layer
    HandledLocally {
        /// Command name
        command: String,
    },
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound { resource } => write!(f, "Not found: {resource}"),
            Self::InvalidParameter { param, reason } => {
                write!(f, "Invalid parameter '{param}': {reason}")
            }
            Self::HandledLocally { command } => {
                write!(f, "Command '{command}' is handled locally")
            }
        }
    }
}

impl std::error::Error for DispatchError {}

/// Committed channel selection with a monotone generation counter for
/// staleness detection.
#[derive(Debug, Clone)]
struct CommittedChannelSelection {
    channel_id: String,
    #[allow(dead_code)] // retained for future staleness checks
    generation: u64,
}

/// Command dispatcher that maps IRC commands to effect commands
///
/// The dispatcher converts IRC-style commands to effect commands with
/// configurable capability checking.
pub struct CommandDispatcher {
    /// Committed channel selection (not a raw mutable string)
    current_channel: Option<CommittedChannelSelection>,
    /// Monotone generation counter for staleness detection
    generation: u64,
}

impl CommandDispatcher {
    /// Create a new command dispatcher.
    #[must_use]
    pub fn new() -> Self {
        Self {
            current_channel: None,
            generation: 0,
        }
    }

    /// Commit a channel selection. The generation counter increments on each
    /// commit, providing a weak staleness boundary.
    pub fn set_current_channel(&mut self, channel: impl Into<String>) {
        self.generation += 1;
        self.current_channel = Some(CommittedChannelSelection {
            channel_id: channel.into(),
            generation: self.generation,
        });
    }

    /// Clear the current channel context
    pub fn clear_current_channel(&mut self) {
        self.current_channel = None;
    }

    /// Get the current channel context
    #[must_use]
    pub fn current_channel(&self) -> Option<&str> {
        self.current_channel.as_ref().map(|s| s.channel_id.as_str())
    }

    /// Dispatch an IRC command to an effect command
    pub fn dispatch(&self, command: ChatCommand) -> Result<EffectCommand, DispatchError> {
        self.map_command(command)
    }

    /// Map command to effect without capability checking (for testing)
    #[cfg(test)]
    pub fn dispatch_unchecked(&self, command: ChatCommand) -> Result<EffectCommand, DispatchError> {
        self.map_command(command)
    }

    /// Require a committed channel selection, returning the channel ID string.
    fn require_current_channel(&self) -> Result<String, DispatchError> {
        self.current_channel
            .as_ref()
            .map(|s| s.channel_id.clone())
            .ok_or_else(|| DispatchError::NotFound {
                resource: "current channel".to_string(),
            })
    }

    /// Get the current channel ID as an optional string (for commands that
    /// accept an optional channel hint).
    fn current_channel_id(&self) -> Option<String> {
        self.current_channel.as_ref().map(|s| s.channel_id.clone())
    }

    /// Internal mapping from command to effect
    fn map_command(&self, command: ChatCommand) -> Result<EffectCommand, DispatchError> {
        match command {
            ChatCommand::Msg { target, text } => Ok(EffectCommand::SendDirectMessage {
                target,
                content: text,
            }),

            ChatCommand::Me { action } => {
                let channel = self.require_current_channel()?;

                Ok(EffectCommand::SendAction { channel, action })
            }

            ChatCommand::Nick { name } => Ok(EffectCommand::UpdateNickname { name }),

            ChatCommand::Who => {
                let channel = self.require_current_channel()?;

                Ok(EffectCommand::ListParticipants { channel })
            }

            ChatCommand::Whois { target } => Ok(EffectCommand::GetUserInfo { target }),

            ChatCommand::Leave => {
                let channel = self.require_current_channel()?;

                Ok(EffectCommand::LeaveChannel { channel })
            }

            ChatCommand::Help { .. } => {
                // Help is handled locally, not via effect system
                Err(DispatchError::HandledLocally {
                    command: "help".to_string(),
                })
            }

            ChatCommand::Neighborhood { name } => Ok(EffectCommand::CreateNeighborhood { name }),

            ChatCommand::NhAdd { home_id } => Ok(EffectCommand::AddHomeToNeighborhood { home_id }),

            ChatCommand::NhLink { home_id } => Ok(EffectCommand::LinkHomeOneHopLink { home_id }),

            ChatCommand::HomeInvite { target } => {
                Ok(EffectCommand::SendHomeInvitation { contact_id: target })
            }

            ChatCommand::HomeAccept => Ok(EffectCommand::AcceptPendingHomeInvitation),

            ChatCommand::Join { channel } => Ok(EffectCommand::JoinChannel { channel }),

            ChatCommand::Kick { target, reason } => {
                let channel = self.require_current_channel()?;

                Ok(EffectCommand::KickUser {
                    channel,
                    target,
                    reason,
                })
            }

            ChatCommand::Ban { target, reason } => Ok(EffectCommand::BanUser {
                channel: self.current_channel_id(),
                target,
                reason,
            }),

            ChatCommand::Unban { target } => Ok(EffectCommand::UnbanUser {
                channel: self.current_channel_id(),
                target,
            }),

            ChatCommand::Mute { target, duration } => Ok(EffectCommand::MuteUser {
                channel: self.current_channel_id(),
                target,
                duration_secs: duration.map(|d| d.as_secs()),
            }),

            ChatCommand::Unmute { target } => Ok(EffectCommand::UnmuteUser {
                channel: self.current_channel_id(),
                target,
            }),

            ChatCommand::Invite { target } => {
                let channel = self.require_current_channel()?;
                Ok(EffectCommand::InviteUser {
                    target,
                    channel,
                    context_id: None,
                    operation_instance_id: None,
                })
            }

            ChatCommand::Topic { text } => {
                let channel = self.require_current_channel()?;

                Ok(EffectCommand::SetTopic { channel, text })
            }

            ChatCommand::Pin { message_id } => Ok(EffectCommand::PinMessage { message_id }),

            ChatCommand::Unpin { message_id } => Ok(EffectCommand::UnpinMessage { message_id }),

            ChatCommand::Op { target } => Ok(EffectCommand::GrantModerator {
                channel: self.current_channel_id(),
                target,
            }),

            ChatCommand::Deop { target } => Ok(EffectCommand::RevokeModerator {
                channel: self.current_channel_id(),
                target,
            }),

            ChatCommand::Mode { channel, flags } => {
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
        let cmd = ChatCommand::Msg {
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
        let cmd = ChatCommand::Nick {
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
        let cmd = ChatCommand::Me {
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

        let cmd = ChatCommand::Me {
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

        let cmd = ChatCommand::Kick {
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
        let cmd = ChatCommand::Mute {
            target: "alice".to_string(),
            duration: Some(Duration::from_secs(300)),
        };

        // Use dispatch_unchecked to test mapping without capability check
        let result = dispatcher.dispatch_unchecked(cmd);
        assert!(result.is_ok());
        match result.unwrap() {
            EffectCommand::MuteUser {
                channel,
                target,
                duration_secs,
            } => {
                assert_eq!(channel, None);
                assert_eq!(target, "alice");
                assert_eq!(duration_secs, Some(300));
            }
            _ => panic!("Wrong effect command type"),
        }
    }

    #[test]
    fn test_dispatch_homeinvite() {
        let dispatcher = CommandDispatcher::new();
        let cmd = ChatCommand::HomeInvite {
            target: "authority-abc".to_string(),
        };

        let result = dispatcher.dispatch_unchecked(cmd);
        assert!(result.is_ok());
        match result.unwrap() {
            EffectCommand::SendHomeInvitation { contact_id } => {
                assert_eq!(contact_id, "authority-abc");
            }
            _ => panic!("Wrong effect command type"),
        }
    }

    #[test]
    fn test_dispatch_homeaccept() {
        let dispatcher = CommandDispatcher::new();
        let cmd = ChatCommand::HomeAccept;

        let result = dispatcher.dispatch_unchecked(cmd);
        assert!(result.is_ok());
        match result.unwrap() {
            EffectCommand::AcceptPendingHomeInvitation => {}
            _ => panic!("Wrong effect command type"),
        }
    }

    #[test]
    fn test_dispatch_op_includes_channel_hint() {
        let mut dispatcher = CommandDispatcher::new();
        dispatcher.set_current_channel("general");

        let cmd = ChatCommand::Op {
            target: "alice".to_string(),
        };
        let result = match dispatcher.dispatch_unchecked(cmd) {
            Ok(result) => result,
            Err(error) => panic!("dispatch failed: {error}"),
        };
        match result {
            EffectCommand::GrantModerator { channel, target } => {
                assert_eq!(channel.as_deref(), Some("general"));
                assert_eq!(target, "alice");
            }
            other => panic!("unexpected effect command: {other:?}"),
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
