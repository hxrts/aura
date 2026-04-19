use super::*;

#[tokio::test]
async fn test_scenario_registration() {
    let handler = SimulationScenarioHandler::new(123);

    let scenario = ScenarioDefinition {
        id: "test_scenario".to_string(),
        name: "Test Scenario".to_string(),
        actions: vec![InjectionAction::ModifyParameter {
            key: "test_param".to_string(),
            value: "test_value".to_string(),
        }],
        trigger: TriggerCondition::Immediate,
        duration: Some(Duration::from_secs(10)),
        priority: 1,
    };

    let result = handler.register_scenario(scenario);
    assert!(result.is_ok());

    let stats = handler.get_injection_stats().unwrap();
    assert_eq!(stats.get("registered_scenarios"), Some(&"1".to_string()));
}

#[tokio::test]
async fn test_scenario_triggering() {
    let handler = SimulationScenarioHandler::new(123);

    let scenario = ScenarioDefinition {
        id: "trigger_test".to_string(),
        name: "Trigger Test".to_string(),
        actions: vec![],
        trigger: TriggerCondition::Immediate,
        duration: Some(Duration::from_secs(10)),
        priority: 1,
    };

    handler.register_scenario(scenario).unwrap();

    let result = handler.trigger_scenario("trigger_test");
    assert!(result.is_ok());

    let stats = handler.get_injection_stats().unwrap();
    assert_eq!(stats.get("total_injections"), Some(&"1".to_string()));
}

#[tokio::test]
async fn test_checkpoint_creation() {
    let handler = SimulationScenarioHandler::new(123);

    let result = handler.create_checkpoint("test_checkpoint");
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_state_inspection() {
    let handler = SimulationScenarioHandler::new(123);

    let result = handler.inspect_state("scenarios", "count").await;
    assert!(result.is_ok());

    // Should return 0 scenarios
    let count = result.unwrap().downcast::<usize>().unwrap();
    assert_eq!(*count, 0);
}

#[tokio::test]
async fn test_event_recording() {
    let handler = SimulationScenarioHandler::new(123);

    let mut event_data = HashMap::new();
    event_data.insert("key".to_string(), "value".to_string());

    let result = handler.record_event("test_event", event_data).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_metric_recording() {
    let handler = SimulationScenarioHandler::new(123);

    let result = handler.record_metric("test_metric", 42.0, "units").await;
    assert!(result.is_ok());

    // Verify metric was recorded
    let metric_result = handler.inspect_state("metrics", "test_metric").await;
    assert!(metric_result.is_ok());

    let metric_value = metric_result.unwrap().downcast::<f64>().unwrap();
    assert_eq!(*metric_value, 42.0);
}

#[tokio::test]
async fn test_randomization_settings() {
    let handler = SimulationScenarioHandler::new(123);

    let result = handler.set_randomization(true, 0.5);
    assert!(result.is_ok());

    let stats = handler.get_injection_stats().unwrap();
    assert_eq!(
        stats.get("randomization_enabled"),
        Some(&"true".to_string())
    );
    assert_eq!(stats.get("injection_probability"), Some(&"0.5".to_string()));
}

#[tokio::test]
async fn test_chat_group_creation() {
    let handler = SimulationScenarioHandler::new(123);

    let result = handler.create_chat_group(
        "Test Group",
        "alice",
        vec!["bob".to_string(), "carol".to_string()],
    );
    assert!(result.is_ok());

    let _group_id = result.unwrap();
    let stats = handler.get_chat_stats().unwrap();
    assert_eq!(stats.get("chat_groups"), Some(&"1".to_string()));

    // Test state inspection
    let group_count = handler.inspect_state("chat", "groups").await.unwrap();
    let count = group_count.downcast::<usize>().unwrap();
    assert_eq!(*count, 1);
}

#[tokio::test]
async fn test_chat_messaging() {
    let handler = SimulationScenarioHandler::new(123);

    let group_id = handler
        .create_chat_group(
            "Test Group",
            "alice",
            vec!["bob".to_string(), "carol".to_string()],
        )
        .unwrap();

    // Test sending messages
    let result1 = handler.send_chat_message(&group_id, "alice", "Hello everyone!");
    assert!(result1.is_ok());

    let result2 = handler.send_chat_message(&group_id, "bob", "Hi Alice!");
    assert!(result2.is_ok());

    let stats = handler.get_chat_stats().unwrap();
    assert_eq!(stats.get("total_messages"), Some(&"2".to_string()));

    // Test that non-members can't send messages
    let result_fail = handler.send_chat_message(&group_id, "dave", "I'm not a member");
    assert!(result_fail.is_err());
}

#[tokio::test]
async fn test_data_loss_simulation() {
    let handler = SimulationScenarioHandler::new(123);

    let group_id = handler
        .create_chat_group("Test Group", "alice", vec!["bob".to_string()])
        .unwrap();

    // Send some messages before data loss
    handler
        .send_chat_message(&group_id, "alice", "Message 1")
        .unwrap();
    handler
        .send_chat_message(&group_id, "bob", "Message 2")
        .unwrap();

    // Simulate data loss for Bob
    let result = handler.simulate_data_loss("bob", "complete_device_loss", true);
    assert!(result.is_ok());

    let stats = handler.get_chat_stats().unwrap();
    assert_eq!(
        stats.get("participants_with_data_loss"),
        Some(&"1".to_string())
    );

    // Check state inspection for data loss
    let loss_count = handler.inspect_state("data_loss", "bob").await.unwrap();
    let count = loss_count.downcast::<usize>().unwrap();
    assert!(*count > 0); // Bob had messages before loss
}

#[tokio::test]
async fn test_guardian_recovery() {
    let handler = SimulationScenarioHandler::new(123);

    // Initiate recovery process
    let result = handler.initiate_guardian_recovery(
        "bob",
        vec!["alice".to_string(), "carol".to_string()],
        2,
    );
    assert!(result.is_ok());

    let stats = handler.get_chat_stats().unwrap();
    assert_eq!(stats.get("active_recoveries"), Some(&"1".to_string()));

    // Verify recovery completion
    let validation_result = handler.verify_recovery_success(
        "bob",
        vec![
            "keys_restored".to_string(),
            "account_accessible".to_string(),
        ],
    );
    assert!(validation_result.is_ok());
    assert!(validation_result.unwrap());

    // Check that recovery is now complete
    let recovery_complete = handler.inspect_state("recovery", "bob").await.unwrap();
    let is_complete = recovery_complete.downcast::<bool>().unwrap();
    assert!(*is_complete);
}

#[tokio::test]
async fn test_message_history_validation() {
    let handler = SimulationScenarioHandler::new(123);

    let group_id = handler
        .create_chat_group("Recovery Test", "alice", vec!["bob".to_string()])
        .unwrap();

    // Send messages before data loss
    handler
        .send_chat_message(&group_id, "alice", "Message 1")
        .unwrap();
    handler
        .send_chat_message(&group_id, "bob", "Message 2")
        .unwrap();
    handler
        .send_chat_message(&group_id, "alice", "Message 3")
        .unwrap();

    // Simulate data loss
    handler
        .simulate_data_loss("bob", "complete_device_loss", true)
        .unwrap();

    // Test message history validation
    let validation_result = handler.validate_message_history("bob", 2, true);
    assert!(validation_result.is_ok());
    assert!(validation_result.unwrap());

    // Test validation failure case
    let validation_fail = handler.validate_message_history("bob", 10, true);
    assert!(validation_fail.is_ok());
    assert!(!validation_fail.unwrap());
}

#[tokio::test]
async fn test_insufficient_guardians_error() {
    let handler = SimulationScenarioHandler::new(123);

    // Try to initiate recovery with insufficient guardians
    let result = handler.initiate_guardian_recovery(
        "bob",
        vec!["alice".to_string()], // Only 1 guardian
        2,                         // But need 2
    );
    assert!(result.is_err());
}

#[tokio::test]
async fn test_scenario_actions_apply_parameter_and_behavior_state() {
    let handler = SimulationScenarioHandler::new(123);

    handler
        .register_scenario(ScenarioDefinition {
            id: "action_apply".to_string(),
            name: "Action Apply".to_string(),
            actions: vec![
                InjectionAction::ModifyParameter {
                    key: "sync_density".to_string(),
                    value: "sparse".to_string(),
                },
                InjectionAction::ModifyBehavior {
                    component: "selector".to_string(),
                    behavior: "weighted_rotation".to_string(),
                },
            ],
            trigger: TriggerCondition::Immediate,
            duration: Some(Duration::from_secs(5)),
            priority: 1,
        })
        .expect("register scenario");

    handler
        .trigger_scenario("action_apply")
        .expect("trigger scenario");

    let parameter = handler
        .inspect_state("scenarios", "parameter:sync_density")
        .await
        .expect("inspect parameter")
        .downcast::<String>()
        .expect("parameter type");
    assert_eq!(&*parameter, "sparse");

    let behavior = handler
        .inspect_state("scenarios", "behavior:selector")
        .await
        .expect("inspect behavior")
        .downcast::<String>()
        .expect("behavior type");
    assert_eq!(&*behavior, "weighted_rotation");
}

#[tokio::test]
async fn test_adaptive_privacy_scenario_support_covers_phase_six_surface() {
    let handler = SimulationScenarioHandler::new(123);

    handler
        .register_scenario(ScenarioDefinition {
            id: "adaptive_privacy_surface".to_string(),
            name: "Adaptive Privacy Surface".to_string(),
            actions: vec![
                InjectionAction::AdaptivePrivacyTransition(
                    AdaptivePrivacyTransition::ConfigureMovement {
                        profile_id: "clustered_social".to_string(),
                        clusters: vec!["home-a".to_string(), "neighborhood-1".to_string()],
                        home_locality_bias: 0.9,
                        neighborhood_locality_bias: 0.7,
                    },
                ),
                InjectionAction::AdaptivePrivacyTransition(
                    AdaptivePrivacyTransition::EstablishAnonymousPath {
                        path_id: "path-a".to_string(),
                        initiator: "alice".to_string(),
                        destination: "bob".to_string(),
                        hops: vec![
                            "relay-1".to_string(),
                            "relay-2".to_string(),
                            "relay-3".to_string(),
                        ],
                        ttl_ticks: 5,
                        reusable: true,
                    },
                ),
                InjectionAction::AdaptivePrivacyTransition(
                    AdaptivePrivacyTransition::ReuseEstablishedPath {
                        path_id: "path-a".to_string(),
                    },
                ),
                InjectionAction::AdaptivePrivacyTransition(
                    AdaptivePrivacyTransition::RecordEstablishFlow {
                        flow_id: "flow-a".to_string(),
                        source: "alice".to_string(),
                        destination: "bob".to_string(),
                        path_id: Some("path-a".to_string()),
                    },
                ),
                InjectionAction::AdaptivePrivacyTransition(
                    AdaptivePrivacyTransition::RecordMoveBatch {
                        batch_id: "batch-a".to_string(),
                        envelope_count: 3,
                    },
                ),
                InjectionAction::AdaptivePrivacyTransition(
                    AdaptivePrivacyTransition::ObserveLocalHealth {
                        provider: "provider-a".to_string(),
                        score: 0.87,
                        latency_ms: 24,
                    },
                ),
                InjectionAction::AdaptivePrivacyTransition(
                    AdaptivePrivacyTransition::RecordCoverTraffic {
                        provider: "provider-a".to_string(),
                        envelope_count: 2,
                    },
                ),
                InjectionAction::AdaptivePrivacyTransition(
                    AdaptivePrivacyTransition::RecordAccountabilityReply {
                        reply_id: "reply-a".to_string(),
                        deadline_ticks: 3,
                        completed_after_ticks: Some(2),
                    },
                ),
                InjectionAction::AdaptivePrivacyTransition(
                    AdaptivePrivacyTransition::RecordRouteDiversity {
                        selector_id: "selector-a".to_string(),
                        unique_paths: 3,
                        dominant_provider: Some("provider-a".to_string()),
                    },
                ),
                InjectionAction::AdaptivePrivacyTransition(
                    AdaptivePrivacyTransition::RecordHonestHopCompromise {
                        path_id: "path-a".to_string(),
                        compromised_hops: vec!["relay-1".to_string()],
                        honest_hops_remaining: 2,
                    },
                ),
                InjectionAction::AdaptivePrivacyTransition(
                    AdaptivePrivacyTransition::RecordPartitionHealCycle {
                        cycle_id: "cycle-a".to_string(),
                        partition_groups: vec![
                            vec!["alice".to_string(), "carol".to_string()],
                            vec!["bob".to_string()],
                        ],
                        heal_after_ticks: 4,
                    },
                ),
                InjectionAction::AdaptivePrivacyTransition(
                    AdaptivePrivacyTransition::RecordChurnBurst {
                        burst_id: "churn-a".to_string(),
                        affected_participants: vec![
                            "alice".to_string(),
                            "bob".to_string(),
                            "carol".to_string(),
                        ],
                        entering: 2,
                        leaving: 1,
                    },
                ),
                InjectionAction::AdaptivePrivacyTransition(
                    AdaptivePrivacyTransition::RecordProviderSaturation {
                        provider: "provider-a".to_string(),
                        queue_depth: 12,
                        utilization: 0.94,
                    },
                ),
                InjectionAction::AdaptivePrivacyTransition(
                    AdaptivePrivacyTransition::RecordHeldObjectRetention {
                        object_id: "held-a".to_string(),
                        selector: "selector:alpha".to_string(),
                        retention_ticks: 6,
                        seeded_from_move: false,
                    },
                ),
                InjectionAction::AdaptivePrivacyTransition(
                    AdaptivePrivacyTransition::RecordSelectorRetrieval {
                        retrieval_id: "retrieval-a".to_string(),
                        selector: "selector:alpha".to_string(),
                        expected_objects: 2,
                        sync_profile: "sparse".to_string(),
                    },
                ),
                InjectionAction::AdaptivePrivacyTransition(
                    AdaptivePrivacyTransition::RecordSyncOpportunity {
                        profile_id: "sync-sparse".to_string(),
                        density: SyncOpportunityDensity::Sparse,
                        peers: vec!["alice".to_string(), "bob".to_string()],
                    },
                ),
                InjectionAction::AdaptivePrivacyTransition(
                    AdaptivePrivacyTransition::RecordMoveToHoldSeed {
                        batch_id: "batch-a".to_string(),
                        object_id: "held-a".to_string(),
                        selector: "selector:alpha".to_string(),
                    },
                ),
            ],
            trigger: TriggerCondition::Immediate,
            duration: Some(Duration::from_secs(5)),
            priority: 10,
        })
        .expect("register scenario");

    handler
        .trigger_scenario("adaptive_privacy_surface")
        .expect("trigger adaptive privacy scenario");

    let movement_profiles = handler
        .inspect_state("adaptive_privacy", "movement_profiles")
        .await
        .expect("movement profiles")
        .downcast::<usize>()
        .expect("movement type");
    assert_eq!(*movement_profiles, 1);

    let active_paths = handler
        .inspect_state("adaptive_privacy", "active_anonymous_paths")
        .await
        .expect("active paths")
        .downcast::<usize>()
        .expect("active path type");
    assert_eq!(*active_paths, 1);

    let reuse_count = handler
        .inspect_state("adaptive_privacy", "path:path-a:reuse_count")
        .await
        .expect("path reuse")
        .downcast::<u64>()
        .expect("reuse type");
    assert_eq!(*reuse_count, 1);

    let establish_flows = handler
        .inspect_state("adaptive_privacy", "establish_flows")
        .await
        .expect("establish flows")
        .downcast::<usize>()
        .expect("establish flows type");
    assert_eq!(*establish_flows, 1);

    let move_batches = handler
        .inspect_state("adaptive_privacy", "move_batches")
        .await
        .expect("move batches")
        .downcast::<usize>()
        .expect("move batches type");
    assert_eq!(*move_batches, 1);

    let local_health = handler
        .inspect_state("adaptive_privacy", "local_health_observations")
        .await
        .expect("local health")
        .downcast::<usize>()
        .expect("local health type");
    assert_eq!(*local_health, 1);

    let cover_events = handler
        .inspect_state("adaptive_privacy", "cover_events")
        .await
        .expect("cover events")
        .downcast::<usize>()
        .expect("cover type");
    assert_eq!(*cover_events, 1);

    let accountability_replies = handler
        .inspect_state("adaptive_privacy", "accountability_replies")
        .await
        .expect("accountability replies")
        .downcast::<usize>()
        .expect("accountability type");
    assert_eq!(*accountability_replies, 1);

    let within_deadline = handler
        .inspect_state("adaptive_privacy", "reply:reply-a:within_deadline")
        .await
        .expect("reply deadline state")
        .downcast::<bool>()
        .expect("reply deadline type");
    assert!(*within_deadline);

    let route_diversity = handler
        .inspect_state("adaptive_privacy", "route_diversity")
        .await
        .expect("route diversity")
        .downcast::<usize>()
        .expect("route diversity type");
    assert_eq!(*route_diversity, 1);

    let honest_hop_compromise_patterns = handler
        .inspect_state("adaptive_privacy", "honest_hop_compromise_patterns")
        .await
        .expect("compromise patterns")
        .downcast::<usize>()
        .expect("compromise pattern type");
    assert_eq!(*honest_hop_compromise_patterns, 1);

    let partition_cycles = handler
        .inspect_state("adaptive_privacy", "partition_heal_cycles")
        .await
        .expect("partition cycles")
        .downcast::<usize>()
        .expect("partition cycle type");
    assert_eq!(*partition_cycles, 1);

    let churn_bursts = handler
        .inspect_state("adaptive_privacy", "churn_bursts")
        .await
        .expect("churn bursts")
        .downcast::<usize>()
        .expect("churn type");
    assert_eq!(*churn_bursts, 1);

    let provider_saturation = handler
        .inspect_state("adaptive_privacy", "provider_saturation")
        .await
        .expect("provider saturation")
        .downcast::<usize>()
        .expect("provider saturation type");
    assert_eq!(*provider_saturation, 1);

    let held_objects = handler
        .inspect_state("adaptive_privacy", "held_objects")
        .await
        .expect("held objects")
        .downcast::<usize>()
        .expect("held object type");
    assert_eq!(*held_objects, 1);

    let seeded_from_move = handler
        .inspect_state("adaptive_privacy", "held:held-a:seeded_from_move")
        .await
        .expect("move to hold seed")
        .downcast::<bool>()
        .expect("seeded from move type");
    assert!(*seeded_from_move);

    let selector_retrievals = handler
        .inspect_state("adaptive_privacy", "selector_retrievals")
        .await
        .expect("selector retrievals")
        .downcast::<usize>()
        .expect("selector retrieval type");
    assert_eq!(*selector_retrievals, 1);

    let sync_opportunities = handler
        .inspect_state("adaptive_privacy", "sync_opportunities")
        .await
        .expect("sync opportunities")
        .downcast::<usize>()
        .expect("sync opportunity type");
    assert_eq!(*sync_opportunities, 1);

    let move_to_hold_seeds = handler
        .inspect_state("adaptive_privacy", "move_to_hold_seeds")
        .await
        .expect("move to hold seeds")
        .downcast::<usize>()
        .expect("move to hold seed type");
    assert_eq!(*move_to_hold_seeds, 1);

    let environment_mobility = handler
        .inspect_state("environment", "mobility_profiles")
        .await
        .expect("environment mobility")
        .downcast::<usize>()
        .expect("environment mobility type");
    assert_eq!(*environment_mobility, 1);

    let environment_admission = handler
        .inspect_state("environment", "link_admissions")
        .await
        .expect("environment admission")
        .downcast::<usize>()
        .expect("environment admission type");
    assert_eq!(*environment_admission, 1);

    let environment_capabilities = handler
        .inspect_state("environment", "node_capabilities")
        .await
        .expect("environment capabilities")
        .downcast::<usize>()
        .expect("environment capability type");
    assert_eq!(*environment_capabilities, 1);

    let environment_trace_entries = handler
        .inspect_state("environment", "trace_entries")
        .await
        .expect("environment trace entries")
        .downcast::<usize>()
        .expect("environment trace type");
    assert_eq!(*environment_trace_entries, 3);

    let artifacts = handler.capture_environment_artifacts();
    let snapshot = artifacts.snapshot;
    assert_eq!(snapshot.mobility_profiles.len(), 1);
    assert_eq!(snapshot.link_admissions.len(), 1);
    assert_eq!(snapshot.node_capabilities.len(), 1);
    assert_eq!(snapshot.mobility_profiles[0].profile_id, "clustered_social");
    assert_eq!(snapshot.link_admissions[0].density, "sparse");
    assert_eq!(snapshot.node_capabilities[0].provider, "provider-a");

    let trace = artifacts.trace;
    assert_eq!(trace.entries.len(), 3);
}

#[tokio::test]
async fn test_adaptive_privacy_path_and_hold_expiry_follows_simulated_time() {
    let handler = SimulationScenarioHandler::new(123);

    handler
        .register_scenario(ScenarioDefinition {
            id: "adaptive_privacy_expiry".to_string(),
            name: "Adaptive Privacy Expiry".to_string(),
            actions: vec![
                InjectionAction::AdaptivePrivacyTransition(
                    AdaptivePrivacyTransition::EstablishAnonymousPath {
                        path_id: "path-exp".to_string(),
                        initiator: "alice".to_string(),
                        destination: "bob".to_string(),
                        hops: vec!["relay-1".to_string(), "relay-2".to_string()],
                        ttl_ticks: 2,
                        reusable: true,
                    },
                ),
                InjectionAction::AdaptivePrivacyTransition(
                    AdaptivePrivacyTransition::RecordHeldObjectRetention {
                        object_id: "held-exp".to_string(),
                        selector: "selector:exp".to_string(),
                        retention_ticks: 2,
                        seeded_from_move: false,
                    },
                ),
            ],
            trigger: TriggerCondition::Immediate,
            duration: Some(Duration::from_secs(5)),
            priority: 1,
        })
        .expect("register expiry scenario");

    handler
        .trigger_scenario("adaptive_privacy_expiry")
        .expect("trigger expiry scenario");

    handler.wait_ticks(2).expect("advance ticks to expiry");

    let path_expired = handler
        .inspect_state("adaptive_privacy", "path:path-exp:expired")
        .await
        .expect("path expired")
        .downcast::<bool>()
        .expect("path expired type");
    assert!(*path_expired);

    let held_expired = handler
        .inspect_state("adaptive_privacy", "held:held-exp:expired")
        .await
        .expect("held object expired")
        .downcast::<bool>()
        .expect("held object expired type");
    assert!(*held_expired);
}

#[tokio::test]
async fn test_checkpoint_baseline_suites_are_persisted() {
    let handler = SimulationScenarioHandler::new(123);
    let suites = ["consensus", "sync", "recovery", "reconfiguration"];

    for suite in suites {
        aura_core::effects::testing::TestingEffects::create_checkpoint(
            &handler,
            &format!("baseline_{suite}"),
            &format!("baseline {suite}"),
        )
        .await
        .expect("create baseline checkpoint");
    }

    let checkpoint_count = handler
        .inspect_state("simulation", "checkpoint_count")
        .await
        .expect("inspect checkpoint count")
        .downcast::<usize>()
        .expect("checkpoint count type");
    assert_eq!(*checkpoint_count, 4);
}

#[tokio::test]
async fn test_restore_and_continue_from_checkpoint() {
    let handler = SimulationScenarioHandler::new(123);

    handler.wait_ticks(5).expect("advance ticks");
    aura_core::effects::testing::TestingEffects::create_checkpoint(
        &handler,
        "restore_resume",
        "restore resume baseline",
    )
    .await
    .expect("create restore checkpoint");
    handler.wait_ticks(9).expect("advance ticks");

    aura_core::effects::testing::TestingEffects::restore_checkpoint(&handler, "restore_resume")
        .await
        .expect("restore checkpoint");
    let restored_tick = handler
        .inspect_state("simulation", "current_tick")
        .await
        .expect("inspect current tick")
        .downcast::<u64>()
        .expect("tick type");
    assert_eq!(*restored_tick, 5);

    handler.wait_ticks(3).expect("continue after restore");
    let resumed_tick = handler
        .inspect_state("simulation", "current_tick")
        .await
        .expect("inspect resumed tick")
        .downcast::<u64>()
        .expect("tick type");
    assert_eq!(*resumed_tick, 8);
}

#[tokio::test]
async fn test_upgrade_resume_from_exported_checkpoint_snapshot() {
    let source = SimulationScenarioHandler::new(123);
    source.wait_ticks(11).expect("advance source ticks");
    aura_core::effects::testing::TestingEffects::create_checkpoint(
        &source,
        "pre_upgrade",
        "pre-upgrade baseline",
    )
    .await
    .expect("create pre-upgrade checkpoint");
    let snapshot = source
        .export_checkpoint_snapshot("pre_upgrade")
        .expect("export snapshot");

    let upgraded = SimulationScenarioHandler::new(999);
    upgraded
        .import_checkpoint_snapshot(snapshot)
        .expect("import snapshot");
    aura_core::effects::testing::TestingEffects::restore_checkpoint(&upgraded, "pre_upgrade")
        .await
        .expect("restore imported checkpoint");
    let upgraded_tick = upgraded
        .inspect_state("simulation", "current_tick")
        .await
        .expect("inspect upgraded tick")
        .downcast::<u64>()
        .expect("tick type");
    assert_eq!(*upgraded_tick, 11);

    upgraded.wait_ticks(2).expect("continue upgraded run");
    let resumed_tick = upgraded
        .inspect_state("simulation", "current_tick")
        .await
        .expect("inspect resumed tick")
        .downcast::<u64>()
        .expect("tick type");
    assert_eq!(*resumed_tick, 13);
}

#[test]
fn test_telltale_fault_pattern_builders() {
    let partition = ScenarioDefinition::telltale_network_partition(
        "partition",
        "Network Partition",
        vec![vec!["a".to_string()], vec!["b".to_string()]],
        Duration::from_secs(5),
    );
    assert!(matches!(
        partition.actions.first(),
        Some(InjectionAction::TriggerFault { fault })
            if matches!(fault.fault, AuraFaultKind::NetworkPartition { .. })
    ));

    let delay = ScenarioDefinition::telltale_message_delay(
        "delay",
        "Delay",
        Duration::from_millis(10),
        Duration::from_millis(50),
    );
    assert!(matches!(
        delay.actions.first(),
        Some(InjectionAction::TriggerFault { fault })
            if matches!(fault.fault, AuraFaultKind::MessageDelay { .. })
    ));

    let drop = ScenarioDefinition::telltale_message_drop("drop", "Drop", 0.5);
    assert!(matches!(
        drop.actions.first(),
        Some(InjectionAction::TriggerFault { fault })
            if matches!(fault.fault, AuraFaultKind::MessageDrop { .. })
    ));

    let node_crash = ScenarioDefinition::telltale_node_crash(
        "crash",
        "Node Crash",
        "coordinator",
        Some(7),
        Some(Duration::from_secs(3)),
    );
    assert!(matches!(
        node_crash.actions.first(),
        Some(InjectionAction::TriggerFault { fault })
            if matches!(fault.fault, AuraFaultKind::NodeCrash { .. })
    ));
    assert!(matches!(node_crash.trigger, TriggerCondition::AtTick(7)));
}

#[test]
fn test_after_step_trigger_activates() {
    let handler = SimulationScenarioHandler::new(321);
    handler
        .register_scenario(ScenarioDefinition {
            id: "after_step".to_string(),
            name: "AfterStep".to_string(),
            actions: vec![],
            trigger: TriggerCondition::AfterStep(5),
            duration: Some(Duration::from_secs(5)),
            priority: 5,
        })
        .expect("register scenario");

    handler.wait_ticks(4).expect("advance ticks");
    let before = handler.get_injection_stats().expect("stats");
    assert_eq!(before.get("total_injections"), Some(&"0".to_string()));

    handler.wait_ticks(1).expect("advance ticks");
    let after = handler.get_injection_stats().expect("stats");
    assert_eq!(after.get("total_injections"), Some(&"1".to_string()));
}

#[test]
fn test_on_event_trigger_activates() {
    let handler = SimulationScenarioHandler::new(654);
    handler
        .register_scenario(ScenarioDefinition {
            id: "on_event".to_string(),
            name: "OnEvent".to_string(),
            actions: vec![],
            trigger: TriggerCondition::OnEvent("network_condition".to_string()),
            duration: Some(Duration::from_secs(5)),
            priority: 5,
        })
        .expect("register scenario");

    handler
        .apply_network_condition("partitioned", vec!["alice".to_string()], 3)
        .expect("apply network condition");
    let stats = handler.get_injection_stats().expect("stats");
    assert_eq!(stats.get("total_injections"), Some(&"1".to_string()));
}
