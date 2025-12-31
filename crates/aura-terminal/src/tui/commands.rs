//! # IRC-Style Commands
//!
//! Command parser and dispatcher for IRC-style slash commands.
//! See `work/neighbor.md` Section 4.4 for the command specification.
//!
//! ## Architecture
//!
//! Business logic has been moved to `aura_app::ui::workflows::chat_commands`.
//! This module re-exports the portable types and adds TUI-specific aliases
//! for backwards compatibility.

// Re-export portable types from aura-app
pub use aura_app::ui::types::{
    all_command_help, command_help, commands_in_category, is_command, normalize_channel_name,
    parse_chat_command, parse_duration, ChatCommand, CommandCapability, CommandCategory,
    CommandError, CommandHelp,
};

/// Type alias for backwards compatibility
pub type IrcCommand = ChatCommand;

/// Type alias for backwards compatibility
pub type ParseError = CommandError;

/// Parse an input string into an IRC command (backwards compatibility alias)
#[inline]
pub fn parse_command(input: &str) -> Result<IrcCommand, ParseError> {
    parse_chat_command(input)
}

// All types (IrcCommand, CommandCapability, ParseError, etc.) are now re-exported from aura-app.
// Tests remain here to verify the TUI integration works correctly.

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

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
