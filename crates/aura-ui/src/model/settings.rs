pub const DEFAULT_CAPABILITY_FULL: &str =
    "send_dm, send_message, update_contact, view_members, join_channel, leave_context, invite, manage_channel, pin_content, moderate:kick, moderate:ban, moderate:mute, grant_moderator";
pub const DEFAULT_CAPABILITY_PARTIAL: &str =
    "send_dm, send_message, update_contact, view_members, join_channel, leave_context";
pub const DEFAULT_CAPABILITY_LIMITED: &str = "send_dm, view_members";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSection {
    Profile,
    GuardianThreshold,
    RequestRecovery,
    Devices,
    Authority,
    Appearance,
    Info,
}

impl SettingsSection {
    pub const ALL: [Self; 7] = [
        Self::Profile,
        Self::GuardianThreshold,
        Self::RequestRecovery,
        Self::Devices,
        Self::Authority,
        Self::Appearance,
        Self::Info,
    ];

    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Profile => "Profile",
            Self::GuardianThreshold => "Guardian Threshold",
            Self::RequestRecovery => "Request Recovery",
            Self::Devices => "Devices",
            Self::Authority => "Authority",
            Self::Appearance => "Appearance",
            Self::Info => "Info",
        }
    }

    #[must_use]
    pub const fn subtitle(self) -> &'static str {
        match self {
            Self::Profile => "Configure profile settings",
            Self::GuardianThreshold => "Configure guardian policy",
            Self::RequestRecovery => "Configure recovery operations",
            Self::Devices => "Configure devices",
            Self::Authority => "Authority scope",
            Self::Appearance => "Theme and display",
            Self::Info => "Application and environment details",
        }
    }

    #[must_use]
    pub const fn dom_id(self) -> &'static str {
        match self {
            Self::Profile => aura_app::ui_contract::settings_section_item_id(
                aura_app::ui_contract::SettingsSectionSurfaceId::Shared(
                    aura_app::ui_contract::SharedSettingsSectionId::Profile,
                ),
            ),
            Self::GuardianThreshold => aura_app::ui_contract::settings_section_item_id(
                aura_app::ui_contract::SettingsSectionSurfaceId::Shared(
                    aura_app::ui_contract::SharedSettingsSectionId::GuardianThreshold,
                ),
            ),
            Self::RequestRecovery => aura_app::ui_contract::settings_section_item_id(
                aura_app::ui_contract::SettingsSectionSurfaceId::Shared(
                    aura_app::ui_contract::SharedSettingsSectionId::RequestRecovery,
                ),
            ),
            Self::Devices => aura_app::ui_contract::settings_section_item_id(
                aura_app::ui_contract::SettingsSectionSurfaceId::Shared(
                    aura_app::ui_contract::SharedSettingsSectionId::Devices,
                ),
            ),
            Self::Authority => aura_app::ui_contract::settings_section_item_id(
                aura_app::ui_contract::SettingsSectionSurfaceId::Shared(
                    aura_app::ui_contract::SharedSettingsSectionId::Authority,
                ),
            ),
            Self::Appearance => aura_app::ui_contract::settings_section_item_id(
                aura_app::ui_contract::SettingsSectionSurfaceId::FrontendSpecific(
                    aura_app::ui_contract::FrontendSpecificSettingsSectionId::Appearance,
                ),
            ),
            Self::Info => aura_app::ui_contract::settings_section_item_id(
                aura_app::ui_contract::SettingsSectionSurfaceId::FrontendSpecific(
                    aura_app::ui_contract::FrontendSpecificSettingsSectionId::Info,
                ),
            ),
        }
    }

    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Profile => 0,
            Self::GuardianThreshold => 1,
            Self::RequestRecovery => 2,
            Self::Devices => 3,
            Self::Authority => 4,
            Self::Appearance => 5,
            Self::Info => 6,
        }
    }

    #[must_use]
    pub const fn from_index(index: usize) -> Self {
        match index {
            0 => Self::Profile,
            1 => Self::GuardianThreshold,
            2 => Self::RequestRecovery,
            3 => Self::Devices,
            4 => Self::Authority,
            5 => Self::Appearance,
            _ => Self::Info,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityTier {
    Full,
    Partial,
    Limited,
}

impl CapabilityTier {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Full => "Full",
            Self::Partial => "Partial",
            Self::Limited => "Limited",
        }
    }

    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Full => Self::Partial,
            Self::Partial => Self::Limited,
            Self::Limited => Self::Full,
        }
    }

    #[must_use]
    pub const fn prev(self) -> Self {
        match self {
            Self::Full => Self::Limited,
            Self::Partial => Self::Full,
            Self::Limited => Self::Partial,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessOverrideLevel {
    Limited,
    Partial,
}

impl AccessOverrideLevel {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Limited => "Limited",
            Self::Partial => "Partial",
        }
    }

    #[must_use]
    pub const fn toggle(self) -> Self {
        match self {
            Self::Limited => Self::Partial,
            Self::Partial => Self::Limited,
        }
    }
}
