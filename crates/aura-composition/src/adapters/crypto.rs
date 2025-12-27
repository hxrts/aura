//! Crypto handler adapter

use crate::adapters::collect_ops;
use crate::registry::{HandlerContext, HandlerError, RegistrableHandler};
use async_trait::async_trait;
use aura_core::effects::crypto::{FrostSigningPackage, KeyDerivationContext, SigningMode};
use aura_core::effects::{CryptoCoreEffects, CryptoExtendedEffects};
use aura_core::{EffectType, ExecutionMode};
use aura_effects::crypto::RealCryptoHandler;
use std::sync::Arc;

/// Adapter for RealCryptoHandler
pub struct CryptoHandlerAdapter {
    core: Arc<dyn CryptoCoreEffects>,
    extended: Option<Arc<dyn CryptoExtendedEffects>>,
}

impl CryptoHandlerAdapter {
    pub fn new(handler: RealCryptoHandler) -> Self {
        let handler = Arc::new(handler);
        let core: Arc<dyn CryptoCoreEffects> = handler.clone();
        let extended: Arc<dyn CryptoExtendedEffects> = handler;
        Self {
            core,
            extended: Some(extended),
        }
    }

    pub fn new_core(handler: Arc<dyn CryptoCoreEffects>) -> Self {
        Self {
            core: handler,
            extended: None,
        }
    }

    pub fn new_extended<T: CryptoExtendedEffects + 'static>(handler: T) -> Self {
        let handler = Arc::new(handler);
        let core: Arc<dyn CryptoCoreEffects> = handler.clone();
        let extended: Arc<dyn CryptoExtendedEffects> = handler;
        Self {
            core,
            extended: Some(extended),
        }
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
                    aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let salt = params.1.unwrap_or_default();
                let result = self
                    .core
                    .hkdf_derive(&params.0, &salt, &params.2, params.3)
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                aura_core::util::serialization::to_vec(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "derive_key" => {
                let params: (Vec<u8>, KeyDerivationContext) =
                    aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result = self
                    .core
                    .derive_key(&params.0, &params.1)
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                aura_core::util::serialization::to_vec(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "ed25519_generate_keypair" => {
                let result = self.core.ed25519_generate_keypair().await.map_err(|e| {
                    HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                aura_core::util::serialization::to_vec(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "ed25519_sign" => {
                let params: (Vec<u8>, Vec<u8>) = aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                let result = self
                    .core
                    .ed25519_sign(&params.0, &params.1)
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                aura_core::util::serialization::to_vec(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "ed25519_verify" => {
                let params: (Vec<u8>, Vec<u8>, Vec<u8>) = aura_core::util::serialization::from_slice(parameters)
                    .map_err(|e| HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    })?;
                let result = self
                    .core
                    .ed25519_verify(&params.0, &params.1, &params.2)
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                aura_core::util::serialization::to_vec(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "ed25519_public_key" => {
                let handler = self.extended.as_ref().ok_or_else(|| {
                    HandlerError::UnknownOperation {
                        effect_type,
                        operation: operation.to_string(),
                    }
                })?;
                let private_key: Vec<u8> = aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                let result = handler
                    .ed25519_public_key(&private_key)
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                aura_core::util::serialization::to_vec(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "generate_signing_keys" => {
                let handler = self.extended.as_ref().ok_or_else(|| {
                    HandlerError::UnknownOperation {
                        effect_type,
                        operation: operation.to_string(),
                    }
                })?;
                let (threshold, max_signers): (u16, u16) =
                    aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result = handler
                    .generate_signing_keys(threshold, max_signers)
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                aura_core::util::serialization::to_vec(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "sign_with_key" => {
                let handler = self.extended.as_ref().ok_or_else(|| {
                    HandlerError::UnknownOperation {
                        effect_type,
                        operation: operation.to_string(),
                    }
                })?;
                let params: (Vec<u8>, Vec<u8>, SigningMode) =
                    aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result = handler
                    .sign_with_key(&params.0, &params.1, params.2)
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                aura_core::util::serialization::to_vec(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "verify_signature" => {
                let handler = self.extended.as_ref().ok_or_else(|| {
                    HandlerError::UnknownOperation {
                        effect_type,
                        operation: operation.to_string(),
                    }
                })?;
                let params: (Vec<u8>, Vec<u8>, Vec<u8>, SigningMode) =
                    aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result = handler
                    .verify_signature(&params.0, &params.1, &params.2, params.3)
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                aura_core::util::serialization::to_vec(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "frost_generate_keys" => {
                let handler = self.extended.as_ref().ok_or_else(|| {
                    HandlerError::UnknownOperation {
                        effect_type,
                        operation: operation.to_string(),
                    }
                })?;
                let (threshold, max_signers): (u16, u16) =
                    aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result = handler
                    .frost_generate_keys(threshold, max_signers)
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                aura_core::util::serialization::to_vec(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "frost_generate_nonces" => {
                let handler = self.extended.as_ref().ok_or_else(|| {
                    HandlerError::UnknownOperation {
                        effect_type,
                        operation: operation.to_string(),
                    }
                })?;
                let key_package: Vec<u8> = aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                    HandlerError::EffectDeserialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: Box::new(e),
                    }
                })?;
                let result = handler
                    .frost_generate_nonces(&key_package)
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                aura_core::util::serialization::to_vec(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "frost_create_signing_package" => {
                let handler = self.extended.as_ref().ok_or_else(|| {
                    HandlerError::UnknownOperation {
                        effect_type,
                        operation: operation.to_string(),
                    }
                })?;
                let params: (Vec<u8>, Vec<Vec<u8>>, Vec<u16>, Vec<u8>) =
                    aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result = handler
                    .frost_create_signing_package(&params.0, &params.1, &params.2, &params.3)
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                aura_core::util::serialization::to_vec(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "frost_sign_share" => {
                let handler = self.extended.as_ref().ok_or_else(|| {
                    HandlerError::UnknownOperation {
                        effect_type,
                        operation: operation.to_string(),
                    }
                })?;
                let params: (FrostSigningPackage, Vec<u8>, Vec<u8>) =
                    aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result = handler
                    .frost_sign_share(&params.0, &params.1, &params.2)
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                aura_core::util::serialization::to_vec(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "frost_aggregate_signatures" => {
                let handler = self.extended.as_ref().ok_or_else(|| {
                    HandlerError::UnknownOperation {
                        effect_type,
                        operation: operation.to_string(),
                    }
                })?;
                let params: (FrostSigningPackage, Vec<Vec<u8>>) =
                    aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result = handler
                    .frost_aggregate_signatures(&params.0, &params.1)
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                aura_core::util::serialization::to_vec(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "frost_verify" => {
                let handler = self.extended.as_ref().ok_or_else(|| {
                    HandlerError::UnknownOperation {
                        effect_type,
                        operation: operation.to_string(),
                    }
                })?;
                let params: (Vec<u8>, Vec<u8>, Vec<u8>) =
                    aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result = handler
                    .frost_verify(&params.0, &params.1, &params.2)
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                aura_core::util::serialization::to_vec(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "aes_gcm_encrypt" => {
                let handler = self.extended.as_ref().ok_or_else(|| {
                    HandlerError::UnknownOperation {
                        effect_type,
                        operation: operation.to_string(),
                    }
                })?;
                let params: (Vec<u8>, [u8; 32], [u8; 12]) =
                    aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result = handler
                    .aes_gcm_encrypt(&params.0, &params.1, &params.2)
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                aura_core::util::serialization::to_vec(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "aes_gcm_decrypt" => {
                let handler = self.extended.as_ref().ok_or_else(|| {
                    HandlerError::UnknownOperation {
                        effect_type,
                        operation: operation.to_string(),
                    }
                })?;
                let params: (Vec<u8>, [u8; 32], [u8; 12]) =
                    aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result = handler
                    .aes_gcm_decrypt(&params.0, &params.1, &params.2)
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                aura_core::util::serialization::to_vec(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "chacha20_encrypt" => {
                let handler = self.extended.as_ref().ok_or_else(|| {
                    HandlerError::UnknownOperation {
                        effect_type,
                        operation: operation.to_string(),
                    }
                })?;
                let params: (Vec<u8>, [u8; 32], [u8; 12]) =
                    aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result = handler
                    .chacha20_encrypt(&params.0, &params.1, &params.2)
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                aura_core::util::serialization::to_vec(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "chacha20_decrypt" => {
                let handler = self.extended.as_ref().ok_or_else(|| {
                    HandlerError::UnknownOperation {
                        effect_type,
                        operation: operation.to_string(),
                    }
                })?;
                let params: (Vec<u8>, [u8; 32], [u8; 12]) =
                    aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result = handler
                    .chacha20_decrypt(&params.0, &params.1, &params.2)
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                aura_core::util::serialization::to_vec(&result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type,
                    operation: operation.to_string(),
                    source: Box::new(e),
                })
            }
            "frost_rotate_keys" => {
                let handler = self.extended.as_ref().ok_or_else(|| {
                    HandlerError::UnknownOperation {
                        effect_type,
                        operation: operation.to_string(),
                    }
                })?;
                let params: (Vec<Vec<u8>>, u16, u16, u16) =
                    aura_core::util::serialization::from_slice(parameters).map_err(|e| {
                        HandlerError::EffectDeserialization {
                            effect_type,
                            operation: operation.to_string(),
                            source: Box::new(e),
                        }
                    })?;
                let result = handler
                    .frost_rotate_keys(&params.0, params.1, params.2, params.3)
                    .await
                    .map_err(|e| HandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                aura_core::util::serialization::to_vec(&result).map_err(|e| HandlerError::EffectSerialization {
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
        collect_ops(effect_type, self.extended.is_some())
    }

    fn supports_effect(&self, effect_type: EffectType) -> bool {
        effect_type == EffectType::Crypto
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Production
    }
}
