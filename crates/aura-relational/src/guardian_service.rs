//! Guardian service implementing GuardianEffects using relational contexts
//! with consensus-backed GuardianBinding facts.

use crate::guardian_request::{
    make_guardian_cancel_fact, make_guardian_request_fact, GuardianRequestPayload,
};
use crate::{run_consensus_with_config, ConsensusConfig, RelationalContext};
use async_trait::async_trait;
use aura_core::effects::guardian::{GuardianAcceptInput, GuardianEffects, GuardianRequestInput};
use aura_core::relational::fact::RelationalFact;
use aura_core::relational::GuardianBinding;
use aura_core::{AuraError, Prestate, Result};
use std::sync::Arc;

/// Guardian service backed by a RelationalContext
pub struct GuardianService {
    /// Underlying relational context storage
    ctx_provider: Arc<dyn Fn() -> Result<Arc<RelationalContext>> + Send + Sync>,
    /// Consensus configuration
    consensus_config: ConsensusConfig,
}

impl GuardianService {
    pub fn new(
        ctx_provider: Arc<dyn Fn() -> Result<Arc<RelationalContext>> + Send + Sync>,
        consensus_config: ConsensusConfig,
    ) -> Self {
        Self {
            ctx_provider,
            consensus_config,
        }
    }

    fn context(&self) -> Result<Arc<RelationalContext>> {
        (self.ctx_provider)()
    }
}

#[async_trait]
impl GuardianEffects for GuardianService {
    async fn request_guardian(&self, input: GuardianRequestInput) -> Result<()> {
        let ctx = self.context()?;

        let payload = GuardianRequestPayload {
            account_commitment: input.account_commitment,
            guardian_commitment: input.guardian_commitment,
            requester: input.account,
            parameters: input.parameters.clone(),
            requested_at: aura_core::time::TimeStamp::PhysicalClock(input.requested_at),
            expires_at: input
                .expires_at
                .map(aura_core::time::TimeStamp::PhysicalClock),
        };

        let fact = make_guardian_request_fact(payload)
            .map_err(|e| AuraError::serialization(e.to_string()))?;
        ctx.add_fact(fact)
    }

    async fn cancel_guardian_request(&self, input: GuardianRequestInput) -> Result<()> {
        let ctx = self.context()?;
        let payload = GuardianRequestPayload {
            account_commitment: input.account_commitment,
            guardian_commitment: input.guardian_commitment,
            requester: input.account,
            parameters: input.parameters.clone(),
            requested_at: aura_core::time::TimeStamp::PhysicalClock(input.requested_at),
            expires_at: input
                .expires_at
                .map(aura_core::time::TimeStamp::PhysicalClock),
        };

        let fact = make_guardian_cancel_fact(payload)
            .map_err(|e| AuraError::serialization(e.to_string()))?;
        ctx.add_fact(fact)
    }

    async fn accept_guardian_request(&self, input: GuardianAcceptInput) -> Result<GuardianBinding> {
        let ctx = self.context()?;

        // Prepare prestate
        let authority_commitments = vec![
            (input.account, input.account_commitment),
            (input.guardian, input.guardian_commitment),
        ];

        let context_commitment = ctx.journal_commitment();
        let prestate = Prestate::new(authority_commitments, context_commitment);

        // Run consensus for GuardianBinding
        let consensus_proof = run_consensus_with_config(
            &prestate,
            &input.parameters,
            self.consensus_config.clone(),
            input.key_packages.clone(),
            input.group_public_key.clone(),
        )
        .await?;

        let binding = GuardianBinding::with_consensus_proof(
            input.account_commitment,
            input.guardian_commitment,
            input.parameters.clone(),
            consensus_proof,
        );

        ctx.add_fact(RelationalFact::GuardianBinding(binding.clone()))?;

        Ok(binding)
    }
}
