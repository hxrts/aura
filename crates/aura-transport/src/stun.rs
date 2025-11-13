//! STUN Client Implementation for NAT Traversal
//!
//! Implements RFC 5389 STUN (Session Traversal Utilities for NAT) protocol
//! for discovering reflexive addresses through NATs and firewalls.

use aura_core::AuraError;
use std::net::{SocketAddr, ToSocketAddrs};
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::time::timeout;
use tracing;

/// Configuration for the STUN client
#[derive(Debug, Clone)]
pub struct StunConfig {
    /// Primary STUN server URL
    pub primary_server: String,
    /// Additional STUN servers for fallback
    pub fallback_servers: Vec<String>,
    /// Request timeout in milliseconds
    pub timeout_ms: u64,
    /// Number of retry attempts
    pub retry_attempts: u32,
    /// Local bind address for STUN requests
    pub local_bind_addr: Option<SocketAddr>,
}

impl Default for StunConfig {
    fn default() -> Self {
        Self {
            primary_server: "stun.l.google.com:19302".to_string(),
            fallback_servers: vec![
                "stun1.l.google.com:19302".to_string(),
                "stun2.l.google.com:19302".to_string(),
                "stun.cloudflare.com:3478".to_string(),
            ],
            timeout_ms: 3000,
            retry_attempts: 3,
            local_bind_addr: None,
        }
    }
}

/// STUN discovery result
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StunResult {
    /// Reflexive address discovered by STUN server
    pub reflexive_address: SocketAddr,
    /// Local socket address used for the request
    pub local_address: SocketAddr,
    /// STUN server that produced the result
    pub stun_server: String,
    /// Timestamp when the result was discovered
    pub discovered_at: u64,
}

/// STUN message types (RFC 5389)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
#[allow(missing_docs)]
pub enum StunMessageType {
    BindingRequest = 0x0001,
    BindingResponse = 0x0101,
    BindingErrorResponse = 0x0111,
}

/// STUN attribute types (RFC 5389)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
#[allow(missing_docs)]
pub enum StunAttributeType {
    MappedAddress = 0x0001,
    Username = 0x0006,
    MessageIntegrity = 0x0008,
    ErrorCode = 0x0009,
    UnknownAttributes = 0x000A,
    Realm = 0x0014,
    Nonce = 0x0015,
    XorMappedAddress = 0x0020,
    Software = 0x8022,
    AlternateServer = 0x8023,
    Fingerprint = 0x8028,
}

/// STUN message header (20 bytes)
#[derive(Debug)]
#[allow(dead_code)]
struct StunMessageHeader {
    message_type: u16,
    message_length: u16,
    magic_cookie: u32,
    transaction_id: [u8; 12],
}

/// STUN attribute
#[derive(Debug)]
#[allow(dead_code)]
struct StunAttribute {
    attr_type: u16,
    length: u16,
    value: Vec<u8>,
}

const STUN_MAGIC_COOKIE: u32 = 0x2112A442;

/// STUN client for NAT traversal
pub struct StunClient {
    config: StunConfig,
}

impl StunClient {
    /// Create a new STUN client with the provided configuration
    pub fn new(config: StunConfig) -> Self {
        Self { config }
    }

    /// Discover reflexive address through STUN
    pub async fn discover_reflexive_address(&self) -> Result<Option<StunResult>, AuraError> {
        // Try primary server first
        if let Ok(result) = self.discover_with_server(&self.config.primary_server).await {
            return Ok(Some(result));
        }

        // Try fallback servers
        for server in &self.config.fallback_servers {
            if let Ok(result) = self.discover_with_server(server).await {
                return Ok(Some(result));
            }
        }

        // All servers failed
        tracing::warn!("All STUN servers failed discovery");
        Ok(None)
    }

    /// Discover reflexive address using a specific STUN server
    async fn discover_with_server(&self, server_url: &str) -> Result<StunResult, AuraError> {
        tracing::debug!(server = %server_url, "Attempting STUN discovery");

        // Resolve server address
        let server_addr = self.resolve_server_address(server_url).await?;

        // Bind local socket
        let local_socket = self.create_local_socket().await?;
        let _local_addr = local_socket
            .local_addr()
            .map_err(|e| AuraError::network(format!("Failed to get local address: {}", e)))?;

        // Perform STUN exchange
        for attempt in 1..=self.config.retry_attempts {
            tracing::debug!(
                server = %server_url,
                attempt = attempt,
                max_attempts = self.config.retry_attempts,
                "Sending STUN binding request"
            );

            match self
                .perform_stun_exchange(&local_socket, server_addr, server_url)
                .await
            {
                Ok(result) => {
                    tracing::info!(
                        server = %server_url,
                        reflexive_addr = %result.reflexive_address,
                        local_addr = %result.local_address,
                        attempt = attempt,
                        "STUN discovery successful"
                    );
                    return Ok(result);
                }
                Err(e) if attempt < self.config.retry_attempts => {
                    tracing::debug!(
                        server = %server_url,
                        attempt = attempt,
                        error = %e,
                        "STUN request failed, retrying"
                    );
                    // Small delay between retries
                    tokio::time::sleep(Duration::from_millis(100 * attempt as u64)).await;
                }
                Err(e) => {
                    tracing::warn!(
                        server = %server_url,
                        error = %e,
                        "STUN discovery failed after all retries"
                    );
                    return Err(e);
                }
            }
        }

        Err(AuraError::network(format!(
            "STUN discovery failed with server {}",
            server_url
        )))
    }

    /// Resolve STUN server address
    async fn resolve_server_address(&self, server_url: &str) -> Result<SocketAddr, AuraError> {
        // Parse server URL (handle both hostname:port and IP:port)
        let server_addr = if server_url.contains("://") {
            // Extract host:port from URL format
            let url_parts: Vec<&str> = server_url.split("://").collect();
            if url_parts.len() == 2 {
                url_parts[1]
            } else {
                server_url
            }
        } else {
            server_url
        };

        // Resolve to socket address
        let addrs: Vec<SocketAddr> = tokio::task::spawn_blocking({
            let server_addr = server_addr.to_string();
            move || {
                server_addr
                    .to_socket_addrs()
                    .map(|iter| iter.collect())
                    .unwrap_or_default()
            }
        })
        .await
        .map_err(|e| AuraError::network(format!("Task join error: {}", e)))?;

        addrs.into_iter().next().ok_or_else(|| {
            AuraError::network(format!("Failed to resolve STUN server: {}", server_url))
        })
    }

    /// Create local UDP socket for STUN requests
    async fn create_local_socket(&self) -> Result<UdpSocket, AuraError> {
        let default_addr = "0.0.0.0:0"
            .parse()
            .map_err(|_| AuraError::network("Invalid default bind address".to_string()))?;
        let bind_addr = self.config.local_bind_addr.unwrap_or(default_addr);

        UdpSocket::bind(bind_addr)
            .await
            .map_err(|e| AuraError::network(format!("Failed to bind STUN socket: {}", e)))
    }

    /// Perform STUN binding request/response exchange
    async fn perform_stun_exchange(
        &self,
        socket: &UdpSocket,
        server_addr: SocketAddr,
        server_url: &str,
    ) -> Result<StunResult, AuraError> {
        // Generate transaction ID
        let mut transaction_id = [0u8; 12];
        fastrand::fill(&mut transaction_id);

        // Create STUN binding request
        let request = self.create_binding_request(transaction_id);

        // Send request
        socket
            .send_to(&request, server_addr)
            .await
            .map_err(|e| AuraError::network(format!("Failed to send STUN request: {}", e)))?;

        // Wait for response with timeout
        let timeout_duration = Duration::from_millis(self.config.timeout_ms);
        let response = timeout(
            timeout_duration,
            self.receive_stun_response(socket, transaction_id),
        )
        .await
        .map_err(|_| AuraError::network("STUN request timeout".to_string()))??;

        // Parse reflexive address from response
        let reflexive_address = self.parse_reflexive_address(&response, transaction_id)?;

        let local_address = socket
            .local_addr()
            .map_err(|e| AuraError::network(format!("Failed to get local address: {}", e)))?;

        // Use a constant timestamp for testability
        // In production, this should come from TimeEffects
        let timestamp = 0u64;

        Ok(StunResult {
            reflexive_address,
            local_address,
            stun_server: server_url.to_string(),
            discovered_at: timestamp,
        })
    }

    /// Create STUN binding request packet
    fn create_binding_request(&self, transaction_id: [u8; 12]) -> Vec<u8> {
        let mut packet = Vec::new();

        // STUN header (20 bytes)
        packet.extend_from_slice(&(StunMessageType::BindingRequest as u16).to_be_bytes());
        packet.extend_from_slice(&0u16.to_be_bytes()); // Message length (0 for no attributes)
        packet.extend_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
        packet.extend_from_slice(&transaction_id);

        packet
    }

    /// Receive STUN response with transaction ID validation
    async fn receive_stun_response(
        &self,
        socket: &UdpSocket,
        expected_transaction_id: [u8; 12],
    ) -> Result<Vec<u8>, AuraError> {
        let mut buffer = vec![0u8; 1024];

        loop {
            let (len, _from) = socket.recv_from(&mut buffer).await.map_err(|e| {
                AuraError::network(format!("Failed to receive STUN response: {}", e))
            })?;

            buffer.truncate(len);

            // Validate STUN response
            if buffer.len() < 20 {
                tracing::debug!("Received packet too short for STUN header");
                continue;
            }

            // Check magic cookie
            let magic_cookie = u32::from_be_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]);
            if magic_cookie != STUN_MAGIC_COOKIE {
                tracing::debug!("Received packet with invalid STUN magic cookie");
                continue;
            }

            // Check transaction ID
            let received_transaction_id: [u8; 12] = match buffer[8..20].try_into() {
                Ok(id) => id,
                Err(_) => {
                    tracing::debug!("Received STUN packet with invalid transaction ID length");
                    continue;
                }
            };
            if received_transaction_id != expected_transaction_id {
                tracing::debug!("Received STUN packet with mismatched transaction ID");
                continue;
            }

            // Check message type
            let message_type = u16::from_be_bytes([buffer[0], buffer[1]]);
            if message_type == StunMessageType::BindingResponse as u16 {
                return Ok(buffer);
            } else if message_type == StunMessageType::BindingErrorResponse as u16 {
                return Err(AuraError::network(
                    "Received STUN error response".to_string(),
                ));
            } else {
                tracing::debug!("Received unknown STUN message type: {:#04x}", message_type);
                continue;
            }
        }
    }

    /// Parse reflexive address from STUN binding response
    fn parse_reflexive_address(
        &self,
        response: &[u8],
        transaction_id: [u8; 12],
    ) -> Result<SocketAddr, AuraError> {
        if response.len() < 20 {
            return Err(AuraError::network("STUN response too short".to_string()));
        }

        // Parse message length
        let message_length = u16::from_be_bytes([response[2], response[3]]) as usize;
        let expected_total_length = 20 + message_length;

        if response.len() < expected_total_length {
            return Err(AuraError::network("STUN response truncated".to_string()));
        }

        // Parse attributes
        let mut offset = 20;
        while offset + 4 <= expected_total_length {
            let attr_type = u16::from_be_bytes([response[offset], response[offset + 1]]);
            let attr_length =
                u16::from_be_bytes([response[offset + 2], response[offset + 3]]) as usize;

            offset += 4;

            if offset + attr_length > expected_total_length {
                break;
            }

            let attr_value = &response[offset..offset + attr_length];

            // Check for XOR-MAPPED-ADDRESS (preferred) or MAPPED-ADDRESS
            if attr_type == StunAttributeType::XorMappedAddress as u16 {
                return self.parse_xor_mapped_address(attr_value, transaction_id);
            } else if attr_type == StunAttributeType::MappedAddress as u16 {
                return self.parse_mapped_address(attr_value);
            }

            // Move to next attribute (pad to 4-byte boundary)
            offset += attr_length;
            offset = (offset + 3) & !3;
        }

        Err(AuraError::network(
            "No reflexive address found in STUN response".to_string(),
        ))
    }

    /// Parse XOR-MAPPED-ADDRESS attribute
    fn parse_xor_mapped_address(
        &self,
        attr_value: &[u8],
        transaction_id: [u8; 12],
    ) -> Result<SocketAddr, AuraError> {
        if attr_value.len() < 8 {
            return Err(AuraError::network(
                "XOR-MAPPED-ADDRESS too short".to_string(),
            ));
        }

        let family = u16::from_be_bytes([attr_value[1], attr_value[1]]) >> 8; // First byte is reserved
        let x_port = u16::from_be_bytes([attr_value[2], attr_value[3]]);

        // XOR port with magic cookie
        let port = x_port ^ (STUN_MAGIC_COOKIE as u16);

        match family {
            0x01 => {
                // IPv4
                if attr_value.len() < 8 {
                    return Err(AuraError::network(
                        "XOR-MAPPED-ADDRESS IPv4 too short".to_string(),
                    ));
                }

                let x_addr = [attr_value[4], attr_value[5], attr_value[6], attr_value[7]];
                let magic_bytes = STUN_MAGIC_COOKIE.to_be_bytes();

                // XOR address with magic cookie
                let addr = [
                    x_addr[0] ^ magic_bytes[0],
                    x_addr[1] ^ magic_bytes[1],
                    x_addr[2] ^ magic_bytes[2],
                    x_addr[3] ^ magic_bytes[3],
                ];

                Ok(SocketAddr::from((addr, port)))
            }
            0x02 => {
                // IPv6 - XOR with magic cookie + transaction ID
                if attr_value.len() < 20 {
                    return Err(AuraError::network(
                        "XOR-MAPPED-ADDRESS IPv6 too short".to_string(),
                    ));
                }

                let mut xor_key = Vec::new();
                xor_key.extend_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
                xor_key.extend_from_slice(&transaction_id);

                let mut addr = [0u8; 16];
                for i in 0..16 {
                    addr[i] = attr_value[4 + i] ^ xor_key[i % xor_key.len()];
                }

                Ok(SocketAddr::from((addr, port)))
            }
            _ => Err(AuraError::network(format!(
                "Unsupported address family: {}",
                family
            ))),
        }
    }

    /// Parse MAPPED-ADDRESS attribute (not XOR'd)
    fn parse_mapped_address(&self, attr_value: &[u8]) -> Result<SocketAddr, AuraError> {
        if attr_value.len() < 8 {
            return Err(AuraError::network("MAPPED-ADDRESS too short".to_string()));
        }

        let family = attr_value[1];
        let port = u16::from_be_bytes([attr_value[2], attr_value[3]]);

        match family {
            0x01 => {
                // IPv4
                if attr_value.len() < 8 {
                    return Err(AuraError::network(
                        "MAPPED-ADDRESS IPv4 too short".to_string(),
                    ));
                }

                let addr = [attr_value[4], attr_value[5], attr_value[6], attr_value[7]];
                Ok(SocketAddr::from((addr, port)))
            }
            0x02 => {
                // IPv6
                if attr_value.len() < 20 {
                    return Err(AuraError::network(
                        "MAPPED-ADDRESS IPv6 too short".to_string(),
                    ));
                }

                let mut addr = [0u8; 16];
                addr.copy_from_slice(&attr_value[4..20]);
                Ok(SocketAddr::from((addr, port)))
            }
            _ => Err(AuraError::network(format!(
                "Unsupported address family: {}",
                family
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn test_stun_config_default() {
        let config = StunConfig::default();
        assert_eq!(config.primary_server, "stun.l.google.com:19302");
        assert_eq!(config.timeout_ms, 3000);
        assert_eq!(config.retry_attempts, 3);
        assert!(!config.fallback_servers.is_empty());
    }

    #[test]
    fn test_stun_message_types() {
        assert_eq!(StunMessageType::BindingRequest as u16, 0x0001);
        assert_eq!(StunMessageType::BindingResponse as u16, 0x0101);
        assert_eq!(StunMessageType::BindingErrorResponse as u16, 0x0111);
    }

    #[test]
    fn test_stun_attribute_types() {
        assert_eq!(StunAttributeType::MappedAddress as u16, 0x0001);
        assert_eq!(StunAttributeType::XorMappedAddress as u16, 0x0020);
        assert_eq!(StunAttributeType::Fingerprint as u16, 0x8028);
    }

    #[test]
    fn test_create_binding_request() {
        let client = StunClient::new(StunConfig::default());
        let transaction_id = [0x12; 12];
        let packet = client.create_binding_request(transaction_id);

        // Check packet structure
        assert_eq!(packet.len(), 20); // STUN header only

        // Check message type (first 2 bytes)
        let msg_type = u16::from_be_bytes([packet[0], packet[1]]);
        assert_eq!(msg_type, StunMessageType::BindingRequest as u16);

        // Check message length (next 2 bytes, should be 0 for no attributes)
        let msg_length = u16::from_be_bytes([packet[2], packet[3]]);
        assert_eq!(msg_length, 0);

        // Check magic cookie (next 4 bytes)
        let magic = u32::from_be_bytes([packet[4], packet[5], packet[6], packet[7]]);
        assert_eq!(magic, STUN_MAGIC_COOKIE);

        // Check transaction ID (last 12 bytes)
        assert_eq!(&packet[8..20], &transaction_id);
    }

    #[test]
    fn test_parse_mapped_address_ipv4() {
        let client = StunClient::new(StunConfig::default());

        // Create IPv4 MAPPED-ADDRESS attribute value
        let mut attr_value = Vec::new();
        attr_value.push(0x00); // Reserved
        attr_value.push(0x01); // IPv4 family
        attr_value.extend_from_slice(&8080u16.to_be_bytes()); // Port
        attr_value.extend_from_slice(&[192, 168, 1, 100]); // IPv4 address

        let result = client.parse_mapped_address(&attr_value).unwrap();

        assert_eq!(result.ip(), IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)));
        assert_eq!(result.port(), 8080);
    }

    #[test]
    fn test_parse_xor_mapped_address_ipv4() {
        let client = StunClient::new(StunConfig::default());
        let transaction_id = [0x12; 12];

        // Original address: 192.168.1.100:8080
        let original_ip = [192, 168, 1, 100];
        let original_port = 8080u16;

        // XOR with magic cookie
        let magic_bytes = STUN_MAGIC_COOKIE.to_be_bytes();
        let xor_ip = [
            original_ip[0] ^ magic_bytes[0],
            original_ip[1] ^ magic_bytes[1],
            original_ip[2] ^ magic_bytes[2],
            original_ip[3] ^ magic_bytes[3],
        ];
        let xor_port = original_port ^ (STUN_MAGIC_COOKIE as u16);

        // Create XOR-MAPPED-ADDRESS attribute value
        let mut attr_value = Vec::new();
        attr_value.push(0x00); // Reserved
        attr_value.push(0x01); // IPv4 family
        attr_value.extend_from_slice(&xor_port.to_be_bytes()); // XOR'd port
        attr_value.extend_from_slice(&xor_ip); // XOR'd IPv4 address

        let result = client
            .parse_xor_mapped_address(&attr_value, transaction_id)
            .unwrap();

        assert_eq!(result.ip(), IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)));
        assert_eq!(result.port(), 8080);
    }

    #[test]
    fn test_parse_mapped_address_invalid() {
        let client = StunClient::new(StunConfig::default());

        // Too short
        let short_attr = vec![0x00, 0x01];
        assert!(client.parse_mapped_address(&short_attr).is_err());

        // Unknown family
        let unknown_family = vec![0x00, 0x99, 0x1f, 0x90, 192, 168, 1, 100];
        assert!(client.parse_mapped_address(&unknown_family).is_err());
    }

    #[tokio::test]
    async fn test_client_creation() {
        let config = StunConfig::default();
        let client = StunClient::new(config.clone());
        assert_eq!(client.config.primary_server, config.primary_server);
    }

    #[tokio::test]
    async fn test_resolve_server_address_with_port() {
        let client = StunClient::new(StunConfig::default());

        // Test with localhost (should resolve)
        let result = client.resolve_server_address("127.0.0.1:19302").await;
        assert!(result.is_ok());
        let addr = result.unwrap();
        assert_eq!(addr.port(), 19302);
        assert_eq!(addr.ip(), IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
    }

    #[tokio::test]
    async fn test_resolve_server_address_invalid() {
        let client = StunClient::new(StunConfig::default());

        // Invalid address should fail
        let result = client.resolve_server_address("invalid.address:99999").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_local_socket() {
        let config = StunConfig::default();
        let client = StunClient::new(config);

        let socket = client.create_local_socket().await;
        assert!(socket.is_ok());

        let socket = socket.unwrap();
        let local_addr = socket.local_addr().unwrap();
        assert_eq!(local_addr.ip(), IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)));
        assert!(local_addr.port() > 0); // Should get ephemeral port
    }

    #[tokio::test]
    async fn test_discover_with_unreachable_server() {
        let client = StunClient::new(StunConfig {
            timeout_ms: 100,   // Very short timeout
            retry_attempts: 1, // Single attempt
            ..StunConfig::default()
        });

        // Use unreachable address
        let result = client.discover_with_server("192.0.2.1:19302").await; // RFC 5737 test address
        assert!(result.is_err());
    }

    #[test]
    fn test_stun_result_creation() {
        let reflexive_addr = "203.0.113.42:12345".parse().unwrap();
        let local_addr = "192.168.1.100:54321".parse().unwrap();

        let result = StunResult {
            reflexive_address: reflexive_addr,
            local_address: local_addr,
            stun_server: "stun.example.com:19302".to_string(),
            discovered_at: 1234567890,
        };

        assert_eq!(result.reflexive_address, reflexive_addr);
        assert_eq!(result.local_address, local_addr);
        assert_eq!(result.stun_server, "stun.example.com:19302");
        assert_eq!(result.discovered_at, 1234567890);
    }
}
