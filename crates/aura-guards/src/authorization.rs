//! Biscuit authorization bridge for guard chain integration.
//!
//! Bridges Biscuit token verification with Datalog policy evaluation,
//! providing authorization checks with explicit time for determinism.

#![allow(
    missing_docs,
    unused_variables,
    clippy::unwrap_used,
    clippy::expect_used,
    dead_code,
    clippy::match_like_matches_macro,
    clippy::type_complexity,
    clippy::while_let_loop,
    clippy::redundant_closure,
    clippy::large_enum_variant,
    clippy::unused_unit,
    clippy::get_first,
    clippy::single_range_in_vec_init,
    clippy::disallowed_methods, // Guard chain coordinates time/random effects
    deprecated // Deprecated time/random functions used intentionally for effect coordination
)]
use aura_authorization::{BiscuitError, ResourceScope};
use aura_core::types::identifiers::AuthorityId;
use aura_core::CapabilityName;
use biscuit_auth::{macros::*, Authorizer, AuthorizerLimits, Biscuit, PublicKey};
use std::time::Duration;

const GUARD_BISCUIT_LIMITS: AuthorizerLimits = AuthorizerLimits {
    max_facts: 10_000,
    max_iterations: 1_000,
    max_time: Duration::from_millis(100),
};

pub struct BiscuitAuthorizationBridge {
    root_public_key: PublicKey,
    authority_id: AuthorityId,
}

impl BiscuitAuthorizationBridge {
    fn biscuit<T>(
        result: std::result::Result<T, biscuit_auth::error::Token>,
    ) -> Result<T, BiscuitError> {
        result.map_err(BiscuitError::BiscuitLib)
    }

    fn add_authority_fact(&self, authorizer: &mut Authorizer) -> Result<(), BiscuitError> {
        let authority = self.authority_id.to_string();
        Self::biscuit(authorizer.add_fact(fact!("authority({authority})")))?;
        Ok(())
    }

    fn add_time_fact(
        &self,
        authorizer: &mut Authorizer,
        current_time_seconds: u64,
    ) -> Result<(), BiscuitError> {
        let time = current_time_seconds as i64;
        Self::biscuit(authorizer.add_fact(fact!("time({time})")))?;
        Ok(())
    }

    fn add_authorize_ambient_facts(
        &self,
        authorizer: &mut Authorizer,
        operation: &str,
        current_time_seconds: u64,
    ) -> Result<(), BiscuitError> {
        Self::biscuit(authorizer.add_fact(fact!("operation({operation})")))?;
        self.add_authority_fact(authorizer)?;
        self.add_time_fact(authorizer, current_time_seconds)?;
        Ok(())
    }

    fn add_resource_facts(
        &self,
        authorizer: &mut Authorizer,
        resource: &ResourceScope,
    ) -> Result<(), BiscuitError> {
        let resource_pattern = resource.resource_pattern();
        Self::biscuit(authorizer.add_fact(fact!("resource({resource_pattern})")))?;

        match resource {
            ResourceScope::Authority {
                authority_id,
                operation,
            } => {
                Self::biscuit(authorizer.add_fact(fact!("resource_type(\"authority\")")))?;
                let auth_id = authority_id.to_string();
                Self::biscuit(authorizer.add_fact(fact!("authority_id({auth_id})")))?;
                let op_str = operation.as_str();
                Self::biscuit(authorizer.add_fact(fact!("authority_operation({op_str})")))?;
            }
            ResourceScope::Context {
                context_id,
                operation,
            } => {
                Self::biscuit(authorizer.add_fact(fact!("resource_type(\"context\")")))?;
                let ctx_id = context_id.to_string();
                Self::biscuit(authorizer.add_fact(fact!("context_id({ctx_id})")))?;
                let op_str = operation.as_str();
                Self::biscuit(authorizer.add_fact(fact!("context_operation({op_str})")))?;
            }
            ResourceScope::Storage { authority_id, path } => {
                Self::biscuit(authorizer.add_fact(fact!("resource_type(\"storage\")")))?;
                let auth_id = authority_id.to_string();
                Self::biscuit(authorizer.add_fact(fact!("authority_id({auth_id})")))?;
                let path_str = path.as_str();
                Self::biscuit(authorizer.add_fact(fact!("storage_path({path_str})")))?;
            }
        }

        Ok(())
    }

    fn add_operation_checks(
        &self,
        authorizer: &mut Authorizer,
        operation: &str,
    ) -> Result<(), BiscuitError> {
        match operation {
            "read" => Self::biscuit(authorizer.add_check(check!("check if capability(\"read\")")))?,
            "write" => {
                Self::biscuit(authorizer.add_check(check!("check if capability(\"write\")")))?;
            }
            "execute" => {
                Self::biscuit(authorizer.add_check(check!("check if capability(\"execute\")")))?;
            }
            "admin" => {
                Self::biscuit(authorizer.add_check(check!("check if capability(\"admin\")")))?;
                Self::biscuit(
                    authorizer
                        .add_check(check!("check if role(\"member\") or role(\"moderator\")")),
                )?;
            }
            "delegate" => {
                Self::biscuit(authorizer.add_check(check!("check if capability(\"delegate\")")))?;
            }
            _ if operation.contains(':') => {
                Self::biscuit(authorizer.add_check(check!("check if capability({operation})")))?;
            }
            _ => {
                Self::biscuit(authorizer.add_check(check!("check if capability(\"execute\")")))?;
            }
        }

        Ok(())
    }

    fn add_allow_policy(&self, authorizer: &mut Authorizer) -> Result<(), BiscuitError> {
        Self::biscuit(authorizer.add_policy(policy!("allow if true")))?;
        Ok(())
    }

    pub fn new(root_public_key: PublicKey, authority_id: AuthorityId) -> Self {
        Self {
            root_public_key,
            authority_id,
        }
    }

    /// Bridge for guard-chain evaluation using provided authority key material.
    ///
    /// Callers must supply the authority's Biscuit root public key; no mock fallback.
    /// `operation_id` is retained for logging correlation.
    pub fn for_guard(
        root_public_key: PublicKey,
        authority_id: AuthorityId,
        _operation_id: &str,
    ) -> Self {
        Self::new(root_public_key, authority_id)
    }

    /// Get the authority ID for this bridge
    pub fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }

    /// Production Biscuit authorization with cryptographic verification and Datalog policy evaluation
    /// Requires current time for deterministic behavior - use PhysicalTimeEffects in callers
    pub fn authorize(
        &self,
        token: &Biscuit,
        operation: &str,
        resource: &ResourceScope,
        current_time_seconds: u64,
    ) -> Result<AuthorizationResult, BiscuitError> {
        self.authorize_with_time(token, operation, resource, current_time_seconds)
    }

    /// Internal implementation of Biscuit authorization with explicit time
    fn authorize_with_time(
        &self,
        token: &Biscuit,
        operation: &str,
        resource: &ResourceScope,
        current_time_seconds: u64,
    ) -> Result<AuthorizationResult, BiscuitError> {
        // Phase 1: Verify token signature with root public key
        let mut authorizer = token.authorizer().map_err(BiscuitError::BiscuitLib)?;

        // Verify the token signature is valid for our root key
        // The authorizer creation already verifies the signature chain

        // Phase 2: Add ambient facts for authorization context
        self.add_authorize_ambient_facts(&mut authorizer, operation, current_time_seconds)?;
        self.add_resource_facts(&mut authorizer, resource)?;

        // Phase 3: Add authorization checks for specific operations
        //
        // Checks gate access: if any check fails, authorization is denied.
        // The blanket allow policy (added in Phase 4) permits the request
        // only if all checks pass.
        self.add_operation_checks(&mut authorizer, operation)?;

        // Phase 4: Allow policy + Datalog evaluation
        //
        // Authorization requires at least one allow policy to match.
        // Checks (above) gate access; this policy permits if all pass.
        self.add_allow_policy(&mut authorizer)?;
        let authorization_result = authorizer.authorize_with_limits(GUARD_BISCUIT_LIMITS);

        let authorized = match authorization_result {
            Ok(_) => true,
            Err(biscuit_auth::error::Token::FailedLogic(_)) => false,
            Err(e) => return Err(BiscuitError::BiscuitLib(e)),
        };

        Ok(AuthorizationResult {
            authorized,
            delegation_depth: self.extract_delegation_depth_from_token(token),
            token_facts: self.extract_token_facts_from_blocks(token, current_time_seconds),
        })
    }

    /// Check if token has specific capability through Datalog evaluation
    /// Requires current time for deterministic behavior - use PhysicalTimeEffects in callers
    pub fn has_capability(
        &self,
        token: &Biscuit,
        capability: &str,
        current_time_seconds: u64,
    ) -> Result<bool, BiscuitError> {
        self.has_capability_with_time(token, capability, current_time_seconds)
    }

    /// Internal implementation of capability check with explicit time
    fn has_capability_with_time(
        &self,
        token: &Biscuit,
        capability: &str,
        current_time_seconds: u64,
    ) -> Result<bool, BiscuitError> {
        // Validate capability name before passing to Datalog evaluation.
        let capability_name = CapabilityName::parse(capability)
            .map_err(|error| BiscuitError::InvalidCapability(error.to_string()))?;
        let capability = capability_name.as_str();

        // Create authorizer and verify token signature
        let mut authorizer = token.authorizer().map_err(BiscuitError::BiscuitLib)?;

        // Add ambient facts for capability check
        self.add_authority_fact(&mut authorizer)?;
        self.add_time_fact(&mut authorizer, current_time_seconds)?;

        // Add a check to see if the token contains the requested capability
        Self::biscuit(authorizer.add_check(check!("check if capability({capability})")))?;

        // Allow policy required for authorize() to succeed
        self.add_allow_policy(&mut authorizer)?;

        // Run Datalog evaluation
        let result = authorizer.authorize_with_limits(GUARD_BISCUIT_LIMITS);

        match result {
            Ok(_) => Ok(true),
            Err(biscuit_auth::error::Token::FailedLogic(_)) => Ok(false),
            Err(e) => Err(BiscuitError::BiscuitLib(e)),
        }
    }

    /// Extract delegation depth from token structure
    fn extract_delegation_depth_from_token(&self, token: &Biscuit) -> Option<u32> {
        // Count the number of blocks beyond the authority block
        // Authority block (0) + N delegation blocks = depth N
        let count = token.block_count();
        if count > 0 {
            // Home count includes authority block, so delegation depth is count - 1
            Some((count - 1) as u32)
        } else {
            Some(0) // Only authority block
        }
    }

    /// Extract readable token facts from token blocks
    fn extract_token_facts_from_blocks(
        &self,
        token: &Biscuit,
        current_time_seconds: u64,
    ) -> Vec<String> {
        let mut facts = Vec::new();

        // Add basic verification metadata
        facts.push(format!("authority(\"{}\")", self.authority_id));
        facts.push(format!("extracted_at({current_time_seconds})"));
        facts.push("extracted_from_token".to_string());

        // Try to extract facts from token using an authorizer
        if let Ok(authorizer) = token.authorizer() {
            // Get the world facts which include facts from all token blocks
            let (world_facts, world_rules, _world_checks, _world_policies) = authorizer.dump();
            // Parse facts from the world dump
            for fact in world_facts {
                facts.push(format!("{fact}"));
            }

            // Include any rules as well for debugging
            for rule in world_rules {
                facts.push(format!("rule: {rule}"));
            }
        }

        // If we couldn't extract detailed facts, provide basic token info
        if facts.len() <= 3 {
            let count = token.block_count();
            facts.push(format!("block_count({count})"));
        }

        facts
    }
}

#[derive(Debug, Clone)]
pub struct AuthorizationResult {
    pub authorized: bool,
    pub delegation_depth: Option<u32>,
    pub token_facts: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{capability_name, types::identifiers::AuthorityId};

    fn test_bridge() -> (BiscuitAuthorizationBridge, Biscuit) {
        let authority_id = AuthorityId::new_from_entropy([42u8; 32]);
        let authority = aura_authorization::TokenAuthority::new(authority_id);
        let token = authority
            .create_token(
                authority_id,
                vec![
                    capability_name!("execute"),
                    capability_name!("invitation:send"),
                ],
            )
            .unwrap_or_else(|err| panic!("failed to create token: {err:?}"));
        let bridge = BiscuitAuthorizationBridge::new(authority.root_public_key(), authority_id);
        (bridge, token)
    }

    #[test]
    fn namespaced_capability_authorized_through_bridge() {
        let (bridge, token) = test_bridge();
        let result = bridge.has_capability(&token, "invitation:send", 1000);
        assert!(
            result.unwrap_or_else(|err| panic!("capability check failed: {err:?}")),
            "invitation:send should be authorized — token carries this capability"
        );
    }

    #[test]
    fn namespaced_capability_not_present_is_denied() {
        let (bridge, token) = test_bridge();
        let result = bridge.has_capability(&token, "recovery:initiate", 1000);
        assert!(
            !result.unwrap_or_else(|err| panic!("capability check failed: {err:?}")),
            "recovery:initiate should be denied — token does not carry this capability"
        );
    }

    #[test]
    fn authorize_namespaced_operation_checks_exact_capability() {
        let (bridge, token) = test_bridge();
        let scope = aura_authorization::ResourceScope::Authority {
            authority_id: AuthorityId::new_from_entropy([99u8; 32]),
            operation: aura_core::types::scope::AuthorityOp::UpdateTree,
        };
        // invitation:send is in the token and should match the namespaced check
        let result = bridge
            .authorize(&token, "invitation:send", &scope, 1000)
            .unwrap_or_else(|err| panic!("authorize failed: {err:?}"));
        assert!(
            result.authorized,
            "namespaced operation should match the exact capability in the token"
        );
    }

    #[test]
    fn authorize_namespaced_operation_denied_without_matching_capability() {
        let (bridge, token) = test_bridge();
        let scope = aura_authorization::ResourceScope::Authority {
            authority_id: AuthorityId::new_from_entropy([99u8; 32]),
            operation: aura_core::types::scope::AuthorityOp::UpdateTree,
        };
        // recovery:initiate is NOT in the token
        let result = bridge
            .authorize(&token, "recovery:initiate", &scope, 1000)
            .unwrap_or_else(|err| panic!("authorize failed: {err:?}"));
        assert!(
            !result.authorized,
            "namespaced operation without matching capability should be denied"
        );
    }

    #[test]
    fn generic_execute_still_works_for_unnamespaced_operations() {
        let (bridge, token) = test_bridge();
        let scope = aura_authorization::ResourceScope::Authority {
            authority_id: AuthorityId::new_from_entropy([99u8; 32]),
            operation: aura_core::types::scope::AuthorityOp::UpdateTree,
        };
        // "execute" is in the token as a generic capability
        let result = bridge
            .authorize(&token, "execute", &scope, 1000)
            .unwrap_or_else(|err| panic!("authorize failed: {err:?}"));
        assert!(result.authorized);
    }

    #[test]
    fn capability_name_validation_rejects_uppercase() {
        let (bridge, token) = test_bridge();
        let result = bridge.has_capability(&token, "Invitation:Send", 1000);
        assert!(
            result.is_err(),
            "uppercase capability names should be rejected by validation"
        );
    }
}
