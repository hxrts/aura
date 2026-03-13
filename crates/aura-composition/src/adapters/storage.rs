//! Storage handler adapter

use crate::adapters::collect_ops;
use crate::adapters::utils::{deserialize_operation_params, serialize_operation_result};
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

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
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
                    deserialize_operation_params(effect_type, operation, parameters)?;
                self.core.store(&params.0, params.1).await.map_err(|e| {
                    HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                Ok(Vec::new()) // store returns void
            }
            "retrieve" => {
                let key: String = deserialize_operation_params(effect_type, operation, parameters)?;
                let result =
                    self.core
                        .retrieve(&key)
                        .await
                        .map_err(|e| HandlerError::ExecutionFailed {
                            source: Box::new(e),
                        })?;
                serialize_operation_result(effect_type, operation, &result)
            }
            "remove" => {
                let key: String = deserialize_operation_params(effect_type, operation, parameters)?;
                let result =
                    self.core
                        .remove(&key)
                        .await
                        .map_err(|e| HandlerError::ExecutionFailed {
                            source: Box::new(e),
                        })?;
                serialize_operation_result(effect_type, operation, &result)
            }
            "list_keys" => {
                let prefix: Option<String> =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                let result = self.core.list_keys(prefix.as_deref()).await.map_err(|e| {
                    HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                serialize_operation_result(effect_type, operation, &result)
            }
            "exists" => {
                let handler =
                    self.extended
                        .as_ref()
                        .ok_or_else(|| HandlerError::UnknownOperation {
                            effect_type,
                            operation: operation.to_string(),
                        })?;
                let key: String = deserialize_operation_params(effect_type, operation, parameters)?;
                let result =
                    handler
                        .exists(&key)
                        .await
                        .map_err(|e| HandlerError::ExecutionFailed {
                            source: Box::new(e),
                        })?;
                serialize_operation_result(effect_type, operation, &result)
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
                    deserialize_operation_params(effect_type, operation, parameters)?;
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
                let keys: Vec<String> =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                let result = handler.retrieve_batch(&keys).await.map_err(|e| {
                    HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                serialize_operation_result(effect_type, operation, &result)
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
                serialize_operation_result(effect_type, operation, &result)
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
