//! Crypto handler adapter

use crate::adapters::collect_ops;
use crate::adapters::utils::{
    deserialize_operation_params, execution_failed, serialize_operation_result,
};
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
        Self::from_extended_handler(handler)
    }

    pub fn new_core(handler: Arc<dyn CryptoCoreEffects>) -> Self {
        Self::from_parts(handler, None)
    }

    pub fn new_extended<T: CryptoExtendedEffects + 'static>(handler: T) -> Self {
        Self::from_extended_handler(handler)
    }

    fn from_parts(
        core: Arc<dyn CryptoCoreEffects>,
        extended: Option<Arc<dyn CryptoExtendedEffects>>,
    ) -> Self {
        Self { core, extended }
    }

    fn from_extended_handler<T>(handler: T) -> Self
    where
        T: CryptoCoreEffects + CryptoExtendedEffects + 'static,
    {
        let handler = Arc::new(handler);
        let core: Arc<dyn CryptoCoreEffects> = handler.clone();
        let extended: Arc<dyn CryptoExtendedEffects> = handler;
        Self::from_parts(core, Some(extended))
    }

    fn extended_handler(
        &self,
        effect_type: EffectType,
        operation: &str,
    ) -> Result<&dyn CryptoExtendedEffects, HandlerError> {
        self.extended
            .as_deref()
            .ok_or_else(|| HandlerError::UnknownOperation {
                effect_type,
                operation: operation.to_string(),
            })
    }

    fn secret_material_result_error(_effect_type: EffectType, operation: &str) -> HandlerError {
        execution_failed(aura_core::AuraError::invalid(format!(
            "operation {operation} returns private key material and is not available through the serialized composition adapter"
        )))
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
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
            "kdf_derive" => {
                let params: (Vec<u8>, Option<Vec<u8>>, Vec<u8>, u32) =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                let salt = params.1.unwrap_or_default();
                let result = self
                    .core
                    .kdf_derive(&params.0, &salt, &params.2, params.3)
                    .await
                    .map_err(execution_failed)?;
                serialize_operation_result(effect_type, operation, &result)
            }
            "derive_key" => {
                let params: (Vec<u8>, KeyDerivationContext) =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                let result = self
                    .core
                    .derive_key(&params.0, &params.1)
                    .await
                    .map_err(execution_failed)?;
                serialize_operation_result(effect_type, operation, &result)
            }
            "ed25519_generate_keypair" => {
                let result = self
                    .core
                    .ed25519_generate_keypair()
                    .await
                    .map_err(execution_failed)?;
                serialize_operation_result(effect_type, operation, &result)
            }
            "ed25519_sign" => {
                let params: (Vec<u8>, Vec<u8>) =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                let result = self
                    .core
                    .ed25519_sign(&params.0, &params.1)
                    .await
                    .map_err(execution_failed)?;
                serialize_operation_result(effect_type, operation, &result)
            }
            "ed25519_verify" => {
                let params: (Vec<u8>, Vec<u8>, Vec<u8>) =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                let result = self
                    .core
                    .ed25519_verify(&params.0, &params.1, &params.2)
                    .await
                    .map_err(execution_failed)?;
                serialize_operation_result(effect_type, operation, &result)
            }
            "ed25519_public_key" => {
                let private_key: Vec<u8> =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                let result = self
                    .extended_handler(effect_type, operation)?
                    .ed25519_public_key(&private_key)
                    .await
                    .map_err(execution_failed)?;
                serialize_operation_result(effect_type, operation, &result)
            }
            "generate_signing_keys" => {
                Err(Self::secret_material_result_error(effect_type, operation))
            }
            "sign_with_key" => {
                let params: (Vec<u8>, Vec<u8>, SigningMode) =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                let result = self
                    .extended_handler(effect_type, operation)?
                    .sign_with_key(&params.0, &params.1, params.2)
                    .await
                    .map_err(execution_failed)?;
                serialize_operation_result(effect_type, operation, &result)
            }
            "verify_signature" => {
                let params: (Vec<u8>, Vec<u8>, Vec<u8>, SigningMode) =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                let result = self
                    .extended_handler(effect_type, operation)?
                    .verify_signature(&params.0, &params.1, &params.2, params.3)
                    .await
                    .map_err(execution_failed)?;
                serialize_operation_result(effect_type, operation, &result)
            }
            "frost_generate_keys" => {
                Err(Self::secret_material_result_error(effect_type, operation))
            }
            "frost_generate_nonces" => {
                let key_package: Vec<u8> =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                let result = self
                    .extended_handler(effect_type, operation)?
                    .frost_generate_nonces(&key_package)
                    .await
                    .map_err(execution_failed)?;
                serialize_operation_result(effect_type, operation, &result)
            }
            "frost_create_signing_package" => {
                let params: (Vec<u8>, Vec<Vec<u8>>, Vec<u16>, Vec<u8>) =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                let result = self
                    .extended_handler(effect_type, operation)?
                    .frost_create_signing_package(&params.0, &params.1, &params.2, &params.3)
                    .await
                    .map_err(execution_failed)?;
                serialize_operation_result(effect_type, operation, &result)
            }
            "frost_sign_share" => {
                let params: (FrostSigningPackage, Vec<u8>, Vec<u8>) =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                let result = self
                    .extended_handler(effect_type, operation)?
                    .frost_sign_share(&params.0, &params.1, &params.2)
                    .await
                    .map_err(execution_failed)?;
                serialize_operation_result(effect_type, operation, &result)
            }
            "frost_aggregate_signatures" => {
                let params: (FrostSigningPackage, Vec<Vec<u8>>) =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                let result = self
                    .extended_handler(effect_type, operation)?
                    .frost_aggregate_signatures(&params.0, &params.1)
                    .await
                    .map_err(execution_failed)?;
                serialize_operation_result(effect_type, operation, &result)
            }
            "frost_verify" => {
                let params: (Vec<u8>, Vec<u8>, Vec<u8>) =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                let result = self
                    .extended_handler(effect_type, operation)?
                    .frost_verify(&params.0, &params.1, &params.2)
                    .await
                    .map_err(execution_failed)?;
                serialize_operation_result(effect_type, operation, &result)
            }
            "aes_gcm_encrypt" => {
                let params: (Vec<u8>, [u8; 32], [u8; 12]) =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                let result = self
                    .extended_handler(effect_type, operation)?
                    .aes_gcm_encrypt(&params.0, &params.1, &params.2)
                    .await
                    .map_err(execution_failed)?;
                serialize_operation_result(effect_type, operation, &result)
            }
            "aes_gcm_decrypt" => {
                let params: (Vec<u8>, [u8; 32], [u8; 12]) =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                let result = self
                    .extended_handler(effect_type, operation)?
                    .aes_gcm_decrypt(&params.0, &params.1, &params.2)
                    .await
                    .map_err(execution_failed)?;
                serialize_operation_result(effect_type, operation, &result)
            }
            "chacha20_encrypt" => {
                let params: (Vec<u8>, [u8; 32], [u8; 12]) =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                let result = self
                    .extended_handler(effect_type, operation)?
                    .chacha20_encrypt(&params.0, &params.1, &params.2)
                    .await
                    .map_err(execution_failed)?;
                serialize_operation_result(effect_type, operation, &result)
            }
            "chacha20_decrypt" => {
                let params: (Vec<u8>, [u8; 32], [u8; 12]) =
                    deserialize_operation_params(effect_type, operation, parameters)?;
                let result = self
                    .extended_handler(effect_type, operation)?
                    .chacha20_decrypt(&params.0, &params.1, &params.2)
                    .await
                    .map_err(execution_failed)?;
                serialize_operation_result(effect_type, operation, &result)
            }
            "frost_rotate_keys" => Err(Self::secret_material_result_error(effect_type, operation)),
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
