//! Retry middleware for effect handlers
//!
//! Adds automatic retry logic to failed effect operations.

use crate::effects::*;
use crate::middleware::Middleware;
use async_trait::async_trait;
use std::time::Duration;
use tokio::time::sleep;
use uuid::Uuid;

/// Retry middleware that automatically retries failed operations
pub struct RetryMiddleware<H> {
    inner: H,
    config: RetryConfig,
}

/// Configuration for retry behavior
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Base delay between retries in milliseconds
    pub base_delay_ms: u64,
    /// Maximum delay between retries in milliseconds
    pub max_delay_ms: u64,
    /// Backoff multiplier (exponential backoff)
    pub backoff_multiplier: f64,
    /// Whether to add jitter to retry delays
    pub use_jitter: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay_ms: 100,
            max_delay_ms: 5000,
            backoff_multiplier: 2.0,
            use_jitter: true,
        }
    }
}

impl<H> RetryMiddleware<H> {
    /// Create a new retry middleware
    pub fn new(handler: H, config: RetryConfig) -> Self {
        Self {
            inner: handler,
            config,
        }
    }

    /// Calculate delay for a retry attempt
    fn calculate_delay(&self, attempt: u32) -> Duration {
        let mut delay_ms = self.config.base_delay_ms;
        
        // Apply exponential backoff
        for _ in 0..attempt {
            delay_ms = ((delay_ms as f64) * self.config.backoff_multiplier) as u64;
        }
        
        // Cap at maximum delay
        delay_ms = delay_ms.min(self.config.max_delay_ms);
        
        // Add jitter if enabled
        if self.config.use_jitter && delay_ms > 0 {
            use rand::Rng;
            let jitter = rand::thread_rng().gen_range(0..=delay_ms / 4);
            delay_ms += jitter;
        }
        
        Duration::from_millis(delay_ms)
    }

    /// Check if an error should be retried
    fn should_retry<E>(&self, _error: &E, attempt: u32) -> bool {
        attempt < self.config.max_retries
    }

    /// Execute an operation with retry logic
    async fn retry_with_attempts<T, E, F>(&self, mut operation: F) -> Result<T, E>
    where
        F: FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, E>> + Send>>,
        E: std::fmt::Debug,
    {
        let mut last_error = None;
        
        for attempt in 0..=self.config.max_retries {
            match operation().await {
                Ok(result) => return Ok(result),
                Err(error) => {
                    last_error = Some(error);
                    
                    // Don't sleep after the last attempt
                    if attempt < self.config.max_retries && self.should_retry(last_error.as_ref().unwrap(), attempt) {
                        let delay = self.calculate_delay(attempt);
                        tracing::debug!("Retrying operation after {:?} (attempt {}/{})", delay, attempt + 1, self.config.max_retries);
                        sleep(delay).await;
                    }
                }
            }
        }
        
        Err(last_error.unwrap())
    }
}

impl<H> Middleware<H> for RetryMiddleware<H> {
    type Decorated = RetryMiddleware<H>;

    fn apply(self, handler: H) -> Self::Decorated {
        RetryMiddleware::new(handler, self.config)
    }
}

// Implement NetworkEffects with retry logic using direct implementation
#[async_trait]
impl<H: NetworkEffects + Send + Sync> NetworkEffects for RetryMiddleware<H> {
    async fn send_to_peer(&self, peer_id: Uuid, message: Vec<u8>) -> Result<(), NetworkError> {
        let mut last_error = None;
        for attempt in 0..=self.config.max_retries {
            match self.inner.send_to_peer(peer_id, message.clone()).await {
                Ok(result) => return Ok(result),
                Err(error) => {
                    last_error = Some(error);
                    if attempt < self.config.max_retries && self.should_retry(last_error.as_ref().unwrap(), attempt) {
                        let delay = self.calculate_delay(attempt);
                        sleep(delay).await;
                    }
                }
            }
        }
        Err(last_error.unwrap())
    }

    async fn broadcast(&self, message: Vec<u8>) -> Result<(), NetworkError> {
        let mut last_error = None;
        for attempt in 0..=self.config.max_retries {
            match self.inner.broadcast(message.clone()).await {
                Ok(result) => return Ok(result),
                Err(error) => {
                    last_error = Some(error);
                    if attempt < self.config.max_retries && self.should_retry(last_error.as_ref().unwrap(), attempt) {
                        let delay = self.calculate_delay(attempt);
                        sleep(delay).await;
                    }
                }
            }
        }
        Err(last_error.unwrap())
    }

    async fn receive(&self) -> Result<(Uuid, Vec<u8>), NetworkError> {
        // Receive operations typically shouldn't be retried automatically
        // as they might consume messages that should only be processed once
        self.inner.receive().await
    }

    async fn receive_from(&self, peer_id: Uuid) -> Result<Vec<u8>, NetworkError> {
        // Same as receive - typically shouldn't be retried
        self.inner.receive_from(peer_id).await
    }

    async fn connected_peers(&self) -> Vec<Uuid> {
        // Query operations can be retried but usually don't fail
        self.inner.connected_peers().await
    }

    async fn is_peer_connected(&self, peer_id: Uuid) -> bool {
        // Query operations can be retried but usually don't fail
        self.inner.is_peer_connected(peer_id).await
    }

    async fn subscribe_to_peer_events(&self) -> Result<PeerEventStream, NetworkError> {
        let mut last_error = None;
        for attempt in 0..=self.config.max_retries {
            match self.inner.subscribe_to_peer_events().await {
                Ok(result) => return Ok(result),
                Err(error) => {
                    last_error = Some(error);
                    if attempt < self.config.max_retries && self.should_retry(last_error.as_ref().unwrap(), attempt) {
                        let delay = self.calculate_delay(attempt);
                        sleep(delay).await;
                    }
                }
            }
        }
        Err(last_error.unwrap())
    }
}

// Implement StorageEffects with retry logic using direct implementation
#[async_trait]
impl<H: StorageEffects + Send + Sync> StorageEffects for RetryMiddleware<H> {
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        let key = key.to_string();
        let mut last_error = None;
        for attempt in 0..=self.config.max_retries {
            match self.inner.store(&key, value.clone()).await {
                Ok(result) => return Ok(result),
                Err(error) => {
                    last_error = Some(error);
                    if attempt < self.config.max_retries && self.should_retry(last_error.as_ref().unwrap(), attempt) {
                        let delay = self.calculate_delay(attempt);
                        sleep(delay).await;
                    }
                }
            }
        }
        Err(last_error.unwrap())
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let key = key.to_string();
        let mut last_error = None;
        for attempt in 0..=self.config.max_retries {
            match self.inner.retrieve(&key).await {
                Ok(result) => return Ok(result),
                Err(error) => {
                    last_error = Some(error);
                    if attempt < self.config.max_retries && self.should_retry(last_error.as_ref().unwrap(), attempt) {
                        let delay = self.calculate_delay(attempt);
                        sleep(delay).await;
                    }
                }
            }
        }
        Err(last_error.unwrap())
    }

    async fn remove(&self, key: &str) -> Result<bool, StorageError> {
        let key = key.to_string();
        let mut last_error = None;
        for attempt in 0..=self.config.max_retries {
            match self.inner.remove(&key).await {
                Ok(result) => return Ok(result),
                Err(error) => {
                    last_error = Some(error);
                    if attempt < self.config.max_retries && self.should_retry(last_error.as_ref().unwrap(), attempt) {
                        let delay = self.calculate_delay(attempt);
                        sleep(delay).await;
                    }
                }
            }
        }
        Err(last_error.unwrap())
    }

    async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        let prefix_owned = prefix.map(|s| s.to_string());
        let mut last_error = None;
        for attempt in 0..=self.config.max_retries {
            let prefix_ref = prefix_owned.as_deref();
            match self.inner.list_keys(prefix_ref).await {
                Ok(result) => return Ok(result),
                Err(error) => {
                    last_error = Some(error);
                    if attempt < self.config.max_retries && self.should_retry(last_error.as_ref().unwrap(), attempt) {
                        let delay = self.calculate_delay(attempt);
                        sleep(delay).await;
                    }
                }
            }
        }
        Err(last_error.unwrap())
    }

    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        let key = key.to_string();
        let mut last_error = None;
        for attempt in 0..=self.config.max_retries {
            match self.inner.exists(&key).await {
                Ok(result) => return Ok(result),
                Err(error) => {
                    last_error = Some(error);
                    if attempt < self.config.max_retries && self.should_retry(last_error.as_ref().unwrap(), attempt) {
                        let delay = self.calculate_delay(attempt);
                        sleep(delay).await;
                    }
                }
            }
        }
        Err(last_error.unwrap())
    }

    async fn store_batch(&self, pairs: std::collections::HashMap<String, Vec<u8>>) -> Result<(), StorageError> {
        let mut last_error = None;
        for attempt in 0..=self.config.max_retries {
            match self.inner.store_batch(pairs.clone()).await {
                Ok(result) => return Ok(result),
                Err(error) => {
                    last_error = Some(error);
                    if attempt < self.config.max_retries && self.should_retry(last_error.as_ref().unwrap(), attempt) {
                        let delay = self.calculate_delay(attempt);
                        sleep(delay).await;
                    }
                }
            }
        }
        Err(last_error.unwrap())
    }

    async fn retrieve_batch(&self, keys: &[String]) -> Result<std::collections::HashMap<String, Vec<u8>>, StorageError> {
        let keys_owned = keys.to_vec();
        let mut last_error = None;
        for attempt in 0..=self.config.max_retries {
            match self.inner.retrieve_batch(&keys_owned).await {
                Ok(result) => return Ok(result),
                Err(error) => {
                    last_error = Some(error);
                    if attempt < self.config.max_retries && self.should_retry(last_error.as_ref().unwrap(), attempt) {
                        let delay = self.calculate_delay(attempt);
                        sleep(delay).await;
                    }
                }
            }
        }
        Err(last_error.unwrap())
    }

    async fn clear_all(&self) -> Result<(), StorageError> {
        let mut last_error = None;
        for attempt in 0..=self.config.max_retries {
            match self.inner.clear_all().await {
                Ok(result) => return Ok(result),
                Err(error) => {
                    last_error = Some(error);
                    if attempt < self.config.max_retries && self.should_retry(last_error.as_ref().unwrap(), attempt) {
                        let delay = self.calculate_delay(attempt);
                        sleep(delay).await;
                    }
                }
            }
        }
        Err(last_error.unwrap())
    }

    async fn stats(&self) -> Result<StorageStats, StorageError> {
        let mut last_error = None;
        for attempt in 0..=self.config.max_retries {
            match self.inner.stats().await {
                Ok(result) => return Ok(result),
                Err(error) => {
                    last_error = Some(error);
                    if attempt < self.config.max_retries && self.should_retry(last_error.as_ref().unwrap(), attempt) {
                        let delay = self.calculate_delay(attempt);
                        sleep(delay).await;
                    }
                }
            }
        }
        Err(last_error.unwrap())
    }
}