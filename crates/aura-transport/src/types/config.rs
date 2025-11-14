//! Transport Configuration with Built-in Privacy Levels
//!
//! Provides minimal transport configuration with integrated privacy preservation.
//! No abstraction layers - direct privacy configuration. Target: <100 lines.

use serde::{Deserialize, Serialize};
use std::time::Duration;

// Re-export PrivacyLevel from envelope for convenience
pub use super::envelope::PrivacyLevel;

/// Minimal configuration with integrated privacy levels
///
/// Provides essential transport settings with privacy-by-design principles
/// built directly into the configuration structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportConfig {
    /// Base privacy level for all transport operations
    pub privacy_level: PrivacyLevel,
    
    /// Connection timeout settings
    pub connection_timeout: Duration,
    
    /// Maximum message size in bytes
    pub max_message_size: usize,
    
    /// Enable relationship-scoped routing
    pub enable_relationship_scoping: bool,
    
    /// Enable capability-based access control
    pub enable_capability_filtering: bool,
    
    /// Maximum concurrent connections
    pub max_connections: usize,
    
    /// Enable message blinding by default
    pub default_blinding: bool,
}

impl Default for TransportConfig {
    /// Privacy-preserving defaults
    fn default() -> Self {
        Self {
            // Privacy-by-design: default to relationship scoped
            privacy_level: PrivacyLevel::RelationshipScoped,
            
            // Conservative timeouts
            connection_timeout: Duration::from_secs(30),
            
            // Reasonable message size limit
            max_message_size: 1024 * 1024, // 1MB
            
            // Privacy features enabled by default
            enable_relationship_scoping: true,
            enable_capability_filtering: true,
            default_blinding: true,
            
            // Conservative connection limit
            max_connections: 100,
        }
    }
}

impl TransportConfig {
    /// Create configuration optimized for clear communication
    pub fn clear() -> Self {
        Self {
            privacy_level: PrivacyLevel::Clear,
            enable_relationship_scoping: false,
            enable_capability_filtering: false,
            default_blinding: false,
            ..Default::default()
        }
    }
    
    /// Create configuration optimized for maximum privacy
    pub fn maximum_privacy() -> Self {
        Self {
            privacy_level: PrivacyLevel::Blinded,
            enable_relationship_scoping: true,
            enable_capability_filtering: true,
            default_blinding: true,
            max_message_size: 64 * 1024, // Smaller messages for privacy
            max_connections: 50, // Fewer connections for privacy
            ..Default::default()
        }
    }
    
    /// Create configuration for testing with relaxed privacy
    pub fn testing() -> Self {
        Self {
            privacy_level: PrivacyLevel::Clear,
            connection_timeout: Duration::from_secs(5),
            enable_relationship_scoping: false,
            enable_capability_filtering: false,
            default_blinding: false,
            max_connections: 10,
            ..Default::default()
        }
    }
    
    /// Validate configuration settings
    pub fn validate(&self) -> Result<(), String> {
        if self.max_message_size == 0 {
            return Err("max_message_size must be greater than 0".to_string());
        }
        
        if self.max_connections == 0 {
            return Err("max_connections must be greater than 0".to_string());
        }
        
        if self.connection_timeout.is_zero() {
            return Err("connection_timeout must be greater than 0".to_string());
        }
        
        // Privacy consistency checks
        if matches!(self.privacy_level, PrivacyLevel::RelationshipScoped) &&
           !self.enable_relationship_scoping {
            return Err("relationship_scoping must be enabled for RelationshipScoped privacy level".to_string());
        }
        
        Ok(())
    }
    
    /// Check if configuration supports privacy features
    pub fn supports_privacy(&self) -> bool {
        !matches!(self.privacy_level, PrivacyLevel::Clear) ||
        self.enable_relationship_scoping ||
        self.enable_capability_filtering ||
        self.default_blinding
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_privacy() {
        let config = TransportConfig::default();
        
        // Should default to privacy-preserving settings
        assert!(matches!(config.privacy_level, PrivacyLevel::RelationshipScoped));
        assert!(config.enable_relationship_scoping);
        assert!(config.enable_capability_filtering);
        assert!(config.default_blinding);
        assert!(config.supports_privacy());
        
        // Should validate successfully
        assert!(config.validate().is_ok());
    }
    
    #[test]
    fn test_privacy_configurations() {
        // Clear configuration
        let clear = TransportConfig::clear();
        assert!(matches!(clear.privacy_level, PrivacyLevel::Clear));
        assert!(!clear.enable_relationship_scoping);
        assert!(!clear.default_blinding);
        assert!(!clear.supports_privacy());
        
        // Maximum privacy configuration
        let max_privacy = TransportConfig::maximum_privacy();
        assert!(matches!(max_privacy.privacy_level, PrivacyLevel::Blinded));
        assert!(max_privacy.enable_relationship_scoping);
        assert!(max_privacy.default_blinding);
        assert!(max_privacy.supports_privacy());
        assert!(max_privacy.max_message_size < TransportConfig::default().max_message_size);
    }
    
    #[test]
    fn test_validation() {
        let mut config = TransportConfig::default();
        
        // Valid configuration
        assert!(config.validate().is_ok());
        
        // Invalid max_message_size
        config.max_message_size = 0;
        assert!(config.validate().is_err());
        
        // Invalid max_connections
        config.max_message_size = 1024;
        config.max_connections = 0;
        assert!(config.validate().is_err());
        
        // Privacy inconsistency
        config.max_connections = 10;
        config.privacy_level = PrivacyLevel::RelationshipScoped;
        config.enable_relationship_scoping = false;
        assert!(config.validate().is_err());
    }
}