//! Console handler adapter

use crate::adapters::collect_ops;
use crate::registry::{HandlerContext, HandlerError, RegistrableHandler};
use async_trait::async_trait;
use aura_core::effects::ConsoleEffects;
use aura_core::{EffectType, ExecutionMode};
use aura_effects::console::RealConsoleHandler;

/// Adapter for RealConsoleHandler
pub struct ConsoleHandlerAdapter {
    handler: RealConsoleHandler,
}

impl ConsoleHandlerAdapter {
    pub fn new(handler: RealConsoleHandler) -> Self {
        Self { handler }
    }
}

#[async_trait]
impl RegistrableHandler for ConsoleHandlerAdapter {
    async fn execute_operation_bytes(
        &self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        _ctx: &HandlerContext,
    ) -> Result<Vec<u8>, HandlerError> {
        if effect_type != EffectType::Console {
            return Err(HandlerError::UnsupportedEffect { effect_type });
        }

        match operation {
            "log_info" => {
                let message = String::from_utf8(parameters.to_vec()).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                self.handler.log_info(&message).await.map_err(|e| {
                    HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                Ok(Vec::new()) // Console operations return void
            }
            "log_warn" => {
                let message = String::from_utf8(parameters.to_vec()).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                self.handler.log_warn(&message).await.map_err(|e| {
                    HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                Ok(Vec::new())
            }
            "log_error" => {
                let message = String::from_utf8(parameters.to_vec()).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                self.handler.log_error(&message).await.map_err(|e| {
                    HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                Ok(Vec::new())
            }
            "log_debug" => {
                let message = String::from_utf8(parameters.to_vec()).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                self.handler.log_debug(&message).await.map_err(|e| {
                    HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
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
        collect_ops(effect_type, false) // Console has no extended operations
    }

    fn supports_effect(&self, effect_type: EffectType) -> bool {
        effect_type == EffectType::Console
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Production
    }
}
