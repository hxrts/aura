//! Transport handler adapter

use crate::adapters::collect_ops;
use crate::adapters::utils::{deserialize_operation_params, serialize_operation_result};
use crate::registry::{HandlerContext, HandlerError, RegistrableHandler};
use async_trait::async_trait;
use aura_core::effects::{NetworkCoreEffects, NetworkExtendedEffects};
use aura_core::{EffectType, ExecutionMode};
use cfg_if::cfg_if;
use std::sync::Arc;

cfg_if! {
    if #[cfg(not(target_arch = "wasm32"))] {
        use aura_effects::TcpTransportHandler as RealTransportHandler;
    }
}

/// Adapter for TcpTransportHandler (NetworkEffects implementation)
pub struct TransportHandlerAdapter {
    core: Arc<dyn NetworkCoreEffects>,
    extended: Option<Arc<dyn NetworkExtendedEffects>>,
}

impl TransportHandlerAdapter {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(handler: RealTransportHandler) -> Self {
        let handler = Arc::new(handler);
        let core: Arc<dyn NetworkCoreEffects> = handler.clone();
        let extended: Arc<dyn NetworkExtendedEffects> = handler;
        Self {
            core,
            extended: Some(extended),
        }
    }

    pub fn new_core(handler: Arc<dyn NetworkCoreEffects>) -> Self {
        Self {
            core: handler,
            extended: None,
        }
    }

    pub fn new_extended<T: NetworkExtendedEffects + 'static>(handler: T) -> Self {
        let handler = Arc::new(handler);
        let core: Arc<dyn NetworkCoreEffects> = handler.clone();
        let extended: Arc<dyn NetworkExtendedEffects> = handler;
        Self {
            core,
            extended: Some(extended),
        }
    }
}

#[async_trait]
impl RegistrableHandler for TransportHandlerAdapter {
    async fn execute_operation_bytes(
        &self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        _ctx: &HandlerContext,
    ) -> Result<Vec<u8>, HandlerError> {
        if effect_type != EffectType::Network {
            return Err(HandlerError::UnsupportedEffect { effect_type });
        }

        match operation {
            "send_to_peer" => {
                let params: (uuid::Uuid, Vec<u8>) =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                self.core
                    .send_to_peer(params.0, params.1)
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                Ok(Vec::new()) // send returns void
            }
            "broadcast" => {
                let message: Vec<u8> =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                self.core
                    .broadcast(message)
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                Ok(Vec::new()) // broadcast returns void
            }
            "receive" => {
                let received = NetworkCoreEffects::receive(&self.core).await.map_err(|e| {
                    HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                serialize_operation_result(effect_type, operation, &received)
            }
            "receive_from" => {
                let handler =
                    self.extended
                        .as_ref()
                        .ok_or_else(|| HandlerError::UnknownOperation {
                            effect_type,
                            operation: operation.to_string(),
                        })?;
                let peer_id: uuid::Uuid =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                let received = handler.receive_from(peer_id).await.map_err(|e| {
                    HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                serialize_operation_result(effect_type, operation, &received)
            }
            "connected_peers" => {
                let handler =
                    self.extended
                        .as_ref()
                        .ok_or_else(|| HandlerError::UnknownOperation {
                            effect_type,
                            operation: operation.to_string(),
                        })?;
                let peers = handler.connected_peers().await;
                serialize_operation_result(effect_type, operation, &peers)
            }
            "is_peer_connected" => {
                let handler =
                    self.extended
                        .as_ref()
                        .ok_or_else(|| HandlerError::UnknownOperation {
                            effect_type,
                            operation: operation.to_string(),
                        })?;
                let peer_id: uuid::Uuid =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                let result = handler.is_peer_connected(peer_id).await;
                serialize_operation_result(effect_type, operation, &result)
            }
            "subscribe_to_peer_events" => Err(HandlerError::ExecutionFailed {
                source: "Peer event streams are not serializable in registry adapters".into(),
            }),
            "open" => {
                let handler =
                    self.extended
                        .as_ref()
                        .ok_or_else(|| HandlerError::UnknownOperation {
                            effect_type,
                            operation: operation.to_string(),
                        })?;
                let address: String =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                let connection_id =
                    handler
                        .open(&address)
                        .await
                        .map_err(|e| HandlerError::ExecutionFailed {
                            source: Box::new(e),
                        })?;
                serialize_operation_result(effect_type, operation, &connection_id)
            }
            "send" => {
                let handler =
                    self.extended
                        .as_ref()
                        .ok_or_else(|| HandlerError::UnknownOperation {
                            effect_type,
                            operation: operation.to_string(),
                        })?;
                let (connection_id, data): (String, Vec<u8>) =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                handler.send(&connection_id, data).await.map_err(|e| {
                    HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                Ok(Vec::new())
            }
            "close" => {
                let handler =
                    self.extended
                        .as_ref()
                        .ok_or_else(|| HandlerError::UnknownOperation {
                            effect_type,
                            operation: operation.to_string(),
                        })?;
                let connection_id: String =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                handler
                    .close(&connection_id)
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
        collect_ops(effect_type, self.extended.is_some())
    }

    fn supports_effect(&self, effect_type: EffectType) -> bool {
        effect_type == EffectType::Network
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Production
    }
}
