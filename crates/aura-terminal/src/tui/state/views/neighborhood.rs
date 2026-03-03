//! Neighborhood screen view state

use crate::tui::navigation::GridNav;
use crate::tui::state::form::{Validatable, ValidationError};
use crate::tui::types::{AccessLevel, Contact};

/// Neighborhood screen mode
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NeighborhoodMode {
    /// Exploring the neighborhood map
    #[default]
    Map,
    /// Inside a home (detail view)
    Detail,
}

/// Focus area inside Detail mode
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DetailFocus {
    /// Channel list on the left
    #[default]
    Channels,
    /// Member list on the left
    Members,
    /// Message list on the right
    Messages,
    /// Input field on the right
    Input,
}

/// Neighborhood screen state
#[derive(Clone, Debug, Default)]
pub struct NeighborhoodViewState {
    /// Current mode (map vs detail)
    pub mode: NeighborhoodMode,

    /// Detail mode focus
    pub detail_focus: DetailFocus,

    /// Grid navigation state (handles 2D wrap-around)
    pub grid: GridNav,

    /// Desired traversal depth when entering a selected home.
    pub enter_depth: AccessLevel,

    /// Selected neighborhood tab index
    pub selected_neighborhood: usize,
    /// Total neighborhoods (V1: 4)
    pub neighborhood_count: usize,

    /// Selected home index in map mode
    pub selected_home: usize,
    /// Total homes in current neighborhood
    pub home_count: usize,

    /// Entered home id (detail mode)
    pub entered_home_id: Option<String>,

    /// Channel navigation
    pub selected_channel: usize,
    pub channel_count: usize,

    /// Member navigation
    pub selected_member: usize,
    pub member_count: usize,

    /// Messaging
    pub insert_mode: bool,
    pub insert_mode_entry_char: Option<char>,
    pub input_buffer: String,
    pub message_scroll: usize,
    pub message_count: usize,

    /// Whether moderator actions are enabled for current user in this home
    pub moderator_actions_enabled: bool,
}

/// State for home creation modal
///
/// Note: Visibility is controlled by ModalQueue, not a `visible` field.
#[derive(Clone, Debug, Default)]
pub struct HomeCreateModalState {
    /// Home name input
    pub name: String,
    /// Home description (optional)
    pub description: String,
    /// Active field (0 = name, 1 = description)
    pub active_field: usize,
    /// Error message if any
    pub error: Option<String>,
    /// Whether creation is in progress
    pub creating: bool,
}

impl HomeCreateModalState {
    /// Create new modal state
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset state (called when dismissed)
    pub fn reset(&mut self) {
        self.name.clear();
        self.description.clear();
        self.active_field = 0;
        self.error = None;
        self.creating = false;
    }

    /// Check if form can be submitted
    #[must_use]
    pub fn can_submit(&self) -> bool {
        !self.name.trim().is_empty() && !self.creating
    }

    /// Move to next field
    pub fn next_field(&mut self) {
        self.active_field = (self.active_field + 1) % 2;
    }

    /// Move to previous field
    pub fn prev_field(&mut self) {
        self.active_field = if self.active_field == 0 { 1 } else { 0 };
    }

    /// Push a character to the active field
    pub fn push_char(&mut self, c: char) {
        match self.active_field {
            0 => self.name.push(c),
            1 => self.description.push(c),
            _ => {}
        }
        self.error = None;
    }

    /// Pop a character from the active field
    pub fn pop_char(&mut self) {
        match self.active_field {
            0 => {
                self.name.pop();
            }
            1 => {
                self.description.pop();
            }
            _ => {}
        }
        self.error = None;
    }

    /// Set an error
    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
        self.creating = false;
    }

    /// Mark as creating
    pub fn start_creating(&mut self) {
        self.creating = true;
        self.error = None;
    }

    /// Get the name
    #[must_use]
    pub fn get_name(&self) -> &str {
        &self.name
    }

    /// Get the description
    #[must_use]
    pub fn get_description(&self) -> Option<&str> {
        if self.description.is_empty() {
            None
        } else {
            Some(&self.description)
        }
    }
}

/// State for moderator assignment modal.
#[derive(Clone, Debug, Default)]
pub struct ModeratorAssignmentModalState {
    /// Candidate contacts (typically home members).
    pub contacts: Vec<Contact>,
    /// Selected candidate index.
    pub selected_index: usize,
    /// True = grant moderator, false = revoke moderator.
    pub assign: bool,
}

impl ModeratorAssignmentModalState {
    /// Create a modal preloaded with candidate contacts.
    #[must_use]
    pub fn new(contacts: Vec<Contact>) -> Self {
        Self {
            contacts,
            selected_index: 0,
            assign: true,
        }
    }

    /// Toggle grant/revoke mode.
    pub fn toggle_mode(&mut self) {
        self.assign = !self.assign;
    }

    /// Return currently selected contact ID.
    #[must_use]
    pub fn selected_contact_id(&self) -> Option<&str> {
        self.contacts
            .get(self.selected_index)
            .map(|contact| contact.id.as_str())
    }
}

/// State for access-level override modal.
#[derive(Clone, Debug)]
pub struct AccessOverrideModalState {
    /// Candidate contacts to override.
    pub contacts: Vec<Contact>,
    /// Selected candidate index.
    pub selected_index: usize,
    /// Override level (bounded to Partial/Limited).
    pub access_level: AccessLevel,
}

impl Default for AccessOverrideModalState {
    fn default() -> Self {
        Self {
            contacts: Vec::new(),
            selected_index: 0,
            access_level: AccessLevel::Limited,
        }
    }
}

impl AccessOverrideModalState {
    /// Create a modal preloaded with candidate contacts.
    #[must_use]
    pub fn new(contacts: Vec<Contact>) -> Self {
        Self {
            contacts,
            selected_index: 0,
            access_level: AccessLevel::Limited,
        }
    }

    /// Toggle bounded override level (Limited <-> Partial).
    pub fn toggle_access_level(&mut self) {
        self.access_level = match self.access_level {
            AccessLevel::Limited => AccessLevel::Partial,
            _ => AccessLevel::Limited,
        };
    }

    /// Return currently selected contact ID.
    #[must_use]
    pub fn selected_contact_id(&self) -> Option<&str> {
        self.contacts
            .get(self.selected_index)
            .map(|contact| contact.id.as_str())
    }
}

/// State for per-home Full/Partial/Limited capability configuration.
#[derive(Clone, Debug)]
pub struct HomeCapabilityConfigModalState {
    /// Comma-separated Full capabilities.
    pub full_caps: String,
    /// Comma-separated Partial capabilities.
    pub partial_caps: String,
    /// Comma-separated Limited capabilities.
    pub limited_caps: String,
    /// Active field (0=Full, 1=Partial, 2=Limited).
    pub active_field: usize,
    /// Optional validation error.
    pub error: Option<String>,
}

impl Default for HomeCapabilityConfigModalState {
    fn default() -> Self {
        Self {
            full_caps: "send_dm,manage_channel,grant_moderator".to_string(),
            partial_caps: "send_dm,read_channels".to_string(),
            limited_caps: "send_dm".to_string(),
            active_field: 0,
            error: None,
        }
    }
}

impl HomeCapabilityConfigModalState {
    /// Advance to the next editable capability level.
    pub fn next_field(&mut self) {
        self.active_field = (self.active_field + 1) % 3;
    }

    /// Append a typed character to the active field.
    pub fn push_char(&mut self, c: char) {
        match self.active_field {
            0 => self.full_caps.push(c),
            1 => self.partial_caps.push(c),
            _ => self.limited_caps.push(c),
        }
        self.error = None;
    }

    /// Delete one character from the active field.
    pub fn pop_char(&mut self) {
        match self.active_field {
            0 => {
                self.full_caps.pop();
            }
            1 => {
                self.partial_caps.pop();
            }
            _ => {
                self.limited_caps.pop();
            }
        }
        self.error = None;
    }

    /// Return true if all capability fields have content.
    #[must_use]
    pub fn can_submit(&self) -> bool {
        !(self.full_caps.trim().is_empty()
            || self.partial_caps.trim().is_empty()
            || self.limited_caps.trim().is_empty())
    }
}

// ============================================================================
// Form Data Types with Validation
// ============================================================================

/// Form data for home creation (portable, validatable)
#[derive(Clone, Debug, Default)]
pub struct HomeFormData {
    /// Home name (required)
    pub name: String,
    /// Optional description
    pub description: String,
}

impl Validatable for HomeFormData {
    fn validate(&self) -> Vec<ValidationError> {
        let mut errors = vec![];
        if self.name.trim().is_empty() {
            errors.push(ValidationError::required("name"));
        } else if self.name.len() > 100 {
            errors.push(ValidationError::too_long("name", 100));
        }
        if self.description.len() > 500 {
            errors.push(ValidationError::too_long("description", 500));
        }
        errors
    }
}
