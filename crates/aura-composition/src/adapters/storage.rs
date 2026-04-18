//! Storage handler adapter

use crate::adapters::collect_ops;
use crate::adapters::utils::{
    deserialize_operation_params, execution_failed, serialize_operation_result, void_result,
};
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
        Self::from_extended_handler(handler)
    }

    pub fn new_core(handler: Arc<dyn StorageCoreEffects>) -> Self {
        Self::from_parts(handler, None)
    }

    pub fn new_extended<T: StorageExtendedEffects + 'static>(handler: T) -> Self {
        Self::from_extended_handler(handler)
    }

    fn from_parts(
        core: Arc<dyn StorageCoreEffects>,
        extended: Option<Arc<dyn StorageExtendedEffects>>,
    ) -> Self {
        Self { core, extended }
    }

    fn from_extended_handler<T>(handler: T) -> Self
    where
        T: StorageCoreEffects + StorageExtendedEffects + 'static,
    {
        let handler = Arc::new(handler);
        let core: Arc<dyn StorageCoreEffects> = handler.clone();
        let extended: Arc<dyn StorageExtendedEffects> = handler;
        Self::from_parts(core, Some(extended))
    }

    fn extended_handler(
        &self,
        effect_type: EffectType,
        operation: &str,
    ) -> Result<&dyn StorageExtendedEffects, HandlerError> {
        self.extended
            .as_deref()
            .ok_or_else(|| HandlerError::UnknownOperation {
                effect_type,
                operation: operation.to_string(),
            })
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
                self.core
                    .store(&params.0, params.1)
                    .await
                    .map_err(execution_failed)?;
                Ok(void_result()) // store returns void
            }
            "retrieve" => {
                let key: String = deserialize_operation_params(effect_type, operation, parameters)?;
                let result = self.core.retrieve(&key).await.map_err(execution_failed)?;
                serialize_operation_result(effect_type, operation, &result)
            }
            "remove" => {
                let key: String = deserialize_operation_params(effect_type, operation, parameters)?;
                let result = self.core.remove(&key).await.map_err(execution_failed)?;
                serialize_operation_result(effect_type, operation, &result)
            }
            "list_keys" => {
                let prefix: Option<String> =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                let result = self
                    .core
                    .list_keys(prefix.as_deref())
                    .await
                    .map_err(execution_failed)?;
                serialize_operation_result(effect_type, operation, &result)
            }
            "exists" => {
                let handler = self.extended_handler(effect_type, operation)?;
                let key: String = deserialize_operation_params(effect_type, operation, parameters)?;
                let result = handler.exists(&key).await.map_err(execution_failed)?;
                serialize_operation_result(effect_type, operation, &result)
            }
            "store_batch" => {
                let handler = self.extended_handler(effect_type, operation)?;
                let pairs: std::collections::HashMap<String, Vec<u8>> =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                handler.store_batch(pairs).await.map_err(execution_failed)?;
                Ok(void_result())
            }
            "retrieve_batch" => {
                let handler = self.extended_handler(effect_type, operation)?;
                let keys: Vec<String> =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                let result = handler
                    .retrieve_batch(&keys)
                    .await
                    .map_err(execution_failed)?;
                serialize_operation_result(effect_type, operation, &result)
            }
            "clear_all" => {
                self.extended_handler(effect_type, operation)?
                    .clear_all()
                    .await
                    .map_err(execution_failed)?;
                Ok(void_result())
            }
            "stats" => {
                let result = self
                    .extended_handler(effect_type, operation)?
                    .stats()
                    .await
                    .map_err(execution_failed)?;
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
