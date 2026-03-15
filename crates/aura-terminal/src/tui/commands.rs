//! # IRC-Style Commands
//!
//! Command parser and dispatcher for IRC-style slash commands.
//!
//! This module re-exports the portable command types from `aura-app`.

pub use aura_app::ui::types::{
    all_command_help, command_help, commands_in_category, is_command, normalize_channel_name,
    parse_chat_command, parse_duration, ChatCommand, CommandCapability, CommandCategory,
    CommandError, CommandHelp,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

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
    fn test_parse_errors() {
        assert!(matches!(
            parse_chat_command("hello"),
            Err(CommandError::NotACommand)
        ));
        assert!(matches!(
            parse_chat_command("/unknown"),
            Err(CommandError::UnknownCommand(_))
        ));
        assert!(matches!(
            parse_chat_command("/msg alice"),
            Err(CommandError::MissingArgument { .. })
        ));
        assert!(matches!(
            parse_chat_command("/kick"),
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
    }

    #[test]
    fn test_command_categories() {
        let cmd = parse_chat_command("/msg alice hi").unwrap();
        assert!(cmd.is_user_command());
        assert!(!cmd.is_moderator_command());

        let cmd = parse_chat_command("/kick alice").unwrap();
        assert!(cmd.is_moderator_command());
        assert!(!cmd.is_user_command());

        let cmd = parse_chat_command("/op alice").unwrap();
        assert!(cmd.is_admin_command());
    }

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_duration("5m").unwrap(), Duration::from_secs(300));
        assert_eq!(parse_duration("2h").unwrap(), Duration::from_secs(7200));
        assert_eq!(parse_duration("1d").unwrap(), Duration::from_secs(86400));
        assert_eq!(parse_duration("10").unwrap(), Duration::from_secs(600));
    }

    #[test]
    fn test_all_command_help() {
        let help = all_command_help();
        assert!(!help.is_empty());
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
