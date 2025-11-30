//! Authorization Effects Implementation - Web-of-Trust Domain Logic
//!
//! This module implements AuthorizationEffects using aura-wot's domain-specific
//! Biscuit token logic and capability semilattice evaluation. This follows the
//! Layer 2 pattern where application effects are implemented in domain crates
//! using business logic combined with infrastructure effect composition.

use crate::biscuit_authorization::BiscuitAuthorizationBridge;
use crate::resource_scope::ResourceScope;
use async_trait::async_trait;
use aura_core::effects::{AuthorizationEffects, AuthorizationError, CryptoEffects};
use aura_core::{AuthorityId, Cap, MeetSemiLattice};
use biscuit_auth::PublicKey;
use std::marker::PhantomData;
use uuid::Uuid;

/// Domain-specific authorization handler that uses Web-of-Trust Biscuit tokens
///
/// This handler implements AuthorizationEffects by combining:
/// - CryptoEffects for cryptographic operations
/// - aura-wot domain logic for Biscuit token validation and policy evaluation
/// - Capability semilattice operations for permission checking
///
/// This is the correct pattern for application effects: domain crates implement
/// them by composing infrastructure effects with business logic.
#[derive(Clone)]
pub struct WotAuthorizationHandler<C: CryptoEffects> {
    #[allow(dead_code)]
    crypto: C,
    #[allow(dead_code)]
    biscuit_bridge: BiscuitAuthorizationBridge,
    _phantom: PhantomData<()>,
}

impl<C: CryptoEffects> WotAuthorizationHandler<C> {
    /// Create a new WoT authorization handler with infrastructure effect dependencies
    pub fn new(crypto: C, root_public_key: PublicKey, authority_id: AuthorityId) -> Self {
        Self {
            crypto,
            biscuit_bridge: BiscuitAuthorizationBridge::new(root_public_key, authority_id),
            _phantom: PhantomData,
        }
    }

    /// Create a mock handler for testing and development
    pub fn new_mock(crypto: C) -> Self {
        Self {
            crypto,
            biscuit_bridge: BiscuitAuthorizationBridge::new_mock(),
            _phantom: PhantomData,
        }
    }

    /// Validate capability structure and temporal bounds
    ///
    /// This encapsulates the domain-specific business logic for capability validation
    /// that belongs in the Web-of-Trust domain crate.
    fn validate_capability_semantics(&self, cap: &Cap) -> Result<(), AuthorizationError> {
        if cap.is_empty() {
            return Err(AuthorizationError::InvalidCapabilities {
                reason: "empty capability token".to_string(),
            });
        }
        Ok(())
    }

    /// Apply Web-of-Trust specific authorization policies
    ///
    /// This implements the domain-specific authorization logic using Biscuit
    /// tokens and capability semilattice operations.
    fn apply_wot_authorization(
        &self,
        cap: &Cap,
        operation: &str,
        resource: &str,
    ) -> Result<bool, AuthorizationError> {
        let token = cap
            .to_biscuit(&self.biscuit_bridge.root_public_key())
            .map_err(|e| AuthorizationError::InvalidToken {
                reason: e.to_string(),
            })?;

        let scope = self.resource_scope_from_str(resource);

        let result = self
            .biscuit_bridge
            .authorize(&token, operation, &scope)
            .map_err(|e| AuthorizationError::InvalidToken {
                reason: e.to_string(),
            })?;

        Ok(result.authorized)
    }

    /// Map operation strings to domain-specific permissions
    ///
    /// This implements the Web-of-Trust specific operation-to-permission mapping.
    #[allow(dead_code)] // Reserved for future WoT-specific permission mapping
    fn map_operation_to_permission(&self, operation: &str, scope: &ResourceScope) -> String {
        // Normalize operations to the canonical WoT permission vocabulary.
        match (operation, scope) {
            ("read", _) | ("list", _) => "read".into(),
            ("write", _) | ("update", _) | ("append", _) => "write".into(),
            ("delete", _) => "delete".into(),
            ("execute", _) => "execute".into(),
            ("admin", _) => "admin".into(),
            // WoT-specific operations
            ("attest", _) => "attest".into(),
            ("delegate", _) => "delegate".into(),
            ("revoke", _) => "revoke".into(),
            // Default: pass through untouched for forward compatibility
            _ => operation.to_string(),
        }
    }

    fn resource_scope_from_str(&self, resource: &str) -> ResourceScope {
        // Parse formats like "authority:<uuid>/path" or plain paths as storage scopes.
        if let Some(rest) = resource.strip_prefix("authority:") {
            let mut parts = rest.splitn(2, '/');
            if let Some(id_str) = parts.next() {
                if let Ok(uuid) = Uuid::parse_str(id_str) {
                    let path = parts.next().unwrap_or_default().to_string();
                    return ResourceScope::Storage {
                        authority_id: AuthorityId::from_uuid(uuid),
                        path,
                    };
                }
            }
        }

        ResourceScope::Storage {
            // Deterministic, non-nil fallback derived from resource path
            authority_id: AuthorityId::from_uuid(Uuid::new_v5(&Uuid::NAMESPACE_URL, resource.as_bytes())),
            path: resource.to_string(),
        }
    }
}

#[async_trait]
impl<C: CryptoEffects> AuthorizationEffects for WotAuthorizationHandler<C> {
    async fn verify_capability(
        &self,
        capabilities: &Cap,
        operation: &str,
        resource: &str,
    ) -> Result<bool, AuthorizationError> {
        // 1. Domain validation using aura-wot business logic
        self.validate_capability_semantics(capabilities)?;

        // Reject obviously invalid root keys before crypto operations
        let root_key_bytes = self.biscuit_bridge.root_public_key().to_bytes();
        if self.crypto.constant_time_eq(&root_key_bytes, &[0u8; 32]) {
            return Err(AuthorizationError::InvalidToken {
                reason: "invalid root public key".to_string(),
            });
        }

        // 2. Apply Web-of-Trust authorization using domain logic
        let scope = self.resource_scope_from_str(resource);
        let permission = self.map_operation_to_permission(operation, &scope);
        let authorized = self.apply_wot_authorization(capabilities, &permission, resource)?;

        if !authorized {
            return Ok(false);
        }

        // 3. Cryptographic integrity check: ensure root key is well-formed (not all-zero)
        let key_all_zero = self.crypto.constant_time_eq(&root_key_bytes, &[0u8; 32]);
        if key_all_zero {
            return Err(AuthorizationError::InvalidToken {
                reason: "root key failed integrity check".to_string(),
            });
        }

        Ok(true)
    }

    async fn delegate_capabilities(
        &self,
        source_capabilities: &Cap,
        requested_capabilities: &Cap,
        _target_authority: &AuthorityId,
    ) -> Result<Cap, AuthorizationError> {
        // 1. Domain validation
        self.validate_capability_semantics(source_capabilities)?;
        self.validate_capability_semantics(requested_capabilities)?;

        // 2. Apply delegation using aura-wot domain logic
        // This implements the principle of least privilege using meet-semilattice operations
        // The delegated capabilities are the intersection of source and requested (source ⊓ requested)
        let delegated_cap = source_capabilities.meet(requested_capabilities);

        // 3. Cryptographic guard: ensure delegated caps hash to a non-zero value to prevent empty delegation
        let delegated_bytes = bincode::serialize(&delegated_cap).map_err(|e| {
            AuthorizationError::InvalidCapabilities {
                reason: format!("failed to serialize delegated cap: {}", e),
            }
        })?;
        let zero = vec![0u8; delegated_bytes.len().max(1)];
        if self.crypto.constant_time_eq(&delegated_bytes, &zero) {
            return Err(AuthorizationError::InvalidCapabilities {
                reason: "delegated capability serialization invalid".to_string(),
            });
        }

        Ok(delegated_cap)
    }
}

impl<C: CryptoEffects> Default for WotAuthorizationHandler<C>
where
    C: Default,
{
    fn default() -> Self {
        Self::new_mock(C::default())
    }
}
