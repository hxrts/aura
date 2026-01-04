//! # Guardian Candidates
//!
//! Type-safe guardian candidate set ensuring non-empty contact list.

use aura_core::identifiers::AuthorityId;
use std::fmt;

/// Error when constructing guardian candidates
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardianSetupError {
    /// No contacts available to select as guardians
    NoContacts,
    /// Not enough contacts for the requested threshold
    InsufficientContacts {
        /// Number of contacts required
        required: usize,
        /// Number of contacts available
        available: usize,
    },
}

impl fmt::Display for GuardianSetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GuardianSetupError::NoContacts => {
                write!(f, "Add contacts first before setting up guardians")
            }
            GuardianSetupError::InsufficientContacts {
                required,
                available,
            } => {
                write!(
                    f,
                    "Need at least {required} contacts for this threshold, but only {available} available"
                )
            }
        }
    }
}

impl std::error::Error for GuardianSetupError {}

/// A non-empty set of contacts eligible to become guardians
///
/// Invariants:
/// - At least one contact must be present
///
/// # Example
///
/// ```rust,ignore
/// let contacts = vec![contact1, contact2, contact3];
/// let candidates = GuardianCandidates::from_contacts(contacts)?;
///
/// // Can now safely open guardian setup modal
/// open_modal(candidates);
/// ```
#[derive(Debug, Clone)]
pub struct GuardianCandidates {
    contacts: Vec<AuthorityId>,
}

impl GuardianCandidates {
    /// Create guardian candidates from a list of contact IDs
    ///
    /// Returns an error if the contact list is empty.
    pub fn from_contacts(contacts: Vec<AuthorityId>) -> Result<Self, GuardianSetupError> {
        if contacts.is_empty() {
            return Err(GuardianSetupError::NoContacts);
        }
        Ok(Self { contacts })
    }

    /// Check if there are enough contacts for a given threshold
    ///
    /// Returns an error if there aren't enough contacts to satisfy
    /// the n value of the threshold.
    pub fn validate_for_threshold(
        &self,
        threshold_n: u8,
    ) -> Result<&Self, GuardianSetupError> {
        let required = threshold_n as usize;
        let available = self.contacts.len();
        if available < required {
            return Err(GuardianSetupError::InsufficientContacts {
                required,
                available,
            });
        }
        Ok(self)
    }

    /// Get the number of available candidates
    pub fn count(&self) -> usize {
        self.contacts.len()
    }

    /// Get the contact IDs
    pub fn contacts(&self) -> &[AuthorityId] {
        &self.contacts
    }

    /// Consume and return the inner contact list
    pub fn into_contacts(self) -> Vec<AuthorityId> {
        self.contacts
    }

    /// Get maximum possible n value for threshold
    pub fn max_threshold_n(&self) -> u8 {
        self.contacts.len().min(255) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn make_authority() -> AuthorityId {
        AuthorityId::from_uuid(Uuid::new_v4())
    }

    #[test]
    fn test_from_contacts_empty() {
        let result = GuardianCandidates::from_contacts(vec![]);
        assert_eq!(result.unwrap_err(), GuardianSetupError::NoContacts);
    }

    #[test]
    fn test_from_contacts_non_empty() {
        let contacts = vec![make_authority(), make_authority()];
        let candidates = GuardianCandidates::from_contacts(contacts).unwrap();
        assert_eq!(candidates.count(), 2);
    }

    #[test]
    fn test_validate_for_threshold() {
        let contacts = vec![make_authority(), make_authority(), make_authority()];
        let candidates = GuardianCandidates::from_contacts(contacts).unwrap();

        // Valid: 2-of-3 with 3 contacts
        assert!(candidates.validate_for_threshold(3).is_ok());

        // Invalid: need 5 but only have 3
        assert_eq!(
            candidates.validate_for_threshold(5).unwrap_err(),
            GuardianSetupError::InsufficientContacts {
                required: 5,
                available: 3
            }
        );
    }
}
