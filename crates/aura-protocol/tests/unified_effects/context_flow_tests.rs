//! Context flow tests for the unified effect system
//!
//! These tests verify that AuraContext flows properly through the effect system
//! and maintains consistency across different execution modes and middleware layers.

use std::collections::HashMap;
use std::time::Duration;
use uuid::Uuid;

use aura_protocol::effects::system::AuraEffectSystem;
use aura_protocol::handlers::{AuraContext, AuraHandler, EffectType, ExecutionMode};
use aura_types::identifiers::DeviceId;

#[tokio::test]
async fn test_basic_context_creation() {
    let device_id = DeviceId::from(Uuid::new_v4());

    // Test different context creation methods
    let test_ctx = AuraContext::for_testing(device_id);
    assert_eq!(test_ctx.device_id, device_id);
    assert_eq!(test_ctx.execution_mode, ExecutionMode::Testing);
    assert!(test_ctx.session_id.is_none());

    let choreo_ctx = AuraContext::for_choreography(device_id);
    assert_eq!(choreo_ctx.device_id, device_id);
    assert!(choreo_ctx.choreographic.is_some());

    let production_ctx = AuraContext::for_production(device_id);
    assert_eq!(production_ctx.device_id, device_id);
    assert_eq!(production_ctx.execution_mode, ExecutionMode::Production);
}

#[tokio::test]
async fn test_context_preservation_through_effects() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);

    // Set up initial context state
    let session_id = Uuid::new_v4();
    ctx.session_id = Some(session_id.into());

    // Add some middleware context
    ctx.middleware
        .add_data("test_key".to_string(), "test_value".to_string());

    // Execute an effect
    let log_params = aura_protocol::effects::console::ConsoleLogParams {
        level: aura_protocol::effects::console::LogLevel::Info,
        message: "Context preservation test".to_string(),
        component: Some("test".to_string()),
    };

    let effect = Effect::new(EffectType::Console, "log", &log_params).unwrap();
    system.execute_effect(effect, &mut ctx).await.unwrap();

    // Verify context was preserved
    assert_eq!(ctx.session_id, Some(session_id.into()));
    assert_eq!(ctx.device_id, device_id);
    assert!(ctx.middleware.get_data("test_key").is_some());
}

#[tokio::test]
async fn test_choreographic_context_flow() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_choreography(device_id);

    // Verify choreographic context is properly set up
    assert!(ctx.choreographic.is_some());

    // Execute a choreographic effect
    let event = aura_protocol::effects::choreographic::ChoreographyEvent::SessionStarted {
        session_id: Uuid::new_v4().to_string(),
        participants: vec![device_id.to_string()],
    };

    let event_params = aura_protocol::effects::choreographic::ChoreographyEventParams { event };
    let effect = Effect::new(EffectType::Choreographic, "emit_event", &event_params).unwrap();

    system.execute_effect(effect, &mut ctx).await.unwrap();

    // Choreographic context should still be present
    assert!(ctx.choreographic.is_some());
}

#[tokio::test]
async fn test_simulation_context_flow() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_simulation(device_id, 42);
    let mut ctx = AuraContext::for_testing(device_id);

    // Set up simulation context
    ctx.simulation = Some(aura_types::handlers::context::SimulationContext {
        seed: Some(42),
        deterministic: Some(true),
        current_time: Some(Duration::from_secs(100)),
        time_acceleration: Some(2.0),
        time_paused: Some(false),
        fault_context: None,
        state_snapshots: Some(HashMap::new()),
        property_violations: Some(Vec::new()),
        chaos_experiments: Some(Vec::new()),
    });

    // Execute an effect that should preserve simulation context
    let hash_params = aura_protocol::effects::crypto::CryptoHashParams {
        algorithm: aura_protocol::effects::crypto::HashAlgorithm::Blake3,
        data: b"simulation context test".to_vec(),
    };

    let effect = Effect::new(EffectType::Crypto, "hash", &hash_params).unwrap();
    system.execute_effect(effect, &mut ctx).await.unwrap();

    // Simulation context should be preserved
    assert!(ctx.simulation.is_some());
    let sim_ctx = ctx.simulation.as_ref().unwrap();
    assert_eq!(sim_ctx.seed, Some(42));
    assert_eq!(sim_ctx.current_time, Some(Duration::from_secs(100)));
    assert_eq!(sim_ctx.time_acceleration, Some(2.0));
}

#[tokio::test]
async fn test_agent_context_flow() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);

    // Set up agent context
    ctx.agent = Some(aura_types::handlers::context::AgentContext {
        device_config: Some(HashMap::new()),
        authentication_state: Some("authenticated".to_string()),
        session_tokens: Some(HashMap::new()),
        stored_credentials: Some(HashMap::new()),
    });

    // Execute effects and verify agent context preservation
    let log_params = aura_protocol::effects::console::ConsoleLogParams {
        level: aura_protocol::effects::console::LogLevel::Info,
        message: "Agent context test".to_string(),
        component: Some("agent".to_string()),
    };

    let effect = Effect::new(EffectType::Console, "log", &log_params).unwrap();
    system.execute_effect(effect, &mut ctx).await.unwrap();

    // Agent context should be preserved
    assert!(ctx.agent.is_some());
    let agent_ctx = ctx.agent.as_ref().unwrap();
    assert_eq!(
        agent_ctx.authentication_state,
        Some("authenticated".to_string())
    );
}

#[tokio::test]
async fn test_middleware_context_modifications() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);

    // Add initial middleware context data
    ctx.middleware
        .add_data("counter".to_string(), "0".to_string());
    ctx.middleware
        .add_data("component".to_string(), "test".to_string());

    // Execute multiple effects and track context changes
    for i in 1..=5 {
        ctx.middleware
            .add_data("counter".to_string(), i.to_string());

        let log_params = aura_protocol::effects::console::ConsoleLogParams {
            level: aura_protocol::effects::console::LogLevel::Info,
            message: format!("Iteration {}", i),
            component: Some("test".to_string()),
        };

        let effect = Effect::new(EffectType::Console, "log", &log_params).unwrap();
        system.execute_effect(effect, &mut ctx).await.unwrap();

        // Verify context modifications persist
        assert_eq!(ctx.middleware.get_data("counter"), Some(&i.to_string()));
    }

    // Final verification
    assert_eq!(ctx.middleware.get_data("counter"), Some(&"5".to_string()));
    assert_eq!(
        ctx.middleware.get_data("component"),
        Some(&"test".to_string())
    );
}

#[tokio::test]
async fn test_context_isolation_between_effects() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx1 = AuraContext::for_testing(device_id);
    let mut ctx2 = AuraContext::for_testing(device_id);

    // Set different session IDs
    ctx1.session_id = Some(Uuid::new_v4().into());
    ctx2.session_id = Some(Uuid::new_v4().into());

    // Execute effects with different contexts
    let effect1 = Effect::new(
        EffectType::Console,
        "log",
        &aura_protocol::effects::console::ConsoleLogParams {
            level: aura_protocol::effects::console::LogLevel::Info,
            message: "Context 1".to_string(),
            component: Some("test1".to_string()),
        },
    )
    .unwrap();

    let effect2 = Effect::new(
        EffectType::Console,
        "log",
        &aura_protocol::effects::console::ConsoleLogParams {
            level: aura_protocol::effects::console::LogLevel::Info,
            message: "Context 2".to_string(),
            component: Some("test2".to_string()),
        },
    )
    .unwrap();

    system.execute_effect(effect1, &mut ctx1).await.unwrap();
    system.execute_effect(effect2, &mut ctx2).await.unwrap();

    // Contexts should remain isolated
    assert_ne!(ctx1.session_id, ctx2.session_id);
}

#[tokio::test]
async fn test_context_error_preservation() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);

    // Set up context state
    let original_session_id = Uuid::new_v4();
    ctx.session_id = Some(original_session_id.into());
    ctx.middleware
        .add_data("important_data".to_string(), "preserve_me".to_string());

    // Execute an effect that will fail
    let effect = Effect::new(EffectType::Console, "invalid_operation", &()).unwrap();
    let result = system.execute_effect(effect, &mut ctx).await;

    // Effect should fail
    assert!(result.is_err());

    // But context should be preserved
    assert_eq!(ctx.session_id, Some(original_session_id.into()));
    assert_eq!(
        ctx.middleware.get_data("important_data"),
        Some(&"preserve_me".to_string())
    );
}

#[tokio::test]
async fn test_nested_context_operations() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);

    // Set up complex context state
    ctx.session_id = Some(Uuid::new_v4().into());
    ctx.choreographic = Some(aura_types::handlers::context::ChoreographicContext {
        current_session: Some(Uuid::new_v4()),
        active_roles: Some(vec![]),
        message_history: Some(Vec::new()),
        protocol_state: Some(HashMap::new()),
    });

    // Execute multiple nested operations
    let operations = vec![
        ("log", "Starting nested operations"),
        ("log", "Processing choreographic state"),
        ("log", "Completing nested operations"),
    ];

    for (op, message) in operations {
        let log_params = aura_protocol::effects::console::ConsoleLogParams {
            level: aura_protocol::effects::console::LogLevel::Info,
            message: message.to_string(),
            component: Some("nested".to_string()),
        };

        let effect = Effect::new(EffectType::Console, op, &log_params).unwrap();
        system.execute_effect(effect, &mut ctx).await.unwrap();
    }

    // All context should be preserved
    assert!(ctx.session_id.is_some());
    assert!(ctx.choreographic.is_some());
}

#[tokio::test]
async fn test_concurrent_context_safety() {
    use std::sync::Arc;
    use tokio::sync::RwLock;

    let device_id = DeviceId::from(Uuid::new_v4());
    let system = Arc::new(RwLock::new(AuraEffectSystem::for_testing(device_id)));

    // Create multiple contexts for concurrent execution
    let mut handles = Vec::new();

    for i in 0..10 {
        let system_clone = system.clone();

        let handle = tokio::spawn(async move {
            let mut system = system_clone.write().await;
            let mut ctx = AuraContext::for_testing(device_id);

            // Set unique session ID
            ctx.session_id = Some(Uuid::new_v4().into());
            ctx.middleware
                .add_data("thread_id".to_string(), i.to_string());

            let log_params = aura_protocol::effects::console::ConsoleLogParams {
                level: aura_protocol::effects::console::LogLevel::Info,
                message: format!("Concurrent operation {}", i),
                component: Some("concurrent".to_string()),
            };

            let effect = Effect::new(EffectType::Console, "log", &log_params).unwrap();
            let result = system.execute_effect(effect, &mut ctx).await;

            // Return context state for verification
            (
                result.is_ok(),
                ctx.middleware.get_data("thread_id").cloned(),
            )
        });

        handles.push(handle);
    }

    // Wait for all operations and verify isolation
    for (i, handle) in handles.into_iter().enumerate() {
        let (success, thread_id) = handle.await.unwrap();
        assert!(success);
        assert_eq!(thread_id, Some(i.to_string()));
    }
}
