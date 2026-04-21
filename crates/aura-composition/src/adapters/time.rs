//! Time handler adapter

use crate::adapters::collect_ops;
use crate::adapters::utils::{
    deserialize_operation_params, execution_failed, serialize_operation_result, void_result,
};
use crate::registry::{HandlerContext, HandlerError, RegistrableHandler};
use async_trait::async_trait;
use aura_core::effects::{LogicalClockEffects, OrderClockEffects, PhysicalTimeEffects};
use aura_core::{EffectType, ExecutionMode};
use aura_effects::time::PhysicalTimeHandler;

/// Adapter for PhysicalTimeHandler (domain-specific time effects)
pub struct TimeHandlerAdapter {
    physical: PhysicalTimeHandler,
    #[allow(deprecated)]
    logical: aura_effects::time::LogicalClockHandler,
    order: aura_effects::time::OrderClockHandler,
}

impl TimeHandlerAdapter {
    pub fn new(handler: PhysicalTimeHandler) -> Self {
        Self {
            physical: handler,
            #[allow(deprecated)]
            logical: aura_effects::time::LogicalClockHandler::new(),
            order: aura_effects::time::OrderClockHandler,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl RegistrableHandler for TimeHandlerAdapter {
    async fn execute_operation_bytes(
        &self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        _ctx: &HandlerContext,
    ) -> Result<Vec<u8>, HandlerError> {
        if effect_type != EffectType::Time {
            return Err(HandlerError::UnsupportedEffect { effect_type });
        }

        match operation {
            "physical_time" => {
                let result = self
                    .physical
                    .physical_time()
                    .await
                    .map_err(execution_failed)?;
                serialize_operation_result(effect_type, operation, &result)
            }
            "sleep_ms" => {
                let millis: u64 = deserialize_operation_params(effect_type, operation, parameters)?;
                let _ = PhysicalTimeEffects::sleep_ms(&self.physical, millis).await;
                Ok(void_result()) // sleep returns void
            }
            "sleep_until" => {
                let epoch: u64 = deserialize_operation_params(effect_type, operation, parameters)?;
                self.physical.sleep_until(epoch).await;
                Ok(void_result())
            }
            "logical_advance" => {
                let observed: Option<aura_core::time::VectorClock> =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                let result = self
                    .logical
                    .logical_advance(observed.as_ref())
                    .await
                    .map_err(execution_failed)?;
                serialize_operation_result(effect_type, operation, &result)
            }
            "logical_now" => {
                let result = self.logical.logical_now().await.map_err(execution_failed)?;
                serialize_operation_result(effect_type, operation, &result)
            }
            "order_time" => {
                let result = self.order.order_time().await.map_err(execution_failed)?;
                serialize_operation_result(effect_type, operation, &result)
            }
            _ => Err(HandlerError::UnknownOperation {
                effect_type,
                operation: operation.to_string(),
            }),
        }
    }

    fn supported_operations(&self, effect_type: EffectType) -> Vec<String> {
        collect_ops(effect_type, false) // Time has no extended operations
    }

    fn supports_effect(&self, effect_type: EffectType) -> bool {
        effect_type == EffectType::Time
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Production
    }
}
