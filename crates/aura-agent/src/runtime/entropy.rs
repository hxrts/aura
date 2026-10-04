//! Checked execution provenance for runtime deterministic entropy.
use aura_core::AuraError;
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
#[error("production runtime cannot admit a deterministic entropy seed")]
pub(crate) struct ProductionSeededEntropyError;

/// Private seed custody exists only after checking actual execution mode.
pub(super) struct NonProductionEntropySeed([u8; 32]);
impl NonProductionEntropySeed {
    #[aura_macros::capability_boundary(
        category = "capability_gated", capability = "NonProductionEntropySeed",
        capability_type = NonProductionEntropySeed, family = "proof_issuer"
    )]
    #[aura_macros::authoritative_source(kind = "proof_issuer")]
    pub(super) fn admit(
        mode: aura_core::effects::ExecutionMode,
        seed: Option<[u8; 32]>,
    ) -> Result<Option<NonProductionEntropySeed>, AuraError> {
        if mode.is_production() && seed.is_some() {
            return Err(AuraError::Invalid {
                message: "production deterministic entropy admission denied".into(),
                source: Some(Arc::new(ProductionSeededEntropyError)),
            });
        }
        Ok(seed.map(Self))
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated", capability = "NonProductionEntropySeed",
        capability_type = NonProductionEntropySeed, receiver_type = NonProductionEntropySeed,
        family = "runtime_helper"
    )]
    pub(super) fn crypto_handler(&self) -> aura_effects::crypto::RealCryptoHandler {
        aura_effects::crypto::RealCryptoHandler::for_simulation_seed(self.0)
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated", capability = "NonProductionEntropySeed",
        capability_type = NonProductionEntropySeed, receiver_type = NonProductionEntropySeed,
        family = "runtime_helper"
    )]
    pub(super) fn random_stream(&self) -> super::subsystems::crypto::CryptoRng {
        use rand::SeedableRng;
        super::subsystems::crypto::CryptoRng::deterministic(rand::rngs::StdRng::from_seed(self.0))
    }
}
