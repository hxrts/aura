//! Privacy-Aware Message Envelope Types
//!
//! Provides essential message wrappers with built-in privacy preservation, context scoping,
//! and minimal framing metadata. Target: <150 lines (concise implementation).

use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::AuraResult;
use serde::{Deserialize, Serialize};

use super::ids::MessageId;
/// Universal message wrapper with essential blinding capabilities
///
/// Integrates privacy preservation directly into the core envelope type.
/// Supports both clear and blinded modes with context scoping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    /// Unique message identifier
    pub message_id: MessageId,
    /// Frame metadata with privacy hints
    pub header: FrameHeader,
    /// Message payload (may be blinded)
    pub payload: Vec<u8>,
    /// Context for scoped routing (relational context)
    pub context_id: Option<ContextId>,
}

/// Context-scoped message envelope for privacy-preserving routing
///
/// Ensures messages are only routed within appropriate relational contexts.
/// Uses AuthorityId for cross-authority communication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopedEnvelope {
    /// Base envelope
    pub envelope: Envelope,
    /// Required relational context
    pub context_id: ContextId,
    /// Sender authority within context
    pub sender: AuthorityId,
    /// Recipient authority within context
    pub recipient: AuthorityId,
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
#[non_exhaustive]
pub enum FrameType {
    /// Clear text message frame
    Clear,
    /// Capability-scoped message frame
    CapabilityScoped,
    /// Context-scoped message frame
    ContextScoped,
    /// Fully blinded message frame
    Blinded,
}

/// Privacy levels for transport operations
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PrivacyLevel {
    /// Clear transmission - no privacy protection
    Clear,
    /// Basic blinding - hide message metadata
    Blinded,
    /// Context-scoped - restrict to relational context
    ContextScoped,
}

impl Envelope {
    /// Create new envelope with minimal privacy preservation
    pub fn new(payload: Vec<u8>) -> Self {
        Self::new_with_id(MessageId::new(), payload)
    }

    /// Create new envelope with specified message ID
    pub fn new_with_id(message_id: MessageId, payload: Vec<u8>) -> Self {
        Self {
            message_id,
            header: FrameHeader {
                frame_type: FrameType::Clear,
                privacy_level: PrivacyLevel::Clear,
                capability_hint: None,
                frame_size: payload.len() as u32,
            },
            payload,
            context_id: None,
        }
    }

    /// Create context-scoped envelope with privacy preservation
    pub fn new_scoped(
        payload: Vec<u8>,
        context_id: ContextId,
        capability_hint: Option<String>,
    ) -> Self {
        Self::new_scoped_with_id(MessageId::new(), payload, context_id, capability_hint)
    }

    /// Create context-scoped envelope with specified message ID
    pub fn new_scoped_with_id(
        message_id: MessageId,
        payload: Vec<u8>,
        context_id: ContextId,
        capability_hint: Option<String>,
    ) -> Self {
        Self {
            message_id,
            header: FrameHeader {
                frame_type: FrameType::ContextScoped,
                privacy_level: PrivacyLevel::ContextScoped,
                capability_hint,
                frame_size: payload.len() as u32,
            },
            payload,
            context_id: Some(context_id),
        }
    }

    /// Create blinded envelope hiding all metadata
    pub fn new_blinded(payload: Vec<u8>) -> Self {
        Self::new_blinded_with_id(MessageId::new(), payload)
    }

    /// Create blinded envelope with specified message ID
    pub fn new_blinded_with_id(message_id: MessageId, payload: Vec<u8>) -> Self {
        Self {
            message_id,
            header: FrameHeader {
                frame_type: FrameType::Blinded,
                privacy_level: PrivacyLevel::Blinded,
                capability_hint: None,
                frame_size: 0, // Hide actual size
            },
            payload,
            context_id: None,
        }
    }

    /// Check if envelope requires context scope
    pub fn requires_context_scope(&self) -> bool {
        matches!(self.header.frame_type, FrameType::ContextScoped) || self.context_id.is_some()
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
        context_id: ContextId,
        sender: AuthorityId,
        recipient: AuthorityId,
    ) -> AuraResult<Self> {
        // Verify envelope supports scoping
        if !envelope.requires_context_scope() {
            return Err(aura_core::AuraError::invalid(
                "Envelope does not support context scoping",
            ));
        }

        Ok(Self {
            envelope,
            context_id,
            sender,
            recipient,
        })
    }

    /// Extract base envelope for routing
    pub fn into_envelope(self) -> Envelope {
        self.envelope
    }

    /// Verify sender within context
    pub fn verify_sender(&self, expected_sender: AuthorityId) -> bool {
        self.sender == expected_sender
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
        assert!(!clear.requires_context_scope());

        // Scoped envelope
        let context_id = ContextId::new_from_entropy([1u8; 32]);
        let scoped = Envelope::new_scoped(payload.clone(), context_id, None);
        assert!(matches!(
            scoped.privacy_level(),
            PrivacyLevel::ContextScoped
        ));
        assert!(scoped.requires_context_scope());

        // Blinded envelope
        let blinded = Envelope::new_blinded(payload);
        assert!(matches!(blinded.privacy_level(), PrivacyLevel::Blinded));
        assert_eq!(blinded.header.frame_size, 0); // Size is hidden
    }

    #[test]
    fn test_scoped_envelope_validation() {
        let payload = b"test message".to_vec();
        let context_id = ContextId::new_from_entropy([2u8; 32]);
        let sender = AuthorityId::new_from_entropy([1u8; 32]);
        let recipient = AuthorityId::new_from_entropy([2u8; 32]);

        // Valid scoped envelope
        let scoped_env = Envelope::new_scoped(payload.clone(), context_id, None);
        let scoped = ScopedEnvelope::new(scoped_env, context_id, sender, recipient);
        assert!(scoped.is_ok());

        // Invalid - envelope doesn't support scoping
        let clear_env = Envelope::new(payload);
        let result = ScopedEnvelope::new(clear_env, context_id, sender, recipient);
        assert!(result.is_err());
    }
}
