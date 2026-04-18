//! Console handler adapter

use crate::adapters::collect_ops;
use crate::adapters::utils::{deserialize_operation_params, execution_failed, void_result};
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

    async fn execute_log_operation(
        &self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
    ) -> Result<Vec<u8>, HandlerError> {
        let message = decode_console_message(effect_type, operation, parameters)?;
        match operation {
            "log_info" => self
                .handler
                .log_info(&message)
                .await
                .map_err(execution_failed)?,
            "log_warn" => self
                .handler
                .log_warn(&message)
                .await
                .map_err(execution_failed)?,
            "log_error" => self
                .handler
                .log_error(&message)
                .await
                .map_err(execution_failed)?,
            "log_debug" => self
                .handler
                .log_debug(&message)
                .await
                .map_err(execution_failed)?,
            _ => unreachable!("log helper only handles console log operations"),
        }

        Ok(void_result())
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
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
            "log_info" | "log_warn" | "log_error" | "log_debug" => {
                self.execute_log_operation(effect_type, operation, parameters)
                    .await
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

fn decode_console_message(
    effect_type: EffectType,
    operation: &str,
    parameters: &[u8],
) -> Result<String, HandlerError> {
    match deserialize_operation_params(effect_type, operation, parameters) {
        Ok(message) => Ok(message),
        Err(_) => String::from_utf8(parameters.to_vec()).map_err(|e| {
            HandlerError::EffectDeserialization {
                effect_type,
                operation: operation.to_string(),
                source: Box::new(e),
            }
        }),
    }
}
