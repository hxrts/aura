//! Metrics middleware for effect handlers
//!
//! Adds metrics collection to all effect operations.

use crate::effects::*;
use crate::middleware::Middleware;
use async_trait::async_trait;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use uuid::Uuid;

/// Metrics middleware that collects performance metrics for effect operations
pub struct MetricsMiddleware<H> {
    inner: H,
    device_id: Uuid,
    metrics: Arc<EffectMetrics>,
}

/// Collected metrics for effect operations
#[derive(Debug, Default)]
pub struct EffectMetrics {
    // Network metrics
    pub messages_sent: AtomicU64,
    pub messages_received: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub bytes_received: AtomicU64,
    pub network_errors: AtomicU64,

    // Crypto metrics
    pub signatures_created: AtomicU64,
    pub signatures_verified: AtomicU64,
    pub hashes_computed: AtomicU64,
    pub crypto_errors: AtomicU64,

    // Time metrics
    pub sleep_operations: AtomicU64,
    pub total_sleep_ms: AtomicU64,
    pub timeout_operations: AtomicU64,

    // Storage metrics
    pub storage_reads: AtomicU64,
    pub storage_writes: AtomicU64,
    pub storage_deletes: AtomicU64,
    pub storage_errors: AtomicU64,
}

impl EffectMetrics {
    /// Get a snapshot of current metrics
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            messages_sent: self.messages_sent.load(Ordering::Relaxed),
            messages_received: self.messages_received.load(Ordering::Relaxed),
            bytes_sent: self.bytes_sent.load(Ordering::Relaxed),
            bytes_received: self.bytes_received.load(Ordering::Relaxed),
            network_errors: self.network_errors.load(Ordering::Relaxed),
            signatures_created: self.signatures_created.load(Ordering::Relaxed),
            signatures_verified: self.signatures_verified.load(Ordering::Relaxed),
            hashes_computed: self.hashes_computed.load(Ordering::Relaxed),
            crypto_errors: self.crypto_errors.load(Ordering::Relaxed),
            sleep_operations: self.sleep_operations.load(Ordering::Relaxed),
            total_sleep_ms: self.total_sleep_ms.load(Ordering::Relaxed),
            timeout_operations: self.timeout_operations.load(Ordering::Relaxed),
            storage_reads: self.storage_reads.load(Ordering::Relaxed),
            storage_writes: self.storage_writes.load(Ordering::Relaxed),
            storage_deletes: self.storage_deletes.load(Ordering::Relaxed),
            storage_errors: self.storage_errors.load(Ordering::Relaxed),
        }
    }
}

/// Immutable snapshot of metrics at a point in time
#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    /// Total number of messages sent
    pub messages_sent: u64,
    /// Total number of messages received
    pub messages_received: u64,
    /// Total bytes sent over network
    pub bytes_sent: u64,
    /// Total bytes received from network
    pub bytes_received: u64,
    /// Total network errors encountered
    pub network_errors: u64,
    /// Total signatures created
    pub signatures_created: u64,
    /// Total signatures verified
    pub signatures_verified: u64,
    /// Total hashes computed
    pub hashes_computed: u64,
    /// Total cryptographic errors encountered
    pub crypto_errors: u64,
    /// Total sleep operations
    pub sleep_operations: u64,
    /// Total milliseconds spent sleeping
    pub total_sleep_ms: u64,
    /// Total timeout operations
    pub timeout_operations: u64,
    /// Total storage read operations
    pub storage_reads: u64,
    /// Total storage write operations
    pub storage_writes: u64,
    /// Total storage delete operations
    pub storage_deletes: u64,
    /// Total storage errors encountered
    pub storage_errors: u64,
}

impl<H> MetricsMiddleware<H> {
    /// Create a new metrics middleware
    pub fn new(handler: H, device_id: Uuid) -> Self {
        Self {
            inner: handler,
            device_id,
            metrics: Arc::new(EffectMetrics::default()),
        }
    }

    /// Get the collected metrics
    pub fn metrics(&self) -> Arc<EffectMetrics> {
        self.metrics.clone()
    }
}

impl<H> Middleware<H> for MetricsMiddleware<H> {
    type Decorated = MetricsMiddleware<H>;

    fn apply(self, handler: H) -> Self::Decorated {
        MetricsMiddleware::new(handler, self.device_id)
    }
}

// Implement NetworkEffects with metrics
#[async_trait]
impl<H: NetworkEffects + Send + Sync> NetworkEffects for MetricsMiddleware<H> {
    async fn send_to_peer(&self, peer_id: Uuid, message: Vec<u8>) -> Result<(), NetworkError> {
        let message_len = message.len() as u64;
        let result = self.inner.send_to_peer(peer_id, message).await;

        match &result {
            Ok(()) => {
                self.metrics.messages_sent.fetch_add(1, Ordering::Relaxed);
                self.metrics
                    .bytes_sent
                    .fetch_add(message_len, Ordering::Relaxed);
            }
            Err(_) => {
                self.metrics.network_errors.fetch_add(1, Ordering::Relaxed);
            }
        }

        result
    }

    async fn broadcast(&self, message: Vec<u8>) -> Result<(), NetworkError> {
        let message_len = message.len() as u64;
        let result = self.inner.broadcast(message).await;

        match &result {
            Ok(()) => {
                // Count as one message sent (broadcast)
                self.metrics.messages_sent.fetch_add(1, Ordering::Relaxed);
                self.metrics
                    .bytes_sent
                    .fetch_add(message_len, Ordering::Relaxed);
            }
            Err(_) => {
                self.metrics.network_errors.fetch_add(1, Ordering::Relaxed);
            }
        }

        result
    }

    async fn receive(&self) -> Result<(Uuid, Vec<u8>), NetworkError> {
        let result = self.inner.receive().await;

        match &result {
            Ok((_, message)) => {
                self.metrics
                    .messages_received
                    .fetch_add(1, Ordering::Relaxed);
                self.metrics
                    .bytes_received
                    .fetch_add(message.len() as u64, Ordering::Relaxed);
            }
            Err(_) => {
                self.metrics.network_errors.fetch_add(1, Ordering::Relaxed);
            }
        }

        result
    }

    async fn receive_from(&self, peer_id: Uuid) -> Result<Vec<u8>, NetworkError> {
        let result = self.inner.receive_from(peer_id).await;

        match &result {
            Ok(message) => {
                self.metrics
                    .messages_received
                    .fetch_add(1, Ordering::Relaxed);
                self.metrics
                    .bytes_received
                    .fetch_add(message.len() as u64, Ordering::Relaxed);
            }
            Err(_) => {
                self.metrics.network_errors.fetch_add(1, Ordering::Relaxed);
            }
        }

        result
    }

    async fn connected_peers(&self) -> Vec<Uuid> {
        self.inner.connected_peers().await
    }

    async fn is_peer_connected(&self, peer_id: Uuid) -> bool {
        self.inner.is_peer_connected(peer_id).await
    }

    async fn subscribe_to_peer_events(&self) -> Result<PeerEventStream, NetworkError> {
        self.inner.subscribe_to_peer_events().await
    }
}

// Implement CryptoEffects with metrics
#[async_trait]
impl<H: CryptoEffects + Send + Sync> CryptoEffects for MetricsMiddleware<H> {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        self.inner.random_bytes(len).await
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        self.inner.random_bytes_32().await
    }

    async fn random_range(&self, range: std::ops::Range<u64>) -> u64 {
        self.inner.random_range(range).await
    }

    async fn blake3_hash(&self, data: &[u8]) -> [u8; 32] {
        let result = self.inner.blake3_hash(data).await;
        self.metrics.hashes_computed.fetch_add(1, Ordering::Relaxed);
        result
    }

    async fn sha256_hash(&self, data: &[u8]) -> [u8; 32] {
        let result = self.inner.sha256_hash(data).await;
        self.metrics.hashes_computed.fetch_add(1, Ordering::Relaxed);
        result
    }

    async fn ed25519_sign(
        &self,
        data: &[u8],
        key: &ed25519_dalek::SigningKey,
    ) -> Result<ed25519_dalek::Signature, CryptoError> {
        let result = self.inner.ed25519_sign(data, key).await;

        match &result {
            Ok(_) => {
                self.metrics
                    .signatures_created
                    .fetch_add(1, Ordering::Relaxed);
            }
            Err(_) => {
                self.metrics.crypto_errors.fetch_add(1, Ordering::Relaxed);
            }
        }

        result
    }

    async fn ed25519_verify(
        &self,
        data: &[u8],
        signature: &ed25519_dalek::Signature,
        public_key: &ed25519_dalek::VerifyingKey,
    ) -> Result<bool, CryptoError> {
        let result = self.inner.ed25519_verify(data, signature, public_key).await;

        match &result {
            Ok(_) => {
                self.metrics
                    .signatures_verified
                    .fetch_add(1, Ordering::Relaxed);
            }
            Err(_) => {
                self.metrics.crypto_errors.fetch_add(1, Ordering::Relaxed);
            }
        }

        result
    }

    async fn ed25519_generate_keypair(
        &self,
    ) -> Result<(ed25519_dalek::SigningKey, ed25519_dalek::VerifyingKey), CryptoError> {
        self.inner.ed25519_generate_keypair().await
    }

    async fn ed25519_public_key(
        &self,
        private_key: &ed25519_dalek::SigningKey,
    ) -> ed25519_dalek::VerifyingKey {
        self.inner.ed25519_public_key(private_key).await
    }

    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
        self.inner.constant_time_eq(a, b)
    }

    fn secure_zero(&self, data: &mut [u8]) {
        self.inner.secure_zero(data)
    }
}

// Implement TimeEffects with metrics
#[async_trait]
impl<H: TimeEffects + Send + Sync> TimeEffects for MetricsMiddleware<H> {
    async fn current_epoch(&self) -> u64 {
        self.inner.current_epoch().await
    }

    async fn sleep_ms(&self, ms: u64) {
        self.metrics
            .sleep_operations
            .fetch_add(1, Ordering::Relaxed);
        self.metrics.total_sleep_ms.fetch_add(ms, Ordering::Relaxed);
        self.inner.sleep_ms(ms).await
    }

    async fn sleep_until(&self, epoch: u64) {
        self.metrics
            .sleep_operations
            .fetch_add(1, Ordering::Relaxed);
        self.inner.sleep_until(epoch).await
    }

    async fn yield_until(&self, condition: WakeCondition) -> Result<(), TimeError> {
        self.inner.yield_until(condition).await
    }

    async fn set_timeout(&self, timeout_ms: u64) -> TimeoutHandle {
        self.metrics
            .timeout_operations
            .fetch_add(1, Ordering::Relaxed);
        self.inner.set_timeout(timeout_ms).await
    }

    async fn cancel_timeout(&self, handle: TimeoutHandle) -> Result<(), TimeError> {
        self.inner.cancel_timeout(handle).await
    }

    fn is_simulated(&self) -> bool {
        self.inner.is_simulated()
    }

    fn register_context(&self, context_id: Uuid) {
        self.inner.register_context(context_id)
    }

    fn unregister_context(&self, context_id: Uuid) {
        self.inner.unregister_context(context_id)
    }

    async fn notify_events_available(&self) {
        self.inner.notify_events_available().await
    }

    fn resolution_ms(&self) -> u64 {
        self.inner.resolution_ms()
    }
}
