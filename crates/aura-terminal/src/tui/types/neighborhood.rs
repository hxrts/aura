use aura_app::ui::types;

/// A member in a home.
#[derive(Clone, Debug, Default)]
pub struct HomeMember {
    pub id: String,
    pub name: String,
    pub is_moderator: bool,
    pub is_self: bool,
}

impl HomeMember {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            ..Default::default()
        }
    }

    pub fn moderator(mut self) -> Self {
        self.is_moderator = true;
        self
    }

    pub fn is_current_user(mut self) -> Self {
        self.is_self = true;
        self
    }
}

impl From<&types::home::HomeMember> for HomeMember {
    fn from(member: &types::home::HomeMember) -> Self {
        Self {
            id: member.id.to_string(),
            name: member.name.clone(),
            is_moderator: member.is_moderator(),
            is_self: false,
        }
    }
}

/// Home storage budget.
#[derive(Clone, Debug, Default)]
pub struct HomeBudget {
    pub total: u64,
    pub used: u64,
    pub member_count: u8,
    pub max_members: u8,
}

impl HomeBudget {
    pub fn usage_percent(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.used as f32 / self.total as f32) * 100.0
        }
    }
}

impl From<&types::HomeFlowBudget> for HomeBudget {
    fn from(budget: &types::HomeFlowBudget) -> Self {
        Self {
            total: budget.total_allocation(),
            used: budget.total_used(),
            member_count: budget.member_count,
            max_members: types::MAX_MEMBERS,
        }
    }
}

/// Home visibility/access level.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AccessLevel {
    #[default]
    Limited,
    Partial,
    Full,
}

impl AccessLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Limited => "Limited",
            Self::Partial => "Partial",
            Self::Full => "Full",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Limited => "->",
            Self::Partial => "◇",
            Self::Full => "⌂",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Limited => Self::Partial,
            Self::Partial => Self::Full,
            Self::Full => Self::Limited,
        }
    }
}

impl From<AccessLevel> for types::AccessLevel {
    fn from(value: AccessLevel) -> Self {
        match value {
            AccessLevel::Limited => Self::Limited,
            AccessLevel::Partial => Self::Partial,
            AccessLevel::Full => Self::Full,
        }
    }
}

impl From<types::AccessLevel> for AccessLevel {
    fn from(value: types::AccessLevel) -> Self {
        match value {
            types::AccessLevel::Limited => Self::Limited,
            types::AccessLevel::Partial => Self::Partial,
            types::AccessLevel::Full => Self::Full,
        }
    }
}

/// Home summary for neighborhood view.
#[derive(Clone, Debug, Default)]
pub struct HomeSummary {
    pub id: String,
    pub name: Option<String>,
    pub member_count: u8,
    pub max_members: u8,
    pub is_home: bool,
    pub can_enter: bool,
}

impl HomeSummary {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            max_members: 8,
            ..Default::default()
        }
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn with_members(mut self, count: u8) -> Self {
        self.member_count = count;
        self
    }

    pub fn home(mut self) -> Self {
        self.is_home = true;
        self.can_enter = true;
        self
    }

    pub fn accessible(mut self) -> Self {
        self.can_enter = true;
        self
    }
}
