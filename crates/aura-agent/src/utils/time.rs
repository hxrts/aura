//! Time utilities for timestamp generation using Effects system
//!
//! Provides consistent, testable timestamp generation across the agent crate
//! by leveraging the injectable Effects system from aura-types.

use aura_types::effects::time::{TimeEffects, ProductionTimeEffects, TestTimeEffects};
use std::sync::Arc;

/// Time provider for agent operations - supports both production and test modes
pub struct AgentTimeProvider {
    effects: Arc<dyn TimeEffects + Send + Sync>,
}

impl AgentTimeProvider {
    /// Create a new time provider using production time effects
    pub fn production() -> Self {
        Self {
            effects: Arc::new(ProductionTimeEffects::new()),
        }
    }
    
    /// Create a time provider using test time effects with controllable time
    pub fn test(seed: u64) -> Self {
        Self {
            effects: Arc::new(TestTimeEffects::new(seed)),
        }
    }
    
    /// Create a time provider from existing time effects
    pub fn from_effects(effects: Arc<dyn TimeEffects + Send + Sync>) -> Self {
        Self { effects }
    }
    
    /// Get current timestamp in seconds since UNIX epoch
    pub fn timestamp_secs(&self) -> u64 {
        self.effects.current_timestamp()
    }
    
    /// Get current timestamp in milliseconds since UNIX epoch
    pub fn timestamp_millis(&self) -> u128 {
        // Convert seconds to milliseconds
        (self.effects.current_timestamp() as u128) * 1000
    }
    
    /// Get the underlying time effects for advanced operations
    pub fn effects(&self) -> &Arc<dyn TimeEffects + Send + Sync> {
        &self.effects
    }
}

impl Default for AgentTimeProvider {
    fn default() -> Self {
        Self::production()
    }
}

/// Global time provider instance for convenience functions
static GLOBAL_TIME_PROVIDER: std::sync::OnceLock<AgentTimeProvider> = std::sync::OnceLock::new();

/// Initialize the global time provider (typically called once at startup)
pub fn init_time_provider(provider: AgentTimeProvider) {
    let _ = GLOBAL_TIME_PROVIDER.set(provider);
}

/// Get current timestamp in milliseconds since UNIX epoch (convenience function)
pub fn timestamp_millis() -> u128 {
    let provider = GLOBAL_TIME_PROVIDER.get_or_init(|| AgentTimeProvider::default());
    provider.timestamp_millis()
}

/// Get current timestamp in seconds since UNIX epoch (convenience function)
pub fn timestamp_secs() -> u64 {
    let provider = GLOBAL_TIME_PROVIDER.get_or_init(|| AgentTimeProvider::default());
    provider.timestamp_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_types::effects::time::TestTimeEffects;

    #[test]
    fn test_agent_time_provider_production() {
        let provider = AgentTimeProvider::production();
        let ts1 = provider.timestamp_secs();
        let ts2 = provider.timestamp_millis();
        
        // Production timestamps should be reasonable (after 2020)
        assert!(ts1 > 1_600_000_000);
        assert!(ts2 > 1_600_000_000_000);
    }
    
    #[test]
    fn test_agent_time_provider_test() {
        let provider = AgentTimeProvider::test(1234567890);
        
        // Test provider should return deterministic time
        let ts1 = provider.timestamp_secs();
        let ts2 = provider.timestamp_secs();
        assert_eq!(ts1, ts2); // Should be same time
        assert_eq!(ts1, 1234567890);
        
        // Milliseconds should be seconds * 1000
        let ts_millis = provider.timestamp_millis();
        assert_eq!(ts_millis, (ts1 as u128) * 1000);
    }
    
    #[test]
    fn test_controllable_test_time() {
        let test_effects = Arc::new(TestTimeEffects::new(1000));
        let provider = AgentTimeProvider::from_effects(test_effects.clone());
        
        // Initial time
        assert_eq!(provider.timestamp_secs(), 1000);
        
        // Advance time manually
        test_effects.advance_time(100);
        assert_eq!(provider.timestamp_secs(), 1100);
        
        // Set specific time
        test_effects.set_time(2000);
        assert_eq!(provider.timestamp_secs(), 2000);
        assert_eq!(provider.timestamp_millis(), 2000000);
    }
    
    #[test]
    fn test_global_convenience_functions() {
        // Initialize with test provider
        let test_provider = AgentTimeProvider::test(5000);
        init_time_provider(test_provider);
        
        // Convenience functions should use the initialized provider
        assert_eq!(timestamp_secs(), 5000);
        assert_eq!(timestamp_millis(), 5000000);
    }
    
    #[test]
    fn test_from_existing_effects() {
        let test_effects = Arc::new(TestTimeEffects::new(7777));
        let provider = AgentTimeProvider::from_effects(test_effects);
        
        assert_eq!(provider.timestamp_secs(), 7777);
        
        // Should have access to the underlying effects
        let effects = provider.effects();
        assert_eq!(effects.current_timestamp(), 7777);
    }
}
