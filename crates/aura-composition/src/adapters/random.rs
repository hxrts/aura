//! Random handler adapter

use crate::adapters::collect_ops;
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
                let len: usize =
                    aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result = self.handler.random_bytes(len).await;
                aura_core::util::serialization::to_vec(&result).map_err(|e| {
                    HandlerError::EffectSerialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })
            }
            "random_bytes_32" => {
                let result = self.handler.random_bytes_32().await;
                aura_core::util::serialization::to_vec(&result).map_err(|e| {
                    HandlerError::EffectSerialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })
            }
            "random_u64" => {
                let result = self.handler.random_u64().await;
                aura_core::util::serialization::to_vec(&result).map_err(|e| {
                    HandlerError::EffectSerialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })
            }
            "random_range" => {
                let (min, max): (u64, u64) = aura_core::util::serialization::from_slice(parameters)
                    .map_err(|e| HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    })?;
                let result = self.handler.random_range(min, max).await;
                aura_core::util::serialization::to_vec(&result).map_err(|e| {
                    HandlerError::EffectSerialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })
            }
            "random_uuid" => {
                let result = self.handler.random_uuid().await;
                aura_core::util::serialization::to_vec(&result).map_err(|e| {
                    HandlerError::EffectSerialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })
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
