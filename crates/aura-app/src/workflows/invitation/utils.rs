#![allow(missing_docs)]

use crate::views::invitations::InvitationType;

/// 1 hour TTL preset in hours
pub const INVITATION_TTL_1_HOUR: u64 = 1;

/// 1 day (24 hours) TTL preset in hours
pub const INVITATION_TTL_1_DAY: u64 = 24;

/// 1 week (168 hours) TTL preset in hours
pub const INVITATION_TTL_1_WEEK: u64 = 168;

/// 30 days (720 hours) TTL preset in hours
pub const INVITATION_TTL_30_DAYS: u64 = 720;

/// Standard TTL presets in hours: 1h, 1d, 1w, 30d
pub const INVITATION_TTL_PRESETS: [u64; 4] = [
    INVITATION_TTL_1_HOUR,
    INVITATION_TTL_1_DAY,
    INVITATION_TTL_1_WEEK,
    INVITATION_TTL_30_DAYS,
];

/// Default TTL for invitations (24 hours)
pub const DEFAULT_INVITATION_TTL_HOURS: u64 = INVITATION_TTL_1_DAY;

#[inline]
#[must_use]
pub const fn ttl_hours_to_ms(hours: u64) -> u64 {
    hours * 60 * 60 * 1000
}

#[must_use]
pub fn format_ttl_display(hours: u64) -> String {
    match hours {
        0 => "No expiration".to_string(),
        1 => "1 hour".to_string(),
        h if h < 24 => format!("{h} hours"),
        24 => "1 day".to_string(),
        h if h < 168 => {
            let days = h / 24;
            if days == 1 {
                "1 day".to_string()
            } else {
                format!("{days} days")
            }
        }
        168 => "1 week".to_string(),
        h if h < 720 => {
            let weeks = h / 168;
            if weeks == 1 {
                "1 week".to_string()
            } else {
                format!("{weeks} weeks")
            }
        }
        720 => "30 days".to_string(),
        h => {
            let days = h / 24;
            format!("{days} days")
        }
    }
}

#[must_use]
pub fn ttl_preset_index(hours: u64) -> usize {
    INVITATION_TTL_PRESETS
        .iter()
        .position(|&preset| preset == hours)
        .unwrap_or(1)
}

#[must_use]
pub fn next_ttl_preset(current_hours: u64) -> u64 {
    let current_index = ttl_preset_index(current_hours);
    let next_index = (current_index + 1) % INVITATION_TTL_PRESETS.len();
    INVITATION_TTL_PRESETS[next_index]
}

#[must_use]
pub fn prev_ttl_preset(current_hours: u64) -> u64 {
    let current_index = ttl_preset_index(current_hours);
    let prev_index = if current_index == 0 {
        INVITATION_TTL_PRESETS.len() - 1
    } else {
        current_index - 1
    };
    INVITATION_TTL_PRESETS[prev_index]
}

/// Portable invitation role value for CLI parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvitationRoleValue {
    Contact,
    Guardian,
    Channel,
}

impl InvitationRoleValue {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Contact => "contact",
            Self::Guardian => "guardian",
            Self::Channel => "channel",
        }
    }

    #[must_use]
    pub fn to_invitation_type(&self) -> InvitationType {
        match self {
            Self::Contact => InvitationType::Home,
            Self::Guardian => InvitationType::Guardian,
            Self::Channel => InvitationType::Chat,
        }
    }
}

impl std::fmt::Display for InvitationRoleValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Contact => write!(f, "contact"),
            Self::Guardian => write!(f, "guardian"),
            Self::Channel => write!(f, "channel"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvitationRoleParseError {
    Empty,
    InvalidRole(String),
}

impl std::fmt::Display for InvitationRoleParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "role cannot be empty"),
            Self::InvalidRole(role) => write!(
                f,
                "invalid invitation role '{role}' (expected one of: contact, guardian, channel)"
            ),
        }
    }
}

impl std::error::Error for InvitationRoleParseError {}

pub fn parse_invitation_role(role: &str) -> Result<InvitationRoleValue, InvitationRoleParseError> {
    let normalized = role.trim();
    if normalized.is_empty() {
        return Err(InvitationRoleParseError::Empty);
    }
    if normalized.eq_ignore_ascii_case("contact") {
        return Ok(InvitationRoleValue::Contact);
    }
    if normalized.eq_ignore_ascii_case("guardian") {
        return Ok(InvitationRoleValue::Guardian);
    }
    if normalized.eq_ignore_ascii_case("channel") {
        return Ok(InvitationRoleValue::Channel);
    }
    Err(InvitationRoleParseError::InvalidRole(
        normalized.to_string(),
    ))
}

#[must_use]
pub fn format_invitation_type(inv_type: InvitationType) -> &'static str {
    match inv_type {
        InvitationType::Home => "Home",
        InvitationType::Guardian => "Guardian",
        InvitationType::Chat => "Channel",
    }
}

#[must_use]
pub fn format_invitation_type_detailed(inv_type: InvitationType, context: Option<&str>) -> String {
    match (inv_type, context) {
        (InvitationType::Home, None) => "Home".to_string(),
        (InvitationType::Home, Some(ctx)) => format!("Home ({ctx})"),
        (InvitationType::Guardian, None) => "Guardian".to_string(),
        (InvitationType::Guardian, Some(ctx)) => format!("Guardian (for: {ctx})"),
        (InvitationType::Chat, None) => "Channel".to_string(),
        (InvitationType::Chat, Some(ctx)) => format!("Channel ({ctx})"),
    }
}
