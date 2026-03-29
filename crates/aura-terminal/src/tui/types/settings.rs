/// Settings section.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SettingsSection {
    #[default]
    Profile,
    Threshold,
    Recovery,
    Devices,
    Authority,
    Observability,
}

impl SettingsSection {
    pub fn all() -> &'static [Self] {
        &[
            Self::Profile,
            Self::Threshold,
            Self::Recovery,
            Self::Devices,
            Self::Authority,
            Self::Observability,
        ]
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Profile => "Profile",
            Self::Threshold => "Guardian Threshold",
            Self::Recovery => "Request Recovery",
            Self::Devices => "Devices",
            Self::Authority => "Authority",
            Self::Observability => "Observability",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Profile => "Your display name and account information",
            Self::Threshold => "Configure guardians for account recovery",
            Self::Recovery => "Request account recovery from guardians",
            Self::Devices => "Manage devices linked to your account",
            Self::Authority => "Manage your authority and multifactor settings",
            Self::Observability => "Network, sync, and discovery metrics",
        }
    }

    pub fn surface_id(self) -> aura_app::ui_contract::SettingsSectionSurfaceId {
        match self {
            Self::Profile => aura_app::ui_contract::SettingsSectionSurfaceId::Shared(
                aura_app::ui_contract::SharedSettingsSectionId::Profile,
            ),
            Self::Threshold => aura_app::ui_contract::SettingsSectionSurfaceId::Shared(
                aura_app::ui_contract::SharedSettingsSectionId::GuardianThreshold,
            ),
            Self::Recovery => aura_app::ui_contract::SettingsSectionSurfaceId::Shared(
                aura_app::ui_contract::SharedSettingsSectionId::RequestRecovery,
            ),
            Self::Devices => aura_app::ui_contract::SettingsSectionSurfaceId::Shared(
                aura_app::ui_contract::SharedSettingsSectionId::Devices,
            ),
            Self::Authority => aura_app::ui_contract::SettingsSectionSurfaceId::Shared(
                aura_app::ui_contract::SharedSettingsSectionId::Authority,
            ),
            Self::Observability => {
                aura_app::ui_contract::SettingsSectionSurfaceId::FrontendSpecific(
                    aura_app::ui_contract::FrontendSpecificSettingsSectionId::Observability,
                )
            }
        }
    }

    pub fn parity_item_id(self) -> &'static str {
        aura_app::ui_contract::settings_section_item_id(self.surface_id())
    }

    pub fn next(self) -> Self {
        match self {
            Self::Profile => Self::Threshold,
            Self::Threshold => Self::Recovery,
            Self::Recovery => Self::Devices,
            Self::Devices => Self::Authority,
            Self::Authority => Self::Observability,
            Self::Observability => Self::Profile,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Profile => Self::Observability,
            Self::Threshold => Self::Profile,
            Self::Recovery => Self::Threshold,
            Self::Devices => Self::Recovery,
            Self::Authority => Self::Devices,
            Self::Observability => Self::Authority,
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::Profile => 0,
            Self::Threshold => 1,
            Self::Recovery => 2,
            Self::Devices => 3,
            Self::Authority => 4,
            Self::Observability => 5,
        }
    }

    pub fn from_index(index: usize) -> Self {
        match index % Self::all().len() {
            0 => Self::Profile,
            1 => Self::Threshold,
            2 => Self::Recovery,
            3 => Self::Devices,
            4 => Self::Authority,
            _ => Self::Observability,
        }
    }
}

/// Authority information for display.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AuthorityInfo {
    pub id: String,
    pub nickname_suggestion: String,
    pub short_id: String,
    pub is_current: bool,
}

impl AuthorityInfo {
    pub fn new(id: impl Into<String>, nickname_suggestion: impl Into<String>) -> Self {
        let id = id.into();
        let short_id = if id.len() > 8 {
            id[..8].to_string()
        } else {
            id.clone()
        };
        Self {
            id,
            nickname_suggestion: nickname_suggestion.into(),
            short_id,
            is_current: false,
        }
    }

    pub fn current(mut self) -> Self {
        self.is_current = true;
        self
    }
}

/// A registered device.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub is_current: bool,
    pub last_seen: Option<u64>,
}

impl Device {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            is_current: false,
            last_seen: None,
        }
    }

    pub fn current(mut self) -> Self {
        self.is_current = true;
        self
    }
}

/// MFA policy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MfaPolicy {
    #[default]
    Disabled,
    SensitiveOnly,
    AlwaysRequired,
}

impl MfaPolicy {
    pub fn name(self) -> &'static str {
        match self {
            Self::Disabled => "Disabled",
            Self::SensitiveOnly => "Sensitive Only",
            Self::AlwaysRequired => "Always Required",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Disabled => "No additional authentication required",
            Self::SensitiveOnly => "MFA for recovery, device changes, and guardian updates",
            Self::AlwaysRequired => "MFA for all authenticated operations",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Disabled => Self::SensitiveOnly,
            Self::SensitiveOnly => Self::AlwaysRequired,
            Self::AlwaysRequired => Self::Disabled,
        }
    }

    pub fn requires_mfa(self) -> bool {
        !matches!(self, Self::Disabled)
    }
}

/// Channel mode flags.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChannelMode {
    pub moderated: bool,
    pub private: bool,
    pub topic_protected: bool,
    pub invite_only: bool,
}

impl ChannelMode {
    pub fn parse_flags(&mut self, flags: &str) {
        let mut adding = true;
        for flag in flags.chars() {
            match flag {
                '+' => adding = true,
                '-' => adding = false,
                'm' => self.moderated = adding,
                'p' => self.private = adding,
                't' => self.topic_protected = adding,
                'i' => self.invite_only = adding,
                _ => {}
            }
        }
    }

    pub fn description(&self) -> Vec<&'static str> {
        let mut description = Vec::new();
        if self.moderated {
            description.push("Moderated");
        }
        if self.private {
            description.push("Private");
        }
        if self.topic_protected {
            description.push("Topic Protected");
        }
        if self.invite_only {
            description.push("Invite Only");
        }
        description
    }
}

impl std::fmt::Display for ChannelMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut flags = String::from("+");
        if self.moderated {
            flags.push('m');
        }
        if self.private {
            flags.push('p');
        }
        if self.topic_protected {
            flags.push('t');
        }
        if self.invite_only {
            flags.push('i');
        }

        if flags.len() == 1 {
            write!(f, "")
        } else {
            write!(f, "{flags}")
        }
    }
}
