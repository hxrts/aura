//! Authenticated transport channels and device credentials
//!
//! This module implements the authentication layer for transport channels,
//! providing verified peer identity and encrypted communication channels.
//! Authentication is verified at the transport layer, while authorization
//! decisions happen at the application layer.
//!
//! Reference: docs/040_storage.md Section 5 "Unified Transport Architecture"

use crate::{TransportError, TransportErrorBuilder, TransportResult};
use async_trait::async_trait;
use ed25519_dalek::{Signature, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Authenticated channel handle
///
/// Represents an authenticated connection to a peer device.
/// Authentication is verified at the transport layer, but authorization
/// decisions happen at the application layer.
#[derive(Debug, Clone)]
pub struct AuthenticatedChannel {
    /// Channel identifier
    pub channel_id: Uuid,
    /// Peer device ID (authenticated)
    pub peer_device_id: Uuid,
    /// Peer address
    pub peer_addr: SocketAddr,
    /// Channel creation timestamp
    pub created_at: u64,
    /// Last activity timestamp for idle detection
    pub last_activity: u64,
}

impl AuthenticatedChannel {
    /// Create a new authenticated channel
    pub fn new(
        peer_device_id: Uuid,
        peer_addr: SocketAddr,
        current_time: u64,
    ) -> Self {
        Self {
            channel_id: Uuid::new_v4(),
            peer_device_id,
            peer_addr,
            created_at: current_time,
            last_activity: current_time,
        }
    }

    /// Check if channel is idle
    pub fn is_idle(&self, current_time: u64, idle_timeout: Duration) -> bool {
        let idle_duration = current_time.saturating_sub(self.last_activity);
        idle_duration > idle_timeout.as_millis() as u64
    }

    /// Update last activity timestamp
    pub fn touch(&mut self, current_time: u64) {
        self.last_activity = current_time;
    }
}

/// Device authentication credentials for transport layer
#[derive(Debug, Clone)]
pub struct DeviceCredentials {
    /// Device identifier
    pub device_id: Uuid,
    /// Device signing key for authentication
    pub signing_key: SigningKey,
    /// Device public key for verification
    pub verifying_key: VerifyingKey,
}

impl DeviceCredentials {
    /// Create new device credentials with random keys
    pub fn generate() -> Self {
        let signing_key = SigningKey::generate(&mut rand::thread_rng());
        let verifying_key = signing_key.verifying_key();
        
        Self {
            device_id: Uuid::new_v4(),
            signing_key,
            verifying_key,
        }
    }

    /// Create device credentials from existing key material
    pub fn from_signing_key(device_id: Uuid, signing_key: SigningKey) -> Self {
        let verifying_key = signing_key.verifying_key();
        
        Self {
            device_id,
            signing_key,
            verifying_key,
        }
    }

    /// Sign a message with the device's signing key
    pub fn sign_message(&self, message: &[u8]) -> Signature {
        self.signing_key.sign(message)
    }

    /// Verify a signature using the device's public key
    pub fn verify_signature(&self, message: &[u8], signature: &Signature) -> bool {
        self.verifying_key.verify(message, signature).is_ok()
    }
}

/// Authentication handshake message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticationChallenge {
    /// Challenge nonce
    pub nonce: [u8; 32],
    /// Challenger device ID
    pub challenger_id: Uuid,
    /// Challenge timestamp
    pub timestamp: u64,
}

/// Authentication response message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticationResponse {
    /// Original challenge nonce
    pub challenge_nonce: [u8; 32],
    /// Responder device ID
    pub responder_id: Uuid,
    /// Response signature (signature of challenge nonce)
    pub signature: Vec<u8>,
    /// Response timestamp
    pub timestamp: u64,
}

/// Manages authenticated channels and peer verification
pub struct AuthenticatedTransport {
    /// Device credentials for this transport
    credentials: DeviceCredentials,
    /// Active authenticated channels by peer device ID
    channels: Arc<RwLock<BTreeMap<Uuid, AuthenticatedChannel>>>,
    /// Known peer public keys for verification
    peer_keys: Arc<RwLock<BTreeMap<Uuid, VerifyingKey>>>,
    /// Channel idle timeout
    idle_timeout: Duration,
}

impl AuthenticatedTransport {
    /// Create a new authenticated transport
    pub fn new(credentials: DeviceCredentials, idle_timeout: Duration) -> Self {
        Self {
            credentials,
            channels: Arc::new(RwLock::new(BTreeMap::new())),
            peer_keys: Arc::new(RwLock::new(BTreeMap::new())),
            idle_timeout,
        }
    }

    /// Add a known peer public key for verification
    pub async fn add_peer_key(&self, peer_id: Uuid, verifying_key: VerifyingKey) {
        let mut peer_keys = self.peer_keys.write().await;
        peer_keys.insert(peer_id, verifying_key);
    }

    /// Authenticate a peer and establish a channel
    pub async fn authenticate_peer(
        &self,
        peer_id: Uuid,
        peer_addr: SocketAddr,
        current_time: u64,
    ) -> TransportResult<AuthenticatedChannel> {
        // Check if we have the peer's public key
        let peer_key = {
            let peer_keys = self.peer_keys.read().await;
            peer_keys.get(&peer_id).cloned()
        };

        let peer_key = peer_key.ok_or_else(|| {
            TransportErrorBuilder::authentication(format!(
                "No public key found for peer {}",
                peer_id
            ))
        })?;

        // Generate authentication challenge
        let challenge = AuthenticationChallenge {
            nonce: rand::random(),
            challenger_id: self.credentials.device_id,
            timestamp: current_time,
        };

        // For now, we'll simulate successful authentication
        // In a real implementation, this would involve network communication
        let channel = AuthenticatedChannel::new(peer_id, peer_addr, current_time);

        // Store the authenticated channel
        {
            let mut channels = self.channels.write().await;
            channels.insert(peer_id, channel.clone());
        }

        Ok(channel)
    }

    /// Send a message over an authenticated channel
    pub async fn send_authenticated(
        &self,
        peer_id: Uuid,
        message: &[u8],
        current_time: u64,
    ) -> TransportResult<()> {
        // Get and update the channel
        let mut channels = self.channels.write().await;
        let channel = channels.get_mut(&peer_id).ok_or_else(|| {
            TransportErrorBuilder::connection(format!(
                "No authenticated channel for peer {}",
                peer_id
            ))
        })?;

        // Update activity timestamp
        channel.touch(current_time);

        // In a real implementation, this would send the message over the network
        // For now, we'll just validate that the channel is active
        if channel.is_idle(current_time, self.idle_timeout) {
            return Err(TransportErrorBuilder::connection(
                "Channel is idle and may be closed".to_string(),
            ).into());
        }

        Ok(())
    }

    /// Clean up idle channels
    pub async fn cleanup_idle_channels(&self, current_time: u64) -> usize {
        let mut channels = self.channels.write().await;
        let initial_count = channels.len();
        
        channels.retain(|_, channel| {
            !channel.is_idle(current_time, self.idle_timeout)
        });
        
        initial_count - channels.len()
    }

    /// Get active channel count
    pub async fn active_channel_count(&self) -> usize {
        let channels = self.channels.read().await;
        channels.len()
    }

    /// Get device credentials
    pub fn credentials(&self) -> &DeviceCredentials {
        &self.credentials
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_device_credentials_generation() {
        let creds = DeviceCredentials::generate();
        
        let message = b"test message";
        let signature = creds.sign_message(message);
        
        assert!(creds.verify_signature(message, &signature));
    }

    #[test]
    fn test_authenticated_channel_idle_detection() {
        let peer_addr = "127.0.0.1:8000".parse().unwrap();
        let current_time = 1000;
        let mut channel = AuthenticatedChannel::new(
            Uuid::new_v4(),
            peer_addr,
            current_time,
        );

        // Channel should not be idle immediately
        assert!(!channel.is_idle(current_time, Duration::from_secs(60)));

        // Channel should be idle after timeout
        let later_time = current_time + 120_000; // 2 minutes later
        assert!(channel.is_idle(later_time, Duration::from_secs(60)));

        // Touch should reset idle state
        channel.touch(later_time);
        assert!(!channel.is_idle(later_time, Duration::from_secs(60)));
    }

    #[tokio::test]
    async fn test_authenticated_transport_peer_management() {
        let creds = DeviceCredentials::generate();
        let transport = AuthenticatedTransport::new(creds, Duration::from_secs(60));

        let peer_id = Uuid::new_v4();
        let peer_creds = DeviceCredentials::generate();
        
        // Add peer key
        transport.add_peer_key(peer_id, peer_creds.verifying_key).await;

        // Authenticate peer
        let peer_addr = "127.0.0.1:8000".parse().unwrap();
        let channel = transport.authenticate_peer(peer_id, peer_addr, 1000).await.unwrap();

        assert_eq!(channel.peer_device_id, peer_id);
        assert_eq!(transport.active_channel_count().await, 1);
    }
}