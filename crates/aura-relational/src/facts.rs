//! Contact and relationship domain facts
//!
//! This module defines contact-specific fact types that implement the `DomainFact`
//! trait from `aura-journal`. These facts are stored as `RelationalFact::Generic`
//! in the journal and reduced using the `ContactFactReducer`.
//!
//! # Architecture
//!
//! Following the Open/Closed Principle:
//! - `aura-journal` provides the generic fact infrastructure
//! - `aura-relational` defines domain-specific fact types without modifying `aura-journal`
//! - Runtime registers `ContactFactReducer` with the `FactRegistry`
//!
//! # Example
//!
//! ```ignore
//! use aura_relational::facts::{ContactFact, ContactFactReducer};
//! use aura_journal::{FactRegistry, DomainFact};
//!
//! // Create a contact fact
//! let fact = ContactFact::Added {
//!     context_id,
//!     owner_id,
//!     contact_id,
//!     petname: "Alice".to_string(),
//!     added_at_ms: 1234567890,
//! };
//!
//! // Convert to generic for storage
//! let generic = fact.to_generic();
//!
//! // Register reducer at runtime
//! registry.register::<ContactFact>("contact", Box::new(ContactFactReducer));
//! ```

use aura_core::identifiers::{AuthorityId, ContextId};
use aura_journal::{
    reduction::{RelationalBinding, RelationalBindingType},
    DomainFact, FactReducer,
};
use serde::{Deserialize, Serialize};

/// Type identifier for contact facts
pub const CONTACT_FACT_TYPE_ID: &str = "contact";

/// Contact domain fact types
///
/// These facts represent contact-related state changes in the journal.
/// They are stored as `RelationalFact::Generic` and reduced by `ContactFactReducer`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ContactFact {
    /// Contact added to authority's contact list
    Added {
        /// Relational context for the contact relationship
        context_id: ContextId,
        /// Authority adding the contact (owner)
        owner_id: AuthorityId,
        /// Authority being added as a contact
        contact_id: AuthorityId,
        /// User-assigned petname for the contact
        petname: String,
        /// Timestamp when contact was added (ms since epoch)
        added_at_ms: u64,
    },
    /// Contact removed from authority's contact list
    Removed {
        /// Relational context for the contact relationship
        context_id: ContextId,
        /// Authority removing the contact (owner)
        owner_id: AuthorityId,
        /// Authority being removed as a contact
        contact_id: AuthorityId,
        /// Timestamp when contact was removed (ms since epoch)
        removed_at_ms: u64,
    },
    /// Contact petname updated
    Renamed {
        /// Relational context for the contact relationship
        context_id: ContextId,
        /// Authority owning the contact list
        owner_id: AuthorityId,
        /// Contact being renamed
        contact_id: AuthorityId,
        /// New petname for the contact
        new_petname: String,
        /// Timestamp when contact was renamed (ms since epoch)
        renamed_at_ms: u64,
    },
}

impl ContactFact {
    /// Get the contact_id from any variant
    pub fn contact_id(&self) -> AuthorityId {
        match self {
            ContactFact::Added { contact_id, .. } => *contact_id,
            ContactFact::Removed { contact_id, .. } => *contact_id,
            ContactFact::Renamed { contact_id, .. } => *contact_id,
        }
    }

    /// Get the owner_id from any variant
    pub fn owner_id(&self) -> AuthorityId {
        match self {
            ContactFact::Added { owner_id, .. } => *owner_id,
            ContactFact::Removed { owner_id, .. } => *owner_id,
            ContactFact::Renamed { owner_id, .. } => *owner_id,
        }
    }
}

impl DomainFact for ContactFact {
    fn type_id(&self) -> &'static str {
        CONTACT_FACT_TYPE_ID
    }

    fn context_id(&self) -> ContextId {
        match self {
            ContactFact::Added { context_id, .. } => *context_id,
            ContactFact::Removed { context_id, .. } => *context_id,
            ContactFact::Renamed { context_id, .. } => *context_id,
        }
    }

    fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self>
    where
        Self: Sized,
    {
        serde_json::from_slice(bytes).ok()
    }
}

/// Reducer for contact facts
///
/// Converts contact facts to relational bindings during journal reduction.
pub struct ContactFactReducer;

impl FactReducer for ContactFactReducer {
    fn handles_type(&self) -> &'static str {
        CONTACT_FACT_TYPE_ID
    }

    fn reduce(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> Option<RelationalBinding> {
        if binding_type != CONTACT_FACT_TYPE_ID {
            return None;
        }

        let fact: ContactFact = serde_json::from_slice(binding_data).ok()?;

        let (sub_type, data) = match &fact {
            ContactFact::Added { contact_id, .. } => {
                ("contact-added".to_string(), contact_id.to_bytes().to_vec())
            }
            ContactFact::Removed { contact_id, .. } => (
                "contact-removed".to_string(),
                contact_id.to_bytes().to_vec(),
            ),
            ContactFact::Renamed { contact_id, .. } => (
                "contact-renamed".to_string(),
                contact_id.to_bytes().to_vec(),
            ),
        };

        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(sub_type),
            context_id,
            data,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_context_id() -> ContextId {
        ContextId::new_from_entropy([42u8; 32])
    }

    fn test_authority_id(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    #[test]
    fn test_contact_fact_serialization() {
        let fact = ContactFact::Added {
            context_id: test_context_id(),
            owner_id: test_authority_id(1),
            contact_id: test_authority_id(2),
            petname: "Alice".to_string(),
            added_at_ms: 1234567890,
        };

        let bytes = fact.to_bytes();
        let restored = ContactFact::from_bytes(&bytes);
        assert!(restored.is_some());
        assert_eq!(restored.unwrap(), fact);
    }

    #[test]
    fn test_contact_fact_to_generic() {
        let fact = ContactFact::Renamed {
            context_id: test_context_id(),
            owner_id: test_authority_id(1),
            contact_id: test_authority_id(2),
            new_petname: "Bob".to_string(),
            renamed_at_ms: 1234567899,
        };

        let generic = fact.to_generic();

        if let aura_journal::RelationalFact::Generic {
            binding_type,
            binding_data,
            ..
        } = generic
        {
            assert_eq!(binding_type, CONTACT_FACT_TYPE_ID);
            let restored = ContactFact::from_bytes(&binding_data);
            assert!(restored.is_some());
        } else {
            panic!("Expected Generic variant");
        }
    }

    #[test]
    fn test_contact_fact_reducer() {
        let reducer = ContactFactReducer;
        assert_eq!(reducer.handles_type(), CONTACT_FACT_TYPE_ID);

        let fact = ContactFact::Added {
            context_id: test_context_id(),
            owner_id: test_authority_id(1),
            contact_id: test_authority_id(2),
            petname: "Test".to_string(),
            added_at_ms: 0,
        };

        let bytes = fact.to_bytes();
        let binding = reducer.reduce(test_context_id(), CONTACT_FACT_TYPE_ID, &bytes);

        assert!(binding.is_some());
        let binding = binding.unwrap();
        assert!(matches!(
            binding.binding_type,
            RelationalBindingType::Generic(ref s) if s == "contact-added"
        ));
    }

    #[test]
    fn test_contact_id_extraction() {
        let contact = test_authority_id(99);

        let facts = vec![
            ContactFact::Added {
                context_id: test_context_id(),
                owner_id: test_authority_id(1),
                contact_id: contact,
                petname: "Alice".to_string(),
                added_at_ms: 0,
            },
            ContactFact::Removed {
                context_id: test_context_id(),
                owner_id: test_authority_id(1),
                contact_id: contact,
                removed_at_ms: 0,
            },
            ContactFact::Renamed {
                context_id: test_context_id(),
                owner_id: test_authority_id(1),
                contact_id: contact,
                new_petname: "Bob".to_string(),
                renamed_at_ms: 0,
            },
        ];

        for fact in facts {
            assert_eq!(fact.contact_id(), contact);
        }
    }

    #[test]
    fn test_type_id_consistency() {
        let facts: Vec<ContactFact> = vec![
            ContactFact::Added {
                context_id: test_context_id(),
                owner_id: test_authority_id(1),
                contact_id: test_authority_id(2),
                petname: "x".to_string(),
                added_at_ms: 0,
            },
            ContactFact::Removed {
                context_id: test_context_id(),
                owner_id: test_authority_id(1),
                contact_id: test_authority_id(2),
                removed_at_ms: 0,
            },
            ContactFact::Renamed {
                context_id: test_context_id(),
                owner_id: test_authority_id(1),
                contact_id: test_authority_id(2),
                new_petname: "y".to_string(),
                renamed_at_ms: 0,
            },
        ];

        for fact in facts {
            assert_eq!(fact.type_id(), CONTACT_FACT_TYPE_ID);
        }
    }
}
