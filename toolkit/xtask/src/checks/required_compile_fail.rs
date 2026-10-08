//! Exact source, discovery and execution evidence for required ownership suites.
use super::support::{command_stdout, repo_root};
use super::vm_session_lifecycle::{require_executed_tests, require_tests};
use anyhow::{bail, Context, Result};
use std::collections::BTreeSet;

struct RequiredSuite {
    package: &'static str,
    harness: &'static str,
    features: &'static [&'static str],
    source: &'static str,
    tests: &'static [&'static str],
}
const SUITES: &[RequiredSuite] = &[
    RequiredSuite {
        package: "hxrts-aura-agent",
        harness: "custom_provider_fidelity",
        features: &[],
        source: "crates/aura-agent/tests/custom_provider_fidelity.rs",
        tests: &[
            "custom_async_builder_retains_selected_handlers_and_native_outages",
            "custom_sync_builder_retains_selected_handlers_and_native_outages",
            "custom_crypto_failure_and_selected_transport_failure_cannot_fall_back",
            "custom_persistent_crypto_uses_same_selected_provider_and_source",
            "interleaved_custom_ingress_retains_other_content_and_source_context_owner",
        ],
    },
    RequiredSuite {
        package: "hxrts-aura-core",
        harness: "compile_fail",
        features: &[],
        source: "crates/aura-core/tests/compile_fail.rs",
        tests: &["ownership_compile_fail_guards"],
    },
    RequiredSuite {
        package: "hxrts-aura-macros",
        harness: "compile_fail",
        features: &[],
        source: "crates/aura-macros/tests/compile_fail.rs",
        tests: &["choreography_annotation_validation"],
    },
    RequiredSuite {
        package: "hxrts-aura-macros",
        harness: "marker_attrs_compile_fail",
        features: &[],
        source: "crates/aura-macros/tests/marker_attrs_compile_fail.rs",
        tests: &["marker_attribute_validation"],
    },
    RequiredSuite {
        package: "hxrts-aura-macros",
        harness: "service_surface_compile_fail",
        features: &[],
        source: "crates/aura-macros/tests/service_surface_compile_fail.rs",
        tests: &["service_surface_validation"],
    },
    RequiredSuite {
        package: "hxrts-aura-app",
        harness: "compile_fail",
        features: &[],
        source: "crates/aura-app/tests/compile_fail.rs",
        tests: &["strong_command_compile_fail_guards"],
    },
    RequiredSuite {
        package: "hxrts-aura-app",
        harness: "compile_fail_signals",
        features: &["signals"],
        source: "crates/aura-app/tests/compile_fail_signals.rs",
        tests: &["signals_compile_fail_guards"],
    },
    RequiredSuite {
        package: "hxrts-aura-agent",
        harness: "compile_fail",
        features: &[],
        source: "crates/aura-agent/tests/compile_fail.rs",
        tests: &["ui", "authorization_cache_publication_is_private"],
    },
    RequiredSuite {
        package: "aura-testkit",
        harness: "process_lock",
        features: &[],
        source: "crates/aura-testkit/tests/process_lock.rs",
        tests: &[
            "forced_process_termination_releases_same_persistent_lock_file",
            "all_compile_fail_suites_share_the_workspace_namespace",
            "invalid_lock_parent_retains_actual_io_cause",
        ],
    },
];
pub(super) fn require_discovered(listing: &str, required: &[String]) -> Result<()> {
    let published: BTreeSet<_> = listing
        .lines()
        .filter_map(|line| line.strip_suffix(": test"))
        .collect();
    for name in required {
        if !published.contains(name.as_str()) {
            bail!("required ownership test not discovered by actual harness: {name}");
        }
    }
    Ok(())
}
fn suite_arguments(suite: &RequiredSuite) -> Vec<String> {
    let mut args = vec![
        "test".into(),
        "-p".into(),
        suite.package.into(),
        "--test".into(),
        suite.harness.into(),
    ];
    if !suite.features.is_empty() {
        args.extend(["--features".into(), suite.features.join(",")]);
    }
    args
}
pub(super) fn run() -> Result<()> {
    let root = repo_root()?;
    for suite in SUITES {
        let source = std::fs::read_to_string(root.join(suite.source))
            .with_context(|| format!("read required ownership suite {}", suite.source))?;
        require_tests(&source, suite.tests)?;
        let required: Vec<_> = suite.tests.iter().map(|test| (*test).to_owned()).collect();
        let base = suite_arguments(suite);
        let mut discovery = base.clone();
        discovery.extend(["--".into(), "--list".into()]);
        require_discovered(&command_stdout("cargo", &discovery)?, &required)?;
        let mut execution = base;
        // Captured test output prevents trybuild's nested diagnostics from
        // splitting the outer harness's exact successful test-result lines.
        execution.extend([
            "--".into(),
            "--format".into(),
            "pretty".into(),
            "--test-threads=1".into(),
        ]);
        require_executed_tests(&command_stdout("cargo", &execution)?, &required)?;
    }
    require_exact_capability_declaration_regressions()?;
    require_rng_custody_regressions()?;
    require_selected_provider_custody_regressions()?;
    require_test_namespace_regressions()?;
    require_protocol_tree_regressions()?;
    require_spawn_configuration_regression()?;
    Ok(())
}
fn require_exact_capability_declaration_regressions() -> Result<()> {
    let root = repo_root()?;
    let tests = [
        "full_validator_rejects_all_decorative_evidence",
        "full_validator_checks_accessor_authorizer_and_issuer_contracts",
        "full_validator_supports_exact_semantic_label_and_typed_receiver_contract",
    ];
    let source = std::fs::read_to_string(root.join("crates/aura-macros/src/lib.rs"))
        .context("read exact capability declaration regressions")?;
    require_tests(&source, &tests)?;
    let required = tests
        .iter()
        .map(|test| format!("declared_capability_signature_tests::{test}"))
        .collect::<Vec<_>>();
    let base: Vec<String> = ["test", "-p", "hxrts-aura-macros", "--lib"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    let mut discovery = base.clone();
    discovery.extend(["--".into(), "--list".into()]);
    require_discovered(&command_stdout("cargo", &discovery)?, &required)?;
    for name in &required {
        let mut execution = base.clone();
        execution.extend([
            name.clone(),
            "--".into(),
            "--exact".into(),
            "--format".into(),
            "pretty".into(),
        ]);
        require_executed_tests(
            &command_stdout("cargo", &execution)?,
            std::slice::from_ref(name),
        )?;
    }
    Ok(())
}
fn require_spawn_configuration_regression() -> Result<()> {
    let root = repo_root()?;
    let name = "spawn_policy_excludes_only_proven_test_configurations";
    let source =
        std::fs::read_to_string(root.join("crates/aura-macros/src/bin/ownership_lints.rs"))
            .context("read production spawn configuration regression")?;
    require_tests(&source, &[name])?;
    let required = vec![format!("tests::{name}")];
    let base: Vec<String> = [
        "test",
        "-p",
        "hxrts-aura-macros",
        "--bin",
        "ownership_lints",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let mut discovery = base.clone();
    discovery.extend(["--".into(), "--list".into()]);
    require_discovered(&command_stdout("cargo", &discovery)?, &required)?;
    let mut execution = base;
    execution.extend([
        required[0].clone(),
        "--".into(),
        "--exact".into(),
        "--format".into(),
        "pretty".into(),
    ]);
    require_executed_tests(&command_stdout("cargo", &execution)?, &required)
}
fn require_rng_custody_regressions() -> Result<()> {
    require_lib_regressions(
        "hxrts-aura-agent",
        "crates/aura-agent/src/runtime/subsystems/crypto.rs",
        "runtime::subsystems::crypto::tests",
        &[
            "cloned_subsystems_continue_one_original_deterministic_stream",
            "independently_seeded_subsystems_remain_reproducible_without_shared_custody",
        ],
    )
}
// Tasks 547 and 605: first-attachment A/B provider substitution and an
// unsupported selected provider must stay exact, executed native regressions.
fn require_selected_provider_custody_regressions() -> Result<()> {
    require_lib_regressions(
        "hxrts-aura-agent",
        "crates/aura-agent/src/runtime/subsystems/crypto.rs",
        "runtime::subsystems::crypto",
        &[
            "selected_secret_handoff_rejects_replacement_and_preexisting_shared_registry",
            "selected_native_provider_without_lifetime_support_retains_structural_unavailability",
        ],
    )
}
fn require_test_namespace_regressions() -> Result<()> {
    require_lib_regressions(
        "hxrts-aura-agent",
        "crates/aura-agent/src/runtime/effects.rs",
        "runtime::effects::test_namespace_tests",
        &[
            "collision_exhaustion_never_returns_or_modifies_a_preexisting_profile",
            "namespace_creation_fault_preserves_native_io_instead_of_returning_a_locator",
        ],
    )
}
fn require_protocol_tree_regressions() -> Result<()> {
    require_lib_regressions(
        "hxrts-aura-protocol",
        "crates/aura-protocol/src/handlers/tree.rs",
        "handlers::tree::tests",
        &[
            "current_decision_lease_excludes_actual_replacement",
            "held_extension_preserves_later_evidence_and_rejects_divergence",
        ],
    )
}
fn require_lib_regressions(
    package: &str,
    source_path: &str,
    module: &str,
    names: &[&str],
) -> Result<()> {
    let root = repo_root()?;
    let source = std::fs::read_to_string(root.join(source_path))
        .with_context(|| format!("read required native owner regressions {source_path}"))?;
    require_tests(&source, names)?;
    let required = names
        .iter()
        .map(|name| format!("{module}::{name}"))
        .collect::<Vec<_>>();
    let base = vec!["test".into(), "-p".into(), package.into(), "--lib".into()];
    let mut discovery = base.clone();
    discovery.extend(["--".into(), "--list".into()]);
    require_discovered(&command_stdout("cargo", &discovery)?, &required)?;
    for name in &required {
        let mut execution = base.clone();
        execution.extend([
            name.clone(),
            "--".into(),
            "--exact".into(),
            "--format".into(),
            "pretty".into(),
        ]);
        require_executed_tests(
            &command_stdout("cargo", &execution)?,
            std::slice::from_ref(name),
        )?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_required_evidence_accepts_real_test_and_rejects_unexecuted_coverage() {
        let required = vec!["required".to_owned()];
        require_tests("#[test] fn required() {}", &["required"]).unwrap();
        require_discovered("required: test\n1 test, 0 benchmarks\n", &required).unwrap();
        require_executed_tests("test required ... ok\n", &required).unwrap();
        for listing in [
            "0 tests, 0 benchmarks\n",
            "required_other: test\n",
            "required: benchmark\n",
        ] {
            assert!(require_discovered(listing, &required).is_err());
        }
        for source in [
            "fn required() {}",
            "#[test] #[ignore] fn required() {}",
            "#[test] #[cfg_attr(feature=\"off\",ignore)] fn required() {}",
        ] {
            assert!(require_tests(source, &["required"]).is_err());
        }
        for output in [
            "test result: ok. 0 passed; 0 failed\n",
            "test required ... ignored\n",
            "test required ... FAILED\n",
            "test required_other ... ok\n",
            "test nested::required ... ok\n",
        ] {
            assert!(require_executed_tests(output, &required).is_err());
        }
    }
    #[test]
    fn actual_inventory_selects_signals_and_each_required_native_lock_regression() {
        let signals = SUITES
            .iter()
            .find(|suite| suite.harness == "compile_fail_signals")
            .unwrap();
        let args = suite_arguments(signals);
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--features", "signals"]));
        let process = SUITES
            .iter()
            .find(|suite| suite.harness == "process_lock")
            .unwrap();
        assert_eq!(process.tests.len(), 3);
        assert!(!process.tests.contains(&"process_lock_child"));
        let listing = process
            .tests
            .iter()
            .map(|test| format!("{test}: test\n"))
            .collect::<String>();
        let required = process
            .tests
            .iter()
            .map(|test| (*test).to_owned())
            .collect::<Vec<_>>();
        require_discovered(&listing, &required).unwrap();
        let output = process
            .tests
            .iter()
            .map(|test| format!("test {test} ... ok\n"))
            .collect::<String>();
        require_executed_tests(&output, &required).unwrap();
        assert!(require_executed_tests("test process_lock_child ... ok\n", &required).is_err());
    }
}
