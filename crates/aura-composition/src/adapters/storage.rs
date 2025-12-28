//! Storage handler adapter

use crate::adapters::collect_ops;
use crate::registry::{HandlerContext, HandlerError, RegistrableHandler};
use async_trait::async_trait;
use aura_core::effects::{StorageCoreEffects, StorageExtendedEffects};
use aura_core::{EffectType, ExecutionMode};
use aura_effects::storage::FilesystemStorageHandler;
use std::sync::Arc;

/// Adapter for FilesystemStorageHandler
pub struct StorageHandlerAdapter {
    core: Arc<dyn StorageCoreEffects>,
    extended: Option<Arc<dyn StorageExtendedEffects>>,
}

impl StorageHandlerAdapter {
    pub fn new(handler: FilesystemStorageHandler) -> Self {
        let handler = Arc::new(handler);
        let core: Arc<dyn StorageCoreEffects> = handler.clone();
        let extended: Arc<dyn StorageExtendedEffects> = handler;
        Self {
            core,
            extended: Some(extended),
        }
    }

    pub fn new_core(handler: Arc<dyn StorageCoreEffects>) -> Self {
        Self {
            core: handler,
            extended: None,
        }
    }

    pub fn new_extended<T: StorageExtendedEffects + 'static>(handler: T) -> Self {
        let handler = Arc::new(handler);
        let core: Arc<dyn StorageCoreEffects> = handler.clone();
        let extended: Arc<dyn StorageExtendedEffects> = handler;
        Self {
            core,
            extended: Some(extended),
        }
    }
}

#[async_trait]
impl RegistrableHandler for StorageHandlerAdapter {
    async fn execute_operation_bytes(
        &self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        _ctx: &HandlerContext,
    ) -> Result<Vec<u8>, HandlerError> {
        if effect_type != EffectType::Storage {
            return Err(HandlerError::UnsupportedEffect { effect_type });
        }

        match operation {
            "store" => {
                let params: (String, Vec<u8>) =
                    aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                self.core.store(&params.0, params.1).await.map_err(|e| {
                    HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                Ok(Vec::new()) // store returns void
            }
            "retrieve" => {
                let key: String =
                    aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result =
                    self.core
                        .retrieve(&key)
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
            "remove" => {
                let key: String =
                    aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result =
                    self.core
                        .remove(&key)
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
            "list_keys" => {
                let prefix: Option<String> = aura_core::util::serialization::from_slice(parameters)
                    .map_err(|e| HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    })?;
                let result = self.core.list_keys(prefix.as_deref()).await.map_err(|e| {
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
            "exists" => {
                let handler =
                    self.extended
                        .as_ref()
                        .ok_or_else(|| HandlerError::UnknownOperation {
                            effect_type,
                            operation: operation.to_string(),
                        })?;
                let key: String =
                    aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result =
                    handler
                        .exists(&key)
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
            "store_batch" => {
                let handler =
                    self.extended
                        .as_ref()
                        .ok_or_else(|| HandlerError::UnknownOperation {
                            effect_type,
                            operation: operation.to_string(),
                        })?;
                let pairs: std::collections::HashMap<String, Vec<u8>> =
                    aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                handler
                    .store_batch(pairs)
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                Ok(Vec::new())
            }
            "retrieve_batch" => {
                let handler =
                    self.extended
                        .as_ref()
                        .ok_or_else(|| HandlerError::UnknownOperation {
                            effect_type,
                            operation: operation.to_string(),
                        })?;
                let keys: Vec<String> = aura_core::util::serialization::from_slice(parameters)
                    .map_err(|e| HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    })?;
                let result = handler.retrieve_batch(&keys).await.map_err(|e| {
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
            "clear_all" => {
                let handler =
                    self.extended
                        .as_ref()
                        .ok_or_else(|| HandlerError::UnknownOperation {
                            effect_type,
                            operation: operation.to_string(),
                        })?;
                handler
                    .clear_all()
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                Ok(Vec::new())
            }
            "stats" => {
                let handler =
                    self.extended
                        .as_ref()
                        .ok_or_else(|| HandlerError::UnknownOperation {
                            effect_type,
                            operation: operation.to_string(),
                        })?;
                let result = handler
                    .stats()
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
        collect_ops(effect_type, self.extended.is_some())
    }

    fn supports_effect(&self, effect_type: EffectType) -> bool {
        effect_type == EffectType::Storage
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Production
    }
}
