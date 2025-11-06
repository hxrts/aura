//! Shared Middleware Pattern for Effect Handlers
//!
//! This module provides a reusable pattern for implementing middleware that wraps
//! effect handlers. It reduces boilerplate code and ensures consistent middleware behavior.

/// A generic middleware wrapper that provides common middleware functionality
///
/// This pattern eliminates the need to manually implement delegation for every effect trait.
/// Instead, middleware can implement specific hooks and let this pattern handle the delegation.
pub trait MiddlewareHooks<H, C> {
    /// Called before each effect operation
    /// Can modify the operation or collect metrics
    ///
    /// # Arguments
    /// * `operation` - The effect operation being performed
    /// * `context` - The operation context
    #[allow(async_fn_in_trait)]
    async fn before_effect(
        &self,
        operation: &EffectOperation,
        context: &C,
    ) -> Result<(), EffectError>;

    /// Called after each effect operation
    /// Can process results or update metrics
    ///
    /// # Arguments
    /// * `operation` - The effect operation that was performed
    /// * `result` - The result of the operation
    /// * `context` - The operation context
    #[allow(async_fn_in_trait)]
    async fn after_effect(
        &self,
        operation: &EffectOperation,
        result: &EffectResult,
        context: &C,
    ) -> Result<(), EffectError>;

    /// Get the inner handler
    fn inner(&self) -> &H;
}

/// Describes an effect operation for middleware hooks
#[derive(Debug, Clone)]
pub enum EffectOperation {
    /// Network-related operation
    Network(NetworkOperation),
    /// Cryptographic operation
    Crypto(CryptoOperation),
    /// Time-related operation
    Time(TimeOperation),
    /// Storage operation
    Storage(StorageOperation),
}

/// Network-specific operations
#[derive(Debug, Clone)]
pub enum NetworkOperation {
    /// Send message to a specific peer
    SendToPeer {
        /// Peer identifier
        peer_id: String,
        /// Size of the message in bytes
        message_size: usize,
    },
    /// Broadcast message to all peers
    Broadcast {
        /// Size of the message in bytes
        message_size: usize,
    },
    /// Receive message from any peer
    Receive,
    /// Receive message from specific peer
    ReceiveFrom {
        /// Peer identifier
        peer_id: String,
    },
    /// Get list of connected peers
    ConnectedPeers,
    /// Check if specific peer is connected
    IsPeerConnected {
        /// Peer identifier
        peer_id: String,
    },
    /// Subscribe to peer connection events
    SubscribeToPeerEvents,
}

/// Crypto-specific operations
#[derive(Debug, Clone)]
pub enum CryptoOperation {
    /// Generate random bytes of specified length
    RandomBytes {
        /// Number of random bytes to generate
        len: usize,
    },
    /// Generate 32 random bytes
    RandomBytes32,
    /// Generate random number in range
    RandomRange,
    /// Compute BLAKE3 hash
    Blake3Hash {
        /// Size of data to hash
        data_size: usize,
    },
    /// Compute SHA256 hash
    Sha256Hash {
        /// Size of data to hash
        data_size: usize,
    },
    /// Sign data with Ed25519
    Ed25519Sign,
    /// Verify Ed25519 signature
    Ed25519Verify,
    /// Generate Ed25519 keypair
    Ed25519GenerateKeypair,
    /// Derive Ed25519 public key
    Ed25519PublicKey,
    /// Constant-time equality check
    ConstantTimeEq,
    /// Securely zero memory
    SecureZero,
}

/// Time-specific operations
#[derive(Debug, Clone)]
pub enum TimeOperation {
    /// Get current epoch timestamp
    CurrentEpoch,
    /// Sleep for milliseconds
    SleepMs {
        /// Duration in milliseconds
        duration_ms: u64,
    },
    /// Sleep until specific epoch
    SleepUntil {
        /// Target epoch timestamp
        epoch: u64,
    },
    /// Yield until condition met
    YieldUntil,
    /// Set a timeout
    SetTimeout {
        /// Timeout duration in milliseconds
        timeout_ms: u64,
    },
    /// Cancel a timeout
    CancelTimeout,
    /// Check if running in simulated time
    IsSimulated,
    /// Register time context
    RegisterContext,
    /// Unregister time context
    UnregisterContext,
    /// Notify that events are available
    NotifyEventsAvailable,
    /// Get time resolution in milliseconds
    ResolutionMs,
}

/// Storage-specific operations
#[derive(Debug, Clone)]
pub enum StorageOperation {
    /// Store a key-value pair
    Store {
        /// Storage key
        key: String,
        /// Size of value in bytes
        value_size: usize,
    },
    /// Retrieve value by key
    Retrieve {
        /// Storage key
        key: String,
    },
    /// Remove entry by key
    Remove {
        /// Storage key
        key: String,
    },
    /// List keys with optional prefix
    ListKeys {
        /// Optional key prefix filter
        prefix: Option<String>,
    },
    /// Check if key exists
    Exists {
        /// Storage key
        key: String,
    },
    /// Store multiple key-value pairs
    StoreBatch {
        /// Number of pairs in batch
        batch_size: usize,
    },
    /// Retrieve multiple values
    RetrieveBatch {
        /// Number of keys to retrieve
        key_count: usize,
    },
    /// Clear all stored data
    ClearAll,
    /// Get storage statistics
    Stats,
}

/// Result of an effect operation
#[derive(Debug, Clone)]
pub enum EffectResult {
    /// Operation succeeded
    Success,
    /// Operation failed with error
    Error(String),
}

/// Generic effect error
#[derive(Debug, thiserror::Error)]
pub enum EffectError {
    /// Middleware processing error
    #[error("Middleware error: {0}")]
    Middleware(String),
    /// Effect operation failed
    #[error("Operation failed: {0}")]
    Operation(String),
}

/// Macro to generate effect trait implementations with middleware hooks
///
/// This macro generates the boilerplate delegation code while calling middleware hooks.
/// It significantly reduces the code needed for implementing middleware.
///
/// # Arguments
/// * `$middleware` - The middleware type implementing MiddlewareHooks
/// * `$context` - The context type for the middleware operations
#[macro_export]
macro_rules! impl_middleware_effects {
    ($middleware:ident, $context:ty) => {
        // NetworkEffects implementation
        #[async_trait]
        impl<H: $crate::effects::NetworkEffects + Send + Sync> crate::effects::NetworkEffects
            for $middleware<H>
        where
            Self: MiddlewareHooks<H, $context>,
        {
            async fn send_to_peer(
                &self,
                peer_id: uuid::Uuid,
                message: Vec<u8>,
            ) -> Result<(), crate::effects::NetworkError> {
                let operation = EffectOperation::Network(NetworkOperation::SendToPeer {
                    peer_id: peer_id.to_string(),
                    message_size: message.len(),
                });

                // TODO: Create proper context
                let context = Default::default();

                self.before_effect(&operation, &context)
                    .await
                    .map_err(|_| crate::effects::NetworkError::ConnectionFailed)?;
                let result = self.inner().send_to_peer(peer_id, message).await;

                let effect_result = match &result {
                    Ok(_) => EffectResult::Success,
                    Err(e) => EffectResult::Error(e.to_string()),
                };

                self.after_effect(&operation, &effect_result, &context)
                    .await
                    .map_err(|_| crate::effects::NetworkError::ConnectionFailed)?;
                result
            }

            async fn broadcast(
                &self,
                message: Vec<u8>,
            ) -> Result<(), crate::effects::NetworkError> {
                let operation = EffectOperation::Network(NetworkOperation::Broadcast {
                    message_size: message.len(),
                });

                let context = Default::default();
                self.before_effect(&operation, &context)
                    .await
                    .map_err(|_| crate::effects::NetworkError::ConnectionFailed)?;
                let result = self.inner().broadcast(message).await;

                let effect_result = match &result {
                    Ok(_) => EffectResult::Success,
                    Err(e) => EffectResult::Error(e.to_string()),
                };

                self.after_effect(&operation, &effect_result, &context)
                    .await
                    .map_err(|_| crate::effects::NetworkError::ConnectionFailed)?;
                result
            }

            async fn receive(&self) -> Result<(uuid::Uuid, Vec<u8>), crate::effects::NetworkError> {
                let operation = EffectOperation::Network(NetworkOperation::Receive);
                let context = Default::default();

                self.before_effect(&operation, &context)
                    .await
                    .map_err(|_| crate::effects::NetworkError::ConnectionFailed)?;
                let result = self.inner().receive().await;

                let effect_result = match &result {
                    Ok(_) => EffectResult::Success,
                    Err(e) => EffectResult::Error(e.to_string()),
                };

                self.after_effect(&operation, &effect_result, &context)
                    .await
                    .map_err(|_| crate::effects::NetworkError::ConnectionFailed)?;
                result
            }

            async fn receive_from(
                &self,
                peer_id: uuid::Uuid,
            ) -> Result<Vec<u8>, crate::effects::NetworkError> {
                let operation = EffectOperation::Network(NetworkOperation::ReceiveFrom {
                    peer_id: peer_id.to_string(),
                });
                let context = Default::default();

                self.before_effect(&operation, &context)
                    .await
                    .map_err(|_| crate::effects::NetworkError::ConnectionFailed)?;
                let result = self.inner().receive_from(peer_id).await;

                let effect_result = match &result {
                    Ok(_) => EffectResult::Success,
                    Err(e) => EffectResult::Error(e.to_string()),
                };

                self.after_effect(&operation, &effect_result, &context)
                    .await
                    .map_err(|_| crate::effects::NetworkError::ConnectionFailed)?;
                result
            }

            async fn connected_peers(&self) -> Vec<uuid::Uuid> {
                self.inner().connected_peers().await
            }

            async fn is_peer_connected(&self, peer_id: uuid::Uuid) -> bool {
                self.inner().is_peer_connected(peer_id).await
            }

            async fn subscribe_to_peer_events(
                &self,
            ) -> Result<crate::effects::PeerEventStream, crate::effects::NetworkError> {
                self.inner().subscribe_to_peer_events().await
            }
        }

        // CryptoEffects implementation
        #[async_trait]
        impl<H: crate::effects::CryptoEffects + Send + Sync> crate::effects::CryptoEffects
            for $middleware<H>
        where
            Self: MiddlewareHooks<H, $context>,
        {
            async fn random_bytes(&self, len: usize) -> Vec<u8> {
                self.inner().random_bytes(len).await
            }

            async fn random_bytes_32(&self) -> [u8; 32] {
                self.inner().random_bytes_32().await
            }

            async fn random_range(&self, range: std::ops::Range<u64>) -> u64 {
                self.inner().random_range(range).await
            }

            async fn blake3_hash(&self, data: &[u8]) -> [u8; 32] {
                let operation = EffectOperation::Crypto(CryptoOperation::Blake3Hash {
                    data_size: data.len(),
                });
                let context = Default::default();

                let _ = self.before_effect(&operation, &context).await;
                let result = self.inner().blake3_hash(data).await;
                let _ = self
                    .after_effect(&operation, &EffectResult::Success, &context)
                    .await;

                result
            }

            async fn sha256_hash(&self, data: &[u8]) -> [u8; 32] {
                let operation = EffectOperation::Crypto(CryptoOperation::Sha256Hash {
                    data_size: data.len(),
                });
                let context = Default::default();

                let _ = self.before_effect(&operation, &context).await;
                let result = self.inner().sha256_hash(data).await;
                let _ = self
                    .after_effect(&operation, &EffectResult::Success, &context)
                    .await;

                result
            }

            async fn ed25519_sign(
                &self,
                data: &[u8],
                key: &ed25519_dalek::SigningKey,
            ) -> Result<ed25519_dalek::Signature, crate::effects::CryptoError> {
                self.inner().ed25519_sign(data, key).await
            }

            async fn ed25519_verify(
                &self,
                data: &[u8],
                signature: &ed25519_dalek::Signature,
                public_key: &ed25519_dalek::VerifyingKey,
            ) -> Result<bool, crate::effects::CryptoError> {
                self.inner()
                    .ed25519_verify(data, signature, public_key)
                    .await
            }

            async fn ed25519_generate_keypair(
                &self,
            ) -> Result<
                (ed25519_dalek::SigningKey, ed25519_dalek::VerifyingKey),
                crate::effects::CryptoError,
            > {
                self.inner().ed25519_generate_keypair().await
            }

            async fn ed25519_public_key(
                &self,
                private_key: &ed25519_dalek::SigningKey,
            ) -> ed25519_dalek::VerifyingKey {
                self.inner().ed25519_public_key(private_key).await
            }

            fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
                self.inner().constant_time_eq(a, b)
            }

            fn secure_zero(&self, data: &mut [u8]) {
                self.inner().secure_zero(data)
            }
        }

        // TimeEffects implementation
        #[async_trait]
        impl<H: crate::effects::TimeEffects + Send + Sync> crate::effects::TimeEffects
            for $middleware<H>
        where
            Self: MiddlewareHooks<H, $context>,
        {
            async fn current_epoch(&self) -> u64 {
                self.inner().current_epoch().await
            }

            async fn sleep_ms(&self, ms: u64) {
                let operation = EffectOperation::Time(TimeOperation::SleepMs { duration_ms: ms });
                let context = Default::default();

                let _ = self.before_effect(&operation, &context).await;
                self.inner().sleep_ms(ms).await;
                let _ = self
                    .after_effect(&operation, &EffectResult::Success, &context)
                    .await;
            }

            async fn sleep_until(&self, epoch: u64) {
                let operation = EffectOperation::Time(TimeOperation::SleepUntil { epoch });
                let context = Default::default();

                let _ = self.before_effect(&operation, &context).await;
                self.inner().sleep_until(epoch).await;
                let _ = self
                    .after_effect(&operation, &EffectResult::Success, &context)
                    .await;
            }

            async fn yield_until(
                &self,
                condition: crate::effects::WakeCondition,
            ) -> Result<(), crate::effects::TimeError> {
                self.inner().yield_until(condition).await
            }

            async fn set_timeout(&self, timeout_ms: u64) -> crate::effects::TimeoutHandle {
                let operation = EffectOperation::Time(TimeOperation::SetTimeout { timeout_ms });
                let context = Default::default();

                let _ = self.before_effect(&operation, &context).await;
                let result = self.inner().set_timeout(timeout_ms).await;
                let _ = self
                    .after_effect(&operation, &EffectResult::Success, &context)
                    .await;

                result
            }

            async fn cancel_timeout(
                &self,
                handle: crate::effects::TimeoutHandle,
            ) -> Result<(), crate::effects::TimeError> {
                self.inner().cancel_timeout(handle).await
            }

            fn is_simulated(&self) -> bool {
                self.inner().is_simulated()
            }

            fn register_context(&self, context_id: uuid::Uuid) {
                self.inner().register_context(context_id)
            }

            fn unregister_context(&self, context_id: uuid::Uuid) {
                self.inner().unregister_context(context_id)
            }

            async fn notify_events_available(&self) {
                self.inner().notify_events_available().await
            }

            fn resolution_ms(&self) -> u64 {
                self.inner().resolution_ms()
            }
        }

        // StorageEffects implementation
        #[async_trait]
        impl<H: crate::effects::StorageEffects + Send + Sync> crate::effects::StorageEffects
            for $middleware<H>
        where
            Self: MiddlewareHooks<H, $context>,
        {
            async fn store(
                &self,
                key: &str,
                value: Vec<u8>,
            ) -> Result<(), crate::effects::StorageError> {
                let operation = EffectOperation::Storage(StorageOperation::Store {
                    key: key.to_string(),
                    value_size: value.len(),
                });
                let context = Default::default();

                let _ = self.before_effect(&operation, &context).await;
                let result = self.inner().store(key, value).await;

                let effect_result = match &result {
                    Ok(_) => EffectResult::Success,
                    Err(e) => EffectResult::Error(e.to_string()),
                };

                let _ = self
                    .after_effect(&operation, &effect_result, &context)
                    .await;
                result
            }

            async fn retrieve(
                &self,
                key: &str,
            ) -> Result<Option<Vec<u8>>, crate::effects::StorageError> {
                self.inner().retrieve(key).await
            }

            async fn remove(&self, key: &str) -> Result<bool, crate::effects::StorageError> {
                self.inner().remove(key).await
            }

            async fn list_keys(
                &self,
                prefix: Option<&str>,
            ) -> Result<Vec<String>, crate::effects::StorageError> {
                self.inner().list_keys(prefix).await
            }

            async fn exists(&self, key: &str) -> Result<bool, crate::effects::StorageError> {
                self.inner().exists(key).await
            }

            async fn store_batch(
                &self,
                pairs: std::collections::HashMap<String, Vec<u8>>,
            ) -> Result<(), crate::effects::StorageError> {
                self.inner().store_batch(pairs).await
            }

            async fn retrieve_batch(
                &self,
                keys: &[String],
            ) -> Result<std::collections::HashMap<String, Vec<u8>>, crate::effects::StorageError>
            {
                self.inner().retrieve_batch(keys).await
            }

            async fn clear_all(&self) -> Result<(), crate::effects::StorageError> {
                self.inner().clear_all().await
            }

            async fn stats(
                &self,
            ) -> Result<crate::effects::StorageStats, crate::effects::StorageError> {
                self.inner().stats().await
            }
        }
    };
}
