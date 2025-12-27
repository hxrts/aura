//! Transport handler adapter

use crate::adapters::collect_ops;
use crate::registry::{HandlerContext, HandlerError, RegistrableHandler};
use async_trait::async_trait;
use aura_core::effects::{NetworkCoreEffects, NetworkExtendedEffects};
use aura_core::{EffectType, ExecutionMode};
use aura_effects::TcpTransportHandler as RealTransportHandler;
use std::sync::Arc;

/// Adapter for TcpTransportHandler (NetworkEffects implementation)
pub struct TransportHandlerAdapter {
    core: Arc<dyn NetworkCoreEffects>,
    extended: Option<Arc<dyn NetworkExtendedEffects>>,
}

impl TransportHandlerAdapter {
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
                    aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                self.core
                    .send_to_peer(params.0, params.1)
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                Ok(Vec::new()) // send returns void
            }
            "broadcast" => {
                let message: Vec<u8> = aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                self.core.broadcast(message).await.map_err(|e| {
                    HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                Ok(Vec::new()) // broadcast returns void
            }
            "receive" => {
                let received = NetworkCoreEffects::receive(&self.core).await.map_err(|e| {
                    HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                aura_core::util::serialization::to_vec(&received).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "receive_from" => {
                let handler = self.extended.as_ref().ok_or_else(|| {
                    HandlerError::UnknownOperation {
                        effect_type,
                        operation: operation.to_string(),
                    }
                })?;
                let peer_id: uuid::Uuid = aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                let received = handler
                    .receive_from(peer_id)
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                aura_core::util::serialization::to_vec(&received).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "connected_peers" => {
                let handler = self.extended.as_ref().ok_or_else(|| {
                    HandlerError::UnknownOperation {
                        effect_type,
                        operation: operation.to_string(),
                    }
                })?;
                let peers = handler.connected_peers().await;
                aura_core::util::serialization::to_vec(&peers).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "is_peer_connected" => {
                let handler = self.extended.as_ref().ok_or_else(|| {
                    HandlerError::UnknownOperation {
                        effect_type,
                        operation: operation.to_string(),
                    }
                })?;
                let peer_id: uuid::Uuid = aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                let result = handler.is_peer_connected(peer_id).await;
                aura_core::util::serialization::to_vec(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "subscribe_to_peer_events" => Err(HandlerError::ExecutionFailed {
                source: "Peer event streams are not serializable in registry adapters".into(),
            }),
            "open" => {
                let handler = self.extended.as_ref().ok_or_else(|| {
                    HandlerError::UnknownOperation {
                        effect_type,
                        operation: operation.to_string(),
                    }
                })?;
                let address: String = aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                let connection_id = handler.open(&address).await.map_err(|e| {
                    HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                aura_core::util::serialization::to_vec(&connection_id).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "send" => {
                let handler = self.extended.as_ref().ok_or_else(|| {
                    HandlerError::UnknownOperation {
                        effect_type,
                        operation: operation.to_string(),
                    }
                })?;
                let (connection_id, data): (String, Vec<u8>) =
                    aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                handler.send(&connection_id, data).await.map_err(|e| {
                    HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                Ok(Vec::new())
            }
            "close" => {
                let handler = self.extended.as_ref().ok_or_else(|| {
                    HandlerError::UnknownOperation {
                        effect_type,
                        operation: operation.to_string(),
                    }
                })?;
                let connection_id: String = aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                handler.close(&connection_id).await.map_err(|e| {
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
        collect_ops(effect_type, self.extended.is_some())
    }

    fn supports_effect(&self, effect_type: EffectType) -> bool {
        effect_type == EffectType::Network
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Production
    }
}
