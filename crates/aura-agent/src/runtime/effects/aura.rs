use super::AuraEffectSystem;
use async_trait::async_trait;
use aura_core::AuthorityId;
use aura_guards::GuardContextProvider;
use aura_protocol::effects::AuraEffects;

// Implementation of AuraEffects (composite trait)
#[async_trait]
impl AuraEffects for AuraEffectSystem {
    fn execution_mode(&self) -> aura_core::effects::ExecutionMode {
        self.execution_mode
    }
}

impl GuardContextProvider for AuraEffectSystem {
    fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }

    fn get_metadata(&self, key: &str) -> Option<String> {
        match key {
            "authority_id" => Some(self.authority_id.to_string()),
            "execution_mode" => Some(format!("{:?}", AuraEffects::execution_mode(self))),
            "device_id" => Some(self.config.device_id().to_string()),
            "biscuit_token" => self.biscuit_cache.read().as_ref().map(|c| c.token_b64.clone()),
            "biscuit_root_pk" => self.biscuit_cache.read().as_ref().map(|c| c.root_pk_b64.clone()),
            _ => None,
        }
    }

    fn execution_mode(&self) -> aura_core::effects::ExecutionMode {
        AuraEffects::execution_mode(self)
    }

    fn can_perform_operation(&self, _operation: &str) -> bool {
        true
    }
}

// ============================================================================
// RuntimeEffectsBundle Implementation (for simulator decoupling)
// ============================================================================

#[cfg(feature = "simulation")]
impl aura_core::effects::RuntimeEffectsBundle for AuraEffectSystem {
    fn is_simulation_mode(&self) -> bool {
        matches!(
            self.execution_mode,
            aura_core::effects::ExecutionMode::Simulation { .. }
        )
    }

    fn simulation_seed(&self) -> Option<u64> {
        match self.execution_mode {
            aura_core::effects::ExecutionMode::Simulation { seed } => Some(seed),
            _ => None,
        }
    }
}
