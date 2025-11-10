//! STUN client stubs used until the real transport stack lands.

use aura_core::AuraError;
use std::net::SocketAddr;

/// Configuration for the stubbed STUN client.
#[derive(Debug, Clone)]
pub struct StunConfig {
    /// Primary STUN server URL (unused in stub).
    pub primary_server: String,
    /// Additional STUN servers (unused in stub).
    pub fallback_servers: Vec<String>,
    /// Request timeout in milliseconds.
    pub timeout_ms: u64,
    /// Number of retry attempts.
    pub retry_attempts: u32,
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
        }
    }
}

/// Placeholder STUN discovery result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StunResult {
    /// Reflexive address discovered (unused in stub).
    pub reflexive_address: SocketAddr,
    /// Local socket address (unused in stub).
    pub local_address: SocketAddr,
    /// Server that produced the result.
    pub stun_server: String,
    /// Timestamp when the result was produced.
    pub discovered_at: u64,
}

/// Stub client that logs intent but never performs real STUN work.
pub struct StunClient {
    config: StunConfig,
}

impl StunClient {
    /// Create a new client with the provided configuration.
    pub fn new(config: StunConfig) -> Self {
        Self { config }
    }

    /// Attempt to discover a reflexive address. The stub always returns `Ok(None)`.
    pub async fn discover_reflexive_address(&self) -> Result<Option<StunResult>, AuraError> {
        tracing::warn!(
            primary = %self.config.primary_server,
            "STUN discovery is not available in the current build; returning no result"
        );
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn discovery_stub_returns_none() {
        let client = StunClient::new(StunConfig::default());
        let result = client
            .discover_reflexive_address()
            .await
            .expect("stub discovery should succeed");
        assert!(result.is_none());
    }
}
