//! Protocol Version Handshake Handler
//!
//! Implements version negotiation between peers during connection establishment.
//! Ensures peers can communicate safely by:
//! 1. Exchanging version information and capabilities
//! 2. Negotiating a mutually supported version
//! 3. Gracefully rejecting incompatible peers with diagnostic info
//!
//! # Protocol Flow
//!
//! ```text
//! Initiator                    Responder
//!    |                            |
//!    |-- VersionHandshakeRequest -->
//!    |                            |
//!    |<-- VersionHandshakeResponse -|
//!    |                            |
//! [Use negotiated version or disconnect]
//! ```
//!
//! # Version Compatibility
//!
//! Compatibility is determined by `SemanticVersion::negotiate_with()`:
//! - Major versions must match for communication
//! - Minor/patch differences are tolerated, using the lower version
//! - Peers advertise their minimum supported version

use aura_core::protocol::{
    check_compatibility, ProtocolCapabilities, VersionCompatibility, CURRENT_VERSION,
    MIN_SUPPORTED_VERSION,
};
use aura_core::SemanticVersion;
use serde::{Deserialize, Serialize};

/// Request for version handshake.
///
/// Sent by the initiating peer to begin version negotiation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionHandshakeRequest {
    /// The peer's current protocol version
    pub version: SemanticVersion,

    /// Minimum version the peer supports
    pub min_version: SemanticVersion,

    /// Capabilities the peer supports at this version
    pub capabilities: Vec<String>,

    /// Random nonce for replay protection
    pub nonce: [u8; 32],
}

impl VersionHandshakeRequest {
    /// Create a new handshake request with current version info.
    pub fn new(nonce: [u8; 32]) -> Self {
        Self {
            version: CURRENT_VERSION,
            min_version: MIN_SUPPORTED_VERSION,
            capabilities: ProtocolCapabilities::CURRENT
                .supported_at(&CURRENT_VERSION)
                .into_iter()
                .map(String::from)
                .collect(),
            nonce,
        }
    }

    /// Create a request with custom version info (for testing).
    pub fn with_version(
        version: SemanticVersion,
        min_version: SemanticVersion,
        nonce: [u8; 32],
    ) -> Self {
        Self {
            version,
            min_version,
            capabilities: ProtocolCapabilities::CURRENT
                .supported_at(&version)
                .into_iter()
                .map(String::from)
                .collect(),
            nonce,
        }
    }
}

/// Response to version handshake.
///
/// Indicates whether versions are compatible and provides diagnostics if not.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VersionHandshakeResponse {
    /// Versions are compatible, proceed with negotiated version
    Accepted {
        /// The negotiated version to use for communication
        negotiated_version: SemanticVersion,
        /// Capabilities available at the negotiated version
        capabilities: Vec<String>,
        /// Echo of the request nonce for verification
        nonce: [u8; 32],
    },

    /// Versions are incompatible, connection should be terminated
    Rejected {
        /// Human-readable reason for rejection
        reason: String,
        /// This peer's version for diagnostics
        receiver_version: SemanticVersion,
        /// Optional URL for upgrade instructions
        upgrade_url: Option<String>,
    },
}

impl VersionHandshakeResponse {
    /// Create an accepted response.
    pub fn accepted(
        negotiated_version: SemanticVersion,
        capabilities: Vec<String>,
        nonce: [u8; 32],
    ) -> Self {
        Self::Accepted {
            negotiated_version,
            capabilities,
            nonce,
        }
    }

    /// Create a rejected response.
    pub fn rejected(reason: String, receiver_version: SemanticVersion) -> Self {
        Self::Rejected {
            reason,
            receiver_version,
            upgrade_url: None,
        }
    }

    /// Create a rejected response with upgrade URL.
    pub fn rejected_with_upgrade(
        reason: String,
        receiver_version: SemanticVersion,
        upgrade_url: String,
    ) -> Self {
        Self::Rejected {
            reason,
            receiver_version,
            upgrade_url: Some(upgrade_url),
        }
    }

    /// Check if the response indicates acceptance.
    pub fn is_accepted(&self) -> bool {
        matches!(self, Self::Accepted { .. })
    }

    /// Get the negotiated version if accepted.
    pub fn negotiated_version(&self) -> Option<SemanticVersion> {
        match self {
            Self::Accepted {
                negotiated_version, ..
            } => Some(*negotiated_version),
            Self::Rejected { .. } => None,
        }
    }
}

/// Version handshake handler.
///
/// Stateless handler for processing version negotiation requests.
#[derive(Debug, Clone, Default)]
pub struct VersionHandshake {
    /// Optional upgrade URL to provide in rejection messages
    upgrade_url: Option<String>,
}

impl VersionHandshake {
    /// Create a new version handshake handler.
    pub fn new() -> Self {
        Self { upgrade_url: None }
    }

    /// Create a handler with a custom upgrade URL.
    pub fn with_upgrade_url(upgrade_url: String) -> Self {
        Self {
            upgrade_url: Some(upgrade_url),
        }
    }

    /// Create a handshake request.
    pub fn create_request(&self, nonce: [u8; 32]) -> VersionHandshakeRequest {
        VersionHandshakeRequest::new(nonce)
    }

    /// Process a handshake request and produce a response.
    ///
    /// Uses the version compatibility check to determine if communication is possible.
    pub fn process_request(&self, request: &VersionHandshakeRequest) -> VersionHandshakeResponse {
        let compatibility = check_compatibility(
            &CURRENT_VERSION,
            &MIN_SUPPORTED_VERSION,
            &request.version,
            &request.min_version,
        );

        match compatibility {
            VersionCompatibility::Compatible {
                negotiated,
                capabilities,
            } => VersionHandshakeResponse::Accepted {
                negotiated_version: negotiated,
                capabilities,
                nonce: request.nonce,
            },
            VersionCompatibility::Incompatible {
                reason,
                local_version: _,
                remote_version: _,
            } => {
                if let Some(ref url) = self.upgrade_url {
                    VersionHandshakeResponse::rejected_with_upgrade(
                        reason,
                        CURRENT_VERSION,
                        url.clone(),
                    )
                } else {
                    VersionHandshakeResponse::rejected(reason, CURRENT_VERSION)
                }
            }
        }
    }

    /// Validate a response matches the original request.
    ///
    /// Checks that the nonce echoed back matches the request nonce.
    pub fn validate_response(
        &self,
        request: &VersionHandshakeRequest,
        response: &VersionHandshakeResponse,
    ) -> Result<SemanticVersion, String> {
        match response {
            VersionHandshakeResponse::Accepted {
                negotiated_version,
                nonce,
                ..
            } => {
                if nonce != &request.nonce {
                    return Err("Nonce mismatch - possible replay attack".to_string());
                }
                Ok(*negotiated_version)
            }
            VersionHandshakeResponse::Rejected {
                reason,
                receiver_version,
                upgrade_url,
            } => {
                let mut msg =
                    format!("Version rejected: {reason} (peer version: {receiver_version})");
                if let Some(url) = upgrade_url {
                    msg.push_str(&format!(". Upgrade at: {url}"));
                }
                Err(msg)
            }
        }
    }
}

/// Peer version information recorded after successful handshake.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerVersionInfo {
    /// The negotiated version for communication
    pub negotiated_version: SemanticVersion,

    /// Capabilities available with this peer
    pub capabilities: Vec<String>,

    /// When the handshake was completed (Unix timestamp ms)
    pub handshake_time_ms: u64,
}

impl PeerVersionInfo {
    /// Check if a specific capability is available.
    pub fn has_capability(&self, capability: &str) -> bool {
        self.capabilities.iter().any(|c| c == capability)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_request() {
        let handler = VersionHandshake::new();
        let nonce = [1u8; 32];
        let request = handler.create_request(nonce);

        assert_eq!(request.version, CURRENT_VERSION);
        assert_eq!(request.min_version, MIN_SUPPORTED_VERSION);
        assert_eq!(request.nonce, nonce);
        assert!(!request.capabilities.is_empty());
    }

    #[test]
    fn test_compatible_handshake() {
        let handler = VersionHandshake::new();
        let nonce = [2u8; 32];
        let request = VersionHandshakeRequest::new(nonce);
        let response = handler.process_request(&request);

        assert!(response.is_accepted());
        assert!(response.negotiated_version().is_some());
    }

    #[test]
    fn test_incompatible_handshake() {
        let handler = VersionHandshake::new();
        let nonce = [3u8; 32];

        // Create a request with incompatible version
        let request = VersionHandshakeRequest::with_version(
            SemanticVersion::new(99, 0, 0), // Future major version
            SemanticVersion::new(99, 0, 0),
            nonce,
        );

        let response = handler.process_request(&request);
        assert!(!response.is_accepted());
    }

    #[test]
    fn test_validate_response_nonce_mismatch() {
        let handler = VersionHandshake::new();
        let request_nonce = [4u8; 32];
        let wrong_nonce = [5u8; 32];

        let request = VersionHandshakeRequest::new(request_nonce);
        let response = VersionHandshakeResponse::Accepted {
            negotiated_version: CURRENT_VERSION,
            capabilities: vec![],
            nonce: wrong_nonce, // Wrong nonce
        };

        let result = handler.validate_response(&request, &response);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Nonce mismatch"));
    }

    #[test]
    fn test_validate_response_success() {
        let handler = VersionHandshake::new();
        let nonce = [6u8; 32];

        let request = VersionHandshakeRequest::new(nonce);
        let response = VersionHandshakeResponse::Accepted {
            negotiated_version: CURRENT_VERSION,
            capabilities: vec!["test".to_string()],
            nonce,
        };

        let result = handler.validate_response(&request, &response);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), CURRENT_VERSION);
    }

    #[test]
    fn test_rejected_with_upgrade_url() {
        let handler = VersionHandshake::with_upgrade_url("https://example.com/upgrade".to_string());
        let nonce = [7u8; 32];

        // Force incompatible request
        let request = VersionHandshakeRequest::with_version(
            SemanticVersion::new(99, 0, 0),
            SemanticVersion::new(99, 0, 0),
            nonce,
        );

        let response = handler.process_request(&request);

        match response {
            VersionHandshakeResponse::Rejected { upgrade_url, .. } => {
                assert!(upgrade_url.is_some());
                assert_eq!(upgrade_url.unwrap(), "https://example.com/upgrade");
            }
            _ => panic!("Expected rejection"),
        }
    }

    #[test]
    fn test_peer_version_info_capability_check() {
        let info = PeerVersionInfo {
            negotiated_version: SemanticVersion::new(1, 0, 0),
            capabilities: vec![
                "ceremony_supersession".to_string(),
                "fact_journal".to_string(),
            ],
            handshake_time_ms: 12345,
        };

        assert!(info.has_capability("ceremony_supersession"));
        assert!(info.has_capability("fact_journal"));
        assert!(!info.has_capability("unknown_capability"));
    }
}
