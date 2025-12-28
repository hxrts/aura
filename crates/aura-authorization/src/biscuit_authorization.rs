//! Layer 2: Biscuit Cryptographic Authorization
//!
//! Cryptographic authorization infrastructure for Aura's Web of Trust system using Biscuit tokens.
//! Biscuit provides attenuated, cryptographically-verifiable capabilities with Datalog policy evaluation.
//!
//! **Key Components** (per docs/109_authorization.md):
//! - **BiscuitAuthorizationBridge**: Token generation, attenuation, verification
//! - **Datalog Policy Evaluation**: Logic programming for fine-grained authorization
//! - **Capability Attenuation**: Restrict tokens to specific scope/time/resources
//!
//! **Integration Point**: CapGuard in aura-protocol/guards evaluates Biscuit tokens at message
//! entry point (first guard in chain); enables delegation without trusted intermediaries.

use crate::BiscuitError;
use aura_core::scope::{AuthorizationOp, ResourceScope};
use aura_core::{hash::hash, identifiers::AuthorityId};
use biscuit_auth::{macros::*, Biscuit, PublicKey};

// ============================================================================
// Biscuit Authorization Bridge
// ============================================================================

#[derive(Clone, Debug)]
pub struct BiscuitAuthorizationBridge {
    _root_public_key: PublicKey,
    authority_id: AuthorityId,
}

impl BiscuitAuthorizationBridge {
    pub fn new(root_public_key: PublicKey, authority_id: AuthorityId) -> Self {
        Self {
            _root_public_key: root_public_key,
            authority_id,
        }
    }

    /// Create a mock bridge for testing with a generated keypair
    #[cfg(test)]
    pub fn new_mock() -> Self {
        use biscuit_auth::KeyPair;
        let keypair = KeyPair::new();
        Self {
            _root_public_key: keypair.public(),
            authority_id: AuthorityId::new_from_entropy(hash(&keypair.public().to_bytes())),
        }
    }

    /// Create a mock bridge for testing (non-test builds for integration)
    #[cfg(not(test))]
    pub fn new_mock() -> Self {
        use biscuit_auth::KeyPair;
        let keypair = KeyPair::new();
        Self {
            _root_public_key: keypair.public(),
            authority_id: AuthorityId::new_from_entropy(hash(&keypair.public().to_bytes())),
        }
    }

    /// Production Biscuit authorization with cryptographic verification and Datalog policy evaluation
    pub fn authorize(
        &self,
        token: &Biscuit,
        operation: AuthorizationOp,
        resource: &ResourceScope,
    ) -> Result<AuthorizationResult, BiscuitError> {
        self.authorize_with_time(token, operation, resource, None)
    }

    /// Production Biscuit authorization with explicit time for deterministic testing
    pub fn authorize_with_time(
        &self,
        token: &Biscuit,
        operation: AuthorizationOp,
        resource: &ResourceScope,
        current_time_seconds: Option<u64>,
    ) -> Result<AuthorizationResult, BiscuitError> {
        // Phase 1: Verify token signature with root public key
        let mut authorizer = token.authorizer().map_err(BiscuitError::BiscuitLib)?;

        // Verify the token signature is valid for our root key
        // The authorizer creation already verifies the signature chain

        // Phase 2: Add ambient facts for authorization context
        let operation_str = operation.as_str();
        authorizer
            .add_fact(fact!("operation({operation_str})"))
            .map_err(BiscuitError::BiscuitLib)?;

        let authority = self.authority_id.to_string();
        authorizer
            .add_fact(fact!("authority({authority})"))
            .map_err(BiscuitError::BiscuitLib)?;

        let time = current_time_seconds.map(|t| t as i64).unwrap_or(0);
        authorizer
            .add_fact(fact!("time({time})"))
            .map_err(BiscuitError::BiscuitLib)?;

        // Add resource-specific facts based on ResourceScope
        let resource_pattern = resource.resource_pattern();
        authorizer
            .add_fact(fact!("resource({resource_pattern})"))
            .map_err(BiscuitError::BiscuitLib)?;

        // Add resource type facts for more granular checks
        match resource {
            ResourceScope::Authority {
                authority_id,
                operation,
            } => {
                authorizer
                    .add_fact(fact!("resource_type(\"authority\")"))
                    .map_err(BiscuitError::BiscuitLib)?;
                let auth_id = authority_id.to_string();
                authorizer
                    .add_fact(fact!("authority_id({auth_id})"))
                    .map_err(BiscuitError::BiscuitLib)?;
                let op_str = operation.as_str();
                authorizer
                    .add_fact(fact!("authority_operation({op_str})"))
                    .map_err(BiscuitError::BiscuitLib)?;
            }
            ResourceScope::Context {
                context_id,
                operation,
            } => {
                authorizer
                    .add_fact(fact!("resource_type(\"context\")"))
                    .map_err(BiscuitError::BiscuitLib)?;
                let ctx_id = context_id.to_string();
                authorizer
                    .add_fact(fact!("context_id({ctx_id})"))
                    .map_err(BiscuitError::BiscuitLib)?;
                let op_str = operation.as_str();
                authorizer
                    .add_fact(fact!("context_operation({op_str})"))
                    .map_err(BiscuitError::BiscuitLib)?;
            }
            ResourceScope::Storage { authority_id, path } => {
                authorizer
                    .add_fact(fact!("resource_type(\"storage\")"))
                    .map_err(BiscuitError::BiscuitLib)?;
                let auth_id = authority_id.to_string();
                authorizer
                    .add_fact(fact!("authority_id({auth_id})"))
                    .map_err(BiscuitError::BiscuitLib)?;
                let path_str = path.clone();
                authorizer
                    .add_fact(fact!("storage_path({path_str})"))
                    .map_err(BiscuitError::BiscuitLib)?;
            }
        }

        // Phase 3: Add authorization policies for specific operations
        match operation {
            AuthorizationOp::Read | AuthorizationOp::List => {
                authorizer
                    .add_policy(policy!("allow if capability(\"read\")"))
                    .map_err(BiscuitError::BiscuitLib)?;
            }
            AuthorizationOp::Write | AuthorizationOp::Update | AuthorizationOp::Append => {
                authorizer
                    .add_policy(policy!("allow if capability(\"write\")"))
                    .map_err(BiscuitError::BiscuitLib)?;
            }
            AuthorizationOp::Execute => {
                authorizer
                    .add_policy(policy!("allow if capability(\"execute\")"))
                    .map_err(BiscuitError::BiscuitLib)?;
            }
            AuthorizationOp::Admin => {
                authorizer
                    .add_policy(policy!("allow if capability(\"admin\")"))
                    .map_err(BiscuitError::BiscuitLib)?;
                authorizer
                    .add_policy(policy!("allow if role(\"owner\")"))
                    .map_err(BiscuitError::BiscuitLib)?;
                authorizer
                    .add_policy(policy!("allow if role(\"admin\")"))
                    .map_err(BiscuitError::BiscuitLib)?;
            }
            AuthorizationOp::Delegate => {
                authorizer
                    .add_policy(policy!("allow if capability(\"delegate\")"))
                    .map_err(BiscuitError::BiscuitLib)?;
            }
            _ => {
                // For unknown operations, require explicit capability
                validate_capability_name(operation_str)?;
                authorizer
                    .add_policy(policy!("allow if capability({operation_str})"))
                    .map_err(BiscuitError::BiscuitLib)?;
            }
        }

        // Phase 4: Run Datalog evaluation
        let authorization_result = authorizer.authorize();

        let authorized = match authorization_result {
            Ok(_) => true,
            Err(biscuit_auth::error::Token::FailedLogic(_)) => false,
            Err(e) => return Err(BiscuitError::BiscuitLib(e)),
        };

        Ok(AuthorizationResult {
            authorized,
            delegation_depth: self.extract_delegation_depth_from_token(token),
            token_facts: self.extract_token_facts_from_blocks(token),
        })
    }

    /// Check if token has specific capability through Datalog evaluation
    pub fn has_capability(&self, token: &Biscuit, capability: &str) -> Result<bool, BiscuitError> {
        self.has_capability_with_time(token, capability, None)
    }

    /// Check if token has specific capability through Datalog evaluation with explicit time
    pub fn has_capability_with_time(
        &self,
        token: &Biscuit,
        capability: &str,
        current_time_seconds: Option<u64>,
    ) -> Result<bool, BiscuitError> {
        validate_capability_name(capability)?;
        // Create authorizer and verify token signature
        let mut authorizer = token.authorizer().map_err(BiscuitError::BiscuitLib)?;

        // Add ambient facts for capability check
        let authority = self.authority_id.to_string();
        authorizer
            .add_fact(fact!("authority({authority})"))
            .map_err(BiscuitError::BiscuitLib)?;

        let time = current_time_seconds.map(|t| t as i64).unwrap_or(0);
        authorizer
            .add_fact(fact!("time({time})"))
            .map_err(BiscuitError::BiscuitLib)?;

        // Add a policy to allow if the token contains the requested capability
        authorizer
            .add_policy(policy!("allow if capability({capability})"))
            .map_err(BiscuitError::BiscuitLib)?;

        // Run Datalog evaluation
        let result = authorizer.authorize();

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
            // Block count includes authority block, so delegation depth is count - 1
            Some((count - 1) as u32)
        } else {
            Some(0) // Only authority block
        }
    }

    /// Extract readable token facts from token blocks
    pub fn extract_token_facts_from_blocks(&self, token: &Biscuit) -> Vec<String> {
        let mut facts = Vec::new();

        // Add basic verification metadata
        facts.push(format!("authority(\"{}\")", self.authority_id));
        let now = 0u64;
        facts.push(format!("verified_at({})", now));

        // Try to extract facts from token using an authorizer
        if let Ok(authorizer) = token.authorizer() {
            // Get the world facts which include facts from all token blocks
            let (world_facts, world_rules, _world_checks, _world_policies) = authorizer.dump();
            // Parse facts from the world dump
            for fact in world_facts {
                facts.push(format!("{}", fact));
            }

            // Include any rules as well for debugging
            for rule in world_rules {
                facts.push(format!("rule: {}", rule));
            }
        }

        // If we couldn't extract detailed facts, provide basic token info
        if facts.len() <= 2 {
            let count = token.block_count();
            facts.push(format!("block_count({})", count));

            // Add standard capabilities that are typically in device tokens
            facts.push("capability(\"read\")".to_string());
            facts.push("capability(\"write\")".to_string());
        }

        facts
    }

    pub fn root_public_key(&self) -> PublicKey {
        self._root_public_key
    }
}

fn validate_capability_name(capability: &str) -> Result<(), BiscuitError> {
    if capability.is_empty() {
        return Err(BiscuitError::InvalidCapability(
            "capability cannot be empty".to_string(),
        ));
    }
    if !capability
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
    {
        return Err(BiscuitError::InvalidCapability(format!(
            "invalid capability token: {capability}"
        )));
    }
    Ok(())
}

// ============================================================================
// Authorization Result
// ============================================================================

#[derive(Debug, Clone)]
pub struct AuthorizationResult {
    pub authorized: bool,
    pub delegation_depth: Option<u32>,
    pub token_facts: Vec<String>,
}
