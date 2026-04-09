//! # IRC-Style Chat Commands - Portable Business Logic
//!
//! Portable command parser and types for IRC-style slash commands.
//! This module provides frontend-agnostic command parsing that can be
//! used by CLI, TUI, mobile, and web frontends.
//!
//! ## Usage
//!
//! ```rust
//! use aura_app::ui::workflows::chat_commands::{parse_chat_command, ChatCommand};
//!
//! let cmd = parse_chat_command("/msg alice hello").unwrap();
//! match cmd {
//!     ChatCommand::Msg { target, text } => {
//!         // Handle private message
//!     }
//!     _ => {}
//! }
//! ```

mod help;
mod parse_support;

use std::time::Duration;

pub use help::{
    all_command_help, command_help, commands_in_category, CommandCategory, CommandHelp,
};
pub use parse_support::{is_command, normalize_channel_name, parse_duration, ParseDurationError};

/// Capability required to execute a command.
///
/// Maps to Biscuit capability strings for authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandCapability {
    /// No capability required
    None,
    /// Send direct messages
    SendDm,
    /// Send messages to current context
    SendMessage,
    /// Update own contact suggestion
    UpdateContact,
    /// View member list
    ViewMembers,
    /// Join a channel
    JoinChannel,
    /// Leave current context
    LeaveContext,
    /// Kick users
    ModerateKick,
    /// Ban/unban users
    ModerateBan,
    /// Mute/unmute users
    ModerateMute,
    /// Invite users
    Invite,
    /// Manage channel settings
    ManageChannel,
    /// Pin/unpin content
    PinContent,
    /// Grant/revoke moderator status
    GrantModerator,
}

impl CommandCapability {
    /// Get capability string for Biscuit evaluation
    #[must_use]
    pub fn as_biscuit_capability(&self) -> &'static str {
        match self {
            Self::None => "",
            Self::SendDm => "send_dm",
            Self::SendMessage => "send_message",
            Self::UpdateContact => "update_contact",
            Self::ViewMembers => "view_members",
            Self::JoinChannel => "join_channel",
            Self::LeaveContext => "leave_context",
            Self::ModerateKick => "moderate:kick",
            Self::ModerateBan => "moderate:ban",
            Self::ModerateMute => "moderate:mute",
            Self::Invite => "invite",
            Self::ManageChannel => "manage_channel",
            Self::PinContent => "pin_content",
            Self::GrantModerator => "grant_moderator",
        }
    }

    /// Check if this capability requires moderator privileges
    #[must_use]
    pub fn requires_moderator(&self) -> bool {
        matches!(
            self,
            Self::ModerateKick
                | Self::ModerateBan
                | Self::ModerateMute
                | Self::Invite
                | Self::ManageChannel
                | Self::PinContent
        )
    }

    /// Check if this capability requires admin privileges
    #[must_use]
    pub fn requires_admin(&self) -> bool {
        matches!(self, Self::GrantModerator)
    }
}

impl std::fmt::Display for CommandCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_biscuit_capability())
    }
}

/// Parsed IRC-style chat command
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatCommand {
    // === User Commands ===
    /// `/msg <user> <text>` - Send private message
    Msg {
        /// Target user to message
        target: String,
        /// Message text
        text: String,
    },

    /// `/me <action>` - Send action/emote
    Me {
        /// Action text
        action: String,
    },

    /// `/nick <name>` - Update contact suggestion
    Nick {
        /// New nickname
        name: String,
    },

    /// `/who` - List participants
    Who,

    /// `/whois <user>` - View user info
    Whois {
        /// Target user
        target: String,
    },

    /// `/leave` - Leave current context
    Leave,

    /// `/join <channel>` - Join or create a channel
    Join {
        /// Channel name to join/create (normalized, without leading #)
        channel: String,
    },

    /// `/help [command]` - Show help
    Help {
        /// Optional command to get help for
        command: Option<String>,
    },

    /// `/neighborhood <name>` - Create/select active neighborhood
    Neighborhood {
        /// Neighborhood display name
        name: String,
    },

    /// `/nhadd <home_id>` - Add a home to the active neighborhood
    NhAdd {
        /// Home ID to add as a member
        home_id: String,
    },

    /// `/nhlink <home_id>` - Link direct one_hop_link to a home
    NhLink {
        /// Home ID to link directly
        home_id: String,
    },

    /// `/homeinvite <user>` - Send a home invitation to a user
    HomeInvite {
        /// User authority ID to invite
        target: String,
    },

    /// `/homeaccept` - Accept the first pending home invitation
    HomeAccept,

    // === Moderator Commands ===
    /// `/kick <user> [reason]` - Remove user
    Kick {
        /// User to kick
        target: String,
        /// Optional kick reason
        reason: Option<String>,
    },

    /// `/ban <user> [reason]` - Ban user
    Ban {
        /// User to ban
        target: String,
        /// Optional ban reason
        reason: Option<String>,
    },

    /// `/unban <user>` - Remove ban
    Unban {
        /// User to unban
        target: String,
    },

    /// `/mute <user> [duration]` - Silence user
    Mute {
        /// User to mute
        target: String,
        /// Optional mute duration
        duration: Option<Duration>,
    },

    /// `/unmute <user>` - Remove mute
    Unmute {
        /// User to unmute
        target: String,
    },

    /// `/invite <user>` - Invite user
    Invite {
        /// User to invite
        target: String,
    },

    /// `/topic <text>` - Set channel topic
    Topic {
        /// New topic text
        text: String,
    },

    /// `/pin <message_id>` - Pin message
    Pin {
        /// Message ID to pin
        message_id: String,
    },

    /// `/unpin <message_id>` - Unpin message
    Unpin {
        /// Message ID to unpin
        message_id: String,
    },

    // === Admin Commands ===
    /// `/op <user>` - Grant moderator capabilities
    Op {
        /// User to promote
        target: String,
    },

    /// `/deop <user>` - Revoke moderator capabilities
    Deop {
        /// User to demote
        target: String,
    },

    /// `/mode <channel> <flags>` - Set channel mode
    Mode {
        /// Target channel
        channel: String,
        /// Mode flags
        flags: String,
    },
}

impl ChatCommand {
    /// Get the capability required to execute this command
    #[must_use]
    pub fn required_capability(&self) -> CommandCapability {
        match self {
            Self::Msg { .. } => CommandCapability::SendDm,
            Self::Me { .. } => CommandCapability::SendMessage,
            Self::Nick { .. } => CommandCapability::UpdateContact,
            Self::Who => CommandCapability::ViewMembers,
            Self::Whois { .. } => CommandCapability::ViewMembers,
            Self::Leave => CommandCapability::LeaveContext,
            Self::Join { .. } => CommandCapability::JoinChannel,
            Self::Help { .. } => CommandCapability::None,
            Self::Neighborhood { .. } | Self::NhAdd { .. } | Self::NhLink { .. } => {
                CommandCapability::None
            }
            Self::HomeInvite { .. } => CommandCapability::Invite,
            Self::HomeAccept => CommandCapability::JoinChannel,
            Self::Kick { .. } => CommandCapability::ModerateKick,
            Self::Ban { .. } | Self::Unban { .. } => CommandCapability::ModerateBan,
            Self::Mute { .. } | Self::Unmute { .. } => CommandCapability::ModerateMute,
            Self::Invite { .. } => CommandCapability::Invite,
            Self::Topic { .. } => CommandCapability::ManageChannel,
            Self::Pin { .. } | Self::Unpin { .. } => CommandCapability::PinContent,
            Self::Op { .. } | Self::Deop { .. } => CommandCapability::GrantModerator,
            Self::Mode { .. } => CommandCapability::ManageChannel,
        }
    }

    /// Get command name
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Msg { .. } => "msg",
            Self::Me { .. } => "me",
            Self::Nick { .. } => "nick",
            Self::Who => "who",
            Self::Whois { .. } => "whois",
            Self::Leave => "leave",
            Self::Join { .. } => "join",
            Self::Help { .. } => "help",
            Self::Neighborhood { .. } => "neighborhood",
            Self::NhAdd { .. } => "nhadd",
            Self::NhLink { .. } => "nhlink",
            Self::HomeInvite { .. } => "homeinvite",
            Self::HomeAccept => "homeaccept",
            Self::Kick { .. } => "kick",
            Self::Ban { .. } => "ban",
            Self::Unban { .. } => "unban",
            Self::Mute { .. } => "mute",
            Self::Unmute { .. } => "unmute",
            Self::Invite { .. } => "invite",
            Self::Topic { .. } => "topic",
            Self::Pin { .. } => "pin",
            Self::Unpin { .. } => "unpin",
            Self::Op { .. } => "op",
            Self::Deop { .. } => "deop",
            Self::Mode { .. } => "mode",
        }
    }

    /// Check if this is a user-level command
    #[must_use]
    pub fn is_user_command(&self) -> bool {
        matches!(
            self,
            Self::Msg { .. }
                | Self::Me { .. }
                | Self::Nick { .. }
                | Self::Who
                | Self::Whois { .. }
                | Self::Leave
                | Self::Join { .. }
                | Self::Help { .. }
                | Self::Neighborhood { .. }
                | Self::NhAdd { .. }
                | Self::NhLink { .. }
                | Self::HomeInvite { .. }
                | Self::HomeAccept
        )
    }

    /// Check if this is a moderator-level command
    #[must_use]
    pub fn is_moderator_command(&self) -> bool {
        matches!(
            self,
            Self::Kick { .. }
                | Self::Ban { .. }
                | Self::Unban { .. }
                | Self::Mute { .. }
                | Self::Unmute { .. }
                | Self::Invite { .. }
                | Self::Topic { .. }
                | Self::Pin { .. }
                | Self::Unpin { .. }
        )
    }

    /// Check if this is an admin-level command
    #[must_use]
    pub fn is_admin_command(&self) -> bool {
        matches!(
            self,
            Self::Op { .. } | Self::Deop { .. } | Self::Mode { .. }
        )
    }

    /// Get the command category
    #[must_use]
    pub fn category(&self) -> CommandCategory {
        if self.is_user_command() {
            CommandCategory::User
        } else if self.is_moderator_command() {
            CommandCategory::Moderator
        } else {
            CommandCategory::Admin
        }
    }
}

/// Command parse error
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    /// Not a command (doesn't start with /)
    NotACommand,
    /// Unknown command name
    UnknownCommand(String),
    /// Missing required argument
    MissingArgument {
        /// Command name
        command: String,
        /// Missing argument name
        argument: String,
    },
    /// Invalid argument format
    InvalidArgument {
        /// Command name
        command: String,
        /// Argument name
        argument: String,
        /// Reason why argument is invalid
        reason: String,
    },
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotACommand => write!(f, "Not a command"),
            Self::UnknownCommand(cmd) => {
                write!(
                    f,
                    "Unknown command: /{cmd}. Run /help for available commands"
                )
            }
            Self::MissingArgument { command, argument } => {
                write!(
                    f,
                    "Missing required argument: /{command} requires <{argument}>"
                )
            }
            Self::InvalidArgument {
                command,
                argument,
                reason,
            } => {
                write!(
                    f,
                    "Invalid argument: /{command}: invalid {argument}: {reason}"
                )
            }
        }
    }
}

impl std::error::Error for CommandError {}

/// Parse an input string into a chat command
pub fn parse_chat_command(input: &str) -> Result<ChatCommand, CommandError> {
    let input = input.trim();

    if !input.starts_with('/') {
        return Err(CommandError::NotACommand);
    }

    let without_slash = &input[1..];
    let mut parts = without_slash.splitn(2, ' ');
    let command = parts.next().unwrap_or("").to_lowercase();
    let args = parts.next().unwrap_or("").trim();

    match command.as_str() {
        "msg" | "m" => {
            let mut arg_parts = args.splitn(2, ' ');
            let target = arg_parts
                .next()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| CommandError::MissingArgument {
                    command: "msg".to_string(),
                    argument: "user".to_string(),
                })?
                .to_string();
            let text = arg_parts
                .next()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| CommandError::MissingArgument {
                    command: "msg".to_string(),
                    argument: "text".to_string(),
                })?
                .to_string();
            Ok(ChatCommand::Msg { target, text })
        }

        "me" => {
            if args.is_empty() {
                return Err(CommandError::MissingArgument {
                    command: "me".to_string(),
                    argument: "action".to_string(),
                });
            }
            Ok(ChatCommand::Me {
                action: args.to_string(),
            })
        }

        "nick" => {
            if args.is_empty() {
                return Err(CommandError::MissingArgument {
                    command: "nick".to_string(),
                    argument: "name".to_string(),
                });
            }
            Ok(ChatCommand::Nick {
                name: args.to_string(),
            })
        }

        "who" => Ok(ChatCommand::Who),

        "whois" => {
            if args.is_empty() {
                return Err(CommandError::MissingArgument {
                    command: "whois".to_string(),
                    argument: "user".to_string(),
                });
            }
            Ok(ChatCommand::Whois {
                target: args.to_string(),
            })
        }

        "leave" | "part" | "quit" => Ok(ChatCommand::Leave),

        "join" | "j" => {
            if args.is_empty() {
                return Err(CommandError::MissingArgument {
                    command: "join".to_string(),
                    argument: "channel".to_string(),
                });
            }
            // Strip leading # if present for normalization
            let channel = normalize_channel_name(args);
            Ok(ChatCommand::Join { channel })
        }

        "help" | "h" | "?" => {
            let command = if args.is_empty() {
                None
            } else {
                Some(args.to_string())
            };
            Ok(ChatCommand::Help { command })
        }

        "neighborhood" | "nh" => {
            if args.is_empty() {
                return Err(CommandError::MissingArgument {
                    command: "neighborhood".to_string(),
                    argument: "name".to_string(),
                });
            }
            Ok(ChatCommand::Neighborhood {
                name: args.to_string(),
            })
        }

        "nhadd" => {
            if args.is_empty() {
                return Err(CommandError::MissingArgument {
                    command: "nhadd".to_string(),
                    argument: "home_id".to_string(),
                });
            }
            Ok(ChatCommand::NhAdd {
                home_id: args.to_string(),
            })
        }

        "nhlink" => {
            if args.is_empty() {
                return Err(CommandError::MissingArgument {
                    command: "nhlink".to_string(),
                    argument: "home_id".to_string(),
                });
            }
            Ok(ChatCommand::NhLink {
                home_id: args.to_string(),
            })
        }

        "homeinvite" | "hinvite" => {
            if args.is_empty() {
                return Err(CommandError::MissingArgument {
                    command: "homeinvite".to_string(),
                    argument: "user".to_string(),
                });
            }
            Ok(ChatCommand::HomeInvite {
                target: args.to_string(),
            })
        }

        "homeaccept" | "haccept" => Ok(ChatCommand::HomeAccept),

        "kick" => {
            let mut arg_parts = args.splitn(2, ' ');
            let target = arg_parts
                .next()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| CommandError::MissingArgument {
                    command: "kick".to_string(),
                    argument: "user".to_string(),
                })?
                .to_string();
            let reason = arg_parts.next().filter(|s| !s.is_empty()).map(String::from);
            Ok(ChatCommand::Kick { target, reason })
        }

        "ban" => {
            let mut arg_parts = args.splitn(2, ' ');
            let target = arg_parts
                .next()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| CommandError::MissingArgument {
                    command: "ban".to_string(),
                    argument: "user".to_string(),
                })?
                .to_string();
            let reason = arg_parts.next().filter(|s| !s.is_empty()).map(String::from);
            Ok(ChatCommand::Ban { target, reason })
        }

        "unban" => {
            if args.is_empty() {
                return Err(CommandError::MissingArgument {
                    command: "unban".to_string(),
                    argument: "user".to_string(),
                });
            }
            Ok(ChatCommand::Unban {
                target: args.to_string(),
            })
        }

        "mute" => {
            let mut arg_parts = args.splitn(2, ' ');
            let target = arg_parts
                .next()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| CommandError::MissingArgument {
                    command: "mute".to_string(),
                    argument: "user".to_string(),
                })?
                .to_string();
            let duration = arg_parts
                .next()
                .filter(|s| !s.is_empty())
                .map(parse_duration)
                .transpose()
                .map_err(|e| CommandError::InvalidArgument {
                    command: "mute".to_string(),
                    argument: "duration".to_string(),
                    reason: e.to_string(),
                })?;
            Ok(ChatCommand::Mute { target, duration })
        }

        "unmute" => {
            if args.is_empty() {
                return Err(CommandError::MissingArgument {
                    command: "unmute".to_string(),
                    argument: "user".to_string(),
                });
            }
            Ok(ChatCommand::Unmute {
                target: args.to_string(),
            })
        }

        "invite" => {
            if args.is_empty() {
                return Err(CommandError::MissingArgument {
                    command: "invite".to_string(),
                    argument: "user".to_string(),
                });
            }
            Ok(ChatCommand::Invite {
                target: args.to_string(),
            })
        }

        "topic" => {
            if args.is_empty() {
                return Err(CommandError::MissingArgument {
                    command: "topic".to_string(),
                    argument: "text".to_string(),
                });
            }
            Ok(ChatCommand::Topic {
                text: args.to_string(),
            })
        }

        "pin" => {
            if args.is_empty() {
                return Err(CommandError::MissingArgument {
                    command: "pin".to_string(),
                    argument: "message_id".to_string(),
                });
            }
            Ok(ChatCommand::Pin {
                message_id: args.to_string(),
            })
        }

        "unpin" => {
            if args.is_empty() {
                return Err(CommandError::MissingArgument {
                    command: "unpin".to_string(),
                    argument: "message_id".to_string(),
                });
            }
            Ok(ChatCommand::Unpin {
                message_id: args.to_string(),
            })
        }

        "op" => {
            if args.is_empty() {
                return Err(CommandError::MissingArgument {
                    command: "op".to_string(),
                    argument: "user".to_string(),
                });
            }
            Ok(ChatCommand::Op {
                target: args.to_string(),
            })
        }

        "deop" => {
            if args.is_empty() {
                return Err(CommandError::MissingArgument {
                    command: "deop".to_string(),
                    argument: "user".to_string(),
                });
            }
            Ok(ChatCommand::Deop {
                target: args.to_string(),
            })
        }

        "mode" => {
            let mut arg_parts = args.splitn(2, ' ');
            let channel = arg_parts
                .next()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| CommandError::MissingArgument {
                    command: "mode".to_string(),
                    argument: "channel".to_string(),
                })?
                .to_string();
            let flags = arg_parts
                .next()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| CommandError::MissingArgument {
                    command: "mode".to_string(),
                    argument: "flags".to_string(),
                })?
                .to_string();
            // Keep channel argument normalization consistent with `/join`.
            let channel = normalize_channel_name(&channel);
            Ok(ChatCommand::Mode { channel, flags })
        }

        _ => Err(CommandError::UnknownCommand(command)),
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_msg() {
        let cmd = parse_chat_command("/msg alice hello there").unwrap();
        assert_eq!(
            cmd,
            ChatCommand::Msg {
                target: "alice".to_string(),
                text: "hello there".to_string()
            }
        );
    }

    #[test]
    fn test_parse_msg_alias() {
        let cmd = parse_chat_command("/m bob hi").unwrap();
        assert_eq!(
            cmd,
            ChatCommand::Msg {
                target: "bob".to_string(),
                text: "hi".to_string()
            }
        );
    }

    #[test]
    fn test_parse_me() {
        let cmd = parse_chat_command("/me waves hello").unwrap();
        assert_eq!(
            cmd,
            ChatCommand::Me {
                action: "waves hello".to_string()
            }
        );
    }

    #[test]
    fn test_parse_nick() {
        let cmd = parse_chat_command("/nick NewName").unwrap();
        assert_eq!(
            cmd,
            ChatCommand::Nick {
                name: "NewName".to_string()
            }
        );
    }

    #[test]
    fn test_parse_who() {
        let cmd = parse_chat_command("/who").unwrap();
        assert_eq!(cmd, ChatCommand::Who);
    }

    #[test]
    fn test_parse_whois() {
        let cmd = parse_chat_command("/whois alice").unwrap();
        assert_eq!(
            cmd,
            ChatCommand::Whois {
                target: "alice".to_string()
            }
        );
    }

    #[test]
    fn test_parse_leave_variants() {
        assert_eq!(parse_chat_command("/leave").unwrap(), ChatCommand::Leave);
        assert_eq!(parse_chat_command("/part").unwrap(), ChatCommand::Leave);
        assert_eq!(parse_chat_command("/quit").unwrap(), ChatCommand::Leave);
    }

    #[test]
    fn test_parse_join() {
        let cmd = parse_chat_command("/join #general").unwrap();
        assert_eq!(
            cmd,
            ChatCommand::Join {
                channel: "general".to_string()
            }
        );

        let cmd = parse_chat_command("/j general").unwrap();
        assert_eq!(
            cmd,
            ChatCommand::Join {
                channel: "general".to_string()
            }
        );
    }

    #[test]
    fn test_parse_help() {
        let cmd = parse_chat_command("/help").unwrap();
        assert_eq!(cmd, ChatCommand::Help { command: None });

        let cmd = parse_chat_command("/help kick").unwrap();
        assert_eq!(
            cmd,
            ChatCommand::Help {
                command: Some("kick".to_string())
            }
        );

        // Aliases
        assert_eq!(
            parse_chat_command("/h").unwrap(),
            ChatCommand::Help { command: None }
        );
        assert_eq!(
            parse_chat_command("/?").unwrap(),
            ChatCommand::Help { command: None }
        );
    }

    #[test]
    fn test_parse_neighborhood_commands() {
        let cmd = parse_chat_command("/neighborhood North Block").unwrap();
        assert_eq!(
            cmd,
            ChatCommand::Neighborhood {
                name: "North Block".to_string()
            }
        );

        let cmd = parse_chat_command("/nhadd home-123").unwrap();
        assert_eq!(
            cmd,
            ChatCommand::NhAdd {
                home_id: "home-123".to_string()
            }
        );

        let cmd = parse_chat_command("/nhlink home-456").unwrap();
        assert_eq!(
            cmd,
            ChatCommand::NhLink {
                home_id: "home-456".to_string()
            }
        );

        let cmd = parse_chat_command("/homeinvite authority-abc").unwrap();
        assert_eq!(
            cmd,
            ChatCommand::HomeInvite {
                target: "authority-abc".to_string()
            }
        );

        let cmd = parse_chat_command("/homeaccept").unwrap();
        assert_eq!(cmd, ChatCommand::HomeAccept);
    }

    #[test]
    fn test_parse_kick() {
        let cmd = parse_chat_command("/kick alice").unwrap();
        assert_eq!(
            cmd,
            ChatCommand::Kick {
                target: "alice".to_string(),
                reason: None
            }
        );

        let cmd = parse_chat_command("/kick alice spamming").unwrap();
        assert_eq!(
            cmd,
            ChatCommand::Kick {
                target: "alice".to_string(),
                reason: Some("spamming".to_string())
            }
        );
    }

    #[test]
    fn test_parse_ban_unban() {
        let cmd = parse_chat_command("/ban alice harassment").unwrap();
        assert_eq!(
            cmd,
            ChatCommand::Ban {
                target: "alice".to_string(),
                reason: Some("harassment".to_string())
            }
        );

        let cmd = parse_chat_command("/unban alice").unwrap();
        assert_eq!(
            cmd,
            ChatCommand::Unban {
                target: "alice".to_string()
            }
        );
    }

    #[test]
    fn test_parse_mute_with_duration() {
        let cmd = parse_chat_command("/mute alice 5m").unwrap();
        assert_eq!(
            cmd,
            ChatCommand::Mute {
                target: "alice".to_string(),
                duration: Some(Duration::from_secs(300))
            }
        );

        let cmd = parse_chat_command("/mute bob 1h").unwrap();
        assert_eq!(
            cmd,
            ChatCommand::Mute {
                target: "bob".to_string(),
                duration: Some(Duration::from_secs(3600))
            }
        );

        let cmd = parse_chat_command("/mute carol").unwrap();
        assert_eq!(
            cmd,
            ChatCommand::Mute {
                target: "carol".to_string(),
                duration: None
            }
        );
    }

    #[test]
    fn test_parse_op_deop() {
        let cmd = parse_chat_command("/op alice").unwrap();
        assert_eq!(
            cmd,
            ChatCommand::Op {
                target: "alice".to_string()
            }
        );

        let cmd = parse_chat_command("/deop alice").unwrap();
        assert_eq!(
            cmd,
            ChatCommand::Deop {
                target: "alice".to_string()
            }
        );
    }

    #[test]
    fn test_parse_mode() {
        let cmd = parse_chat_command("/mode #general +i").unwrap();
        assert_eq!(
            cmd,
            ChatCommand::Mode {
                channel: "general".to_string(),
                flags: "+i".to_string()
            }
        );
    }

    #[test]
    fn test_parse_topic_pin_unpin() {
        let cmd = parse_chat_command("/topic Welcome to our channel!").unwrap();
        assert_eq!(
            cmd,
            ChatCommand::Topic {
                text: "Welcome to our channel!".to_string()
            }
        );

        let cmd = parse_chat_command("/pin msg123").unwrap();
        assert_eq!(
            cmd,
            ChatCommand::Pin {
                message_id: "msg123".to_string()
            }
        );

        let cmd = parse_chat_command("/unpin msg123").unwrap();
        assert_eq!(
            cmd,
            ChatCommand::Unpin {
                message_id: "msg123".to_string()
            }
        );
    }

    #[test]
    fn test_parse_invite() {
        let cmd = parse_chat_command("/invite alice").unwrap();
        assert_eq!(
            cmd,
            ChatCommand::Invite {
                target: "alice".to_string()
            }
        );
    }

    #[test]
    fn test_parse_unmute() {
        let cmd = parse_chat_command("/unmute alice").unwrap();
        assert_eq!(
            cmd,
            ChatCommand::Unmute {
                target: "alice".to_string()
            }
        );
    }

    #[test]
    fn test_parse_errors() {
        // Not a command
        assert!(matches!(
            parse_chat_command("hello"),
            Err(CommandError::NotACommand)
        ));

        // Unknown command
        assert!(matches!(
            parse_chat_command("/unknown"),
            Err(CommandError::UnknownCommand(_))
        ));

        // Missing argument
        assert!(matches!(
            parse_chat_command("/msg alice"),
            Err(CommandError::MissingArgument { .. })
        ));

        assert!(matches!(
            parse_chat_command("/kick"),
            Err(CommandError::MissingArgument { .. })
        ));

        assert!(matches!(
            parse_chat_command("/mode #general"),
            Err(CommandError::MissingArgument { .. })
        ));
    }

    #[test]
    fn test_is_command() {
        assert!(is_command("/help"));
        assert!(is_command("  /msg alice hi"));
        assert!(!is_command("hello"));
        assert!(!is_command("hello /me waves"));
    }

    #[test]
    fn test_command_capability() {
        let cmd = parse_chat_command("/kick alice").unwrap();
        assert_eq!(cmd.required_capability(), CommandCapability::ModerateKick);

        let cmd = parse_chat_command("/help").unwrap();
        assert_eq!(cmd.required_capability(), CommandCapability::None);

        let cmd = parse_chat_command("/op alice").unwrap();
        assert_eq!(cmd.required_capability(), CommandCapability::GrantModerator);
    }

    #[test]
    fn test_command_categories() {
        let cmd = parse_chat_command("/msg alice hi").unwrap();
        assert!(cmd.is_user_command());
        assert!(!cmd.is_moderator_command());
        assert_eq!(cmd.category(), CommandCategory::User);

        let cmd = parse_chat_command("/kick alice").unwrap();
        assert!(cmd.is_moderator_command());
        assert!(!cmd.is_user_command());
        assert_eq!(cmd.category(), CommandCategory::Moderator);

        let cmd = parse_chat_command("/op alice").unwrap();
        assert!(cmd.is_admin_command());
        assert_eq!(cmd.category(), CommandCategory::Admin);
    }

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_duration("5m").unwrap(), Duration::from_secs(300));
        assert_eq!(parse_duration("2h").unwrap(), Duration::from_secs(7200));
        assert_eq!(parse_duration("1d").unwrap(), Duration::from_secs(86400));
        // Default to minutes
        assert_eq!(parse_duration("10").unwrap(), Duration::from_secs(600));
        // Case insensitive
        assert_eq!(parse_duration("5M").unwrap(), Duration::from_secs(300));
        assert_eq!(parse_duration("2H").unwrap(), Duration::from_secs(7200));
    }

    #[test]
    fn test_parse_duration_errors() {
        assert!(parse_duration("").is_err());
        assert!(parse_duration("abc").is_err());
        assert!(parse_duration("5x").is_err());
        assert!(parse_duration(&format!("{}d", u64::MAX)).is_err());
    }

    #[test]
    fn test_normalize_channel_name() {
        assert_eq!(normalize_channel_name("#general"), "general");
        assert_eq!(normalize_channel_name("##double"), "double"); // strips all leading #
        assert_eq!(normalize_channel_name("general"), "general");
    }

    #[test]
    fn test_all_command_help() {
        let help = all_command_help();
        assert!(!help.is_empty());

        // Check we have all categories
        assert!(help.iter().any(|h| h.category == CommandCategory::User));
        assert!(help
            .iter()
            .any(|h| h.category == CommandCategory::Moderator));
        assert!(help.iter().any(|h| h.category == CommandCategory::Admin));
    }

    #[test]
    fn test_command_help() {
        let help = command_help("kick").unwrap();
        assert_eq!(help.name, "kick");
        assert_eq!(help.category, CommandCategory::Moderator);

        assert!(command_help("nonexistent").is_none());
    }

    #[test]
    fn test_commands_in_category() {
        let user_cmds = commands_in_category(CommandCategory::User);
        assert!(user_cmds
            .iter()
            .all(|h| h.category == CommandCategory::User));
        assert!(user_cmds.iter().any(|h| h.name == "help"));

        let mod_cmds = commands_in_category(CommandCategory::Moderator);
        assert!(mod_cmds.iter().any(|h| h.name == "kick"));
    }

    #[test]
    fn test_capability_biscuit_strings() {
        assert_eq!(CommandCapability::SendDm.as_biscuit_capability(), "send_dm");
        assert_eq!(
            CommandCapability::ModerateKick.as_biscuit_capability(),
            "moderate:kick"
        );
        assert_eq!(CommandCapability::None.as_biscuit_capability(), "");
    }

    #[test]
    fn test_capability_privilege_levels() {
        assert!(!CommandCapability::SendDm.requires_moderator());
        assert!(!CommandCapability::SendDm.requires_admin());

        assert!(CommandCapability::ModerateKick.requires_moderator());
        assert!(!CommandCapability::ModerateKick.requires_admin());

        assert!(CommandCapability::GrantModerator.requires_admin());
    }

    #[test]
    fn test_command_category_all() {
        let all = CommandCategory::all();
        assert_eq!(all.len(), 3);
        assert!(all.contains(&CommandCategory::User));
        assert!(all.contains(&CommandCategory::Moderator));
        assert!(all.contains(&CommandCategory::Admin));
    }

    #[test]
    fn test_case_insensitive_commands() {
        assert!(parse_chat_command("/MSG alice hi").is_ok());
        assert!(parse_chat_command("/Msg alice hi").is_ok());
        assert!(parse_chat_command("/KICK bob").is_ok());
    }

    #[test]
    fn test_command_error_display_has_stable_prefixes() {
        let missing = parse_chat_command("/msg alice").expect_err("missing text must fail");
        assert!(missing.to_string().contains("Missing required argument"));

        let invalid =
            parse_chat_command("/mute alice notaduration").expect_err("invalid duration must fail");
        assert!(invalid.to_string().contains("Invalid argument"));

        let unknown = parse_chat_command("/totally-unknown").expect_err("unknown must fail");
        assert!(unknown.to_string().contains("Unknown command"));
    }
}
