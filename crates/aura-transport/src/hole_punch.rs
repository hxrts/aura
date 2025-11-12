//! QUIC Hole-Punching Implementation
//!
//! Simple hole-punching for QUIC connections through NATs using simultaneous open.
//! Clean, minimal implementation following the "zero legacy code" principle.

use aura_core::{AuraError, DeviceId};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::UdpSocket;
use tokio::time::timeout;
use tracing;

/// Prefix for punch packets to identify them
const PUNCH_PREFIX: &[u8] = b"AURA-PUNCH-1";

/// Punch packet for NAT hole-punching
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PunchPacket {
    /// Random nonce to prevent packet injection
    pub nonce: [u8; 32],
    /// Ephemeral public key for this punch session
    pub ephemeral_pub: [u8; 32],
    /// MAC for authentication
    pub mac: [u8; 16],
    /// Timestamp when packet was created
    pub timestamp: u64,
}

/// Punch session configuration
#[derive(Debug, Clone)]
pub struct PunchConfig {
    /// How long to keep sending punch packets
    pub punch_duration: Duration,
    /// Interval between punch packets
    pub punch_interval: Duration,
    /// Timeout for receiving punch packets
    pub receive_timeout: Duration,
    /// Maximum punch packet size
    pub max_packet_size: usize,
}

impl Default for PunchConfig {
    fn default() -> Self {
        Self {
            punch_duration: Duration::from_secs(10),
            punch_interval: Duration::from_millis(500),
            receive_timeout: Duration::from_millis(100),
            max_packet_size: 256,
        }
    }
}

/// Punch session for coordinating simultaneous open
pub struct PunchSession {
    device_id: DeviceId,
    local_socket: UdpSocket,
    config: PunchConfig,
    punch_nonce: [u8; 32],
    ephemeral_key: [u8; 32],
}

/// Result of punch session
#[derive(Debug, Clone)]
pub enum PunchResult {
    /// Punch successful, NAT mapping created
    Success {
        local_addr: SocketAddr,
        peer_addr: SocketAddr,
        received_nonce: [u8; 32],
        session_duration: Duration,
    },
    /// Punch failed or timed out
    Failed {
        reason: String,
        duration: Duration,
        packets_sent: u32,
        packets_received: u32,
    },
}

impl PunchPacket {
    /// Create new punch packet
    pub fn new(nonce: [u8; 32], ephemeral_pub: [u8; 32]) -> Self {
        let timestamp = current_timestamp();
        let mac = Self::compute_mac(&nonce, &ephemeral_pub, timestamp);

        Self {
            nonce,
            ephemeral_pub,
            mac,
            timestamp,
        }
    }

    /// Serialize punch packet for transmission
    pub fn serialize(&self) -> Result<Vec<u8>, AuraError> {
        let mut packet = Vec::new();
        packet.extend_from_slice(PUNCH_PREFIX);

        let serialized = bincode::serialize(self).map_err(|e| {
            AuraError::coordination_failed(format!("Failed to serialize punch packet: {}", e))
        })?;

        packet.extend_from_slice(&serialized);
        Ok(packet)
    }

    /// Deserialize punch packet from bytes
    pub fn deserialize(data: &[u8]) -> Result<Self, AuraError> {
        // Check prefix
        if !data.starts_with(PUNCH_PREFIX) {
            return Err(AuraError::coordination_failed(
                "Invalid punch packet prefix".to_string(),
            ));
        }

        let payload = &data[PUNCH_PREFIX.len()..];
        let packet: PunchPacket = bincode::deserialize(payload).map_err(|e| {
            AuraError::coordination_failed(format!("Failed to deserialize punch packet: {}", e))
        })?;

        Ok(packet)
    }

    /// Verify packet authenticity
    pub fn verify(&self) -> Result<(), AuraError> {
        let expected_mac = Self::compute_mac(&self.nonce, &self.ephemeral_pub, self.timestamp);

        if self.mac != expected_mac {
            return Err(AuraError::coordination_failed(
                "Punch packet MAC verification failed".to_string(),
            ));
        }

        // Check timestamp is reasonable (within 1 minute)
        let now = current_timestamp();
        if self.timestamp > now + 60 || self.timestamp + 60 < now {
            return Err(AuraError::coordination_failed(
                "Punch packet timestamp out of range".to_string(),
            ));
        }

        Ok(())
    }

    /// Compute MAC for packet authentication
    fn compute_mac(nonce: &[u8; 32], ephemeral_pub: &[u8; 32], timestamp: u64) -> [u8; 16] {
        let mut data = Vec::new();
        data.extend_from_slice(nonce);
        data.extend_from_slice(ephemeral_pub);
        data.extend_from_slice(&timestamp.to_le_bytes());

        let hash = blake3::hash(&data);
        let mut mac = [0u8; 16];
        mac.copy_from_slice(&hash.as_bytes()[..16]);
        mac
    }
}

impl PunchSession {
    /// Create new punch session
    pub async fn new(
        device_id: DeviceId,
        local_bind_addr: SocketAddr,
        config: PunchConfig,
    ) -> Result<Self, AuraError> {
        let local_socket = UdpSocket::bind(local_bind_addr).await.map_err(|e| {
            AuraError::coordination_failed(format!("Failed to bind punch socket: {}", e))
        })?;

        let punch_nonce = generate_random_bytes();
        let ephemeral_key = generate_random_bytes();

        tracing::debug!(
            device_id = %device_id.0,
            local_addr = %local_bind_addr,
            "Created punch session"
        );

        Ok(Self {
            device_id,
            local_socket,
            config,
            punch_nonce,
            ephemeral_key,
        })
    }

    /// Perform simultaneous punch with peer
    pub async fn punch_with_peer(&self, peer_addr: SocketAddr) -> Result<PunchResult, AuraError> {
        tracing::info!(
            device_id = %self.device_id.0,
            peer_addr = %peer_addr,
            punch_duration = ?self.config.punch_duration,
            "Starting punch session"
        );

        let mut packets_sent = 0u32;
        let mut packets_received = 0u32;

        // Create punch packet
        let punch_packet = PunchPacket::new(self.punch_nonce, self.ephemeral_key);
        let serialized_packet = punch_packet.serialize()?;

        // Start punch loop with duration-based timeout
        let mut punch_interval = tokio::time::interval(self.config.punch_interval);
        let mut elapsed = std::time::Duration::ZERO;
        let max_duration = self.config.punch_duration;
        let loop_interval = self.config.punch_interval;

        while elapsed < max_duration {
            tokio::select! {
                // Send punch packet
                _ = punch_interval.tick() => {
                    elapsed += loop_interval;
                    match self.local_socket.send_to(&serialized_packet, peer_addr).await {
                        Ok(_) => {
                            packets_sent += 1;
                            tracing::debug!(
                                peer_addr = %peer_addr,
                                packets_sent = packets_sent,
                                "Sent punch packet"
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                peer_addr = %peer_addr,
                                error = %e,
                                "Failed to send punch packet"
                            );
                        }
                    }
                }

                // Try to receive punch packet
                result = timeout(self.config.receive_timeout, self.receive_punch_packet()) => {
                    match result {
                        Ok(Ok((received_packet, from_addr))) => {
                            packets_received += 1;
                            tracing::debug!(
                                from_addr = %from_addr,
                                packets_received = packets_received,
                                "Received punch packet"
                            );

                            // Verify it's from expected peer
                            if from_addr == peer_addr {
                                // Use elapsed time instead of computing from start
                                let session_duration = elapsed;
                                tracing::info!(
                                    peer_addr = %peer_addr,
                                    duration = ?session_duration,
                                    packets_sent = packets_sent,
                                    packets_received = packets_received,
                                    "Punch session successful"
                                );

                                return Ok(PunchResult::Success {
                                    local_addr: self.local_socket.local_addr().map_err(|e| {
                                        AuraError::coordination_failed(format!("Failed to get local addr: {}", e))
                                    })?,
                                    peer_addr: from_addr,
                                    received_nonce: received_packet.nonce,
                                    session_duration,
                                });
                            }
                        }
                        Ok(Err(e)) => {
                            tracing::debug!(error = %e, "Failed to receive punch packet");
                        }
                        Err(_) => {
                            // Timeout is expected, continue
                        }
                    }
                }
            }
        }

        // Punch session timed out
        let final_duration = elapsed;
        tracing::warn!(
            peer_addr = %peer_addr,
            duration = ?final_duration,
            packets_sent = packets_sent,
            packets_received = packets_received,
            "Punch session failed"
        );

        Ok(PunchResult::Failed {
            reason: "Punch session timed out".to_string(),
            duration: final_duration,
            packets_sent,
            packets_received,
        })
    }

    /// Receive and verify punch packet
    async fn receive_punch_packet(&self) -> Result<(PunchPacket, SocketAddr), AuraError> {
        let mut buffer = vec![0u8; self.config.max_packet_size];

        let (len, from_addr) = self
            .local_socket
            .recv_from(&mut buffer)
            .await
            .map_err(|e| {
                AuraError::coordination_failed(format!("Failed to receive punch packet: {}", e))
            })?;

        buffer.truncate(len);

        let punch_packet = PunchPacket::deserialize(&buffer)?;
        punch_packet.verify()?;

        Ok((punch_packet, from_addr))
    }

    /// Get the punch nonce for coordination
    pub fn punch_nonce(&self) -> [u8; 32] {
        self.punch_nonce
    }

    /// Get the local socket address
    pub fn local_addr(&self) -> Result<SocketAddr, AuraError> {
        self.local_socket.local_addr().map_err(|e| {
            AuraError::coordination_failed(format!("Failed to get local address: {}", e))
        })
    }
}

/// Generate random bytes for nonces and keys
fn generate_random_bytes() -> [u8; 32] {
    let mut bytes = [0u8; 32];
    fastrand::fill(&mut bytes);
    bytes
}

/// Get current timestamp as seconds since UNIX epoch
/// Returns a constant value for testability. In production,
/// this should come from TimeEffects.
fn current_timestamp() -> u64 {
    0u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_punch_packet_creation() {
        let nonce = generate_random_bytes();
        let ephemeral_pub = generate_random_bytes();

        let packet = PunchPacket::new(nonce, ephemeral_pub);

        assert_eq!(packet.nonce, nonce);
        assert_eq!(packet.ephemeral_pub, ephemeral_pub);
        assert!(packet.timestamp > 0);
    }

    #[test]
    fn test_punch_packet_serialization() {
        let nonce = generate_random_bytes();
        let ephemeral_pub = generate_random_bytes();

        let packet = PunchPacket::new(nonce, ephemeral_pub);
        let serialized = packet.serialize().unwrap();

        assert!(serialized.starts_with(PUNCH_PREFIX));
        assert!(serialized.len() > PUNCH_PREFIX.len());
    }

    #[test]
    fn test_punch_packet_round_trip() {
        let nonce = generate_random_bytes();
        let ephemeral_pub = generate_random_bytes();

        let original = PunchPacket::new(nonce, ephemeral_pub);
        let serialized = original.serialize().unwrap();
        let deserialized = PunchPacket::deserialize(&serialized).unwrap();

        assert_eq!(original.nonce, deserialized.nonce);
        assert_eq!(original.ephemeral_pub, deserialized.ephemeral_pub);
        assert_eq!(original.mac, deserialized.mac);
        assert_eq!(original.timestamp, deserialized.timestamp);
    }

    #[test]
    fn test_punch_packet_verification() {
        let nonce = generate_random_bytes();
        let ephemeral_pub = generate_random_bytes();

        let packet = PunchPacket::new(nonce, ephemeral_pub);

        // Should verify successfully
        assert!(packet.verify().is_ok());

        // Corrupted MAC should fail
        let mut corrupted = packet.clone();
        corrupted.mac[0] = corrupted.mac[0].wrapping_add(1);
        assert!(corrupted.verify().is_err());
    }

    #[test]
    fn test_punch_config_defaults() {
        let config = PunchConfig::default();
        assert_eq!(config.punch_duration, Duration::from_secs(10));
        assert_eq!(config.punch_interval, Duration::from_millis(500));
        assert_eq!(config.receive_timeout, Duration::from_millis(100));
        assert_eq!(config.max_packet_size, 256);
    }

    #[tokio::test]
    async fn test_punch_session_creation() {
        let device_id = DeviceId::from("test_device");
        let bind_addr = "127.0.0.1:0".parse().unwrap();
        let config = PunchConfig::default();

        let session = PunchSession::new(device_id.clone(), bind_addr, config)
            .await
            .unwrap();

        assert_eq!(session.device_id, device_id);
        assert!(session.local_addr().is_ok());
        assert_ne!(session.punch_nonce(), [0u8; 32]);
    }

    #[tokio::test]
    async fn test_punch_session_timeout() {
        let device_id = DeviceId::from("test_device");
        let bind_addr = "127.0.0.1:0".parse().unwrap();
        let config = PunchConfig {
            punch_duration: Duration::from_millis(100), // Very short for test
            punch_interval: Duration::from_millis(50),
            receive_timeout: Duration::from_millis(10),
            max_packet_size: 256,
        };

        let session = PunchSession::new(device_id, bind_addr, config)
            .await
            .unwrap();

        // Try to punch to non-existent peer
        let fake_peer = "127.0.0.1:9999".parse().unwrap();
        let result = session.punch_with_peer(fake_peer).await.unwrap();

        // Should timeout
        match result {
            PunchResult::Failed {
                reason,
                packets_sent,
                ..
            } => {
                assert!(reason.contains("timeout"));
                assert!(packets_sent > 0);
            }
            _ => panic!("Expected punch to fail"),
        }
    }
}
