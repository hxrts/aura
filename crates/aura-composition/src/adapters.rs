//! Individual handler adapters for the composition system
//!
//! This module provides adapter structs that wrap individual effect handlers
//! from the effects crate and expose the RegistrableHandler trait for use in
//! the effect registry.

use crate::registry::{HandlerContext, HandlerError, RegistrableHandler};
use async_trait::async_trait;
use aura_core::effects::{
    ConsoleEffects, CryptoEffects, NetworkEffects, PhysicalTimeEffects, RandomEffects,
    StorageEffects, SystemEffects,
};
use aura_core::{EffectType, ExecutionMode};
use aura_effects::{
    console::RealConsoleHandler, crypto::RealCryptoHandler, random::RealRandomHandler,
    storage::FilesystemStorageHandler, system::logging::LoggingSystemHandler,
    time::PhysicalTimeHandler, TcpTransportHandler as RealTransportHandler,
};

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
        if effect_type == EffectType::Console {
            vec![
                "log_info".to_string(),
                "log_warn".to_string(),
                "log_error".to_string(),
                "log_debug".to_string(),
            ]
        } else {
            Vec::new()
        }
    }

    fn supports_effect(&self, effect_type: EffectType) -> bool {
        effect_type == EffectType::Console
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Production
    }
}

/// Adapter for RealRandomHandler
pub struct RandomHandlerAdapter {
    handler: RealRandomHandler,
}

impl RandomHandlerAdapter {
    pub fn new(handler: RealRandomHandler) -> Self {
        Self { handler }
    }
}

#[async_trait]
impl RegistrableHandler for RandomHandlerAdapter {
    async fn execute_operation_bytes(
        &self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        _ctx: &HandlerContext,
    ) -> Result<Vec<u8>, HandlerError> {
        if effect_type != EffectType::Random {
            return Err(HandlerError::UnsupportedEffect { effect_type });
        }

        match operation {
            "random_bytes" => {
                let len: usize = bincode::deserialize(parameters).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                let result = self.handler.random_bytes(len).await;
                bincode::serialize(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "random_bytes_32" => {
                let result = self.handler.random_bytes_32().await;
                bincode::serialize(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "random_u64" => {
                let result = self.handler.random_u64().await;
                bincode::serialize(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            _ => Err(HandlerError::UnknownOperation {
                effect_type,
                operation: operation.to_string(),
            }),
        }
    }

    fn supported_operations(&self, effect_type: EffectType) -> Vec<String> {
        if effect_type == EffectType::Random {
            vec![
                "random_bytes".to_string(),
                "random_bytes_32".to_string(),
                "random_u64".to_string(),
            ]
        } else {
            Vec::new()
        }
    }

    fn supports_effect(&self, effect_type: EffectType) -> bool {
        effect_type == EffectType::Random
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Production
    }
}

/// Adapter for RealCryptoHandler
pub struct CryptoHandlerAdapter {
    handler: RealCryptoHandler,
}

impl CryptoHandlerAdapter {
    pub fn new(handler: RealCryptoHandler) -> Self {
        Self { handler }
    }
}

#[async_trait]
impl RegistrableHandler for CryptoHandlerAdapter {
    async fn execute_operation_bytes(
        &self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        _ctx: &HandlerContext,
    ) -> Result<Vec<u8>, HandlerError> {
        if effect_type != EffectType::Crypto {
            return Err(HandlerError::UnsupportedEffect { effect_type });
        }

        match operation {
            "hkdf_derive" => {
                // Parameters would be (ikm, salt, info, length)
                let params: (Vec<u8>, Option<Vec<u8>>, Vec<u8>, usize) =
                    bincode::deserialize(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let salt = params.1.unwrap_or_default();
                let result = self
                    .handler
                    .hkdf_derive(&params.0, &salt, &params.2, params.3)
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                bincode::serialize(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "ed25519_generate_keypair" => {
                let result = self.handler.ed25519_generate_keypair().await.map_err(|e| {
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
            "ed25519_sign" => {
                let params: (Vec<u8>, Vec<u8>) = bincode::deserialize(parameters).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                let result = self
                    .handler
                    .ed25519_sign(&params.0, &params.1)
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                bincode::serialize(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "ed25519_verify" => {
                let params: (Vec<u8>, Vec<u8>, Vec<u8>) = bincode::deserialize(parameters)
                    .map_err(|e| HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    })?;
                let result = self
                    .handler
                    .ed25519_verify(&params.0, &params.1, &params.2)
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                bincode::serialize(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            _ => Err(HandlerError::UnknownOperation {
                effect_type,
                operation: operation.to_string(),
            }),
        }
    }

    fn supported_operations(&self, effect_type: EffectType) -> Vec<String> {
        if effect_type == EffectType::Crypto {
            vec![
                "hkdf_derive".to_string(),
                "ed25519_generate_keypair".to_string(),
                "ed25519_sign".to_string(),
                "ed25519_verify".to_string(),
            ]
        } else {
            Vec::new()
        }
    }

    fn supports_effect(&self, effect_type: EffectType) -> bool {
        effect_type == EffectType::Crypto
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Production
    }
}

/// Adapter for FilesystemStorageHandler
pub struct StorageHandlerAdapter {
    handler: FilesystemStorageHandler,
}

impl StorageHandlerAdapter {
    pub fn new(handler: FilesystemStorageHandler) -> Self {
        Self { handler }
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
                let params: (String, Vec<u8>) = bincode::deserialize(parameters).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                self.handler.store(&params.0, params.1).await.map_err(|e| {
                    HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                Ok(Vec::new()) // store returns void
            }
            "retrieve" => {
                let key: String = bincode::deserialize(parameters).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                let result = self.handler.retrieve(&key).await.map_err(|e| {
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
            "remove" => {
                let key: String = bincode::deserialize(parameters).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                let result =
                    self.handler
                        .remove(&key)
                        .await
                        .map_err(|e| HandlerError::ExecutionFailed {
                            source: Box::new(e),
                        })?;
                bincode::serialize(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "list_keys" => {
                let prefix: Option<String> = bincode::deserialize(parameters).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                let result = self
                    .handler
                    .list_keys(prefix.as_deref())
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                bincode::serialize(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            _ => Err(HandlerError::UnknownOperation {
                effect_type,
                operation: operation.to_string(),
            }),
        }
    }

    fn supported_operations(&self, effect_type: EffectType) -> Vec<String> {
        if effect_type == EffectType::Storage {
            vec![
                "store".to_string(),
                "retrieve".to_string(),
                "remove".to_string(),
                "list_keys".to_string(),
            ]
        } else {
            Vec::new()
        }
    }

    fn supports_effect(&self, effect_type: EffectType) -> bool {
        effect_type == EffectType::Storage
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Production
    }
}

/// Adapter for PhysicalTimeHandler (domain-specific time effects)
pub struct TimeHandlerAdapter {
    handler: PhysicalTimeHandler,
}

impl TimeHandlerAdapter {
    pub fn new(handler: PhysicalTimeHandler) -> Self {
        Self { handler }
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
            "sleep_ms" => {
                let millis: u64 = bincode::deserialize(parameters).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                let _ = PhysicalTimeEffects::sleep_ms(&self.handler, millis).await;
                Ok(Vec::new()) // sleep returns void
            }
            "sleep_until" => {
                let epoch: u64 = bincode::deserialize(parameters).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                self.handler.sleep_until(epoch).await;
                Ok(Vec::new())
            }
            _ => Err(HandlerError::UnknownOperation {
                effect_type,
                operation: operation.to_string(),
            }),
        }
    }

    fn supported_operations(&self, effect_type: EffectType) -> Vec<String> {
        if effect_type == EffectType::Time {
            vec!["sleep_ms".to_string(), "sleep_until".to_string()]
        } else {
            Vec::new()
        }
    }

    fn supports_effect(&self, effect_type: EffectType) -> bool {
        effect_type == EffectType::Time
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Production
    }
}

/// Adapter for TcpTransportHandler (NetworkEffects implementation)
pub struct TransportHandlerAdapter {
    handler: RealTransportHandler,
}

impl TransportHandlerAdapter {
    pub fn new(handler: RealTransportHandler) -> Self {
        Self { handler }
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
                    bincode::deserialize(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                self.handler
                    .send_to_peer(params.0, params.1)
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                Ok(Vec::new()) // send returns void
            }
            "broadcast" => {
                let message: Vec<u8> = bincode::deserialize(parameters).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                self.handler.broadcast(message).await.map_err(|e| {
                    HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                Ok(Vec::new()) // broadcast returns void
            }
            "receive" => {
                let received = NetworkEffects::receive(&self.handler).await.map_err(|e| {
                    HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                bincode::serialize(&received).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            _ => Err(HandlerError::UnknownOperation {
                effect_type,
                operation: operation.to_string(),
            }),
        }
    }

    fn supported_operations(&self, effect_type: EffectType) -> Vec<String> {
        if effect_type == EffectType::Network {
            vec![
                "send_to_peer".to_string(),
                "broadcast".to_string(),
                "receive".to_string(),
            ]
        } else {
            Vec::new()
        }
    }

    fn supports_effect(&self, effect_type: EffectType) -> bool {
        effect_type == EffectType::Network
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Production
    }
}

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
        if effect_type == EffectType::System {
            vec![
                "log".to_string(),
                "log_with_context".to_string(),
                "health_check".to_string(),
                "get_system_info".to_string(),
                "set_config".to_string(),
                "get_config".to_string(),
                "get_metrics".to_string(),
                "restart_component".to_string(),
                "shutdown".to_string(),
            ]
        } else {
            Vec::new()
        }
    }

    fn supports_effect(&self, effect_type: EffectType) -> bool {
        effect_type == EffectType::System
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Production
    }
}
