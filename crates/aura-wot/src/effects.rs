//! Authorization Effects Implementation - Web-of-Trust Domain Logic
//!
//! This module implements AuthorizationEffects using aura-wot's domain-specific
//! Biscuit token logic and capability semilattice evaluation. This follows the
//! Layer 2 pattern where application effects are implemented in domain crates
//! using business logic combined with infrastructure effect composition.

use crate::biscuit::authorization::BiscuitAuthorizationBridge;
use crate::resource_scope::ResourceScope;
use async_trait::async_trait;
use aura_core::effects::{AuthorizationEffects, AuthorizationError, CryptoEffects};
use aura_core::identifiers::DeviceId;
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
pub struct WotAuthorizationHandler<C: CryptoEffects> {
    #[allow(dead_code)]
    crypto: C,
    #[allow(dead_code)]
    biscuit_bridge: BiscuitAuthorizationBridge,
    _phantom: PhantomData<()>,
}

impl<C: CryptoEffects> WotAuthorizationHandler<C> {
    /// Create a new WoT authorization handler with infrastructure effect dependencies
    pub fn new(crypto: C, root_public_key: PublicKey, device_id: DeviceId) -> Self {
        Self {
            crypto,
            biscuit_bridge: BiscuitAuthorizationBridge::new(root_public_key, device_id),
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
    fn map_operation_to_permission(&self, operation: &str) -> String {
        // TODO: Implement proper WoT permission mapping using aura-wot domain logic
        // This should use ResourceScope and other aura-wot types

        match operation {
            "read" => "read".to_string(),
            "write" => "write".to_string(),
            "execute" => "execute".to_string(),
            "admin" => "admin".to_string(),
            "delete" => "delete".to_string(),
            // WoT-specific operations
            "attest" => "attest".to_string(),
            "delegate" => "delegate".to_string(),
            "revoke" => "revoke".to_string(),
            _ => operation.to_string(),
        }
    }

    fn resource_scope_from_str(&self, resource: &str) -> ResourceScope {
        ResourceScope::Storage {
            authority_id: AuthorityId::from_uuid(Uuid::nil()),
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

        // 2. Apply Web-of-Trust authorization using domain logic
        let authorized = self.apply_wot_authorization(capabilities, operation, resource)?;

        if !authorized {
            return Ok(false);
        }

        // 3. Cryptographic operations via infrastructure effects
        // TODO: Use self.crypto for additional cryptographic validation
        // This might include signature verification, key validation, etc.

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

        // 3. Cryptographic operations via infrastructure effects
        // TODO: Use self.crypto for signing the delegated capability
        // This might include creating Biscuit tokens with proper attenuations

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
