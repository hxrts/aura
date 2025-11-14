//! Comprehensive Unit Tests for Peer Management
//!
//! Tests privacy-preserving peer management including capability blinding,
//! relationship-scoped discovery, and privacy-aware selection.

use super::{
    info::{PeerInfo, BlindedPeerCapabilities, ScopedPeerMetrics},
    selection::{PrivacyAwareSelectionCriteria, RelationshipScopedDiscovery},
};
use crate::types::{PrivacyLevel, TransportConfig};
use aura_core::{DeviceId, ContextId};
use std::collections::HashMap;

#[cfg(test)]
mod peer_info_tests {
    use super::*;

    #[test]
    fn test_peer_info_creation() {
        let device_id = DeviceId::new();
        let capabilities = vec!["transport".to_string(), "messaging".to_string()];
        
        let peer = PeerInfo::new(device_id, "test-peer".to_string(), capabilities.clone());
        
        assert_eq!(peer.device_id(), device_id);
        assert_eq!(peer.display_name(), "test-peer");
        assert_eq!(peer.capabilities(), &capabilities);
        assert!(!peer.is_capability_blinded());
    }

    #[test]
    fn test_blinded_peer_info() {
        let device_id = DeviceId::new();
        let capabilities = vec!["secret_cap".to_string(), "public_cap".to_string()];
        
        let peer = PeerInfo::new_blinded(device_id, "blinded-peer".to_string(), capabilities);
        
        assert_eq!(peer.device_id(), device_id);
        assert_eq!(peer.display_name(), "blinded-peer");
        assert!(peer.is_capability_blinded());
        
        // Should be able to check for capabilities without revealing full list
        assert!(peer.has_capability_blinded("secret_cap"));
        assert!(peer.has_capability_blinded("public_cap"));
        assert!(!peer.has_capability_blinded("nonexistent"));
        
        // Public capabilities should be filtered/blinded
        let public_caps = peer.capabilities_public();
        assert!(!public_caps.is_empty()); // Some capabilities should be visible
    }

    #[test]
    fn test_peer_capability_queries() {
        let device_id = DeviceId::new();
        let capabilities = vec![
            "transport_basic".to_string(),
            "transport_secure".to_string(), 
            "messaging".to_string(),
            "file_transfer".to_string(),
        ];
        
        let peer = PeerInfo::new(device_id, "test-peer".to_string(), capabilities);
        
        // Test direct capability checks
        assert!(peer.has_capability("transport_basic"));
        assert!(peer.has_capability("messaging"));
        assert!(!peer.has_capability("nonexistent"));
        
        // Test capability prefix matching
        assert!(peer.has_capability_prefix("transport"));
        assert!(peer.has_capability_prefix("transport_"));
        assert!(!peer.has_capability_prefix("video"));
        
        // Test multiple capability requirements
        let required = vec!["transport_basic".to_string(), "messaging".to_string()];
        assert!(peer.has_capabilities(&required));
        
        let missing_required = vec!["transport_basic".to_string(), "nonexistent".to_string()];
        assert!(!peer.has_capabilities(&missing_required));
    }

    #[test]
    fn test_peer_metrics() {
        let device_id = DeviceId::new();
        let context = ContextId::new();
        
        let mut peer = PeerInfo::new(device_id, "metrics-peer".to_string(), vec!["test".to_string()]);
        
        // Test metrics recording
        peer.record_successful_connection(context);
        peer.record_failed_connection(context);
        peer.update_latency(context, std::time::Duration::from_millis(50));
        
        let metrics = peer.scoped_metrics(context);
        assert!(metrics.connection_success_rate() < 1.0); // Had one failure
        assert!(metrics.average_latency().is_some());
        assert_eq!(metrics.average_latency().unwrap().as_millis(), 50);
    }

    #[test]
    fn test_peer_serialization() {
        let device_id = DeviceId::new();
        let capabilities = vec!["cap1".to_string(), "cap2".to_string()];
        
        let original = PeerInfo::new_blinded(device_id, "serializable-peer".to_string(), capabilities);
        
        // JSON serialization roundtrip
        let json = serde_json::to_string(&original).expect("Serialization failed");
        let deserialized: PeerInfo = serde_json::from_str(&json)
            .expect("Deserialization failed");
        
        assert_eq!(original.device_id(), deserialized.device_id());
        assert_eq!(original.display_name(), deserialized.display_name());
        assert_eq!(original.is_capability_blinded(), deserialized.is_capability_blinded());
        
        // Capability queries should work the same
        assert_eq!(
            original.has_capability_blinded("cap1"),
            deserialized.has_capability_blinded("cap1")
        );
    }
}

#[cfg(test)]
mod selection_criteria_tests {
    use super::*;

    #[test]
    fn test_basic_selection_criteria() {
        let criteria = PrivacyAwareSelectionCriteria {
            required_capabilities: vec!["transport".to_string(), "secure".to_string()],
            privacy_level: PrivacyLevel::Blinded,
            relationship_scope: None,
            max_capability_disclosure: 3,
            require_capability_proofs: false,
        };
        
        // Create test peers
        let matching_peer = PeerInfo::new(
            DeviceId::new(),
            "matching".to_string(),
            vec!["transport".to_string(), "secure".to_string(), "extra".to_string()],
        );
        
        let non_matching_peer = PeerInfo::new(
            DeviceId::new(), 
            "non-matching".to_string(),
            vec!["transport".to_string()], // Missing "secure"
        );
        
        // Test selection
        assert!(criteria.matches_peer(&matching_peer));
        assert!(!criteria.matches_peer(&non_matching_peer));
    }

    #[test]
    fn test_relationship_scoped_selection() {
        let context = ContextId::new();
        let criteria = PrivacyAwareSelectionCriteria {
            required_capabilities: vec!["messaging".to_string()],
            privacy_level: PrivacyLevel::RelationshipScoped,
            relationship_scope: Some(context),
            max_capability_disclosure: 2,
            require_capability_proofs: true,
        };
        
        let peer = PeerInfo::new(
            DeviceId::new(),
            "scoped-peer".to_string(),
            vec!["messaging".to_string(), "file_transfer".to_string()],
        );
        
        // Should match when in the right relationship scope
        assert!(criteria.matches_peer(&peer));
        assert_eq!(criteria.privacy_level, PrivacyLevel::RelationshipScoped);
        assert_eq!(criteria.relationship_scope.unwrap(), context);
    }

    #[test]
    fn test_capability_disclosure_limits() {
        let criteria = PrivacyAwareSelectionCriteria {
            required_capabilities: vec!["basic".to_string()],
            privacy_level: PrivacyLevel::Blinded,
            relationship_scope: None,
            max_capability_disclosure: 2, // Limit to 2 capabilities
            require_capability_proofs: false,
        };
        
        let peer_many_caps = PeerInfo::new(
            DeviceId::new(),
            "many-caps".to_string(),
            vec![
                "basic".to_string(),
                "advanced".to_string(), 
                "expert".to_string(),
                "secret".to_string(),
            ],
        );
        
        // Should match but limit capability disclosure
        assert!(criteria.matches_peer(&peer_many_caps));
        
        let disclosed_caps = criteria.get_disclosed_capabilities(&peer_many_caps);
        assert!(disclosed_caps.len() <= criteria.max_capability_disclosure);
        assert!(disclosed_caps.contains(&"basic".to_string())); // Required cap always disclosed
    }

    #[test] 
    fn test_privacy_aware_selection_with_proofs() {
        let criteria = PrivacyAwareSelectionCriteria {
            required_capabilities: vec!["verified_transport".to_string()],
            privacy_level: PrivacyLevel::RelationshipScoped,
            relationship_scope: Some(ContextId::new()),
            max_capability_disclosure: 1,
            require_capability_proofs: true,
        };
        
        let peer_with_proofs = PeerInfo::new_with_proofs(
            DeviceId::new(),
            "proven-peer".to_string(),
            vec!["verified_transport".to_string()],
            HashMap::from([("verified_transport".to_string(), vec![0x01, 0x02, 0x03])]),
        );
        
        let peer_without_proofs = PeerInfo::new(
            DeviceId::new(),
            "unproven-peer".to_string(),
            vec!["verified_transport".to_string()],
        );
        
        // Should only match peer with capability proofs when required
        assert!(criteria.matches_peer(&peer_with_proofs));
        assert!(!criteria.matches_peer(&peer_without_proofs));
    }
}

#[cfg(test)]
mod discovery_tests {
    use super::*;

    #[test]
    fn test_relationship_scoped_discovery() {
        let context1 = ContextId::new();
        let context2 = ContextId::new();
        
        let mut discovery = RelationshipScopedDiscovery::new();
        
        // Add peers to different relationship contexts
        let peer1 = PeerInfo::new(
            DeviceId::new(),
            "family-peer".to_string(),
            vec!["messaging".to_string()],
        );
        
        let peer2 = PeerInfo::new(
            DeviceId::new(),
            "work-peer".to_string(),
            vec!["messaging".to_string(), "file_sharing".to_string()],
        );
        
        discovery.add_peer_to_context(context1, peer1.clone());
        discovery.add_peer_to_context(context2, peer2.clone());
        
        // Discovery should be scoped to relationship context
        let family_peers = discovery.discover_peers_in_context(context1);
        let work_peers = discovery.discover_peers_in_context(context2);
        
        assert_eq!(family_peers.len(), 1);
        assert_eq!(work_peers.len(), 1);
        assert_eq!(family_peers[0].device_id(), peer1.device_id());
        assert_eq!(work_peers[0].device_id(), peer2.device_id());
        
        // Peers should not cross relationship boundaries
        assert_ne!(family_peers[0].device_id(), work_peers[0].device_id());
    }

    #[test]
    fn test_discovery_with_selection_criteria() {
        let context = ContextId::new();
        let mut discovery = RelationshipScopedDiscovery::new();
        
        // Add various peers
        let peers = vec![
            PeerInfo::new(
                DeviceId::new(),
                "basic-peer".to_string(),
                vec!["transport".to_string()],
            ),
            PeerInfo::new(
                DeviceId::new(),
                "secure-peer".to_string(),
                vec!["transport".to_string(), "secure".to_string()],
            ),
            PeerInfo::new(
                DeviceId::new(),
                "advanced-peer".to_string(),
                vec!["transport".to_string(), "secure".to_string(), "advanced".to_string()],
            ),
        ];
        
        for peer in peers {
            discovery.add_peer_to_context(context, peer);
        }
        
        // Test selection with criteria
        let secure_criteria = PrivacyAwareSelectionCriteria {
            required_capabilities: vec!["transport".to_string(), "secure".to_string()],
            privacy_level: PrivacyLevel::RelationshipScoped,
            relationship_scope: Some(context),
            max_capability_disclosure: 3,
            require_capability_proofs: false,
        };
        
        let secure_peers = discovery.discover_peers_matching(context, &secure_criteria);
        assert_eq!(secure_peers.len(), 2); // Should find secure-peer and advanced-peer
        
        for peer in &secure_peers {
            assert!(peer.has_capability("secure"));
            assert!(peer.has_capability("transport"));
        }
    }

    #[test]
    fn test_discovery_privacy_isolation() {
        let mut discovery = RelationshipScopedDiscovery::new();
        
        // Create multiple isolated relationship contexts
        let mut contexts = Vec::new();
        for i in 0..5 {
            let context = ContextId::new();
            let peer = PeerInfo::new(
                DeviceId::new(),
                format!("peer-{}", i),
                vec![format!("capability-{}", i)],
            );
            
            discovery.add_peer_to_context(context, peer);
            contexts.push(context);
        }
        
        // Each context should only see its own peers
        for (i, context) in contexts.iter().enumerate() {
            let peers = discovery.discover_peers_in_context(*context);
            assert_eq!(peers.len(), 1);
            assert_eq!(peers[0].display_name(), format!("peer-{}", i));
            
            // Verify isolation - no peer should have capabilities from other contexts
            assert!(peers[0].has_capability(&format!("capability-{}", i)));
            for j in 0..5 {
                if i != j {
                    assert!(!peers[0].has_capability(&format!("capability-{}", j)));
                }
            }
        }
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;

    #[test]
    fn test_capability_blinding_preserves_functionality() {
        let device_id = DeviceId::new();
        let capabilities = vec![
            "transport".to_string(),
            "secure_messaging".to_string(),
            "file_transfer".to_string(),
            "voice_call".to_string(),
        ];
        
        // Create both blinded and non-blinded versions
        let clear_peer = PeerInfo::new(device_id, "clear-peer".to_string(), capabilities.clone());
        let blinded_peer = PeerInfo::new_blinded(device_id, "blinded-peer".to_string(), capabilities);
        
        // Both should support the same capability queries
        for capability in ["transport", "secure_messaging", "nonexistent"] {
            assert_eq!(
                clear_peer.has_capability(capability),
                blinded_peer.has_capability_blinded(capability),
                "Capability check mismatch for: {}", capability
            );
        }
        
        // Blinded peer should hide implementation details
        assert!(!clear_peer.is_capability_blinded());
        assert!(blinded_peer.is_capability_blinded());
        assert!(blinded_peer.capabilities_public().len() <= clear_peer.capabilities().len());
    }

    #[test]
    fn test_selection_criteria_consistency() {
        let context = ContextId::new();
        let base_criteria = PrivacyAwareSelectionCriteria {
            required_capabilities: vec!["basic".to_string()],
            privacy_level: PrivacyLevel::Blinded,
            relationship_scope: Some(context),
            max_capability_disclosure: 2,
            require_capability_proofs: false,
        };
        
        let test_peer = PeerInfo::new(
            DeviceId::new(),
            "test-peer".to_string(),
            vec!["basic".to_string(), "extra".to_string()],
        );
        
        // Criteria matching should be deterministic
        for _ in 0..10 {
            assert!(base_criteria.matches_peer(&test_peer));
        }
        
        // Capability disclosure should be consistent
        let disclosed1 = base_criteria.get_disclosed_capabilities(&test_peer);
        let disclosed2 = base_criteria.get_disclosed_capabilities(&test_peer);
        assert_eq!(disclosed1, disclosed2);
    }

    #[test]
    fn test_relationship_scope_isolation_property() {
        let mut discovery = RelationshipScopedDiscovery::new();
        
        // Same peer in different relationship contexts should be isolated
        let peer_id = DeviceId::new();
        let peer1 = PeerInfo::new(peer_id, "context1-peer".to_string(), vec!["cap1".to_string()]);
        let peer2 = PeerInfo::new(peer_id, "context2-peer".to_string(), vec!["cap2".to_string()]);
        
        let context1 = ContextId::new();
        let context2 = ContextId::new();
        
        discovery.add_peer_to_context(context1, peer1);
        discovery.add_peer_to_context(context2, peer2);
        
        // Each context should only see its version of the peer
        let peers1 = discovery.discover_peers_in_context(context1);
        let peers2 = discovery.discover_peers_in_context(context2);
        
        assert_eq!(peers1.len(), 1);
        assert_eq!(peers2.len(), 1);
        assert_eq!(peers1[0].device_id(), peer_id);
        assert_eq!(peers2[0].device_id(), peer_id);
        
        // But they should have different capabilities (relationship-specific)
        assert!(peers1[0].has_capability("cap1"));
        assert!(!peers1[0].has_capability("cap2"));
        assert!(peers2[0].has_capability("cap2"));
        assert!(!peers2[0].has_capability("cap1"));
    }
}