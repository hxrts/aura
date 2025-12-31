use super::AuraEffectSystem;
use async_trait::async_trait;
use aura_core::effects::{LogicalClockEffects, OrderClockEffects, PhysicalTimeEffects, TimeEffects, TimeError};

// Time effects backed by the production physical clock handler.
#[async_trait]
impl PhysicalTimeEffects for AuraEffectSystem {
    async fn physical_time(&self) -> Result<aura_core::time::PhysicalTime, TimeError> {
        self.time_handler.physical_time().await
    }

    async fn sleep_ms(&self, ms: u64) -> Result<(), TimeError> {
        self.time_handler.sleep_ms(ms).await
    }
}

#[async_trait]
impl TimeEffects for AuraEffectSystem {}

#[async_trait]
impl LogicalClockEffects for AuraEffectSystem {
    async fn logical_advance(
        &self,
        observed: Option<&aura_core::time::VectorClock>,
    ) -> Result<aura_core::time::LogicalTime, TimeError> {
        self.logical_clock.logical_advance(observed).await
    }

    async fn logical_now(&self) -> Result<aura_core::time::LogicalTime, TimeError> {
        self.logical_clock.logical_now().await
    }
}

#[async_trait]
impl OrderClockEffects for AuraEffectSystem {
    async fn order_time(&self) -> Result<aura_core::time::OrderTime, TimeError> {
        self.order_clock.order_time().await
    }
}
