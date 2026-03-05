//! Random handler adapter

use crate::adapters::collect_ops;
use crate::adapters::utils::{deserialize_operation_params, serialize_operation_result};
use crate::registry::{HandlerContext, HandlerError, RegistrableHandler};
use async_trait::async_trait;
use aura_core::effects::{RandomCoreEffects, RandomExtendedEffects};
use aura_core::{EffectType, ExecutionMode};
use aura_effects::random::RealRandomHandler;
use std::sync::Arc;

/// Adapter for RealRandomHandler
pub struct RandomHandlerAdapter {
    handler: Arc<dyn RandomCoreEffects>,
}

impl RandomHandlerAdapter {
    pub fn new(handler: RealRandomHandler) -> Self {
        Self {
            handler: Arc::new(handler),
        }
    }

    pub fn new_core(handler: Arc<dyn RandomCoreEffects>) -> Self {
        Self { handler }
    }
}

#[async_trait]
impl RegistrableHandler for RandomHandlerAdapter {
    async fn execute_operation_bytes(
        &self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        _ctx: &HandlerContext,
    ) -> Result<Vec<u8>, HandlerError> {
        if effect_type != EffectType::Random {
            return Err(HandlerError::UnsupportedEffect { effect_type });
        }

        match operation {
            "random_bytes" => {
                let len: usize = deserialize_operation_params(effect_type, operation, parameters)?;
                let result = self.handler.random_bytes(len).await;
                serialize_operation_result(effect_type, operation, &result)
            }
            "random_bytes_32" => {
                let result = self.handler.random_bytes_32().await;
                serialize_operation_result(effect_type, operation, &result)
            }
            "random_u64" => {
                let result = self.handler.random_u64().await;
                serialize_operation_result(effect_type, operation, &result)
            }
            "random_range" => {
                let (min, max): (u64, u64) =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                let result = self.handler.random_range(min, max).await;
                serialize_operation_result(effect_type, operation, &result)
            }
            "random_uuid" => {
                let result = self.handler.random_uuid().await;
                serialize_operation_result(effect_type, operation, &result)
            }
            _ => Err(HandlerError::UnknownOperation {
                effect_type,
                operation: operation.to_string(),
            }),
        }
    }

    fn supported_operations(&self, effect_type: EffectType) -> Vec<String> {
        collect_ops(effect_type, true)
    }

    fn supports_effect(&self, effect_type: EffectType) -> bool {
        effect_type == EffectType::Random
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Production
    }
}
