//! Connection Priority Management
//!
//! Implements connection establishment priority logic for NAT traversal:
//! 1. Direct QUIC (local network or public IP)
//! 2. QUIC via STUN reflexive addresses
//! 3. WebSocket relay fallback
//!
//! Clean implementation following "zero legacy code" principle.

use aura_core::{AuraError, DeviceId};
use aura_protocol::messages::social::rendezvous::{
    TransportDescriptor, TransportKind, TransportOfferPayload,
};
use aura_transport::{PunchConfig, PunchResult, PunchSession, StunClient, StunConfig, StunResult};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::time::timeout;
use tracing;

/// Connection priority manager for NAT traversal
pub struct ConnectionManager {
    device_id: DeviceId,
    stun_client: StunClient,
    connection_timeout: Duration,
}

/// Connection establishment result
#[derive(Debug, Clone)]
pub enum ConnectionResult {
    /// Successfully established direct connection
    DirectConnection {
        transport: TransportDescriptor,
        address: SocketAddr,
        method: ConnectionMethod,
    },
    /// Connection failed after trying all methods
    Failed {
        attempts: Vec<ConnectionAttempt>,
        final_error: String,
    },
}

/// Connection method used
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionMethod {
    /// Direct connection without NAT traversal
    Direct,
    /// Connection via STUN reflexive address
    StunReflexive,
    /// Connection via coordinated hole-punching
    HolePunch,
    /// Connection via WebSocket relay
    WebSocketRelay,
}

/// Individual connection attempt result
#[derive(Debug, Clone)]
pub struct ConnectionAttempt {
    pub method: ConnectionMethod,
    pub transport: TransportDescriptor,
    pub address: String,
    pub duration: Duration,
    pub error: Option<String>,
}

/// Configuration for connection establishment
#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    /// Timeout per connection attempt
    pub attempt_timeout: Duration,
    /// Overall connection timeout
    pub total_timeout: Duration,
    /// Whether to enable STUN discovery
    pub enable_stun: bool,
    /// Whether to enable hole-punching
    pub enable_hole_punch: bool,
    /// Whether to enable relay fallback
    pub enable_relay_fallback: bool,
    /// Configuration for punch sessions
    pub punch_config: PunchConfig,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            attempt_timeout: Duration::from_secs(2),
            total_timeout: Duration::from_secs(10),
            enable_stun: true,
            enable_hole_punch: true,
            enable_relay_fallback: true,
            punch_config: PunchConfig::default(),
        }
    }
}

impl ConnectionManager {
    /// Create new connection manager
    pub fn new(device_id: DeviceId, stun_config: StunConfig) -> Self {
        let stun_client = StunClient::new(stun_config);

        Self {
            device_id,
            stun_client,
            connection_timeout: Duration::from_secs(2),
        }
    }

    /// Establish connection using priority logic: direct → reflexive → relay
    pub async fn establish_connection(
        &self,
        peer_id: DeviceId,
        offers: Vec<TransportDescriptor>,
        config: ConnectionConfig,
    ) -> Result<ConnectionResult, AuraError> {
        tracing::info!(
            peer_id = %peer_id.0,
            num_offers = offers.len(),
            "Starting connection establishment with priority logic"
        );

        let start_time = std::time::Instant::now();
        let mut attempts = Vec::new();

        // Phase 1: Try direct connections first
        for offer in &offers {
            if start_time.elapsed() > config.total_timeout {
                break;
            }

            for address in &offer.local_addresses {
                let attempt_start = std::time::Instant::now();

                match self
                    .try_direct_connection(offer.clone(), address, &config)
                    .await
                {
                    Ok(addr) => {
                        tracing::info!(
                            peer_id = %peer_id.0,
                            address = %address,
                            duration = ?attempt_start.elapsed(),
                            "Direct connection successful"
                        );

                        return Ok(ConnectionResult::DirectConnection {
                            transport: offer.clone(),
                            address: addr,
                            method: ConnectionMethod::Direct,
                        });
                    }
                    Err(e) => {
                        attempts.push(ConnectionAttempt {
                            method: ConnectionMethod::Direct,
                            transport: offer.clone(),
                            address: address.clone(),
                            duration: attempt_start.elapsed(),
                            error: Some(e.to_string()),
                        });

                        tracing::debug!(
                            address = %address,
                            error = %e,
                            duration = ?attempt_start.elapsed(),
                            "Direct connection failed"
                        );
                    }
                }
            }
        }

        // Phase 2: Try STUN reflexive addresses if enabled
        if config.enable_stun {
            for offer in &offers {
                if start_time.elapsed() > config.total_timeout {
                    break;
                }

                // Try existing reflexive addresses first
                for address in &offer.reflexive_addresses {
                    let attempt_start = std::time::Instant::now();

                    match self
                        .try_reflexive_connection(offer.clone(), address, &config)
                        .await
                    {
                        Ok(addr) => {
                            tracing::info!(
                                peer_id = %peer_id.0,
                                reflexive_addr = %address,
                                duration = ?attempt_start.elapsed(),
                                "STUN reflexive connection successful"
                            );

                            return Ok(ConnectionResult::DirectConnection {
                                transport: offer.clone(),
                                address: addr,
                                method: ConnectionMethod::StunReflexive,
                            });
                        }
                        Err(e) => {
                            attempts.push(ConnectionAttempt {
                                method: ConnectionMethod::StunReflexive,
                                transport: offer.clone(),
                                address: address.clone(),
                                duration: attempt_start.elapsed(),
                                error: Some(e.to_string()),
                            });

                            tracing::debug!(
                                reflexive_addr = %address,
                                error = %e,
                                duration = ?attempt_start.elapsed(),
                                "STUN reflexive connection failed"
                            );
                        }
                    }
                }

                // Try discovering new reflexive address for QUIC transports
                if offer.kind == TransportKind::Quic {
                    let attempt_start = std::time::Instant::now();

                    match self.discover_and_try_stun(offer.clone(), &config).await {
                        Ok((addr, stun_result)) => {
                            tracing::info!(
                                peer_id = %peer_id.0,
                                discovered_addr = %stun_result.reflexive_address,
                                duration = ?attempt_start.elapsed(),
                                "STUN discovery and connection successful"
                            );

                            return Ok(ConnectionResult::DirectConnection {
                                transport: offer.clone(),
                                address: addr,
                                method: ConnectionMethod::StunReflexive,
                            });
                        }
                        Err(e) => {
                            attempts.push(ConnectionAttempt {
                                method: ConnectionMethod::StunReflexive,
                                transport: offer.clone(),
                                address: "stun_discovery".to_string(),
                                duration: attempt_start.elapsed(),
                                error: Some(e.to_string()),
                            });

                            tracing::debug!(
                                error = %e,
                                duration = ?attempt_start.elapsed(),
                                "STUN discovery failed"
                            );
                        }
                    }
                }
            }
        }

        // Phase 3: Try relay fallback if enabled
        if config.enable_relay_fallback {
            for offer in &offers {
                if start_time.elapsed() > config.total_timeout {
                    break;
                }

                if offer.kind == TransportKind::WebSocket {
                    let attempt_start = std::time::Instant::now();

                    match self
                        .try_relay_connection(offer.clone(), &peer_id, &config)
                        .await
                    {
                        Ok(addr) => {
                            tracing::info!(
                                peer_id = %peer_id.0,
                                duration = ?attempt_start.elapsed(),
                                "WebSocket relay connection successful"
                            );

                            return Ok(ConnectionResult::DirectConnection {
                                transport: offer.clone(),
                                address: addr,
                                method: ConnectionMethod::WebSocketRelay,
                            });
                        }
                        Err(e) => {
                            attempts.push(ConnectionAttempt {
                                method: ConnectionMethod::WebSocketRelay,
                                transport: offer.clone(),
                                address: "relay".to_string(),
                                duration: attempt_start.elapsed(),
                                error: Some(e.to_string()),
                            });

                            tracing::debug!(
                                error = %e,
                                duration = ?attempt_start.elapsed(),
                                "WebSocket relay connection failed"
                            );
                        }
                    }
                }
            }
        }

        // All methods failed
        let final_error = format!(
            "All connection methods failed after {} attempts in {:?}",
            attempts.len(),
            start_time.elapsed()
        );

        tracing::warn!(
            peer_id = %peer_id.0,
            attempts = attempts.len(),
            duration = ?start_time.elapsed(),
            "Connection establishment failed"
        );

        Ok(ConnectionResult::Failed {
            attempts,
            final_error,
        })
    }

    /// Establish connection with coordinated hole-punching using offer/answer exchange
    pub async fn establish_connection_with_punch(
        &self,
        peer_id: DeviceId,
        offer: &TransportOfferPayload,
        answer: &TransportOfferPayload,
        config: ConnectionConfig,
    ) -> Result<ConnectionResult, AuraError> {
        tracing::info!(
            peer_id = %peer_id.0,
            offer_has_punch = offer.punch_nonce.is_some(),
            answer_has_punch = answer.punch_nonce.is_some(),
            "Starting coordinated hole-punch connection"
        );

        // Check if both sides have punch nonces for coordination
        let (offer_nonce, answer_nonce) = match (offer.punch_nonce, answer.punch_nonce) {
            (Some(offer_nonce), Some(answer_nonce)) => (offer_nonce, answer_nonce),
            _ => {
                tracing::debug!("No coordinated punch nonces, falling back to standard connection");
                return self
                    .establish_connection(peer_id, offer.transports.clone(), config)
                    .await;
            }
        };

        let start_time = std::time::Instant::now();

        // Try coordinated hole-punching for QUIC transports with reflexive addresses
        if config.enable_hole_punch {
            for transport in &offer.transports {
                if start_time.elapsed() > config.total_timeout {
                    break;
                }

                if transport.kind == TransportKind::Quic
                    && !transport.reflexive_addresses.is_empty()
                {
                    let attempt_start = std::time::Instant::now();

                    match self
                        .try_coordinated_punch(
                            transport,
                            &peer_id,
                            offer_nonce,
                            answer_nonce,
                            &config,
                        )
                        .await
                    {
                        Ok(addr) => {
                            tracing::info!(
                                peer_id = %peer_id.0,
                                duration = ?attempt_start.elapsed(),
                                "Coordinated hole-punch successful"
                            );

                            return Ok(ConnectionResult::DirectConnection {
                                transport: transport.clone(),
                                address: addr,
                                method: ConnectionMethod::HolePunch,
                            });
                        }
                        Err(e) => {
                            tracing::debug!(
                                error = %e,
                                duration = ?attempt_start.elapsed(),
                                "Coordinated hole-punch failed"
                            );
                        }
                    }
                }
            }
        }

        // Fallback to standard connection establishment
        tracing::debug!("Hole-punch failed, falling back to standard connection methods");
        self.establish_connection(peer_id, offer.transports.clone(), config)
            .await
    }

    /// Try direct connection to address
    async fn try_direct_connection(
        &self,
        transport: TransportDescriptor,
        address: &str,
        config: &ConnectionConfig,
    ) -> Result<SocketAddr, AuraError> {
        let addr: SocketAddr = address.parse().map_err(|e| {
            AuraError::coordination_failed(format!("Invalid address '{}': {}", address, e))
        })?;

        // Implement actual connection logic based on transport type
        match transport.kind {
            TransportKind::Quic => {
                // Would implement QUIC connection
                timeout(config.attempt_timeout, self.try_quic_connection(addr))
                    .await
                    .map_err(|_| {
                        AuraError::coordination_failed("QUIC connection timeout".to_string())
                    })?
            }
            TransportKind::WebSocket => {
                // Would implement WebSocket connection
                timeout(config.attempt_timeout, self.try_websocket_connection(addr))
                    .await
                    .map_err(|_| {
                        AuraError::coordination_failed("WebSocket connection timeout".to_string())
                    })?
            }
            _ => Err(AuraError::coordination_failed(format!(
                "Unsupported transport for direct connection: {:?}",
                transport.kind
            ))),
        }
    }

    /// Try reflexive connection via STUN address
    async fn try_reflexive_connection(
        &self,
        transport: TransportDescriptor,
        reflexive_address: &str,
        config: &ConnectionConfig,
    ) -> Result<SocketAddr, AuraError> {
        // Parse reflexive address
        let addr: SocketAddr = reflexive_address.parse().map_err(|e| {
            AuraError::coordination_failed(format!(
                "Invalid reflexive address '{}': {}",
                reflexive_address, e
            ))
        })?;

        // Only QUIC supports reflexive connections in this implementation
        if transport.kind != TransportKind::Quic {
            return Err(AuraError::coordination_failed(
                "Reflexive connections only supported for QUIC transport".to_string(),
            ));
        }

        timeout(config.attempt_timeout, self.try_quic_connection(addr))
            .await
            .map_err(|_| {
                AuraError::coordination_failed("QUIC reflexive connection timeout".to_string())
            })?
    }

    /// Discover STUN reflexive address and try connection
    async fn discover_and_try_stun(
        &self,
        transport: TransportDescriptor,
        config: &ConnectionConfig,
    ) -> Result<(SocketAddr, StunResult), AuraError> {
        // Discover reflexive address
        let stun_result = self
            .stun_client
            .discover_reflexive_address()
            .await?
            .ok_or_else(|| AuraError::coordination_failed("STUN discovery failed".to_string()))?;

        tracing::debug!(
            local_addr = %stun_result.local_address,
            reflexive_addr = %stun_result.reflexive_address,
            stun_server = %stun_result.stun_server,
            "STUN discovery successful"
        );

        // Try connection using discovered address
        let connection_addr = timeout(
            config.attempt_timeout,
            self.try_quic_connection(stun_result.reflexive_address),
        )
        .await
        .map_err(|_| {
            AuraError::coordination_failed("QUIC STUN connection timeout".to_string())
        })??;

        Ok((connection_addr, stun_result))
    }

    /// Try relay connection via WebSocket
    async fn try_relay_connection(
        &self,
        transport: TransportDescriptor,
        peer_id: &DeviceId,
        config: &ConnectionConfig,
    ) -> Result<SocketAddr, AuraError> {
        // Implementation would:
        // 1. Find suitable relay from guardian/friend list
        // 2. Establish WebSocket connection to relay
        // 3. Send relay request for target peer
        // 4. Return relay connection details

        // For now, return placeholder error
        Err(AuraError::coordination_failed(
            "Relay connections not yet implemented".to_string(),
        ))
    }

    /// Try QUIC connection (placeholder)
    async fn try_quic_connection(&self, addr: SocketAddr) -> Result<SocketAddr, AuraError> {
        // Placeholder implementation
        // In real implementation, this would:
        // 1. Create QUIC client
        // 2. Attempt connection to addr
        // 3. Verify handshake
        // 4. Return connection details

        tracing::debug!(addr = %addr, "Simulating QUIC connection attempt");

        // Simulate connection delay
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Simulate connection failure for demo
        Err(AuraError::coordination_failed(
            "QUIC connection simulation".to_string(),
        ))
    }

    /// Try WebSocket connection (placeholder)
    async fn try_websocket_connection(&self, addr: SocketAddr) -> Result<SocketAddr, AuraError> {
        // Placeholder implementation
        // In real implementation, this would:
        // 1. Create WebSocket client
        // 2. Attempt connection to addr
        // 3. Verify handshake
        // 4. Return connection details

        tracing::debug!(addr = %addr, "Simulating WebSocket connection attempt");

        // Simulate connection delay
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Simulate connection failure for demo
        Err(AuraError::coordination_failed(
            "WebSocket connection simulation".to_string(),
        ))
    }

    /// Try coordinated hole-punching using offer/answer nonces
    async fn try_coordinated_punch(
        &self,
        transport: &TransportDescriptor,
        peer_id: &DeviceId,
        offer_nonce: [u8; 32],
        answer_nonce: [u8; 32],
        config: &ConnectionConfig,
    ) -> Result<SocketAddr, AuraError> {
        tracing::debug!(
            peer_id = %peer_id.0,
            reflexive_addrs = %transport.reflexive_addresses.len(),
            "Starting coordinated hole-punch"
        );

        // Use the first reflexive address for punch coordination
        let reflexive_addr = transport.reflexive_addresses.first().ok_or_else(|| {
            AuraError::coordination_failed("No reflexive addresses available".to_string())
        })?;

        let peer_addr: SocketAddr = reflexive_addr.parse().map_err(|e| {
            AuraError::coordination_failed(format!(
                "Invalid reflexive address '{}': {}",
                reflexive_addr, e
            ))
        })?;

        // Create punch session with combined nonce (offer XOR answer for determinism)
        let mut combined_nonce = [0u8; 32];
        for i in 0..32 {
            combined_nonce[i] = offer_nonce[i] ^ answer_nonce[i];
        }

        // Bind to local address for punch session
        let local_bind_addr: SocketAddr = "0.0.0.0:0".parse().unwrap();
        let punch_session = PunchSession::new(
            self.device_id.clone(),
            local_bind_addr,
            config.punch_config.clone(),
        )
        .await?;

        tracing::debug!(
            local_addr = ?punch_session.local_addr(),
            peer_addr = %peer_addr,
            "Starting simultaneous punch session"
        );

        // Perform simultaneous punch with timeout
        let punch_result = timeout(
            config.attempt_timeout,
            punch_session.punch_with_peer(peer_addr),
        )
        .await
        .map_err(|_| AuraError::coordination_failed("Punch session timeout".to_string()))??;

        match punch_result {
            PunchResult::Success {
                local_addr,
                peer_addr: confirmed_peer_addr,
                session_duration,
                ..
            } => {
                tracing::info!(
                    local_addr = %local_addr,
                    peer_addr = %confirmed_peer_addr,
                    duration = ?session_duration,
                    "Punch session successful, NAT mappings established"
                );

                // Return the local address for subsequent QUIC connection
                Ok(local_addr)
            }
            PunchResult::Failed {
                reason,
                packets_sent,
                packets_received,
                ..
            } => {
                tracing::debug!(
                    reason = %reason,
                    packets_sent = packets_sent,
                    packets_received = packets_received,
                    "Punch session failed"
                );

                Err(AuraError::coordination_failed(format!(
                    "Punch failed: {}",
                    reason
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_connection_manager_creation() {
        let device_id = DeviceId("test_device".to_string());
        let stun_config = StunConfig::default();
        let manager = ConnectionManager::new(device_id.clone(), stun_config);

        assert_eq!(manager.device_id, device_id);
    }

    #[tokio::test]
    async fn test_connection_priority_logic() {
        let device_id = DeviceId("test_device".to_string());
        let stun_config = StunConfig::default();
        let manager = ConnectionManager::new(device_id.clone(), stun_config);

        let offers = vec![
            TransportDescriptor::quic("192.168.1.100:8080".to_string(), "aura".to_string()),
            TransportDescriptor::websocket("ws://relay.example.com:8081".to_string()),
        ];

        let config = ConnectionConfig {
            attempt_timeout: Duration::from_millis(100),
            total_timeout: Duration::from_millis(500),
            enable_stun: false,           // Disable for test
            enable_hole_punch: false,     // Disable for test
            enable_relay_fallback: false, // Disable for test
            punch_config: PunchConfig::default(),
        };

        let peer_id = DeviceId("peer_device".to_string());
        let result = manager
            .establish_connection(peer_id, offers, config)
            .await
            .unwrap();

        // Should fail since we have placeholder implementations
        match result {
            ConnectionResult::Failed { attempts, .. } => {
                assert!(!attempts.is_empty());
                assert_eq!(attempts[0].method, ConnectionMethod::Direct);
            }
            _ => panic!("Expected connection to fail with placeholder implementations"),
        }
    }

    #[test]
    fn test_connection_config_defaults() {
        let config = ConnectionConfig::default();
        assert_eq!(config.attempt_timeout, Duration::from_secs(2));
        assert_eq!(config.total_timeout, Duration::from_secs(10));
        assert!(config.enable_stun);
        assert!(config.enable_hole_punch);
        assert!(config.enable_relay_fallback);
    }

    #[test]
    fn test_transport_descriptor_extensions() {
        let mut transport =
            TransportDescriptor::quic("192.168.1.100:8080".to_string(), "aura".to_string());

        assert_eq!(transport.local_addresses.len(), 1);
        assert_eq!(transport.reflexive_addresses.len(), 0);

        transport.add_reflexive_address("203.0.113.42:12345".to_string());
        assert_eq!(transport.reflexive_addresses.len(), 1);

        let all_addrs = transport.get_all_addresses();
        assert_eq!(all_addrs.len(), 2);

        let priority_addrs = transport.get_priority_addresses();
        assert_eq!(priority_addrs[0], "203.0.113.42:12345"); // Reflexive first
        assert_eq!(priority_addrs[1], "192.168.1.100:8080"); // Local second
    }

    #[tokio::test]
    async fn test_coordinated_hole_punch() {
        use aura_protocol::messages::social::rendezvous::{
            TransportDescriptor, TransportKind, TransportOfferPayload,
        };

        let device_id = DeviceId("test_device".to_string());
        let stun_config = StunConfig::default();
        let manager = ConnectionManager::new(device_id.clone(), stun_config);

        // Create transport with reflexive address
        let mut transport = TransportDescriptor {
            kind: TransportKind::Quic,
            metadata: std::collections::BTreeMap::new(),
            local_addresses: vec!["192.168.1.100:8080".to_string()],
            reflexive_addresses: vec!["203.0.113.42:12345".to_string()],
        };

        // Create offer and answer with punch nonces
        let offer_nonce = [1u8; 32];
        let answer_nonce = [2u8; 32];

        let offer = TransportOfferPayload::new_offer_with_punch(
            vec![transport.clone()],
            vec![],
            offer_nonce,
        );

        let answer = TransportOfferPayload::new_answer_with_punch(vec![transport], 0, answer_nonce);

        let config = ConnectionConfig {
            attempt_timeout: Duration::from_millis(100),
            total_timeout: Duration::from_millis(500),
            enable_stun: false,
            enable_hole_punch: true,
            enable_relay_fallback: false,
            punch_config: PunchConfig {
                punch_duration: Duration::from_millis(50),
                punch_interval: Duration::from_millis(10),
                receive_timeout: Duration::from_millis(5),
                max_packet_size: 256,
            },
        };

        let peer_id = DeviceId("peer_device".to_string());
        let result = manager
            .establish_connection_with_punch(peer_id, &offer, &answer, config)
            .await
            .unwrap();

        // Should fail since we have placeholder implementations and no actual peer
        match result {
            ConnectionResult::Failed { attempts, .. } => {
                // This is expected - we don't have actual QUIC/punch infrastructure running
                assert!(!attempts.is_empty());
            }
            _ => {
                // Unexpected success in test environment would be surprising
                // but not necessarily wrong if punch logic worked
            }
        }
    }

    #[test]
    fn test_transport_offer_punch_nonce() {
        use aura_protocol::messages::social::rendezvous::TransportOfferPayload;

        let punch_nonce = [42u8; 32];
        let offer = TransportOfferPayload::new_offer_with_punch(vec![], vec![], punch_nonce);

        assert_eq!(offer.get_punch_nonce(), Some(punch_nonce));

        let basic_offer = TransportOfferPayload::new_offer(vec![], vec![]);
        assert_eq!(basic_offer.get_punch_nonce(), None);

        let enhanced_offer = basic_offer.with_punch_nonce(punch_nonce);
        assert_eq!(enhanced_offer.get_punch_nonce(), Some(punch_nonce));
    }
}
