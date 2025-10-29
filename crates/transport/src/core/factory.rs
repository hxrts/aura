//! Transport factory for creating and configuring transport implementations
//!
//! Provides a clean interface for instantiating different transport types
//! with appropriate configuration and capability wrapping.

use crate::{
    adapters::{
        https_relay::HttpsRelayTransport,
        memory::MemoryTransport,
        noise_tcp::{NoiseTcpTransport, NoiseTcpTransportBuilder},
        simple_tcp::{SimpleTcpTransport, SimpleTcpTransportBuilder},
    },
    CapabilityTransportAdapter, ConnectionManager, TransportError, TransportErrorBuilder,
    TransportResult,
};
use aura_crypto::{DeviceKeyManager, Effects};
use aura_types::{DeviceId, DeviceIdExt, IndividualId};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Enum for different transport implementations
pub enum AnyTransport {
    /// In-memory transport for testing and local dev
    Memory(MemoryTransport),
    /// HTTPS relay transport for production
    HttpsRelay(HttpsRelayTransport),
    /// Noise protocol over TCP transport
    NoiseTcp(NoiseTcpTransport),
    /// Simple TCP transport without encryption
    SimpleTcp(SimpleTcpTransport),
}

#[async_trait::async_trait]
impl ConnectionManager for AnyTransport {
    async fn connect(
        &self,
        peer_id: &str,
        my_ticket: &crate::PresenceTicket,
        peer_ticket: &crate::PresenceTicket,
    ) -> TransportResult<crate::Connection> {
        match self {
            AnyTransport::Memory(t) => t.connect(peer_id, my_ticket, peer_ticket).await,
            AnyTransport::HttpsRelay(t) => t.connect(peer_id, my_ticket, peer_ticket).await,
            AnyTransport::NoiseTcp(t) => t.connect(peer_id, my_ticket, peer_ticket).await,
            AnyTransport::SimpleTcp(t) => t.connect(peer_id, my_ticket, peer_ticket).await,
        }
    }

    async fn send(&self, conn: &crate::Connection, message: &[u8]) -> TransportResult<()> {
        match self {
            AnyTransport::Memory(t) => t.send(conn, message).await,
            AnyTransport::HttpsRelay(t) => t.send(conn, message).await,
            AnyTransport::NoiseTcp(t) => t.send(conn, message).await,
            AnyTransport::SimpleTcp(t) => t.send(conn, message).await,
        }
    }

    async fn receive(
        &self,
        conn: &crate::Connection,
        timeout: std::time::Duration,
    ) -> TransportResult<Option<Vec<u8>>> {
        match self {
            AnyTransport::Memory(t) => t.receive(conn, timeout).await,
            AnyTransport::HttpsRelay(t) => t.receive(conn, timeout).await,
            AnyTransport::NoiseTcp(t) => t.receive(conn, timeout).await,
            AnyTransport::SimpleTcp(t) => t.receive(conn, timeout).await,
        }
    }

    async fn broadcast(
        &self,
        connections: &[crate::Connection],
        message: &[u8],
    ) -> TransportResult<crate::BroadcastResult> {
        match self {
            AnyTransport::Memory(t) => t.broadcast(connections, message).await,
            AnyTransport::HttpsRelay(t) => t.broadcast(connections, message).await,
            AnyTransport::NoiseTcp(t) => t.broadcast(connections, message).await,
            AnyTransport::SimpleTcp(t) => t.broadcast(connections, message).await,
        }
    }

    async fn disconnect(&self, conn: &crate::Connection) -> TransportResult<()> {
        match self {
            AnyTransport::Memory(t) => t.disconnect(conn).await,
            AnyTransport::HttpsRelay(t) => t.disconnect(conn).await,
            AnyTransport::NoiseTcp(t) => t.disconnect(conn).await,
            AnyTransport::SimpleTcp(t) => t.disconnect(conn).await,
        }
    }

    async fn is_connected(&self, conn: &crate::Connection) -> bool {
        match self {
            AnyTransport::Memory(t) => t.is_connected(conn).await,
            AnyTransport::HttpsRelay(t) => t.is_connected(conn).await,
            AnyTransport::NoiseTcp(t) => t.is_connected(conn).await,
            AnyTransport::SimpleTcp(t) => t.is_connected(conn).await,
        }
    }
}

/// Configuration for transport creation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TransportConfig {
    /// In-memory transport adapter for testing
    Memory {
        /// Optional device ID for transport identification
        device_id: Option<DeviceId>,
    },

    /// Simulation transport for deterministic testing
    #[serde(skip)]
    Simulation {
        /// Participant ID for simulation
        participant_id: String,
    },

    /// HTTPS relay transport configuration
    HttpsRelay {
        /// The relay server URL
        relay_url: String,
        /// Connection timeout in seconds
        timeout_seconds: u64,
        /// Maximum number of connection retries
        max_retries: u32,
    },

    /// Direct P2P transport with Noise protocol
    NoiseTcp {
        /// Address to listen on for incoming connections
        listen_address: String,
        /// Connection timeout in seconds
        connection_timeout_seconds: u64,
    },

    /// Simple TCP transport (without encryption, for testing)
    SimpleTcp {
        /// Address to listen on for incoming connections
        listen_address: String,
        /// Connection timeout in seconds
        connection_timeout_seconds: u64,
    },

    /// Future: QUIC transport configuration
    #[serde(skip)]
    Quic {
        /// Address to bind the QUIC endpoint
        bind_address: String,
        /// Optional path to TLS certificate
        certificate_path: Option<String>,
        /// Optional path to private key
        private_key_path: Option<String>,
    },

    /// Future: WebRTC transport configuration
    #[serde(skip)]
    WebRtc {
        /// List of ICE server URLs
        ice_servers: Vec<String>,
        /// Optional TURN server username
        turn_username: Option<String>,
        /// Optional TURN server password
        turn_password: Option<String>,
    },
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self::Memory { device_id: None }
    }
}

/// Configuration for capability-based transport wrapping
pub struct CapabilityConfig {
    /// Individual identity for capability evaluation
    pub individual_id: IndividualId,
    /// Device key manager for cryptographic operations
    pub device_key_manager: DeviceKeyManager,
    /// Effects for deterministic operations
    pub effects: Effects,
}

/// Transport factory for creating configured transport instances
pub struct TransportFactory;

impl TransportFactory {
    /// Create a raw transport without capability wrapping
    ///
    /// # Arguments
    /// * `config` - Transport configuration specifying type and parameters
    ///
    /// # Returns
    /// * `Ok(AnyTransport)` - Configured transport instance
    /// * `Err(TransportError)` - Configuration or creation error
    pub async fn create_raw_transport(config: &TransportConfig) -> TransportResult<AnyTransport> {
        match config {
            TransportConfig::Memory { .. } => {
                let transport = MemoryTransport::new();
                Ok(AnyTransport::Memory(transport))
            }

            TransportConfig::HttpsRelay {
                relay_url,
                timeout_seconds,
                max_retries,
            } => {
                // Create a device ID for HTTPS relay transport
                let effects = Effects::test(); // TODO: Use real effects in production
                let device_id = DeviceId::new_with_effects(&effects);

                let transport = HttpsRelayTransport::new(
                    device_id,
                    relay_url.clone(),
                    *timeout_seconds,
                    *max_retries,
                );
                Ok(AnyTransport::HttpsRelay(transport))
            }

            TransportConfig::NoiseTcp {
                listen_address,
                connection_timeout_seconds,
            } => {
                // Create device key and ID for Noise TCP transport
                let effects = Effects::test(); // TODO: Use real effects in production
                let device_key =
                    ed25519_dalek::SigningKey::from_bytes(&effects.random_bytes::<32>());
                let device_id = DeviceId::new_with_effects(&effects);

                // Parse listen address
                let listen_addr: std::net::SocketAddr = listen_address.parse().map_err(|e| {
                    TransportError::configuration_error(&format!("Invalid listen address: {}", e))
                })?;

                let transport = NoiseTcpTransportBuilder::new()
                    .device_key(device_key)
                    .device_id(device_id)
                    .listen_addr(listen_addr)
                    .connection_timeout(std::time::Duration::from_secs(*connection_timeout_seconds))
                    .build()
                    .await?;

                Ok(AnyTransport::NoiseTcp(transport))
            }

            TransportConfig::SimpleTcp {
                listen_address,
                connection_timeout_seconds,
            } => {
                // Create device ID for Simple TCP transport
                let effects = Effects::test(); // TODO: Use real effects in production
                let device_id = DeviceId::new_with_effects(&effects);

                // Parse listen address
                let listen_addr: std::net::SocketAddr = listen_address.parse().map_err(|e| {
                    TransportError::configuration_error(&format!("Invalid listen address: {}", e))
                })?;

                let transport = SimpleTcpTransportBuilder::new()
                    .device_id(device_id)
                    .listen_addr(listen_addr)
                    .connection_timeout(std::time::Duration::from_secs(*connection_timeout_seconds))
                    .build()
                    .await?;

                Ok(AnyTransport::SimpleTcp(transport))
            }

            _ => Err(TransportErrorBuilder::transport(
                "Transport type not yet implemented".to_string(),
            )),
        }
    }

    /// Create a capability-wrapped transport
    ///
    /// This wraps the raw transport with capability-based authentication
    /// and authorization checking.
    ///
    /// # Arguments
    /// * `transport_config` - Base transport configuration
    /// * `capability_config` - Capability system configuration
    ///
    /// # Returns
    /// * `Ok(CapabilityTransportAdapter<AnyTransport>)` - Capability-wrapped transport
    /// * `Err(TransportError)` - Configuration or creation error
    pub async fn create_capability_transport(
        transport_config: &TransportConfig,
        capability_config: CapabilityConfig,
    ) -> TransportResult<CapabilityTransportAdapter<AnyTransport>> {
        let raw_transport = Self::create_raw_transport(transport_config).await?;

        let capability_transport = CapabilityTransportAdapter::new(
            Arc::new(raw_transport),
            capability_config.individual_id,
            capability_config.device_key_manager,
            capability_config.effects,
        );

        Ok(capability_transport)
    }

    /// Create a transport from configuration with automatic capability wrapping
    ///
    /// This is the main entry point for creating production-ready transports.
    /// All transports are automatically wrapped with capability-based authentication.
    ///
    /// # Arguments
    /// * `transport_config` - Base transport configuration
    /// * `capability_config` - Capability system configuration
    ///
    /// # Returns
    /// * `Ok(CapabilityTransportAdapter<AnyTransport>)` - Ready-to-use capability transport
    /// * `Err(TransportError)` - Configuration or creation error
    pub async fn create_transport(
        transport_config: &TransportConfig,
        capability_config: CapabilityConfig,
    ) -> TransportResult<CapabilityTransportAdapter<AnyTransport>> {
        Self::create_capability_transport(transport_config, capability_config).await
    }

    /// Get available transport types
    ///
    /// Returns a list of transport types that can be created by this factory.
    /// Useful for configuration validation and user interfaces.
    pub fn available_transport_types() -> Vec<&'static str> {
        vec![
            "memory",
            "simulation",  // Created directly by simulation system
            "https_relay", // Future implementation
            "quic",        // Future implementation
            "webrtc",      // Future implementation
        ]
    }

    /// Check if a transport type is implemented
    ///
    /// # Arguments
    /// * `transport_type` - Transport type name to check
    ///
    /// # Returns
    /// * `true` if the transport type is implemented and can be created
    /// * `false` if the transport type is not yet implemented
    pub fn is_transport_implemented(transport_type: &str) -> bool {
        matches!(transport_type, "memory" | "https_relay")
    }

    /// Validate transport configuration
    ///
    /// Checks if the provided configuration is valid and can be used
    /// to create a transport instance.
    ///
    /// # Arguments
    /// * `config` - Transport configuration to validate
    ///
    /// # Returns
    /// * `Ok(())` if configuration is valid
    /// * `Err(TransportError)` if configuration has issues
    pub fn validate_config(config: &TransportConfig) -> TransportResult<()> {
        match config {
            TransportConfig::Memory { .. } => {
                // Memory transport always valid
                Ok(())
            }

            TransportConfig::Simulation { participant_id } => {
                if participant_id.is_empty() {
                    return Err(TransportErrorBuilder::transport(
                        "Simulation participant ID cannot be empty".to_string(),
                    ));
                }
                // Note: Simulation transport should be created directly by simulation system
                Ok(())
            }

            TransportConfig::HttpsRelay {
                relay_url,
                timeout_seconds,
                max_retries,
            } => {
                if relay_url.is_empty() {
                    return Err(TransportErrorBuilder::transport(
                        "HTTPS relay URL cannot be empty".to_string(),
                    ));
                }

                if *timeout_seconds == 0 {
                    return Err(TransportErrorBuilder::transport(
                        "Timeout must be greater than 0".to_string(),
                    ));
                }

                if *max_retries > 10 {
                    return Err(TransportErrorBuilder::transport(
                        "Max retries should not exceed 10".to_string(),
                    ));
                }

                // HTTPS relay is now implemented
                Ok(())
            }

            TransportConfig::Quic { bind_address, .. } => {
                if bind_address.is_empty() {
                    return Err(TransportErrorBuilder::transport(
                        "QUIC bind address cannot be empty".to_string(),
                    ));
                }

                // For now, reject since not implemented
                Err(TransportErrorBuilder::transport(
                    "QUIC transport not yet implemented".to_string(),
                ))
            }

            TransportConfig::WebRtc { ice_servers, .. } => {
                if ice_servers.is_empty() {
                    return Err(TransportErrorBuilder::transport(
                        "WebRTC requires at least one ICE server".to_string(),
                    ));
                }

                // For now, reject since not implemented
                Err(TransportErrorBuilder::transport(
                    "WebRTC transport not yet implemented".to_string(),
                ))
            }

            TransportConfig::NoiseTcp {
                listen_address,
                connection_timeout_seconds,
            } => {
                if listen_address.is_empty() {
                    return Err(TransportErrorBuilder::transport(
                        "Noise TCP transport requires listen address".to_string(),
                    ));
                }
                if *connection_timeout_seconds == 0 {
                    return Err(TransportErrorBuilder::transport(
                        "Noise TCP transport requires positive timeout".to_string(),
                    ));
                }
                Ok(())
            }

            TransportConfig::SimpleTcp {
                listen_address,
                connection_timeout_seconds,
            } => {
                if listen_address.is_empty() {
                    return Err(TransportErrorBuilder::transport(
                        "Simple TCP transport requires listen address".to_string(),
                    ));
                }
                if *connection_timeout_seconds == 0 {
                    return Err(TransportErrorBuilder::transport(
                        "Simple TCP transport requires positive timeout".to_string(),
                    ));
                }
                Ok(())
            }
        }
    }
}

/// Builder pattern for transport configuration
pub struct TransportConfigBuilder {
    config: TransportConfig,
}

impl TransportConfigBuilder {
    /// Start building a memory transport configuration
    pub fn memory() -> Self {
        Self {
            config: TransportConfig::Memory { device_id: None },
        }
    }

    /// Start building a simulation transport configuration
    pub fn simulation(participant_id: impl Into<String>) -> Self {
        Self {
            config: TransportConfig::Simulation {
                participant_id: participant_id.into(),
            },
        }
    }

    /// Start building an HTTPS relay transport configuration
    pub fn https_relay(relay_url: impl Into<String>) -> Self {
        Self {
            config: TransportConfig::HttpsRelay {
                relay_url: relay_url.into(),
                timeout_seconds: 30,
                max_retries: 3,
            },
        }
    }

    /// Start building a QUIC transport configuration
    pub fn quic(bind_address: impl Into<String>) -> Self {
        Self {
            config: TransportConfig::Quic {
                bind_address: bind_address.into(),
                certificate_path: None,
                private_key_path: None,
            },
        }
    }

    /// Start building a WebRTC transport configuration
    pub fn webrtc() -> Self {
        Self {
            config: TransportConfig::WebRtc {
                ice_servers: vec!["stun:stun.l.google.com:19302".to_string()],
                turn_username: None,
                turn_password: None,
            },
        }
    }

    /// Set device ID for memory transport
    pub fn with_device_id(mut self, device_id: DeviceId) -> Self {
        if let TransportConfig::Memory {
            device_id: ref mut id,
        } = self.config
        {
            *id = Some(device_id);
        }
        self
    }

    /// Set timeout for HTTPS relay transport
    pub fn with_timeout(mut self, timeout_seconds: u64) -> Self {
        if let TransportConfig::HttpsRelay {
            timeout_seconds: ref mut timeout,
            ..
        } = self.config
        {
            *timeout = timeout_seconds;
        }
        self
    }

    /// Set max retries for HTTPS relay transport
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        if let TransportConfig::HttpsRelay {
            max_retries: ref mut retries,
            ..
        } = self.config
        {
            *retries = max_retries;
        }
        self
    }

    /// Add ICE server for WebRTC transport
    pub fn with_ice_server(mut self, ice_server: impl Into<String>) -> Self {
        if let TransportConfig::WebRtc { ice_servers, .. } = &mut self.config {
            ice_servers.push(ice_server.into());
        }
        self
    }

    /// Set TURN credentials for WebRTC transport
    pub fn with_turn_credentials(
        mut self,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        if let TransportConfig::WebRtc {
            turn_username,
            turn_password,
            ..
        } = &mut self.config
        {
            *turn_username = Some(username.into());
            *turn_password = Some(password.into());
        }
        self
    }

    /// Build the transport configuration
    pub fn build(self) -> TransportConfig {
        self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
    use DeviceId;

    #[test]
    fn test_transport_config_builder() {
        // Test memory transport builder
        let memory_config = TransportConfigBuilder::memory()
            .with_device_id(DeviceId(Uuid::new_v4()))
            .build();

        assert!(matches!(memory_config, TransportConfig::Memory { .. }));

        // Test simulation transport builder
        let sim_config = TransportConfigBuilder::simulation("participant-1").build();

        assert!(matches!(sim_config, TransportConfig::Simulation { .. }));

        // Test HTTPS relay builder
        let https_config = TransportConfigBuilder::https_relay("https://relay.example.com")
            .with_timeout(60)
            .with_max_retries(5)
            .build();

        assert!(matches!(https_config, TransportConfig::HttpsRelay { .. }));

        // Test WebRTC builder
        let webrtc_config = TransportConfigBuilder::webrtc()
            .with_ice_server("stun:stun.example.com:3478")
            .with_turn_credentials("user", "pass")
            .build();

        assert!(matches!(webrtc_config, TransportConfig::WebRtc { .. }));
    }

    #[test]
    fn test_config_validation() {
        // Valid memory config
        let memory_config = TransportConfig::Memory { device_id: None };
        assert!(TransportFactory::validate_config(&memory_config).is_ok());

        // Valid simulation config
        let sim_config = TransportConfig::Simulation {
            participant_id: "participant-1".to_string(),
        };
        assert!(TransportFactory::validate_config(&sim_config).is_ok());

        // Invalid simulation config (empty participant ID)
        let invalid_sim = TransportConfig::Simulation {
            participant_id: "".to_string(),
        };
        assert!(TransportFactory::validate_config(&invalid_sim).is_err());

        // Valid HTTPS config
        let valid_https = TransportConfig::HttpsRelay {
            relay_url: "https://relay.example.com".to_string(),
            timeout_seconds: 30,
            max_retries: 3,
        };
        assert!(TransportFactory::validate_config(&valid_https).is_ok());

        // Invalid HTTPS config (empty URL)
        let invalid_https = TransportConfig::HttpsRelay {
            relay_url: "".to_string(),
            timeout_seconds: 30,
            max_retries: 3,
        };
        assert!(TransportFactory::validate_config(&invalid_https).is_err());
    }

    #[test]
    fn test_available_transport_types() {
        let types = TransportFactory::available_transport_types();
        assert!(types.contains(&"memory"));
        assert!(types.contains(&"simulation"));
        assert!(types.contains(&"https_relay"));
        assert!(types.contains(&"quic"));
        assert!(types.contains(&"webrtc"));
    }

    #[test]
    fn test_transport_implementation_status() {
        assert!(TransportFactory::is_transport_implemented("memory"));
        assert!(TransportFactory::is_transport_implemented("https_relay"));
        assert!(!TransportFactory::is_transport_implemented("quic"));
        assert!(!TransportFactory::is_transport_implemented("webrtc"));
    }

    #[tokio::test]
    async fn test_raw_transport_creation() {
        // Test memory transport creation
        let memory_config = TransportConfig::Memory {
            device_id: Some(DeviceId(Uuid::new_v4())),
        };
        let transport = TransportFactory::create_raw_transport(&memory_config).await;
        assert!(transport.is_ok());

        // Test HTTPS relay transport (now implemented)
        let https_config = TransportConfig::HttpsRelay {
            relay_url: "https://relay.example.com".to_string(),
            timeout_seconds: 30,
            max_retries: 3,
        };
        let transport = TransportFactory::create_raw_transport(&https_config).await;
        assert!(transport.is_ok());
    }
}
