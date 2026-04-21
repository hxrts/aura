use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;
use crate::config::{
    CompatibilityAction, CompatibilityStep, InstanceConfig, InstanceMode, RunConfig,
    RunSection, ScreenSource,
};
use crate::coordinator::HarnessCoordinator;
use aura_app::ui::contract::{
    ConfirmationState, FieldId, ListId, ListItemSnapshot, ListSnapshot, OperationId,
    OperationInstanceId, OperationSnapshot, OperationState, RuntimeEventId,
    RuntimeEventSnapshot, ScreenId, SelectionSnapshot, UiReadiness, UiSnapshot,
};
use aura_app::ui::scenarios::ScenarioAction;
use aura_app::ui_contract::{
    next_projection_revision, ChannelFactKey, QuiescenceSnapshot, QuiescenceState,
};
use serde_json::json;

#[allow(clippy::disallowed_methods)]
fn unique_test_dir(label: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let suffix = COUNTER.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "aura-harness-{label}-{}-{suffix}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root)
        .unwrap_or_else(|error| panic!("create temp test dir failed: {error}"));
    root
}

fn run_report_once(run: &RunConfig, scenario: &ScenarioConfig) -> ScenarioReport {
    let mut tool_api = ToolApi::new(
        HarnessCoordinator::from_run_config(run).unwrap_or_else(|error| panic!("{error}")),
    );
    if let Err(error) = tool_api.start_all() {
        panic!("start_all failed: {error}");
    }
    let report = ScenarioExecutor::new(ExecutionMode::Compatibility)
        .execute(scenario, &mut tool_api)
        .unwrap_or_else(|error| panic!("execute failed: {error}"));
    if let Err(error) = tool_api.stop_all() {
        panic!("stop_all failed: {error}");
    }
    report
}

fn test_scenario_config(
    id: &str,
    goal: &str,
    compatibility_steps: Vec<CompatibilityStep>,
) -> ScenarioConfig {
    ScenarioConfig {
        schema_version: 1,
        id: id.to_string(),
        goal: goal.to_string(),
        classification: None,
        execution_mode: Some("compatibility".to_string()),
        required_capabilities: vec![],
        compatibility_steps,
        semantic_steps: Vec::new(),
    }
}

fn test_ui_operation_handle(
    operation_id: OperationId,
    instance_id: OperationInstanceId,
) -> UiOperationHandle {
    serde_json::from_value(json!({
        "id": operation_id,
        "instance_id": instance_id,
    }))
    .unwrap_or_else(|error| panic!("failed to build test ui operation handle: {error}"))
}

#[test]
fn shared_semantic_lane_rejects_raw_ui_requests() {
    for request in [
        ToolRequest::SendKeys {
            instance_id: "alice".to_string(),
            keys: "hello".to_string(),
        },
        ToolRequest::SendKey {
            instance_id: "alice".to_string(),
            key: ToolKey::Enter,
            repeat: 1,
        },
        ToolRequest::ActivateControl {
            instance_id: "alice".to_string(),
            control_id: ControlId::NavChat,
        },
        ToolRequest::ActivateListItem {
            instance_id: "alice".to_string(),
            list_id: ListId::Channels,
            item_id: "channel-1".to_string(),
        },
        ToolRequest::ClickButton {
            instance_id: "alice".to_string(),
            label: "submit".to_string(),
            selector: None,
        },
        ToolRequest::FillInput {
            instance_id: "alice".to_string(),
            selector: "#aura-input".to_string(),
            value: "value".to_string(),
        },
        ToolRequest::FillField {
            instance_id: "alice".to_string(),
            field_id: FieldId::ChatInput,
            value: "value".to_string(),
        },
    ] {
        assert!(
            shared_semantic_raw_ui_request(&request),
            "shared semantic lane must reject raw request {request:?}"
        );
    }
}

#[test]
fn shared_semantic_lane_allows_observation_and_semantic_adjacent_requests() {
    for request in [
        ToolRequest::UiState {
            instance_id: "alice".to_string(),
        },
        ToolRequest::Screen {
            instance_id: "alice".to_string(),
            screen_source: ScreenSource::Default,
        },
        ToolRequest::GetAuthorityId {
            instance_id: "alice".to_string(),
        },
        ToolRequest::DiagnosticListChannels {
            instance_id: "alice".to_string(),
        },
        ToolRequest::DiagnosticCurrentSelection {
            instance_id: "alice".to_string(),
        },
        ToolRequest::ReadClipboard {
            instance_id: "alice".to_string(),
        },
        ToolRequest::Restart {
            instance_id: "alice".to_string(),
        },
    ] {
        assert!(
            !shared_semantic_raw_ui_request(&request),
            "shared semantic lane should allow non-raw request {request:?}"
        );
    }
}

#[test]
fn shared_open_screen_and_settings_use_semantic_submission_not_raw_ui() {
    let source = include_str!("executor.rs");
    let (_, semantic_body) = source
        .split_once("fn execute_semantic_step(")
        .unwrap_or_else(|| panic!("missing execute_semantic_step"));
    let semantic_body = semantic_body
        .split_once("fn semantic_metadata_step(")
        .map(|(body, _)| body)
        .unwrap_or_else(|| panic!("missing semantic step end"));
    let (open_screen_prefix, open_settings_and_rest) = semantic_body
        .split_once("SemanticAction::Intent(IntentAction::OpenSettingsSection(section)) => {")
        .unwrap_or_else(|| panic!("missing open settings branch"));
    let open_screen_branch = open_screen_prefix
        .split_once("SemanticAction::Intent(IntentAction::OpenScreen { screen, .. }) => {")
        .map(|(_, branch)| branch)
        .unwrap_or_else(|| panic!("missing open screen branch"));
    let open_settings_branch = open_settings_and_rest
        .split_once("SemanticAction::Variables(variable) =>")
        .map(|(branch, _)| branch)
        .unwrap_or_else(|| panic!("missing variables branch after open settings"));
    assert!(
        open_screen_branch.contains("let open_intent = IntentAction::OpenScreen {"),
        "shared OpenScreen branch must submit the semantic intent"
    );
    assert!(
        open_settings_branch.contains("IntentAction::OpenSettingsSection(*section)"),
        "shared OpenSettingsSection branch must submit the semantic intent"
    );
    assert!(
        !open_screen_branch
            .contains("plan_activate_control_request(&instance_id, nav_control_id_for_screen(*screen_id))"),
        "shared OpenScreen branch must not fall back to raw control activation"
    );
    assert!(
        !open_settings_branch.contains(
            "ToolRequest::ActivateListItem {\n                    instance_id: instance_id.clone(),\n                    list_id: ListId::SettingsSections,"
        ),
        "shared OpenSettingsSection branch must not fall back to raw list activation"
    );
}

#[test]
fn canonical_trace_parity_ignores_actor_ids_and_revisions() {
    let local = vec![
        CanonicalTraceEvent::ActionRequested {
            request: SharedActionRequest {
                actor: ActorId("alice".to_string()),
                intent: IntentAction::JoinChannel {
                    channel_name: "shared".to_string(),
                },
                contract: IntentAction::JoinChannel {
                    channel_name: "shared".to_string(),
                }
                .contract(),
            },
            observed_revision: Some(UiSnapshot::loading(ScreenId::Chat).revision),
        },
        CanonicalTraceEvent::ActionSucceeded {
            fact: TerminalSuccessFact {
                handle: SharedActionHandle {
                    action_id: SharedActionId("alice-join".to_string()),
                    actor: ActorId("alice".to_string()),
                    intent: IntentAction::JoinChannel {
                        channel_name: "shared".to_string(),
                    }
                    .kind(),
                    contract: IntentAction::JoinChannel {
                        channel_name: "shared".to_string(),
                    }
                    .contract(),
                    baseline_revision: None,
                },
                success: TerminalSuccessKind::RuntimeEvent(RuntimeEventKind::ChannelJoined),
                observed_revision: None,
            },
        },
    ];
    let peer = vec![
        CanonicalTraceEvent::ActionRequested {
            request: SharedActionRequest {
                actor: ActorId("bob".to_string()),
                intent: IntentAction::JoinChannel {
                    channel_name: "shared".to_string(),
                },
                contract: IntentAction::JoinChannel {
                    channel_name: "shared".to_string(),
                }
                .contract(),
            },
            observed_revision: None,
        },
        CanonicalTraceEvent::ActionSucceeded {
            fact: TerminalSuccessFact {
                handle: SharedActionHandle {
                    action_id: SharedActionId("bob-join".to_string()),
                    actor: ActorId("bob".to_string()),
                    intent: IntentAction::JoinChannel {
                        channel_name: "shared".to_string(),
                    }
                    .kind(),
                    contract: IntentAction::JoinChannel {
                        channel_name: "shared".to_string(),
                    }
                    .contract(),
                    baseline_revision: Some(UiSnapshot::loading(ScreenId::Chat).revision),
                },
                success: TerminalSuccessKind::RuntimeEvent(RuntimeEventKind::ChannelJoined),
                observed_revision: Some(UiSnapshot::loading(ScreenId::Neighborhood).revision),
            },
        },
    ];

    compare_canonical_traces_for_parity(&local, &peer)
        .unwrap_or_else(|error| panic!("trace parity should hold: {error}"));
}

#[test]
fn canonical_trace_parity_rejects_shape_mismatch() {
    let local = vec![CanonicalTraceEvent::ActionSucceeded {
        fact: TerminalSuccessFact {
            handle: SharedActionHandle {
                action_id: SharedActionId("alice-send".to_string()),
                actor: ActorId("alice".to_string()),
                intent: IntentAction::SendChatMessage {
                    message: "hello".to_string(),
                    channel_id: None,
                    context_id: None,
                }
                .kind(),
                contract: IntentAction::SendChatMessage {
                    message: "hello".to_string(),
                    channel_id: None,
                    context_id: None,
                }
                .contract(),
                baseline_revision: None,
            },
            success: TerminalSuccessKind::RuntimeEvent(RuntimeEventKind::MessageCommitted),
            observed_revision: None,
        },
    }];
    let peer = vec![CanonicalTraceEvent::ActionSucceeded {
        fact: TerminalSuccessFact {
            handle: SharedActionHandle {
                action_id: SharedActionId("bob-send".to_string()),
                actor: ActorId("bob".to_string()),
                intent: IntentAction::SendChatMessage {
                    message: "hello".to_string(),
                    channel_id: None,
                    context_id: None,
                }
                .kind(),
                contract: IntentAction::SendChatMessage {
                    message: "hello".to_string(),
                    channel_id: None,
                    context_id: None,
                }
                .contract(),
                baseline_revision: None,
            },
            success: TerminalSuccessKind::Readiness(UiReadiness::Ready),
            observed_revision: None,
        },
    }];

    let error = compare_canonical_traces_for_parity(&local, &peer)
        .err()
        .unwrap_or_else(|| panic!("trace mismatch must fail"));
    assert!(error.to_string().contains("canonical trace mismatch"));
}

#[test]
fn action_preconditions_fail_diagnostically_before_issue() {
    let snapshot = UiSnapshot::loading(ScreenId::Chat);
    let failures = unsatisfied_action_preconditions(
        &IntentAction::SendChatMessage {
            message: "hello".to_string(),
            channel_id: None,
            context_id: None,
        }
        .contract(),
        &snapshot,
    );
    assert!(
        failures
            .iter()
            .any(|failure| failure.contains("readiness=")),
        "expected readiness failure, got {failures:?}"
    );
    assert!(
        failures
            .iter()
            .all(|failure| !failure.contains("quiescence=")),
        "unexpected quiescence failure, got {failures:?}"
    );
    assert!(
        !failures
            .iter()
            .any(|failure| failure.contains("runtime_event=")),
        "unexpected runtime-event failure, got {failures:?}"
    );
}

#[test]
fn action_precondition_wait_step_tracks_all_declared_preconditions() {
    let step = SemanticStep {
        id: "wait-before-remove-device".to_string(),
        action: ScenarioAction::Intent(IntentAction::RemoveSelectedDevice { device_id: None }),
        actor: Some(ActorId("bob".to_string())),
        timeout_ms: Some(4000),
    };
    let wait_step = action_precondition_wait_step(
        &step,
        &IntentAction::RemoveSelectedDevice { device_id: None }.contract(),
    );

    assert!(matches!(wait_step.action, CompatibilityAction::WaitFor));
    assert_eq!(wait_step.screen_id, Some(ScreenId::Settings));
    assert_eq!(wait_step.readiness, Some(UiReadiness::Ready));
    assert_eq!(wait_step.quiescence, Some(QuiescenceState::Settled));
    assert_eq!(wait_step.instance.as_deref(), Some("bob"));
}

#[test]
fn action_precondition_wait_success_returns_without_bailing() {
    let source = include_str!("executor.rs");
    let anchor = "if let Err(wait_error) =\n        wait_for_semantic_state(&wait_step, tool_api, context, &instance_id, timeout_ms)\n    {";
    let start = source
        .find(anchor)
        .unwrap_or_else(|| panic!("missing precondition wait block"));
    let tail = &source[start..];
    let expected_tail = "    Ok(())\n}";
    assert!(
        tail.contains(expected_tail),
        "precondition wait block must return Ok(()) after successful waits"
    );
}

#[test]
fn semantic_wait_can_require_confirmed_list_items() {
    let step = crate::config::CompatibilityStep {
        id: "wait-confirmed-contact".to_string(),
        action: crate::config::CompatibilityAction::WaitFor,
        list_id: Some(ListId::Contacts),
        item_id: Some("authority-1".to_string()),
        confirmation: Some(ConfirmationState::Confirmed),
        ..Default::default()
    };
    let snapshot = UiSnapshot {
        screen: ScreenId::Contacts,
        focused_control: None,
        open_modal: None,
        readiness: UiReadiness::Ready,
        revision: next_projection_revision(None),
        quiescence: QuiescenceSnapshot::settled(),
        selections: vec![SelectionSnapshot {
            list: ListId::Contacts,
            item_id: "authority-1".to_string(),
        }],
        lists: vec![ListSnapshot {
            id: ListId::Contacts,
            items: vec![ListItemSnapshot {
                id: "authority-1".to_string(),
                selected: true,
                confirmation: ConfirmationState::Confirmed,
                is_current: false,
            }],
        }],
        messages: Vec::new(),
        operations: Vec::new(),
        toasts: Vec::new(),
        runtime_events: Vec::new(),
    };

    assert!(semantic_wait_matches(&step, &snapshot));
}

#[test]
fn semantic_expectation_wait_step_resolves_template_backed_selection_ids() {
    let step = SemanticStep {
        id: "wait-current-authority".to_string(),
        action: ScenarioAction::Expect(Expectation::SelectionIs {
            list: ListId::Authorities,
            item_id: "${alice_authority_id}".to_string(),
        }),
        actor: Some(ActorId("alice".to_string())),
        timeout_ms: Some(4000),
    };
    let mut context = ScenarioContext::default();
    context.vars.insert(
        "alice_authority_id".to_string(),
        "authority-1234".to_string(),
    );

    let wait_step = semantic_expectation_wait_step(
        &step,
        match &step.action {
            ScenarioAction::Expect(expectation) => expectation,
            _ => unreachable!("test step uses an expectation action"),
        },
        &context,
    )
    .unwrap_or_else(|error| panic!("selection wait should resolve templates: {error}"));

    assert_eq!(wait_step.list_id, Some(ListId::Authorities));
    assert_eq!(wait_step.item_id.as_deref(), Some("authority-1234"));
}

#[test]
fn semantic_wait_for_instance_requires_list_count_match() {
    let step = crate::config::CompatibilityStep {
        id: "wait-two-devices".to_string(),
        action: crate::config::CompatibilityAction::WaitFor,
        list_id: Some(ListId::Devices),
        count: Some(2),
        ..Default::default()
    };
    let snapshot = UiSnapshot {
        screen: ScreenId::Settings,
        focused_control: None,
        open_modal: None,
        readiness: UiReadiness::Ready,
        revision: ProjectionRevision {
            semantic_seq: 1,
            render_seq: None,
        },
        quiescence: QuiescenceSnapshot::settled(),
        selections: Vec::new(),
        lists: vec![ListSnapshot {
            id: ListId::Devices,
            items: vec![ListItemSnapshot {
                id: "device:current".to_string(),
                selected: false,
                confirmation: ConfirmationState::Confirmed,
                is_current: true,
            }],
        }],
        messages: Vec::new(),
        operations: Vec::new(),
        toasts: Vec::new(),
        runtime_events: Vec::new(),
    };

    assert!(!semantic_wait_matches_for_instance(
        &step,
        &snapshot,
        &ScenarioContext::default(),
        "bob"
    ));
}

#[test]
fn semantic_wait_rejects_pending_local_when_confirmed_is_required() {
    let step = crate::config::CompatibilityStep {
        id: "wait-confirmed-contact".to_string(),
        action: crate::config::CompatibilityAction::WaitFor,
        list_id: Some(ListId::Contacts),
        item_id: Some("authority-1".to_string()),
        confirmation: Some(ConfirmationState::Confirmed),
        ..Default::default()
    };
    let snapshot = UiSnapshot {
        screen: ScreenId::Contacts,
        focused_control: None,
        open_modal: None,
        readiness: UiReadiness::Ready,
        revision: next_projection_revision(None),
        quiescence: QuiescenceSnapshot::settled(),
        selections: vec![SelectionSnapshot {
            list: ListId::Contacts,
            item_id: "authority-1".to_string(),
        }],
        lists: vec![ListSnapshot {
            id: ListId::Contacts,
            items: vec![ListItemSnapshot {
                id: "authority-1".to_string(),
                selected: true,
                confirmation: ConfirmationState::PendingLocal,
                is_current: false,
            }],
        }],
        messages: Vec::new(),
        operations: Vec::new(),
        toasts: Vec::new(),
        runtime_events: Vec::new(),
    };

    assert!(!semantic_wait_matches(&step, &snapshot));
}

#[test]
fn semantic_wait_can_require_ready_state() {
    let step = crate::config::CompatibilityStep {
        id: "wait-ready".to_string(),
        action: crate::config::CompatibilityAction::WaitFor,
        readiness: Some(UiReadiness::Ready),
        ..Default::default()
    };
    let snapshot = UiSnapshot {
        screen: ScreenId::Neighborhood,
        focused_control: None,
        open_modal: None,
        readiness: UiReadiness::Ready,
        revision: next_projection_revision(None),
        quiescence: QuiescenceSnapshot::settled(),
        selections: Vec::new(),
        lists: Vec::new(),
        messages: Vec::new(),
        operations: Vec::new(),
        toasts: Vec::new(),
        runtime_events: Vec::new(),
    };

    assert!(semantic_wait_matches(&step, &snapshot));
}

#[test]
fn semantic_wait_can_require_operation_state() {
    let step = crate::config::CompatibilityStep {
        id: "wait-op".to_string(),
        action: crate::config::CompatibilityAction::WaitFor,
        operation_id: Some(OperationId::invitation_accept_contact()),
        operation_state: Some(OperationState::Succeeded),
        ..Default::default()
    };
    let snapshot = UiSnapshot {
        screen: ScreenId::Contacts,
        focused_control: None,
        open_modal: None,
        readiness: UiReadiness::Ready,
        revision: next_projection_revision(None),
        quiescence: QuiescenceSnapshot::settled(),
        selections: Vec::new(),
        lists: Vec::new(),
        messages: Vec::new(),
        operations: vec![OperationSnapshot {
            id: OperationId::invitation_accept_contact(),
            instance_id: OperationInstanceId("test-operation-instance".to_string()),
            state: OperationState::Succeeded,
        }],
        toasts: Vec::new(),
        runtime_events: Vec::new(),
    };

    assert!(semantic_wait_matches(&step, &snapshot));
}

#[test]
fn semantic_wait_operation_state_uses_recorded_handle_for_instance() {
    let step = crate::config::CompatibilityStep {
        id: "wait-op-handle".to_string(),
        action: crate::config::CompatibilityAction::WaitFor,
        operation_id: Some(OperationId::invitation_accept_contact()),
        operation_state: Some(OperationState::Succeeded),
        ..Default::default()
    };
    let snapshot = UiSnapshot {
        screen: ScreenId::Contacts,
        focused_control: None,
        open_modal: None,
        readiness: UiReadiness::Ready,
        revision: next_projection_revision(None),
        quiescence: QuiescenceSnapshot::settled(),
        selections: Vec::new(),
        lists: Vec::new(),
        messages: Vec::new(),
        operations: vec![
            OperationSnapshot {
                id: OperationId::invitation_accept_contact(),
                instance_id: OperationInstanceId("stale-instance".to_string()),
                state: OperationState::Failed,
            },
            OperationSnapshot {
                id: OperationId::invitation_accept_contact(),
                instance_id: OperationInstanceId("fresh-instance".to_string()),
                state: OperationState::Succeeded,
            },
        ],
        toasts: Vec::new(),
        runtime_events: Vec::new(),
    };
    let mut context = ScenarioContext::default();
    context.last_operation_handle.insert(
        "alice".to_string(),
        test_ui_operation_handle(
            OperationId::invitation_accept_contact(),
            OperationInstanceId("fresh-instance".to_string()),
        ),
    );

    assert!(
        !semantic_wait_matches(&step, &snapshot),
        "generic operation-id matching should still see the stale first instance"
    );
    assert!(
        semantic_wait_matches_for_instance(&step, &snapshot, &context, "alice"),
        "handle-aware matching must follow the recorded instance instead of the first matching operation id"
    );
}

#[test]
fn operation_handle_match_requires_matching_instance_and_state() {
    let handle = test_ui_operation_handle(
        OperationId::invitation_accept_contact(),
        OperationInstanceId("handle-instance".to_string()),
    );
    let matching_snapshot = UiSnapshot {
        screen: ScreenId::Contacts,
        focused_control: None,
        open_modal: None,
        readiness: UiReadiness::Ready,
        revision: next_projection_revision(None),
        quiescence: QuiescenceSnapshot::settled(),
        selections: Vec::new(),
        lists: Vec::new(),
        messages: Vec::new(),
        operations: vec![OperationSnapshot {
            id: OperationId::invitation_accept_contact(),
            instance_id: OperationInstanceId("handle-instance".to_string()),
            state: OperationState::Succeeded,
        }],
        toasts: Vec::new(),
        runtime_events: Vec::new(),
    };
    let wrong_instance_snapshot = UiSnapshot {
        operations: vec![OperationSnapshot {
            id: OperationId::invitation_accept_contact(),
            instance_id: OperationInstanceId("other-instance".to_string()),
            state: OperationState::Succeeded,
        }],
        ..matching_snapshot.clone()
    };
    let wrong_state_snapshot = UiSnapshot {
        operations: vec![OperationSnapshot {
            id: OperationId::invitation_accept_contact(),
            instance_id: OperationInstanceId("handle-instance".to_string()),
            state: OperationState::Failed,
        }],
        ..matching_snapshot.clone()
    };

    assert!(operation_handle_matches(
        &matching_snapshot,
        &handle,
        OperationState::Succeeded
    ));
    assert!(!operation_handle_matches(
        &wrong_instance_snapshot,
        &handle,
        OperationState::Succeeded
    ));
    assert!(!operation_handle_matches(
        &wrong_state_snapshot,
        &handle,
        OperationState::Succeeded
    ));
}

#[test]
fn escape_insert_guard_only_for_single_non_control_keys() {
    assert!(should_escape_insert_before_send_keys("r"));
    assert!(should_escape_insert_before_send_keys("3"));
    assert!(!should_escape_insert_before_send_keys("\n"));
    assert!(!should_escape_insert_before_send_keys("\u{1b}"));
    assert!(!should_escape_insert_before_send_keys("hi"));
}

#[test]
fn toast_contains_aliases_retry_variants() {
    assert!(toast_contains_matches(
        "No message selected",
        "Retrying message…"
    ));
    assert!(toast_contains_matches(
        "Neighborhood",
        "neighborhood updated"
    ));
    assert!(toast_contains_matches(
        "MFA requires at least 2 devices",
        "Cannot configure multifactor: requires at least 2 devices"
    ));
    assert!(!toast_contains_matches(
        "No message selected",
        "Invitation Created"
    ));
}

#[test]
fn compatibility_and_agent_modes_share_same_transition_path() {
    let temp_root = unique_test_dir("executor-test");

    let run = RunConfig {
        schema_version: 1,
        run: RunSection {
            name: "executor-test".to_string(),
            pty_rows: Some(40),
            pty_cols: Some(120),
            artifact_dir: Some(temp_root.join("artifacts")),
            global_budget_ms: None,
            step_budget_ms: None,
            seed: Some(5),
            max_cpu_percent: None,
            max_memory_bytes: None,
            max_open_files: None,
            require_remote_artifact_sync: false,
            runtime_substrate: crate::config::RuntimeSubstrate::default(),
        },
        instances: vec![InstanceConfig {
            id: "alice".to_string(),
            mode: InstanceMode::Local,
            data_dir: temp_root,
            device_id: None,
            bind_address: "127.0.0.1:45001".to_string(),
            demo_mode: false,
            command: Some("bash".to_string()),
            args: vec!["-lc".to_string(), "cat".to_string()],
            env: vec![],
            log_path: None,
            ssh_host: None,
            ssh_user: None,
            ssh_port: None,
            ssh_strict_host_key_checking: true,
            ssh_known_hosts_file: None,
            ssh_fingerprint: None,
            ssh_require_fingerprint: false,
            ssh_dry_run: true,
            remote_workdir: None,
            lan_discovery: None,
            tunnel: None,
        }],
    };

    let scenario = test_scenario_config(
        "executor-smoke",
        "verify transitions",
        vec![CompatibilityStep {
            id: "step-1".to_string(),
            action: CompatibilityAction::LaunchInstances,
            instance: None,
            timeout_ms: None,
            ..Default::default()
        }],
    );

    let mut compatibility_api = ToolApi::new(
        HarnessCoordinator::from_run_config(&run).unwrap_or_else(|error| panic!("{error}")),
    );
    if let Err(error) = compatibility_api.start_all() {
        panic!("start_all failed: {error}");
    }
    let compatibility = ScenarioExecutor::new(ExecutionMode::Compatibility)
        .execute(&scenario, &mut compatibility_api)
        .unwrap_or_else(|error| panic!("compatibility execute failed: {error}"));
    if let Err(error) = compatibility_api.stop_all() {
        panic!("stop_all failed: {error}");
    }

    let mut agent_api = ToolApi::new(
        HarnessCoordinator::from_run_config(&run).unwrap_or_else(|error| panic!("{error}")),
    );
    if let Err(error) = agent_api.start_all() {
        panic!("start_all failed: {error}");
    }
    let agent = ScenarioExecutor::new(ExecutionMode::Agent)
        .execute(&scenario, &mut agent_api)
        .unwrap_or_else(|error| panic!("agent execute failed: {error}"));
    if let Err(error) = agent_api.stop_all() {
        panic!("stop_all failed: {error}");
    }

    assert_eq!(compatibility.states_visited, agent.states_visited);
}

#[test]
fn repeated_runs_with_same_seed_share_same_report_shape() {
    let temp_root = unique_test_dir("determinism-test");

    let run = RunConfig {
        schema_version: 1,
        run: RunSection {
            name: "executor-determinism".to_string(),
            pty_rows: Some(40),
            pty_cols: Some(120),
            artifact_dir: Some(temp_root.join("artifacts")),
            global_budget_ms: None,
            step_budget_ms: None,
            seed: Some(11),
            max_cpu_percent: None,
            max_memory_bytes: None,
            max_open_files: None,
            require_remote_artifact_sync: false,
            runtime_substrate: crate::config::RuntimeSubstrate::default(),
        },
        instances: vec![InstanceConfig {
            id: "alice".to_string(),
            mode: InstanceMode::Local,
            data_dir: temp_root,
            device_id: None,
            bind_address: "127.0.0.1:45011".to_string(),
            demo_mode: false,
            command: Some("bash".to_string()),
            args: vec!["-lc".to_string(), "cat".to_string()],
            env: vec![],
            log_path: None,
            ssh_host: None,
            ssh_user: None,
            ssh_port: None,
            ssh_strict_host_key_checking: true,
            ssh_known_hosts_file: None,
            ssh_fingerprint: None,
            ssh_require_fingerprint: false,
            ssh_dry_run: true,
            remote_workdir: None,
            lan_discovery: None,
            tunnel: None,
        }],
    };

    let scenario = test_scenario_config(
        "executor-determinism",
        "verify repeated harness determinism",
        vec![CompatibilityStep {
            id: "step-1".to_string(),
            action: CompatibilityAction::LaunchInstances,
            instance: None,
            timeout_ms: None,
            ..Default::default()
        }],
    );

    let first = run_report_once(&run, &scenario);
    let second = run_report_once(&run, &scenario);

    assert_eq!(first.scenario_id, second.scenario_id);
    assert_eq!(first.execution_mode, second.execution_mode);
    assert_eq!(first.states_visited, second.states_visited);
    assert_eq!(first.transitions, second.transitions);
    assert_eq!(first.canonical_trace, second.canonical_trace);
    assert_eq!(first.completed, second.completed);
}

#[test]
fn send_chat_command_dismisses_toast_then_sends_slash_command() {
    let temp_root = unique_test_dir("executor-chat-command");

    let run = RunConfig {
        schema_version: 1,
        run: RunSection {
            name: "executor-chat-command".to_string(),
            pty_rows: Some(40),
            pty_cols: Some(120),
            artifact_dir: Some(temp_root.join("artifacts")),
            global_budget_ms: None,
            step_budget_ms: None,
            seed: Some(7),
            max_cpu_percent: None,
            max_memory_bytes: None,
            max_open_files: None,
            require_remote_artifact_sync: false,
            runtime_substrate: crate::config::RuntimeSubstrate::default(),
        },
        instances: vec![InstanceConfig {
            id: "alice".to_string(),
            mode: InstanceMode::Local,
            data_dir: temp_root,
            device_id: None,
            bind_address: "127.0.0.1:45003".to_string(),
            demo_mode: false,
            command: Some("bash".to_string()),
            args: vec!["-lc".to_string(), "cat".to_string()],
            env: vec![],
            log_path: None,
            ssh_host: None,
            ssh_user: None,
            ssh_port: None,
            ssh_strict_host_key_checking: true,
            ssh_known_hosts_file: None,
            ssh_fingerprint: None,
            ssh_require_fingerprint: false,
            ssh_dry_run: true,
            remote_workdir: None,
            lan_discovery: None,
            tunnel: None,
        }],
    };

    let scenario = test_scenario_config(
        "executor-chat-command",
        "verify chat command action",
        vec![CompatibilityStep {
            id: "step-1".to_string(),
            action: CompatibilityAction::SendChatCommand,
            instance: Some("alice".to_string()),
            command: Some("join slash-lab".to_string()),
            timeout_ms: None,
            ..Default::default()
        }],
    );

    let mut api = ToolApi::new(
        HarnessCoordinator::from_run_config(&run).unwrap_or_else(|error| panic!("{error}")),
    );
    if let Err(error) = api.start_all() {
        panic!("start_all failed: {error}");
    }

    if let Err(error) =
        ScenarioExecutor::new(ExecutionMode::Compatibility).execute(&scenario, &mut api)
    {
        panic!("send_chat_command execute failed: {error}");
    }

    if let Err(error) = api.stop_all() {
        panic!("stop_all failed: {error}");
    }

    let action_log = api.action_log();
    let filtered = action_log
        .iter()
        .filter(|record| !matches!(record.request, ToolRequest::UiState { .. }))
        .collect::<Vec<_>>();
    assert!(
        filtered.len() >= 4,
        "expected at least four non-UiState tool actions"
    );

    match &filtered[0].request {
        ToolRequest::SendKey {
            instance_id,
            key: ToolKey::Esc,
            repeat,
        } => {
            assert_eq!(instance_id, "alice");
            assert_eq!(*repeat, 1);
        }
        other => panic!("expected SendKey(Esc) first, got {other:?}"),
    }

    let mut next_index = 1usize;
    if matches!(
        filtered.get(1).map(|record| &record.request),
        Some(ToolRequest::SendKeys { instance_id, keys })
            if instance_id == "alice" && keys == "2"
    ) {
        match &filtered[2].request {
            ToolRequest::WaitFor {
                instance_id,
                pattern,
                timeout_ms: _,
                ..
            } => {
                assert_eq!(instance_id, "alice");
                assert_eq!(pattern, "Channels");
            }
            other => panic!("expected WaitFor after chat nav, got {other:?}"),
        }
        next_index = 3;
    }

    match &filtered[next_index].request {
        ToolRequest::SendKey {
            instance_id,
            key: ToolKey::Esc,
            repeat,
        } => {
            assert_eq!(instance_id, "alice");
            assert_eq!(*repeat, 1);
        }
        other => panic!("expected SendKey(Esc) before command entry, got {other:?}"),
    }

    match &filtered[next_index + 1].request {
        ToolRequest::SendKeys { instance_id, keys } => {
            assert_eq!(instance_id, "alice");
            assert_eq!(keys, "i");
        }
        other => panic!("expected SendKeys for insert mode, got {other:?}"),
    }

    match &filtered[next_index + 2].request {
        ToolRequest::SendKeys { instance_id, keys } => {
            assert_eq!(instance_id, "alice");
            assert_eq!(keys, "/join slash-lab\n");
        }
        other => panic!("expected SendKeys for slash command, got {other:?}"),
    }
}

#[test]
fn tui_semantic_actions_emit_expected_tool_requests() {
    assert!(matches!(
        plan_activate_control_request("alice", ControlId::NavChat),
        ToolRequest::ActivateControl {
            instance_id,
            control_id: ControlId::NavChat,
        } if instance_id == "alice"
    ));
    assert!(matches!(
        plan_fill_field_request("alice", FieldId::ChatInput, "typed-value".to_string()),
        ToolRequest::FillField {
            instance_id,
            field_id: FieldId::ChatInput,
            value,
        } if instance_id == "alice" && value == "typed-value"
    ));
    assert!(matches!(
        plan_dismiss_transient_request("alice"),
        ToolRequest::SendKey {
            instance_id,
            key: ToolKey::Esc,
            repeat: 1,
        } if instance_id == "alice"
    ));
}

#[test]
fn send_chat_message_uses_tui_insert_sequence() {
    let requests = plan_tui_send_chat_message_request("alice", "hello-semantic");
    assert_eq!(requests.len(), 1);
    assert!(matches!(
        &requests[0],
        ToolRequest::SendKeys { instance_id, keys }
        if instance_id == "alice" && keys == "ihello-semantic\n\x1b"
    ));
}

#[test]
fn send_clipboard_retries_until_clipboard_file_is_written() {
    let temp_root = unique_test_dir("executor-send-clipboard");
    let alice_data = temp_root.join("alice");
    let bob_data = temp_root.join("bob");
    let _ = std::fs::create_dir_all(&alice_data);
    let _ = std::fs::create_dir_all(&bob_data);

    let run = RunConfig {
        schema_version: 1,
        run: RunSection {
            name: "executor-send-clipboard".to_string(),
            pty_rows: Some(40),
            pty_cols: Some(120),
            artifact_dir: Some(temp_root.join("artifacts")),
            global_budget_ms: None,
            step_budget_ms: None,
            seed: Some(8),
            max_cpu_percent: None,
            max_memory_bytes: None,
            max_open_files: None,
            require_remote_artifact_sync: false,
            runtime_substrate: crate::config::RuntimeSubstrate::default(),
        },
        instances: vec![
            InstanceConfig {
                id: "alice".to_string(),
                mode: InstanceMode::Local,
                data_dir: alice_data.clone(),
                device_id: None,
                bind_address: "127.0.0.1:45011".to_string(),
                demo_mode: false,
                command: Some("bash".to_string()),
                args: vec!["-lc".to_string(), "cat".to_string()],
                env: vec![],
                log_path: None,
                ssh_host: None,
                ssh_user: None,
                ssh_port: None,
                ssh_strict_host_key_checking: true,
                ssh_known_hosts_file: None,
                ssh_fingerprint: None,
                ssh_require_fingerprint: false,
                ssh_dry_run: true,
                remote_workdir: None,
                lan_discovery: None,
                tunnel: None,
            },
            InstanceConfig {
                id: "bob".to_string(),
                mode: InstanceMode::Local,
                data_dir: bob_data,
                device_id: None,
                bind_address: "127.0.0.1:45012".to_string(),
                demo_mode: false,
                command: Some("bash".to_string()),
                args: vec!["-lc".to_string(), "cat".to_string()],
                env: vec![],
                log_path: None,
                ssh_host: None,
                ssh_user: None,
                ssh_port: None,
                ssh_strict_host_key_checking: true,
                ssh_known_hosts_file: None,
                ssh_fingerprint: None,
                ssh_require_fingerprint: false,
                ssh_dry_run: true,
                remote_workdir: None,
                lan_discovery: None,
                tunnel: None,
            },
        ],
    };

    let scenario = test_scenario_config(
        "executor-send-clipboard",
        "verify send_clipboard retry",
        vec![CompatibilityStep {
            id: "step-1".to_string(),
            action: CompatibilityAction::SendClipboard,
            instance: Some("bob".to_string()),
            source_instance: Some("alice".to_string()),
            timeout_ms: Some(2000),
            ..Default::default()
        }],
    );

    let mut api = ToolApi::new(
        HarnessCoordinator::from_run_config(&run).unwrap_or_else(|error| panic!("{error}")),
    );
    if let Err(error) = api.start_all() {
        panic!("start_all failed: {error}");
    }

    let clipboard_path = alice_data.join(".harness-transient/clipboard.txt");
    let mut writer = std::process::Command::new("sh")
        .arg("-c")
        .arg("sleep 0.2; printf 'invite-code-123\\n' > \"$1\"")
        .arg("harness-clipboard-writer")
        .arg(&clipboard_path)
        .spawn()
        .unwrap_or_else(|error| panic!("spawn delayed clipboard writer failed: {error}"));

    if let Err(error) =
        ScenarioExecutor::new(ExecutionMode::Compatibility).execute(&scenario, &mut api)
    {
        panic!("send_clipboard execute failed: {error}");
    }

    let writer_status = writer
        .wait()
        .unwrap_or_else(|error| panic!("wait for delayed clipboard writer failed: {error}"));
    assert!(writer_status.success(), "delayed clipboard writer failed");
    if let Err(error) = api.stop_all() {
        panic!("stop_all failed: {error}");
    }

    let action_log = api.action_log();
    let sent_to_bob = action_log.iter().any(|entry| {
        matches!(
            &entry.request,
            ToolRequest::SendKeys { instance_id, keys }
            if instance_id == "bob" && keys.contains("invite-code-123")
        )
    });
    assert!(
        sent_to_bob,
        "send_clipboard should eventually send copied text to bob"
    );
}

#[test]
fn send_clipboard_long_payload_is_chunked_and_reassembled() {
    let temp_root = unique_test_dir("executor-send-clipboard-chunked");
    let alice_data = temp_root.join("alice");
    let bob_data = temp_root.join("bob");
    let _ = std::fs::create_dir_all(&alice_data);
    let _ = std::fs::create_dir_all(&bob_data);

    let run = RunConfig {
        schema_version: 1,
        run: RunSection {
            name: "executor-send-clipboard-chunked".to_string(),
            pty_rows: Some(40),
            pty_cols: Some(120),
            artifact_dir: Some(temp_root.join("artifacts")),
            global_budget_ms: None,
            step_budget_ms: None,
            seed: Some(9),
            max_cpu_percent: None,
            max_memory_bytes: None,
            max_open_files: None,
            require_remote_artifact_sync: false,
            runtime_substrate: crate::config::RuntimeSubstrate::default(),
        },
        instances: vec![
            InstanceConfig {
                id: "alice".to_string(),
                mode: InstanceMode::Local,
                data_dir: alice_data.clone(),
                device_id: None,
                bind_address: "127.0.0.1:45021".to_string(),
                demo_mode: false,
                command: Some("bash".to_string()),
                args: vec!["-lc".to_string(), "cat".to_string()],
                env: vec![],
                log_path: None,
                ssh_host: None,
                ssh_user: None,
                ssh_port: None,
                ssh_strict_host_key_checking: true,
                ssh_known_hosts_file: None,
                ssh_fingerprint: None,
                ssh_require_fingerprint: false,
                ssh_dry_run: true,
                remote_workdir: None,
                lan_discovery: None,
                tunnel: None,
            },
            InstanceConfig {
                id: "bob".to_string(),
                mode: InstanceMode::Local,
                data_dir: bob_data,
                device_id: None,
                bind_address: "127.0.0.1:45022".to_string(),
                demo_mode: false,
                command: Some("bash".to_string()),
                args: vec!["-lc".to_string(), "cat".to_string()],
                env: vec![],
                log_path: None,
                ssh_host: None,
                ssh_user: None,
                ssh_port: None,
                ssh_strict_host_key_checking: true,
                ssh_known_hosts_file: None,
                ssh_fingerprint: None,
                ssh_require_fingerprint: false,
                ssh_dry_run: true,
                remote_workdir: None,
                lan_discovery: None,
                tunnel: None,
            },
        ],
    };

    let scenario = test_scenario_config(
        "executor-send-clipboard-chunked",
        "verify long clipboard payload chunking",
        vec![CompatibilityStep {
            id: "step-1".to_string(),
            action: CompatibilityAction::SendClipboard,
            instance: Some("bob".to_string()),
            source_instance: Some("alice".to_string()),
            timeout_ms: Some(2000),
            ..Default::default()
        }],
    );

    let long_payload = "aura:v1:".to_string()
        + &"x".repeat(CLIPBOARD_PASTE_CHUNK_CHARS * 3 + 7)
        + ":127.0.0.1:41001";

    let mut api = ToolApi::new(
        HarnessCoordinator::from_run_config(&run).unwrap_or_else(|error| panic!("{error}")),
    );
    if let Err(error) = api.start_all() {
        panic!("start_all failed: {error}");
    }

    let clipboard_path = alice_data.join(".harness-transient/clipboard.txt");
    let _ = std::fs::write(&clipboard_path, format!("{long_payload}\n"));

    if let Err(error) =
        ScenarioExecutor::new(ExecutionMode::Compatibility).execute(&scenario, &mut api)
    {
        panic!("send_clipboard execute failed: {error}");
    }

    if let Err(error) = api.stop_all() {
        panic!("stop_all failed: {error}");
    }

    let chunks: Vec<String> = api
        .action_log()
        .iter()
        .filter_map(|entry| match &entry.request {
            ToolRequest::SendKeys { instance_id, keys } if instance_id == "bob" => {
                Some(keys.clone())
            }
            _ => None,
        })
        .collect();
    assert!(
        chunks.len() > 1,
        "expected long clipboard text to be chunked"
    );
    let reassembled = chunks.join("");
    assert_eq!(reassembled, long_payload);
}

#[test]
fn wait_contract_refs_cover_all_parity_wait_kinds() {
    let modal = WaitContractRef::Modal(ModalId::AddDevice);
    let runtime = WaitContractRef::RuntimeEvent(RuntimeEventKind::MessageCommitted);
    let screen = WaitContractRef::Screen(ScreenId::Chat);
    let readiness = WaitContractRef::Readiness(aura_app::ui::contract::UiReadiness::Ready);
    let quiescence =
        WaitContractRef::Quiescence(aura_app::ui_contract::QuiescenceState::Settled);
    let operation = WaitContractRef::OperationState {
        operation_id: OperationId::invitation_accept_contact(),
        state: OperationState::Succeeded,
        label: "accept_contact_invitation",
    };

    assert!(matches!(modal, WaitContractRef::Modal(ModalId::AddDevice)));
    assert!(matches!(
        runtime,
        WaitContractRef::RuntimeEvent(RuntimeEventKind::MessageCommitted)
    ));
    assert!(matches!(screen, WaitContractRef::Screen(ScreenId::Chat)));
    assert!(matches!(
        readiness,
        WaitContractRef::Readiness(aura_app::ui::contract::UiReadiness::Ready)
    ));
    assert!(matches!(
        quiescence,
        WaitContractRef::Quiescence(aura_app::ui_contract::QuiescenceState::Settled)
    ));
    assert!(matches!(
        operation,
        WaitContractRef::OperationState {
            operation_id: _,
            state: OperationState::Succeeded,
            label: "accept_contact_invitation"
        }
    ));
}

#[test]
fn shared_intent_waits_bind_only_to_declared_barriers() {
    let step = crate::config::CompatibilityStep {
        id: "declared-wait".to_string(),
        ..Default::default()
    };
    let start_device_contract = IntentAction::StartDeviceEnrollment {
        device_name: "phone".to_string(),
        code_name: "device_code".to_string(),
        invitee_authority_id: "authority:peer".to_string(),
    }
    .contract();
    assert!(ensure_wait_contract_declared(
        &step,
        &start_device_contract,
        WaitContractRef::Screen(ScreenId::Settings),
    )
    .is_ok());
    assert!(ensure_wait_contract_declared(
        &step,
        &start_device_contract,
        WaitContractRef::Readiness(aura_app::ui::contract::UiReadiness::Ready),
    )
    .is_ok());
    assert!(ensure_wait_contract_declared(
        &step,
        &start_device_contract,
        WaitContractRef::OperationState {
            operation_id: OperationId::device_enrollment(),
            state: OperationState::Succeeded,
            label: "start_device_enrollment",
        },
    )
    .is_ok());
    assert!(ensure_wait_contract_declared(
        &step,
        &start_device_contract,
        WaitContractRef::RuntimeEvent(RuntimeEventKind::DeviceEnrollmentCodeReady),
    )
    .is_ok());
    assert!(ensure_wait_contract_declared(
        &step,
        &start_device_contract,
        WaitContractRef::RuntimeEvent(RuntimeEventKind::MessageCommitted),
    )
    .is_err());
    assert!(ensure_wait_contract_declared(
        &step,
        &start_device_contract,
        WaitContractRef::Modal(ModalId::AddDevice),
    )
    .is_err());

    let import_contract = IntentAction::ImportDeviceEnrollmentCode {
        code: "invite".to_string(),
    }
    .contract();
    assert!(ensure_wait_contract_declared(
        &step,
        &import_contract,
        WaitContractRef::Screen(ScreenId::Neighborhood),
    )
    .is_ok());
    assert!(ensure_wait_contract_declared(
        &step,
        &import_contract,
        WaitContractRef::Readiness(aura_app::ui::contract::UiReadiness::Ready),
    )
    .is_ok());
    assert!(ensure_wait_contract_declared(
        &step,
        &import_contract,
        WaitContractRef::Modal(ModalId::AddDevice),
    )
    .is_err());
}

#[test]
fn semantic_intent_templates_are_resolved_before_submission() {
    let mut context = ScenarioContext::default();
    context.vars.insert(
        "alice_authority_id".to_string(),
        "authority-a2e0c941-1dc2-088e-ffb4-102cb124ac38".to_string(),
    );
    let resolved = resolve_intent_templates(
        &IntentAction::CreateContactInvitation {
            receiver_authority_id: "${alice_authority_id}".to_string(),
            code_name: Some("contact_code".to_string()),
        },
        &context,
    )
    .unwrap_or_else(|error| panic!("{error}"));

    assert!(matches!(
        resolved,
        IntentAction::CreateContactInvitation {
            receiver_authority_id,
            code_name,
        } if receiver_authority_id == "authority-a2e0c941-1dc2-088e-ffb4-102cb124ac38"
            && code_name.as_deref() == Some("contact_code")
    ));
}

#[test]
fn join_channel_templates_reject_channel_id_templates() {
    let mut context = ScenarioContext::default();
    context.vars.insert(
        "shared_channel_id".to_string(),
        "channel:d2063fb67d0f80f6061878a00623a3608c72ec5b3e08088324064174068cec76".to_string(),
    );
    let error = resolve_intent_templates(
        &IntentAction::JoinChannel {
            channel_name: "${shared_channel_id}".to_string(),
        },
        &context,
    )
    .err()
    .unwrap_or_else(|| panic!("channel id template must fail"));

    assert!(
        error
            .to_string()
            .contains("join_channel requires an authoritative shared channel name"),
        "unexpected error: {error:#}"
    );
}

#[test]
fn exact_handle_required_for_parity_critical_submission() {
    let step = crate::config::CompatibilityStep {
        id: "join-channel".to_string(),
        ..Default::default()
    };
    let response = SemanticCommandResponse::accepted_without_value();

    let error =
        require_semantic_unit_submission_with_exact_handle(&step, "join_channel", response)
            .err()
            .unwrap_or_else(|| panic!("missing handle must fail"));

    assert!(
        error
            .to_string()
            .contains("missing canonical ui operation handle with exact instance tracking"),
        "unexpected error: {error:#}"
    );
}

#[test]
fn parity_critical_executor_paths_do_not_fallback_to_runtime_event_waits() {
    let source = include_str!("executor.rs");
    let production_source = source
        .split("\n#[cfg(test)]\nmod tests {")
        .next()
        .unwrap_or(source);
    let invite_fallback_label = format!("{}{}", "pending_home_", "invitation_ready");
    let accept_fallback_label = format!("{}{}", "invitation_", "accepted");
    assert!(
        !production_source.contains(&invite_fallback_label),
        "invite_actor_to_channel must not hide missing terminal publication behind readiness fallback"
    );
    assert!(
        !production_source.contains(&accept_fallback_label),
        "accept_pending_channel_invitation must not hide missing terminal publication behind readiness fallback"
    );
    for forbidden in [
        format!("{}{}", "resolve_channel_id_for_", "shared_name("),
        format!("{}{}", "unique_authoritative_", "shared_channel_id("),
        format!("{}{}", "capture_authoritative_", "channel_id("),
        format!("{}{}", "capture_unique_shared_", "channel_id("),
    ] {
        assert!(
            !production_source.contains(&forbidden),
            "executor shared semantic channel flows must not re-materialize channel ids through {forbidden}"
        );
    }
}

#[test]
fn shared_semantic_variable_actions_use_typed_authority_helpers() {
    let source = include_str!("executor.rs");
    let production_source = source
        .split("\n#[cfg(test)]\nmod tests {")
        .next()
        .unwrap_or(source);
    assert!(
        production_source.contains("tool_api.prepare_device_enrollment_invitee_authority("),
        "shared semantic executor should use the typed invitee-authority helper"
    );
    assert!(
        production_source.contains("tool_api.current_authority_id("),
        "shared semantic executor should use the typed current-authority helper"
    );
    assert!(
        !production_source.contains(".get(\"authority_id\")"),
        "shared semantic executor must not field-peek authority_id out of raw JSON payloads"
    );
}

#[test]
fn create_account_and_home_wait_for_declared_contract_barriers() {
    let source = include_str!("executor.rs");
    let production_source = source
        .split("\n#[cfg(test)]\nmod tests {")
        .next()
        .unwrap_or(source);

    let create_branch = production_source
        .split("IntentAction::CreateAccount { .. } | IntentAction::CreateHome { .. } => {")
        .nth(1)
        .unwrap_or_else(|| panic!("create_account/create_home branch missing"));
    let create_branch = create_branch
        .split("IntentAction::CreateChannel { channel_name } => {")
        .next()
        .unwrap_or(create_branch);

    assert!(
        create_branch.contains("wait_for_contract_barriers("),
        "create_account/create_home branch must converge on shared contract barriers"
    );
}

#[test]
fn projection_freshness_classifies_restart_explicitly() {
    let baseline = ProjectionRevision {
        semantic_seq: 7,
        render_seq: Some(7),
    };
    let snapshot = UiSnapshot {
        revision: ProjectionRevision {
            semantic_seq: 3,
            render_seq: Some(1),
        },
        ..UiSnapshot::loading(ScreenId::Chat)
    };

    assert!(matches!(
        classify_projection_freshness(Some(baseline), &snapshot),
        ProjectionFreshness::Restarted { baseline: observed_baseline, observed }
            if observed_baseline == baseline && observed == snapshot.revision
    ));
}

#[test]
fn projection_freshness_does_not_treat_restart_as_satisfied() {
    let baseline = ProjectionRevision {
        semantic_seq: 7,
        render_seq: Some(7),
    };
    let snapshot = UiSnapshot {
        revision: ProjectionRevision {
            semantic_seq: 6,
            render_seq: Some(9),
        },
        ..UiSnapshot::loading(ScreenId::Chat)
    };

    assert!(!matches!(
        classify_projection_freshness(Some(baseline), &snapshot),
        ProjectionFreshness::Satisfied
    ));
}

#[test]
fn semantic_wait_restart_handling_resumes_projection_based_waits() {
    let step = crate::config::CompatibilityStep {
        id: "screen-ready".to_string(),
        action: crate::config::CompatibilityAction::WaitFor,
        screen_id: Some(ScreenId::Neighborhood),
        ..Default::default()
    };

    assert_eq!(
        semantic_wait_restart_handling(&step),
        SemanticWaitRestartHandling::ResumeAfterRestart
    );
}

#[test]
fn semantic_wait_restart_handling_fails_closed_for_runtime_events_and_operation_waits() {
    let runtime_event_step = crate::config::CompatibilityStep {
        id: "wait-contact-link".to_string(),
        action: crate::config::CompatibilityAction::WaitFor,
        runtime_event_kind: Some(RuntimeEventKind::ContactLinkReady),
        ..Default::default()
    };
    let operation_step = crate::config::CompatibilityStep {
        id: "wait-op".to_string(),
        action: crate::config::CompatibilityAction::WaitFor,
        operation_id: Some(OperationId::create_channel()),
        operation_state: Some(OperationState::Succeeded),
        ..Default::default()
    };

    assert_eq!(
        semantic_wait_restart_handling(&runtime_event_step),
        SemanticWaitRestartHandling::FailClosed
    );
    assert_eq!(
        semantic_wait_restart_handling(&operation_step),
        SemanticWaitRestartHandling::FailClosed
    );
}

#[test]
fn semantic_wait_restart_reset_clears_stale_projection_and_event_versions() {
    let baseline = ProjectionRevision {
        semantic_seq: 7,
        render_seq: Some(7),
    };
    let mut context = ScenarioContext::default();
    context
        .pending_projection_baseline
        .insert("alice".to_string(), baseline);
    let mut required_newer_than = Some(baseline);
    let mut snapshot_version = Some(19);

    reset_semantic_wait_after_restart(
        &mut context,
        "alice",
        &mut required_newer_than,
        &mut snapshot_version,
    );

    assert!(required_newer_than.is_none());
    assert!(snapshot_version.is_none());
    assert!(!context.pending_projection_baseline.contains_key("alice"));
}

#[test]
fn consuming_projection_baseline_clears_live_required_newer_than_state() {
    let baseline = ProjectionRevision {
        semantic_seq: 7,
        render_seq: Some(7),
    };
    let snapshot = UiSnapshot {
        revision: ProjectionRevision {
            semantic_seq: 8,
            render_seq: Some(1),
        },
        ..UiSnapshot::loading(ScreenId::Settings)
    };
    let mut context = ScenarioContext::default();
    context
        .pending_projection_baseline
        .insert("alice".to_string(), baseline);
    let mut required_newer_than = Some(baseline);

    consume_projection_baseline(&mut context, "alice", &snapshot, &mut required_newer_than);

    assert!(required_newer_than.is_none());
    assert!(!context.pending_projection_baseline.contains_key("alice"));
}

#[test]
fn projection_wait_can_resume_when_matching_snapshot_differs_from_baseline() {
    let step = crate::config::CompatibilityStep {
        id: "devices-count-one".to_string(),
        action: crate::config::CompatibilityAction::WaitFor,
        list_id: Some(ListId::Devices),
        count: Some(1),
        ..Default::default()
    };
    let baseline = UiSnapshot {
        revision: ProjectionRevision {
            semantic_seq: 12,
            render_seq: Some(8),
        },
        screen: ScreenId::Settings,
        lists: vec![ListSnapshot {
            id: ListId::Devices,
            items: vec![
                ListItemSnapshot {
                    id: "device-a".to_string(),
                    selected: false,
                    confirmation: ConfirmationState::Confirmed,
                    is_current: false,
                },
                ListItemSnapshot {
                    id: "device-b".to_string(),
                    selected: false,
                    confirmation: ConfirmationState::Confirmed,
                    is_current: true,
                },
            ],
        }],
        ..UiSnapshot::loading(ScreenId::Settings)
    };
    let matching_snapshot = UiSnapshot {
        revision: baseline.revision,
        screen: ScreenId::Settings,
        lists: vec![ListSnapshot {
            id: ListId::Devices,
            items: vec![ListItemSnapshot {
                id: "device-b".to_string(),
                selected: false,
                confirmation: ConfirmationState::Confirmed,
                is_current: true,
            }],
        }],
        ..UiSnapshot::loading(ScreenId::Settings)
    };
    let mut context = ScenarioContext::default();
    set_projection_baseline(&mut context, "alice", baseline);

    assert!(projection_wait_can_resume_from_matching_snapshot(
        &step,
        &matching_snapshot,
        &context,
        "alice",
        Some(matching_snapshot.revision),
        SemanticWaitRestartHandling::ResumeAfterRestart,
    ));
}

#[test]
fn browser_ui_snapshot_issue_classifies_restart_and_timeout() {
    let restart = anyhow!("Target page, context or browser has been closed");
    let timeout = anyhow!("Playwright driver ui_state timed out for request 7");
    let unknown = anyhow!("some other error");

    assert_eq!(
        classify_browser_ui_snapshot_issue(&restart),
        Some(BrowserUiSnapshotIssue::BrowserRestarted)
    );
    assert_eq!(
        classify_browser_ui_snapshot_issue(&timeout),
        Some(BrowserUiSnapshotIssue::TransientTimeout)
    );
    assert_eq!(classify_browser_ui_snapshot_issue(&unknown), None);
}

#[test]
fn semantic_wait_runtime_events_require_authoritative_runtime_facts() {
    let step = crate::config::CompatibilityStep {
        id: "wait-contact-link".to_string(),
        action: crate::config::CompatibilityAction::WaitFor,
        runtime_event_kind: Some(RuntimeEventKind::ContactLinkReady),
        ..Default::default()
    };
    let mut snapshot = UiSnapshot::loading(ScreenId::Contacts);
    snapshot.lists = vec![ListSnapshot {
        id: ListId::Contacts,
        items: vec![ListItemSnapshot {
            id: "contact-1".to_string(),
            selected: false,
            confirmation: ConfirmationState::Confirmed,
            is_current: false,
        }],
    }];

    assert!(
        !semantic_wait_matches(&step, &snapshot),
        "runtime event waits must not fall back to list/UI state"
    );
}

#[test]
fn semantic_wait_accepts_amp_transition_runtime_events_only_from_ui_snapshot() {
    let step = crate::config::CompatibilityStep {
        id: "wait-amp-transition".to_string(),
        action: crate::config::CompatibilityAction::WaitFor,
        runtime_event_kind: Some(RuntimeEventKind::AmpChannelTransitionUpdated),
        contains: Some("evidence-1".to_string()),
        ..Default::default()
    };
    let mut snapshot = UiSnapshot::loading(ScreenId::Notifications);
    snapshot.runtime_events.push(RuntimeEventSnapshot {
        id: RuntimeEventId::synthetic("runtime-event-amp-channel-a"),
        fact: RuntimeFact::AmpChannelTransitionUpdated {
            transition: aura_app::ui_contract::AmpChannelTransitionSnapshot {
                channel: ChannelFactKey::identified("channel-a"),
                stable_epoch: 2,
                state: aura_app::ui_contract::AmpTransitionState::A2Conflict,
                live_transition_id: None,
                finalized_transition_id: None,
                conflict_evidence: vec!["evidence-1".to_string()],
                emergency_policy: Some(
                    aura_app::ui_contract::AmpTransitionPolicySnapshot::EmergencyQuarantine,
                ),
                suspect_authorities: vec!["authority-1".to_string()],
                quarantine_epochs: vec![3],
                prune_before_epochs: Vec::new(),
                cryptoshred_active: false,
                accusation_history: Vec::new(),
            },
        },
    });

    assert!(
        semantic_wait_matches(&step, &snapshot),
        "AMP transition waits must resolve through UiSnapshot.runtime_events"
    );

    let mut list_only_snapshot = UiSnapshot::loading(ScreenId::Notifications);
    list_only_snapshot.lists = vec![ListSnapshot {
        id: ListId::Notifications,
        items: vec![ListItemSnapshot {
            id: "amp-transition:channel-a".to_string(),
            selected: false,
            confirmation: ConfirmationState::Confirmed,
            is_current: false,
        }],
    }];
    assert!(
        !semantic_wait_matches(&step, &list_only_snapshot),
        "AMP transition waits must not fall back to notification list ids"
    );
}

#[test]
fn semantic_wait_channel_runtime_events_require_authoritative_channel_binding_id() {
    let step = crate::config::CompatibilityStep {
        id: "wait-channel-membership".to_string(),
        action: crate::config::CompatibilityAction::WaitFor,
        runtime_event_kind: Some(RuntimeEventKind::ChannelMembershipReady),
        ..Default::default()
    };
    let mut context = ScenarioContext::default();
    context.current_channel_binding.insert(
        "bob".to_string(),
        ChannelBinding {
            channel_id:
                "channel:d2063fb67d0f80f6061878a00623a3608c72ec5b3e08088324064174068cec76"
                    .to_string(),
            context_id: "ctx:d2063fb67d0f80f6061878a00623a3608c72ec5b3e08088324064174068cec76"
                .to_string(),
        },
    );

    let mut snapshot = UiSnapshot::loading(ScreenId::Chat);
    snapshot.runtime_events.push(RuntimeEventSnapshot {
        id: RuntimeEventId(
            "channel_membership_ready:channel:d2063fb67d0f80f6061878a00623a3608c72ec5b3e08088324064174068cec76"
                .to_string(),
        ),
        fact: RuntimeFact::ChannelMembershipReady {
            channel: ChannelFactKey {
                id: Some(
                    "channel:d2063fb67d0f80f6061878a00623a3608c72ec5b3e08088324064174068cec76"
                        .to_string(),
                ),
                name: Some("shared-parity-lab".to_string()),
            },
            member_count: Some(2),
        },
    });

    assert!(semantic_wait_matches_for_instance(
        &step, &snapshot, &context, "bob"
    ));
}

#[test]
fn semantic_wait_helpers_do_not_use_raw_dom_or_text_fallbacks() {
    let source = include_str!("executor.rs");
    for helper in [
        "fn wait_for_semantic_state(",
        "fn wait_for_runtime_event_snapshot(",
        "fn wait_for_operation_handle_state(",
    ] {
        let start = source
            .find(helper)
            .unwrap_or_else(|| panic!("missing helper source for {helper}"));
        let tail = &source[start..];
        let end = tail.find("\nfn ").unwrap_or(tail.len());
        let body = &tail[..end];
        assert!(
            !body.contains("wait_for_diagnostic_dom_patterns("),
            "{helper} must not resolve through DOM pattern fallbacks"
        );
        assert!(
            !body.contains("diagnostic_dom_snapshot("),
            "{helper} must not resolve through raw DOM snapshots"
        );
        assert!(
            !body.contains("diagnostic_screen_contains("),
            "{helper} must not resolve through raw text fallbacks"
        );
        assert!(
            !body.contains("tail_log("),
            "{helper} must not resolve through diagnostic log fallbacks"
        );
    }
}

#[test]
fn raw_text_fallbacks_are_explicitly_diagnostic_only() {
    let source = include_str!("executor.rs");
    let start = source
        .find("fn diagnostic_screen_contains(")
        .unwrap_or_else(|| panic!("missing diagnostic_screen_contains helper"));
    let tail = &source[start..];
    let end = tail.find("\nfn ").unwrap_or(tail.len());
    let body = &tail[..end];
    assert!(!body.contains("FallbackObservationMode"));
}

#[test]
fn diagnostic_capture_paths_do_not_peek_legacy_screen_field_names() {
    let source = include_str!("executor.rs");
    assert!(
        source.contains("DiagnosticScreenCapture"),
        "diagnostic capture helpers must deserialize an explicit diagnostic capture type"
    );
    assert!(
        !source.contains(".get(\"authoritative_screen\")"),
        "executor diagnostics must not peek ambiguous authoritative_screen fields"
    );
    assert!(
        !source.contains(".get(\"raw_screen\")"),
        "executor diagnostics must not peek ambiguous raw_screen fields"
    );
    assert!(
        !source.contains(".get(\"normalized_screen\")"),
        "executor diagnostics must not peek ambiguous normalized_screen fields"
    );
}
