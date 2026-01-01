//! Unified typed message system for Aura
//!
//! This module implements the core message types that match the formal specification:
//! ```rust
//! # use aura_core::AuthTag;
//! struct Msg<Ctx, Payload, Version> {
//!   ctx: Ctx,                 // RID or GID or DKD-context
//!   payload: Payload,         // typed by protocol role/state
//!   ver: Version,             // semantic version nego
//!   auth: AuthTag,            // signatures/MACs/AEAD tags
//! }
//! ```

use crate::types::identifiers::MessageContext;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Unified authentication tag for message verification
///
/// Supports different authentication methods used across the Aura system:
/// - Threshold signatures for multi-party protocols
/// - MAC tags for authenticated encryption
/// - AEAD authentication tags for confidential messaging
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AuthTag {
    /// FROST threshold signature with participant information
    ThresholdSignature {
        /// The aggregated signature bytes
        signature: Vec<u8>,
        /// Number of participants that signed (without revealing identities)
        participant_count: u16,
        /// Total number of participants in the threshold group
        threshold_config: u16,
    },
    /// MAC tag for message authentication
    Mac {
        /// The MAC tag bytes
        tag: [u8; 32],
        /// Algorithm identifier (e.g., "HMAC-SHA256")
        algorithm: String,
    },
    /// AEAD authentication tag
    AeadTag {
        /// The authentication tag
        tag: Vec<u8>,
        /// Algorithm identifier (e.g., "ChaCha20Poly1305", "AES-GCM")
        algorithm: String,
        /// Nonce used for this encryption
        nonce: Vec<u8>,
    },
    /// No authentication (for public messages)
    None,
}

impl AuthTag {
    /// Create a threshold signature auth tag
    pub fn threshold_signature(
        signature: Vec<u8>,
        participant_count: u16,
        threshold_config: u16,
    ) -> Self {
        Self::ThresholdSignature {
            signature,
            participant_count,
            threshold_config,
        }
    }

    /// Create a MAC auth tag
    pub fn mac(tag: [u8; 32], algorithm: impl Into<String>) -> Self {
        Self::Mac {
            tag,
            algorithm: algorithm.into(),
        }
    }

    /// Create an AEAD auth tag
    pub fn aead_tag(tag: Vec<u8>, algorithm: impl Into<String>, nonce: Vec<u8>) -> Self {
        Self::AeadTag {
            tag,
            algorithm: algorithm.into(),
            nonce,
        }
    }

    /// Check if this is a valid auth tag (basic validation)
    pub fn is_valid(&self) -> bool {
        match self {
            AuthTag::ThresholdSignature {
                signature,
                participant_count,
                threshold_config,
            } => {
                !signature.is_empty()
                    && participant_count > &0
                    && participant_count <= threshold_config
            }
            AuthTag::Mac { tag: _, algorithm } => !algorithm.is_empty(),
            AuthTag::AeadTag {
                tag,
                algorithm,
                nonce,
            } => !tag.is_empty() && !algorithm.is_empty() && !nonce.is_empty(),
            AuthTag::None => true,
        }
    }

    /// Get the authentication strength level (for capability checking)
    pub fn auth_strength(&self) -> AuthStrength {
        match self {
            AuthTag::ThresholdSignature { .. } => AuthStrength::ThresholdSignature,
            AuthTag::Mac { .. } => AuthStrength::MacAuthenticated,
            AuthTag::AeadTag { .. } => AuthStrength::AeadAuthenticated,
            AuthTag::None => AuthStrength::Unauthenticated,
        }
    }
}

impl fmt::Display for AuthTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuthTag::ThresholdSignature {
                participant_count,
                threshold_config,
                ..
            } => {
                write!(f, "ThresholdSig({participant_count}/{threshold_config})")
            }
            AuthTag::Mac { algorithm, .. } => {
                write!(f, "MAC({algorithm})")
            }
            AuthTag::AeadTag { algorithm, .. } => {
                write!(f, "AEAD({algorithm})")
            }
            AuthTag::None => write!(f, "NoAuth"),
        }
    }
}

/// Authentication strength levels for capability checking
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AuthStrength {
    /// No authentication
    Unauthenticated = 0,
    /// MAC authenticated but not encrypted
    MacAuthenticated = 1,
    /// AEAD authenticated and encrypted
    AeadAuthenticated = 2,
    /// Threshold signature (highest strength)
    ThresholdSignature = 3,
}

/// Semantic version for protocol negotiation
///
/// Follows semantic versioning (MAJOR.MINOR.PATCH) with protocol compatibility rules:
/// - MAJOR: Breaking changes, incompatible protocols
/// - MINOR: Backward-compatible feature additions
/// - PATCH: Bug fixes, fully compatible
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SemanticVersion {
    /// Major version (breaking changes)
    pub major: u16,
    /// Minor version (backward-compatible features)
    pub minor: u16,
    /// Patch version (bug fixes)
    pub patch: u16,
}

impl SemanticVersion {
    /// Create a new semantic version
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Current Aura protocol version
    pub const CURRENT: SemanticVersion = SemanticVersion::new(1, 0, 0);

    /// Check if this version is compatible with another version
    ///
    /// Compatible if:
    /// - Same major version (no breaking changes)
    /// - This minor >= other minor (backward compatibility)
    pub fn is_compatible_with(&self, other: &SemanticVersion) -> bool {
        self.major == other.major && self.minor >= other.minor
    }

    /// Check if this version can communicate with another
    ///
    /// Two-way compatibility: both versions must be compatible with each other
    pub fn can_communicate_with(&self, other: &SemanticVersion) -> bool {
        self.is_compatible_with(other) || other.is_compatible_with(self)
    }

    /// Get the negotiated version between two versions
    ///
    /// Returns the highest common compatible version, or None if incompatible
    pub fn negotiate_with(&self, other: &SemanticVersion) -> Option<SemanticVersion> {
        if !self.can_communicate_with(other) {
            return None;
        }

        // Same major version required
        if self.major != other.major {
            return None;
        }

        // Use minimum minor version for compatibility
        let negotiated_minor = self.minor.min(other.minor);

        Some(SemanticVersion::new(self.major, negotiated_minor, 0))
    }

    /// Check if this version is newer than another
    pub fn is_newer_than(&self, other: &SemanticVersion) -> bool {
        (self.major, self.minor, self.patch) > (other.major, other.minor, other.patch)
    }
}

impl fmt::Display for SemanticVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl std::str::FromStr for SemanticVersion {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 3 {
            return Err(format!("Invalid version format: {s}"));
        }

        let major = parts[0]
            .parse::<u16>()
            .map_err(|_| format!("Invalid major version: {}", parts[0]))?;
        let minor = parts[1]
            .parse::<u16>()
            .map_err(|_| format!("Invalid minor version: {}", parts[1]))?;
        let patch = parts[2]
            .parse::<u16>()
            .map_err(|_| format!("Invalid patch version: {}", parts[2]))?;

        Ok(SemanticVersion::new(major, minor, patch))
    }
}

/// Unified typed message structure matching the formal specification
///
/// This is the core message type that enforces:
/// - Context isolation (privacy partitions)
/// - Typed payloads (protocol safety)
/// - Version negotiation (compatibility)
/// - Authentication (security)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypedMessage<P> {
    /// Privacy context (RID/GID/DKD) - enforces partition isolation
    pub context: MessageContext,
    /// Typed payload (protocol-specific)
    pub payload: P,
    /// Protocol version for negotiation
    pub version: SemanticVersion,
    /// Authentication tag (signatures/MACs/AEAD)
    pub auth: AuthTag,
}

impl<P> TypedMessage<P> {
    /// Create a new typed message
    pub fn new(
        context: MessageContext,
        payload: P,
        version: SemanticVersion,
        auth: AuthTag,
    ) -> Self {
        Self {
            context,
            payload,
            version,
            auth,
        }
    }

    /// Create a message with current protocol version
    #[must_use]
    pub fn with_current_version(context: MessageContext, payload: P, auth: AuthTag) -> Self {
        Self::new(context, payload, SemanticVersion::CURRENT, auth)
    }

    /// Create an unauthenticated message (for public data)
    pub fn unauthenticated(context: MessageContext, payload: P, version: SemanticVersion) -> Self {
        Self::new(context, payload, version, AuthTag::None)
    }

    /// Check if this message can be processed with a given protocol version
    pub fn is_compatible_with_version(&self, local_version: &SemanticVersion) -> bool {
        self.version.can_communicate_with(local_version)
    }

    /// Check if this message is from a compatible context
    pub fn is_from_context(&self, expected_context: &MessageContext) -> bool {
        self.context.is_compatible_with(expected_context)
    }

    /// Check if authentication is sufficient for required strength
    pub fn has_sufficient_auth(&self, required_strength: AuthStrength) -> bool {
        self.auth.auth_strength() >= required_strength
    }

    /// Get the context hash for routing
    pub fn context_hash(&self) -> [u8; 32] {
        self.context.context_hash()
    }

    /// Map the payload to a different type (preserving metadata)
    pub fn map_payload<Q, F>(self, f: F) -> TypedMessage<Q>
    where
        F: FnOnce(P) -> Q,
    {
        TypedMessage {
            context: self.context,
            payload: f(self.payload),
            version: self.version,
            auth: self.auth,
        }
    }
}

impl<P> fmt::Display for TypedMessage<P>
where
    P: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Msg[{}|v{}|{}]", self.context, self.version, self.auth)
    }
}

/// Alias matching the formal specification syntax
/// Note: Context and Version are part of TypedMessage structure
pub type Msg<Payload> = TypedMessage<Payload>;

/// Message validation result
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageValidation {
    /// Message is valid and can be processed
    Valid,
    /// Message context doesn't match expected context
    ContextMismatch {
        /// Expected message context
        expected: MessageContext,
        /// Actual message context
        actual: MessageContext,
    },
    /// Message version is incompatible
    VersionIncompatible {
        /// Local protocol version
        local_version: SemanticVersion,
        /// Message protocol version
        message_version: SemanticVersion,
    },
    /// Authentication is insufficient
    InsufficientAuth {
        /// Required authentication strength
        required: AuthStrength,
        /// Provided authentication strength
        provided: AuthStrength,
    },
    /// Authentication tag is invalid
    InvalidAuth,
}

impl MessageValidation {
    /// Check if validation passed
    pub fn is_valid(&self) -> bool {
        matches!(self, MessageValidation::Valid)
    }
}

/// Message validator for enforcing context isolation and security policies
#[derive(Debug, Clone)]
pub struct MessageValidator {
    /// Expected context for messages
    pub expected_context: MessageContext,
    /// Local protocol version
    pub local_version: SemanticVersion,
    /// Minimum required authentication strength
    pub min_auth_strength: AuthStrength,
}

impl MessageValidator {
    /// Create a new message validator
    pub fn new(
        expected_context: MessageContext,
        local_version: SemanticVersion,
        min_auth_strength: AuthStrength,
    ) -> Self {
        Self {
            expected_context,
            local_version,
            min_auth_strength,
        }
    }

    /// Validate a message against policies
    pub fn validate<P>(&self, message: &TypedMessage<P>) -> MessageValidation {
        // Check context isolation
        if !message.is_from_context(&self.expected_context) {
            return MessageValidation::ContextMismatch {
                expected: self.expected_context.clone(),
                actual: message.context.clone(),
            };
        }

        // Check version compatibility
        if !message.is_compatible_with_version(&self.local_version) {
            return MessageValidation::VersionIncompatible {
                local_version: self.local_version,
                message_version: message.version,
            };
        }

        // Check authentication strength
        if !message.has_sufficient_auth(self.min_auth_strength) {
            return MessageValidation::InsufficientAuth {
                required: self.min_auth_strength,
                provided: message.auth.auth_strength(),
            };
        }

        // Check auth tag validity
        if !message.auth.is_valid() {
            return MessageValidation::InvalidAuth;
        }

        MessageValidation::Valid
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::identifiers::DeviceId;

    #[test]
    fn test_semantic_version_compatibility() {
        let v1_0_0 = SemanticVersion::new(1, 0, 0);
        let v1_1_0 = SemanticVersion::new(1, 1, 0);
        let v1_1_5 = SemanticVersion::new(1, 1, 5);
        let v2_0_0 = SemanticVersion::new(2, 0, 0);

        // Same major version, newer minor is compatible with older
        assert!(v1_1_0.is_compatible_with(&v1_0_0));
        assert!(!v1_0_0.is_compatible_with(&v1_1_0));

        // Can communicate both ways if one is compatible
        assert!(v1_1_0.can_communicate_with(&v1_0_0));
        assert!(v1_0_0.can_communicate_with(&v1_1_0));

        // Different major versions are incompatible
        assert!(!v2_0_0.can_communicate_with(&v1_0_0));

        // Negotiation picks minimum minor version
        let negotiated = v1_1_5.negotiate_with(&v1_0_0).unwrap();
        assert_eq!(negotiated, SemanticVersion::new(1, 0, 0));
    }

    #[test]
    fn test_message_context_isolation() {
        let device1 = DeviceId::new_from_entropy([18u8; 32]);
        let device2 = DeviceId::new_from_entropy([19u8; 32]);
        let device3 = DeviceId::new_from_entropy([20u8; 32]);

        let relay_ctx1 = MessageContext::relay_between(&device1, &device2);
        let relay_ctx2 = MessageContext::relay_between(&device2, &device3);
        let dkd_ctx = MessageContext::dkd_context("messaging", [0u8; 32]);

        // Same context should be compatible
        assert!(relay_ctx1.is_compatible_with(&relay_ctx1));

        // Different contexts should not be compatible
        assert!(!relay_ctx1.is_compatible_with(&relay_ctx2));
        assert!(!relay_ctx1.is_compatible_with(&dkd_ctx));
    }

    #[test]
    fn test_typed_message_validation() {
        let context = MessageContext::dkd_context("test", [1u8; 32]);
        let version = SemanticVersion::new(1, 0, 0);
        let auth = AuthTag::threshold_signature(vec![1, 2, 3], 3, 5);

        let message = TypedMessage::new(context.clone(), "test payload", version, auth);

        let validator = MessageValidator::new(context, version, AuthStrength::ThresholdSignature);

        let validation = validator.validate(&message);
        assert!(validation.is_valid());
    }

    #[test]
    fn test_context_mismatch() {
        let context1 = MessageContext::dkd_context("app1", [1u8; 32]);
        let context2 = MessageContext::dkd_context("app2", [2u8; 32]);
        let version = SemanticVersion::new(1, 0, 0);
        let auth = AuthTag::None;

        let message = TypedMessage::new(context1, "payload", version, auth);
        let validator = MessageValidator::new(context2, version, AuthStrength::Unauthenticated);

        let validation = validator.validate(&message);
        assert!(matches!(
            validation,
            MessageValidation::ContextMismatch { .. }
        ));
    }

    #[test]
    fn test_auth_strength() {
        assert!(AuthStrength::ThresholdSignature > AuthStrength::AeadAuthenticated);
        assert!(AuthStrength::AeadAuthenticated > AuthStrength::MacAuthenticated);
        assert!(AuthStrength::MacAuthenticated > AuthStrength::Unauthenticated);
    }
}
