//! # Contacts View State

use aura_core::identifiers::AuthorityId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Re-export ReadReceiptPolicy from aura-relational for convenience
pub use aura_relational::ReadReceiptPolicy;

// =============================================================================
// Error Types
// =============================================================================

/// Error type for contact operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContactError {
    /// The specified contact was not found
    ContactNotFound(AuthorityId),
    /// Contact already exists (for add operations)
    ContactAlreadyExists(AuthorityId),
}

impl std::fmt::Display for ContactError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ContactNotFound(id) => write!(f, "Contact not found: {id}"),
            Self::ContactAlreadyExists(id) => write!(f, "Contact already exists: {id}"),
        }
    }
}

impl std::error::Error for ContactError {}

// =============================================================================
// Contact Update Types
// =============================================================================

/// Update operations for contacts (explicit mutations).
#[derive(Debug, Clone)]
pub enum ContactUpdate {
    /// Set the nickname
    SetNickname(String),
    /// Set the suggested name
    SetSuggestedName(Option<String>),
    /// Set guardian status
    SetGuardian(bool),
    /// Set resident status
    SetResident(bool),
    /// Update last interaction time
    SetLastInteraction(Option<u64>),
    /// Set online status
    SetOnline(bool),
    /// Set read receipt policy
    SetReadReceiptPolicy(ReadReceiptPolicy),
}

// =============================================================================
// Contact Suggestion Types
// =============================================================================

/// Policy for handling incoming contact name suggestions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum SuggestionPolicy {
    /// Automatically accept contact suggestions
    #[default]
    AutoAccept,
    /// Prompt the user before accepting suggestions
    PromptFirst,
    /// Ignore incoming suggestions
    Ignore,
}

/// What the user shares about themselves to contacts
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct MySuggestion {
    /// Display name to share
    pub display_name: Option<String>,
    /// Status message to share
    pub status: Option<String>,
}

// =============================================================================
// Contact Types
// =============================================================================

/// A contact
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Contact {
    /// Contact identifier (authority ID)
    pub id: AuthorityId,
    /// Nickname (user-assigned name)
    pub nickname: String,
    /// Suggested name (from contact or system)
    pub suggested_name: Option<String>,
    /// Whether this contact is a guardian
    pub is_guardian: bool,
    /// Whether this contact is a home resident
    pub is_resident: bool,
    /// Last interaction time (ms since epoch)
    pub last_interaction: Option<u64>,
    /// Whether contact is online
    pub is_online: bool,
    /// Read receipt policy for this contact (privacy-first: disabled by default)
    pub read_receipt_policy: ReadReceiptPolicy,
}

/// Contacts state
///
/// Domain state for contacts. UI concerns like selection and search filter
/// are maintained separately in the TUI layer.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ContactsState {
    /// All contacts, keyed by authority ID (private)
    #[serde(with = "contacts_serde")]
    contacts: HashMap<AuthorityId, Contact>,
}

// Custom serde for HashMap<AuthorityId, Contact> to maintain compatibility
mod contacts_serde {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(
        contacts: &HashMap<AuthorityId, Contact>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Serialize as Vec for backward compatibility
        let vec: Vec<&Contact> = contacts.values().collect();
        vec.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<HashMap<AuthorityId, Contact>, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Deserialize from Vec for backward compatibility
        let vec: Vec<Contact> = Vec::deserialize(deserializer)?;
        Ok(vec.into_iter().map(|c| (c.id, c)).collect())
    }
}

impl ContactsState {
    // =========================================================================
    // Constructors
    // =========================================================================

    /// Create a new empty ContactsState.
    pub fn new() -> Self {
        Self {
            contacts: HashMap::new(),
        }
    }

    /// Create ContactsState from an iterator of contacts.
    pub fn from_iter(contacts: impl IntoIterator<Item = Contact>) -> Self {
        Self {
            contacts: contacts.into_iter().map(|c| (c.id, c)).collect(),
        }
    }

    /// Create ContactsState from a HashMap (factory method).
    pub fn from_parts(contacts: HashMap<AuthorityId, Contact>) -> Self {
        Self { contacts }
    }

    // =========================================================================
    // Accessors (Query Methods)
    // =========================================================================

    /// Get contact by ID.
    pub fn contact(&self, id: &AuthorityId) -> Option<&Contact> {
        self.contacts.get(id)
    }

    /// Get mutable reference to contact by ID.
    pub fn contact_mut(&mut self, id: &AuthorityId) -> Option<&mut Contact> {
        self.contacts.get_mut(id)
    }

    /// Check if a contact exists.
    pub fn has_contact(&self, id: &AuthorityId) -> bool {
        self.contacts.contains_key(id)
    }

    /// Get all contacts as an iterator.
    pub fn all_contacts(&self) -> impl Iterator<Item = &Contact> {
        self.contacts.values()
    }

    /// Get contact count.
    pub fn contact_count(&self) -> usize {
        self.contacts.len()
    }

    /// Check if there are no contacts.
    pub fn is_empty(&self) -> bool {
        self.contacts.is_empty()
    }

    /// Get contacts matching a search filter.
    ///
    /// Matches against nickname and suggested_name (case-insensitive).
    pub fn contacts_matching(&self, filter: &str) -> Vec<&Contact> {
        if filter.is_empty() {
            return self.contacts.values().collect();
        }
        let filter_lower = filter.to_lowercase();
        self.contacts
            .values()
            .filter(|c| {
                c.nickname.to_lowercase().contains(&filter_lower)
                    || c.suggested_name
                        .as_ref()
                        .map(|n| n.to_lowercase().contains(&filter_lower))
                        .unwrap_or(false)
            })
            .collect()
    }

    /// Get guardian contacts.
    pub fn guardians(&self) -> Vec<&Contact> {
        self.contacts.values().filter(|c| c.is_guardian).collect()
    }

    /// Get resident contacts.
    pub fn residents(&self) -> Vec<&Contact> {
        self.contacts.values().filter(|c| c.is_resident).collect()
    }

    /// Get display name for a contact.
    ///
    /// Returns nickname if set, otherwise suggested_name, otherwise the ID as fallback.
    pub fn get_display_name(&self, id: &AuthorityId) -> String {
        if let Some(contact) = self.contact(id) {
            if !contact.nickname.is_empty() {
                return contact.nickname.clone();
            }
            if let Some(name) = &contact.suggested_name {
                return name.clone();
            }
        }
        id.to_string()
    }

    /// Get read receipt policy for a contact.
    ///
    /// Returns Disabled (privacy-first default) if contact not found.
    pub fn get_read_receipt_policy(&self, contact_id: &AuthorityId) -> ReadReceiptPolicy {
        self.contact(contact_id)
            .map(|c| c.read_receipt_policy)
            .unwrap_or_default()
    }

    // =========================================================================
    // Mutation Methods
    // =========================================================================

    /// Apply a contact (upsert - insert or replace).
    ///
    /// This is an idempotent operation suitable for CRDT-style updates.
    pub fn apply_contact(&mut self, contact: Contact) {
        self.contacts.insert(contact.id, contact);
    }

    /// Add a new contact (fails if already exists).
    pub fn add_contact(&mut self, contact: Contact) -> Result<(), ContactError> {
        if self.contacts.contains_key(&contact.id) {
            return Err(ContactError::ContactAlreadyExists(contact.id));
        }
        self.contacts.insert(contact.id, contact);
        Ok(())
    }

    /// Update a contact with a specific update operation.
    pub fn update_contact(
        &mut self,
        id: &AuthorityId,
        update: ContactUpdate,
    ) -> Result<(), ContactError> {
        let contact = self
            .contacts
            .get_mut(id)
            .ok_or_else(|| ContactError::ContactNotFound(*id))?;

        match update {
            ContactUpdate::SetNickname(nickname) => contact.nickname = nickname,
            ContactUpdate::SetSuggestedName(name) => contact.suggested_name = name,
            ContactUpdate::SetGuardian(is_guardian) => contact.is_guardian = is_guardian,
            ContactUpdate::SetResident(is_resident) => contact.is_resident = is_resident,
            ContactUpdate::SetLastInteraction(time) => contact.last_interaction = time,
            ContactUpdate::SetOnline(is_online) => contact.is_online = is_online,
            ContactUpdate::SetReadReceiptPolicy(policy) => contact.read_receipt_policy = policy,
        }
        Ok(())
    }

    /// Remove a contact.
    pub fn remove_contact(&mut self, id: &AuthorityId) -> Result<Contact, ContactError> {
        self.contacts
            .remove(id)
            .ok_or_else(|| ContactError::ContactNotFound(*id))
    }

    /// Retain only contacts matching a predicate.
    pub fn retain(&mut self, f: impl FnMut(&AuthorityId, &mut Contact) -> bool) {
        self.contacts.retain(f);
    }

    /// Clear all contacts.
    pub fn clear(&mut self) {
        self.contacts.clear();
    }

    // =========================================================================
    // Legacy Methods (Deprecated - Backward Compatibility)
    // =========================================================================

    /// Set guardian status for a contact.
    ///
    /// Updates the is_guardian flag on the contact if it exists.
    #[deprecated(since = "0.2.0", note = "Use update_contact with ContactUpdate::SetGuardian")]
    pub fn set_guardian_status(&mut self, contact_id: AuthorityId, is_guardian: bool) {
        if let Some(contact) = self.contacts.get_mut(&contact_id) {
            contact.is_guardian = is_guardian;
        }
    }

    /// Set nickname for a contact.
    ///
    /// If the contact doesn't exist, creates a new contact entry.
    #[deprecated(since = "0.2.0", note = "Use update_contact or apply_contact instead")]
    pub fn set_nickname(&mut self, target: AuthorityId, nickname: String) {
        if let Some(contact) = self.contacts.get_mut(&target) {
            contact.nickname = nickname;
        } else {
            // Create new contact with the nickname
            self.contacts.insert(
                target,
                Contact {
                    id: target,
                    nickname,
                    suggested_name: None,
                    is_guardian: false,
                    is_resident: false,
                    last_interaction: None,
                    is_online: false,
                    read_receipt_policy: ReadReceiptPolicy::default(),
                },
            );
        }
    }

    /// Set read receipt policy for a contact.
    ///
    /// If the contact doesn't exist, this is a no-op.
    #[deprecated(since = "0.2.0", note = "Use update_contact with ContactUpdate::SetReadReceiptPolicy")]
    pub fn set_read_receipt_policy(&mut self, contact_id: AuthorityId, policy: ReadReceiptPolicy) {
        if let Some(contact) = self.contacts.get_mut(&contact_id) {
            contact.read_receipt_policy = policy;
        }
    }
}
