//! Relational facts for cross-authority coordination
//!
//! This module defines the facts that can be stored in relational contexts
//! to coordinate relationships between authorities.
//!
//! # Design: Typed Storage (No Stringly-Typed Vec<u8>)
//!
//! `GenericBinding` stores a `FactEnvelope` directly rather than serialized bytes.
//! This eliminates:
//! - Double serialization when storing/transporting bindings
//! - Runtime type mismatches (envelope.type_id is validated at construction)
//! - Stringly-typed error patterns ("poison pill" bindings)
//!
//! # WASM Optimization
//!
//! When crossing Rust/JS boundaries, only `envelope.payload` is raw bytes.
//! The envelope metadata (type_id, schema_version) remains typed.

use crate::types::facts::{FactEnvelope, FactError, FactTypeId, MAX_FACT_PAYLOAD_BYTES};
use serde::{Deserialize, Serialize};

/// Maximum size for relational binding payload.
///
/// Aligned with `MAX_FACT_PAYLOAD_BYTES` to ensure consistency.
pub const MAX_RELATIONAL_BINDING_DATA_BYTES: usize = MAX_FACT_PAYLOAD_BYTES;

/// Facts that can be stored in relational contexts
///
/// These facts represent cross-authority relationships and operations
/// that require coordination between multiple authorities.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RelationalFact {
    /// Guardian binding between authorities
    GuardianBinding(super::guardian::GuardianBinding),
    /// Recovery grant approval
    RecoveryGrant(super::recovery::RecoveryGrant),
    /// Generic binding for extensibility
    Generic(GenericBinding),
}

/// Error type for generic binding operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum GenericBindingError {
    /// Payload exceeds size limit
    #[error("payload exceeds {max} bytes (got {size})")]
    PayloadTooLarge {
        /// Actual size in bytes
        size: u64,
        /// Maximum allowed size
        max: u64,
    },

    /// Underlying fact error
    #[error("fact error: {0}")]
    FactError(String),
}

impl From<FactError> for GenericBindingError {
    fn from(e: FactError) -> Self {
        match e {
            FactError::PayloadTooLarge { size, max } => {
                GenericBindingError::PayloadTooLarge { size, max }
            }
            other => GenericBindingError::FactError(other.to_string()),
        }
    }
}

/// Generic binding for application-specific relationships.
///
/// This type allows for extensible relational facts without modifying
/// the core `RelationalFact` enum. Applications can define their own
/// binding schemas and store them as generic bindings.
///
/// # Typed Storage
///
/// Unlike a stringly-typed `Vec<u8>` approach, `GenericBinding` stores
/// a `FactEnvelope` directly. This provides:
///
/// - **Type safety**: `envelope.type_id` is always valid and accessible
/// - **No double serialization**: Only `envelope.payload` is raw bytes
/// - **Validation at construction**: Size and type checks happen in `try_new`
///
/// # Example
///
/// ```ignore
/// use aura_core::relational::GenericBinding;
/// use aura_core::types::facts::FactEnvelope;
///
/// let envelope = FactEnvelope { /* ... */ };
/// let binding = GenericBinding::try_new(envelope)?;
///
/// // Type ID is directly accessible (no parsing)
/// assert_eq!(binding.type_id().as_str(), "chat/v1");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenericBinding {
    /// The typed fact envelope (contains type_id, schema_version, encoding, payload)
    envelope: FactEnvelope,
    /// Optional consensus proof if binding required agreement
    consensus_proof: Option<super::consensus::ConsensusProof>,
}

// Manual Ord implementation since FactEnvelope doesn't derive Ord
impl PartialOrd for GenericBinding {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for GenericBinding {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Compare by type_id first, then schema_version, then payload
        match self.envelope.type_id.as_str().cmp(other.envelope.type_id.as_str()) {
            std::cmp::Ordering::Equal => {}
            ord => return ord,
        }
        match self.envelope.schema_version.cmp(&other.envelope.schema_version) {
            std::cmp::Ordering::Equal => {}
            ord => return ord,
        }
        self.envelope.payload.cmp(&other.envelope.payload)
    }
}

impl GenericBinding {
    /// Create a new generic binding with validation.
    ///
    /// # Errors
    ///
    /// Returns error if payload exceeds `MAX_RELATIONAL_BINDING_DATA_BYTES`.
    pub fn try_new(envelope: FactEnvelope) -> Result<Self, GenericBindingError> {
        if envelope.payload.len() > MAX_RELATIONAL_BINDING_DATA_BYTES {
            return Err(GenericBindingError::PayloadTooLarge {
                size: envelope.payload.len() as u64,
                max: MAX_RELATIONAL_BINDING_DATA_BYTES as u64,
            });
        }

        Ok(Self {
            envelope,
            consensus_proof: None,
        })
    }

    /// Create a validated generic binding with consensus proof.
    ///
    /// # Errors
    ///
    /// Returns error if validation fails (see `try_new`).
    pub fn try_with_consensus_proof(
        envelope: FactEnvelope,
        consensus_proof: super::consensus::ConsensusProof,
    ) -> Result<Self, GenericBindingError> {
        let mut binding = Self::try_new(envelope)?;
        binding.consensus_proof = Some(consensus_proof);
        Ok(binding)
    }

    /// Get the fact type ID.
    ///
    /// This is directly accessible without parsing - no stringly-typed lookups.
    #[must_use]
    pub fn type_id(&self) -> &FactTypeId {
        &self.envelope.type_id
    }

    /// Get the binding type as a string (convenience method).
    ///
    /// Equivalent to `type_id().as_str()`.
    #[must_use]
    pub fn binding_type(&self) -> &str {
        self.envelope.type_id.as_str()
    }

    /// Get the schema version.
    #[must_use]
    pub fn schema_version(&self) -> u16 {
        self.envelope.schema_version
    }

    /// Get the full envelope (typed access).
    #[must_use]
    pub fn envelope(&self) -> &FactEnvelope {
        &self.envelope
    }

    /// Take ownership of the envelope.
    #[must_use]
    pub fn into_envelope(self) -> FactEnvelope {
        self.envelope
    }

    /// Get the raw payload bytes.
    ///
    /// For WASM/JS interop, this is the only `Vec<u8>` - no double serialization.
    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.envelope.payload
    }

    /// Take ownership of the payload bytes.
    ///
    /// For WASM/JS interop, avoids copying when consuming the binding.
    #[must_use]
    pub fn into_payload(self) -> Vec<u8> {
        self.envelope.payload
    }

    /// Check if this binding has consensus proof.
    #[must_use]
    pub fn has_consensus_proof(&self) -> bool {
        self.consensus_proof.is_some()
    }

    /// Get the consensus proof, if any.
    #[must_use]
    pub fn consensus_proof(&self) -> Option<&super::consensus::ConsensusProof> {
        self.consensus_proof.as_ref()
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::facts::{FactEncoding, FactTypeId};

    fn test_envelope(type_id: &str, payload: &[u8]) -> FactEnvelope {
        FactEnvelope {
            type_id: FactTypeId::from(type_id),
            schema_version: 1,
            encoding: FactEncoding::DagCbor,
            payload: payload.to_vec(),
        }
    }

    #[test]
    fn test_try_new_valid() {
        let envelope = test_envelope("test/v1", b"payload");
        let binding = GenericBinding::try_new(envelope).unwrap();

        assert_eq!(binding.type_id().as_str(), "test/v1");
        assert_eq!(binding.binding_type(), "test/v1");
        assert_eq!(binding.schema_version(), 1);
        assert_eq!(binding.payload(), b"payload");
        assert!(!binding.has_consensus_proof());
    }

    #[test]
    fn test_try_new_oversized() {
        let oversized = vec![0u8; MAX_RELATIONAL_BINDING_DATA_BYTES + 1];
        let envelope = test_envelope("test/v1", &oversized);
        let result = GenericBinding::try_new(envelope);

        assert!(matches!(
            result,
            Err(GenericBindingError::PayloadTooLarge { .. })
        ));
    }

    #[test]
    fn test_into_payload() {
        let envelope = test_envelope("test/v1", b"my payload");
        let binding = GenericBinding::try_new(envelope).unwrap();
        let payload = binding.into_payload();

        assert_eq!(payload, b"my payload");
    }

    #[test]
    fn test_envelope_access() {
        let envelope = test_envelope("chat/v2", b"chat data");
        let binding = GenericBinding::try_new(envelope.clone()).unwrap();

        assert_eq!(binding.envelope().type_id.as_str(), "chat/v2");
        assert_eq!(binding.envelope().schema_version, 1);

        let recovered = binding.into_envelope();
        assert_eq!(recovered.type_id.as_str(), "chat/v2");
    }

    #[test]
    fn test_serialization_roundtrip() {
        let envelope = test_envelope("test/v1", b"test data");
        let binding = GenericBinding::try_new(envelope).unwrap();

        // Serialize the binding
        let bytes = crate::util::serialization::to_vec(&binding).unwrap();

        // Deserialize
        let recovered: GenericBinding = crate::util::serialization::from_slice(&bytes).unwrap();

        assert_eq!(recovered.type_id().as_str(), "test/v1");
        assert_eq!(recovered.payload(), b"test data");
    }

    #[test]
    fn test_ordering() {
        let b1 = GenericBinding::try_new(test_envelope("aaa", b"data")).unwrap();
        let b2 = GenericBinding::try_new(test_envelope("bbb", b"data")).unwrap();
        let b3 = GenericBinding::try_new(test_envelope("aaa", b"other")).unwrap();

        assert!(b1 < b2); // aaa < bbb
        assert!(b1 < b3); // same type, data < other
    }
}
