//! # IRC-Style Commands
//!
//! Command parser and dispatcher for IRC-style slash commands.
//! See `work/neighbor.md` Section 4.4 for the command specification.

use std::time::Duration;

/// Capability required to execute a command
#[derive(Debug, Clone, PartialEq, Eq)]
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
    pub fn as_str(&self) -> &'static str {
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
}

/// Parsed IRC-style command
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IrcCommand {
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
        /// Channel name to join/create
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

impl IrcCommand {
    /// Get the capability required to execute this command
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
    pub fn is_admin_command(&self) -> bool {
        matches!(
            self,
            Self::Op { .. } | Self::Deop { .. } | Self::Mode { .. }
        )
    }
}

/// Command parse error
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
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

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotACommand => write!(f, "Not a command"),
            Self::UnknownCommand(cmd) => write!(f, "Unknown command: /{}", cmd),
            Self::MissingArgument { command, argument } => {
                write!(f, "/{} requires <{}>", command, argument)
            }
            Self::InvalidArgument {
                command,
                argument,
                reason,
            } => {
                write!(f, "/{}: invalid {}: {}", command, argument, reason)
            }
        }
    }
}

impl std::error::Error for ParseError {}

/// Parse an input string into an IRC command
pub fn parse_command(input: &str) -> Result<IrcCommand, ParseError> {
    let input = input.trim();

    if !input.starts_with('/') {
        return Err(ParseError::NotACommand);
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
                .ok_or_else(|| ParseError::MissingArgument {
                    command: "msg".to_string(),
                    argument: "user".to_string(),
                })?
                .to_string();
            let text = arg_parts
                .next()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| ParseError::MissingArgument {
                    command: "msg".to_string(),
                    argument: "text".to_string(),
                })?
                .to_string();
            Ok(IrcCommand::Msg { target, text })
        }

        "me" => {
            if args.is_empty() {
                return Err(ParseError::MissingArgument {
                    command: "me".to_string(),
                    argument: "action".to_string(),
                });
            }
            Ok(IrcCommand::Me {
                action: args.to_string(),
            })
        }

        "nick" => {
            if args.is_empty() {
                return Err(ParseError::MissingArgument {
                    command: "nick".to_string(),
                    argument: "name".to_string(),
                });
            }
            Ok(IrcCommand::Nick {
                name: args.to_string(),
            })
        }

        "who" => Ok(IrcCommand::Who),

        "whois" => {
            if args.is_empty() {
                return Err(ParseError::MissingArgument {
                    command: "whois".to_string(),
                    argument: "user".to_string(),
                });
            }
            Ok(IrcCommand::Whois {
                target: args.to_string(),
            })
        }

        "leave" | "part" | "quit" => Ok(IrcCommand::Leave),

        "join" | "j" => {
            if args.is_empty() {
                return Err(ParseError::MissingArgument {
                    command: "join".to_string(),
                    argument: "channel".to_string(),
                });
            }
            // Strip leading # if present for normalization
            let channel = args.trim_start_matches('#').to_string();
            Ok(IrcCommand::Join { channel })
        }

        "help" | "h" | "?" => {
            let command = if args.is_empty() {
                None
            } else {
                Some(args.to_string())
            };
            Ok(IrcCommand::Help { command })
        }

        "kick" => {
            let mut arg_parts = args.splitn(2, ' ');
            let target = arg_parts
                .next()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| ParseError::MissingArgument {
                    command: "kick".to_string(),
                    argument: "user".to_string(),
                })?
                .to_string();
            let reason = arg_parts.next().filter(|s| !s.is_empty()).map(String::from);
            Ok(IrcCommand::Kick { target, reason })
        }

        "ban" => {
            let mut arg_parts = args.splitn(2, ' ');
            let target = arg_parts
                .next()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| ParseError::MissingArgument {
                    command: "ban".to_string(),
                    argument: "user".to_string(),
                })?
                .to_string();
            let reason = arg_parts.next().filter(|s| !s.is_empty()).map(String::from);
            Ok(IrcCommand::Ban { target, reason })
        }

        "unban" => {
            if args.is_empty() {
                return Err(ParseError::MissingArgument {
                    command: "unban".to_string(),
                    argument: "user".to_string(),
                });
            }
            Ok(IrcCommand::Unban {
                target: args.to_string(),
            })
        }

        "mute" => {
            let mut arg_parts = args.splitn(2, ' ');
            let target = arg_parts
                .next()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| ParseError::MissingArgument {
                    command: "mute".to_string(),
                    argument: "user".to_string(),
                })?
                .to_string();
            let duration = arg_parts
                .next()
                .filter(|s| !s.is_empty())
                .map(|s| parse_duration(s))
                .transpose()
                .map_err(|e| ParseError::InvalidArgument {
                    command: "mute".to_string(),
                    argument: "duration".to_string(),
                    reason: e,
                })?;
            Ok(IrcCommand::Mute { target, duration })
        }

        "unmute" => {
            if args.is_empty() {
                return Err(ParseError::MissingArgument {
                    command: "unmute".to_string(),
                    argument: "user".to_string(),
                });
            }
            Ok(IrcCommand::Unmute {
                target: args.to_string(),
            })
        }

        "invite" => {
            if args.is_empty() {
                return Err(ParseError::MissingArgument {
                    command: "invite".to_string(),
                    argument: "user".to_string(),
                });
            }
            Ok(IrcCommand::Invite {
                target: args.to_string(),
            })
        }

        "topic" => {
            if args.is_empty() {
                return Err(ParseError::MissingArgument {
                    command: "topic".to_string(),
                    argument: "text".to_string(),
                });
            }
            Ok(IrcCommand::Topic {
                text: args.to_string(),
            })
        }

        "pin" => {
            if args.is_empty() {
                return Err(ParseError::MissingArgument {
                    command: "pin".to_string(),
                    argument: "message_id".to_string(),
                });
            }
            Ok(IrcCommand::Pin {
                message_id: args.to_string(),
            })
        }

        "unpin" => {
            if args.is_empty() {
                return Err(ParseError::MissingArgument {
                    command: "unpin".to_string(),
                    argument: "message_id".to_string(),
                });
            }
            Ok(IrcCommand::Unpin {
                message_id: args.to_string(),
            })
        }

        "op" => {
            if args.is_empty() {
                return Err(ParseError::MissingArgument {
                    command: "op".to_string(),
                    argument: "user".to_string(),
                });
            }
            Ok(IrcCommand::Op {
                target: args.to_string(),
            })
        }

        "deop" => {
            if args.is_empty() {
                return Err(ParseError::MissingArgument {
                    command: "deop".to_string(),
                    argument: "user".to_string(),
                });
            }
            Ok(IrcCommand::Deop {
                target: args.to_string(),
            })
        }

        "mode" => {
            let mut arg_parts = args.splitn(2, ' ');
            let channel = arg_parts
                .next()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| ParseError::MissingArgument {
                    command: "mode".to_string(),
                    argument: "channel".to_string(),
                })?
                .to_string();
            let flags = arg_parts
                .next()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| ParseError::MissingArgument {
                    command: "mode".to_string(),
                    argument: "flags".to_string(),
                })?
                .to_string();
            Ok(IrcCommand::Mode { channel, flags })
        }

        _ => Err(ParseError::UnknownCommand(command)),
    }
}

/// Parse a duration string (e.g., "5m", "1h", "30s")
fn parse_duration(s: &str) -> Result<Duration, String> {
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
        .map_err(|_| format!("invalid number: {}", num_str))?;

    let secs = match unit {
        's' => num,
        'm' => num * 60,
        'h' => num * 3600,
        'd' => num * 86400,
        _ => return Err(format!("unknown unit: {}", unit)),
    };

    Ok(Duration::from_secs(secs))
}

/// Check if input looks like a command (starts with /)
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    pub fn name(&self) -> &'static str {
        match self {
            Self::User => "User Commands",
            Self::Moderator => "Moderator Commands",
            Self::Admin => "Admin Commands",
        }
    }
}

/// Get help for all commands
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
pub fn command_help(name: &str) -> Option<CommandHelp> {
    all_command_help()
        .into_iter()
        .find(|h| h.name == name.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_msg() {
        let cmd = parse_command("/msg alice hello there").unwrap();
        assert_eq!(
            cmd,
            IrcCommand::Msg {
                target: "alice".to_string(),
                text: "hello there".to_string()
            }
        );
    }

    #[test]
    fn test_parse_msg_alias() {
        let cmd = parse_command("/m bob hi").unwrap();
        assert_eq!(
            cmd,
            IrcCommand::Msg {
                target: "bob".to_string(),
                text: "hi".to_string()
            }
        );
    }

    #[test]
    fn test_parse_me() {
        let cmd = parse_command("/me waves hello").unwrap();
        assert_eq!(
            cmd,
            IrcCommand::Me {
                action: "waves hello".to_string()
            }
        );
    }

    #[test]
    fn test_parse_nick() {
        let cmd = parse_command("/nick NewName").unwrap();
        assert_eq!(
            cmd,
            IrcCommand::Nick {
                name: "NewName".to_string()
            }
        );
    }

    #[test]
    fn test_parse_who() {
        let cmd = parse_command("/who").unwrap();
        assert_eq!(cmd, IrcCommand::Who);
    }

    #[test]
    fn test_parse_whois() {
        let cmd = parse_command("/whois alice").unwrap();
        assert_eq!(
            cmd,
            IrcCommand::Whois {
                target: "alice".to_string()
            }
        );
    }

    #[test]
    fn test_parse_leave_variants() {
        assert_eq!(parse_command("/leave").unwrap(), IrcCommand::Leave);
        assert_eq!(parse_command("/part").unwrap(), IrcCommand::Leave);
        assert_eq!(parse_command("/quit").unwrap(), IrcCommand::Leave);
    }

    #[test]
    fn test_parse_help() {
        let cmd = parse_command("/help").unwrap();
        assert_eq!(cmd, IrcCommand::Help { command: None });

        let cmd = parse_command("/help kick").unwrap();
        assert_eq!(
            cmd,
            IrcCommand::Help {
                command: Some("kick".to_string())
            }
        );
    }

    #[test]
    fn test_parse_kick() {
        let cmd = parse_command("/kick alice").unwrap();
        assert_eq!(
            cmd,
            IrcCommand::Kick {
                target: "alice".to_string(),
                reason: None
            }
        );

        let cmd = parse_command("/kick alice spamming").unwrap();
        assert_eq!(
            cmd,
            IrcCommand::Kick {
                target: "alice".to_string(),
                reason: Some("spamming".to_string())
            }
        );
    }

    #[test]
    fn test_parse_mute_with_duration() {
        let cmd = parse_command("/mute alice 5m").unwrap();
        assert_eq!(
            cmd,
            IrcCommand::Mute {
                target: "alice".to_string(),
                duration: Some(Duration::from_secs(300))
            }
        );

        let cmd = parse_command("/mute bob 1h").unwrap();
        assert_eq!(
            cmd,
            IrcCommand::Mute {
                target: "bob".to_string(),
                duration: Some(Duration::from_secs(3600))
            }
        );
    }

    #[test]
    fn test_parse_op_deop() {
        let cmd = parse_command("/op alice").unwrap();
        assert_eq!(
            cmd,
            IrcCommand::Op {
                target: "alice".to_string()
            }
        );

        let cmd = parse_command("/deop alice").unwrap();
        assert_eq!(
            cmd,
            IrcCommand::Deop {
                target: "alice".to_string()
            }
        );
    }

    #[test]
    fn test_parse_mode() {
        let cmd = parse_command("/mode #general +i").unwrap();
        assert_eq!(
            cmd,
            IrcCommand::Mode {
                channel: "#general".to_string(),
                flags: "+i".to_string()
            }
        );
    }

    #[test]
    fn test_parse_errors() {
        // Not a command
        assert!(matches!(
            parse_command("hello"),
            Err(ParseError::NotACommand)
        ));

        // Unknown command
        assert!(matches!(
            parse_command("/unknown"),
            Err(ParseError::UnknownCommand(_))
        ));

        // Missing argument
        assert!(matches!(
            parse_command("/msg alice"),
            Err(ParseError::MissingArgument { .. })
        ));

        assert!(matches!(
            parse_command("/kick"),
            Err(ParseError::MissingArgument { .. })
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
        let cmd = parse_command("/kick alice").unwrap();
        assert_eq!(cmd.required_capability(), CommandCapability::ModerateKick);

        let cmd = parse_command("/help").unwrap();
        assert_eq!(cmd.required_capability(), CommandCapability::None);
    }

    #[test]
    fn test_command_categories() {
        let cmd = parse_command("/msg alice hi").unwrap();
        assert!(cmd.is_user_command());
        assert!(!cmd.is_moderator_command());

        let cmd = parse_command("/kick alice").unwrap();
        assert!(cmd.is_moderator_command());
        assert!(!cmd.is_user_command());

        let cmd = parse_command("/op alice").unwrap();
        assert!(cmd.is_admin_command());
    }

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_duration("5m").unwrap(), Duration::from_secs(300));
        assert_eq!(parse_duration("2h").unwrap(), Duration::from_secs(7200));
        assert_eq!(parse_duration("1d").unwrap(), Duration::from_secs(86400));
        // Default to minutes
        assert_eq!(parse_duration("10").unwrap(), Duration::from_secs(600));
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
}
