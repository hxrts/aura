//! Required VM lifecycle evidence and its serialized execution lane.
use super::support::{command_stdout, repo_root};
use anyhow::{bail, Context, Result};
use std::{collections::BTreeSet, env, fs, path::Path};
use syn::visit::Visit;

struct LifecycleSuite {
    source: &'static str,
    functions: &'static [&'static str],
    harness_prefix: &'static str,
    filter: &'static str,
}
const DEPENDENCY_SUITES: &[LifecycleSuite] = &[
    LifecycleSuite {
        source: "tests/unit/threaded_runtime_tests.rs",
        functions: &[
            "required_threaded_reap_preserves_other_sessions_and_stable_ids",
            "required_threaded_reap_rejects_epoch_and_index_faults_before_mutation",
            "required_threaded_reap_preserves_actual_poison_failure",
            "required_threaded_reap_acknowledges_completed_worker_scope",
            "required_threaded_reap_preserves_natural_terminal_epoch",
        ],
        harness_prefix: "threaded::tests::",
        filter: "required_threaded_reap",
    },
    LifecycleSuite {
        source: "tests/unit/protocol_machine/tests_runtime_progress.rs",
        functions: &[
            "required_cooperative_reap_removes_target_and_preserves_other_sessions",
            "required_cooperative_reap_validates_deserialized_stable_ids",
            "required_cooperative_reap_preserves_natural_terminal_epoch",
            "required_cooperative_reap_rejects_active_epoch_exhaustion_before_mutation",
        ],
        harness_prefix: "engine::tests::",
        filter: "required_cooperative_reap",
    },
];
const CORE_SUITES: &[LifecycleSuite] = &[
    LifecycleSuite {
        source: "crates/aura-core/src/types/facts.rs",
        functions: &[
            "required_fact_decoder_rejects_json_through_both_entry_points",
            "required_fact_decoder_retains_native_cbor_cause_through_both_entry_points",
        ],
        harness_prefix: "types::facts::tests::",
        filter: "types::facts::tests::required_fact_decoder_",
    },
    LifecycleSuite {
        source: "crates/aura-core/src/time/timeout.rs",
        functions: &["required_timeout_drops_cancelled_query_before_reacquiring_observation_owner"],
        harness_prefix: "time::timeout::tests::",
        filter: "time::timeout::tests::required_timeout_drops_cancelled_query_before_reacquiring_observation_owner",
    },
];
const TERMINAL_OBSERVATION_SUITES: &[LifecycleSuite] = &[
    LifecycleSuite {
        source: "crates/aura-core/src/time/timeout.rs",
        functions: &[
            "required_terminal_ack_bounds_hung_clock_read_and_drops_original_lease",
            "required_terminal_ack_bounds_contended_observation_gate",
            "required_terminal_ack_bounds_checkpoint_and_rejects_late_completion",
            "required_terminal_ack_preserves_native_timer_failure_without_publication",
            "required_terminal_ack_rejects_native_checkpoint_failure",
            "required_terminal_ack_publishes_once_with_original_guard_after_checkpoint",
            "required_terminal_ack_rejects_unsupported_provider_before_publication",
            "required_terminal_ack_rejects_checkpoint_expiry_in_its_completion_poll",
            "required_terminal_ack_bounds_hung_postcheckpoint_read",
            "required_terminal_ack_retains_postcheckpoint_provider_failure",
            "required_terminal_ack_rejects_postcheckpoint_rollback",
        ],
        harness_prefix: "time::timeout::tests::",
        filter: "time::timeout::tests::required_terminal_ack_",
    },
    LifecycleSuite {
        source: "crates/aura-core/src/time/timeout.rs",
        functions: &[
            "required_plain_timeout_bounds_hung_initial_clock_read",
            "required_plain_timeout_bounds_hung_success_clock_read",
            "required_plain_timeout_bounds_observation_gate_before_clock_or_operation",
        ],
        harness_prefix: "time::timeout::tests::",
        filter: "time::timeout::tests::required_plain_timeout_",
    },
    LifecycleSuite {
        source: "crates/aura-core/src/time/timeout.rs",
        functions: &[
            "initial_publication_ack_precedes_clock_observation_and_retains_result",
            "initial_publication_bounds_write_readback_and_post_ack_clock",
            "initial_publication_deadline_wins_same_turn_and_bounds_gate",
            "initial_publication_preserves_native_failure_without_clock_read",
            "initial_publication_preserves_clock_failure_and_refuses_expired_ack",
        ],
        harness_prefix: "time::timeout::tests::",
        filter: "time::timeout::tests::initial_publication_",
    },
];
const APP_OBSERVATION_SUITES: &[LifecycleSuite] = &[LifecycleSuite {
    source: "crates/aura-app/src/workflows/runtime.rs",
    functions: &[
        "runtime_required_clock_reads_are_bounded_for_executor_child_and_retry",
        "runtime_observation_retains_original_selected_provider_and_uncertainty",
    ],
    harness_prefix: "workflows::runtime::clock_owner_regressions::",
    filter: "workflows::runtime::clock_owner_regressions::runtime_",
}];
const NATIVE_RUNTIME_OBSERVATION_SUITES: &[LifecycleSuite] = &[LifecycleSuite {
    source: "crates/aura-agent/src/runtime_bridge/tests.rs",
    functions: &["required_native_absolute_deadline_retains_selected_provider_witness"],
    harness_prefix: "runtime_bridge::tests::",
    filter: "runtime_bridge::tests::required_native_absolute_deadline_",
}];
const ABSOLUTE_PROVIDER_SUITES: &[LifecycleSuite] = &[
    LifecycleSuite {
        source: "crates/aura-testkit/src/time/manual_physical_clock.rs",
        functions: &[
            "required_absolute_deadline_preserves_endpoint_after_delayed_registration",
            "required_absolute_deadline_preserves_rollback_and_native_timer_fault",
        ],
        harness_prefix: "time::manual_physical_clock::tests::",
        filter: "time::manual_physical_clock::tests::required_absolute_deadline_",
    },
    LifecycleSuite {
        source: "crates/aura-testkit/src/time/controllable_time.rs",
        functions: &[
            "required_absolute_deadline_frozen_scaled_clock_needs_actual_progress",
            "required_absolute_deadline_shared_control_keeps_rollback_visible",
        ],
        harness_prefix: "time::controllable_time::tests::",
        filter: "time::controllable_time::tests::required_absolute_deadline_",
    },
];
const NATIVE_ABSOLUTE_PROVIDER_SUITES: &[LifecycleSuite] = &[LifecycleSuite {
    source: "crates/aura-effects/src/time.rs",
    functions: &[
        "required_absolute_deadline_at_epoch_returns_actual_native_clock",
        "required_absolute_deadline_waits_for_actual_native_endpoint",
        "required_absolute_deadline_rechecks_clock_before_pending_timer",
    ],
    harness_prefix: "time::absolute_deadline_tests::",
    filter: "time::absolute_deadline_tests::required_absolute_deadline_",
}];
const SIGNATURE_SUITES: &[LifecycleSuite] = &[LifecycleSuite {
    source: "crates/aura-signature/src/transcript.rs",
    functions: &["required_encoding_preserves_canonical_wire_and_native_codec_failure"],
    harness_prefix: "transcript::tests::",
    filter:
        "transcript::tests::required_encoding_preserves_canonical_wire_and_native_codec_failure",
}];
const SYNC_NATIVE_SOURCE_SUITES: &[LifecycleSuite] = &[
    LifecycleSuite {
        source: "crates/aura-sync/src/core/session/tests.rs",
        functions: &[
            "required_session_original_deadline_survives_activation_without_renewal",
            "required_session_endpoint_overflow_fails_before_allocation_with_native_cause",
            "required_session_original_rollback_fails_before_allocation_with_native_cause",
        ],
        harness_prefix: "core::session::tests::",
        filter: "core::session::tests::required_session_",
    },
    LifecycleSuite {
        source: "crates/aura-sync/src/services/sync/tests.rs",
        functions: &[
            "required_sync_exact_session_drop_retires_initializing_target_and_preserves_foreign_session",
            "required_sync_partial_session_admission_retires_original_issued_subset",
            "required_sync_cancelled_future_retires_its_exact_issued_session",
        ],
        harness_prefix: "services::sync::tests::",
        filter: "services::sync::tests::required_sync_",
    },

    LifecycleSuite {
        source: "crates/aura-sync/src/core/errors.rs",
        functions: &[
            "required_sync_new_diagnostic_preserves_category_native_cause_and_clone",
            "required_sync_nested_and_terminal_causes_are_retained_without_reclassification",
            "required_sync_biscuit_guard_preserves_native_capability_failure",
        ],
        harness_prefix: "core::errors::native_cause_tests::",
        filter: "core::errors::native_cause_tests::",
    },
    LifecycleSuite {
        source: "crates/aura-sync/src/core/wire.rs",
        functions: &["required_sync_json_codec_retains_actual_malformed_payload_source"],
        harness_prefix: "core::wire::required_source_tests::",
        filter: "core::wire::required_source_tests::",
    },
];
const AGENT_SUITES: &[LifecycleSuite] = &[LifecycleSuite {
    source: "crates/aura-agent/src/runtime/services/threshold_signing/enrollment_transcript_signing.rs",
    functions: &["malformed_native_package_retains_original_codec_cause"],
    harness_prefix: "runtime::services::threshold_signing::enrollment_transcript_signing::native_commitment_tests::",
    filter: "runtime::services::threshold_signing::enrollment_transcript_signing::native_commitment_tests::malformed_native_package_retains_original_codec_cause",
},
LifecycleSuite {
    source: "crates/aura-agent/src/runtime/services/ceremony_tracker.rs",
    functions: &["original_completion_observation_releases_execution_lease_without_renewing_clock"],
    harness_prefix: "runtime::services::ceremony_tracker::tests::",
    filter: "runtime::services::ceremony_tracker::tests::original_completion_observation_releases_execution_lease_without_renewing_clock",
},
LifecycleSuite {
    source: "crates/aura-agent/src/handlers/invitation/enrollment_vm_admission.rs",
    functions: &["actual_admitted_vm_control_and_acceptance_reject_foreign_decisions"],
    harness_prefix: "handlers::invitation::enrollment_vm_admission::tests::",
    filter: "handlers::invitation::enrollment_vm_admission::tests::actual_admitted_vm_control_and_acceptance_reject_foreign_decisions",
},

    LifecycleSuite {
        source: "crates/aura-agent/src/runtime/time_handler.rs",
        functions: &["required_absolute_deadline_keeps_actual_configured_provider_and_native_failure"],
        harness_prefix: "runtime::time_handler::tests::",
        filter: "runtime::time_handler::tests::required_absolute_deadline_",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/reactive/app_signal_views.rs",
        functions: &[
            "required_signal_views_retain_actual_unregistered_snapshot_failure",
            "required_signal_views_matching_domain_codec_faults_are_terminal",
        ],
        harness_prefix: "reactive::app_signal_views::tests::",
        filter: "reactive::app_signal_views::tests::required_signal_views_",
    },

    LifecycleSuite {
        source: "crates/aura-agent/src/handlers/chat_service.rs",
        functions: &[
            "required_chat_matching_malformed_committed_json_is_not_absence_or_empty_success",
            "required_chat_matching_unsupported_committed_schema_is_not_absence_or_empty_success",
        ],
        harness_prefix: "handlers::chat_service::tests::",
        filter: "handlers::chat_service::tests::required_chat_matching_",
    },

    LifecycleSuite {
        source: "crates/aura-agent/src/runtime/services/sync_command_registry.rs",
        functions: &[
            "required_sync_registry_start_caller_frame_stays_bounded_before_first_poll",
            "required_sync_registry_rejects_foreign_actual_admission_before_birth",
            "required_sync_registered_lifetime_releases_start_admission_and_acknowledges_stop",
            "required_sync_command_returns_actual_peer_protocol_refusal_before_success",
            "required_sync_idle_round_does_not_claim_peer_completion_or_keep_start_admission",
            "required_sync_start_clock_fault_precedes_actor_birth_and_releases_admission",
            "required_sync_daemon_runs_real_tracked_peer_round_and_retains_task_fault",
            "required_sync_whole_shutdown_stops_registered_daemon_before_task_cancellation",
        ],
        harness_prefix: "runtime::services::sync_command_registry::tests::",
        filter: "runtime::services::sync_command_registry::tests::",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/runtime/services/sync_manager.rs",
        functions: &[
            "required_manual_sync_clock_failure_retains_original_source_without_time_zero",
        ],
        harness_prefix: "runtime::services::sync_manager::tests::",
        filter: "runtime::services::sync_manager::tests::required_manual_sync_",
    },

    LifecycleSuite {
        source: "crates/aura-agent/src/task_registry.rs",
        functions: &["actual_shutdown_task_root_rejects_equal_named_foreign_and_sibling_scopes"],
        harness_prefix: "task_registry::shutdown_scope_tests::",
        filter: "task_registry::shutdown_scope_tests::",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/runtime/services/threshold_signing.rs",
        functions: &["required_service_stop_retains_original_shutdown_deadline_under_actual_state_contention", "required_service_health_retains_original_deadline_and_sticky_clock_rollback", "required_prior_task_failure_withholds_whole_shutdown_authority_termination"],
        harness_prefix: "runtime::services::threshold_signing::original_stop_window_tests::",
        filter: "runtime::services::threshold_signing::original_stop_window_tests::",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/reactive/scheduler.rs",
        functions: &[
            "required_processing_target_ignores_older_batch_and_retains_completed_highwater",
            "required_processing_target_closed_scheduler_retains_native_watch_source",
            "required_processing_target_preserves_actual_scheduler_clock_failure",
            "required_processing_target_original_deadline_and_foreign_owner_fail_closed",
            "failed_publication_envelope_cannot_advance_foreign_scheduler_highwater",
            "required_processing_target_survives_actual_diagnostic_lag_and_coalescing",
            "required_processing_cancelled_blocked_enqueue_releases_original_sequence_owner",
            "required_projection_failure_preserves_native_cause_and_blocks_batch_ack",
            "required_issued_target_retains_failed_projection_without_processing_ack",
        ],
        harness_prefix: "reactive::scheduler::tests::",
        filter: "reactive::scheduler::tests::",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/handlers/chat_service.rs",
        functions: &[
            "required_chat_processing_closed_original_sink_preserves_native_cause",
            "required_chat_public_admission_caller_future_stays_bounded_before_first_poll",
            "required_chat_pipeline_attachment_rejects_foreign_and_standalone_ingress",
            "required_chat_admission_retains_actual_clock_failure_before_mutation",
            "required_chat_runtime_without_ingress_cannot_mint_processing_success",
        ],
        harness_prefix: "handlers::chat_service::tests::",
        filter: "handlers::chat_service::tests::required_chat_",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/runtime/services/reactive_pipeline_service.rs",
        functions: &[
            "required_startup_replay_retains_original_window_after_signal_registration",
            "required_startup_replay_clock_failure_keeps_pipeline_for_owned_cleanup",
        ],
        harness_prefix: "runtime::services::reactive_pipeline_service::tests::",
        filter: "runtime::services::reactive_pipeline_service::tests::required_startup_",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/runtime/system/lifecycle.rs",
        functions: &["required_startup_cleanup_aggregate_retains_actual_primary_and_secondary_sources"],
        harness_prefix: "runtime::system::lifecycle::startup_source_tests::",
        filter: "runtime::system::lifecycle::startup_source_tests::",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/reactive/scheduler.rs",
        functions: &["required_graceful_shutdown_processes_accepted_queued_ingress_before_completion"],
        harness_prefix: "reactive::scheduler::tests::",
        filter: "reactive::scheduler::tests::required_graceful_shutdown_processes_accepted_queued_ingress_before_completion",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/runtime/system.rs",
        functions: &[
            "admitted_operation_blocks_handoff_until_its_actual_lease_drops",
            "foreign_lease_cannot_close_or_drain_original_runtime",
            "cancellation_releases_operation_but_does_not_reopen_closed_admission",
        ],
        harness_prefix: "runtime::system::operation_drain_tests::",
        filter: "runtime::system::operation_drain_tests::",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/runtime/system.rs",
        functions: &[
            "admission_capacity_is_finite_and_released_only_by_actual_lease_drop",
            "closed_admission_has_precedence_over_resource_capacity",
        ],
        harness_prefix: "runtime::system::operation_capacity_tests::",
        filter: "runtime::system::operation_capacity_tests::",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/runtime/system.rs",
        functions: &[
            "actual_runtime_effects_lease_blocks_operation_drain_and_closes_stale_admission",
            "equal_identity_distinct_profile_runtime_cannot_use_foreign_effects_lease",
            "actual_shutdown_waits_for_original_effect_lease_before_scheduler_and_stopped_publication",
        ],
        harness_prefix: "runtime::system::actual_effects_admission_tests::",
        filter: "runtime::system::actual_effects_admission_tests::",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/handlers/invitation_service.rs",
        functions: &["retained_invitation_service_clone_rejects_import_after_admission_closes"],
        harness_prefix: "handlers::invitation_service::tests::",
        filter: "handlers::invitation_service::tests::retained_invitation_service_clone_rejects_import_after_admission_closes",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/task_registry.rs",
        functions: &["original_shutdown_window_preserves_required_clock_fault_without_claiming_drain"],
        harness_prefix: "task_registry::tests::",
        filter: "task_registry::tests::original_shutdown_window_preserves_required_clock_fault_without_claiming_drain",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/task_registry.rs",
        functions: &["original_shutdown_window_acknowledges_actual_descendant_destruction"],
        harness_prefix: "task_registry::descendant_supervision_tests::",
        filter: "task_registry::descendant_supervision_tests::original_shutdown_window_acknowledges_actual_descendant_destruction",
    },

    LifecycleSuite {
        source: "crates/aura-agent/src/handlers/invitation/tests.rs",
        functions: &["accepting_guardian_invitation_surfaces_choreography_failure"],
        harness_prefix: "handlers::invitation::tests::",
        filter: "handlers::invitation::tests::accepting_guardian_invitation_surfaces_choreography_failure",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/handlers/invitation/tests.rs",
        functions: &["guardian_acceptance_records_verified_recovery_key"],
        harness_prefix: "handlers::invitation::tests::",
        filter: "handlers::invitation::tests::guardian_acceptance_records_verified_recovery_key",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/handlers/invitation/guardian.rs",
        functions: &[
            "required_guardian_pair_verifier_rejects_foreign_runtime_owner",
            "required_guardian_partial_key_loss_reopens_without_replacement",
            "required_guardian_key_continuity_preserves_original_transcript_encoding",
            "required_guardian_configured_ed25519_outages_preserve_native_cause_and_original_keys",
            "required_guardian_concurrent_birth_preserves_one_original_pair",
            "required_guardian_signer_and_verifier_preserve_native_failure",
            "required_guardian_original_window_bounds_held_import_before_key_birth",
        ],
        harness_prefix: "handlers::invitation::guardian::tests::",
        filter: "handlers::invitation::guardian::tests::required_guardian",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/handlers/invitation/contact_confirmation.rs",
        functions: &[
            "required_contact_import_corruption_is_not_absence_or_cached_pending",
            "required_contact_decision_lease_rejects_foreign_runtime_and_releases_on_drop",
            "contact_confirmation_child_preserves_parent_deadline_and_shared_rollback",
            "contact_confirmation_required_clock_fault_retains_provider_source",
        ],
        harness_prefix:
            "handlers::invitation::contact_confirmation::owned_contact_continuation_tests::",
        filter: "handlers::invitation::contact_confirmation::owned_contact_continuation_tests::",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/core/agent.rs",
        functions: &["already_closed_runtime_does_not_publish_false_shutdown_completion"],
        harness_prefix: "core::agent::shutdown_admission_tests::",
        filter: "core::agent::shutdown_admission_tests::already_closed_runtime_does_not_publish_false_shutdown_completion",
    },

    LifecycleSuite {
        source: "crates/aura-agent/src/handlers/invitation/contact_confirmation.rs",
        functions: &["required_contact_verifier_retains_native_failure_and_distinguishes_invalid_signature"],
        harness_prefix: "handlers::invitation::contact_confirmation::required_contact_identity_tests::",
        filter: "handlers::invitation::contact_confirmation::required_contact_identity_tests::required_contact_verifier_retains_native_failure_and_distinguishes_invalid_signature",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/handlers/invitation/issued_identity.rs",
        functions: &[
            "required_guardian_identity_preserves_original_issuer_after_rotation_and_restart",
            "required_contact_identity_preserves_original_issuer_after_rotation_and_restart",
            "required_guardian_identity_copied_record_cannot_reconstruct_original_issuer",
            "required_guardian_identity_rejects_foreign_runtime_owner",
        ],
        harness_prefix: "handlers::invitation::issued_identity::tests::",
        filter: "handlers::invitation::issued_identity::tests::",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/handlers/invitation/tests.rs",
        functions: &["accepting_contact_invitation_notifies_sender_and_adds_contact"],
        harness_prefix: "handlers::invitation::tests::",
        filter: "handlers::invitation::tests::accepting_contact_invitation_notifies_sender_and_adds_contact",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/handlers/invitation/tests.rs",
        functions: &["revoked_contact_invitation_acceptance_adds_no_contact"],
        harness_prefix: "handlers::invitation::tests::",
        filter: "handlers::invitation::tests::revoked_contact_invitation_acceptance_adds_no_contact",
    },

LifecycleSuite {
    source: "crates/aura-agent/src/handlers/invitation/execution.rs",
    functions: &["required_stage_budget_failure_preserves_generated_cause_and_timeout_kind"],
    harness_prefix: "handlers::invitation::execution::required_stage_source_tests::",
    filter: "handlers::invitation::execution::required_stage_source_tests::required_stage_budget_failure_preserves_generated_cause_and_timeout_kind",
},
    LifecycleSuite {
        source: "crates/aura-agent/src/runtime/services/rendezvous_manager.rs",
        functions: &["required_manager_identity_rejects_corrupt_primary_without_fallback"],
        harness_prefix: "runtime::services::rendezvous_manager::tests::",
        filter: "runtime::services::rendezvous_manager::tests::required_manager_identity_rejects_corrupt_primary_without_fallback",
    },

    LifecycleSuite {
        source: "crates/aura-agent/src/handlers/rendezvous.rs",
        functions: &["required_identity_handler_rejects_corrupt_primary_without_fallback"],
        harness_prefix: "handlers::rendezvous::tests::",
        filter: "handlers::rendezvous::tests::required_identity_handler_rejects_corrupt_primary_without_fallback",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/handlers/invitation/contact_confirmation.rs",
        functions: &["required_contact_identity_missing_key_is_typed_failure"],
        harness_prefix: "handlers::invitation::contact_confirmation::required_contact_identity_tests::",
        filter: "handlers::invitation::contact_confirmation::required_contact_identity_tests::required_contact_identity_missing_key_is_typed_failure",
    },

    LifecycleSuite {
        source: "crates/aura-agent/src/handlers/rendezvous_identity.rs",
        functions: &["actual_bootstrap_required_identity_decrypts_both_layouts_and_preserves_codec_source"],
        harness_prefix: "handlers::rendezvous_identity::required_identity_envelope_tests::",
        filter: "handlers::rendezvous_identity::required_identity_envelope_tests::actual_bootstrap_required_identity_decrypts_both_layouts_and_preserves_codec_source",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/runtime/services/threshold_signing.rs",
        functions: &["commit_key_rotation_uses_threshold_config_metadata_written_by_effects"],
        harness_prefix: "runtime::services::threshold_signing::tests::",
        filter: "runtime::services::threshold_signing::tests::commit_key_rotation_uses_threshold_config_metadata_written_by_effects",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/handlers/invitation/tests.rs",
        functions: &["owned_enrollment_secret_retirement_restarts_and_preserves_reissued_generation"],
        harness_prefix: "handlers::invitation::tests::",
        filter: "handlers::invitation::tests::owned_enrollment_secret_retirement_restarts_and_preserves_reissued_generation",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/runtime/builder.rs",
        functions: &[
            "testing_owned_profile_retains_actual_lease_until_runtime_shutdown",
            "ordinary_testing_cannot_accept_production_profile_lease",
            "testing_owned_profile_cannot_retarget_a_foreign_configuration",
        ],
        harness_prefix: "runtime::builder::owned_testing_profile_tests::",
        filter: "runtime::builder::owned_testing_profile_tests::",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/runtime/services/ceremony_tracker.rs",
        functions: &[
            "registered_window_rejects_replaced_allocation_before_checkpoint",
            "registered_window_postoperation_checkpoint_failure_prevents_success_publication",
            "registered_window_checkpoint_failure_blocks_operation_and_retains_storage_source",
        ],
        harness_prefix: "runtime::services::ceremony_tracker::tests::",
        filter: "runtime::services::ceremony_tracker::tests::registered_window_",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/runtime/services/ceremony_tracker.rs",
        functions: &["observed_snapshot_cannot_replace_registered_execution_window"],
        harness_prefix: "runtime::services::ceremony_tracker::tests::",
        filter: "runtime::services::ceremony_tracker::tests::observed_snapshot_cannot_replace_registered_execution_window",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/handlers/invitation_service.rs",
        functions: &["required_window_rejection_retains_execution_and_settlement_sources"],
        harness_prefix: "handlers::invitation_service::required_enrollment_task_tests::",
        filter: "handlers::invitation_service::required_enrollment_task_tests::required_window_rejection_retains_execution_and_settlement_sources",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/runtime_bridge/error_boundary.rs",
        functions: &["actual_activated_threshold_identity_requires_quorum_owner_with_native_source"],
        harness_prefix: "runtime_bridge::error_boundary::actual_enrolled_identity_quorum_tests::",
        filter: "runtime_bridge::error_boundary::actual_enrolled_identity_quorum_tests::",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/runtime/effects/crypto.rs",
        functions: &["truly_old_missing_response_policy_uses_protected_original_registration_and_preserves_bytes"],
        harness_prefix: "runtime::effects::crypto::missing_response_policy_history_tests::",
        filter: "runtime::effects::crypto::missing_response_policy_history_tests::",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/runtime/effects/crypto.rs",
        functions: &["original_response_quorum_is_distinct_from_signing_policy"],
        harness_prefix: "runtime::effects::crypto::",
        filter: "runtime::effects::crypto::original_response_quorum_is_distinct_from_signing_policy",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/runtime/services/enrollment_window.rs",
        functions: &[
            "actual_admitted_checkpoint_updates_without_mutating_anchor_and_never_repairs_live_loss",
            "original_legacy_interval_is_attenuated_without_clamping_fresh_signed_window",
        ],
        harness_prefix: "runtime::services::enrollment_window::admitted_clock_split_tests::",
        filter: "runtime::services::enrollment_window::admitted_clock_split_tests::",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/runtime/effects/crypto.rs",
        functions: &[
            "protected_legacy_profile_migration_preserves_registered_first_decision",
            "same_epoch_reissue_preserves_original_history_and_rejects_mutable_registration_forgery",
        ],
        harness_prefix: "runtime::effects::crypto::enrollment_generation_history_tests::",
        filter: "runtime::effects::crypto::enrollment_generation_history_tests::",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/handlers/invitation/enrollment_parent_archive.rs",
        functions: &["confirmed_archive_evidence_is_move_owned_and_not_deserializable"],
        harness_prefix: "handlers::invitation::enrollment_parent_archive::guards::",
        filter: "handlers::invitation::enrollment_parent_archive::guards::confirmed_archive_evidence_is_move_owned_and_not_deserializable",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/handlers/invitation/enrollment_vm_admission.rs",
        functions: &["real_committed_confirmation_is_durable_and_reverified_before_activation_capability"],
        harness_prefix: "handlers::invitation::enrollment_vm_admission::committed_receipt_tests::",
        filter: "handlers::invitation::enrollment_vm_admission::committed_receipt_tests::real_committed_confirmation_is_durable_and_reverified_before_activation_capability",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/task_registry.rs",
        functions: &["completed_primary_subsidiary_failure_retains_source_in_health_and_drain"],
        harness_prefix: "task_registry::registered_task_context_tests::",
        filter: "task_registry::registered_task_context_tests::completed_primary_subsidiary_failure_retains_source_in_health_and_drain",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/handlers/invitation_service.rs",
        functions: &["public_live_cancellation_retry_preserves_primary_and_original_window"],
        harness_prefix: "handlers::invitation_service::required_enrollment_task_tests::",
        filter: "handlers::invitation_service::required_enrollment_task_tests::public_live_cancellation_retry_preserves_primary_and_original_window",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/handlers/invitation/tests.rs",
        functions: &["two_runtime_cancelled_notice_recovers_original_window_after_restart"],
        harness_prefix: "handlers::invitation::tests::",
        filter: "handlers::invitation::tests::two_runtime_cancelled_notice_recovers_original_window_after_restart",
    },

    LifecycleSuite {
        source: "crates/aura-agent/src/handlers/invitation_service.rs",
        functions: &[
            "cancelled_notice_owner_rejects_active_and_preserves_original_interval",
            "cancelled_notice_owner_expiry_preserves_cancelled_without_new_window",
            "cancelled_notice_owner_requires_original_checkpoint_and_retains_storage_failure",
            "cancelled_notice_owner_rejects_actual_foreign_runtime",
        ],
        harness_prefix: "handlers::invitation_service::required_enrollment_task_tests::",
        filter: "handlers::invitation_service::required_enrollment_task_tests::cancelled_notice_",
    },

    LifecycleSuite {
        source: "crates/aura-agent/src/runtime/subsystems/choreography.rs",
        functions: &["equal_metadata_from_another_runtime_cannot_authorize_owner_mutation"],
        harness_prefix: "runtime::subsystems::choreography::tests::",
        filter: "runtime::subsystems::choreography::tests::",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/runtime/vm_host_bridge.rs",
        functions: &[
            "threaded_close_reaps_exact_session_and_retains_actual_failure",
            "close_and_reap_vm_session_removes_exact_cooperative_target",
        ],
        harness_prefix: "runtime::vm_host_bridge::tests::",
        filter: "runtime::vm_host_bridge::tests::",
    },
    LifecycleSuite {
        source: "crates/aura-agent/src/runtime/session_ingress.rs",
        functions: &[
            "dropping_live_vm_retires_exact_runtime_binding_and_fragment_custody",
            "stale_dropped_vm_cannot_retire_transferred_runtime_owner",
            "actual_supervisor_abort_retires_live_vm_before_idle_publication",
            "guardian_terminal_requires_actual_vm_close_and_preserves_both_native_failures",
        ],
        harness_prefix: "runtime::session_ingress::tests::",
        filter: "runtime::session_ingress::tests::",
    },
];

pub(super) fn require_tests(source: &str, expected: &[&str]) -> Result<()> {
    fn meta_is_ignored(meta: &syn::Meta) -> bool {
        if meta.path().is_ident("ignore") {
            return true;
        }
        if !meta.path().is_ident("cfg_attr") {
            return false;
        }
        let syn::Meta::List(list) = meta else {
            return true;
        };
        match list.parse_args_with(
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
        ) {
            Ok(arguments) => arguments.iter().skip(1).any(meta_is_ignored),
            // Malformed conditional attributes cannot satisfy required evidence.
            Err(_) => true,
        }
    }
    struct LargeStackTestDeclaration {
        name: syn::Ident,
    }
    impl syn::parse::Parse for LargeStackTestDeclaration {
        fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
            let name = input.parse()?;
            input.parse::<syn::Token![,]>()?;
            input.parse::<syn::Block>()?;
            if !input.is_empty() {
                return Err(input.error("unexpected async test declaration tokens"));
            }
            Ok(Self { name })
        }
    }
    struct TestInventory {
        required: BTreeSet<String>,
        observed: BTreeSet<String>,
    }
    impl<'ast> Visit<'ast> for TestInventory {
        fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
            let name = item.sig.ident.to_string();
            if self.required.contains(&name) {
                let is_test = item.attrs.iter().any(|attribute| {
                    let path = attribute.path();
                    path.is_ident("test")
                        || (path.segments.len() == 2
                            && path.segments[0].ident == "tokio"
                            && path.segments[1].ident == "test")
                });
                if is_test
                    && !item
                        .attrs
                        .iter()
                        .any(|attribute| meta_is_ignored(&attribute.meta))
                {
                    self.observed.insert(name);
                }
            }
            syn::visit::visit_item_fn(self, item);
        }
        fn visit_item_macro(&mut self, item: &'ast syn::ItemMacro) {
            if item.mac.path.is_ident("large_stack_async_test")
                && !item
                    .attrs
                    .iter()
                    .any(|attribute| meta_is_ignored(&attribute.meta))
            {
                if let Ok(declaration) =
                    syn::parse2::<LargeStackTestDeclaration>(item.mac.tokens.clone())
                {
                    let name = declaration.name.to_string();
                    if self.required.contains(&name) {
                        self.observed.insert(name);
                    }
                }
            }
            syn::visit::visit_item_macro(self, item);
        }
    }
    let syntax = syn::parse_file(source).context("parse required VM lifecycle evidence")?;
    let mut inventory = TestInventory {
        required: expected.iter().map(|name| (*name).to_owned()).collect(),
        observed: BTreeSet::new(),
    };
    inventory.visit_file(&syntax);
    let missing: Vec<_> = inventory.required.difference(&inventory.observed).collect();
    if !missing.is_empty() {
        bail!("vm-session-lifecycle: required nonignored tests missing: {missing:?}");
    }
    Ok(())
}

pub(super) fn require_executed_tests(output: &str, required: &[String]) -> Result<()> {
    let executed: BTreeSet<_> = output
        .lines()
        .filter_map(|line| {
            line.strip_prefix("test ")
                .and_then(|line| line.strip_suffix(" ... ok"))
                .map(|name| {
                    name.strip_suffix(" - compile fail")
                        .or_else(|| name.strip_suffix(" - compile"))
                        .unwrap_or(name)
                })
        })
        .collect();
    for name in required {
        if !executed.contains(name.as_str()) {
            bail!("vm-session-lifecycle: required test did not execute successfully: {name}");
        }
    }
    Ok(())
}

/// Bind execution evidence to the exact item published by rustdoc, including
/// its source line. Duplicate or similarly named entries cannot fill a guard.
fn required_doctests(listing: &str, item: &str, count: usize) -> Result<Vec<String>> {
    let marker = format!(" - {item} (line ");
    let mut required = BTreeSet::new();
    for name in listing
        .lines()
        .filter_map(|line| line.strip_suffix(": test"))
    {
        let Some((_, location)) = name.split_once(&marker) else {
            continue;
        };
        let Some(line) = location.strip_suffix(')') else {
            continue;
        };
        if line.is_empty() || !line.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        if !required.insert(name.to_owned()) {
            bail!("vm-session-lifecycle: duplicate required doctest: {name}");
        }
    }
    if required.len() != count {
        bail!(
            "vm-session-lifecycle: required doctests for {item} missing or changed: {}/{count}",
            required.len()
        );
    }
    Ok(required.into_iter().collect())
}

fn run_suites(root: &Path, base: &[String], suites: &[LifecycleSuite]) -> Result<()> {
    for suite in suites {
        let source = fs::read_to_string(root.join(suite.source))
            .with_context(|| format!("read required lifecycle inventory {}", suite.source))?;
        require_tests(&source, suite.functions)?;
    }
    let mut discovery = base.to_vec();
    discovery.extend(["--".into(), "--list".into()]);
    let listing = command_stdout("cargo", &discovery)?;
    let published: BTreeSet<&str> = listing
        .lines()
        .filter_map(|line| line.strip_suffix(": test"))
        .collect();
    for suite in suites {
        for function in suite.functions {
            let exact = format!("{}{function}", suite.harness_prefix);
            if !exact.contains(suite.filter) {
                bail!("vm-session-lifecycle: configured filter does not select required test: {exact}");
            }
            if !published.contains(exact.as_str()) {
                bail!(
                    "vm-session-lifecycle: required test not published by actual harness: {exact}"
                );
            }
        }
        let mut args = base.to_vec();
        args.extend([suite.filter.into(), "--".into(), "--nocapture".into()]);
        let output = command_stdout("cargo", &args)?;
        let required: Vec<_> = suite
            .functions
            .iter()
            .map(|function| format!("{}{function}", suite.harness_prefix))
            .collect();
        require_executed_tests(&output, &required)?;
    }
    Ok(())
}

fn released_machine_manifest(metadata: &str) -> Result<std::path::PathBuf> {
    let metadata: serde_json::Value = serde_json::from_str(metadata)?;
    let packages = metadata["packages"]
        .as_array()
        .context("Cargo package metadata missing")?;
    let matches: Vec<_> = packages
        .iter()
        .filter(|package| package["name"] == "telltale-machine")
        .collect();
    if matches.len() != 1 {
        bail!("required lifecycle gate needs exactly one resolved telltale-machine package");
    }
    let package = matches[0];
    if package["version"] != "17.0.1"
        || package["source"] != "registry+https://github.com/rust-lang/crates.io-index"
    {
        bail!("required lifecycle gate needs released crates.io telltale-machine17.0.1");
    }
    let manifest = package["manifest_path"]
        .as_str()
        .context("released machine manifest missing")?;
    let manifest = std::path::PathBuf::from(manifest);
    if !manifest.is_absolute() || manifest.file_name().is_none_or(|name| name != "Cargo.toml") {
        bail!("released machine manifest must be an absolute Cargo.toml path");
    }
    Ok(manifest)
}

pub fn run() -> Result<()> {
    let root = repo_root()?;
    let target = env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("target"));
    let target = if target.is_absolute() {
        target
    } else {
        root.join(target)
    };
    let common = vec![
        "test".into(),
        "--lib".into(),
        "--target-dir".into(),
        target.to_string_lossy().into_owned(),
    ];
    let mut dependency = common.clone();
    let metadata = command_stdout(
        "cargo",
        &[
            "metadata".into(),
            "--format-version".into(),
            "1".into(),
            "--locked".into(),
            "--manifest-path".into(),
            root.join("Cargo.toml").to_string_lossy().into_owned(),
        ],
    )?;
    let dependency_manifest = released_machine_manifest(&metadata)?.canonicalize()?;
    let dependency_root = dependency_manifest
        .parent()
        .context("released machine package root missing")?;
    dependency.extend([
        "--manifest-path".into(),
        dependency_manifest.to_string_lossy().into_owned(),
        "--features".into(),
        "multi-thread".into(),
    ]);
    run_suites(dependency_root, &dependency, DEPENDENCY_SUITES)?;
    let mut core = common.clone();
    core.extend(["-p".into(), "hxrts-aura-core".into()]);
    run_suites(&root, &core, CORE_SUITES)?;
    run_suites(&root, &core, TERMINAL_OBSERVATION_SUITES)?;
    run_absolute_deadline_domain_doctest(&target)?;
    let mut effects = common.clone();
    effects.extend(["-p".into(), "hxrts-aura-effects".into()]);
    run_suites(&root, &effects, NATIVE_ABSOLUTE_PROVIDER_SUITES)?;
    let mut signature = common.clone();
    signature.extend(["-p".into(), "hxrts-aura-signature".into()]);
    run_suites(&root, &signature, SIGNATURE_SUITES)?;
    let mut testkit = common.clone();
    testkit.extend(["-p".into(), "aura-testkit".into()]);
    run_suites(&root, &testkit, ABSOLUTE_PROVIDER_SUITES)?;
    run_suites(&root, &testkit, &[LifecycleSuite {
        source: "crates/aura-testkit/src/time/manual_physical_clock.rs",
        functions: &["provider_faults_are_one_shot_and_wake_original_waiting_sleep"],
        harness_prefix: "time::manual_physical_clock::tests::",
        filter: "time::manual_physical_clock::tests::provider_faults_are_one_shot_and_wake_original_waiting_sleep",
    }])?;
    let mut sync = common.clone();
    sync.extend(["-p".into(), "hxrts-aura-sync".into()]);
    run_suites(&root, &sync, SYNC_NATIVE_SOURCE_SUITES)?;
    for (item, count) in [
        ("core::errors::SyncDiagnostic", 1),
        ("core::errors::sync_error_with_cause", 2),
    ] {
        let docs = vec![
            "test".into(),
            "--doc".into(),
            "--target-dir".into(),
            target.to_string_lossy().into_owned(),
            "-p".into(),
            "hxrts-aura-sync".into(),
            item.into(),
        ];
        let mut discovery = docs.clone();
        discovery.extend(["--".into(), "--list".into()]);
        let required = required_doctests(&command_stdout("cargo", &discovery)?, item, count)?;
        require_executed_tests(&command_stdout("cargo", &docs)?, &required)?;
    }
    let mut agent = common;
    agent.extend(["-p".into(), "hxrts-aura-agent".into()]);
    run_suites(&root, &agent, AGENT_SUITES)?;
    let owner_docs = vec![
        "test".into(),
        "--doc".into(),
        "--target-dir".into(),
        target.to_string_lossy().into_owned(),
        "-p".into(),
        "hxrts-aura-agent".into(),
        "SessionOwnerCapability".into(),
    ];
    let mut discovery = owner_docs.clone();
    discovery.extend(["--".into(), "--list".into()]);
    let listing = command_stdout("cargo", &discovery)?;
    let required_owner = required_doctests(
        &listing,
        "runtime::subsystems::choreography::SessionOwnerCapability",
        4,
    )?;
    require_executed_tests(&command_stdout("cargo", &owner_docs)?, &required_owner)?;
    let mut activity_docs = owner_docs.clone();
    *activity_docs.last_mut().context("activity guard filter")? =
        "runtime::system::RuntimeSystem::activity_gate".into();
    let mut activity_discovery = activity_docs.clone();
    activity_discovery.extend(["--".into(), "--list".into()]);
    let listing = command_stdout("cargo", &activity_discovery)?;
    let required_activity =
        required_doctests(&listing, "runtime::system::RuntimeSystem::activity_gate", 3)?;
    require_executed_tests(
        &command_stdout("cargo", &activity_docs)?,
        &required_activity,
    )?;
    let mut contact_docs = owner_docs;
    *contact_docs
        .last_mut()
        .context("contact verifier guard filter")? =
        "handlers::invitation::InvitationHandler".into();
    let mut contact_discovery = contact_docs.clone();
    contact_discovery.extend(["--".into(), "--list".into()]);
    let required_contact = required_doctests(
        &command_stdout("cargo", &contact_discovery)?,
        "handlers::invitation::InvitationHandler",
        6,
    )?;
    require_executed_tests(&command_stdout("cargo", &contact_docs)?, &required_contact)?;
    let mut command_docs = contact_docs;
    let item = "runtime::services::sync_command_registry::AdmittedSyncCommandCapability";
    *command_docs
        .last_mut()
        .context("registered sync custody guard filter")? = item.into();
    let mut command_discovery = command_docs.clone();
    command_discovery.extend(["--".into(), "--list".into()]);
    let required_command =
        required_doctests(&command_stdout("cargo", &command_discovery)?, item, 2)?;
    require_executed_tests(&command_stdout("cargo", &command_docs)?, &required_command)?;
    println!("vm-session-lifecycle: required target retirement evidence clean");

    Ok(())
}

/// Focused required source/discovery/execution lane for original terminal observations.
pub fn run_absolute_time_observation() -> Result<()> {
    let root = repo_root()?;
    let target = env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("target"));
    let target = if target.is_absolute() {
        target
    } else {
        root.join(target)
    };
    for (package, suites) in [
        ("hxrts-aura-core", TERMINAL_OBSERVATION_SUITES),
        ("hxrts-aura-effects", NATIVE_ABSOLUTE_PROVIDER_SUITES),
        ("hxrts-aura-agent", NATIVE_RUNTIME_OBSERVATION_SUITES),
        ("aura-testkit", ABSOLUTE_PROVIDER_SUITES),
    ] {
        let args = vec![
            "test".into(),
            "--lib".into(),
            "--target-dir".into(),
            target.to_string_lossy().into_owned(),
            "-p".into(),
            package.into(),
        ];
        run_suites(&root, &args, suites)?;
    }
    let app = vec![
        "test".into(),
        "--lib".into(),
        "--target-dir".into(),
        target.to_string_lossy().into_owned(),
        "-p".into(),
        "hxrts-aura-app".into(),
        "--features".into(),
        "native,app-internals,web-js".into(),
    ];
    run_suites(&root, &app, APP_OBSERVATION_SUITES)?;
    run_absolute_deadline_domain_doctest(&target)?;
    println!("absolute-time-observation: required original observation evidence clean");
    Ok(())
}

fn run_absolute_deadline_domain_doctest(target: &Path) -> Result<()> {
    let args = vec![
        "test".into(),
        "--doc".into(),
        "--target-dir".into(),
        target.to_string_lossy().into_owned(),
        "-p".into(),
        "hxrts-aura-core".into(),
        "PhysicalTimeEffects::wait_until_physical_deadline".into(),
    ];
    let mut discovery = args.clone();
    discovery.extend(["--".into(), "--list".into()]);
    let required = required_doctests(
        &command_stdout("cargo", &discovery)?,
        "effects::time::PhysicalTimeEffects::wait_until_physical_deadline",
        1,
    )?;
    require_executed_tests(&command_stdout("cargo", &args)?, &required)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn released_machine_requires_unique_registry_release_provenance() {
        let package = serde_json::json!({
            "name": "telltale-machine", "version": "17.0.1",
            "source": "registry+https://github.com/rust-lang/crates.io-index",
            "manifest_path": "/registry/telltale-machine-17.0.1/Cargo.toml"
        });
        let metadata = |packages| serde_json::json!({"packages": packages}).to_string();
        assert!(released_machine_manifest(&metadata(vec![package.clone()])).is_ok());
        assert!(released_machine_manifest(&metadata(Vec::<serde_json::Value>::new())).is_err());
        assert!(
            released_machine_manifest(&metadata(vec![package.clone(), package.clone()])).is_err()
        );
        for (field, value) in [
            ("version", serde_json::json!("15.0.0")),
            ("source", serde_json::Value::Null),
            (
                "source",
                serde_json::json!("git+https://github.com/hxrts/telltale"),
            ),
            ("manifest_path", serde_json::json!("third_party/Cargo.toml")),
        ] {
            let mut invalid = package.clone();
            invalid[field] = value;
            assert!(released_machine_manifest(&metadata(vec![invalid])).is_err());
        }
    }

    #[test]
    fn doctest_evidence_requires_unique_exact_discovery_and_actual_execution() {
        let item = "runtime::Owner";
        let first = "src/owner.rs - runtime::Owner (line 10)";
        let second = "src/owner.rs - runtime::Owner (line 20)";
        let listing = format!("{first}: test\n{second}: test\n");
        let required = required_doctests(&listing, item, 2).unwrap();
        let passed = format!("test {first} ... ok\ntest {second} - compile fail ... ok\n");
        assert!(require_executed_tests(&passed, &required).is_ok());
        for listing in [
            String::new(),
            format!("{first}: test\n{first}: test\n"),
            format!("{first}: test\nsrc/owner.rs - runtime::OwnerOther (line 20): test\n"),
            format!("{first}: test\nsrc/owner.rs - runtime::Owner (line invalid): test\n"),
            format!("{listing}src/owner.rs - runtime::Owner (line 30): test\n"),
        ] {
            assert!(required_doctests(&listing, item, 2).is_err(), "{listing}");
        }
        for terminal in ["ignored", "FAILED"] {
            let output =
                format!("test {first} ... ok\ntest {second} - compile fail ... {terminal}\n");
            assert!(require_executed_tests(&output, &required).is_err());
        }
        assert!(require_executed_tests("test result: ok. 0 passed\n", &required).is_err());
        assert!(require_executed_tests(&format!("test {first} ... ok\n"), &required).is_err());
    }

    #[test]
    fn required_suites_select_their_actual_package_harness() {
        assert!(!DEPENDENCY_SUITES.is_empty());
        assert!(!AGENT_SUITES.is_empty());
        assert!(!CORE_SUITES.is_empty());
        for suite in CORE_SUITES {
            assert!(suite.source.starts_with("crates/aura-core/"));
        }
        assert!(!SIGNATURE_SUITES.is_empty());
        for suite in SIGNATURE_SUITES {
            assert!(suite.source.starts_with("crates/aura-signature/"));
        }
        for suite in TERMINAL_OBSERVATION_SUITES {
            assert!(suite.source.starts_with("crates/aura-core/"));
        }
        assert!(!NATIVE_RUNTIME_OBSERVATION_SUITES.is_empty());
        for suite in NATIVE_RUNTIME_OBSERVATION_SUITES {
            assert!(suite.source.starts_with("crates/aura-agent/"));
        }
        assert!(!APP_OBSERVATION_SUITES.is_empty());
        for suite in APP_OBSERVATION_SUITES {
            assert!(suite.source.starts_with("crates/aura-app/"));
        }
        for suite in ABSOLUTE_PROVIDER_SUITES {
            assert!(suite.source.starts_with("crates/aura-testkit/"));
        }
        for suite in NATIVE_ABSOLUTE_PROVIDER_SUITES {
            assert!(suite.source.starts_with("crates/aura-effects/"));
        }
        for suite in DEPENDENCY_SUITES {
            assert!(suite.source.starts_with("tests/"));
        }
        for suite in AGENT_SUITES {
            assert!(suite.source.starts_with("crates/aura-agent/"));
        }
        let contact = AGENT_SUITES
            .iter()
            .find(|suite| {
                suite
                    .harness_prefix
                    .ends_with("owned_contact_continuation_tests::")
            })
            .expect("required Contact suite selects actual agent harness");
        assert_eq!(contact.functions.len(), 4);
    }

    #[test]
    fn execution_evidence_rejects_zero_ignored_failed_and_similar_names() {
        let required = vec!["owner::required".to_owned()];
        assert!(require_executed_tests("test owner::required ... ok\n", &required).is_ok());
        for output in [
            "test result: ok. 0 passed; 0 failed; 0 ignored\n",
            "test owner::required ... ignored\n",
            "test owner::required ... FAILED\n",
            "test owner::required_other ... ok\n",
            "owner::required: test\n",
        ] {
            assert!(
                require_executed_tests(output, &required).is_err(),
                "{output}"
            );
        }
    }
    #[test]
    fn execution_evidence_matches_actual_rustdoc_compile_suffixes() {
        let required = vec!["src/example.rs - Owner::boundary (line 10)".to_owned()];
        for suffix in [" - compile", " - compile fail"] {
            let output = format!("test {}{suffix} ... ok\n", required[0]);
            assert!(require_executed_tests(&output, &required).is_ok());
        }
        for output in [
            "test src/example.rs - Owner::boundary (line 11) - compile ... ok\n",
            "test src/example.rs - Owner::boundary (line 10) - compile fail ... FAILED\n",
            "test src/example.rs - Owner::boundary (line 10) - compile ... ignored\n",
            "src/example.rs - Owner::boundary (line 10): test\n",
        ] {
            assert!(require_executed_tests(output, &required).is_err());
        }
    }
    #[test]
    fn inventory_validates_typed_async_macro_and_rejects_ignored_malformed_or_unrelated() {
        assert!(require_tests(
            "large_stack_async_test!(target, { let _ = 1; });",
            &["target"]
        )
        .is_ok());
        for source in [
            "#[ignore] large_stack_async_test!(target, {});",
            "#[cfg_attr(any(), cfg_attr(any(), ignore))] large_stack_async_test!(target, {});",
            "large_stack_async_test!(target);",
            "large_stack_async_test!(target, 123);",
            "large_stack_async_test!(target, {}, extra);",
            "unrelated_test!(target, {});",
            "unrelated::large_stack_async_test!(target, {});",
            "large_stack_async_test!(another, {});",
            "// large_stack_async_test!(target, {});",
        ] {
            assert!(require_tests(source, &["target"]).is_err(), "{source}");
        }
    }
    #[test]
    fn required_quorum_window_and_control_evidence_rejects_missing_ignored_or_zero_execution() {
        for name in [
            "original_completion_observation_releases_execution_lease_without_renewing_clock",
            "actual_admitted_vm_control_and_acceptance_reject_foreign_decisions",
            "malformed_native_package_retains_original_codec_cause",
        ] {
            let suite = AGENT_SUITES
                .iter()
                .find(|suite| suite.functions.contains(&name))
                .expect("actual quorum boundary fixture must remain required");
            let exact = format!("{}{name}", suite.harness_prefix);
            assert_eq!(suite.filter, exact);
            assert!(require_tests(
                &format!("#[tokio::test] async fn {name}() {{}}"),
                suite.functions
            )
            .is_ok());
            assert!(require_tests(
                &format!("#[tokio::test] #[ignore] async fn {name}() {{}}"),
                suite.functions
            )
            .is_err());
            assert!(
                require_tests("#[tokio::test] async fn unrelated() {}", suite.functions).is_err()
            );
            let required = vec![exact.clone()];
            assert!(require_executed_tests("test result: ok. 0 passed", &required).is_err());
            assert!(
                require_executed_tests(&format!("test {exact} ... ignored"), &required).is_err()
            );
            assert!(
                require_executed_tests(&format!("test {exact}_lookalike ... ok"), &required)
                    .is_err()
            );
            assert!(require_executed_tests(&format!("test {exact} ... ok"), &required).is_ok());
        }
    }

    #[test]
    fn inventory_rejects_missing_ignored_and_fabricated_test_attributes() {
        assert!(require_tests("#[test] fn target() {}", &["target"]).is_ok());
        assert!(require_tests("#[tokio::test] async fn target() {}", &["target"]).is_ok());
        assert!(require_tests("#[test] fn another() {}", &["target"]).is_err());
        assert!(require_tests("#[test] #[ignore] fn target() {}", &["target"]).is_err());
        assert!(require_tests(
            "#[test] #[cfg_attr(any(), ignore)] fn target() {}",
            &["target"]
        )
        .is_err());
        assert!(require_tests(
            "#[test] #[cfg_attr(any(), cfg_attr(any(), ignore))] fn target() {}",
            &["target"]
        )
        .is_err());
        assert!(require_tests("#[test] #[cfg_attr(any(), cfg_attr(any(), ignore = \"not mandatory\"))] fn target() {}", &["target"]).is_err());
        assert!(require_tests("#[fake::test] fn target() {}", &["target"]).is_err());
        assert!(require_tests("// #[test] fn target() {}", &["target"]).is_err());
    }
}
