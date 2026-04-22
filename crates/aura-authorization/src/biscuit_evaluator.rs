//! Layer 2: Biscuit Cryptographic Authorization Evaluation
//!
//! Cryptographic authorization infrastructure for Aura's Web of Trust system using Biscuit tokens.
//! Biscuit provides attenuated, cryptographically-verifiable capabilities with Datalog policy evaluation.
//!
//! **Key Components** (per docs/106_authorization.md):
//! - **BiscuitAuthorizationBridge**: Token generation, attenuation, verification
//! - **Datalog Policy Evaluation**: Logic programming for fine-grained authorization
//! - **Capability Attenuation**: Restrict tokens to specific scope/time/resources
//!
//! **Integration Point**: CapGuard in aura-protocol/guards evaluates Biscuit tokens at message
//! entry point (first guard in chain); enables delegation without trusted intermediaries.

use crate::BiscuitError;
use aura_core::types::scope::{AuthorizationOp, ResourceScope};
use aura_core::{types::identifiers::AuthorityId, CapabilityName, CapabilityNameError};
use biscuit_auth::{macros::*, AuthorizerLimits, Biscuit, PublicKey};
use std::time::Duration;

const AURA_BISCUIT_LIMITS: AuthorizerLimits = AuthorizerLimits {
    max_facts: 10_000,
    max_iterations: 1_000,
    max_time: Duration::from_millis(50),
};

/// Biscuit token that has been reparsed under the configured authority root key.
#[derive(Clone, Debug)]
pub struct VerifiedBiscuitToken {
    token: Biscuit,
}

impl VerifiedBiscuitToken {
    /// Verify serialized Biscuit bytes against the configured root public key.
    pub fn from_bytes(bytes: &[u8], root_public_key: PublicKey) -> Result<Self, BiscuitError> {
        let token = Biscuit::from(bytes, root_public_key).map_err(BiscuitError::BiscuitLib)?;
        Ok(Self { token })
    }

    /// Re-serialize and reparse an existing Biscuit under the configured root
    /// public key before it can be used for authorization decisions.
    pub fn from_token(token: &Biscuit, root_public_key: PublicKey) -> Result<Self, BiscuitError> {
        let bytes = token.to_vec().map_err(BiscuitError::BiscuitLib)?;
        Self::from_bytes(&bytes, root_public_key)
    }

    /// Borrow the verified Biscuit token.
    #[must_use]
    pub fn token(&self) -> &Biscuit {
        &self.token
    }

    /// Create an authorizer from the verified token.
    pub fn authorizer(&self) -> Result<biscuit_auth::Authorizer, BiscuitError> {
        self.token.authorizer().map_err(BiscuitError::BiscuitLib)
    }
}

// ============================================================================
// Biscuit Authorization Bridge
// ============================================================================

#[derive(Clone, Debug)]
pub struct BiscuitAuthorizationBridge {
    root_public_key: PublicKey,
    authority_id: AuthorityId,
}

impl BiscuitAuthorizationBridge {
    pub fn new(root_public_key: PublicKey, authority_id: AuthorityId) -> Self {
        Self {
            root_public_key,
            authority_id,
        }
    }

    #[cfg(test)]
    fn test_bridge() -> Self {
        use biscuit_auth::KeyPair;
        let keypair = KeyPair::new();
        Self {
            root_public_key: keypair.public(),
            authority_id: AuthorityId::new_from_entropy(aura_core::hash::hash(
                &keypair.public().to_bytes(),
            )),
        }
    }

    /// Create a mock bridge for testing with a generated keypair
    #[cfg(test)]
    pub fn new_mock() -> Self {
        Self::test_bridge()
    }

    /// Production Biscuit authorization with cryptographic verification and Datalog policy evaluation
    ///
    /// Callers must supply time via `authorize_with_time`; omitting it fails closed.
    pub fn authorize(
        &self,
        token: &Biscuit,
        operation: AuthorizationOp,
        resource: &ResourceScope,
    ) -> Result<AuthorizationResult, BiscuitError> {
        self.authorize_with_time(token, operation, resource, None)
    }

    /// Production Biscuit authorization with explicit time for deterministic testing and expiry checks.
    pub fn authorize_with_time(
        &self,
        token: &Biscuit,
        operation: AuthorizationOp,
        resource: &ResourceScope,
        current_time_seconds: Option<u64>,
    ) -> Result<AuthorizationResult, BiscuitError> {
        let verified_token = VerifiedBiscuitToken::from_token(token, self.root_public_key)?;
        self.authorize_verified_with_time(
            &verified_token,
            operation,
            resource,
            current_time_seconds,
        )
    }

    /// Authorize a token that has already passed root-key verification.
    pub fn authorize_verified_with_time(
        &self,
        token: &VerifiedBiscuitToken,
        operation: AuthorizationOp,
        resource: &ResourceScope,
        current_time_seconds: Option<u64>,
    ) -> Result<AuthorizationResult, BiscuitError> {
        let current_time_seconds = require_time(current_time_seconds)?;
        let mut authorizer = token.authorizer()?;
        // Phase 2: Add ambient facts for authorization context
        let operation_name =
            CapabilityName::parse(operation.as_str()).map_err(invalid_capability_error)?;
        let operation_str = operation_name.as_str();
        self.add_operation_authority_time_facts(
            &mut authorizer,
            operation_str,
            current_time_seconds,
        )?;

        // Add resource-specific facts based on ResourceScope
        self.add_resource_facts(&mut authorizer, resource)?;

        // Phase 3: Add authorization policies for specific operations
        self.add_operation_policies(&mut authorizer, operation, operation_str)?;

        // Phase 4: Run Datalog evaluation
        let authorization_result = authorizer.authorize_with_limits(AURA_BISCUIT_LIMITS);

        let authorized = match authorization_result {
            Ok(_) => true,
            Err(biscuit_auth::error::Token::FailedLogic(_)) => false,
            Err(e) => return Err(BiscuitError::BiscuitLib(e)),
        };

        Ok(AuthorizationResult {
            authorized,
            delegation_depth: self.extract_delegation_depth_from_token(token.token()),
            token_facts: self.extract_diagnostic_token_facts(token),
        })
    }

    /// Check if token has specific capability through Datalog evaluation
    ///
    /// Callers must supply time via `has_capability_with_time`; omitting it fails closed.
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
        let verified_token = VerifiedBiscuitToken::from_token(token, self.root_public_key)?;
        self.has_verified_capability_with_time(&verified_token, capability, current_time_seconds)
    }

    /// Check if a verified token has a specific capability through Datalog evaluation.
    pub fn has_verified_capability_with_time(
        &self,
        token: &VerifiedBiscuitToken,
        capability: &str,
        current_time_seconds: Option<u64>,
    ) -> Result<bool, BiscuitError> {
        let current_time_seconds = require_time(current_time_seconds)?;
        let capability_name =
            CapabilityName::parse(capability).map_err(invalid_capability_error)?;
        let capability = capability_name.as_str();
        let mut authorizer = token.authorizer()?;

        // Add ambient facts for capability check
        self.add_authority_and_time_facts(&mut authorizer, current_time_seconds)?;

        // Add a policy to allow if the token contains the requested capability
        Self::add_policy(
            &mut authorizer,
            policy!("allow if capability({capability})"),
        )?;

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
    pub fn extract_diagnostic_token_facts(&self, token: &VerifiedBiscuitToken) -> Vec<String> {
        let mut facts = Vec::new();

        // Add basic verification metadata
        facts.push(format!("authority(\"{}\")", self.authority_id));
        let now = 0u64;
        facts.push(format!("verified_at({})", now));

        // Try to extract facts from the verified token using an authorizer.
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
            let count = token.token().block_count();
            facts.push(format!("block_count({})", count));
        }

        facts
    }

    pub fn root_public_key(&self) -> PublicKey {
        self.root_public_key
    }

    pub fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }

    fn add_operation_authority_time_facts(
        &self,
        authorizer: &mut biscuit_auth::Authorizer,
        operation_str: &str,
        current_time_seconds: u64,
    ) -> Result<(), BiscuitError> {
        Self::add_fact(authorizer, fact!("operation({operation_str})"))?;
        self.add_authority_and_time_facts(authorizer, current_time_seconds)
    }

    fn add_authority_and_time_facts(
        &self,
        authorizer: &mut biscuit_auth::Authorizer,
        current_time_seconds: u64,
    ) -> Result<(), BiscuitError> {
        let authority = self.authority_id.to_string();
        let time = current_time_seconds as i64;
        Self::add_fact(authorizer, fact!("authority({authority})"))?;
        Self::add_fact(authorizer, fact!("time({time})"))
    }

    fn add_resource_facts(
        &self,
        authorizer: &mut biscuit_auth::Authorizer,
        resource: &ResourceScope,
    ) -> Result<(), BiscuitError> {
        let resource_pattern = resource.resource_pattern();
        Self::add_fact(authorizer, fact!("resource({resource_pattern})"))?;

        match resource {
            ResourceScope::Authority {
                authority_id,
                operation,
            } => {
                let auth_id = authority_id.to_string();
                let op_str = operation.as_str();
                Self::add_fact(authorizer, fact!("resource_type(\"authority\")"))?;
                Self::add_fact(authorizer, fact!("authority_id({auth_id})"))?;
                Self::add_fact(authorizer, fact!("authority_operation({op_str})"))
            }
            ResourceScope::Context {
                context_id,
                operation,
            } => {
                let ctx_id = context_id.to_string();
                let op_str = operation.as_str();
                Self::add_fact(authorizer, fact!("resource_type(\"context\")"))?;
                Self::add_fact(authorizer, fact!("context_id({ctx_id})"))?;
                Self::add_fact(authorizer, fact!("context_operation({op_str})"))
            }
            ResourceScope::Storage { authority_id, path } => {
                let auth_id = authority_id.to_string();
                let path_str = path.as_str();
                Self::add_fact(authorizer, fact!("resource_type(\"storage\")"))?;
                Self::add_fact(authorizer, fact!("authority_id({auth_id})"))?;
                Self::add_fact(authorizer, fact!("storage_path({path_str})"))
            }
        }
    }

    fn add_operation_policies(
        &self,
        authorizer: &mut biscuit_auth::Authorizer,
        operation: AuthorizationOp,
        operation_str: &str,
    ) -> Result<(), BiscuitError> {
        match operation {
            AuthorizationOp::Read | AuthorizationOp::List => {
                Self::add_policy(authorizer, policy!("allow if capability(\"read\")"))
            }
            AuthorizationOp::Write | AuthorizationOp::Update | AuthorizationOp::Append => {
                Self::add_policy(authorizer, policy!("allow if capability(\"write\")"))
            }
            AuthorizationOp::Execute => {
                Self::add_policy(authorizer, policy!("allow if capability(\"execute\")"))
            }
            AuthorizationOp::Admin => {
                Self::add_policy(authorizer, policy!("allow if capability(\"admin\")"))?;
                Self::add_policy(authorizer, policy!("allow if role(\"member\")"))?;
                Self::add_policy(authorizer, policy!("allow if role(\"moderator\")"))
            }
            AuthorizationOp::Delegate => {
                Self::add_policy(authorizer, policy!("allow if capability(\"delegate\")"))
            }
            _ => {
                // For unknown operations, require explicit capability.
                Self::add_policy(authorizer, policy!("allow if capability({operation_str})"))
            }
        }
    }

    fn add_fact(
        authorizer: &mut biscuit_auth::Authorizer,
        fact: biscuit_auth::builder::Fact,
    ) -> Result<(), BiscuitError> {
        authorizer.add_fact(fact).map_err(BiscuitError::BiscuitLib)
    }

    fn add_policy(
        authorizer: &mut biscuit_auth::Authorizer,
        policy: biscuit_auth::builder::Policy,
    ) -> Result<(), BiscuitError> {
        authorizer
            .add_policy(policy)
            .map_err(BiscuitError::BiscuitLib)
    }
}

fn invalid_capability_error(error: CapabilityNameError) -> BiscuitError {
    BiscuitError::InvalidCapability(error.to_string())
}

fn require_time(current_time_seconds: Option<u64>) -> Result<u64, BiscuitError> {
    current_time_seconds.ok_or(BiscuitError::TimeRequired)
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
