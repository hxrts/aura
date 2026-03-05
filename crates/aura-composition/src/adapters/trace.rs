//! Trace handler adapter

use crate::adapters::collect_ops;
use crate::adapters::utils::{deserialize_operation_params, serialize_operation_result};
use crate::registry::{HandlerContext, HandlerError, RegistrableHandler};
use async_trait::async_trait;
use aura_core::effects::trace::{TraceEffects, TraceEvent, TraceSpanId};
use aura_core::{EffectType, ExecutionMode};
use aura_effects::trace::TraceHandler;
use std::sync::Arc;

/// Adapter for TraceHandler
pub struct TraceHandlerAdapter {
    handler: Arc<dyn TraceEffects>,
}

impl TraceHandlerAdapter {
    pub fn new(handler: TraceHandler) -> Self {
        Self {
            handler: Arc::new(handler),
        }
    }

    pub fn new_shared(handler: Arc<dyn TraceEffects>) -> Self {
        Self { handler }
    }
}

#[async_trait]
impl RegistrableHandler for TraceHandlerAdapter {
    async fn execute_operation_bytes(
        &self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        _ctx: &HandlerContext,
    ) -> Result<Vec<u8>, HandlerError> {
        if effect_type != EffectType::Trace {
            return Err(HandlerError::UnsupportedEffect { effect_type });
        }

        match operation {
            "trace_event" => {
                let event: TraceEvent =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                self.handler.trace_event(event).await;
                Ok(Vec::new())
            }
            "trace_span" => {
                let event: TraceEvent =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                let span = self.handler.trace_span(event).await;
                serialize_operation_result(effect_type, operation, &span)
            }
            "trace_span_end" => {
                let span: TraceSpanId =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                self.handler.trace_span_end(span).await;
                Ok(Vec::new())
            }
            _ => Err(HandlerError::UnknownOperation {
                effect_type,
                operation: operation.to_string(),
            }),
        }
    }

    fn supported_operations(&self, effect_type: EffectType) -> Vec<String> {
        collect_ops(effect_type, false)
    }

    fn supports_effect(&self, effect_type: EffectType) -> bool {
        effect_type == EffectType::Trace
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Production
    }
}
