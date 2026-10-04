//! Actual source, discovery and execution evidence for selected lifetime custody.
use super::required_compile_fail::require_discovered;
use super::support::{command_stdout, repo_root};
use super::vm_session_lifecycle::{require_executed_tests, require_tests};
use anyhow::{Context, Result};

const GROUPS: &[(&str, &[&str])] = &[
    ("crates/aura-effects/src/secure/allocation_lifetime.rs", &[
        "secure::allocation_lifetime::codec_tests::protected_json_second_pass_cannot_grow_its_secret_buffer",
        "secure::allocation_lifetime::codec_tests::protected_json_encoding_preserves_wire_bounds_and_partial_failure_sources",
        "secure::allocation_lifetime::codec_tests::private_record_secret_codec_preserves_wire_and_rejects_unbounded_or_malformed_fields",
        "secure::allocation_lifetime::ack_tests::required_negative_ack_failure_retains_io_and_retry_acknowledges_original",
        "secure::allocation_lifetime::original_owner_missing_leaf_retains_required_storage_absence",
        "secure::allocation_lifetime::tests::allocation_lifetime_reopens_original_negative_tombstone",
        "secure::allocation_lifetime::tests::positive_secret_birth_cannot_be_reclassified_negative",
        "secure::allocation_lifetime::tests::selected_physical_lease_issues_only_one_root_and_denies_generic_namespace",
    ]),
    ("crates/aura-effects/src/secure/allocation_lifetime/initialization.rs", &[
        "secure::allocation_lifetime::initialization::tests::original_initialization_recovers_exact_prelink_ciphertext_after_process_death",
        "secure::allocation_lifetime::initialization::tests::original_mutable_initialization_successors_recover_after_process_death",
        "secure::allocation_lifetime::initialization::tests::original_initial_cutover_requires_exact_archived_custody_on_reopen",
        "secure::allocation_lifetime::initialization::tests::original_initial_cutover_preserves_substituted_source_and_target_evidence",
        "secure::allocation_lifetime::initialization::tests::original_initial_cutover_authenticates_history_and_rejects_unknown_transaction_inventory",
        "secure::allocation_lifetime::initialization::tests::original_initial_cutover_recovers_two_successive_original_index_transitions",
        "secure::allocation_lifetime::initialization::tests::original_initial_cutover_recovers_acknowledged_custody_and_exchange_after_process_death",
        "secure::allocation_lifetime::initialization::tests::original_mutable_successor_requires_independent_seal_and_retains_evidence",
        "secure::allocation_lifetime::initialization::tests::original_mutable_successor_rejects_foreign_physical_profile",
        "secure::allocation_lifetime::initialization::tests::original_mutable_successor_cannot_replay_over_exposed_positive_allocation",
        "secure::allocation_lifetime::initialization::tests::original_initialization_recovers_linked_stage_after_actual_process_death",
        "secure::allocation_lifetime::initialization::tests::unrelated_ciphertext_alias_cannot_authorize_original_link_recovery",
        "secure::allocation_lifetime::initialization::tests::malformed_and_conflicting_prelink_initialization_cannot_replace_original_root",
        "secure::allocation_lifetime::initialization::tests::malformed_prelink_ciphertext_does_not_publish_a_birth_or_replace_key",
        "secure::allocation_lifetime::initialization::tests::anonymous_legacy_stage_is_retained_and_cannot_authorize_a_new_root",
        "secure::allocation_lifetime::initialization::tests::original_recovery_conflict_after_observation_retains_stage_and_native_source",
        "secure::allocation_lifetime::initialization::tests::original_recovery_identical_acknowledged_ciphertext_finishes_without_replacement",
        "secure::allocation_lifetime::initialization::tests::original_recovery_stage_substitution_after_observation_cannot_publish",
        "secure::allocation_lifetime::initialization::tests::unknown_stage_in_other_namespace_blocks_original_and_live_handoff_without_deletion",
        "secure::allocation_lifetime::initialization::tests::initial_stage_names_cannot_authorize_payload_or_survive_handoff_exhaustion",
        "secure::allocation_lifetime::initialization::tests::stage_inventory_streams_max_allocation_layout_and_more_than_4096_ordinary_records",
        "secure::allocation_lifetime::initialization::tests::original_publication_reader_reopens_live_profile_with_more_than_4096_namespace_siblings",
        "secure::allocation_lifetime::initialization::tests::stage_inventory_retains_ambiguity_and_structural_depth_native_causes",
        "secure::allocation_lifetime::initialization::tests::original_recovery_source_path_substitution_between_check_and_link_cannot_acknowledge",
        "secure::allocation_lifetime::initialization::tests::once_live_target_loss_cannot_be_repaired_from_a_stale_original_stage",
        "secure::allocation_lifetime::initialization::tests::conflicting_staged_checkpoint_preserves_actual_positive_first_decision",
        "secure::allocation_lifetime::initialization::tests::authenticated_root_and_anchor_codec_failures_remain_serialization",
        "secure::allocation_lifetime::initialization::tests::completed_legacy_handoff_missing_original_birth_cannot_remigrate",
        "secure::allocation_lifetime::initialization::tests::corrupted_legacy_record_prevents_migration_without_replacing_original_key",
        "secure::allocation_lifetime::initialization::tests::handed_empty_profile_missing_ready_seal_cannot_reinitialize",
        "secure::allocation_lifetime::initialization::tests::interrupted_birth_handoff_recovers_original_secret_once",
        "secure::allocation_lifetime::initialization::tests::interrupted_legacy_migration_reuses_acknowledged_original_birth_anchor",
        "secure::allocation_lifetime::initialization::tests::interrupted_pre_live_initialization_finishes_original_birth_seal",
        "secure::allocation_lifetime::initialization::tests::interrupted_ready_publication_does_not_allocate_a_replacement_root",
        "secure::allocation_lifetime::initialization::tests::legacy_selected_profile_migrates_without_rewriting_permanent_records",
        "secure::allocation_lifetime::initialization::tests::live_owner_requires_original_birth_and_directory_marker_on_every_read",
        "secure::allocation_lifetime::initialization::tests::missing_lifecycle_with_retained_birth_never_reconstructs_phase",
        "secure::allocation_lifetime::initialization::tests::missing_live_leaf_cannot_restore_from_initialization_or_use_empty_inventory",
        "secure::allocation_lifetime::initialization::tests::missing_original_birth_seal_rejects_existing_live_profile",
        "secure::allocation_lifetime::initialization::tests::once_live_missing_checkpoint_fails_without_reconstruction",
        "secure::allocation_lifetime::initialization::tests::retained_birth_cannot_reconstruct_lost_completed_lifetime_state",
    ]),
    ("crates/aura-effects/src/secure/allocation_lifetime/initialization/legacy_migration.rs", &[
        "secure::allocation_lifetime::initialization::legacy_migration::tests::actual_cumulative_legacy_ciphertext_bound_preserves_original_profile",
    ]),
];
pub(super) fn run() -> Result<()> {
    let root = repo_root()?;
    let mut required = Vec::new();
    for (path, tests) in GROUPS {
        let source = std::fs::read_to_string(root.join(path))
            .with_context(|| format!("read selected lifetime regressions {path}"))?;
        let local: Vec<_> = tests
            .iter()
            .map(|name| name.rsplit("::").next().unwrap_or(name))
            .collect();
        require_tests(&source, &local)?;
        required.extend(tests.iter().map(|name| (*name).to_owned()));
    }
    let base: Vec<String> = [
        "test",
        "-p",
        "hxrts-aura-effects",
        "--lib",
        "secure::allocation_lifetime::",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let mut discovery = base.clone();
    discovery.extend(["--".into(), "--list".into()]);
    require_discovered(&command_stdout("cargo", &discovery)?, &required)?;
    let mut execution = base;
    execution.extend(["--".into(), "--format".into(), "pretty".into()]);
    require_executed_tests(&command_stdout("cargo", &execution)?, &required)?;
    require_production_entropy_custody()
}

#[derive(Clone, Copy)]
enum EntropyTestTarget {
    Agent,
    Toolkit,
}
fn entropy_test_command(target: EntropyTestTarget, name: &str) -> Vec<String> {
    let mut command = match target {
        EntropyTestTarget::Agent => vec!["test", "-p", "hxrts-aura-agent", "--lib"],
        EntropyTestTarget::Toolkit => vec![
            "test",
            "--manifest-path",
            "toolkit/xtask/Cargo.toml",
            "--bin",
            "aura-toolkit-xtask",
        ],
    }
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    command.push(name.to_owned());
    command
}
fn require_production_entropy_custody() -> Result<()> {
    for (target, source, name) in [
        (EntropyTestTarget::Agent, "crates/aura-agent/src/runtime/effects.rs", "runtime::effects::tests::production_seed_rejection_precedes_profile_io_with_custom_real_crypto"),
        (EntropyTestTarget::Agent, "crates/aura-agent/src/runtime/effects.rs", "runtime::effects::tests::simulation_constructor_preserves_seeded_stream_across_actual_subsystem_clones"),
        (EntropyTestTarget::Agent, "crates/aura-agent/src/runtime/effects.rs", "runtime::effects::tests::seeded_simulation_constructor_retains_configured_crypto_and_random_dispatch"),
        (EntropyTestTarget::Toolkit, "toolkit/xtask/src/checks/runtime_entropy_scope.rs", "checks::policy::runtime_entropy_scope::tests::checked_entropy_origin_requires_actual_factory_and_typed_receivers"),
        (EntropyTestTarget::Toolkit, "toolkit/xtask/src/checks/runtime_entropy_scope.rs", "checks::policy::runtime_entropy_scope::tests::absolute_entropy_owner_contract_rejects_foreign_suffix_and_relative_path"),
        (EntropyTestTarget::Toolkit, "toolkit/xtask/src/checks/required_secret_lifetime.rs", "checks::required_secret_lifetime::entropy_command_tests::excluded_toolkit_inventory_uses_its_actual_manifest_and_binary"),
    ] {
        let contents = std::fs::read_to_string(repo_root()?.join(source))?;
        require_tests(&contents, &[name.rsplit("::").next().unwrap_or(name)])?;
        let required = vec![name.to_owned()];
        let base = entropy_test_command(target, name);
        let mut discovery = base.clone();
        discovery.extend(["--".into(), "--list".into()]);
        require_discovered(&command_stdout("cargo", &discovery)?, &required)?;
        let mut execution = base;
        execution.extend(["--".into(), "--exact".into(), "--format".into(), "pretty".into()]);
        require_executed_tests(&command_stdout("cargo", &execution)?, &required)?;
    }
    Ok(())
}
#[cfg(test)]
mod entropy_command_tests {
    use super::*;
    #[test]
    fn excluded_toolkit_inventory_uses_its_actual_manifest_and_binary() {
        assert_eq!(
            entropy_test_command(EntropyTestTarget::Toolkit, "exact_test"),
            [
                "test",
                "--manifest-path",
                "toolkit/xtask/Cargo.toml",
                "--bin",
                "aura-toolkit-xtask",
                "exact_test"
            ]
        );
        assert_eq!(
            entropy_test_command(EntropyTestTarget::Agent, "exact_test"),
            ["test", "-p", "hxrts-aura-agent", "--lib", "exact_test"]
        );
    }
}
