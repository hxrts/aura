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

use std::time::Duration;

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
    /// Grant/revoke steward status
    GrantSteward,
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
            Self::GrantSteward => "grant_steward",
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
        matches!(self, Self::GrantSteward)
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
    /// `/op <user>` - Grant steward capabilities
    Op {
        /// User to promote
        target: String,
    },

    /// `/deop <user>` - Revoke steward capabilities
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
            Self::Kick { .. } => CommandCapability::ModerateKick,
            Self::Ban { .. } | Self::Unban { .. } => CommandCapability::ModerateBan,
            Self::Mute { .. } | Self::Unmute { .. } => CommandCapability::ModerateMute,
            Self::Invite { .. } => CommandCapability::Invite,
            Self::Topic { .. } => CommandCapability::ManageChannel,
            Self::Pin { .. } | Self::Unpin { .. } => CommandCapability::PinContent,
            Self::Op { .. } | Self::Deop { .. } => CommandCapability::GrantSteward,
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
            Self::UnknownCommand(cmd) => write!(f, "Unknown command: /{cmd}"),
            Self::MissingArgument { command, argument } => {
                write!(f, "/{command} requires <{argument}>")
            }
            Self::InvalidArgument {
                command,
                argument,
                reason,
            } => {
                write!(f, "/{command}: invalid {argument}: {reason}")
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
                .map(|s| parse_duration(s))
                .transpose()
                .map_err(|e| CommandError::InvalidArgument {
                    command: "mute".to_string(),
                    argument: "duration".to_string(),
                    reason: e,
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
            Ok(ChatCommand::Mode { channel, flags })
        }

        _ => Err(CommandError::UnknownCommand(command)),
    }
}

/// Parse a duration string (e.g., "5m", "1h", "30s", "1d")
///
/// Supported formats:
/// - `Ns` - N seconds
/// - `Nm` - N minutes
/// - `Nh` - N hours
/// - `Nd` - N days
/// - `N` - N minutes (default unit)
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim().to_lowercase();

    if s.is_empty() {
        return Err("empty duration".to_string());
    }

    let (num_str, unit) = if s.ends_with('s') {
        (&s[..s.len() - 1], 's')
    } else if s.ends_with('m') {
        (&s[..s.len() - 1], 'm')
    } else if s.ends_with('h') {
        (&s[..s.len() - 1], 'h')
    } else if s.ends_with('d') {
        (&s[..s.len() - 1], 'd')
    } else {
        // Default to minutes if no unit
        (s.as_str(), 'm')
    };

    let num: u64 = num_str
        .parse()
        .map_err(|_| format!("invalid number: {num_str}"))?;

    let secs = match unit {
        's' => num,
        'm' => num * 60,
        'h' => num * 3600,
        'd' => num * 86400,
        _ => return Err(format!("unknown unit: {unit}")),
    };

    Ok(Duration::from_secs(secs))
}

/// Normalize a channel name by stripping leading # characters
#[must_use]
pub fn normalize_channel_name(name: &str) -> String {
    name.trim_start_matches('#').to_string()
}

/// Check if input looks like a command (starts with /)
#[must_use]
pub fn is_command(input: &str) -> bool {
    input.trim().starts_with('/')
}

/// Command help information
#[derive(Debug, Clone)]
pub struct CommandHelp {
    /// Command name
    pub name: &'static str,
    /// Command syntax
    pub syntax: &'static str,
    /// Command description
    pub description: &'static str,
    /// Required capability
    pub capability: CommandCapability,
    /// Command category
    pub category: CommandCategory,
}

/// Command category
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandCategory {
    /// User commands available to everyone
    User,
    /// Moderator commands for stewards
    Moderator,
    /// Admin commands for home stewards
    Admin,
}

impl CommandCategory {
    /// Get category display name
    #[must_use]
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::User => "User Commands",
            Self::Moderator => "Moderator Commands",
            Self::Admin => "Admin Commands",
        }
    }

    /// Get all categories in order
    #[must_use]
    pub fn all() -> &'static [CommandCategory] {
        &[Self::User, Self::Moderator, Self::Admin]
    }
}

impl std::fmt::Display for CommandCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Get help for all commands
#[must_use]
pub fn all_command_help() -> Vec<CommandHelp> {
    vec![
        // User commands
        CommandHelp {
            name: "msg",
            syntax: "/msg <user> <text>",
            description: "Send a private message to a user",
            capability: CommandCapability::SendDm,
            category: CommandCategory::User,
        },
        CommandHelp {
            name: "me",
            syntax: "/me <action>",
            description: "Send an action/emote message",
            capability: CommandCapability::SendMessage,
            category: CommandCategory::User,
        },
        CommandHelp {
            name: "nick",
            syntax: "/nick <name>",
            description: "Update your contact suggestion (display name)",
            capability: CommandCapability::UpdateContact,
            category: CommandCategory::User,
        },
        CommandHelp {
            name: "who",
            syntax: "/who",
            description: "List all participants in current context",
            capability: CommandCapability::ViewMembers,
            category: CommandCategory::User,
        },
        CommandHelp {
            name: "whois",
            syntax: "/whois <user>",
            description: "View detailed info about a user",
            capability: CommandCapability::ViewMembers,
            category: CommandCategory::User,
        },
        CommandHelp {
            name: "leave",
            syntax: "/leave",
            description: "Leave the current context",
            capability: CommandCapability::LeaveContext,
            category: CommandCategory::User,
        },
        CommandHelp {
            name: "join",
            syntax: "/join <channel>",
            description: "Join or create a channel (e.g., /join general)",
            capability: CommandCapability::JoinChannel,
            category: CommandCategory::User,
        },
        CommandHelp {
            name: "help",
            syntax: "/help [command]",
            description: "Show help for commands",
            capability: CommandCapability::None,
            category: CommandCategory::User,
        },
        // Moderator commands
        CommandHelp {
            name: "kick",
            syntax: "/kick <user> [reason]",
            description: "Remove a user from the home/channel",
            capability: CommandCapability::ModerateKick,
            category: CommandCategory::Moderator,
        },
        CommandHelp {
            name: "ban",
            syntax: "/ban <user> [reason]",
            description: "Ban a user from the home",
            capability: CommandCapability::ModerateBan,
            category: CommandCategory::Moderator,
        },
        CommandHelp {
            name: "unban",
            syntax: "/unban <user>",
            description: "Remove a user's ban",
            capability: CommandCapability::ModerateBan,
            category: CommandCategory::Moderator,
        },
        CommandHelp {
            name: "mute",
            syntax: "/mute <user> [duration]",
            description: "Temporarily silence a user (e.g., /mute alice 5m)",
            capability: CommandCapability::ModerateMute,
            category: CommandCategory::Moderator,
        },
        CommandHelp {
            name: "unmute",
            syntax: "/unmute <user>",
            description: "Remove a user's mute",
            capability: CommandCapability::ModerateMute,
            category: CommandCategory::Moderator,
        },
        CommandHelp {
            name: "invite",
            syntax: "/invite <user>",
            description: "Invite a user to the home/channel",
            capability: CommandCapability::Invite,
            category: CommandCategory::Moderator,
        },
        CommandHelp {
            name: "topic",
            syntax: "/topic <text>",
            description: "Set the channel topic",
            capability: CommandCapability::ManageChannel,
            category: CommandCategory::Moderator,
        },
        CommandHelp {
            name: "pin",
            syntax: "/pin <message_id>",
            description: "Pin a message to the channel",
            capability: CommandCapability::PinContent,
            category: CommandCategory::Moderator,
        },
        CommandHelp {
            name: "unpin",
            syntax: "/unpin <message_id>",
            description: "Unpin a message from the channel",
            capability: CommandCapability::PinContent,
            category: CommandCategory::Moderator,
        },
        // Admin commands
        CommandHelp {
            name: "op",
            syntax: "/op <user>",
            description: "Grant steward capabilities to a user",
            capability: CommandCapability::GrantSteward,
            category: CommandCategory::Admin,
        },
        CommandHelp {
            name: "deop",
            syntax: "/deop <user>",
            description: "Revoke steward capabilities from a user",
            capability: CommandCapability::GrantSteward,
            category: CommandCategory::Admin,
        },
        CommandHelp {
            name: "mode",
            syntax: "/mode <channel> <flags>",
            description: "Set channel mode (e.g., +i for invite-only)",
            capability: CommandCapability::ManageChannel,
            category: CommandCategory::Admin,
        },
    ]
}

/// Get help for a specific command
#[must_use]
pub fn command_help(name: &str) -> Option<CommandHelp> {
    all_command_help()
        .into_iter()
        .find(|h| h.name == name.to_lowercase())
}

/// Get all commands in a specific category
#[must_use]
pub fn commands_in_category(category: CommandCategory) -> Vec<CommandHelp> {
    all_command_help()
        .into_iter()
        .filter(|h| h.category == category)
        .collect()
}

#[cfg(test)]
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
                channel: "#general".to_string(),
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
        assert_eq!(cmd.required_capability(), CommandCapability::GrantSteward);
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
        assert!(user_cmds.iter().all(|h| h.category == CommandCategory::User));
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

        assert!(CommandCapability::GrantSteward.requires_admin());
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
}
