//! Protocol Version Constants and Capabilities
//!
//! Centralized version constants and capability tracking for protocol negotiation.
//! All protocol handlers should use these constants rather than hardcoding versions.
//!
//! # Version Policy
//!
//! - **Major version bump**: Breaking changes requiring migration
//! - **Minor version bump**: New features, backwards compatible
//! - **Patch version bump**: Bug fixes, fully compatible
//!
//! When negotiating versions, peers select the highest mutually-supported version.
//! If no compatible version exists, the handshake fails with diagnostic info.

use crate::messages::SemanticVersion;
use serde::{Deserialize, Serialize};

/// Minimum protocol version that this node supports.
///
/// Peers running versions below this will be rejected during handshake.
/// This should be updated when deprecating old protocol versions.
pub const MIN_SUPPORTED_VERSION: SemanticVersion = SemanticVersion::new(1, 0, 0);

/// Current protocol version implemented by this node.
///
/// This is the version advertised during handshakes and represents
/// the full feature set available.
pub const CURRENT_VERSION: SemanticVersion = SemanticVersion::new(1, 0, 0);

/// Individual protocol capability with version requirement.
///
/// Capabilities are feature flags that can be negotiated between peers.
/// Each capability has a minimum version requirement.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProtocolCapability {
    /// Capability identifier (e.g., "ceremony_supersession", "fact_journal")
    pub name: String,

    /// Minimum version that supports this capability
    pub min_version: SemanticVersion,

    /// Human-readable description
    pub description: String,
}

impl ProtocolCapability {
    /// Create a new protocol capability.
    pub fn new(name: &str, min_version: SemanticVersion) -> Self {
        Self {
            name: name.to_string(),
            min_version,
            description: String::new(),
        }
    }
}

/// Collection of protocol capabilities with version requirements.
///
/// This struct tracks which features are available at which protocol versions,
/// enabling feature negotiation during handshakes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolCapabilities {
    /// Minimum version for ceremony supersession support
    pub ceremony_supersession: SemanticVersion,

    /// Minimum version for version handshake protocol
    pub version_handshake: SemanticVersion,

    /// Minimum version for fact-based journal system
    pub fact_journal: SemanticVersion,

    /// Minimum version for leaderless consensus fallback
    pub leaderless_consensus: SemanticVersion,

    /// Minimum version for FROST threshold signatures
    pub frost_signatures: SemanticVersion,
}

impl ProtocolCapabilities {
    /// Current capability set for this protocol version.
    ///
    /// All capabilities are available from version 1.0.0 in the initial release.
    /// When adding new capabilities, set their min_version to the version that
    /// introduced them.
    pub const CURRENT: Self = Self {
        ceremony_supersession: SemanticVersion::new(1, 0, 0),
        version_handshake: SemanticVersion::new(1, 0, 0),
        fact_journal: SemanticVersion::new(1, 0, 0),
        leaderless_consensus: SemanticVersion::new(1, 0, 0),
        frost_signatures: SemanticVersion::new(1, 0, 0),
    };

    /// Check if a capability is supported at a given version.
    pub fn is_supported(&self, capability: &str, version: &SemanticVersion) -> bool {
        let min_version = match capability {
            "ceremony_supersession" => &self.ceremony_supersession,
            "version_handshake" => &self.version_handshake,
            "fact_journal" => &self.fact_journal,
            "leaderless_consensus" => &self.leaderless_consensus,
            "frost_signatures" => &self.frost_signatures,
            _ => return false, // Unknown capability
        };

        version >= min_version
    }

    /// Get list of capabilities supported at a given version.
    pub fn supported_at(&self, version: &SemanticVersion) -> Vec<&'static str> {
        let mut supported = Vec::new();

        if version >= &self.ceremony_supersession {
            supported.push("ceremony_supersession");
        }
        if version >= &self.version_handshake {
            supported.push("version_handshake");
        }
        if version >= &self.fact_journal {
            supported.push("fact_journal");
        }
        if version >= &self.leaderless_consensus {
            supported.push("leaderless_consensus");
        }
        if version >= &self.frost_signatures {
            supported.push("frost_signatures");
        }

        supported
    }

    /// Get list of all known capability names.
    pub fn all_capability_names() -> &'static [&'static str] {
        &[
            "ceremony_supersession",
            "version_handshake",
            "fact_journal",
            "leaderless_consensus",
            "frost_signatures",
        ]
    }
}

impl Default for ProtocolCapabilities {
    fn default() -> Self {
        Self::CURRENT
    }
}

/// Version compatibility result from negotiation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionCompatibility {
    /// Versions are fully compatible, use negotiated version
    Compatible {
        /// The negotiated version to use
        negotiated: SemanticVersion,
        /// Capabilities available at the negotiated version
        capabilities: Vec<String>,
    },

    /// Versions are incompatible
    Incompatible {
        /// Reason for incompatibility
        reason: String,
        /// Local version for diagnostics
        local_version: SemanticVersion,
        /// Remote version for diagnostics
        remote_version: SemanticVersion,
    },
}

/// Check version compatibility between local and remote versions.
///
/// Returns the negotiated version if compatible, or error details if not.
pub fn check_compatibility(
    local: &SemanticVersion,
    local_min: &SemanticVersion,
    remote: &SemanticVersion,
    remote_min: &SemanticVersion,
) -> VersionCompatibility {
    // Try to negotiate a common version
    if let Some(negotiated) = local.negotiate_with(remote) {
        // Check if negotiated version meets both minimum requirements
        if &negotiated >= local_min && &negotiated >= remote_min {
            let capabilities = ProtocolCapabilities::CURRENT
                .supported_at(&negotiated)
                .into_iter()
                .map(String::from)
                .collect();

            return VersionCompatibility::Compatible {
                negotiated,
                capabilities,
            };
        }
    }

    // Incompatible
    VersionCompatibility::Incompatible {
        reason: format!(
            "No compatible version: local {local} (min {local_min}), remote {remote} (min {remote_min})"
        ),
        local_version: *local,
        remote_version: *remote,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_constants() {
        assert!(CURRENT_VERSION >= MIN_SUPPORTED_VERSION);
    }

    #[test]
    fn test_capability_check() {
        let caps = ProtocolCapabilities::CURRENT;
        let v1_0_0 = SemanticVersion::new(1, 0, 0);
        let v0_9_0 = SemanticVersion::new(0, 9, 0);

        assert!(caps.is_supported("ceremony_supersession", &v1_0_0));
        assert!(caps.is_supported("fact_journal", &v1_0_0));
        assert!(!caps.is_supported("ceremony_supersession", &v0_9_0));
        assert!(!caps.is_supported("unknown_capability", &v1_0_0));
    }

    #[test]
    fn test_supported_at() {
        let caps = ProtocolCapabilities::CURRENT;
        let v1_0_0 = SemanticVersion::new(1, 0, 0);

        let supported = caps.supported_at(&v1_0_0);
        assert!(supported.contains(&"ceremony_supersession"));
        assert!(supported.contains(&"version_handshake"));
        assert!(supported.contains(&"fact_journal"));
    }

    #[test]
    fn test_compatibility_check_compatible() {
        let v1_0_0 = SemanticVersion::new(1, 0, 0);
        let v1_1_0 = SemanticVersion::new(1, 1, 0);

        let result = check_compatibility(&v1_1_0, &v1_0_0, &v1_0_0, &v1_0_0);

        match result {
            VersionCompatibility::Compatible { negotiated, .. } => {
                assert_eq!(negotiated, v1_0_0);
            }
            _ => panic!("Expected compatible"),
        }
    }

    #[test]
    fn test_compatibility_check_incompatible() {
        let v1_0_0 = SemanticVersion::new(1, 0, 0);
        let v2_0_0 = SemanticVersion::new(2, 0, 0);

        let result = check_compatibility(&v2_0_0, &v2_0_0, &v1_0_0, &v1_0_0);

        match result {
            VersionCompatibility::Incompatible { reason, .. } => {
                assert!(reason.contains("No compatible version"));
            }
            _ => panic!("Expected incompatible"),
        }
    }
}
