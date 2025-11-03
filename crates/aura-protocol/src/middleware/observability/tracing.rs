//! Tracing middleware for effect handlers
//!
//! Adds distributed tracing to all effect operations.

use crate::effects::*;
use crate::middleware::Middleware;
use async_trait::async_trait;
use tracing::instrument;
use uuid::Uuid;

/// Tracing middleware that adds distributed tracing to effect operations
pub struct TracingMiddleware<H> {
    inner: H,
    device_id: Uuid,
    service_name: String,
}

impl<H> TracingMiddleware<H> {
    /// Create a new tracing middleware
    pub fn new(handler: H, device_id: Uuid, service_name: String) -> Self {
        Self {
            inner: handler,
            device_id,
            service_name,
        }
    }
}

impl<H> Middleware<H> for TracingMiddleware<H> {
    type Decorated = TracingMiddleware<H>;

    fn apply(self, handler: H) -> Self::Decorated {
        TracingMiddleware::new(handler, self.device_id, self.service_name)
    }
}

// Implement NetworkEffects with tracing
#[async_trait]
impl<H: NetworkEffects + Send + Sync> NetworkEffects for TracingMiddleware<H> {
    #[instrument(skip(self, message), fields(device_id = %self.device_id, peer_id = %peer_id))]
    async fn send_to_peer(&self, peer_id: Uuid, message: Vec<u8>) -> Result<(), NetworkError> {
        tracing::debug!("Sending message to peer {} ({} bytes)", peer_id, message.len());
        let result = self.inner.send_to_peer(peer_id, message).await;
        match &result {
            Ok(()) => tracing::debug!("Successfully sent message to peer {}", peer_id),
            Err(e) => tracing::warn!("Failed to send message to peer {}: {}", peer_id, e),
        }
        result
    }

    #[instrument(skip(self, message), fields(device_id = %self.device_id))]
    async fn broadcast(&self, message: Vec<u8>) -> Result<(), NetworkError> {
        tracing::debug!("Broadcasting message ({} bytes)", message.len());
        let result = self.inner.broadcast(message).await;
        match &result {
            Ok(()) => tracing::debug!("Successfully broadcasted message"),
            Err(e) => tracing::warn!("Failed to broadcast message: {}", e),
        }
        result
    }

    #[instrument(skip(self), fields(device_id = %self.device_id))]
    async fn receive(&self) -> Result<(Uuid, Vec<u8>), NetworkError> {
        tracing::debug!("Waiting to receive message");
        let result = self.inner.receive().await;
        match &result {
            Ok((from, message)) => tracing::debug!("Received message from {} ({} bytes)", from, message.len()),
            Err(e) => tracing::debug!("Receive failed: {}", e),
        }
        result
    }

    #[instrument(skip(self), fields(device_id = %self.device_id, peer_id = %peer_id))]
    async fn receive_from(&self, peer_id: Uuid) -> Result<Vec<u8>, NetworkError> {
        tracing::debug!("Waiting to receive message from peer {}", peer_id);
        let result = self.inner.receive_from(peer_id).await;
        match &result {
            Ok(message) => tracing::debug!("Received message from {} ({} bytes)", peer_id, message.len()),
            Err(e) => tracing::debug!("Receive from {} failed: {}", peer_id, e),
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

// Implement CryptoEffects with tracing
#[async_trait]
impl<H: CryptoEffects + Send + Sync> CryptoEffects for TracingMiddleware<H> {
    #[instrument(skip(self), fields(device_id = %self.device_id))]
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        tracing::trace!("Generating {} random bytes", len);
        self.inner.random_bytes(len).await
    }

    #[instrument(skip(self), fields(device_id = %self.device_id))]
    async fn random_bytes_32(&self) -> [u8; 32] {
        tracing::trace!("Generating 32 random bytes (array)");
        self.inner.random_bytes_32().await
    }

    #[instrument(skip(self), fields(device_id = %self.device_id))]
    async fn random_range(&self, range: std::ops::Range<u64>) -> u64 {
        tracing::trace!("Generating random number in range {:?}", range);
        self.inner.random_range(range).await
    }

    #[instrument(skip(self, data), fields(device_id = %self.device_id))]
    async fn blake3_hash(&self, data: &[u8]) -> [u8; 32] {
        tracing::trace!("Computing Blake3 hash of {} bytes", data.len());
        self.inner.blake3_hash(data).await
    }

    #[instrument(skip(self, data), fields(device_id = %self.device_id))]
    async fn sha256_hash(&self, data: &[u8]) -> [u8; 32] {
        tracing::trace!("Computing SHA256 hash of {} bytes", data.len());
        self.inner.sha256_hash(data).await
    }

    #[instrument(skip(self, data, key), fields(device_id = %self.device_id))]
    async fn ed25519_sign(&self, data: &[u8], key: &ed25519_dalek::SigningKey) -> Result<ed25519_dalek::Signature, CryptoError> {
        tracing::debug!("Signing {} bytes with Ed25519", data.len());
        let result = self.inner.ed25519_sign(data, key).await;
        match &result {
            Ok(_) => tracing::debug!("Successfully signed data"),
            Err(e) => tracing::warn!("Signing failed: {}", e),
        }
        result
    }

    #[instrument(skip(self, data, signature, public_key), fields(device_id = %self.device_id))]
    async fn ed25519_verify(&self, data: &[u8], signature: &ed25519_dalek::Signature, public_key: &ed25519_dalek::VerifyingKey) -> Result<bool, CryptoError> {
        tracing::debug!("Verifying Ed25519 signature for {} bytes", data.len());
        let result = self.inner.ed25519_verify(data, signature, public_key).await;
        match &result {
            Ok(true) => tracing::debug!("Signature verification succeeded"),
            Ok(false) => tracing::warn!("Signature verification failed"),
            Err(e) => tracing::warn!("Signature verification error: {}", e),
        }
        result
    }

    #[instrument(skip(self), fields(device_id = %self.device_id))]
    async fn ed25519_generate_keypair(&self) -> Result<(ed25519_dalek::SigningKey, ed25519_dalek::VerifyingKey), CryptoError> {
        tracing::debug!("Generating Ed25519 keypair");
        let result = self.inner.ed25519_generate_keypair().await;
        match &result {
            Ok(_) => tracing::debug!("Successfully generated keypair"),
            Err(e) => tracing::warn!("Keypair generation failed: {}", e),
        }
        result
    }

    async fn ed25519_public_key(&self, private_key: &ed25519_dalek::SigningKey) -> ed25519_dalek::VerifyingKey {
        self.inner.ed25519_public_key(private_key).await
    }

    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
        self.inner.constant_time_eq(a, b)
    }

    fn secure_zero(&self, data: &mut [u8]) {
        self.inner.secure_zero(data)
    }
}

// Add implementations for other effect traits as needed...
// For brevity, I'll implement the most important ones

#[async_trait]
impl<H: TimeEffects + Send + Sync> TimeEffects for TracingMiddleware<H> {
    async fn current_epoch(&self) -> u64 {
        self.inner.current_epoch().await
    }

    #[instrument(skip(self), fields(device_id = %self.device_id))]
    async fn sleep_ms(&self, ms: u64) {
        tracing::trace!("Sleeping for {}ms", ms);
        self.inner.sleep_ms(ms).await
    }

    #[instrument(skip(self), fields(device_id = %self.device_id))]
    async fn sleep_until(&self, epoch: u64) {
        tracing::trace!("Sleeping until epoch {}", epoch);
        self.inner.sleep_until(epoch).await
    }

    #[instrument(skip(self), fields(device_id = %self.device_id))]
    async fn yield_until(&self, condition: WakeCondition) -> Result<(), TimeError> {
        tracing::trace!("Yielding until condition: {:?}", condition);
        self.inner.yield_until(condition).await
    }

    async fn set_timeout(&self, timeout_ms: u64) -> TimeoutHandle {
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