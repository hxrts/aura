//! Capability tokens for access control
//!
//! This module provides authorization-specific capability tokens with rich metadata
//! for enforcement and delegation. These are built on top of the canonical
//! CapabilityToken in aura-types, adding authorization-layer concerns.

use crate::{Action, AuthorizationError, Resource, Result, Subject};
use aura_types::CapabilityId;
use serde::{Deserialize, Serialize};

/// A capability token that grants specific permissions to a subject
///
/// This is an authorization-layer wrapper around the canonical aura-types::CapabilityToken,
/// providing authorization-specific features like delegation depth, conditions, and issuer signatures.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityToken {
    /// Unique identifier for this capability
    pub id: CapabilityId,

    /// Subject this capability is granted to
    pub subject: Subject,

    /// Resource this capability grants access to
    pub resource: Resource,

    /// Actions this capability allows
    pub actions: Vec<Action>,

    /// When this capability was issued
    pub issued_at: u64,

    /// When this capability expires (None = never expires)
    pub expires_at: Option<u64>,

    /// Who issued this capability
    pub issuer: aura_types::DeviceId,

    /// Signature proving the issuer authorized this capability
    pub issuer_signature: aura_crypto::Ed25519Signature,

    /// Whether this capability can be delegated
    pub delegatable: bool,

    /// Maximum delegation depth remaining
    pub delegation_depth: u8,

    /// Conditions that must be met for this capability to be valid
    pub conditions: Vec<CapabilityCondition>,
}

/// Conditions that can be attached to capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CapabilityCondition {
    /// Only valid during a specific time window
    TimeWindow { start: u64, end: u64 },

    /// Only valid when used from specific devices
    DeviceRestriction {
        allowed_devices: Vec<aura_types::DeviceId>,
    },

    /// Only valid for a limited number of uses
    UsageLimit { max_uses: u32, current_uses: u32 },

    /// Only valid when combined with other capabilities
    RequiresCombination {
        required_capabilities: Vec<CapabilityId>,
    },

    /// Custom condition with arbitrary data
    Custom {
        condition_type: String,
        condition_data: serde_json::Value,
    },
}

impl CapabilityToken {
    /// Create a new capability token
    pub fn new(
        subject: Subject,
        resource: Resource,
        actions: Vec<Action>,
        issuer: aura_types::DeviceId,
        delegatable: bool,
        delegation_depth: u8,
    ) -> Self {
        Self {
            id: CapabilityId::random(),
            subject,
            resource,
            actions,
            issued_at: current_timestamp(),
            expires_at: None,
            issuer,
            issuer_signature: aura_crypto::Ed25519Signature::default(), // Will be filled by caller
            delegatable,
            delegation_depth,
            conditions: Vec::new(),
        }
    }

    /// Check if this capability is currently valid
    pub fn is_valid(&self, current_time: u64) -> Result<()> {
        // Check expiration
        if let Some(expires_at) = self.expires_at {
            if current_time > expires_at {
                return Err(AuthorizationError::CapabilityExpired(format!(
                    "Capability {} expired at {}",
                    self.id, expires_at
                )));
            }
        }

        // Check conditions
        for condition in &self.conditions {
            self.check_condition(condition, current_time)?;
        }

        Ok(())
    }

    /// Check if this capability allows a specific action
    pub fn allows_action(&self, action: &Action) -> bool {
        self.actions.contains(action)
    }

    /// Check if this capability grants access to a specific resource
    pub fn grants_access_to(&self, resource: &Resource) -> bool {
        // For now, require exact match
        // In a full implementation, this would support resource hierarchies
        &self.resource == resource
    }

    /// Sign this capability token
    pub fn sign(&mut self, signing_key: &aura_crypto::Ed25519SigningKey) -> Result<()> {
        let token_bytes = self.serialize_for_signature()?;
        self.issuer_signature = aura_crypto::ed25519_sign(signing_key, &token_bytes);
        Ok(())
    }

    /// Verify the signature on this capability token
    pub fn verify_signature(
        &self,
        issuer_public_key: &aura_crypto::Ed25519VerifyingKey,
    ) -> Result<()> {
        let token_bytes = self.serialize_for_signature()?;
        aura_crypto::ed25519_verify(issuer_public_key, &token_bytes, &self.issuer_signature)
            .map_err(|e| {
                AuthorizationError::InvalidCapability(format!(
                    "Capability signature verification failed: {}",
                    e
                ))
            })?;
        Ok(())
    }

    /// Add a condition to this capability
    pub fn add_condition(&mut self, condition: CapabilityCondition) {
        self.conditions.push(condition);
    }

    /// Set expiration time
    pub fn set_expiration(&mut self, expires_at: u64) {
        self.expires_at = Some(expires_at);
    }

    /// Check a specific condition
    fn check_condition(&self, condition: &CapabilityCondition, current_time: u64) -> Result<()> {
        match condition {
            CapabilityCondition::TimeWindow { start, end } => {
                if current_time < *start || current_time > *end {
                    return Err(AuthorizationError::InvalidCapability(format!(
                        "Capability not valid at time {}",
                        current_time
                    )));
                }
            }
            CapabilityCondition::UsageLimit {
                max_uses,
                current_uses,
            } => {
                if current_uses >= max_uses {
                    return Err(AuthorizationError::InvalidCapability(
                        "Capability usage limit exceeded".to_string(),
                    ));
                }
            }
            // Other conditions would be checked here
            _ => {
                // For now, other conditions are not implemented
            }
        }
        Ok(())
    }

    /// Serialize capability for signature verification (excludes the signature field)
    fn serialize_for_signature(&self) -> Result<Vec<u8>> {
        // Create a version without the signature for signing/verification
        let unsigned_token = UnsignedCapabilityToken {
            id: self.id,
            subject: self.subject.clone(),
            resource: self.resource.clone(),
            actions: self.actions.clone(),
            issued_at: self.issued_at,
            expires_at: self.expires_at,
            issuer: self.issuer,
            delegatable: self.delegatable,
            delegation_depth: self.delegation_depth,
            conditions: self.conditions.clone(),
        };

        serde_json::to_vec(&unsigned_token).map_err(|e| {
            AuthorizationError::SerializationError(format!("Failed to serialize capability: {}", e))
        })
    }
}

/// Capability token without signature for signing/verification
#[derive(Serialize)]
struct UnsignedCapabilityToken {
    pub id: CapabilityId,
    pub subject: Subject,
    pub resource: Resource,
    pub actions: Vec<Action>,
    pub issued_at: u64,
    pub expires_at: Option<u64>,
    pub issuer: aura_types::DeviceId,
    pub delegatable: bool,
    pub delegation_depth: u8,
    pub conditions: Vec<CapabilityCondition>,
}

/// Get current timestamp (placeholder implementation)
#[allow(clippy::disallowed_methods)]
fn current_timestamp() -> u64 {
    // In a real implementation, this would use proper time
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_crypto::Effects;
    use aura_types::{AccountIdExt, DeviceIdExt};

    fn create_test_token(effects: &Effects) -> CapabilityToken {
        let subject = Subject::Device(aura_types::DeviceId::new_with_effects(effects));
        let resource = Resource::Account(aura_types::AccountId::new_with_effects(effects));
        let actions = vec![Action::Read, Action::Write];
        let issuer = aura_types::DeviceId::new_with_effects(effects);

        CapabilityToken::new(subject, resource, actions, issuer, true, 3)
    }

    #[test]
    fn test_capability_token_creation() {
        let effects = Effects::test();
        let token = create_test_token(&effects);

        assert!(token.delegatable);
        assert_eq!(token.delegation_depth, 3);
        assert_eq!(token.actions.len(), 2);
        assert!(token.allows_action(&Action::Read));
        assert!(token.allows_action(&Action::Write));
        assert!(!token.allows_action(&Action::Delete));
    }

    #[test]
    fn test_capability_token_validity() {
        let effects = Effects::test();
        let mut token = create_test_token(&effects);

        let current_time = current_timestamp();

        // Should be valid when just created
        assert!(token.is_valid(current_time).is_ok());

        // Should be invalid after expiration
        token.set_expiration(current_time - 1);
        assert!(token.is_valid(current_time).is_err());
    }

    #[test]
    fn test_capability_token_signing() {
        let effects = Effects::test();
        let mut token = create_test_token(&effects);

        let signing_key = aura_crypto::generate_ed25519_key();
        let verifying_key = aura_crypto::ed25519_verifying_key(&signing_key);

        // Sign the token
        assert!(token.sign(&signing_key).is_ok());

        // Verify the signature
        assert!(token.verify_signature(&verifying_key).is_ok());

        // Verify with wrong key should fail
        let wrong_signing_key = aura_crypto::generate_ed25519_key();
        let wrong_key = aura_crypto::ed25519_verifying_key(&wrong_signing_key);
        assert!(token.verify_signature(&wrong_key).is_err());
    }

    #[test]
    fn test_capability_conditions() {
        let effects = Effects::test();
        let mut token = create_test_token(&effects);

        let current_time = current_timestamp();

        // Add time window condition
        token.add_condition(CapabilityCondition::TimeWindow {
            start: current_time - 100,
            end: current_time + 100,
        });

        // Should be valid within window
        assert!(token.is_valid(current_time).is_ok());

        // Should be invalid outside window
        assert!(token.is_valid(current_time + 200).is_err());
    }
}
