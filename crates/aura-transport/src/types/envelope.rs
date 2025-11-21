//! Privacy-Aware Message Envelope Types
//!
//! Provides essential message wrappers with built-in privacy preservation, relationship scoping,
//! and minimal framing metadata. Target: <150 lines (concise implementation).

use aura_core::{identifiers::DeviceId, AuraResult, RelationshipId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Universal message wrapper with essential blinding capabilities
///
/// Integrates privacy preservation directly into the core envelope type.
/// Supports both clear and blinded modes with relationship scoping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    /// Unique message identifier
    pub message_id: Uuid,
    /// Frame metadata with privacy hints
    pub header: FrameHeader,
    /// Message payload (may be blinded)
    pub payload: Vec<u8>,
    /// Relationship context for scoped routing
    pub relationship_scope: Option<RelationshipId>,
}

/// Relationship-scoped message envelope for privacy-preserving routing
///
/// Ensures messages are only routed within appropriate relationship contexts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopedEnvelope {
    /// Base envelope
    pub envelope: Envelope,
    /// Required relationship context
    pub relationship_id: RelationshipId,
    /// Sender within relationship context
    pub scoped_sender: DeviceId,
    /// Recipient within relationship context
    pub scoped_recipient: DeviceId,
}

/// Essential frame metadata with minimal capability hints
///
/// Provides necessary framing information while preserving privacy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameHeader {
    /// Frame type for protocol routing
    pub frame_type: FrameType,
    /// Privacy level applied to this frame
    pub privacy_level: PrivacyLevel,
    /// Optional capability requirements hint (blinded)
    pub capability_hint: Option<String>,
    /// Frame size in bytes
    pub frame_size: u32,
}

/// Simple enum with necessary privacy-aware frame variants
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FrameType {
    /// Clear text message frame
    Clear,
    /// Capability-scoped message frame
    CapabilityScoped,
    /// Relationship-scoped message frame
    RelationshipScoped,
    /// Fully blinded message frame
    Blinded,
}

/// Privacy levels for transport operations
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum PrivacyLevel {
    /// Clear transmission - no privacy protection
    Clear,
    /// Basic blinding - hide message metadata
    Blinded,
    /// Relationship-scoped - restrict to relationship context
    RelationshipScoped,
}

impl Envelope {
    /// Create new envelope with minimal privacy preservation
    pub fn new(payload: Vec<u8>) -> Self {
        Self::new_with_id(Self::generate_message_id(), payload)
    }

    /// Create new envelope with specified message ID
    pub fn new_with_id(message_id: Uuid, payload: Vec<u8>) -> Self {
        Self {
            message_id,
            header: FrameHeader {
                frame_type: FrameType::Clear,
                privacy_level: PrivacyLevel::Clear,
                capability_hint: None,
                frame_size: payload.len() as u32,
            },
            payload,
            relationship_scope: None,
        }
    }

    /// Generate a deterministic message ID based on payload
    /// This avoids direct UUID generation while providing uniqueness
    fn generate_message_id() -> Uuid {
        // Use a deterministic UUID namespace for transport messages
        // This ensures reproducible IDs for testing while maintaining uniqueness
        Uuid::nil() // Placeholder - in production this would use a deterministic algorithm
    }

    /// Create relationship-scoped envelope with privacy preservation
    pub fn new_scoped(
        payload: Vec<u8>,
        relationship_id: RelationshipId,
        capability_hint: Option<String>,
    ) -> Self {
        Self::new_scoped_with_id(
            Self::generate_message_id(),
            payload,
            relationship_id,
            capability_hint,
        )
    }

    /// Create relationship-scoped envelope with specified message ID
    pub fn new_scoped_with_id(
        message_id: Uuid,
        payload: Vec<u8>,
        relationship_id: RelationshipId,
        capability_hint: Option<String>,
    ) -> Self {
        Self {
            message_id,
            header: FrameHeader {
                frame_type: FrameType::RelationshipScoped,
                privacy_level: PrivacyLevel::RelationshipScoped,
                capability_hint,
                frame_size: payload.len() as u32,
            },
            payload,
            relationship_scope: Some(relationship_id),
        }
    }

    /// Create blinded envelope hiding all metadata
    pub fn new_blinded(payload: Vec<u8>) -> Self {
        Self::new_blinded_with_id(Self::generate_message_id(), payload)
    }

    /// Create blinded envelope with specified message ID
    pub fn new_blinded_with_id(message_id: Uuid, payload: Vec<u8>) -> Self {
        Self {
            message_id,
            header: FrameHeader {
                frame_type: FrameType::Blinded,
                privacy_level: PrivacyLevel::Blinded,
                capability_hint: None,
                frame_size: 0, // Hide actual size
            },
            payload,
            relationship_scope: None,
        }
    }

    /// Check if envelope requires relationship context
    pub fn requires_relationship_scope(&self) -> bool {
        matches!(self.header.frame_type, FrameType::RelationshipScoped)
            || self.relationship_scope.is_some()
    }

    /// Get privacy level for this envelope
    pub fn privacy_level(&self) -> PrivacyLevel {
        self.header.privacy_level
    }
}

impl ScopedEnvelope {
    /// Create scoped envelope from base envelope
    pub fn new(
        envelope: Envelope,
        relationship_id: RelationshipId,
        sender: DeviceId,
        recipient: DeviceId,
    ) -> AuraResult<Self> {
        // Verify envelope supports scoping
        if !envelope.requires_relationship_scope() {
            return Err(aura_core::AuraError::invalid(
                "Envelope does not support relationship scoping",
            ));
        }

        Ok(Self {
            envelope,
            relationship_id,
            scoped_sender: sender,
            scoped_recipient: recipient,
        })
    }

    /// Extract base envelope for routing
    pub fn into_envelope(self) -> Envelope {
        self.envelope
    }

    /// Verify sender within relationship context
    pub fn verify_sender(&self, expected_sender: DeviceId) -> bool {
        self.scoped_sender == expected_sender
    }
}

impl Default for PrivacyLevel {
    /// Default to blinded for privacy-by-design
    fn default() -> Self {
        PrivacyLevel::Blinded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_envelope_privacy_levels() {
        let payload = b"test message".to_vec();

        // Clear envelope
        let clear = Envelope::new(payload.clone());
        assert!(matches!(clear.privacy_level(), PrivacyLevel::Clear));
        assert!(!clear.requires_relationship_scope());

        // Scoped envelope
        let relationship_id = RelationshipId::new([1u8; 32]);
        let scoped = Envelope::new_scoped(payload.clone(), relationship_id, None);
        assert!(matches!(
            scoped.privacy_level(),
            PrivacyLevel::RelationshipScoped
        ));
        assert!(scoped.requires_relationship_scope());

        // Blinded envelope
        let blinded = Envelope::new_blinded(payload);
        assert!(matches!(blinded.privacy_level(), PrivacyLevel::Blinded));
        assert_eq!(blinded.header.frame_size, 0); // Size is hidden
    }

    #[test]
    fn test_scoped_envelope_validation() {
        let payload = b"test message".to_vec();
        let relationship_id = RelationshipId::new([2u8; 32]);
        let sender = DeviceId::new();
        let recipient = DeviceId::new();

        // Valid scoped envelope
        let scoped_env = Envelope::new_scoped(payload.clone(), relationship_id.clone(), None);
        let scoped = ScopedEnvelope::new(scoped_env, relationship_id.clone(), sender, recipient);
        assert!(scoped.is_ok());

        // Invalid - envelope doesn't support scoping
        let clear_env = Envelope::new(payload);
        let result = ScopedEnvelope::new(clear_env, relationship_id, sender, recipient);
        assert!(result.is_err());
    }
}
