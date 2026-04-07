use super::CommandCapability;

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
    /// Moderator commands for moderators
    Moderator,
    /// Admin commands for home moderators
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
        CommandHelp {
            name: "neighborhood",
            syntax: "/neighborhood <name>",
            description: "Create/select the active neighborhood",
            capability: CommandCapability::None,
            category: CommandCategory::User,
        },
        CommandHelp {
            name: "nhadd",
            syntax: "/nhadd <home_id>",
            description: "Add a home as a neighborhood member",
            capability: CommandCapability::None,
            category: CommandCategory::User,
        },
        CommandHelp {
            name: "nhlink",
            syntax: "/nhlink <home_id>",
            description: "Create direct one_hop_link to a home",
            capability: CommandCapability::None,
            category: CommandCategory::User,
        },
        CommandHelp {
            name: "homeinvite",
            syntax: "/homeinvite <user>",
            description: "Send a home invitation to a user authority",
            capability: CommandCapability::Invite,
            category: CommandCategory::User,
        },
        CommandHelp {
            name: "homeaccept",
            syntax: "/homeaccept",
            description: "Accept the first pending home invitation",
            capability: CommandCapability::JoinChannel,
            category: CommandCategory::User,
        },
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
        CommandHelp {
            name: "op",
            syntax: "/op <user>",
            description: "Grant moderator capabilities to a user",
            capability: CommandCapability::GrantModerator,
            category: CommandCategory::Admin,
        },
        CommandHelp {
            name: "deop",
            syntax: "/deop <user>",
            description: "Revoke moderator capabilities from a user",
            capability: CommandCapability::GrantModerator,
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
    let normalized = name.to_lowercase();
    all_command_help()
        .into_iter()
        .find(|help| help.name == normalized)
}

/// Get all commands in a specific category
#[must_use]
pub fn commands_in_category(category: CommandCategory) -> Vec<CommandHelp> {
    all_command_help()
        .into_iter()
        .filter(|help| help.category == category)
        .collect()
}
