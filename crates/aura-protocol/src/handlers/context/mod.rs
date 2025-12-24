//! Context Management for Handler Operations
//!
//! This module provides the unified context infrastructure for handler operations,
//! supporting pure functional operations with immutable, thread-safe context.
//!
//! The `AuraContext` type flows through all handler operations without mutation,
//! ensuring thread-safe access. All modifications return new instances rather
//! than mutating in place.

mod agent;
mod choreographic;
mod context;
mod middleware;
mod simulation;

// Re-export all public types
pub use agent::{AgentContext, AuthenticationState, PlatformInfo, SessionMetadata};
pub use choreographic::ChoreographicContext;
pub use context::AuraContext;
pub use middleware::{MetricsContext, MiddlewareContext, RetryContext, TracingContext};
pub use simulation::{FaultInjectionSettings, PropertyCheckingConfig, SimulationContext};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::choreographic::ChoreographicRole;
    use crate::handlers::ExecutionMode;
    use aura_core::identifiers::DeviceId;
    use aura_core::SessionId;
    use std::time::Duration;
    use uuid::Uuid;

    #[test]
    fn test_immutable_choreographic_context() {
        let digest = aura_core::hash::hash(b"handler-choreo-role-0");
        let mut uuid_bytes = [0u8; 16];
        uuid_bytes.copy_from_slice(&digest[..16]);
        let role_id = Uuid::from_bytes(uuid_bytes);
        let role = ChoreographicRole::new(role_id, 0);
        let participants = vec![role];
        let ctx = ChoreographicContext::new(role, participants, 1);

        assert_eq!(ctx.current_role, role);
        assert_eq!(ctx.epoch, 1);
        assert_eq!(ctx.participant_count(), 1);

        // Test immutable state management
        let ctx2 = ctx.with_state("test", &42u32).unwrap();

        // Original context unchanged
        assert!(ctx.get_state::<u32>("test").unwrap().is_none());

        // New context has the value
        let value: Option<u32> = ctx2.get_state("test").unwrap();
        assert_eq!(value, Some(42));
    }

    #[test]
    fn test_immutable_simulation_context() {
        let ctx = SimulationContext::new(42);
        assert_eq!(ctx.seed, 42);
        assert_eq!(ctx.simulation_time, Duration::ZERO);
        assert!(!ctx.time_controlled);

        let ctx2 = ctx.with_time_advanced(Duration::from_secs(1));
        assert_eq!(ctx.simulation_time, Duration::ZERO); // Original unchanged
        assert_eq!(ctx2.simulation_time, Duration::from_secs(1)); // New has change

        let ctx3 = ctx2.with_time_control();
        assert!(!ctx2.time_controlled); // Original unchanged
        assert!(ctx3.time_controlled); // New has change
    }

    #[test]
    fn test_immutable_agent_context() {
        let device_id = DeviceId::from(uuid::Uuid::from_bytes([1u8; 16]));
        let ctx = AgentContext::new(device_id);

        // Test immutable configuration
        let ctx2 = ctx.with_config("key", "value");
        assert_eq!(ctx.get_config("key"), None); // Original unchanged
        assert_eq!(ctx2.get_config("key"), Some("value")); // New has value

        // Test immutable sessions
        let session_id = SessionId::from_uuid(uuid::Uuid::from_u128(1));
        let ctx3 = ctx2.with_session(session_id, "test_session", 1000);

        assert!(ctx2.get_session(&session_id).is_none()); // Original unchanged
        assert!(ctx3.get_session(&session_id).is_some()); // New has session

        let ctx4 = ctx3.without_session(&session_id);
        assert!(ctx3.get_session(&session_id).is_some()); // Original unchanged
        assert!(ctx4.get_session(&session_id).is_none()); // New removed
    }

    #[test]
    fn test_immutable_middleware_context() {
        let ctx = MiddlewareContext::new();

        // Test immutable custom data
        let ctx2 = ctx.with_custom_data("test", &42u32).unwrap();
        assert!(ctx.get_custom_data::<u32>("test").unwrap().is_none()); // Original unchanged
        assert_eq!(ctx2.get_custom_data::<u32>("test").unwrap(), Some(42)); // New has value

        // Test immutable tracing
        let ctx3 = ctx2.with_tracing("trace123".to_string(), "span456".to_string());
        assert!(!ctx2.tracing.enabled); // Original unchanged
        assert!(ctx3.tracing.enabled); // New enabled

        // Test immutable metrics
        let ctx4 = ctx3.with_metrics();
        assert!(!ctx3.metrics.enabled); // Original unchanged
        assert!(ctx4.metrics.enabled); // New enabled

        let ctx5 = ctx4.with_metrics_label("service", "test");
        assert!(ctx4.metrics.labels.is_empty()); // Original unchanged
        assert_eq!(
            ctx5.metrics.labels.get("service"),
            Some(&"test".to_string())
        );
    }

    #[test]
    fn test_immutable_aura_context() {
        let device_id = DeviceId::from(uuid::Uuid::from_bytes([1u8; 16]));

        let ctx = AuraContext::for_testing(device_id);
        assert_eq!(ctx.execution_mode, ExecutionMode::Testing);
        assert!(ctx.is_deterministic());

        // Test immutable metadata
        let ctx2 = ctx.with_metadata("key", "value");
        assert!(ctx.metadata.is_empty()); // Original unchanged
        assert_eq!(ctx2.metadata.get("key"), Some(&"value".to_string()));

        // Test immutable session
        let session_id = SessionId::from_uuid(uuid::Uuid::from_u128(2));
        let ctx3 = ctx2.with_session(session_id);
        assert!(ctx2.session_id.is_none()); // Original unchanged
        assert_eq!(ctx3.session_id, Some(session_id));

        // Test child operation
        let new_op_id = Uuid::from_bytes([1u8; 16]);
        let ctx4 = ctx3.child_operation(new_op_id);
        assert_ne!(ctx3.operation_id, new_op_id); // Original unchanged
        assert_eq!(ctx4.operation_id, new_op_id);
    }
}
