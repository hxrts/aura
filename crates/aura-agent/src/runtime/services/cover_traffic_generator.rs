//! Runtime-owned cover traffic planning.
#![allow(dead_code)]

use super::config_profiles::impl_service_config_profiles;
use super::traits::{RuntimeService, RuntimeServiceContext, ServiceError, ServiceHealth};
use async_trait::async_trait;
use aura_core::service::{MoveEnvelope, MovePathBinding};
use tokio::sync::RwLock;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoverTrafficGeneratorCommand {
    PlanCover,
}

#[derive(Debug, Clone)]
pub struct CoverTrafficGeneratorConfig {
    pub activity_cover_floor_per_second: u32,
    pub mixing_mass_target_per_second: u32,
    pub reserved_budget_units: u32,
}

impl Default for CoverTrafficGeneratorConfig {
    fn default() -> Self {
        Self {
            activity_cover_floor_per_second: 1,
            mixing_mass_target_per_second: 4,
            reserved_budget_units: 2,
        }
    }
}

impl_service_config_profiles!(CoverTrafficGeneratorConfig {
    pub fn for_testing() -> Self {
        Self {
            activity_cover_floor_per_second: 2,
            mixing_mass_target_per_second: 5,
            reserved_budget_units: 3,
        }
    }
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverTrafficPlan {
    pub synthetic_cover_packets: u32,
    pub reserved_budget_units: u32,
    pub observed_accountability_reply_packets: u32,
    pub envelopes: Vec<MoveEnvelope>,
}

#[derive(Debug)]
struct CoverTrafficGeneratorState {
    last_plan: Option<CoverTrafficPlan>,
    lifecycle: ServiceHealth,
}

impl Default for CoverTrafficGeneratorState {
    fn default() -> Self {
        Self {
            last_plan: None,
            lifecycle: ServiceHealth::NotStarted,
        }
    }
}

#[aura_macros::actor_owned(
    owner = "cover_traffic_generator",
    domain = "adaptive_privacy_cover",
    gate = "cover_traffic_command_ingress",
    command = CoverTrafficGeneratorCommand,
    capacity = 32,
    category = "actor_owned"
)]
pub struct CoverTrafficGeneratorService {
    config: CoverTrafficGeneratorConfig,
    state: RwLock<CoverTrafficGeneratorState>,
}

impl CoverTrafficGeneratorService {
    pub fn new(config: CoverTrafficGeneratorConfig) -> Self {
        Self {
            config,
            state: RwLock::new(CoverTrafficGeneratorState::default()),
        }
    }

    pub async fn plan_cover(
        &self,
        binding: MovePathBinding,
        application_rate_per_second: u32,
        sync_blended_rate_per_second: u32,
        accountability_reply_rate_per_second: u32,
    ) -> CoverTrafficPlan {
        let target_gap = self.config.mixing_mass_target_per_second.saturating_sub(
            application_rate_per_second.saturating_add(sync_blended_rate_per_second),
        );
        let synthetic_cover_packets = self.config.activity_cover_floor_per_second.max(target_gap);
        let envelopes = (0..synthetic_cover_packets)
            .map(|_| MoveEnvelope::opaque(binding.clone(), vec![0u8; 32]))
            .collect::<Vec<_>>();
        let plan = CoverTrafficPlan {
            synthetic_cover_packets,
            reserved_budget_units: self.config.reserved_budget_units,
            observed_accountability_reply_packets: accountability_reply_rate_per_second,
            envelopes,
        };
        self.state.write().await.last_plan = Some(plan.clone());
        plan
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl RuntimeService for CoverTrafficGeneratorService {
    fn name(&self) -> &'static str {
        "cover_traffic_generator"
    }

    fn dependencies(&self) -> &[&'static str] {
        &["selection_manager", "move_manager"]
    }

    async fn start(&self, _ctx: &RuntimeServiceContext) -> Result<(), ServiceError> {
        self.state.write().await.lifecycle = ServiceHealth::Healthy;
        Ok(())
    }

    async fn stop(&self) -> Result<(), ServiceError> {
        self.state.write().await.lifecycle = ServiceHealth::Stopped;
        Ok(())
    }

    async fn health(&self) -> ServiceHealth {
        self.state.read().await.lifecycle.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::service::{LinkEndpoint, LinkProtocol, MovePath};

    fn direct_binding() -> MovePathBinding {
        MovePathBinding::Direct(MovePath::direct(LinkEndpoint::direct(
            LinkProtocol::Tcp,
            "127.0.0.1:9000",
        )))
    }

    #[tokio::test]
    async fn cover_generator_keeps_non_zero_floor() {
        let generator =
            CoverTrafficGeneratorService::new(CoverTrafficGeneratorConfig::for_testing());
        let plan = generator.plan_cover(direct_binding(), 10, 10, 4).await;
        assert_eq!(plan.synthetic_cover_packets, 2);
        assert_eq!(plan.observed_accountability_reply_packets, 4);
        assert_eq!(plan.envelopes.len(), 2);
    }

    #[tokio::test]
    async fn cover_generator_reserves_budget_separately_from_real_traffic() {
        let generator =
            CoverTrafficGeneratorService::new(CoverTrafficGeneratorConfig::for_testing());
        let plan = generator.plan_cover(direct_binding(), 1, 1, 9).await;
        assert_eq!(plan.reserved_budget_units, 3);
        assert!(plan.synthetic_cover_packets >= 3);
        assert_eq!(plan.observed_accountability_reply_packets, 9);
    }
}
