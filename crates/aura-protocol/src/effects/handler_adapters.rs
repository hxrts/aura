//! Aura handler adapters for stateless effect execution
//!
//! This module bridges the concrete `aura-effects` handlers with the
//! `AuraHandler` trait expected by the stateless effect executor. Each
//! adapter is responsible for (de)serializing the byte-level parameters
//! used by the executor and delegating to the underlying effect trait
//! implementation without holding any additional state.

use std::{collections::HashMap, error::Error, io, sync::Arc, time::Duration};

use async_trait::async_trait;
use aura_core::{
    effects::{
        crypto::{FrostSigningPackage, KeyDerivationContext},
        ConsoleEffects, CryptoEffects, JournalEffects, NetworkEffects, RandomEffects,
        StorageEffects, TimeEffects, TimeoutHandle, WakeCondition,
    },
    hash::hash,
    relationships::ContextId,
    DeviceId, FlowBudget, Journal,
};
use serde::{de::DeserializeOwned, Serialize};

use crate::handlers::{
    context_immutable::AuraContext, AuraHandler, AuraHandlerError, EffectType, ExecutionMode,
};

/// Helper to serialize a value into bytes using bincode with rich error context.
fn serialize_with_context<T: Serialize>(
    value: &T,
    effect_type: EffectType,
    operation: &str,
) -> Result<Vec<u8>, AuraHandlerError> {
    bincode::serialize(value).map_err(|e| AuraHandlerError::EffectSerialization {
        effect_type,
        operation: operation.to_string(),
        source: Box::new(e),
    })
}

/// Helper to deserialize a value from bytes with context-aware errors.
fn deserialize_with_context<T: DeserializeOwned>(
    bytes: &[u8],
    effect_type: EffectType,
    operation: &str,
) -> Result<T, AuraHandlerError> {
    bincode::deserialize(bytes).map_err(|e| AuraHandlerError::EffectDeserialization {
        effect_type,
        operation: operation.to_string(),
        source: Box::new(e),
    })
}

/// Produce a boxed invalid-data error for adapter conversions.
fn invalid_data_error(message: impl Into<String>) -> Box<dyn Error + Send + Sync> {
    Box::new(io::Error::new(io::ErrorKind::InvalidData, message.into()))
}

/// Convenience macro to implement common AuraHandler methods for adapters.
macro_rules! impl_handler_meta {
    ($ty:ty, $effect_variant:expr) => {
        fn supports_effect(&self, effect_type: EffectType) -> bool {
            effect_type == $effect_variant
        }

        fn execution_mode(&self) -> ExecutionMode {
            self.mode
        }
    };
}

/// Adapter for `TimeEffects` implementations.
pub struct TimeHandlerAdapter<T> {
    inner: Arc<T>,
    mode: ExecutionMode,
}

impl<T> TimeHandlerAdapter<T>
where
    T: TimeEffects + Send + Sync + 'static,
{
    pub fn new(inner: T, mode: ExecutionMode) -> Self {
        Self {
            inner: Arc::new(inner),
            mode,
        }
    }

    fn inner(&self) -> &T {
        &self.inner
    }
}

#[async_trait]
impl<T> AuraHandler for TimeHandlerAdapter<T>
where
    T: TimeEffects + Send + Sync + 'static,
{
    async fn execute_effect(
        &self,
        _effect_type: EffectType,
        operation: &str,
        params: &[u8],
        _ctx: &AuraContext,
    ) -> Result<Vec<u8>, AuraHandlerError> {
        let effect_type = EffectType::Time;
        Ok(match operation {
            "current_epoch" => self.inner().current_epoch().await.to_le_bytes().to_vec(),
            "current_timestamp" => self
                .inner()
                .current_timestamp()
                .await
                .to_le_bytes()
                .to_vec(),
            "current_timestamp_millis" => self
                .inner()
                .current_timestamp_millis()
                .await
                .to_le_bytes()
                .to_vec(),
            "sleep_ms" => {
                let duration = deserialize_with_context::<u64>(params, effect_type, operation)?;
                self.inner().sleep_ms(duration).await;
                Vec::new()
            }
            "sleep_until" => {
                let epoch = deserialize_with_context::<u64>(params, effect_type, operation)?;
                self.inner().sleep_until(epoch).await;
                Vec::new()
            }
            "delay" => {
                let duration =
                    deserialize_with_context::<Duration>(params, effect_type, operation)?;
                self.inner().delay(duration).await;
                Vec::new()
            }
            "yield_until" => {
                let condition =
                    deserialize_with_context::<WakeCondition>(params, effect_type, operation)?;
                self.inner().yield_until(condition).await.map_err(|e| {
                    AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                Vec::new()
            }
            "set_timeout" => {
                let timeout_ms = deserialize_with_context::<u64>(params, effect_type, operation)?;
                let handle = self.inner().set_timeout(timeout_ms).await;
                serialize_with_context(&handle, effect_type, operation)?
            }
            "cancel_timeout" => {
                let handle =
                    deserialize_with_context::<TimeoutHandle>(params, effect_type, operation)?;
                self.inner().cancel_timeout(handle).await.map_err(|e| {
                    AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                Vec::new()
            }
            "notify_events_available" => {
                self.inner().notify_events_available().await;
                Vec::new()
            }
            _ => {
                return Err(AuraHandlerError::UnsupportedOperation {
                    effect_type,
                    operation: operation.to_string(),
                })
            }
        })
    }

    async fn execute_session(
        &self,
        _session: aura_core::LocalSessionType,
        _ctx: &AuraContext,
    ) -> Result<(), AuraHandlerError> {
        Err(AuraHandlerError::UnsupportedOperation {
            effect_type: EffectType::Time,
            operation: "session".to_string(),
        })
    }

    impl_handler_meta!(Self, EffectType::Time);
}

/// Adapter for `NetworkEffects` implementations.
pub struct NetworkHandlerAdapter<T> {
    inner: Arc<T>,
    mode: ExecutionMode,
}

impl<T> NetworkHandlerAdapter<T>
where
    T: NetworkEffects + Send + Sync + 'static,
{
    pub fn new(inner: T, mode: ExecutionMode) -> Self {
        Self {
            inner: Arc::new(inner),
            mode,
        }
    }

    fn inner(&self) -> &T {
        &self.inner
    }
}

#[async_trait]
impl<T> AuraHandler for NetworkHandlerAdapter<T>
where
    T: NetworkEffects + Send + Sync + 'static,
{
    async fn execute_effect(
        &self,
        _effect_type: EffectType,
        operation: &str,
        params: &[u8],
        _ctx: &AuraContext,
    ) -> Result<Vec<u8>, AuraHandlerError> {
        let effect_type = EffectType::Network;
        Ok(match operation {
            "send_to_peer" => {
                let (peer_id, payload) = deserialize_with_context::<(uuid::Uuid, Vec<u8>)>(
                    params,
                    effect_type,
                    operation,
                )?;
                self.inner()
                    .send_to_peer(peer_id, payload)
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                Vec::new()
            }
            "broadcast" => {
                self.inner().broadcast(params.to_vec()).await.map_err(|e| {
                    AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                Vec::new()
            }
            "receive" => {
                let received = self.inner().receive().await.map_err(|e| {
                    AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                serialize_with_context(&received, effect_type, operation)?
            }
            "receive_from" => {
                let peer = deserialize_with_context::<uuid::Uuid>(params, effect_type, operation)?;
                let payload = self.inner().receive_from(peer).await.map_err(|e| {
                    AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                serialize_with_context(&payload, effect_type, operation)?
            }
            "connected_peers" => {
                let peers = self.inner().connected_peers().await;
                serialize_with_context(&peers, effect_type, operation)?
            }
            "is_peer_connected" => {
                let peer = deserialize_with_context::<uuid::Uuid>(params, effect_type, operation)?;
                let is_connected = self.inner().is_peer_connected(peer).await;
                serialize_with_context(&is_connected, effect_type, operation)?
            }
            "subscribe_to_peer_events" => {
                return Err(AuraHandlerError::UnsupportedOperation {
                    effect_type,
                    operation: operation.to_string(),
                })
            }
            _ => {
                return Err(AuraHandlerError::UnsupportedOperation {
                    effect_type,
                    operation: operation.to_string(),
                })
            }
        })
    }

    async fn execute_session(
        &self,
        _session: aura_core::LocalSessionType,
        _ctx: &AuraContext,
    ) -> Result<(), AuraHandlerError> {
        Err(AuraHandlerError::UnsupportedOperation {
            effect_type: EffectType::Network,
            operation: "session".to_string(),
        })
    }

    impl_handler_meta!(Self, EffectType::Network);
}

/// Adapter for `CryptoEffects` implementations.
pub struct CryptoHandlerAdapter<T> {
    inner: Arc<T>,
    mode: ExecutionMode,
}

impl<T> CryptoHandlerAdapter<T>
where
    T: CryptoEffects + Send + Sync + 'static,
{
    pub fn new(inner: T, mode: ExecutionMode) -> Self {
        Self {
            inner: Arc::new(inner),
            mode,
        }
    }

    fn inner(&self) -> &T {
        &self.inner
    }
}

fn to_fixed_array<const N: usize>(
    bytes: Vec<u8>,
    effect_type: EffectType,
    operation: &str,
    field: &str,
) -> Result<[u8; N], AuraHandlerError> {
    {
        let len = bytes.len();
        bytes
            .try_into()
            .map_err(|_| AuraHandlerError::EffectDeserialization {
                effect_type,
                operation: operation.to_string(),
                source: invalid_data_error(format!(
                    "Expected {} bytes for {} but received {}",
                    N,
                    field,
                    len
                )),
            })
    }
}

#[async_trait]
impl<T> AuraHandler for CryptoHandlerAdapter<T>
where
    T: CryptoEffects + Send + Sync + 'static,
{
    async fn execute_effect(
        &self,
        _effect_type: EffectType,
        operation: &str,
        params: &[u8],
        _ctx: &AuraContext,
    ) -> Result<Vec<u8>, AuraHandlerError> {
        let effect_type = EffectType::Crypto;
        Ok(match operation {
            "hash" => hash(params).to_vec(),
            "hmac" => {
                let (key, data) =
                    deserialize_with_context::<(Vec<u8>, Vec<u8>)>(params, effect_type, operation)?;
                // HMAC is not an algebraic effect - use simple hash for now
                let mut combined = Vec::new();
                combined.extend_from_slice(&key);
                combined.extend_from_slice(&data);
                hash(&combined).to_vec()
            }
            "ed25519_sign" => {
                let (message, private_key) =
                    deserialize_with_context::<(Vec<u8>, Vec<u8>)>(params, effect_type, operation)?;
                self.inner()
                    .ed25519_sign(&message, &private_key)
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?
            }
            "ed25519_verify" => {
                let (message, signature, public_key) =
                    deserialize_with_context::<(Vec<u8>, Vec<u8>, Vec<u8>)>(
                        params,
                        effect_type,
                        operation,
                    )?;
                let verified = self
                    .inner()
                    .ed25519_verify(&message, &signature, &public_key)
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                serialize_with_context(&verified, effect_type, operation)?
            }
            "ed25519_generate_keypair" => {
                let pair = self.inner().ed25519_generate_keypair().await.map_err(|e| {
                    AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                serialize_with_context(&pair, effect_type, operation)?
            }
            "ed25519_public_key" => {
                let public_key = self.inner().ed25519_public_key(params).await.map_err(|e| {
                    AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                public_key
            }
            "hkdf_derive" => {
                let (ikm, salt, info, len) = deserialize_with_context::<(
                    Vec<u8>,
                    Vec<u8>,
                    Vec<u8>,
                    usize,
                )>(params, effect_type, operation)?;
                self.inner()
                    .hkdf_derive(&ikm, &salt, &info, len)
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?
            }
            "derive_key" => {
                let (master_key, context) = deserialize_with_context::<(
                    Vec<u8>,
                    KeyDerivationContext,
                )>(params, effect_type, operation)?;
                self.inner()
                    .derive_key(&master_key, &context)
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?
            }
            "frost_generate_keys" => {
                let (threshold, max_signers) =
                    deserialize_with_context::<(u16, u16)>(params, effect_type, operation)?;
                let shares = self
                    .inner()
                    .frost_generate_keys(threshold, max_signers)
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                serialize_with_context(&shares, effect_type, operation)?
            }
            "frost_generate_nonces" => self.inner().frost_generate_nonces().await.map_err(|e| {
                AuraHandlerError::ExecutionFailed {
                    source: Box::new(e),
                }
            })?,
            "frost_create_signing_package" => {
                let (message, nonces, participants) =
                    deserialize_with_context::<(Vec<u8>, Vec<Vec<u8>>, Vec<u16>)>(
                        params,
                        effect_type,
                        operation,
                    )?;
                let package = self
                    .inner()
                    .frost_create_signing_package(&message, &nonces, &participants)
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                serialize_with_context(&package, effect_type, operation)?
            }
            "frost_sign_share" => {
                let (package, key_share, nonces) =
                    deserialize_with_context::<(FrostSigningPackage, Vec<u8>, Vec<u8>)>(
                        params,
                        effect_type,
                        operation,
                    )?;
                self.inner()
                    .frost_sign_share(&package, &key_share, &nonces)
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?
            }
            "frost_aggregate_signatures" => {
                let (package, shares) = deserialize_with_context::<(
                    FrostSigningPackage,
                    Vec<Vec<u8>>,
                )>(params, effect_type, operation)?;
                self.inner()
                    .frost_aggregate_signatures(&package, &shares)
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?
            }
            "frost_verify" => {
                let (message, signature, group_key) =
                    deserialize_with_context::<(Vec<u8>, Vec<u8>, Vec<u8>)>(
                        params,
                        effect_type,
                        operation,
                    )?;
                let verified = self
                    .inner()
                    .frost_verify(&message, &signature, &group_key)
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                serialize_with_context(&verified, effect_type, operation)?
            }
            "frost_rotate_keys" => {
                let (shares, old_threshold, new_threshold, new_max) =
                    deserialize_with_context::<(Vec<Vec<u8>>, u16, u16, u16)>(
                        params,
                        effect_type,
                        operation,
                    )?;
                let rotated = self
                    .inner()
                    .frost_rotate_keys(&shares, old_threshold, new_threshold, new_max)
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                serialize_with_context(&rotated, effect_type, operation)?
            }
            "chacha20_encrypt" | "chacha20_decrypt" | "aes_gcm_encrypt" | "aes_gcm_decrypt" => {
                let (payload, key_vec, nonce_vec) =
                    deserialize_with_context::<(Vec<u8>, Vec<u8>, Vec<u8>)>(
                        params,
                        effect_type,
                        operation,
                    )?;
                let key = to_fixed_array::<32>(key_vec, effect_type, operation, "key")?;
                let nonce = to_fixed_array::<12>(nonce_vec, effect_type, operation, "nonce")?;
                match operation {
                    "chacha20_encrypt" => {
                        self.inner().chacha20_encrypt(&payload, &key, &nonce).await
                    }
                    "chacha20_decrypt" => {
                        self.inner().chacha20_decrypt(&payload, &key, &nonce).await
                    }
                    "aes_gcm_encrypt" => self.inner().aes_gcm_encrypt(&payload, &key, &nonce).await,
                    "aes_gcm_decrypt" => self.inner().aes_gcm_decrypt(&payload, &key, &nonce).await,
                    _ => unreachable!(),
                }
                .map_err(|e| AuraHandlerError::ExecutionFailed {
                    source: Box::new(e),
                })?
            }
            _ => {
                return Err(AuraHandlerError::UnsupportedOperation {
                    effect_type,
                    operation: operation.to_string(),
                })
            }
        })
    }

    async fn execute_session(
        &self,
        _session: aura_core::LocalSessionType,
        _ctx: &AuraContext,
    ) -> Result<(), AuraHandlerError> {
        Err(AuraHandlerError::UnsupportedOperation {
            effect_type: EffectType::Crypto,
            operation: "session".to_string(),
        })
    }

    impl_handler_meta!(Self, EffectType::Crypto);
}

/// Adapter for `StorageEffects` implementations.
pub struct StorageHandlerAdapter<T> {
    inner: Arc<T>,
    mode: ExecutionMode,
}

impl<T> StorageHandlerAdapter<T>
where
    T: StorageEffects + Send + Sync + 'static,
{
    pub fn new(inner: T, mode: ExecutionMode) -> Self {
        Self {
            inner: Arc::new(inner),
            mode,
        }
    }

    fn inner(&self) -> &T {
        &self.inner
    }
}

fn decode_utf8_param(
    params: &[u8],
    effect_type: EffectType,
    operation: &str,
) -> Result<String, AuraHandlerError> {
    std::str::from_utf8(params)
        .map(|s| s.to_string())
        .map_err(|e| AuraHandlerError::EffectDeserialization {
            effect_type,
            operation: operation.to_string(),
            source: Box::new(e),
        })
}

#[async_trait]
impl<T> AuraHandler for StorageHandlerAdapter<T>
where
    T: StorageEffects + Send + Sync + 'static,
{
    async fn execute_effect(
        &self,
        _effect_type: EffectType,
        operation: &str,
        params: &[u8],
        _ctx: &AuraContext,
    ) -> Result<Vec<u8>, AuraHandlerError> {
        let effect_type = EffectType::Storage;
        Ok(match operation {
            "store" => {
                let (key, value) =
                    deserialize_with_context::<(String, Vec<u8>)>(params, effect_type, operation)?;
                self.inner().store(&key, value).await.map_err(|e| {
                    AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                Vec::new()
            }
            "retrieve" => {
                let key = decode_utf8_param(params, effect_type, operation)?;
                let value = self.inner().retrieve(&key).await.map_err(|e| {
                    AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                serialize_with_context(&value, effect_type, operation)?
            }
            "remove" => {
                let key = decode_utf8_param(params, effect_type, operation)?;
                let removed = self.inner().remove(&key).await.map_err(|e| {
                    AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                serialize_with_context(&removed, effect_type, operation)?
            }
            "list_keys" => {
                let prefix =
                    deserialize_with_context::<Option<String>>(params, effect_type, operation)?;
                let result = self
                    .inner()
                    .list_keys(prefix.as_deref())
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                serialize_with_context(&result, effect_type, operation)?
            }
            "exists" => {
                let key = decode_utf8_param(params, effect_type, operation)?;
                let exists = self.inner().exists(&key).await.map_err(|e| {
                    AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                serialize_with_context(&exists, effect_type, operation)?
            }
            "store_batch" => {
                let batch = deserialize_with_context::<HashMap<String, Vec<u8>>>(
                    params,
                    effect_type,
                    operation,
                )?;
                self.inner().store_batch(batch).await.map_err(|e| {
                    AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                Vec::new()
            }
            "retrieve_batch" => {
                let keys = deserialize_with_context::<Vec<String>>(params, effect_type, operation)?;
                let result = self.inner().retrieve_batch(&keys).await.map_err(|e| {
                    AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                serialize_with_context(&result, effect_type, operation)?
            }
            "clear_all" => {
                self.inner()
                    .clear_all()
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                Vec::new()
            }
            "stats" => {
                let stats =
                    self.inner()
                        .stats()
                        .await
                        .map_err(|e| AuraHandlerError::ExecutionFailed {
                            source: Box::new(e),
                        })?;
                serialize_with_context(&stats, effect_type, operation)?
            }
            "get_usage" => {
                let stats =
                    self.inner()
                        .stats()
                        .await
                        .map_err(|e| AuraHandlerError::ExecutionFailed {
                            source: Box::new(e),
                        })?;
                serialize_with_context(&stats.total_size, effect_type, operation)?
            }
            _ => {
                return Err(AuraHandlerError::UnsupportedOperation {
                    effect_type,
                    operation: operation.to_string(),
                })
            }
        })
    }

    async fn execute_session(
        &self,
        _session: aura_core::LocalSessionType,
        _ctx: &AuraContext,
    ) -> Result<(), AuraHandlerError> {
        Err(AuraHandlerError::UnsupportedOperation {
            effect_type: EffectType::Storage,
            operation: "session".to_string(),
        })
    }

    impl_handler_meta!(Self, EffectType::Storage);
}

/// Adapter for `ConsoleEffects` implementations.
pub struct ConsoleHandlerAdapter<T> {
    inner: Arc<T>,
    mode: ExecutionMode,
}

impl<T> ConsoleHandlerAdapter<T>
where
    T: ConsoleEffects + Send + Sync + 'static,
{
    pub fn new(inner: T, mode: ExecutionMode) -> Self {
        Self {
            inner: Arc::new(inner),
            mode,
        }
    }

    fn inner(&self) -> &T {
        &self.inner
    }
}

#[async_trait]
impl<T> AuraHandler for ConsoleHandlerAdapter<T>
where
    T: ConsoleEffects + Send + Sync + 'static,
{
    async fn execute_effect(
        &self,
        _effect_type: EffectType,
        operation: &str,
        params: &[u8],
        _ctx: &AuraContext,
    ) -> Result<Vec<u8>, AuraHandlerError> {
        let effect_type = EffectType::Console;
        let message = deserialize_with_context::<String>(params, effect_type, operation)?;
        match operation {
            "log_info" => self.inner().log_info(&message).await.map_err(|e| {
                AuraHandlerError::ExecutionFailed {
                    source: Box::new(e),
                }
            })?,
            "log_warn" => self.inner().log_warn(&message).await.map_err(|e| {
                AuraHandlerError::ExecutionFailed {
                    source: Box::new(e),
                }
            })?,
            "log_error" => self.inner().log_error(&message).await.map_err(|e| {
                AuraHandlerError::ExecutionFailed {
                    source: Box::new(e),
                }
            })?,
            "log_debug" => self.inner().log_debug(&message).await.map_err(|e| {
                AuraHandlerError::ExecutionFailed {
                    source: Box::new(e),
                }
            })?,
            _ => {
                return Err(AuraHandlerError::UnsupportedOperation {
                    effect_type,
                    operation: operation.to_string(),
                })
            }
        }
        Ok(Vec::new())
    }

    async fn execute_session(
        &self,
        _session: aura_core::LocalSessionType,
        _ctx: &AuraContext,
    ) -> Result<(), AuraHandlerError> {
        Err(AuraHandlerError::UnsupportedOperation {
            effect_type: EffectType::Console,
            operation: "session".to_string(),
        })
    }

    impl_handler_meta!(Self, EffectType::Console);
}

/// Adapter for `RandomEffects` implementations.
pub struct RandomHandlerAdapter<T> {
    inner: Arc<T>,
    mode: ExecutionMode,
}

impl<T> RandomHandlerAdapter<T>
where
    T: RandomEffects + Send + Sync + 'static,
{
    pub fn new(inner: T, mode: ExecutionMode) -> Self {
        Self {
            inner: Arc::new(inner),
            mode,
        }
    }

    fn inner(&self) -> &T {
        &self.inner
    }
}

#[async_trait]
impl<T> AuraHandler for RandomHandlerAdapter<T>
where
    T: RandomEffects + Send + Sync + 'static,
{
    async fn execute_effect(
        &self,
        _effect_type: EffectType,
        operation: &str,
        params: &[u8],
        _ctx: &AuraContext,
    ) -> Result<Vec<u8>, AuraHandlerError> {
        let effect_type = EffectType::Random;
        Ok(match operation {
            "random_bytes" => {
                let len = deserialize_with_context::<usize>(params, effect_type, operation)?;
                self.inner().random_bytes(len).await
            }
            "random_u64" => {
                serialize_with_context(&self.inner().random_u64().await, effect_type, operation)?
            }
            "random_bytes_32" => self.inner().random_bytes_32().await.to_vec(),
            "random_range" => {
                let (min, max) =
                    deserialize_with_context::<(u64, u64)>(params, effect_type, operation)?;
                let value = self.inner().random_range(min, max).await;
                serialize_with_context(&value, effect_type, operation)?
            }
            _ => {
                return Err(AuraHandlerError::UnsupportedOperation {
                    effect_type,
                    operation: operation.to_string(),
                })
            }
        })
    }

    async fn execute_session(
        &self,
        _session: aura_core::LocalSessionType,
        _ctx: &AuraContext,
    ) -> Result<(), AuraHandlerError> {
        Err(AuraHandlerError::UnsupportedOperation {
            effect_type: EffectType::Random,
            operation: "session".to_string(),
        })
    }

    impl_handler_meta!(Self, EffectType::Random);
}

/// Adapter for `JournalEffects` implementations.
pub struct JournalHandlerAdapter<T> {
    inner: Arc<T>,
    mode: ExecutionMode,
}

impl<T> JournalHandlerAdapter<T>
where
    T: JournalEffects + Send + Sync + 'static,
{
    pub fn new(inner: T, mode: ExecutionMode) -> Self {
        Self {
            inner: Arc::new(inner),
            mode,
        }
    }

    fn inner(&self) -> &T {
        &self.inner
    }
}

#[async_trait]
impl<T> AuraHandler for JournalHandlerAdapter<T>
where
    T: JournalEffects + Send + Sync + 'static,
{
    async fn execute_effect(
        &self,
        _effect_type: EffectType,
        operation: &str,
        params: &[u8],
        _ctx: &AuraContext,
    ) -> Result<Vec<u8>, AuraHandlerError> {
        let effect_type = EffectType::Journal;
        Ok(match operation {
            "merge_facts" => {
                let (target, delta) =
                    deserialize_with_context::<(Journal, Journal)>(params, effect_type, operation)?;
                let merged = self
                    .inner()
                    .merge_facts(&target, &delta)
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                serialize_with_context(&merged, effect_type, operation)?
            }
            "refine_caps" => {
                let (target, refinement) =
                    deserialize_with_context::<(Journal, Journal)>(params, effect_type, operation)?;
                let refined = self
                    .inner()
                    .refine_caps(&target, &refinement)
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                serialize_with_context(&refined, effect_type, operation)?
            }
            "get_journal" => {
                let journal = self.inner().get_journal().await.map_err(|e| {
                    AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                serialize_with_context(&journal, effect_type, operation)?
            }
            "persist_journal" => {
                let journal = deserialize_with_context::<Journal>(params, effect_type, operation)?;
                self.inner().persist_journal(&journal).await.map_err(|e| {
                    AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;
                Vec::new()
            }
            "get_flow_budget" => {
                let (context, peer) = deserialize_with_context::<(ContextId, DeviceId)>(
                    params,
                    effect_type,
                    operation,
                )?;
                let budget = self
                    .inner()
                    .get_flow_budget(&context, &peer)
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                serialize_with_context(&budget, effect_type, operation)?
            }
            "update_flow_budget" => {
                let (context, peer, budget) = deserialize_with_context::<(
                    ContextId,
                    DeviceId,
                    FlowBudget,
                )>(params, effect_type, operation)?;
                let updated = self
                    .inner()
                    .update_flow_budget(&context, &peer, &budget)
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                serialize_with_context(&updated, effect_type, operation)?
            }
            "charge_flow_budget" => {
                let (context, peer, cost) = deserialize_with_context::<(ContextId, DeviceId, u32)>(
                    params,
                    effect_type,
                    operation,
                )?;
                let charged = self
                    .inner()
                    .charge_flow_budget(&context, &peer, cost)
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;
                serialize_with_context(&charged, effect_type, operation)?
            }
            _ => {
                return Err(AuraHandlerError::UnsupportedOperation {
                    effect_type,
                    operation: operation.to_string(),
                })
            }
        })
    }

    async fn execute_session(
        &self,
        _session: aura_core::LocalSessionType,
        _ctx: &AuraContext,
    ) -> Result<(), AuraHandlerError> {
        Err(AuraHandlerError::UnsupportedOperation {
            effect_type: EffectType::Journal,
            operation: "session".to_string(),
        })
    }

    impl_handler_meta!(Self, EffectType::Journal);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::immutable::AuraContext;
    use aura_core::identifiers::DeviceId;
    use aura_effects::{storage::MemoryStorageHandler, time::SimulatedTimeHandler};
    use uuid::Uuid;

    #[tokio::test]
    async fn time_adapter_serializes_timestamp() {
        let adapter =
            TimeHandlerAdapter::new(SimulatedTimeHandler::new_at_epoch(), ExecutionMode::Testing);
        let ctx = AuraContext::for_testing(DeviceId::from(Uuid::nil()));

        let bytes = adapter
            .execute_effect(EffectType::Time, "current_timestamp_millis", &[], &ctx)
            .await
            .expect("time effect should succeed");

        let mut timestamp_bytes = [0u8; 8];
        timestamp_bytes.copy_from_slice(&bytes[..8]);
        assert_eq!(u64::from_le_bytes(timestamp_bytes), 0);
    }

    #[tokio::test]
    async fn storage_adapter_round_trips_values() {
        let adapter =
            StorageHandlerAdapter::new(MemoryStorageHandler::new(), ExecutionMode::Testing);
        let ctx = AuraContext::for_testing(DeviceId::from(Uuid::nil()));

        let store_params = bincode::serialize(&("demo".to_string(), vec![1u8, 2, 3])).unwrap();
        adapter
            .execute_effect(EffectType::Storage, "store", &store_params, &ctx)
            .await
            .expect("store effect");

        let retrieved = adapter
            .execute_effect(EffectType::Storage, "retrieve", b"demo", &ctx)
            .await
            .expect("retrieve effect");

        let value: Option<Vec<u8>> = bincode::deserialize(&retrieved).unwrap();
        assert_eq!(value, Some(vec![1, 2, 3]));
    }
}
