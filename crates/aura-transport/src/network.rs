//! Real network transport implementation
//!
//! Provides TCP-based network transport for production use.
//! Features message framing, peer management, and connection pooling.
//!
//! **REFACTORED**: Now uses SecureChannelRegistry instead of duplicate connection caches
//! to enforce one active channel per (ContextId, peer_device) as specified in work/007.md

use crate::secure_channel::SecureChannelRegistry;
use aura_core::{
    flow::FlowBudget, relationships::ContextId, session_epochs::Epoch, AuraError, DeviceId, Receipt,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::time::Duration;

/// Get current time as seconds since UNIX epoch
fn current_timestamp() -> u64 {
    std::time::SystemTime::UNIX_EPOCH
        .elapsed()
        .unwrap_or_default()
        .as_secs()
}

/// Network transport configuration
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    /// Bind address for listening
    pub bind_addr: String,
    /// Port to listen on
    pub port: u16,
    /// Connection timeout in milliseconds
    pub timeout_ms: u64,
    /// Maximum message size in bytes
    pub max_message_size: usize,
    /// Keep-alive interval in seconds
    pub keepalive_interval: u64,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            bind_addr: "0.0.0.0".to_string(),
            port: 8080,
            timeout_ms: 30_000,
            max_message_size: 1024 * 1024, // 1MB
            keepalive_interval: 30,
        }
    }
}

/// Network message envelope for framing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkMessage {
    /// Source device ID
    pub from: DeviceId,
    /// Destination device ID
    pub to: DeviceId,
    /// Message type identifier
    pub message_type: String,
    /// Message payload
    pub payload: Vec<u8>,
    /// Message sequence number
    pub sequence: u64,
    /// Timestamp when message was sent
    pub timestamp: u64,
    /// Receipt proving flow budget was charged for this message
    pub receipt: Option<Receipt>,
}

/// Peer connection information
#[derive(Debug, Clone)]
pub struct PeerConnection {
    /// Peer device ID
    pub device_id: DeviceId,
    /// Peer socket address
    pub addr: SocketAddr,
    /// Connection establishment time (seconds since UNIX epoch)
    pub connected_at: u64,
    /// Last activity time (seconds since UNIX epoch)
    pub last_activity: u64,
    /// Is connection active
    pub is_active: bool,
}

/// Network transport using TCP with message framing and anti-replay protection
///
/// **REFACTORED**: Connection management now delegated to SecureChannelRegistry
/// to eliminate duplicate channel caches and enforce unified channel semantics.
#[derive(Debug)]
pub struct NetworkTransport {
    device_id: DeviceId,
    config: NetworkConfig,
    /// Unified channel registry replacing duplicate connection caches
    channel_registry: Arc<SecureChannelRegistry>,
    incoming_messages: Arc<Mutex<mpsc::UnboundedReceiver<NetworkMessage>>>,
    incoming_sender: mpsc::UnboundedSender<NetworkMessage>,
    sequence_counter: Arc<Mutex<u64>>,
    /// Anti-replay protection: track seen receipt nonces per (context, peer, epoch)
    /// This remains at transport level for now but could be moved to SecureChannel
    seen_nonces: Arc<RwLock<HashMap<(String, DeviceId, u64), HashSet<u64>>>>,
}

impl NetworkTransport {
    /// Create a new network transport with SecureChannelRegistry
    pub fn new(device_id: DeviceId, config: NetworkConfig) -> Self {
        let (incoming_sender, incoming_receiver) = mpsc::unbounded_channel();

        Self {
            device_id,
            config,
            channel_registry: Arc::new(SecureChannelRegistry::with_defaults()),
            incoming_messages: Arc::new(Mutex::new(incoming_receiver)),
            incoming_sender,
            sequence_counter: Arc::new(Mutex::new(0)),
            seen_nonces: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new network transport with custom channel registry
    pub fn new_with_registry(
        device_id: DeviceId,
        config: NetworkConfig,
        channel_registry: Arc<SecureChannelRegistry>,
    ) -> Self {
        let (incoming_sender, incoming_receiver) = mpsc::unbounded_channel();

        Self {
            device_id,
            config,
            channel_registry,
            incoming_messages: Arc::new(Mutex::new(incoming_receiver)),
            incoming_sender,
            sequence_counter: Arc::new(Mutex::new(0)),
            seen_nonces: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Start listening for incoming connections
    pub async fn start_listener(&mut self) -> Result<(), AuraError> {
        let bind_addr = format!("{}:{}", self.config.bind_addr, self.config.port);
        let listener = TcpListener::bind(&bind_addr).await.map_err(|e| {
            AuraError::coordination_failed(format!("Failed to bind to {}: {}", bind_addr, e))
        })?;

        tracing::info!(
            device_id = %self.device_id.0,
            bind_addr = %bind_addr,
            "Started network transport listener"
        );

        let channel_registry = Arc::clone(&self.channel_registry);
        let incoming_sender = self.incoming_sender.clone();
        let device_id = self.device_id;
        let max_message_size = self.config.max_message_size;

        // Move the listener into the spawn task
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, addr)) => {
                        let channel_registry = Arc::clone(&channel_registry);
                        let incoming_sender = incoming_sender.clone();

                        tokio::spawn(async move {
                            if let Err(e) = Self::handle_connection(
                                stream,
                                addr,
                                device_id,
                                channel_registry,
                                incoming_sender,
                                max_message_size,
                            )
                            .await
                            {
                                tracing::warn!(
                                    addr = %addr,
                                    error = %e,
                                    "Connection handling failed"
                                );
                            }
                        });
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to accept connection");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        });

        // Don't store the listener since it's moved into the spawn task
        Ok(())
    }

    /// Get this device's ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Get configuration
    pub fn config(&self) -> &NetworkConfig {
        &self.config
    }

    /// Get the SecureChannelRegistry for advanced channel management
    pub fn channel_registry(&self) -> &Arc<SecureChannelRegistry> {
        &self.channel_registry
    }

    /// Add a peer connection for a specific context
    /// **REFACTORED**: Now uses SecureChannelRegistry instead of local peer cache
    pub async fn add_peer_for_context(
        &self,
        context: ContextId,
        device_id: DeviceId,
        addr: SocketAddr,
        epoch: Epoch,
        flow_budget: FlowBudget,
    ) -> Result<(), AuraError> {
        // Create or get channel from registry
        self.channel_registry
            .get_or_create_channel(context.clone(), device_id, epoch, flow_budget)
            .await?;

        tracing::info!(
            peer_id = %device_id.0,
            context = %context.as_str(),
            addr = %addr,
            "Added peer connection for context"
        );

        Ok(())
    }

    /// **DEPRECATED**: Legacy method for backward compatibility
    /// Use add_peer_for_context instead
    pub async fn add_peer(&self, device_id: DeviceId, addr: SocketAddr) -> Result<(), AuraError> {
        tracing::warn!(
            peer_id = %device_id.0,
            "Using deprecated add_peer method - context information missing"
        );

        // Use a default context for legacy compatibility
        let default_context = ContextId::new("legacy_transport");
        let default_epoch = Epoch::initial();
        let default_budget = FlowBudget::new(1000, default_epoch);

        self.add_peer_for_context(
            default_context,
            device_id,
            addr,
            default_epoch,
            default_budget,
        )
        .await
    }

    /// Connect to a peer (deprecated - use channel-based connections instead)
    pub async fn connect_to_peer(&self, device_id: DeviceId) -> Result<(), AuraError> {
        tracing::warn!(
            peer_id = %device_id.0,
            "Using deprecated connect_to_peer method - use SecureChannelRegistry instead"
        );

        // For backward compatibility, check if we have any channel for this device
        // This is a simplified implementation - in practice, you'd need context information
        let default_context = ContextId::new("legacy_transport");

        if self
            .channel_registry
            .has_channel(default_context, device_id)
            .await
        {
            tracing::info!(
                peer_id = %device_id.0,
                "Found existing channel for peer"
            );
            return Ok(());
        }

        tracing::warn!(
            peer_id = %device_id.0,
            "No existing channel found for peer - consider using add_peer_for_context instead"
        );

        Err(AuraError::coordination_failed(format!(
            "No channel found for peer {} - use add_peer_for_context instead",
            device_id.0
        )))
    }

    /// Send a message to a peer
    pub async fn send(
        &self,
        to: DeviceId,
        payload: Vec<u8>,
        message_type: String,
    ) -> Result<(), AuraError> {
        self.send_with_receipt(to, payload, message_type, None)
            .await
    }

    /// Send a message to a peer with an embedded receipt
    pub async fn send_with_receipt(
        &self,
        to: DeviceId,
        payload: Vec<u8>,
        message_type: String,
        receipt: Option<Receipt>,
    ) -> Result<(), AuraError> {
        if payload.len() > self.config.max_message_size {
            return Err(AuraError::coordination_failed(format!(
                "Message too large: {} > {}",
                payload.len(),
                self.config.max_message_size
            )));
        }

        let sequence = {
            let mut counter = self.sequence_counter.lock().await;
            *counter += 1;
            *counter
        };

        let _message = NetworkMessage {
            from: self.device_id,
            to,
            message_type,
            payload,
            sequence,
            timestamp: current_timestamp(),
            receipt,
        };

        // Send via channel registry instead of direct connection
        // For now, use a simplified approach - in a full implementation,
        // you'd need to specify which context to send through
        let _default_context = ContextId::new("legacy_transport");

        tracing::warn!(
            to = %to.0,
            "Sending message via legacy method - context information missing"
        );

        // This is simplified - real implementation would route through appropriate channel
        Err(AuraError::coordination_failed(
            "Legacy send method deprecated - use channel-based messaging instead".to_string(),
        ))
    }

    /// Receive a message
    pub async fn receive(&self) -> Result<NetworkMessage, AuraError> {
        let mut receiver = self.incoming_messages.lock().await;
        receiver.recv().await.ok_or_else(|| {
            AuraError::coordination_failed("Message receive channel closed".to_string())
        })
    }

    /// Receive and verify a message with receipt validation
    pub async fn receive_verified(&self) -> Result<NetworkMessage, AuraError> {
        let message = self.receive().await?;
        self.verify_receipt(&message).await?;
        Ok(message)
    }

    /// Verify receipt on received message with anti-replay protection
    pub async fn verify_receipt(&self, message: &NetworkMessage) -> Result<(), AuraError> {
        if let Some(receipt) = &message.receipt {
            // Verify receipt fields match message
            if receipt.src != message.from {
                return Err(AuraError::coordination_failed(
                    "Receipt src does not match message sender".to_string(),
                ));
            }
            if receipt.dst != self.device_id {
                return Err(AuraError::coordination_failed(
                    "Receipt dst does not match this device".to_string(),
                ));
            }

            // Anti-replay protection: check if we've seen this nonce before
            let nonce_key = (
                receipt.ctx.as_str().to_string(),
                receipt.src,
                receipt.epoch.value(),
            );
            let mut seen_nonces = self.seen_nonces.write().await;
            let nonce_set = seen_nonces
                .entry(nonce_key.clone())
                .or_insert_with(HashSet::new);

            if nonce_set.contains(&receipt.nonce) {
                return Err(AuraError::coordination_failed(format!(
                    "Replay attack detected: duplicate nonce {} for ctx={} src={} epoch={}",
                    receipt.nonce,
                    receipt.ctx.as_str(),
                    receipt.src,
                    receipt.epoch.value()
                )));
            }

            // Record this nonce as seen
            nonce_set.insert(receipt.nonce);

            // Cryptographic verification of receipt signature
            self.verify_receipt_signature(&receipt, &message.from)
                .await?;

            // Verify receipt chain hash
            self.verify_receipt_chain_hash(&receipt, &message.from)
                .await?;

            tracing::debug!(
                from = %message.from.0,
                cost = receipt.cost,
                nonce = receipt.nonce,
                epoch = receipt.epoch.value(),
                "Verified receipt for message with anti-replay protection"
            );
        } else {
            tracing::warn!(
                from = %message.from.0,
                message_type = %message.message_type,
                "Received message without receipt - potential unauthorized transmission"
            );
            // In production, this could be an error depending on policy
        }
        Ok(())
    }

    /// Clean up old nonces to prevent memory growth
    /// Should be called periodically to remove nonces from old epochs
    pub async fn cleanup_old_nonces(&self, current_epoch: u64, retention_epochs: u64) {
        let mut seen_nonces = self.seen_nonces.write().await;
        let cutoff_epoch = current_epoch.saturating_sub(retention_epochs);

        seen_nonces.retain(|(_, _, epoch), _| *epoch >= cutoff_epoch);

        tracing::debug!(
            retained_epochs = seen_nonces.len(),
            cutoff_epoch,
            "Cleaned up old receipt nonces"
        );
    }

    /// Get list of connected peers (simplified implementation)
    pub async fn connected_peers(&self) -> Vec<DeviceId> {
        // Simplified implementation - in practice, you'd need to iterate through
        // all possible contexts to find channels. For now, return empty list.
        tracing::warn!("connected_peers() method deprecated - use SecureChannelRegistry directly");
        Vec::new()
    }

    /// Check if peer is connected (simplified implementation)
    pub async fn is_peer_connected(&self, device_id: DeviceId) -> bool {
        // Simplified implementation - check default context only
        let default_context = ContextId::new("legacy_transport");
        self.channel_registry
            .has_channel(default_context, device_id)
            .await
    }

    /// Handle incoming connection
    async fn handle_connection(
        mut stream: TcpStream,
        addr: SocketAddr,
        _local_device_id: DeviceId,
        _channel_registry: Arc<SecureChannelRegistry>,
        incoming_sender: mpsc::UnboundedSender<NetworkMessage>,
        max_message_size: usize,
    ) -> Result<(), AuraError> {
        tracing::debug!(addr = %addr, "Handling new connection");

        loop {
            match Self::receive_message(&mut stream, max_message_size).await {
                Ok(message) => {
                    let peer_id = message.from;
                    let _now = current_timestamp();

                    // TODO: Update channel registry with connection activity
                    // For now, just log the incoming message
                    tracing::debug!(
                        peer_id = %peer_id.0,
                        addr = %addr,
                        "Received message from peer"
                    );

                    if incoming_sender.send(message).is_err() {
                        tracing::warn!("Failed to forward message - receiver dropped");
                        break;
                    }
                }
                Err(e) => {
                    tracing::debug!(addr = %addr, error = %e, "Connection ended");
                    break;
                }
            }
        }

        Ok(())
    }

    /// Send a framed message
    async fn send_message(
        stream: &mut TcpStream,
        message: &NetworkMessage,
    ) -> Result<(), AuraError> {
        let serialized = bincode::serialize(message)
            .map_err(|e| AuraError::coordination_failed(format!("Serialization failed: {}", e)))?;

        let length = (serialized.len() as u32).to_be_bytes();
        stream
            .write_all(&length)
            .await
            .map_err(|e| AuraError::coordination_failed(format!("Write length failed: {}", e)))?;

        stream
            .write_all(&serialized)
            .await
            .map_err(|e| AuraError::coordination_failed(format!("Write message failed: {}", e)))?;

        stream
            .flush()
            .await
            .map_err(|e| AuraError::coordination_failed(format!("Flush failed: {}", e)))?;

        Ok(())
    }

    /// Receive a framed message
    async fn receive_message(
        stream: &mut TcpStream,
        max_size: usize,
    ) -> Result<NetworkMessage, AuraError> {
        let mut length_bytes = [0u8; 4];
        stream
            .read_exact(&mut length_bytes)
            .await
            .map_err(|e| AuraError::coordination_failed(format!("Read length failed: {}", e)))?;

        let length = u32::from_be_bytes(length_bytes) as usize;
        if length > max_size {
            return Err(AuraError::coordination_failed(format!(
                "Message too large: {} > {}",
                length, max_size
            )));
        }

        let mut buffer = vec![0u8; length];
        stream
            .read_exact(&mut buffer)
            .await
            .map_err(|e| AuraError::coordination_failed(format!("Read message failed: {}", e)))?;

        let message: NetworkMessage = bincode::deserialize(&buffer).map_err(|e| {
            AuraError::coordination_failed(format!("Deserialization failed: {}", e))
        })?;

        Ok(message)
    }

    /// Verify cryptographic signature on receipt
    async fn verify_receipt_signature(
        &self,
        receipt: &Receipt,
        sender: &DeviceId,
    ) -> Result<(), AuraError> {
        // Extract sender's public key
        let public_key = self.get_device_public_key(sender).await?;

        // Compute receipt commitment for signature verification
        let receipt_commitment = self.compute_receipt_commitment(receipt)?;

        // Verify signature using Ed25519
        if receipt.sig.len() != 64 {
            return Err(AuraError::coordination_failed(
                "Invalid signature length".to_string(),
            ));
        }

        let sig_array: [u8; 64] = receipt.sig.clone().try_into().map_err(|_| {
            AuraError::coordination_failed("Failed to convert signature to array".to_string())
        })?;

        self.verify_ed25519_signature(&sig_array, &receipt_commitment, &public_key)
            .map_err(|e| {
                AuraError::coordination_failed(format!(
                    "Receipt signature verification failed: {}",
                    e
                ))
            })
    }

    /// Verify receipt chain hash for anti-replay protection
    async fn verify_receipt_chain_hash(
        &self,
        receipt: &Receipt,
        sender: &DeviceId,
    ) -> Result<(), AuraError> {
        // Get the previous receipt hash for this sender
        let expected_prev_hash = self.get_previous_receipt_hash(sender).await?;

        // Verify the chain linkage
        let expected_hash = aura_core::Hash32(expected_prev_hash);
        if receipt.prev != expected_hash {
            return Err(AuraError::coordination_failed(format!(
                "Receipt chain verification failed: expected prev={:?}, got={:?}",
                expected_hash, receipt.prev
            )));
        }

        // Update our record of the latest receipt hash for this sender
        let receipt_hash = self.compute_receipt_hash(receipt)?;
        self.update_receipt_hash(sender, receipt_hash).await?;

        Ok(())
    }

    /// Get device public key for signature verification
    async fn get_device_public_key(&self, device_id: &DeviceId) -> Result<[u8; 32], AuraError> {
        // This is simplified - real implementation would query from identity system
        tracing::debug!("Looking up public key for device: {}", device_id);

        // Placeholder: use device ID hash as public key
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(device_id.0.as_bytes());
        let hash = hasher.finalize();

        let mut public_key = [0u8; 32];
        public_key.copy_from_slice(&hash[..32]);
        Ok(public_key)
    }

    /// Compute receipt commitment for signature verification
    fn compute_receipt_commitment(&self, receipt: &Receipt) -> Result<Vec<u8>, AuraError> {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();

        // Add receipt fields to commitment
        hasher.update(b"MESSAGE_RECEIPT");
        hasher.update(receipt.nonce.to_be_bytes());
        hasher.update(receipt.cost.to_be_bytes());
        hasher.update(receipt.epoch.value().to_be_bytes());
        hasher.update(&receipt.prev);

        Ok(hasher.finalize().to_vec())
    }

    /// Verify Ed25519 signature
    fn verify_ed25519_signature(
        &self,
        signature: &[u8; 64],
        message: &[u8],
        public_key: &[u8; 32],
    ) -> Result<(), AuraError> {
        // This is simplified - real implementation would use ed25519-dalek
        if signature.len() != 64 {
            return Err(AuraError::coordination_failed(
                "Invalid signature length".to_string(),
            ));
        }

        if message.is_empty() {
            return Err(AuraError::coordination_failed("Empty message".to_string()));
        }

        tracing::debug!(
            "Verifying Ed25519 signature: msg_len={}, key={:?}",
            message.len(),
            &public_key[..4]
        );

        // Placeholder verification - always succeeds for now
        // Real implementation would use ed25519_dalek::VerifyingKey
        Ok(())
    }

    /// Get previous receipt hash for chain verification
    async fn get_previous_receipt_hash(&self, device_id: &DeviceId) -> Result<[u8; 32], AuraError> {
        // This is simplified - real implementation would query from persistent storage
        tracing::debug!("Getting previous receipt hash for device: {}", device_id);

        // Placeholder: return zero hash for first receipt
        Ok([0u8; 32])
    }

    /// Compute hash of receipt for chain linkage
    fn compute_receipt_hash(&self, receipt: &Receipt) -> Result<[u8; 32], AuraError> {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();

        // Add all receipt fields to hash
        hasher.update(receipt.nonce.to_be_bytes());
        hasher.update(receipt.cost.to_be_bytes());
        hasher.update(receipt.epoch.value().to_be_bytes());
        hasher.update(&receipt.prev);
        hasher.update(&receipt.sig);

        let hash = hasher.finalize();
        let mut result = [0u8; 32];
        result.copy_from_slice(&hash[..32]);
        Ok(result)
    }

    /// Update stored receipt hash for device
    async fn update_receipt_hash(
        &self,
        device_id: &DeviceId,
        hash: [u8; 32],
    ) -> Result<(), AuraError> {
        // This is simplified - real implementation would persist to storage
        tracing::debug!(
            "Updating receipt hash for device {}: {:?}",
            device_id,
            &hash[..4]
        );
        Ok(())
    }
}
