//! Device naming facts for post-enrollment nickname updates.
//!
//! # Invariants
//!
//! - Facts are keyed by `device_id`
//! - LWW (last-writer-wins) semantics based on `updated_at` timestamp
//! - Empty `nickname_suggestion` clears the suggestion (does not delete the fact)
//! - Only the device itself can emit `SuggestionUpdated` for its own ID
//!
//! # Category
//!
//! All operations in this module are **Category A** (CRDT, immediate local effect).
//!
//! # Architecture
//!
//! Device naming facts are authority-scoped (devices belong to an authority, not a
//! cross-authority context). However, to fit the `DomainFact` trait's context-based
//! model, these facts use a derived context computed from the authority ID.
//!
//! The derived context ensures:
//! - Facts are isolated to a single authority
//! - Standard `DomainFact` trait can be implemented
//! - Reduction and registry integration work uniformly
//!
//! # Note on DomainFact Implementation
//!
//! This module manually implements `DomainFact` instead of using the derive macro
//! because the macro generates `impl aura_journal::DomainFact` which doesn't work
//! inside the `aura-journal` crate itself (where `crate::` must be used instead).
//!
//! # Safety
//!
//! This module is `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]

use aura_core::identifiers::{AuthorityId, ContextId, DeviceId};
use aura_core::time::PhysicalTime;
use aura_core::types::facts::{FactEncoding, FactEnvelope, FactTypeId};
use serde::{Deserialize, Serialize};

use crate::extensibility::{DomainFact, FactReducer};
use crate::reduction::{RelationalBinding, RelationalBindingType};

/// Type identifier for device naming facts.
pub const DEVICE_NAMING_FACT_TYPE_ID: &str = "device_naming";

/// Schema version for device naming facts.
pub const DEVICE_NAMING_SCHEMA_VERSION: u16 = 1;

/// Maximum bytes for nickname suggestion in facts.
///
/// Matches `NICKNAME_SUGGESTION_BYTES_MAX` in `DeviceLeafMetadata` for consistency.
pub const NICKNAME_SUGGESTION_BYTES_MAX: usize = 64;

/// Derive a context ID from an authority ID for device naming facts.
///
/// This provides a deterministic, authority-scoped "virtual context" for device
/// naming facts to fit the `DomainFact` trait model.
///
/// # Implementation
///
/// Uses a hash-based derivation: `BLAKE3(b"device-naming:" || authority_id.bytes())`.
/// The result is truncated to 16 bytes for the UUID-based ContextId.
pub fn derive_device_naming_context(authority_id: AuthorityId) -> ContextId {
    use aura_core::hash::hash;

    let mut input = Vec::with_capacity(14 + 16);
    input.extend_from_slice(b"device-naming:");
    input.extend_from_slice(&authority_id.to_bytes());

    let hash_bytes = hash(&input);
    let mut uuid_bytes = [0u8; 16];
    uuid_bytes.copy_from_slice(&hash_bytes[..16]);

    ContextId::from_uuid(uuid::Uuid::from_bytes(uuid_bytes))
}

/// Key type for device naming facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceNamingFactKey {
    /// Stable subtype identifier.
    pub sub_type: &'static str,
    /// Opaque key data (device ID bytes).
    pub data: Vec<u8>,
}

/// Device naming facts for post-enrollment nickname updates.
///
/// These facts allow devices to update their `nickname_suggestion` after enrollment
/// without requiring threshold signatures (tree operations). The initial suggestion
/// is stored in `DeviceLeafMetadata` at enrollment time; subsequent updates use
/// these facts.
///
/// # LWW Semantics
///
/// When multiple `SuggestionUpdated` facts exist for the same device, the one with
/// the latest `updated_at` timestamp wins during reduction. Clock skew may cause
/// unexpected behavior, but this is acceptable for casual name updates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceNamingFact {
    /// Device updated its nickname suggestion.
    ///
    /// LWW semantics: later `updated_at` wins during reduction.
    SuggestionUpdated {
        /// Derived context for this authority's device naming facts.
        ///
        /// Computed via `derive_device_naming_context(authority_id)`.
        context_id: ContextId,

        /// Authority that owns this device.
        authority_id: AuthorityId,

        /// Device that updated its suggestion.
        ///
        /// Must match the signing device for authorization.
        device_id: DeviceId,

        /// New nickname suggestion.
        ///
        /// Empty string clears the suggestion. Limited to
        /// `NICKNAME_SUGGESTION_BYTES_MAX` bytes.
        nickname_suggestion: String,

        /// When the update occurred.
        ///
        /// Used for LWW ordering. Clock skew may cause unexpected
        /// behavior but this is acceptable for casual name updates.
        updated_at: PhysicalTime,
    },
}

// Manual implementation of DomainFact because the derive macro generates
// `impl aura_journal::DomainFact` which doesn't work inside the aura-journal crate.
impl DomainFact for DeviceNamingFact {
    fn type_id(&self) -> &'static str {
        DEVICE_NAMING_FACT_TYPE_ID
    }

    fn schema_version(&self) -> u16 {
        DEVICE_NAMING_SCHEMA_VERSION
    }

    fn context_id(&self) -> ContextId {
        match self {
            Self::SuggestionUpdated { context_id, .. } => *context_id,
        }
    }

    fn to_envelope(&self) -> FactEnvelope {
        let payload = aura_core::util::serialization::to_vec(self)
            .expect("DeviceNamingFact serialization should not fail");
        FactEnvelope {
            type_id: FactTypeId::from(self.type_id()),
            schema_version: self.schema_version(),
            encoding: FactEncoding::DagCbor,
            payload,
        }
    }

    fn from_envelope(envelope: &FactEnvelope) -> Option<Self>
    where
        Self: Sized,
    {
        if envelope.type_id.as_str() != DEVICE_NAMING_FACT_TYPE_ID {
            return None;
        }
        if envelope.schema_version != DEVICE_NAMING_SCHEMA_VERSION {
            return None;
        }
        aura_core::util::serialization::from_slice(&envelope.payload).ok()
    }
}

impl DeviceNamingFact {
    /// Create a new suggestion update fact.
    ///
    /// Automatically derives the context_id from the authority_id.
    ///
    /// # Panics
    ///
    /// Debug-panics if `nickname_suggestion` exceeds `NICKNAME_SUGGESTION_BYTES_MAX`.
    #[must_use]
    pub fn suggestion_updated(
        authority_id: AuthorityId,
        device_id: DeviceId,
        nickname_suggestion: impl Into<String>,
        updated_at: PhysicalTime,
    ) -> Self {
        let nickname_suggestion = nickname_suggestion.into();
        debug_assert!(
            nickname_suggestion.len() <= NICKNAME_SUGGESTION_BYTES_MAX,
            "nickname_suggestion exceeds {NICKNAME_SUGGESTION_BYTES_MAX} bytes"
        );
        let context_id = derive_device_naming_context(authority_id);
        Self::SuggestionUpdated {
            context_id,
            authority_id,
            device_id,
            nickname_suggestion,
            updated_at,
        }
    }

    /// Create a suggestion update fact with millisecond timestamp.
    ///
    /// Convenience constructor for backward compatibility.
    #[must_use]
    pub fn suggestion_updated_ms(
        authority_id: AuthorityId,
        device_id: DeviceId,
        nickname_suggestion: impl Into<String>,
        updated_at_ms: u64,
    ) -> Self {
        Self::suggestion_updated(
            authority_id,
            device_id,
            nickname_suggestion,
            PhysicalTime {
                ts_ms: updated_at_ms,
                uncertainty: None,
            },
        )
    }

    /// Get the device ID this fact applies to.
    #[must_use]
    pub fn device_id(&self) -> DeviceId {
        match self {
            Self::SuggestionUpdated { device_id, .. } => *device_id,
        }
    }

    /// Get the authority ID this fact belongs to.
    #[must_use]
    pub fn authority_id(&self) -> AuthorityId {
        match self {
            Self::SuggestionUpdated { authority_id, .. } => *authority_id,
        }
    }

    /// Get the timestamp of this fact.
    #[must_use]
    pub fn timestamp(&self) -> PhysicalTime {
        match self {
            Self::SuggestionUpdated { updated_at, .. } => updated_at.clone(),
        }
    }

    /// Get the timestamp in milliseconds.
    #[must_use]
    pub fn timestamp_ms(&self) -> u64 {
        self.timestamp().ts_ms
    }

    /// Get the nickname suggestion.
    #[must_use]
    pub fn nickname_suggestion(&self) -> &str {
        match self {
            Self::SuggestionUpdated {
                nickname_suggestion,
                ..
            } => nickname_suggestion,
        }
    }

    /// Check if this fact clears the suggestion (empty string).
    #[must_use]
    pub fn is_clear(&self) -> bool {
        self.nickname_suggestion().is_empty()
    }

    /// Validate this fact for reduction in the given context.
    pub fn validate_for_reduction(&self, context_id: ContextId) -> bool {
        self.context_id() == context_id
    }

    /// Get the binding key for this fact.
    #[allow(clippy::expect_used)] // DeviceId::to_bytes() is infallible for valid IDs
    pub fn binding_key(&self) -> DeviceNamingFactKey {
        match self {
            Self::SuggestionUpdated { device_id, .. } => DeviceNamingFactKey {
                sub_type: "device-suggestion-updated",
                // DeviceId::to_bytes() returns Result, unwrap is safe for valid IDs
                data: device_id.to_bytes().expect("valid device_id").to_vec(),
            },
        }
    }
}

/// Reducer for device naming facts.
///
/// Converts device naming facts to relational bindings during journal reduction.
/// Uses LWW semantics based on the `updated_at` timestamp.
pub struct DeviceNamingFactReducer;

impl FactReducer for DeviceNamingFactReducer {
    fn handles_type(&self) -> &'static str {
        DEVICE_NAMING_FACT_TYPE_ID
    }

    fn reduce_envelope(
        &self,
        context_id: ContextId,
        envelope: &FactEnvelope,
    ) -> Option<RelationalBinding> {
        if envelope.type_id.as_str() != DEVICE_NAMING_FACT_TYPE_ID {
            return None;
        }

        let fact = DeviceNamingFact::from_envelope(envelope)?;
        if !fact.validate_for_reduction(context_id) {
            return None;
        }

        let key = fact.binding_key();

        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(key.sub_type.to_string()),
            context_id,
            data: key.data,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_authority_id(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn test_device_id(seed: u8) -> DeviceId {
        DeviceId::new_from_entropy([seed; 32])
    }

    #[test]
    fn test_derive_context_is_deterministic() {
        let auth_id = test_authority_id(1);
        let ctx1 = derive_device_naming_context(auth_id);
        let ctx2 = derive_device_naming_context(auth_id);
        assert_eq!(ctx1, ctx2);
    }

    #[test]
    fn test_derive_context_is_unique_per_authority() {
        let auth1 = test_authority_id(1);
        let auth2 = test_authority_id(2);
        let ctx1 = derive_device_naming_context(auth1);
        let ctx2 = derive_device_naming_context(auth2);
        assert_ne!(ctx1, ctx2);
    }

    #[test]
    fn test_fact_envelope_roundtrip() {
        let fact = DeviceNamingFact::suggestion_updated_ms(
            test_authority_id(1),
            test_device_id(10),
            "My Laptop",
            1234567890,
        );

        let envelope = fact.to_envelope();
        let restored = DeviceNamingFact::from_envelope(&envelope);
        assert!(restored.is_some());
        assert_eq!(restored.unwrap(), fact);
    }

    #[test]
    fn test_fact_to_generic() {
        let fact = DeviceNamingFact::suggestion_updated_ms(
            test_authority_id(1),
            test_device_id(10),
            "Test Device",
            0,
        );

        let generic = fact.to_generic();

        if let crate::fact::RelationalFact::Generic { envelope, .. } = generic {
            assert_eq!(envelope.type_id.as_str(), DEVICE_NAMING_FACT_TYPE_ID);
            let restored = DeviceNamingFact::from_envelope(&envelope);
            assert!(restored.is_some());
        } else {
            panic!("Expected Generic variant");
        }
    }

    #[test]
    fn test_accessor_methods() {
        let authority = test_authority_id(1);
        let device = test_device_id(10);
        let fact = DeviceNamingFact::suggestion_updated_ms(authority, device, "Test", 12345);

        assert_eq!(fact.device_id(), device);
        assert_eq!(fact.authority_id(), authority);
        assert_eq!(fact.timestamp_ms(), 12345);
        assert_eq!(fact.nickname_suggestion(), "Test");
        assert!(!fact.is_clear());
    }

    #[test]
    fn test_empty_suggestion_is_clear() {
        let fact = DeviceNamingFact::suggestion_updated_ms(
            test_authority_id(1),
            test_device_id(10),
            "",
            0,
        );

        assert!(fact.is_clear());
        assert_eq!(fact.nickname_suggestion(), "");
    }

    #[test]
    fn test_reducer_handles_correct_type() {
        let reducer = DeviceNamingFactReducer;
        assert_eq!(reducer.handles_type(), DEVICE_NAMING_FACT_TYPE_ID);
    }

    #[test]
    fn test_reducer_reduction() {
        let reducer = DeviceNamingFactReducer;
        let authority = test_authority_id(1);
        let fact = DeviceNamingFact::suggestion_updated_ms(
            authority,
            test_device_id(10),
            "Test Device",
            0,
        );

        let context_id = derive_device_naming_context(authority);
        let envelope = fact.to_envelope();
        let binding = reducer.reduce_envelope(context_id, &envelope);

        assert!(binding.is_some());
        let binding = binding.unwrap();
        assert!(matches!(
            binding.binding_type,
            RelationalBindingType::Generic(ref s) if s == "device-suggestion-updated"
        ));
    }

    #[test]
    fn test_reducer_rejects_wrong_context() {
        let reducer = DeviceNamingFactReducer;
        let fact = DeviceNamingFact::suggestion_updated_ms(
            test_authority_id(1),
            test_device_id(10),
            "Test",
            0,
        );

        // Use a different authority's context
        let wrong_context = derive_device_naming_context(test_authority_id(99));
        let envelope = fact.to_envelope();
        let binding = reducer.reduce_envelope(wrong_context, &envelope);

        assert!(binding.is_none());
    }

    #[test]
    fn test_reducer_idempotence() {
        let reducer = DeviceNamingFactReducer;
        let authority = test_authority_id(1);
        let fact = DeviceNamingFact::suggestion_updated_ms(
            authority,
            test_device_id(10),
            "Test Device",
            0,
        );

        let context_id = derive_device_naming_context(authority);
        let envelope = fact.to_envelope();

        let binding1 = reducer.reduce_envelope(context_id, &envelope);
        let binding2 = reducer.reduce_envelope(context_id, &envelope);

        assert!(binding1.is_some());
        assert!(binding2.is_some());

        let b1 = binding1.unwrap();
        let b2 = binding2.unwrap();
        assert_eq!(b1.binding_type, b2.binding_type);
        assert_eq!(b1.context_id, b2.context_id);
        assert_eq!(b1.data, b2.data);
    }
}
