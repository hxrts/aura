//! Time handler adapter

use crate::adapters::collect_ops;
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

#[async_trait]
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
                let result = self.physical.physical_time().await.map_err(|e| {
                    HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                aura_core::util::serialization::to_vec(&result).map_err(|e| {
                    HandlerError::EffectSerialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })
            }
            "sleep_ms" => {
                let millis: u64 =
                    aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let _ = PhysicalTimeEffects::sleep_ms(&self.physical, millis).await;
                Ok(Vec::new()) // sleep returns void
            }
            "sleep_until" => {
                let epoch: u64 =
                    aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                self.physical.sleep_until(epoch).await;
                Ok(Vec::new())
            }
            "logical_advance" => {
                let observed: Option<aura_core::time::VectorClock> =
                    aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result = self
                    .logical
                    .logical_advance(observed.as_ref())
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                aura_core::util::serialization::to_vec(&result).map_err(|e| {
                    HandlerError::EffectSerialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })
            }
            "logical_now" => {
                let result = self.logical.logical_now().await.map_err(|e| {
                    HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                aura_core::util::serialization::to_vec(&result).map_err(|e| {
                    HandlerError::EffectSerialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })
            }
            "order_time" => {
                let result =
                    self.order
                        .order_time()
                        .await
                        .map_err(|e| HandlerError::ExecutionFailed {
                            source: Box::new(e),
                        })?;
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
        collect_ops(effect_type, false) // Time has no extended operations
    }

    fn supports_effect(&self, effect_type: EffectType) -> bool {
        effect_type == EffectType::Time
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Production
    }
}
