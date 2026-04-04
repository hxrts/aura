#![allow(missing_docs)]

use crate::workflows::chat_commands::{parse_chat_command, ChatCommand, CommandError};

/// Parse-level command values (never executable directly).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedCommand {
    Msg {
        target: String,
        text: String,
    },
    Me {
        action: String,
    },
    Nick {
        name: String,
    },
    Who,
    Whois {
        target: String,
    },
    Leave,
    Join {
        channel: String,
    },
    Help {
        command: Option<String>,
    },
    Neighborhood {
        name: String,
    },
    NhAdd {
        home_id: String,
    },
    NhLink {
        home_id: String,
    },
    HomeInvite {
        target: String,
    },
    HomeAccept,
    Kick {
        target: String,
        reason: Option<String>,
    },
    Ban {
        target: String,
        reason: Option<String>,
    },
    Unban {
        target: String,
    },
    Mute {
        target: String,
        duration: Option<std::time::Duration>,
    },
    Unmute {
        target: String,
    },
    Invite {
        target: String,
    },
    Topic {
        text: String,
    },
    Pin {
        message_id: String,
    },
    Unpin {
        message_id: String,
    },
    Op {
        target: String,
    },
    Deop {
        target: String,
    },
    Mode {
        channel: String,
        flags: String,
    },
}

impl ParsedCommand {
    /// Parse a user input command string into `ParsedCommand`.
    pub fn parse(input: &str) -> Result<Self, CommandError> {
        parse_chat_command(input).map(Self::from)
    }
}

impl From<ChatCommand> for ParsedCommand {
    fn from(value: ChatCommand) -> Self {
        match value {
            ChatCommand::Msg { target, text } => Self::Msg { target, text },
            ChatCommand::Me { action } => Self::Me { action },
            ChatCommand::Nick { name } => Self::Nick { name },
            ChatCommand::Who => Self::Who,
            ChatCommand::Whois { target } => Self::Whois { target },
            ChatCommand::Leave => Self::Leave,
            ChatCommand::Join { channel } => Self::Join { channel },
            ChatCommand::Help { command } => Self::Help { command },
            ChatCommand::Neighborhood { name } => Self::Neighborhood { name },
            ChatCommand::NhAdd { home_id } => Self::NhAdd { home_id },
            ChatCommand::NhLink { home_id } => Self::NhLink { home_id },
            ChatCommand::HomeInvite { target } => Self::HomeInvite { target },
            ChatCommand::HomeAccept => Self::HomeAccept,
            ChatCommand::Kick { target, reason } => Self::Kick { target, reason },
            ChatCommand::Ban { target, reason } => Self::Ban { target, reason },
            ChatCommand::Unban { target } => Self::Unban { target },
            ChatCommand::Mute { target, duration } => Self::Mute { target, duration },
            ChatCommand::Unmute { target } => Self::Unmute { target },
            ChatCommand::Invite { target } => Self::Invite { target },
            ChatCommand::Topic { text } => Self::Topic { text },
            ChatCommand::Pin { message_id } => Self::Pin { message_id },
            ChatCommand::Unpin { message_id } => Self::Unpin { message_id },
            ChatCommand::Op { target } => Self::Op { target },
            ChatCommand::Deop { target } => Self::Deop { target },
            ChatCommand::Mode { channel, flags } => Self::Mode { channel, flags },
        }
    }
}
