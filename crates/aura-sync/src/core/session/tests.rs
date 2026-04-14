use super::*;
use aura_core::AuraError;
use aura_testkit::builders::test_device_id;

/// Helper function to create `PhysicalTime` for tests.
fn test_time(ts_ms: u64) -> PhysicalTime {
    PhysicalTime {
        ts_ms,
        uncertainty: None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct TestProtocolState {
    phase: String,
    data: Vec<u8>,
}

#[test]
fn test_session_creation_and_activation() {
    let now = test_time(1000000);
    let mut manager =
        SessionManager::<TestProtocolState>::new(SessionConfig::default(), now.clone());
    let participants = vec![test_device_id(1), test_device_id(2)];

    let session_id = manager.create_session(participants.clone(), &now).unwrap();
    assert_eq!(manager.count_active_sessions(), 0);

    let initial_state = TestProtocolState {
        phase: "initialization".to_string(),
        data: vec![1, 2, 3],
    };
    manager
        .activate_session(session_id, initial_state.clone(), &now)
        .unwrap();
    assert_eq!(manager.count_active_sessions(), 1);

    let session = manager.get_session(&session_id).unwrap();
    match session {
        SessionState::Active {
            protocol_state,
            participants: session_participants,
            ..
        } => {
            assert_eq!(protocol_state, &initial_state);
            assert_eq!(session_participants, &participants);
        }
        _ => panic!("Session should be active"),
    }
}

#[test]
fn test_session_completion() {
    let now = test_time(1000000);
    let mut manager =
        SessionManager::<TestProtocolState>::new(SessionConfig::default(), now.clone());
    let session_id = manager
        .create_session(vec![test_device_id(1)], &now)
        .unwrap();

    let initial_state = TestProtocolState {
        phase: "test".to_string(),
        data: vec![],
    };
    manager
        .activate_session(session_id, initial_state, &now)
        .unwrap();

    let mut metadata = HashMap::new();
    metadata.insert("test_key".to_string(), "test_value".to_string());

    manager
        .complete_session(session_id, 100, 1024, metadata, &test_time(1000100))
        .unwrap();
    assert_eq!(manager.count_active_sessions(), 0);
    assert_eq!(manager.count_completed_sessions(), 1);

    let session = manager.get_session(&session_id).unwrap();
    match session {
        SessionState::Completed(SessionResult::Success {
            operations_count,
            bytes_transferred,
            ..
        }) => {
            assert_eq!(*operations_count, 100);
            assert_eq!(*bytes_transferred, 1024);
        }
        _ => panic!("Session should be completed successfully"),
    }
}

#[test]
fn test_session_failure() {
    let now = test_time(1000000);
    let mut manager =
        SessionManager::<TestProtocolState>::new(SessionConfig::default(), now.clone());
    let session_id = manager
        .create_session(vec![test_device_id(1)], &now)
        .unwrap();

    let initial_state = TestProtocolState {
        phase: "test".to_string(),
        data: vec![],
    };
    manager
        .activate_session(session_id, initial_state, &now)
        .unwrap();

    let error = SessionError::ProtocolViolation {
        constraint: "test constraint".to_string(),
    };
    manager
        .fail_session(session_id, error, None, &test_time(1000010))
        .unwrap();

    let session = manager.get_session(&session_id).unwrap();
    match session {
        SessionState::Completed(SessionResult::Failure {
            error: session_error,
            ..
        }) => match session_error {
            SessionError::ProtocolViolation { constraint } => {
                assert_eq!(constraint, "test constraint");
            }
            _ => panic!("Wrong error type"),
        },
        _ => panic!("Session should be completed with failure"),
    }
}

#[test]
fn test_concurrent_session_limit() {
    let config = SessionConfig {
        max_concurrent_sessions: 2,
        ..SessionConfig::default()
    };
    let now = test_time(1000000);
    let mut manager = SessionManager::<TestProtocolState>::new(config, now.clone());

    let session1 = manager
        .create_session(vec![test_device_id(1)], &now)
        .unwrap();
    let session2 = manager
        .create_session(vec![test_device_id(1)], &now)
        .unwrap();

    let state = TestProtocolState {
        phase: "test".to_string(),
        data: vec![],
    };
    manager
        .activate_session(session1, state.clone(), &now)
        .unwrap();
    manager.activate_session(session2, state, &now).unwrap();

    let result = manager.create_session(vec![test_device_id(1)], &now);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), AuraError::Internal { .. }));
}

#[test]
fn test_session_timeout() {
    let config = SessionConfig {
        timeout: Duration::from_millis(100),
        ..SessionConfig::default()
    };
    let now = test_time(1000000);
    let mut manager = SessionManager::<TestProtocolState>::new(config, now.clone());

    let session_id = manager
        .create_session(vec![test_device_id(1)], &now)
        .unwrap();

    let future_time = test_time(1000200);
    let state = TestProtocolState {
        phase: "test".to_string(),
        data: vec![],
    };
    let result = manager.activate_session(session_id, state, &future_time);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), AuraError::Internal { .. }));
}

#[test]
fn test_cleanup_stale_sessions() {
    let config = SessionConfig {
        cleanup_interval: Duration::from_millis(50),
        ..SessionConfig::default()
    };
    let now = test_time(1000000);
    let mut manager = SessionManager::<TestProtocolState>::new(config, now.clone());

    let session_id = manager
        .create_session(vec![test_device_id(1)], &now)
        .unwrap();
    let state = TestProtocolState {
        phase: "test".to_string(),
        data: vec![],
    };
    manager.activate_session(session_id, state, &now).unwrap();
    manager
        .complete_session(session_id, 0, 0, HashMap::new(), &test_time(1000050))
        .unwrap();

    assert_eq!(manager.sessions.len(), 1);

    let cleanup_time = test_time(1000200);
    let removed = manager.cleanup_stale_sessions(&cleanup_time).unwrap();
    assert!(removed > 0);
}

#[test]
fn test_session_statistics() {
    let now = test_time(1000000);
    let mut manager =
        SessionManager::<TestProtocolState>::new(SessionConfig::default(), now.clone());

    for i in 0..3 {
        let session_id = manager
            .create_session(vec![test_device_id(1)], &now)
            .unwrap();
        let state = TestProtocolState {
            phase: "test".to_string(),
            data: vec![],
        };
        manager.activate_session(session_id, state, &now).unwrap();

        if i < 2 {
            manager
                .complete_session(
                    session_id,
                    10 * (i + 1),
                    100 * (i + 1),
                    HashMap::new(),
                    &test_time(1000000 + 100 * (i + 1)),
                )
                .unwrap();
        } else {
            let error = SessionError::ProtocolViolation {
                constraint: "test".to_string(),
            };
            manager
                .fail_session(session_id, error, None, &test_time(1000050))
                .unwrap();
        }
    }

    let stats = manager.get_statistics();
    assert_eq!(stats.total_sessions, 3);
    assert_eq!(stats.completed_sessions, 2);
    assert_eq!(stats.failed_sessions, 1);
    assert_eq!(stats.timeout_sessions, 0);
    assert!((stats.success_rate_percent - 66.67).abs() < 0.1);
}
