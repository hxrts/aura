//! Logging system handler adapter

use crate::registry::{HandlerContext, HandlerError, RegistrableHandler};
use async_trait::async_trait;
use aura_core::effects::registry as effect_registry;
use aura_core::effects::SystemEffects;
use aura_core::{EffectType, ExecutionMode};
use aura_effects::system::logging::LoggingSystemHandler;

/// Adapter for LoggingSystemHandler
pub struct LoggingSystemHandlerAdapter {
    handler: LoggingSystemHandler,
}

impl LoggingSystemHandlerAdapter {
    pub fn new(handler: LoggingSystemHandler) -> Self {
        Self { handler }
    }
}

#[async_trait]
impl RegistrableHandler for LoggingSystemHandlerAdapter {
    async fn execute_operation_bytes(
        &self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        _ctx: &HandlerContext,
    ) -> Result<Vec<u8>, HandlerError> {
        if effect_type != EffectType::System {
            return Err(HandlerError::UnsupportedEffect { effect_type });
        }

        match operation {
            "log" => {
                let (level, component, message): (String, String, String) =
                    bincode::deserialize(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                self.handler
                    .log(&level, &component, &message)
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                Ok(Vec::new())
            }
            "log_with_context" => {
                let (level, component, message, context): (
                    String,
                    String,
                    String,
                    std::collections::HashMap<String, String>,
                ) = bincode::deserialize(parameters).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                self.handler
                    .log_with_context(&level, &component, &message, context)
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                Ok(Vec::new())
            }
            "health_check" => {
                let result = self.handler.health_check().await.map_err(|e| {
                    HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                bincode::serialize(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "get_system_info" => {
                let result = self.handler.get_system_info().await.map_err(|e| {
                    HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                bincode::serialize(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "set_config" => {
                let (key, value): (String, String) =
                    bincode::deserialize(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                self.handler.set_config(&key, &value).await.map_err(|e| {
                    HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                Ok(Vec::new())
            }
            "get_config" => {
                let key: String = bincode::deserialize(parameters).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                let value = self.handler.get_config(&key).await.map_err(|e| {
                    HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                bincode::serialize(&value).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "get_metrics" => {
                let result = self.handler.get_metrics().await.map_err(|e| {
                    HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                bincode::serialize(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "restart_component" => {
                let component: String = bincode::deserialize(parameters).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                self.handler
                    .restart_component(&component)
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                Ok(Vec::new())
            }
            "shutdown" => {
                self.handler
                    .shutdown()
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                Ok(Vec::new())
            }
            _ => Err(HandlerError::UnknownOperation {
                effect_type,
                operation: operation.to_string(),
            }),
        }
    }

    fn supported_operations(&self, effect_type: EffectType) -> Vec<String> {
        effect_registry::operations_for(effect_type)
            .iter()
            .map(|op| (*op).to_string())
            .collect()
    }

    fn supports_effect(&self, effect_type: EffectType) -> bool {
        effect_type == EffectType::System
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Production
    }
}
