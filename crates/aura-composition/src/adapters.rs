//! Individual handler adapters for the composition system
//!
//! This module provides adapter structs that wrap individual effect handlers
//! from the effects crate and expose the RegistrableHandler trait for use in
//! the effect registry.

use crate::registry::{HandlerContext, HandlerError, RegistrableHandler};
use async_trait::async_trait;
use aura_core::effects::{
    ConsoleEffects, CryptoCoreEffects, CryptoExtendedEffects, LogicalClockEffects,
    NetworkCoreEffects, NetworkExtendedEffects, OrderClockEffects, PhysicalTimeEffects,
    RandomCoreEffects, RandomExtendedEffects, StorageCoreEffects, StorageExtendedEffects,
    SystemEffects,
};
use aura_core::effects::crypto::{FrostSigningPackage, KeyDerivationContext, SigningMode};
use aura_core::effects::registry as effect_registry;
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
        effect_registry::operations_for(effect_type)
            .iter()
            .map(|op| (*op).to_string())
            .collect()
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
            "random_range" => {
                let (min, max): (u64, u64) = bincode::deserialize(parameters).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                let result = self.handler.random_range(min, max).await;
                bincode::serialize(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "random_uuid" => {
                let result = self.handler.random_uuid().await;
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
        effect_registry::operations_for(effect_type)
            .iter()
            .map(|op| (*op).to_string())
            .collect()
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
            "derive_key" => {
                let params: (Vec<u8>, KeyDerivationContext) =
                    bincode::deserialize(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result = self
                    .handler
                    .derive_key(&params.0, &params.1)
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
            "ed25519_public_key" => {
                let private_key: Vec<u8> = bincode::deserialize(parameters).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                let result = self
                    .handler
                    .ed25519_public_key(&private_key)
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
            "generate_signing_keys" => {
                let (threshold, max_signers): (u16, u16) =
                    bincode::deserialize(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result = self
                    .handler
                    .generate_signing_keys(threshold, max_signers)
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
            "sign_with_key" => {
                let params: (Vec<u8>, Vec<u8>, SigningMode) =
                    bincode::deserialize(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result = self
                    .handler
                    .sign_with_key(&params.0, &params.1, params.2)
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
            "verify_signature" => {
                let params: (Vec<u8>, Vec<u8>, Vec<u8>, SigningMode) =
                    bincode::deserialize(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result = self
                    .handler
                    .verify_signature(&params.0, &params.1, &params.2, params.3)
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
            "frost_generate_keys" => {
                let (threshold, max_signers): (u16, u16) =
                    bincode::deserialize(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result = self
                    .handler
                    .frost_generate_keys(threshold, max_signers)
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
            "frost_generate_nonces" => {
                let key_package: Vec<u8> = bincode::deserialize(parameters).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                let result = self
                    .handler
                    .frost_generate_nonces(&key_package)
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
            "frost_create_signing_package" => {
                let params: (Vec<u8>, Vec<Vec<u8>>, Vec<u16>, Vec<u8>) =
                    bincode::deserialize(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result = self
                    .handler
                    .frost_create_signing_package(&params.0, &params.1, &params.2, &params.3)
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
            "frost_sign_share" => {
                let params: (FrostSigningPackage, Vec<u8>, Vec<u8>) =
                    bincode::deserialize(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result = self
                    .handler
                    .frost_sign_share(&params.0, &params.1, &params.2)
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
            "frost_aggregate_signatures" => {
                let params: (FrostSigningPackage, Vec<Vec<u8>>) =
                    bincode::deserialize(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result = self
                    .handler
                    .frost_aggregate_signatures(&params.0, &params.1)
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
            "frost_verify" => {
                let params: (Vec<u8>, Vec<u8>, Vec<u8>) =
                    bincode::deserialize(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result = self
                    .handler
                    .frost_verify(&params.0, &params.1, &params.2)
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
            "aes_gcm_encrypt" => {
                let params: (Vec<u8>, [u8; 32], [u8; 12]) =
                    bincode::deserialize(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result = self
                    .handler
                    .aes_gcm_encrypt(&params.0, &params.1, &params.2)
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
            "aes_gcm_decrypt" => {
                let params: (Vec<u8>, [u8; 32], [u8; 12]) =
                    bincode::deserialize(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result = self
                    .handler
                    .aes_gcm_decrypt(&params.0, &params.1, &params.2)
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
            "chacha20_encrypt" => {
                let params: (Vec<u8>, [u8; 32], [u8; 12]) =
                    bincode::deserialize(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result = self
                    .handler
                    .chacha20_encrypt(&params.0, &params.1, &params.2)
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
            "chacha20_decrypt" => {
                let params: (Vec<u8>, [u8; 32], [u8; 12]) =
                    bincode::deserialize(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result = self
                    .handler
                    .chacha20_decrypt(&params.0, &params.1, &params.2)
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
            "frost_rotate_keys" => {
                let params: (Vec<Vec<u8>>, u16, u16, u16) =
                    bincode::deserialize(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result = self
                    .handler
                    .frost_rotate_keys(&params.0, params.1, params.2, params.3)
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
        effect_registry::operations_for(effect_type)
            .iter()
            .map(|op| (*op).to_string())
            .collect()
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
            "exists" => {
                let key: String = bincode::deserialize(parameters).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                let result = self.handler.exists(&key).await.map_err(|e| {
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
            "store_batch" => {
                let pairs: std::collections::HashMap<String, Vec<u8>> =
                    bincode::deserialize(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                self.handler.store_batch(pairs).await.map_err(|e| {
                    HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                Ok(Vec::new())
            }
            "retrieve_batch" => {
                let keys: Vec<String> = bincode::deserialize(parameters).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                let result = self.handler.retrieve_batch(&keys).await.map_err(|e| {
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
            "clear_all" => {
                self.handler.clear_all().await.map_err(|e| {
                    HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                Ok(Vec::new())
            }
            "stats" => {
                let result = self.handler.stats().await.map_err(|e| {
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
        effect_type == EffectType::Storage
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Production
    }
}

/// Adapter for PhysicalTimeHandler (domain-specific time effects)
pub struct TimeHandlerAdapter {
    physical: PhysicalTimeHandler,
    #[allow(deprecated)]
    logical: aura_effects::time::LogicalClockHandler,
    order: aura_effects::time::OrderClockHandler,
}

impl TimeHandlerAdapter {
    pub fn new(handler: PhysicalTimeHandler) -> Self {
        Self {
            physical: handler,
            #[allow(deprecated)]
            logical: aura_effects::time::LogicalClockHandler::new(),
            order: aura_effects::time::OrderClockHandler::default(),
        }
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
            "physical_time" => {
                let result = self.physical.physical_time().await.map_err(|e| {
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
            "sleep_ms" => {
                let millis: u64 = bincode::deserialize(parameters).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                let _ = PhysicalTimeEffects::sleep_ms(&self.physical, millis).await;
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
                self.physical.sleep_until(epoch).await;
                Ok(Vec::new())
            }
            "logical_advance" => {
                let observed: Option<aura_core::time::VectorClock> =
                    bincode::deserialize(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result = self
                    .logical
                    .logical_advance(observed.as_ref())
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
            "logical_now" => {
                let result = self.logical.logical_now().await.map_err(|e| {
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
            "order_time" => {
                let result = self.order.order_time().await.map_err(|e| {
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
                let received = NetworkCoreEffects::receive(&self.handler).await.map_err(|e| {
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
            "receive_from" => {
                let peer_id: uuid::Uuid = bincode::deserialize(parameters).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                let received = self
                    .handler
                    .receive_from(peer_id)
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                bincode::serialize(&received).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "connected_peers" => {
                let peers = self.handler.connected_peers().await;
                bincode::serialize(&peers).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "is_peer_connected" => {
                let peer_id: uuid::Uuid = bincode::deserialize(parameters).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                let result = self.handler.is_peer_connected(peer_id).await;
                bincode::serialize(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "subscribe_to_peer_events" => Err(HandlerError::ExecutionFailed {
                source: "Peer event streams are not serializable in registry adapters".into(),
            }),
            "open" => {
                let address: String = bincode::deserialize(parameters).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                let connection_id =
                    NetworkExtendedEffects::open(&self.handler, &address)
                        .await
                        .map_err(|e| {
                    HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                bincode::serialize(&connection_id).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "send" => {
                let (connection_id, data): (String, Vec<u8>) =
                    bincode::deserialize(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                NetworkExtendedEffects::send(&self.handler, &connection_id, data)
                    .await
                    .map_err(|e| {
                    HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                Ok(Vec::new())
            }
            "close" => {
                let connection_id: String = bincode::deserialize(parameters).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                NetworkExtendedEffects::close(&self.handler, &connection_id)
                    .await
                    .map_err(|e| {
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
        effect_registry::operations_for(effect_type)
            .iter()
            .map(|op| (*op).to_string())
            .collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::effects::registry::operations_for;

    #[test]
    fn test_supported_operations_match_registry_map() {
        let console_ops = ConsoleHandlerAdapter::new(RealConsoleHandler::new())
            .supported_operations(EffectType::Console);
        assert_eq!(
            console_ops,
            operations_for(EffectType::Console)
                .iter()
                .map(|op| (*op).to_string())
                .collect::<Vec<_>>()
        );

        let random_ops =
            RandomHandlerAdapter::new(RealRandomHandler::new()).supported_operations(EffectType::Random);
        assert!(random_ops.contains(&"random_range".to_string()));
        assert!(random_ops.contains(&"random_uuid".to_string()));

        let storage_ops =
            StorageHandlerAdapter::new(FilesystemStorageHandler::with_default_path())
                .supported_operations(EffectType::Storage);
        assert!(storage_ops.contains(&"store_batch".to_string()));
        assert!(storage_ops.contains(&"stats".to_string()));
    }
}
