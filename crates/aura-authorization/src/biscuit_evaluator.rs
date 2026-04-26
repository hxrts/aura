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

pub const AURA_BISCUIT_LIMITS: AuthorizerLimits = AuthorizerLimits {
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

    /// Production Biscuit authorization with explicit time and pre-verified token evidence.
    pub fn authorize_with_time(
        &self,
        token: &VerifiedBiscuitToken,
        operation: AuthorizationOp,
        resource: &ResourceScope,
        current_time_seconds: Option<u64>,
    ) -> Result<AuthorizationResult, BiscuitError> {
        self.authorize_with_time_and_limits(
            token,
            operation,
            resource,
            current_time_seconds,
            AURA_BISCUIT_LIMITS,
        )
    }

    /// Check if a verified token has a specific capability through Datalog evaluation.
    pub fn has_capability_with_time(
        &self,
        token: &VerifiedBiscuitToken,
        capability: &str,
        current_time_seconds: Option<u64>,
    ) -> Result<bool, BiscuitError> {
        self.has_capability_with_time_and_limits(
            token,
            capability,
            current_time_seconds,
            AURA_BISCUIT_LIMITS,
        )
    }

    fn authorize_with_time_and_limits(
        &self,
        token: &VerifiedBiscuitToken,
        operation: AuthorizationOp,
        resource: &ResourceScope,
        current_time_seconds: Option<u64>,
        limits: AuthorizerLimits,
    ) -> Result<AuthorizationResult, BiscuitError> {
        let current_time_seconds = require_time(current_time_seconds)?;
        let operation_name =
            CapabilityName::parse(operation.as_str()).map_err(invalid_capability_error)?;
        let operation_str = operation_name.as_str();
        let mut authorizer = token.authorizer()?;
        self.add_operation_authority_time_facts(
            &mut authorizer,
            operation_str,
            current_time_seconds,
        )?;
        self.add_resource_facts(&mut authorizer, resource)?;
        self.add_scoped_operation_policy(&mut authorizer, operation, operation_str, resource)?;

        let authorized = match authorizer.authorize_with_limits(limits) {
            Ok(_) => true,
            Err(biscuit_auth::error::Token::FailedLogic(_)) => {
                tracing::debug!(
                    operation = operation_str,
                    resource = %resource.resource_pattern(),
                    "Biscuit authorization denied by policy"
                );
                false
            }
            Err(e @ biscuit_auth::error::Token::RunLimit(_)) => {
                tracing::warn!(
                    operation = operation_str,
                    resource = %resource.resource_pattern(),
                    error = %e,
                    "Biscuit authorization denied by resource limit"
                );
                return Err(BiscuitError::BiscuitLib(e));
            }
            Err(e) => return Err(BiscuitError::BiscuitLib(e)),
        };

        Ok(AuthorizationResult {
            authorized,
            delegation_depth: self.extract_delegation_depth_from_token(token.token()),
            token_facts: self.extract_diagnostic_token_facts(token),
        })
    }

    fn has_capability_with_time_and_limits(
        &self,
        token: &VerifiedBiscuitToken,
        capability: &str,
        current_time_seconds: Option<u64>,
        limits: AuthorizerLimits,
    ) -> Result<bool, BiscuitError> {
        let current_time_seconds = require_time(current_time_seconds)?;
        let capability_name =
            CapabilityName::parse(capability).map_err(invalid_capability_error)?;
        let capability = capability_name.as_str();
        let mut authorizer = token.authorizer()?;

        self.add_authority_and_time_facts(&mut authorizer, current_time_seconds)?;
        Self::add_policy(
            &mut authorizer,
            policy!("allow if capability({capability})"),
        )?;

        match authorizer.authorize_with_limits(limits) {
            Ok(_) => Ok(true),
            Err(biscuit_auth::error::Token::FailedLogic(_)) => {
                tracing::debug!(capability, "Biscuit capability denied by policy");
                Ok(false)
            }
            Err(e @ biscuit_auth::error::Token::RunLimit(_)) => {
                tracing::warn!(capability, error = %e, "Biscuit capability denied by resource limit");
                Err(BiscuitError::BiscuitLib(e))
            }
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

    fn add_scoped_operation_policy(
        &self,
        authorizer: &mut biscuit_auth::Authorizer,
        operation: AuthorizationOp,
        operation_str: &str,
        resource: &ResourceScope,
    ) -> Result<(), BiscuitError> {
        let mut add_scoped_capability_policy =
            |capability: &str| self.add_scoped_capability_policy(authorizer, capability, resource);
        match operation {
            AuthorizationOp::Read | AuthorizationOp::List => add_scoped_capability_policy("read"),
            AuthorizationOp::Write | AuthorizationOp::Update | AuthorizationOp::Append => {
                add_scoped_capability_policy("write")
            }
            AuthorizationOp::Execute => add_scoped_capability_policy("execute"),
            AuthorizationOp::Admin => self.add_scoped_admin_policy(authorizer, resource),
            AuthorizationOp::Delegate => add_scoped_capability_policy("delegate"),
            _ => {
                // For unknown operations, require explicit capability.
                add_scoped_capability_policy(operation_str)
            }
        }
    }

    fn add_scoped_capability_policy(
        &self,
        authorizer: &mut biscuit_auth::Authorizer,
        capability: &str,
        resource: &ResourceScope,
    ) -> Result<(), BiscuitError> {
        match resource {
            ResourceScope::Authority { authority_id, .. } => {
                let auth_id = authority_id.to_string();
                Self::add_policy(
                    authorizer,
                    policy!(
                        "allow if capability({capability}), scope_authority({auth_id}), authority_id({auth_id})"
                    ),
                )
            }
            ResourceScope::Context { context_id, .. } => {
                let ctx_id = context_id.to_string();
                let authority_scope_ctx_id = ctx_id.clone();
                let authority = self.authority_id.to_string();
                Self::add_policy(
                    authorizer,
                    policy!(
                        "allow if capability({capability}), scope_context({ctx_id}), context_id({ctx_id})"
                    ),
                )?;
                Self::add_policy(
                    authorizer,
                    policy!(
                        "allow if capability({capability}), scope_authority_contexts({authority}), authority({authority}), context_id({authority_scope_ctx_id})"
                    ),
                )
            }
            ResourceScope::Storage { authority_id, path } => {
                let auth_id = authority_id.to_string();
                let path_str = path.as_str();
                Self::add_policy(
                    authorizer,
                    policy!(
                        "allow if capability({capability}), scope_authority({auth_id}), authority_id({auth_id}), scope_storage_path({path_str}), storage_path({path_str})"
                    ),
                )
            }
        }
    }

    fn add_scoped_admin_policy(
        &self,
        authorizer: &mut biscuit_auth::Authorizer,
        resource: &ResourceScope,
    ) -> Result<(), BiscuitError> {
        match resource {
            ResourceScope::Authority { authority_id, .. } => {
                let auth_id = authority_id.to_string();
                let moderator_auth_id = auth_id.clone();
                Self::add_policy(
                    authorizer,
                    policy!(
                        "allow if capability(\"admin\"), role(\"member\"), scope_authority({auth_id}), authority_id({auth_id})"
                    ),
                )?;
                Self::add_policy(
                    authorizer,
                    policy!(
                        "allow if capability(\"admin\"), role(\"moderator\"), scope_authority({moderator_auth_id}), authority_id({moderator_auth_id})"
                    ),
                )
            }
            ResourceScope::Context { context_id, .. } => {
                let ctx_id = context_id.to_string();
                let moderator_ctx_id = ctx_id.clone();
                let authority_ctx_id = ctx_id.clone();
                let moderator_authority_ctx_id = ctx_id.clone();
                let authority = self.authority_id.to_string();
                let moderator_authority = authority.clone();
                Self::add_policy(
                    authorizer,
                    policy!(
                        "allow if capability(\"admin\"), role(\"member\"), scope_context({ctx_id}), context_id({ctx_id})"
                    ),
                )?;
                Self::add_policy(
                    authorizer,
                    policy!(
                        "allow if capability(\"admin\"), role(\"moderator\"), scope_context({moderator_ctx_id}), context_id({moderator_ctx_id})"
                    ),
                )?;
                Self::add_policy(
                    authorizer,
                    policy!(
                        "allow if capability(\"admin\"), role(\"member\"), scope_authority_contexts({authority}), authority({authority}), context_id({authority_ctx_id})"
                    ),
                )?;
                Self::add_policy(
                    authorizer,
                    policy!(
                        "allow if capability(\"admin\"), role(\"moderator\"), scope_authority_contexts({moderator_authority}), authority({moderator_authority}), context_id({moderator_authority_ctx_id})"
                    ),
                )
            }
            ResourceScope::Storage { authority_id, path } => {
                let auth_id = authority_id.to_string();
                let path_str = path.as_str();
                let moderator_auth_id = auth_id.clone();
                let moderator_path_str = path_str.to_string();
                Self::add_policy(
                    authorizer,
                    policy!(
                        "allow if capability(\"admin\"), role(\"member\"), scope_authority({auth_id}), authority_id({auth_id}), scope_storage_path({path_str}), storage_path({path_str})"
                    ),
                )?;
                Self::add_policy(
                    authorizer,
                    policy!(
                        "allow if capability(\"admin\"), role(\"moderator\"), scope_authority({moderator_auth_id}), authority_id({moderator_auth_id}), scope_storage_path({moderator_path_str}), storage_path({moderator_path_str})"
                    ),
                )
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

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::types::scope::{AuthorityOp, AuthorizationOp};
    use biscuit_auth::{builder::BiscuitBuilder, error::RunLimit, KeyPair};

    fn test_bridge(keypair: &KeyPair) -> BiscuitAuthorizationBridge {
        BiscuitAuthorizationBridge::new(keypair.public(), AuthorityId::new_from_entropy([7; 32]))
    }

    fn test_scope() -> ResourceScope {
        ResourceScope::Authority {
            authority_id: AuthorityId::new_from_entropy([8; 32]),
            operation: AuthorityOp::UpdateTree,
        }
    }

    fn token_with_many_facts(count: usize) -> (KeyPair, VerifiedBiscuitToken) {
        let keypair = KeyPair::new();
        let mut builder = BiscuitBuilder::new();
        builder
            .add_fact(fact!("capability(\"read\")"))
            .expect("add capability");
        for index in 0..count {
            let index = index as i64;
            builder
                .add_fact(fact!("spam({index})"))
                .unwrap_or_else(|err| panic!("add spam fact {index}: {err:?}"));
        }
        let token = builder.build(&keypair).expect("build token");
        let verified = VerifiedBiscuitToken::from_token(&token, keypair.public()).expect("verify");
        (keypair, verified)
    }

    fn token_with_iteration_chain(steps: usize) -> (KeyPair, VerifiedBiscuitToken) {
        let keypair = KeyPair::new();
        let mut builder = BiscuitBuilder::new();
        builder
            .add_fact(fact!("capability(\"read\")"))
            .expect("add capability");
        builder.add_fact(fact!("chain(0)")).expect("add chain seed");
        for step in 0..steps {
            let rule = format!("chain({}) <- chain({step})", step + 1);
            builder
                .add_rule(rule.as_str())
                .unwrap_or_else(|err| panic!("add chain rule {step}: {err:?}"));
        }
        let token = builder.build(&keypair).expect("build token");
        let verified = VerifiedBiscuitToken::from_token(&token, keypair.public()).expect("verify");
        (keypair, verified)
    }

    #[test]
    fn authorize_with_limits_rejects_tokens_that_exceed_fact_budget() {
        let (keypair, verified) = token_with_iteration_chain(32);
        let bridge = test_bridge(&keypair);
        let error = bridge
            .authorize_with_time_and_limits(
                &verified,
                AuthorizationOp::Read,
                &test_scope(),
                Some(1_000),
                AuthorizerLimits {
                    max_facts: 8,
                    max_iterations: 64,
                    max_time: Duration::from_secs(1),
                },
            )
            .expect_err("fact-budget token must fail closed");

        assert!(matches!(
            error,
            BiscuitError::BiscuitLib(biscuit_auth::error::Token::RunLimit(RunLimit::TooManyFacts))
        ));
    }

    #[test]
    fn capability_checks_reject_tokens_that_exceed_iteration_budget() {
        let (keypair, verified) = token_with_iteration_chain(32);
        let bridge = test_bridge(&keypair);
        let error = bridge
            .has_capability_with_time_and_limits(
                &verified,
                "read",
                Some(1_000),
                AuthorizerLimits {
                    max_facts: AURA_BISCUIT_LIMITS.max_facts,
                    max_iterations: 8,
                    max_time: Duration::from_secs(1),
                },
            )
            .expect_err("iteration-budget token must fail closed");

        assert!(matches!(
            error,
            BiscuitError::BiscuitLib(biscuit_auth::error::Token::RunLimit(
                RunLimit::TooManyIterations
            ))
        ));
    }

    #[test]
    fn authorize_with_limits_rejects_timeout_budget_exhaustion() {
        let (keypair, verified) = token_with_many_facts(1);
        let bridge = test_bridge(&keypair);
        let error = bridge
            .authorize_with_time_and_limits(
                &verified,
                AuthorizationOp::Read,
                &test_scope(),
                Some(1_000),
                AuthorizerLimits {
                    max_facts: AURA_BISCUIT_LIMITS.max_facts,
                    max_iterations: AURA_BISCUIT_LIMITS.max_iterations,
                    max_time: Duration::ZERO,
                },
            )
            .expect_err("zero-time budget must fail closed");

        assert!(matches!(
            error,
            BiscuitError::BiscuitLib(biscuit_auth::error::Token::RunLimit(RunLimit::Timeout))
        ));
    }
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
