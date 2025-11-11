//! Real network handler using actual network transport
//!
//! Provides real network communication for production use with:
//! - Exponential backoff retry logic
//! - Circuit breaker patterns for fault tolerance
//! - Timeout handling and rate limiting
//! - Connection health monitoring
//! - Message buffering and ordering

use aura_core::effects::{NetworkEffects, NetworkError, PeerEventStream};
use async_trait::async_trait;
use aura_core::{DeviceId, Receipt};
use aura_transport::{NetworkMessage, NetworkTransport};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock, Mutex};
use tokio::time::{timeout, sleep};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Configuration for network retry and reliability behavior
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    /// Maximum number of retry attempts for failed operations
    pub max_retries: usize,
    /// Initial delay between retries (exponential backoff)
    pub initial_retry_delay: Duration,
    /// Maximum delay between retries
    pub max_retry_delay: Duration,
    /// Timeout for individual network operations
    pub operation_timeout: Duration,
    /// Circuit breaker failure threshold
    pub circuit_breaker_failure_threshold: usize,
    /// Circuit breaker recovery timeout
    pub circuit_breaker_recovery_timeout: Duration,
    /// Rate limiting: max messages per time window
    pub rate_limit_max_messages: usize,
    /// Rate limiting: time window duration
    pub rate_limit_window: Duration,
    /// Maximum size of message buffer per peer
    pub max_message_buffer_size: usize,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_retry_delay: Duration::from_millis(100),
            max_retry_delay: Duration::from_secs(10),
            operation_timeout: Duration::from_secs(30),
            circuit_breaker_failure_threshold: 5,
            circuit_breaker_recovery_timeout: Duration::from_secs(60),
            rate_limit_max_messages: 100,
            rate_limit_window: Duration::from_secs(1),
            max_message_buffer_size: 1000,
        }
    }
}

/// Circuit breaker state for managing peer connections
#[derive(Debug, Clone, PartialEq)]
enum CircuitBreakerState {
    Closed,
    Open { opened_at: Instant },
    HalfOpen,
}

/// Per-peer connection state and health information
#[derive(Debug)]
struct PeerConnectionState {
    uuid: Uuid,
    device_id: DeviceId,
    circuit_breaker: CircuitBreakerState,
    failure_count: usize,
    last_failure: Option<Instant>,
    message_buffer: VecDeque<Vec<u8>>,
    rate_limiter: RateLimiter,
}

/// Simple rate limiter using a sliding window
#[derive(Debug)]
struct RateLimiter {
    messages: VecDeque<Instant>,
    max_messages: usize,
    window: Duration,
}

impl RateLimiter {
    fn new(max_messages: usize, window: Duration) -> Self {
        Self {
            messages: VecDeque::new(),
            max_messages,
            window,
        }
    }

    fn check_and_record(&mut self) -> bool {
        let now = Instant::now();
        
        // Remove old messages outside the window
        while let Some(&front_time) = self.messages.front() {
            if now.duration_since(front_time) > self.window {
                self.messages.pop_front();
            } else {
                break;
            }
        }
        
        // Check if we're within rate limit
        if self.messages.len() >= self.max_messages {
            false
        } else {
            self.messages.push_back(now);
            true
        }
    }
}

/// Real network handler for production use with comprehensive error handling
pub struct RealNetworkHandler {
    device_id: DeviceId,
    transport: Arc<NetworkTransport>,
    config: NetworkConfig,
    peer_states: Arc<RwLock<HashMap<Uuid, PeerConnectionState>>>,
    global_rate_limiter: Arc<Mutex<RateLimiter>>,
}

impl RealNetworkHandler {
    /// Create a new real network handler with transport integration and default config
    pub fn new(transport: Arc<NetworkTransport>) -> Self {
        Self::with_config(transport, NetworkConfig::default())
    }

    /// Create a new real network handler with custom configuration
    pub fn with_config(transport: Arc<NetworkTransport>, config: NetworkConfig) -> Self {
        let device_id = DeviceId::from(transport.device_id().0.to_string());
        let global_rate_limiter = RateLimiter::new(
            config.rate_limit_max_messages,
            config.rate_limit_window,
        );
        
        Self {
            device_id,
            transport,
            config,
            peer_states: Arc::new(RwLock::new(HashMap::new())),
            global_rate_limiter: Arc::new(Mutex::new(global_rate_limiter)),
        }
    }

    /// Register a UUID <-> DeviceId mapping for peer communication
    pub async fn register_peer(&self, uuid: Uuid, device_id: DeviceId) {
        let mut peer_states = self.peer_states.write().await;
        let rate_limiter = RateLimiter::new(
            self.config.rate_limit_max_messages,
            self.config.rate_limit_window,
        );

        peer_states.insert(uuid, PeerConnectionState {
            uuid,
            device_id,
            circuit_breaker: CircuitBreakerState::Closed,
            failure_count: 0,
            last_failure: None,
            message_buffer: VecDeque::new(),
            rate_limiter,
        });

        debug!("Registered peer {} with device_id {:?}", uuid, device_id);
    }

    /// Check if a peer's circuit breaker allows operations
    async fn check_circuit_breaker(&self, peer_uuid: Uuid) -> Result<(), NetworkError> {
        let peer_states = self.peer_states.read().await;
        
        if let Some(state) = peer_states.get(&peer_uuid) {
            match &state.circuit_breaker {
                CircuitBreakerState::Closed => Ok(()),
                CircuitBreakerState::Open { opened_at } => {
                    if opened_at.elapsed() >= self.config.circuit_breaker_recovery_timeout {
                        // Circuit breaker should transition to half-open
                        drop(peer_states);
                        self.set_circuit_breaker_state(peer_uuid, CircuitBreakerState::HalfOpen).await;
                        Ok(())
                    } else {
                        Err(NetworkError::CircuitBreakerOpen {
                            reason: format!("Peer {} circuit breaker is open", peer_uuid),
                        })
                    }
                }
                CircuitBreakerState::HalfOpen => {
                    // Allow one test operation
                    Ok(())
                }
            }
        } else {
            Err(NetworkError::PeerUnreachable {
                peer_id: peer_uuid.to_string(),
            })
        }
    }

    /// Update circuit breaker state for a peer
    async fn set_circuit_breaker_state(&self, peer_uuid: Uuid, state: CircuitBreakerState) {
        let mut peer_states = self.peer_states.write().await;
        if let Some(peer_state) = peer_states.get_mut(&peer_uuid) {
            peer_state.circuit_breaker = state;
        }
    }

    /// Record a failure for a peer and potentially open the circuit breaker
    async fn record_failure(&self, peer_uuid: Uuid, error: &str) {
        let mut peer_states = self.peer_states.write().await;
        
        if let Some(peer_state) = peer_states.get_mut(&peer_uuid) {
            peer_state.failure_count += 1;
            peer_state.last_failure = Some(Instant::now());

            warn!(
                "Peer {} failure count: {}, error: {}",
                peer_uuid, peer_state.failure_count, error
            );

            if peer_state.failure_count >= self.config.circuit_breaker_failure_threshold {
                peer_state.circuit_breaker = CircuitBreakerState::Open {
                    opened_at: Instant::now(),
                };
                error!("Circuit breaker opened for peer {} after {} failures", peer_uuid, peer_state.failure_count);
            }
        }
    }

    /// Record a success for a peer and potentially close the circuit breaker
    async fn record_success(&self, peer_uuid: Uuid) {
        let mut peer_states = self.peer_states.write().await;
        
        if let Some(peer_state) = peer_states.get_mut(&peer_uuid) {
            peer_state.failure_count = 0;
            peer_state.last_failure = None;

            match peer_state.circuit_breaker {
                CircuitBreakerState::HalfOpen => {
                    peer_state.circuit_breaker = CircuitBreakerState::Closed;
                    info!("Circuit breaker closed for peer {} after successful test", peer_uuid);
                }
                _ => {}
            }
        }
    }

    /// Check rate limiting for a peer
    async fn check_rate_limit(&self, peer_uuid: Uuid) -> Result<(), NetworkError> {
        // Check global rate limit first
        {
            let mut global_limiter = self.global_rate_limiter.lock().await;
            if !global_limiter.check_and_record() {
                return Err(NetworkError::RateLimitExceeded {
                    limit: self.config.rate_limit_max_messages,
                    window_ms: self.config.rate_limit_window.as_millis() as u64,
                });
            }
        }

        // Check per-peer rate limit
        let mut peer_states = self.peer_states.write().await;
        if let Some(peer_state) = peer_states.get_mut(&peer_uuid) {
            if !peer_state.rate_limiter.check_and_record() {
                return Err(NetworkError::RateLimitExceeded {
                    limit: self.config.rate_limit_max_messages,
                    window_ms: self.config.rate_limit_window.as_millis() as u64,
                });
            }
        }

        Ok(())
    }

    /// Get the device ID for a peer UUID
    async fn get_device_id(&self, peer_uuid: Uuid) -> Option<DeviceId> {
        let peer_states = self.peer_states.read().await;
        peer_states.get(&peer_uuid).map(|state| state.device_id)
    }

    /// Send a message with receipt using retry logic and circuit breaker
    pub async fn send_with_receipt(
        &self,
        peer_uuid: Uuid,
        message: Vec<u8>,
        receipt: Option<Receipt>,
    ) -> Result<(), NetworkError> {
        self.send_with_retry(peer_uuid, message, receipt).await
    }

    /// Send a message with comprehensive retry logic
    async fn send_with_retry(
        &self,
        peer_uuid: Uuid,
        message: Vec<u8>,
        receipt: Option<Receipt>,
    ) -> Result<(), NetworkError> {
        // Check circuit breaker first
        self.check_circuit_breaker(peer_uuid).await?;
        
        // Check rate limiting
        self.check_rate_limit(peer_uuid).await?;

        let device_id = self.get_device_id(peer_uuid).await
            .ok_or_else(|| NetworkError::PeerUnreachable {
                peer_id: peer_uuid.to_string(),
            })?;

        let mut last_error = String::new();
        let mut delay = self.config.initial_retry_delay;

        for attempt in 0..=self.config.max_retries {
            let result = timeout(
                self.config.operation_timeout,
                self.transport.send_with_receipt(
                    device_id, 
                    message.clone(), 
                    "data".to_string(), 
                    receipt.clone()
                )
            ).await;

            match result {
                Ok(Ok(())) => {
                    // Success - record it and return
                    self.record_success(peer_uuid).await;
                    debug!("Successfully sent message to peer {} after {} attempts", peer_uuid, attempt + 1);
                    return Ok(());
                }
                Ok(Err(transport_error)) => {
                    last_error = format!("Transport error: {}", transport_error);
                }
                Err(_timeout) => {
                    last_error = format!("Timeout after {}ms", self.config.operation_timeout.as_millis());
                }
            }

            // Record failure for circuit breaker tracking
            self.record_failure(peer_uuid, &last_error).await;

            // If this was our last attempt, break
            if attempt == self.config.max_retries {
                break;
            }

            // Wait before retry with exponential backoff
            warn!("Send attempt {} failed for peer {}: {}. Retrying in {:?}...", 
                  attempt + 1, peer_uuid, last_error, delay);
            
            sleep(delay).await;
            
            // Increase delay for next attempt (exponential backoff)
            delay = std::cmp::min(delay * 2, self.config.max_retry_delay);

            // Check circuit breaker again before retry
            if let Err(cb_error) = self.check_circuit_breaker(peer_uuid).await {
                return Err(cb_error);
            }
        }

        // All retries exhausted
        error!("Failed to send message to peer {} after {} attempts. Last error: {}", 
               peer_uuid, self.config.max_retries + 1, last_error);

        Err(NetworkError::RetriesExhausted {
            attempts: self.config.max_retries + 1,
            last_error,
        })
    }

    /// Receive and verify a message with timeout and error handling
    pub async fn receive_verified(&self) -> Result<(Uuid, Vec<u8>, Option<Receipt>), NetworkError> {
        let result = timeout(
            self.config.operation_timeout,
            self.transport.receive_verified()
        ).await;

        let message = match result {
            Ok(Ok(message)) => message,
            Ok(Err(transport_error)) => {
                return Err(NetworkError::ReceiveFailed(transport_error.to_string()));
            }
            Err(_timeout) => {
                return Err(NetworkError::Timeout {
                    operation: "receive_verified".to_string(),
                    timeout_ms: self.config.operation_timeout.as_millis() as u64,
                });
            }
        };

        // Look up the peer UUID for this device ID
        let peer_uuid = {
            let peer_states = self.peer_states.read().await;
            peer_states.iter()
                .find(|(_, state)| state.device_id == message.from)
                .map(|(uuid, _)| *uuid)
        };

        if let Some(uuid) = peer_uuid {
            debug!("Received message from registered peer {}", uuid);
            Ok((uuid, message.payload, message.receipt))
        } else {
            // Auto-register unknown peers
            let uuid = Uuid::new_v4();
            info!("Auto-registering new peer {} with device_id {:?}", uuid, message.from);
            self.register_peer(uuid, message.from).await;
            Ok((uuid, message.payload, message.receipt))
        }
    }

    /// Add a message to the buffer for a peer
    async fn buffer_message(&self, peer_uuid: Uuid, message: Vec<u8>) -> Result<(), NetworkError> {
        let mut peer_states = self.peer_states.write().await;
        
        if let Some(peer_state) = peer_states.get_mut(&peer_uuid) {
            if peer_state.message_buffer.len() >= self.config.max_message_buffer_size {
                // Remove oldest message to make room
                peer_state.message_buffer.pop_front();
                warn!("Message buffer full for peer {}, dropped oldest message", peer_uuid);
            }
            peer_state.message_buffer.push_back(message);
            Ok(())
        } else {
            Err(NetworkError::PeerUnreachable {
                peer_id: peer_uuid.to_string(),
            })
        }
    }

    /// Get a buffered message from a specific peer
    async fn get_buffered_message(&self, peer_uuid: Uuid) -> Option<Vec<u8>> {
        let mut peer_states = self.peer_states.write().await;
        peer_states.get_mut(&peer_uuid)?.message_buffer.pop_front()
    }
}

#[async_trait]
impl NetworkEffects for RealNetworkHandler {
    async fn send_to_peer(&self, peer_id: Uuid, message: Vec<u8>) -> Result<(), NetworkError> {
        self.send_with_receipt(peer_id, message, None).await
    }

    async fn broadcast(&self, message: Vec<u8>) -> Result<(), NetworkError> {
        let connected_peers = self.connected_peers().await;
        if connected_peers.is_empty() {
            warn!("No connected peers for broadcast");
            return Ok(());
        }

        let mut errors = Vec::new();
        let mut successful_sends = 0;

        for peer_id in &connected_peers {
            match self.send_to_peer(*peer_id, message.clone()).await {
                Ok(()) => {
                    successful_sends += 1;
                }
                Err(e) => {
                    errors.push((*peer_id, e));
                }
            }
        }

        debug!("Broadcast completed: {}/{} successful", successful_sends, connected_peers.len());

        // If more than half failed, consider it a network partition
        if successful_sends * 2 < connected_peers.len() {
            let error_details = errors.iter()
                .map(|(peer_id, error)| format!("{}:{}", peer_id, error))
                .collect::<Vec<_>>()
                .join(", ");
            
            return Err(NetworkError::NetworkPartition {
                details: format!("Broadcast failed to majority of peers: {}", error_details),
            });
        }

        // If some failed but majority succeeded, log warnings but return success
        for (peer_id, error) in errors {
            warn!("Failed to broadcast to peer {}: {}", peer_id, error);
        }

        Ok(())
    }

    async fn receive(&self) -> Result<(Uuid, Vec<u8>), NetworkError> {
        let (uuid, payload, _receipt) = self.receive_verified().await?;
        Ok((uuid, payload))
    }

    async fn receive_from(&self, peer_id: Uuid) -> Result<Vec<u8>, NetworkError> {
        // Check for buffered message first
        if let Some(buffered_message) = self.get_buffered_message(peer_id).await {
            return Ok(buffered_message);
        }

        // Receive messages and buffer those not from target peer
        let mut attempts = 0;
        const MAX_RECEIVE_ATTEMPTS: usize = 100; // Prevent infinite loops

        loop {
            if attempts >= MAX_RECEIVE_ATTEMPTS {
                return Err(NetworkError::Timeout {
                    operation: "receive_from".to_string(),
                    timeout_ms: self.config.operation_timeout.as_millis() as u64 * MAX_RECEIVE_ATTEMPTS as u64,
                });
            }

            attempts += 1;

            let (sender_uuid, payload) = self.receive().await?;
            if sender_uuid == peer_id {
                return Ok(payload);
            } else {
                // Buffer message from other peer
                if let Err(e) = self.buffer_message(sender_uuid, payload).await {
                    warn!("Failed to buffer message from peer {}: {}", sender_uuid, e);
                }
            }
        }
    }

    async fn connected_peers(&self) -> Vec<Uuid> {
        let transport_peers = self.transport.connected_peers().await;
        let peer_states = self.peer_states.read().await;
        
        transport_peers
            .into_iter()
            .filter_map(|device_id| {
                peer_states.iter()
                    .find(|(_, state)| state.device_id == device_id)
                    .map(|(uuid, _)| *uuid)
            })
            .collect()
    }

    async fn is_peer_connected(&self, peer_id: Uuid) -> bool {
        // Check circuit breaker status first
        if let Err(_) = self.check_circuit_breaker(peer_id).await {
            return false;
        }

        // Check transport connection status
        if let Some(device_id) = self.get_device_id(peer_id).await {
            self.transport.is_peer_connected(device_id).await
        } else {
            false
        }
    }

    async fn subscribe_to_peer_events(&self) -> Result<PeerEventStream, NetworkError> {
        let (sender, receiver) = mpsc::unbounded_channel();
        
        // TODO: Implement proper peer event subscription from transport layer
        // For now, we create a mock stream that would be replaced with real transport events
        // Real implementation would:
        // 1. Subscribe to transport layer connection events
        // 2. Map DeviceId events to UUID events
        // 3. Forward events through the channel
        
        tokio::spawn(async move {
            // This is a placeholder - in a real implementation, this would:
            // - Monitor transport layer for connection/disconnection events
            // - Convert DeviceId-based events to UUID-based events
            // - Send events through the channel
            // For now, we just keep the channel alive
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                // In production, this would monitor actual transport events
                // and send them via sender.send(PeerEvent::...)
            }
        });

        Ok(Box::pin(
            tokio_stream::wrappers::UnboundedReceiverStream::new(receiver),
        ))
    }
}
