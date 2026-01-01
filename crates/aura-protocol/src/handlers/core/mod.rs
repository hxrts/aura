//! Layer 4: Core Handler Infrastructure - Type Erasure for Protocol Integration
//!
//! Provides type-erased handler traits for dynamic dispatch and integration with
//! the protocol layer's guard chain and multi-party coordination systems.
//!
//! **Components**:
//! - **Erased**: Type-erased handler traits for dynamic dispatch; enables runtime polymorphism
//! - **Factory**: Handler construction with configuration, builder patterns, and platform detection
//!
//! **Design Pattern** (per docs/106_effect_system_and_runtime.md):
//! - Handlers implement effect traits (Layer 1 interfaces)
//! - aura-composition provides registry and composition (Layer 3)
//! - This module provides protocol integration (Layer 4)
//! - Type-erasing enables plugin systems and dynamic handler loading
//!
//! **Integration**: Works with guard chain (aura-protocol/guards) to enforce authorization,
//! flow budgets, and privacy at message entry points

// Type erasure
pub mod erased;

// Factory components
mod builder;
mod config;
mod error;
mod platform;
mod traits;

// Re-export erased types
pub use erased::{AuraHandler, BoxedHandler, HandlerUtils};

// Re-export factory types
pub use builder::AuraHandlerBuilder;
pub use config::{
    AuraHandlerConfig, MiddlewareConfig, MiddlewareSpec, PlatformConfig, SimulationConfig,
};
pub use error::FactoryError;
pub use platform::{PlatformDetector, PlatformInfo};
pub use traits::AuraHandlerFactory;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::ExecutionMode;
    use aura_core::identifiers::DeviceId;

    #[test]
    fn test_platform_detection() {
        let info = PlatformDetector::detect_platform().expect("platform detection should succeed");

        // Basic sanity checks
        assert!(!info.os.is_empty());
        assert!(!info.arch.is_empty());
    }

    #[test]
    fn test_config_for_testing() {
        let device_id = DeviceId::new_from_entropy([3u8; 32]);
        let config = AuraHandlerConfig::for_testing(device_id);

        assert_eq!(config.device_id, device_id);
        assert!(matches!(config.execution_mode, ExecutionMode::Testing));
    }

    #[test]
    fn test_middleware_config_defaults() {
        let middleware = MiddlewareConfig::default();

        // Default: logging enabled, metrics/tracing disabled
        assert!(middleware.enable_logging);
        assert!(!middleware.enable_metrics);
        assert!(!middleware.enable_tracing);
    }
}
