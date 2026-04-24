use std::{
    collections::{BTreeSet, HashSet},
    env, fs,
    path::Path,
    process::Command,
};

use anyhow::{bail, Context, Result};
use regex::Regex;

use super::support::{
    command_stdout, contains, first_match_line, git_diff, read, read_lines, repo_relative,
    repo_root, rg_exists, rg_lines, rg_non_comment_lines, run_ok, run_ok_in_dir, rust_files_under,
};

pub fn run_service_registry_ownership() -> Result<()> {
    let repo_root = repo_root()?;
    let legacy =
        repo_root.join("crates/aura-agent/src/runtime/services/rendezvous_cache_manager.rs");
    if legacy.exists() {
        bail!("service-registry-ownership: legacy rendezvous_cache_manager.rs must be removed");
    }

    if !rg_exists(&vec![
        "-n".into(),
        r"#\[aura_macros::actor_owned\(".into(),
        repo_relative(repo_root.join("crates/aura-agent/src/runtime/services/service_registry.rs")),
    ])? {
        bail!(
            "service-registry-ownership: service_registry.rs must declare the actor-owned registry service"
        );
    }

    let legacy_hits = rg_lines(&vec![
        "-n".into(),
        "RendezvousCacheManager|pending_channels|descriptor_cache".into(),
        repo_relative(repo_root.join("crates/aura-agent/src")),
        repo_relative(repo_root.join("crates/aura-sync/src")),
        repo_relative(repo_root.join("crates/aura-rendezvous/src")),
    ])?;
    if !legacy_hits.is_empty() {
        for hit in legacy_hits {
            eprintln!("{hit}");
        }
        bail!(
            "service-registry-ownership: legacy duplicate rendezvous cache ownership paths are still present"
        );
    }

    let duplicate_stores = rg_lines(&vec![
        "-n".into(),
        r"HashMap<\(\s*ContextId,\s*AuthorityId\s*\),\s*RendezvousDescriptor>".into(),
        repo_relative(repo_root.join("crates/aura-agent/src/runtime/services")),
        "-g".into(),
        "*.rs".into(),
    ])?;
    let duplicates: Vec<_> = duplicate_stores
        .into_iter()
        .filter(|hit| {
            !hit.contains("service_registry.rs") && !hit.contains("rendezvous_manager.rs")
        })
        .collect();
    if !duplicates.is_empty() {
        for hit in duplicates {
            eprintln!("{hit}");
        }
        bail!(
            "service-registry-ownership: duplicate runtime descriptor stores detected outside service_registry/rendezvous_manager"
        );
    }

    println!("service-registry-ownership: ok");
    Ok(())
}

pub fn run_runtime_error_boundary() -> Result<()> {
    let repo_root = repo_root()?;
    let violations = rg_non_comment_lines(&vec![
        "-n".into(),
        "AuraError::(internal|terminal|permission_denied|not_found|invalid_input)\\(format!|(?:crate::core::)?AgentError::(internal|runtime|effects|invalid|config)\\(format!|SemanticOperationError::[A-Za-z_]+\\(\\s*format!".into(),
        repo_relative(repo_root.join("crates/aura-app/src/workflows")),
        repo_relative(repo_root.join("crates/aura-agent/src/handlers/invitation")),
        repo_relative(repo_root.join("crates/aura-agent/src/runtime_bridge")),
        repo_relative(repo_root.join("crates/aura-terminal/src/tui")),
        repo_relative(repo_root.join("crates/aura-web/src")),
        "-g".into(),
        "*.rs".into(),
    ])?;
    if !violations.is_empty() {
        for hit in violations {
            eprintln!("{hit}");
        }
        bail!(
            "typed-error-boundary: parity-critical workflow/runtime paths still use stringly primary error construction"
        );
    }
    println!("typed error boundary: clean");
    Ok(())
}

pub fn run_testing_exception_boundary() -> Result<()> {
    let repo_root = repo_root()?;
    let allowed_entries = [
        "crates/aura-testkit/src/flow_budget.rs|host-only deterministic budget test state",
        "crates/aura-testkit/src/handlers/memory/choreographic_memory.rs|stateful in-memory test handler",
        "crates/aura-testkit/src/handlers/mock.rs|stateful mock handler surface",
        "crates/aura-testkit/src/infrastructure/time.rs|deterministic native time fixture state",
        "crates/aura-testkit/src/mock_effects.rs|stateful mock effects surface",
        "crates/aura-testkit/src/mock_runtime_bridge.rs|native-only runtime bridge teardown bookkeeping",
        "crates/aura-testkit/src/stateful_effects/biometric.rs|stateful biometric test double",
        "crates/aura-testkit/src/stateful_effects/console.rs|stateful console test double",
        "crates/aura-testkit/src/stateful_effects/crypto.rs|stateful crypto test double",
        "crates/aura-testkit/src/stateful_effects/journal.rs|stateful journal test double",
        "crates/aura-testkit/src/stateful_effects/random.rs|stateful random test double",
        "crates/aura-testkit/src/stateful_effects/terminal.rs|stateful terminal test double",
        "crates/aura-testkit/src/stateful_effects/time.rs|stateful time test double",
        "crates/aura-testkit/src/stateful_effects/vm_bridge.rs|stateful VM bridge test double",
        "crates/aura-testkit/src/time/controllable_time.rs|controllable deterministic time source",
    ];
    let expected: BTreeSet<_> = allowed_entries
        .iter()
        .map(|entry| entry.split_once('|').unwrap().0.to_string())
        .collect();
    let actual: BTreeSet<_> = rg_lines(&vec![
        "-l".into(),
        r"^#!\[allow\(clippy::disallowed_types\)\]".into(),
        repo_relative(repo_root.join("crates/aura-testkit/src")),
        "-g".into(),
        "*.rs".into(),
    ])?
    .into_iter()
    .collect();
    if expected != actual {
        eprintln!("expected:");
        for path in &expected {
            eprintln!("  {path}");
        }
        eprintln!("actual:");
        for path in &actual {
            eprintln!("  {path}");
        }
        bail!("testkit exception boundary: disallowed_types allowlist drift");
    }

    let lib_file = repo_root.join("crates/aura-testkit/src/lib.rs");
    for target in [
        "pub mod mock_runtime_bridge;",
        "pub use mock_runtime_bridge::MockRuntimeBridge;",
    ] {
        assert_cfg_pair(&lib_file, target)?;
    }

    println!(
        "testkit exception boundary: clean ({} named disallowed_types exceptions)",
        expected.len()
    );
    Ok(())
}

pub fn run_ownership_category_declarations() -> Result<()> {
    let repo_root = repo_root()?;
    let ownership_doc = repo_root.join("docs/122_ownership_model.md");
    let project_structure_doc = repo_root.join("docs/999_project_structure.md");
    let testing_guide = repo_root.join("docs/804_testing_guide.md");

    for file in [
        &ownership_doc,
        &project_structure_doc,
        &testing_guide,
        &repo_root.join("docs/001_system_architecture.md"),
        &repo_root.join("docs/103_effect_system.md"),
        &repo_root.join("docs/104_runtime.md"),
    ] {
        if !file.exists() {
            bail!(
                "ownership-category-declarations: missing required ownership guidance doc: {}",
                file.display()
            );
        }
    }

    let ownership_contents = read(&ownership_doc)?;
    for category in ["`Pure`", "`MoveOwned`", "`ActorOwned`", "`Observed`"] {
        if !ownership_contents.contains(category) {
            bail!(
                "ownership-category-declarations: ownership model doc missing category declaration: {category}"
            );
        }
    }

    let testing_contents = read(&testing_guide)?;
    if !testing_contents.contains("### Shared Semantic Ownership Inventory") {
        bail!(
            "ownership-category-declarations: testing guide must define the shared semantic ownership inventory"
        );
    }
    for row in [
        "Semantic command / handle contract",
        "Semantic operation lifecycle",
        "Channel / invitation / delivery readiness",
        "Runtime-facing async service state",
        "TUI command ingress",
        "TUI shell / callbacks / subscriptions",
        "Browser harness bridge",
        "Harness executor / wait model",
        "Ownership transfer / stale-owner invalidation",
    ] {
        if !testing_contents.contains(row) {
            bail!(
                "ownership-category-declarations: testing guide ownership inventory missing row: {row}"
            );
        }
    }

    let crates_root = repo_root.join("crates");
    let mut missing_arch = Vec::new();
    let mut violations = Vec::new();
    for entry in crates_root.read_dir().context("reading crates/")? {
        let entry = entry?;
        let crate_dir = entry.path();
        if !crate_dir.is_dir() {
            continue;
        }
        let cargo_toml = crate_dir.join("Cargo.toml");
        let src_dir = crate_dir.join("src");
        let arch = crate_dir.join("ARCHITECTURE.md");
        if !cargo_toml.exists() || !src_dir.exists() {
            continue;
        }
        if !arch.exists() {
            missing_arch.push(repo_relative(&arch));
            continue;
        }
        let contents = read(&arch)?;
        let crate_name = crate_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if !(contents.contains("## Ownership Model")
            || contents.contains("## Ownership Inventory")
            || contents.contains("### Ownership Inventory"))
        {
            violations.push(format!(
                "{}: missing ownership section",
                repo_relative(&arch)
            ));
        }
        if !["`Pure`", "`MoveOwned`", "`ActorOwned`", "`Observed`"]
            .iter()
            .any(|category| contents.contains(category))
        {
            violations.push(format!(
                "{}: missing explicit ownership category declarations",
                repo_relative(&arch)
            ));
        }
        if matches!(
            crate_name,
            "aura-agent"
                | "aura-app"
                | "aura-terminal"
                | "aura-web"
                | "aura-harness"
                | "aura-ui"
                | "aura-simulator"
                | "aura-testkit"
        ) && !(contents.contains("## Ownership Inventory")
            || contents.contains("### Ownership Inventory")
            || contents.contains("### Inventory"))
        {
            violations.push(format!(
                "{}: high-risk crate missing Ownership Inventory section",
                repo_relative(&arch)
            ));
        }
        if matches!(
            crate_name,
            "aura-authentication"
                | "aura-chat"
                | "aura-invitation"
                | "aura-recovery"
                | "aura-relational"
                | "aura-rendezvous"
                | "aura-social"
                | "aura-sync"
        ) {
            let lib_file = src_dir.join("lib.rs");
            if !lib_file.exists() {
                violations.push(format!(
                    "{}: Layer 5 crate missing src/lib.rs for OPERATION_CATEGORIES enforcement",
                    repo_relative(&crate_dir)
                ));
            } else if !contains(&lib_file, "pub const OPERATION_CATEGORIES")? {
                violations.push(format!(
                    "{}: Layer 5 crate missing OPERATION_CATEGORIES declaration",
                    repo_relative(&lib_file)
                ));
            }
            if !contents.contains("OPERATION_CATEGORIES") {
                violations.push(format!(
                    "{}: Layer 5 crate must document OPERATION_CATEGORIES linkage",
                    repo_relative(&arch)
                ));
            }
        }
    }

    let agent_arch = repo_root.join("crates/aura-agent/ARCHITECTURE.md");
    if agent_arch.exists() {
        let contents = read(&agent_arch)?;
        if !contents.contains("Structured Concurrency Model") {
            violations.push(format!(
                "{}: must define the structured concurrency model",
                repo_relative(&agent_arch)
            ));
        }
        if !contents.contains("Session Ownership") {
            violations.push(format!(
                "{}: must define session ownership",
                repo_relative(&agent_arch)
            ));
        }
    }

    if !missing_arch.is_empty() {
        for entry in missing_arch {
            eprintln!("{entry}");
        }
        bail!(
            "ownership-category-declarations: crates are missing required ARCHITECTURE.md ownership declarations"
        );
    }
    if !violations.is_empty() {
        for violation in violations {
            eprintln!("{violation}");
        }
        bail!("ownership-category-declarations: ownership category declarations are incomplete");
    }

    println!("ownership category declarations: clean");
    Ok(())
}

pub fn run_ownership_annotation_ratchet(args: &[String]) -> Result<()> {
    let mode = args.first().map(String::as_str).context(
        "ownership-annotation-ratchet: usage: check ownership-annotation-ratchet <semantic-owner|actor-owned|capability-boundary>",
    )?;
    let repo_root = repo_root()?;
    let diff_range = std::env::var("AURA_OWNERSHIP_RATCHET_DIFF_RANGE")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| default_diff_range().ok().flatten())
        .unwrap_or_else(|| "HEAD".to_string());

    let (scope_paths, required_attr) = match mode {
        "semantic-owner" => (
            vec![
                "crates/aura-app/src/workflows",
                "crates/aura-web/src",
                "crates/aura-terminal/src",
            ],
            "#[aura_macros::semantic_owner",
        ),
        "actor-owned" => (
            vec!["crates/aura-agent/src/runtime/services"],
            "#[aura_macros::actor_",
        ),
        "capability-boundary" => (
            vec![
                "crates/aura-app/src/workflows",
                "crates/aura-agent/src/runtime_bridge",
                "crates/aura-agent/src/reactive",
                "crates/aura-chat/src",
                "crates/aura-invitation/src",
                "crates/aura-recovery/src",
            ],
            "#[aura_macros::capability_boundary",
        ),
        other => bail!("ownership-annotation-ratchet: unknown mode: {other}"),
    };

    let diff = git_diff(&{
        let mut args = vec!["diff".into(), "-U3".into(), diff_range.clone(), "--".into()];
        args.extend(scope_paths.iter().map(|path| path.to_string()));
        args
    })
    .unwrap_or_default();
    let mut violations = Vec::new();
    let mut current_file = String::new();
    let mut window: Vec<String> = Vec::new();

    for line in diff.lines() {
        if let Some(file) = line.strip_prefix("+++ b/") {
            current_file = file.to_string();
            window.clear();
            continue;
        }
        if line.starts_with("@@") {
            window.clear();
            continue;
        }
        if let Some(added) = line.strip_prefix('+') {
            if line.starts_with("+++") {
                continue;
            }
            window.push(added.to_string());
            if window.len() > 16 {
                window.remove(0);
            }
            if candidate_requires_attr(mode, &current_file, added)
                && !window.iter().any(|entry| entry.contains(required_attr))
            {
                violations.push(format!(
                    "{current_file}: added boundary appears to require {required_attr} near {added}"
                ));
            }
        }
    }

    for violation in completeness_violations(&repo_root, mode)? {
        violations.push(violation);
    }

    if !violations.is_empty() {
        for violation in violations {
            eprintln!("✖ {violation}");
        }
        bail!("ownership-annotation-ratchet({mode}): violation(s) detected");
    }

    if diff.trim().is_empty() {
        println!(
            "ownership-annotation-ratchet({mode}): no diff in scope; completeness clean (0 named exclusions)"
        );
    } else {
        println!("ownership-annotation-ratchet({mode}): clean (0 named exclusions)");
    }
    Ok(())
}

pub fn run_ownership_workflow_tag_ratchet() -> Result<()> {
    let repo_root = repo_root()?;
    let workflow_root = repo_root.join("crates/aura-app/src/workflows");
    let legacy_hits = rg_lines(&vec![
        "-n".into(),
        "OWNERSHIP: (view-write-legacy|view-read-for-decision|fallback-heuristic)".into(),
        repo_relative(&workflow_root),
    ])?;
    if !legacy_hits.is_empty() {
        for hit in legacy_hits {
            eprintln!("{hit}");
        }
        bail!("legacy workflow ownership tags are no longer allowed");
    }
    let deprecated_hits = rg_lines(&vec![
        "-n".into(),
        "OWNERSHIP: deprecated-legacy-bridge".into(),
        repo_relative(&workflow_root),
    ])?;
    if !deprecated_hits.is_empty() {
        for hit in deprecated_hits {
            eprintln!("{hit}");
        }
        bail!("deprecated workflow bridge tags are no longer allowed");
    }

    let sensitive_hits = rg_lines(&vec![
        "-n".into(),
        "with_(chat|homes|contacts|recovery|neighborhood)_state|views_mut\\(|chat_snapshot\\(|contacts_snapshot\\(|recovery_snapshot\\(|core\\.snapshot\\(|snapshot\\(\\)".into(),
        repo_relative(&workflow_root),
    ])?;
    let final_re = Regex::new(
        r"OWNERSHIP: (observed|observed-display-update|authoritative-source|first-run-default|fact-backed|test-only-helper)",
    )?;
    let mut violations = Vec::new();
    for hit in sensitive_hits {
        let (file, line_number) = parse_hit_path_line(&hit)?;
        if file.ends_with("crates/aura-app/src/workflows/mod.rs") {
            continue;
        }
        let lines = read_lines(repo_root.join(file))?;
        if is_after_cfg_test(&lines, line_number) {
            continue;
        }
        let start = line_number.saturating_sub(60).max(1);
        let has_tag = lines
            .iter()
            .enumerate()
            .skip(start - 1)
            .take(line_number - start + 1)
            .any(|(_, line)| final_re.is_match(line));
        if !has_tag {
            violations.push(hit);
        }
    }
    if !violations.is_empty() {
        eprintln!(
            "projection-sensitive workflow sites must carry a final ownership classification:"
        );
        for violation in violations {
            eprintln!("{violation}");
        }
        bail!("workflow ownership tags are incomplete");
    }
    let classified_count = rg_lines(&vec![
        "-n".into(),
        final_re.as_str().into(),
        repo_relative(&workflow_root),
    ])?
    .len();
    println!("workflow ownership audit clean ({classified_count} final ownership tags)");
    Ok(())
}

pub fn run_ignored_test_count_ratchet() -> Result<()> {
    const MAX_IGNORED_TEST_ANNOTATIONS: usize = 47;

    let repo_root = repo_root()?;
    let ignore_hits = rg_lines(&vec![
        "-n".into(),
        r#"^\s*#\[ignore(?:\s*=\s*"[^"]+")?\]"#.into(),
        repo_relative(repo_root.join("crates")),
        "-g".into(),
        "*.rs".into(),
    ])?;
    let current_count = ignore_hits.len();

    if current_count > MAX_IGNORED_TEST_ANNOTATIONS {
        eprintln!(
            "ignored-test-count-ratchet: current count {} exceeds baseline {}",
            current_count, MAX_IGNORED_TEST_ANNOTATIONS
        );
        for hit in ignore_hits {
            eprintln!("{hit}");
        }
        bail!("ignored-test-count-ratchet: ignored test count increased");
    }

    if current_count < MAX_IGNORED_TEST_ANNOTATIONS {
        println!(
            "ignored-test-count-ratchet: clean (current {} < baseline {}; lower the baseline)",
            current_count, MAX_IGNORED_TEST_ANNOTATIONS
        );
    } else {
        println!(
            "ignored-test-count-ratchet: clean ({} ignored test annotations)",
            current_count
        );
    }

    Ok(())
}

pub fn run_protocol_device_enrollment_contract() -> Result<()> {
    let repo_root = repo_root()?;
    let targets = [
        repo_relative(repo_root.join("crates/aura-app")),
        repo_relative(repo_root.join("crates/aura-agent")),
        repo_relative(repo_root.join("crates/aura-terminal")),
        repo_relative(repo_root.join("crates/aura-testkit")),
    ];
    let mut failed = false;
    for pattern in [
        "invitee_authority_id: Option<AuthorityId>",
        "_invitee_authority_id: Option<AuthorityId>",
        r"start_device_enrollment\([^)]*, None\)",
        "invitee_authority_id: None",
        r"unwrap_or\(authority_id\)",
    ] {
        let mut args = vec!["-n".into(), pattern.into()];
        args.extend(targets.iter().cloned());
        let hits = rg_lines(&args)?;
        if !hits.is_empty() {
            eprintln!("device-enrollment-authority-contract: forbidden pattern matched: {pattern}");
            for hit in hits {
                eprintln!("{hit}");
            }
            failed = true;
        }
    }
    if failed {
        bail!("device-enrollment-authority-contract: violations detected");
    }
    println!("device-enrollment-authority-contract: clean");
    Ok(())
}

pub fn run_runtime_boundary_allowlist(args: &[String]) -> Result<()> {
    let mode = args.first().map(String::as_str).context(
        "runtime-boundary-allowlist: usage: check runtime-boundary-allowlist <instrumentation|concurrency>",
    )?;
    let repo_root = repo_root()?;
    let (label, matches, skip_pattern, approved_sites, fail_msg) = match mode {
        "instrumentation" => (
            "runtime instrumentation schema",
            rg_non_comment_lines(&vec![
                "-n".into(),
                r#"event\s*=\s*"runtime\."#.into(),
                repo_relative(repo_root.join("crates/aura-agent/src/runtime")),
                repo_relative(repo_root.join("crates/aura-agent/src/task_registry.rs")),
                "-g".into(),
                "*.rs".into(),
            ])?,
            Regex::new(r"^crates/aura-agent/src/runtime/instrumentation\.rs:")?,
            vec![
                Regex::new(r"^crates/aura-agent/src/task_registry\.rs:")?,
                Regex::new(r"^crates/aura-agent/src/runtime/services/ceremony_tracker\.rs:")?,
                Regex::new(r"^crates/aura-agent/src/runtime/services/rendezvous_manager\.rs:")?,
                Regex::new(r"^crates/aura-agent/src/runtime/services/maintenance_service\.rs:")?,
                Regex::new(r"^crates/aura-agent/src/runtime/services/sync_manager\.rs:")?,
                Regex::new(r"^crates/aura-agent/src/runtime/system\.rs:")?,
            ],
            "runtime event names must come from runtime/instrumentation.rs or be explicitly allowlisted",
        ),
        "concurrency" => (
            "async concurrency envelope",
            rg_non_comment_lines(&vec![
                "-n".into(),
                "AuraVmRuntimeMode::ThreadedReplayDeterministic|AuraVmRuntimeMode::ThreadedEnvelopeBounded|AuraVmRuntimeSelector::for_policy\\(|new_with_contracts_and_selector\\(|canonical_fallback_policy\\(".into(),
                repo_relative(repo_root.join("crates/aura-agent/src")),
                "-g".into(),
                "*.rs".into(),
            ])?,
            Regex::new(
                r"^crates/aura-agent/src/runtime/(vm_hardening|vm_host_bridge|choreo_engine)\.rs:",
            )?,
            vec![Regex::new(
                r"^crates/aura-agent/src/runtime/contracts\.rs:.*canonical_fallback_policy\(",
            )?],
            "non-admitted concurrency path bypasses vm_hardening.rs / vm_host_bridge.rs / choreo_engine.rs",
        ),
        other => bail!("runtime-boundary-allowlist: unknown mode {other}"),
    };
    let violations: Vec<_> = matches
        .into_iter()
        .filter(|hit| !skip_pattern.is_match(hit))
        .filter(|hit| !approved_sites.iter().any(|approved| approved.is_match(hit)))
        .collect();
    if !violations.is_empty() {
        for violation in violations {
            eprintln!("{violation}");
        }
        bail!("{label}: {fail_msg}");
    }
    println!("{label}: clean");
    Ok(())
}

pub fn run_runtime_shutdown_order() -> Result<()> {
    let repo_root = repo_root()?;
    let target = repo_root.join("crates/aura-agent/src/runtime/system.rs");
    if !target.exists() {
        bail!(
            "runtime-shutdown-order: missing target: {}",
            target.display()
        );
    }
    let reactive = first_match_line(&target, "self.reactive_pipeline_service.stop().await")?
        .context("runtime-shutdown-order: missing reactive pipeline shutdown step")?;
    let task_tree = first_match_line(&target, "shutdown_with_timeout(Duration::from_secs(5))")?
        .context("runtime-shutdown-order: missing runtime task tree shutdown step")?;
    let stop_services = first_match_line(&target, "self.stop_services().await")?
        .context("runtime-shutdown-order: missing stop_services step")?;
    let lifecycle = first_match_line(&target, "lifecycle_manager.shutdown(ctx).await")?
        .context("runtime-shutdown-order: missing lifecycle shutdown step")?;
    if reactive >= task_tree {
        bail!(
            "runtime-shutdown-order: reactive pipeline must shut down before task tree cancellation"
        );
    }
    if task_tree >= stop_services {
        bail!("runtime-shutdown-order: runtime task tree must cancel before service teardown");
    }
    if stop_services >= lifecycle {
        bail!("runtime-shutdown-order: services must stop before lifecycle manager shutdown");
    }
    if contains(&target, "runtime_tasks.shutdown();")? {
        bail!("runtime-shutdown-order: found legacy unbounded runtime_tasks.shutdown() call");
    }
    println!("runtime shutdown order: clean");
    Ok(())
}

pub fn run_protocol_choreo_wiring() -> Result<()> {
    let repo_root = repo_root()?;
    let doc = repo_root.join("docs/110_mpst_and_choreography.md");
    if !doc.exists() {
        bail!(
            "protocol-choreo-wiring: missing choreography audit doc: {}",
            doc.display()
        );
    }
    let mut protocols = Vec::new();
    for line in read(&doc)?.lines() {
        if !line.contains("| Spec-only |") {
            continue;
        }
        let cells: Vec<_> = line.split('|').map(str::trim).collect();
        if cells.len() > 2 && !cells[1].is_empty() {
            protocols.push(cells[1].to_string());
        }
    }
    if protocols.is_empty() {
        println!("protocol-choreo-wiring: no Spec-only protocols listed; skipping check");
        return Ok(());
    }
    let mut violations = Vec::new();
    for protocol in protocols {
        let hits = rg_lines(&vec![
            "-n".into(),
            format!(r"\b{protocol}\b"),
            repo_relative(repo_root.join("crates/aura-agent")),
            repo_relative(repo_root.join("crates/aura-app")),
            repo_relative(repo_root.join("crates/aura-terminal")),
        ])?;
        if !hits.is_empty() {
            violations.push(protocol);
        }
    }
    if !violations.is_empty() {
        for protocol in &violations {
            eprintln!(
                "protocol-choreo-wiring: protocol `{protocol}` is marked Spec-only but referenced in runtime/app/UI code"
            );
        }
        bail!("protocol-choreo-wiring: choreography wiring lint failed");
    }
    println!("protocol-choreo-wiring: passed");
    Ok(())
}

pub fn run_privacy_runtime_locality() -> Result<()> {
    let repo_root = repo_root()?;
    let selection_manager =
        repo_root.join("crates/aura-agent/src/runtime/services/selection_manager.rs");
    let registry = repo_root.join("crates/aura-agent/src/runtime/services/service_registry.rs");
    let agent_arch = repo_root.join("crates/aura-agent/ARCHITECTURE.md");
    if rg_exists(&vec![
        "-n".into(),
        r#"authoritative = ".*LocalSelectionProfile"#.into(),
        repo_relative(&selection_manager),
    ])? {
        bail!(
            "adaptive-privacy-runtime-locality: LocalSelectionProfile must remain runtime-local, not an authoritative service-surface object"
        );
    }
    if !rg_exists(&vec![
        "-n".into(),
        r#"authoritative = """#.into(),
        repo_relative(&selection_manager),
    ])? {
        bail!(
            "adaptive-privacy-runtime-locality: selection_manager service_surface must declare an empty authoritative set"
        );
    }
    if !rg_exists(&vec![
        "-n".into(),
        r#"runtime_local = ".*selection_profiles.*""#.into(),
        repo_relative(&selection_manager),
    ])? {
        bail!(
            "adaptive-privacy-runtime-locality: selection_manager service_surface must declare selection profiles as runtime-local state"
        );
    }

    let uses = rg_lines(&vec![
        "-n".into(),
        "LocalSelectionProfile".into(),
        repo_relative(repo_root.join("crates/aura-agent/src")),
        repo_relative(repo_root.join("crates/aura-app/src")),
        repo_relative(repo_root.join("crates/aura-terminal/src")),
        repo_relative(repo_root.join("crates/aura-web/src")),
        repo_relative(repo_root.join("crates/aura-harness/src")),
        "-g".into(),
        "*.rs".into(),
    ])?;
    let leaked: Vec<_> = uses
        .into_iter()
        .filter(|hit| {
            !hit.contains("crates/aura-agent/src/runtime/services/selection_manager.rs")
                && !hit.contains("crates/aura-agent/src/runtime/services/mod.rs")
                && !hit.contains("crates/aura-agent/src/lib.rs")
        })
        .collect();
    if !leaked.is_empty() {
        for hit in leaked {
            eprintln!("{hit}");
        }
        bail!(
            "adaptive-privacy-runtime-locality: LocalSelectionProfile must not escape the runtime-owned selection service surface"
        );
    }
    if !rg_exists(&vec![
        "-n".into(),
        "SelectionState".into(),
        repo_relative(&registry),
    ])? {
        bail!(
            "adaptive-privacy-runtime-locality: service_registry must store SelectionState snapshots for sanctioned runtime-local queries"
        );
    }
    let agent_arch_contents = read(&agent_arch)?;
    if !agent_arch_contents.contains(
        "Adaptive privacy runtime-owned services include `SelectionManager`, `LocalHealthObserver`, `CoverTrafficGenerator`, and `AnonymousPathManager`",
    ) {
        bail!("adaptive-privacy-runtime-locality: aura-agent ARCHITECTURE.md must document the adaptive privacy runtime-owned service set");
    }
    if !agent_arch_contents.contains("`LocalSelectionProfile` is runtime-local") {
        bail!(
            "adaptive-privacy-runtime-locality: aura-agent ARCHITECTURE.md must state that LocalSelectionProfile is runtime-local"
        );
    }
    println!("adaptive-privacy-runtime-locality: ok");
    Ok(())
}

pub fn run_privacy_legacy_sweep() -> Result<()> {
    let repo_root = repo_root()?;
    run_privacy_runtime_locality()?;
    run_privacy_onion_quarantine()?;

    let legacy_hits = rg_lines(&vec![
        "-n".into(),
        "TransportSelector|CandidateKind|ConnectionCandidate|on_candidates_changed\\(|select_establish_path(_with_probing)?\\(".into(),
        repo_relative(repo_root.join("crates/aura-rendezvous")),
        repo_relative(repo_root.join("crates/aura-protocol")),
        repo_relative(repo_root.join("crates/aura-testkit")),
    ])?;
    if !legacy_hits.is_empty() {
        for hit in legacy_hits {
            eprintln!("{hit}");
        }
        bail!(
            "adaptive-privacy-phase5-legacy-sweep: legacy non-runtime selection ownership paths must be removed"
        );
    }

    if rg_exists(&vec![
        "-n".into(),
        "upcoming runtime/app integration|upcoming.*land".into(),
        repo_relative(repo_root.join("crates/aura-agent/src/runtime/services/mod.rs")),
    ])? {
        bail!(
            "adaptive-privacy-phase5-legacy-sweep: transitional transparent-envelope scaffolding comments must be removed from runtime service exports"
        );
    }

    let setup_hits = rg_lines(&vec![
        "-n".into(),
        "TransparentAnonymousSetupLayer|TransparentAnonymousSetupObject".into(),
        repo_relative(repo_root.join("crates")),
    ])?;
    let setup_violations: Vec<_> = setup_hits
        .into_iter()
        .filter(|hit| {
            !hit.starts_with("crates/aura-core/src/service.rs:")
                && !hit.starts_with("crates/aura-core/src/lib.rs:")
                && !hit.starts_with("crates/aura-agent/src/runtime/services/path_manager.rs:")
        })
        .collect();
    if !setup_violations.is_empty() {
        for hit in setup_violations {
            eprintln!("{hit}");
        }
        bail!(
            "adaptive-privacy-phase5-legacy-sweep: transparent anonymous setup objects must stay scoped to aura-core service types and the runtime path manager"
        );
    }

    let service_file = repo_root.join("crates/aura-core/src/service.rs");
    for traffic_class in [
        "HoldDeposit",
        "HoldRetrieval",
        "Cover",
        "AccountabilityReply",
    ] {
        if !contains(
            &service_file,
            &format!("TransparentMoveTrafficClass::{traffic_class}"),
        )? {
            bail!(
                "adaptive-privacy-phase5-legacy-sweep: shared transparent move envelope must carry {traffic_class} traffic"
            );
        }
    }

    let cover = repo_root.join("crates/aura-agent/src/runtime/services/cover_traffic_generator.rs");
    if !contains(&cover, "MoveEnvelope::opaque")? {
        bail!(
            "adaptive-privacy-phase5-legacy-sweep: cover traffic planning must stay on the shared Move envelope substrate"
        );
    }
    if contains(&cover, "TransportEnvelope")? {
        bail!(
            "adaptive-privacy-phase5-legacy-sweep: cover traffic planning must not bypass the shared Move envelope substrate"
        );
    }

    let runtime_hits = rg_lines(&vec![
        "-n".into(),
        "TransportHint::|tcp_direct\\(|quic_reflexive|fallback_direct_route".into(),
        repo_relative(repo_root.join("crates/aura-agent/src/runtime/services/move_manager.rs")),
        repo_relative(
            repo_root.join("crates/aura-agent/src/runtime/services/selection_manager.rs"),
        ),
        repo_relative(
            repo_root.join("crates/aura-agent/src/runtime/services/cover_traffic_generator.rs"),
        ),
    ])?;
    if !runtime_hits.is_empty() {
        for hit in runtime_hits {
            eprintln!("{hit}");
        }
        bail!(
            "adaptive-privacy-phase5-legacy-sweep: runtime adaptive-privacy services must not reintroduce implicit route setup or direct transport fallback"
        );
    }

    for legacy_pattern in [
        "mailbox polling",
        "identity-addressed retrieval",
        "direct return channels",
    ] {
        let hits = rg_lines(&vec![
            "-n".into(),
            legacy_pattern.into(),
            repo_relative(repo_root.join("crates/aura-agent/src/runtime/services")),
            repo_relative(repo_root.join("crates/aura-core/src/service.rs")),
        ])?;
        if !hits.is_empty() {
            bail!(
                "adaptive-privacy-phase5-legacy-sweep: runtime adaptive-privacy services still reference legacy transport assumption: {legacy_pattern}"
            );
        }
    }

    println!("adaptive-privacy-phase5-legacy-sweep: ok");
    Ok(())
}

pub fn run_harness_typed_semantic_errors() -> Result<()> {
    let repo_root = repo_root()?;
    let violations = rg_non_comment_lines(&vec![
        "-n".into(),
        "OpError::Failed\\(format!|TerminalError::Operation\\(format!|AuraError::agent\\(format!"
            .into(),
        repo_relative(repo_root.join("crates/aura-app/src/workflows")),
        repo_relative(repo_root.join("crates/aura-terminal/src/tui/effects/operational")),
        repo_relative(repo_root.join("crates/aura-terminal/src/tui/context")),
        repo_relative(repo_root.join("crates/aura-web/src")),
        repo_relative(repo_root.join("crates/aura-harness/src")),
        "-g".into(),
        "*.rs".into(),
    ])?;
    if !violations.is_empty() {
        for hit in violations {
            eprintln!("{hit}");
        }
        bail!(
            "harness-typed-semantic-errors: parity-critical shared semantic paths still rely on stringly error construction"
        );
    }
    println!("harness typed semantic errors: clean");
    Ok(())
}

pub fn run_harness_typed_json_boundary() -> Result<()> {
    let repo_root = repo_root()?;
    let violations = rg_non_comment_lines(&vec![
        "-n".into(),
        "serde_json::Value|serde_json::from_value\\(".into(),
        repo_relative(repo_root.join("crates/aura-harness/src/executor.rs")),
        repo_relative(repo_root.join("crates/aura-harness/src/replay.rs")),
        repo_relative(repo_root.join("crates/aura-harness/src/backend/mod.rs")),
        repo_relative(repo_root.join("crates/aura-harness/src/backend/local_pty.rs")),
        repo_relative(repo_root.join("crates/aura-terminal/src/tui/harness_state")),
        repo_relative(repo_root.join("crates/aura-web/src/harness_bridge.rs")),
        "-g".into(),
        "*.rs".into(),
    ])?;
    if !violations.is_empty() {
        for hit in violations {
            eprintln!("{hit}");
        }
        bail!(
            "harness-typed-json-boundary: shared semantic core still relies on raw serde_json::Value plumbing"
        );
    }
    println!("harness typed json boundary: clean");
    Ok(())
}

pub fn run_harness_authoritative_fact_boundary() -> Result<()> {
    let repo_root = repo_root()?;
    let violations = rg_non_comment_lines(&vec![
        "-n".into(),
        "AuthoritativeSemanticFact::(OperationStatus|PendingHomeInvitationReady|ContactLinkReady|ChannelMembershipReady|RecipientPeersResolved|PeerChannelReady|MessageDeliveryReady)".into(),
        repo_relative(repo_root.join("crates/aura-terminal/src")),
        repo_relative(repo_root.join("crates/aura-web/src")),
        repo_relative(repo_root.join("crates/aura-harness/src")),
        "-g".into(),
        "*.rs".into(),
    ])?;
    if !violations.is_empty() {
        for hit in violations {
            eprintln!("{hit}");
        }
        bail!(
            "harness-authoritative-fact-boundary: frontend-facing modules are handling authoritative semantic facts outside approved boundaries"
        );
    }
    println!("harness authoritative fact boundary: clean");
    Ok(())
}

pub fn run_observed_layer_boundaries() -> Result<()> {
    let repo_root = repo_root()?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "hxrts-aura-app".into(),
            "--test".into(),
            "compile_fail".into(),
            "--".into(),
            "--nocapture".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "run".into(),
            "-q".into(),
            "-p".into(),
            "hxrts-aura-macros".into(),
            "--bin".into(),
            "ownership_lints".into(),
            "--".into(),
            "harness-readiness-ownership".into(),
            repo_relative(repo_root.join("crates/aura-agent/src/reactive/app_signal_views.rs")),
            repo_relative(
                repo_root.join("crates/aura-agent/src/reactive/app_signal_projection.rs"),
            ),
            repo_relative(repo_root.join("crates/aura-terminal/src")),
            repo_relative(repo_root.join("crates/aura-web/src")),
            repo_relative(repo_root.join("crates/aura-harness/src")),
        ],
    )?;
    let ui_violations = rg_lines(&vec![
        "-n".into(),
        "publish_authoritative_|replace_authoritative_semantic_facts_of_kind\\(".into(),
        repo_relative(repo_root.join("crates/aura-ui/src")),
        "-g".into(),
        "*.rs".into(),
    ])?;
    if !ui_violations.is_empty() {
        for hit in ui_violations {
            eprintln!("{hit}");
        }
        bail!(
            "observed-layer-authorship: observed UI modules may not author authoritative semantic truth"
        );
    }
    println!("observed layer authorship: clean");
    Ok(())
}

pub fn run_harness_actor_vs_move_ownership() -> Result<()> {
    let repo_root = repo_root()?;
    let docs = [
        repo_root.join("docs/804_testing_guide.md"),
        repo_root.join("crates/aura-app/ARCHITECTURE.md"),
        repo_root.join("crates/aura-terminal/ARCHITECTURE.md"),
        repo_root.join("crates/aura-web/ARCHITECTURE.md"),
        repo_root.join("crates/aura-harness/ARCHITECTURE.md"),
        repo_root.join("crates/aura-agent/ARCHITECTURE.md"),
    ];
    for doc in &docs {
        if !doc.exists() {
            bail!(
                "harness-actor-vs-move-ownership: missing required ownership-model doc: {}",
                doc.display()
            );
        }
    }
    let testing = read(&docs[0])?;
    for needle in [
        "Shared Semantic Ownership Model",
        "`Pure`",
        "`MoveOwned`",
        "`ActorOwned`",
        "`Observed`",
    ] {
        if !testing.contains(needle) {
            bail!("harness-actor-vs-move-ownership: testing guide missing `{needle}`");
        }
    }
    for (path, required) in [
        (
            &docs[1],
            vec![
                "## Ownership Model",
                "primarily a `Pure` plus `MoveOwned`",
                "not `ActorOwned`",
            ],
        ),
        (
            &docs[2],
            vec!["## Ownership Model", "`Observed`", "must not own"],
        ),
        (
            &docs[3],
            vec!["## Ownership Model", "`Observed`", "must not own"],
        ),
        (
            &docs[4],
            vec![
                "## Ownership Model",
                "`Observed`",
                "must not author semantic lifecycle truth",
            ],
        ),
        (
            &docs[5],
            vec![
                "actor services solve long-lived runtime supervision and lifecycle",
                "move semantics solve session and endpoint ownership transfer",
            ],
        ),
    ] {
        let contents = read(path)?;
        for needle in required {
            if !contents.contains(needle) {
                bail!(
                    "harness-actor-vs-move-ownership: {} missing required text `{needle}`",
                    path.display()
                );
            }
        }
    }
    run_ok(
        "cargo",
        &[
            "run".into(),
            "-q".into(),
            "-p".into(),
            "hxrts-aura-macros".into(),
            "--bin".into(),
            "ownership_lints".into(),
            "--".into(),
            "harness-readiness-ownership".into(),
            repo_relative(repo_root.join("crates/aura-agent/src/reactive/app_signal_views.rs")),
            repo_relative(
                repo_root.join("crates/aura-agent/src/reactive/app_signal_projection.rs"),
            ),
            repo_relative(repo_root.join("crates/aura-terminal/src")),
            repo_relative(repo_root.join("crates/aura-web/src")),
            repo_relative(repo_root.join("crates/aura-harness/src")),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "run".into(),
            "-q".into(),
            "-p".into(),
            "hxrts-aura-macros".into(),
            "--bin".into(),
            "ownership_lints".into(),
            "--".into(),
            "harness-move-ownership-boundary".into(),
            repo_relative(repo_root.join("crates/aura-app")),
            repo_relative(repo_root.join("crates/aura-terminal")),
            repo_relative(repo_root.join("crates/aura-web")),
            repo_relative(repo_root.join("crates/aura-harness")),
        ],
    )?;
    println!("harness actor-vs-move ownership: clean");
    Ok(())
}

pub fn run_browser_restart_boundary() -> Result<()> {
    let repo_root = repo_root()?;
    let driver = repo_root.join("crates/aura-harness/playwright-driver/src/playwright_driver.ts");
    if !driver.exists() {
        bail!(
            "browser semantic restart boundary: missing driver source: {}",
            driver.display()
        );
    }
    let contents = read(&driver)?;
    if contents.contains("pending_semantic_payload")
        || contents.contains("pending_runtime_stage_payload")
    {
        bail!(
            "browser semantic restart boundary: legacy restart seed payload plumbing is still present in the Playwright driver"
        );
    }
    let submit_body = extract_ts_function(&contents, "async function submitSemanticCommand(params)")
        .context("browser semantic restart boundary: could not locate submitSemanticCommand in Playwright driver")?;
    let runtime_body = extract_ts_function(&contents, "async function stageRuntimeIdentity(params)")
        .context("browser semantic restart boundary: could not locate stageRuntimeIdentity in Playwright driver")?;
    if submit_body.contains("restartPageSession(") {
        bail!(
            "browser semantic restart boundary: submitSemanticCommand must fail closed instead of replaying through restartPageSession"
        );
    }
    if !submit_body.contains("submit_semantic_command_enqueue_failed_closed") {
        bail!(
            "browser semantic restart boundary: submitSemanticCommand no longer exposes an explicit fail-closed semantic enqueue path"
        );
    }
    if runtime_body.contains("restartPageSession(") {
        bail!(
            "browser semantic restart boundary: stageRuntimeIdentity must fail closed instead of replaying through restartPageSession"
        );
    }
    if !runtime_body.contains("stage_runtime_identity_enqueue_failed_closed") {
        bail!(
            "browser semantic restart boundary: stageRuntimeIdentity no longer exposes an explicit fail-closed runtime-stage enqueue path"
        );
    }
    println!("browser semantic restart boundary: clean");
    Ok(())
}

pub fn run_service_surface_declarations() -> Result<()> {
    let repo_root = repo_root()?;
    for file in [
        repo_root.join("crates/aura-rendezvous/src/service.rs"),
        repo_root.join("crates/aura-agent/src/runtime/services/move_manager.rs"),
    ] {
        if !rg_exists(&vec![
            "-n".into(),
            r"#\[aura_macros::service_surface\(".into(),
            repo_relative(&file),
        ])? {
            bail!(
                "service-surface-declarations: missing #[aura_macros::service_surface(...)] declaration in {}",
                file.display()
            );
        }
    }
    for file in [
        repo_root.join("crates/aura-rendezvous/src/descriptor.rs"),
        repo_root.join("crates/aura-rendezvous/src/service.rs"),
        repo_root.join("crates/aura-agent/src/runtime/services/move_manager.rs"),
    ] {
        let hits = rg_lines(&vec![
            "-n".into(),
            r"\b(home|neighborhood|guardian|friend|fof)\b".into(),
            repo_relative(&file),
        ])?;
        if !hits.is_empty() {
            for hit in hits {
                eprintln!("{hit}");
            }
            bail!(
                "service-surface-declarations: social-role-specific vocabulary is forbidden in Establish/Move surface files: {}",
                file.display()
            );
        }
    }
    let exceptions = rg_lines(&vec![
        "-n".into(),
        "service_surface_(exception|allowlist|compat_alias)".into(),
        repo_relative(repo_root.join("crates")),
        repo_relative(repo_root.join("scripts")),
        repo_relative(repo_root.join("work")),
        repo_relative(repo_root.join("docs")),
    ])?;
    for hit in exceptions {
        let (file, line_number) = parse_hit_path_line(&hit)?;
        let lines = read_lines(repo_root.join(file))?;
        let context = lines
            .iter()
            .skip(line_number - 1)
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        if !context.contains("owner =") || !context.contains("remove_by =") {
            bail!(
                "service-surface-declarations: service-surface exception in {file}:{line_number} must declare owner = ... and remove_by = ..."
            );
        }
    }
    println!("service-surface declaration policy passed");
    Ok(())
}

pub fn run_harness_ownership_policy() -> Result<()> {
    run_ownership_category_declarations()?;
    run_privacy_runtime_locality()?;
    run_privacy_legacy_sweep()?;
    run_harness_actor_vs_move_ownership()?;
    let repo_root = repo_root()?;
    for args in [
        vec![
            "run".into(),
            "-q".into(),
            "-p".into(),
            "hxrts-aura-macros".into(),
            "--bin".into(),
            "ownership_lints".into(),
            "--".into(),
            "harness-readiness-ownership".into(),
            repo_relative(repo_root.join("crates/aura-agent/src/reactive/app_signal_views.rs")),
            repo_relative(repo_root.join("crates/aura-terminal/src")),
            repo_relative(repo_root.join("crates/aura-web/src")),
            repo_relative(repo_root.join("crates/aura-harness/src")),
        ],
        vec![
            "run".into(),
            "-q".into(),
            "-p".into(),
            "hxrts-aura-macros".into(),
            "--bin".into(),
            "ownership_lints".into(),
            "--".into(),
            "terminal-shell-explicit-exit-intent".into(),
            repo_relative(repo_root.join("crates/aura-terminal/src")),
        ],
        vec![
            "run".into(),
            "-q".into(),
            "-p".into(),
            "hxrts-aura-macros".into(),
            "--bin".into(),
            "ownership_lints".into(),
            "--".into(),
            "optional-owner-boundary".into(),
            repo_relative(repo_root.join("crates/aura-app/src")),
            repo_relative(repo_root.join("crates/aura-agent/src/runtime_bridge")),
            repo_relative(repo_root.join("crates/aura-ui/src")),
            repo_relative(repo_root.join("crates/aura-web/src")),
            repo_relative(repo_root.join("crates/aura-testkit/src")),
        ],
        vec![
            "run".into(),
            "-q".into(),
            "-p".into(),
            "hxrts-aura-macros".into(),
            "--bin".into(),
            "ownership_lints".into(),
            "--".into(),
            "harness-move-ownership-boundary".into(),
            repo_relative(repo_root.join("crates/aura-app")),
            repo_relative(repo_root.join("crates/aura-terminal")),
            repo_relative(repo_root.join("crates/aura-web")),
            repo_relative(repo_root.join("crates/aura-harness")),
        ],
    ] {
        run_ok("cargo", &args)?;
    }
    run_harness_typed_semantic_errors()?;
    run_ok(
        "./scripts/toolkit-shell.sh",
        &[
            "toolkit-dylint".into(),
            "--repo-root".into(),
            ".".into(),
            "--lint-path".into(),
            "./toolkit/lints/harness_boundaries".into(),
            "--all".into(),
            "--".into(),
            "--all-targets".into(),
        ],
    )?;
    run_harness_authoritative_fact_boundary()?;
    run_ok("just", &["ci-frontend-portability".into()])?;
    println!("harness ownership policy: clean");
    Ok(())
}

pub fn run_ownership_policy() -> Result<()> {
    let repo_root = repo_root()?;
    run_ownership_category_declarations()?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "hxrts-aura-macros".into(),
            "--test".into(),
            "service_surface_compile_fail".into(),
            "--".into(),
            "--nocapture".into(),
        ],
    )?;
    run_service_surface_declarations()?;
    run_service_registry_ownership()?;
    for mode in ["semantic-owner", "actor-owned", "capability-boundary"] {
        run_ownership_annotation_ratchet(&[mode.to_string()])?;
    }
    run_ok(
        "cargo",
        &[
            "run".into(),
            "-q".into(),
            "-p".into(),
            "hxrts-aura-macros".into(),
            "--bin".into(),
            "ownership_lints".into(),
            "--".into(),
            "actor-owned-task-spawn".into(),
            repo_relative(repo_root.join("crates/aura-agent/src")),
            repo_relative(repo_root.join("crates/aura-app/src")),
            repo_relative(repo_root.join("crates/aura-core/src")),
            repo_relative(repo_root.join("crates/aura-effects/src")),
            repo_relative(repo_root.join("crates/aura-harness/src")),
            repo_relative(repo_root.join("crates/aura-terminal/src")),
            repo_relative(repo_root.join("crates/aura-ui/src")),
            repo_relative(repo_root.join("crates/aura-web/src")),
        ],
    )?;
    run_runtime_boundary_allowlist(&["concurrency".to_string()])?;
    run_runtime_shutdown_order()?;
    run_runtime_boundary_allowlist(&["instrumentation".to_string()])?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "hxrts-aura-core".into(),
            "--test".into(),
            "compile_fail".into(),
            "--".into(),
            "--nocapture".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "hxrts-aura-app".into(),
            "--test".into(),
            "compile_fail".into(),
            "--".into(),
            "--nocapture".into(),
        ],
    )?;
    run_runtime_error_boundary()?;
    run_protocol_device_enrollment_contract()?;
    run_runtime_typed_lifecycle_bridge()?;
    run_ownership_workflow_tag_ratchet()?;
    run_observed_layer_boundaries()?;
    run_remote_ingress_boundary()?;
    run_canonical_remote_apply_boundary()?;
    run_journal_authorization_wiring_boundary()?;
    run_raw_transport_send_boundary()?;
    run_invitation_legacy_fallback_boundary()?;
    run_biscuit_verifier_boundary()?;
    run_signed_transcript_boundary()?;
    run_trusted_key_resolution_boundary()?;
    run_security_bypass_symbols()?;
    run_secret_field_wrappers()?;
    run_security_bug_class_regressions()?;
    run_sync_biscuit_boundary()?;
    run_guard_operation_boundary()?;
    run_flow_budget_fail_closed_boundary()?;
    run_credential_transport_boundary()?;
    run_runtime_service_io_boundary()?;
    run_security_exception_metadata()?;
    run_security_boolean_escape_hatch_boundary()?;
    run_harness_ownership_policy()?;
    run_browser_restart_boundary()?;
    run_testing_exception_boundary()?;
    println!("ownership policy: clean");
    Ok(())
}

pub fn run_security_boundary_policy() -> Result<()> {
    run_security_boundary_policy_self_checks()?;
    run_remote_ingress_boundary()?;
    run_canonical_remote_apply_boundary()?;
    run_journal_authorization_wiring_boundary()?;
    run_biscuit_verifier_boundary()?;
    run_signed_transcript_boundary()?;
    run_trusted_key_resolution_boundary()?;
    run_raw_transport_send_boundary()?;
    run_invitation_legacy_fallback_boundary()?;
    run_security_bypass_symbols()?;
    run_secret_field_wrappers()?;
    run_security_bug_class_regressions()?;
    run_sync_biscuit_boundary()?;
    run_guard_operation_boundary()?;
    run_flow_budget_fail_closed_boundary()?;
    run_credential_transport_boundary()?;
    run_runtime_service_io_boundary()?;
    run_security_exception_metadata()?;
    run_security_boolean_escape_hatch_boundary()?;
    println!("security boundary policy: clean");
    Ok(())
}

fn run_security_boundary_policy_self_checks() -> Result<()> {
    let samples = [
        (
            "sync hard-coded Biscuit root",
            is_sync_biscuit_boundary_violation("let dev_key_hex = \"0102030405060708090a0b0c0d0e0f\";", ""),
        ),
        (
            "sync dummy ResourceScope authority",
            is_sync_biscuit_boundary_violation(
                "AuthorityId::new_from_entropy([1u8; 32])",
                "ResourceScope::Authority { authority_id: AuthorityId::new_from_entropy([1u8; 32]) }",
            ),
        ),
        (
            "empty guard operation bypass",
            is_guard_operation_boundary_violation(
                "if request.operation.is_empty() {",
                "if request.operation.is_empty() {\n    return true;\n}",
            ),
        ),
        (
            "infallible guard operation constructor",
            is_guard_operation_boundary_violation(
                "impl From<&str> for GuardOperationId {",
                "impl From<&str> for GuardOperationId {}",
            ),
        ),
        (
            "flow budget masked lookup error",
            is_flow_budget_fail_closed_boundary_violation(
                ".await.unwrap_or_default()",
                "get_flow_budget(ctx, peer).await.unwrap_or_default()",
            ),
        ),
        (
            "implicit max flow budget headroom",
            is_flow_budget_fail_closed_boundary_violation(
                "FlowCost::new(u32::MAX)",
                "FlowCost::new(u32::MAX)",
            ),
        ),
        (
            "credential in URL query",
            is_credential_transport_boundary_violation(
                "let retrieval_token_url = format!(\"/path?token={retrieval_token}\");",
            ),
        ),
        (
            "direct credential comparison",
            is_credential_transport_boundary_violation("if auth_token == expected_auth_token {"),
        ),
        (
            "unbounded runtime accept spawn",
            is_runtime_service_io_boundary_violation(
                "listener.accept().await",
                "spawn_named(\"bootstrap_broker_conn\", async move {})",
            ),
        ),
        (
            "runtime read without deadline",
            is_runtime_service_io_boundary_violation(
                "stream.read(&mut buf).await",
                "let n = stream.read(&mut buf).await?;",
            ),
        ),
    ];

    let failures: Vec<_> = samples
        .into_iter()
        .filter_map(|(name, detected)| (!detected).then_some(name))
        .collect();
    if !failures.is_empty() {
        bail!(
            "security-boundary-policy self-checks failed to detect old shapes: {}",
            failures.join(", ")
        );
    }
    Ok(())
}

pub fn run_security_bug_class_regressions() -> Result<()> {
    let repo_root = repo_root()?;
    let crates_dir = repo_root.join("crates");
    let mut violations = Vec::new();

    for path in rust_files_under(&crates_dir) {
        let rel = security_rel_path(&repo_root, &path);
        if security_bug_class_skip_file(&rel) {
            continue;
        }
        let lines = read_lines(&path)?;
        for (idx, line) in lines.iter().enumerate() {
            if line_is_test_scoped(&lines, idx) {
                continue;
            }
            let trimmed = line.trim_start();
            if trimmed.is_empty() || trimmed.starts_with("//") {
                continue;
            }
            let context = lines[idx.saturating_sub(12)..lines.len().min(idx + 13)].join("\n");

            if is_plain_biscuit_authorize(line, &context)
                && !known_security_bug_class_violation(&rel, line, "biscuit-authorize-limits")
            {
                violations.push(format!(
                    "{rel}:{} production Biscuit evaluation must use authorize_with_limits(...)",
                    idx + 1
                ));
            }

            if is_independent_storage_biscuit_allow(line)
                && !known_security_bug_class_violation(&rel, line, "storage-biscuit-policy-shape")
            {
                violations.push(format!(
                    "{rel}:{} storage Biscuit allow policy must conjunct authority/account, capability, operation, and resource",
                    idx + 1
                ));
            }

            if starts_secure_generate_key_signature(trimmed) {
                let signature = signature_window(&lines, idx);
                if returns_raw_secret_bytes(&signature)
                    && !known_security_bug_class_violation(
                        &rel,
                        &signature,
                        "secure-generate-key-return",
                    )
                {
                    violations.push(format!(
                        "{rel}:{} secure_generate_key must return public material or an opaque handle, not Vec<u8> secret material",
                        idx + 1
                    ));
                }
            }

            if returns_generated_secret_material(line, &context)
                && !known_security_bug_class_violation(&rel, line, "secure-generate-key-return")
            {
                violations.push(format!(
                    "{rel}:{} generated secure key material is returned to callers",
                    idx + 1
                ));
            }

            if is_peer_bloom_deserialize(line, &context)
                && !known_security_bug_class_violation(&rel, line, "wire-deserialize-validation")
            {
                violations.push(format!(
                    "{rel}:{} peer BloomFilter deserialization must immediately validate wire invariants",
                    idx + 1
                ));
            }

            if is_frame_canonicality_smell(line, &context)
                && !known_security_bug_class_violation(&rel, line, "frame-canonicality")
            {
                violations.push(format!(
                    "{rel}:{} frame payload length must be private/derived or serialization must prove it matches payload.len()",
                    idx + 1
                ));
            }

            if is_harness_ingress_env_smell(line, &context, &rel)
                && !known_security_bug_class_violation(&rel, line, "harness-ingress-gating")
            {
                violations.push(format!(
                    "{rel}:{} harness ingress/export must be gated by typed harness mode/capability, not env/path presence alone",
                    idx + 1
                ));
            }

            if is_predictable_freshness_id(line, &rel)
                && !known_security_bug_class_violation(&rel, line, "predictable-freshness-id")
            {
                violations.push(format!(
                    "{rel}:{} auth/session/request ids must come from RandomEffects or an owner-scoped nonce, not timestamp/epoch formatting",
                    idx + 1
                ));
            }

            if is_string_prefix_authorization(line, &context, &rel)
                && !known_security_bug_class_violation(&rel, line, "string-prefix-authorization")
            {
                violations.push(format!(
                    "{rel}:{} authorization/resource matching must use typed segment-aware matching, not starts_with/contains string checks",
                    idx + 1
                ));
            }

            if is_deterministic_context_constructor_use(line)
                && !known_security_bug_class_violation(
                    &rel,
                    line,
                    "deterministic-context-constructor",
                )
            {
                violations.push(format!(
                    "{rel}:{} deterministic context/session constructors are test/simulation-only; production workflow entry points must use fresh constructors",
                    idx + 1
                ));
            }
        }
    }

    if !violations.is_empty() {
        bail!("security-bug-class-regressions:\n{}", violations.join("\n"));
    }

    println!("security bug-class regression policy: clean");
    Ok(())
}

pub fn run_sync_biscuit_boundary() -> Result<()> {
    let repo_root = repo_root()?;
    let target = repo_root.join("crates/aura-sync/src/infrastructure");
    let mut violations = Vec::new();

    for path in rust_files_under(&target) {
        let rel = security_rel_path(&repo_root, &path);
        let lines = read_lines(&path)?;
        for (idx, line) in lines.iter().enumerate() {
            if line_is_test_scoped(&lines, idx) {
                continue;
            }
            let context = lines[idx.saturating_sub(8)..lines.len().min(idx + 9)].join("\n");
            if is_sync_biscuit_boundary_violation(line, &context) {
                violations.push(format!(
                    "{rel}:{} production sync Biscuit validation must not use hard-coded roots, literal root bytes, or dummy deterministic scopes",
                    idx + 1
                ));
            }
        }
    }

    if !violations.is_empty() {
        bail!("sync-biscuit-boundary:\n{}", violations.join("\n"));
    }

    println!("sync Biscuit boundary policy: clean");
    Ok(())
}

pub fn run_guard_operation_boundary() -> Result<()> {
    let repo_root = repo_root()?;
    let target = repo_root.join("crates/aura-guards/src/guards");
    let mut violations = Vec::new();

    for path in rust_files_under(&target) {
        let rel = security_rel_path(&repo_root, &path);
        let lines = read_lines(&path)?;
        for (idx, line) in lines.iter().enumerate() {
            if line_is_test_scoped(&lines, idx) {
                continue;
            }
            let window = lines[idx..lines.len().min(idx + 6)].join("\n");
            if is_guard_operation_boundary_violation(line, &window) {
                violations.push(format!(
                    "{rel}:{} guard operation ids must not expose empty-operation authorization bypasses or infallible raw-string construction",
                    idx + 1
                ));
            }
        }
    }

    if !violations.is_empty() {
        bail!("guard-operation-boundary:\n{}", violations.join("\n"));
    }

    println!("guard operation boundary policy: clean");
    Ok(())
}

pub fn run_flow_budget_fail_closed_boundary() -> Result<()> {
    let repo_root = repo_root()?;
    let target = repo_root.join("crates/aura-guards/src/guards");
    let mut violations = Vec::new();

    for path in rust_files_under(&target) {
        let rel = security_rel_path(&repo_root, &path);
        let lines = read_lines(&path)?;
        for (idx, line) in lines.iter().enumerate() {
            if line_is_test_scoped(&lines, idx) {
                continue;
            }
            let context = lines[idx.saturating_sub(4)..lines.len().min(idx + 5)].join("\n");
            if is_flow_budget_fail_closed_boundary_violation(line, &context) {
                violations.push(format!(
                    "{rel}:{} flow-budget lookup errors, missing state, or zero limits must not create implicit headroom",
                    idx + 1
                ));
            }
        }
    }

    if !violations.is_empty() {
        bail!(
            "flow-budget-fail-closed-boundary:\n{}",
            violations.join("\n")
        );
    }

    println!("flow budget fail-closed boundary policy: clean");
    Ok(())
}

pub fn run_credential_transport_boundary() -> Result<()> {
    let repo_root = repo_root()?;
    let target = repo_root.join("crates/aura-agent/src/runtime/services");
    let mut violations = Vec::new();

    for path in rust_files_under(&target) {
        let rel = security_rel_path(&repo_root, &path);
        let lines = read_lines(&path)?;
        for (idx, line) in lines.iter().enumerate() {
            if line_is_test_scoped(&lines, idx) {
                continue;
            }
            if is_credential_transport_boundary_violation(line) {
                violations.push(format!(
                    "{rel}:{} bearer/retrieval credentials must not use URL query transport or direct equality comparisons",
                    idx + 1
                ));
            }
        }
    }

    if !violations.is_empty() {
        bail!("credential-transport-boundary:\n{}", violations.join("\n"));
    }

    println!("credential transport boundary policy: clean");
    Ok(())
}

pub fn run_runtime_service_io_boundary() -> Result<()> {
    let repo_root = repo_root()?;
    let target = repo_root.join("crates/aura-agent/src/runtime/services");
    let mut violations = Vec::new();

    for path in rust_files_under(&target) {
        let rel = security_rel_path(&repo_root, &path);
        let lines = read_lines(&path)?;
        for (idx, line) in lines.iter().enumerate() {
            if line_is_test_scoped(&lines, idx) {
                continue;
            }
            let context = lines[idx.saturating_sub(10)..lines.len().min(idx + 12)].join("\n");
            if is_runtime_service_io_boundary_violation(line, &context) {
                violations.push(format!(
                    "{rel}:{} runtime service accept/read loops must use bounded concurrency and explicit deadlines",
                    idx + 1
                ));
            }
        }
    }

    if !violations.is_empty() {
        bail!("runtime-service-io-boundary:\n{}", violations.join("\n"));
    }

    println!("runtime service I/O boundary policy: clean");
    Ok(())
}

fn is_sync_biscuit_boundary_violation(line: &str, context: &str) -> bool {
    line.contains("dev_key_hex")
        || line.contains("0102030405060708090a0b0c0d0e0f")
        || (context.contains("ResourceScope")
            && line.contains("AuthorityId::new_from_entropy([1u8; 32])"))
        || line.contains("PublicKey::from_bytes(&[")
        || line.contains("PublicKey::from_bytes(&[0")
        || line.contains("PublicKey::from_bytes(&[1")
}

fn is_guard_operation_boundary_violation(line: &str, context: &str) -> bool {
    (line.contains("request.operation.is_empty()")
        && (context.contains("return true") || context.contains("\"allow\"")))
        || line.contains("impl From<String> for GuardOperationId")
        || line.contains("impl From<&str> for GuardOperationId")
}

fn is_flow_budget_fail_closed_boundary_violation(line: &str, context: &str) -> bool {
    (context.contains("get_flow_budget") && line.contains("unwrap_or_default()"))
        || line.contains("FlowCost::new(u32::MAX)")
        || (context.contains("limit == 0")
            && (context.contains("generous") || context.contains("unconfigured")))
}

fn is_credential_transport_boundary_violation(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    let credential_context = [
        "auth_token",
        "retrieval_token",
        "bearer",
        "secret",
        "credential",
    ]
    .iter()
    .any(|needle| lower.contains(needle));

    credential_context
        && ((line.contains("?token=") || line.contains("strip_prefix(\"token=\")"))
            || (line.contains("==")
                && !line.contains("constant_time")
                && !line.contains("diff ==")))
}

fn is_runtime_service_io_boundary_violation(line: &str, context: &str) -> bool {
    (line.contains(".accept().await")
        && context.contains("spawn_named")
        && !(context.contains("Semaphore")
            || context.contains("try_acquire")
            || context.contains("acquire_owned")))
        || (line.contains(".read(&mut") && line.contains(".await") && !context.contains("timeout("))
}

fn security_rel_path(repo_root: &Path, path: &Path) -> String {
    path.strip_prefix(repo_root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

fn security_bug_class_skip_file(rel: &str) -> bool {
    rel.contains("crates/aura-testkit/")
        || rel.contains("crates/aura-harness/")
        || rel.contains("crates/aura-simulator/")
        || rel.contains("crates/aura-quint/")
        || rel.contains("crates/aura-macros/")
        || rel.contains("crates/aura-terminal/src/demo/")
        || rel.contains("/tests/")
        || rel.contains("/benches/")
        || rel.ends_with("/tests.rs")
        || rel.ends_with("/test_support.rs")
        || rel.ends_with("/benches.rs")
}

fn is_plain_biscuit_authorize(line: &str, context: &str) -> bool {
    line.contains(".authorize()")
        && !line.contains("authorize_with_limits")
        && (context.contains("Biscuit")
            || context.contains("Authorizer")
            || context.contains("authorizer"))
}

fn is_independent_storage_biscuit_allow(line: &str) -> bool {
    line.contains("policy!(")
        && (line.contains("allow if authority_id(") || line.contains("allow if account("))
        && !(line.contains("capability(") && line.contains("resource("))
}

fn starts_secure_generate_key_signature(trimmed: &str) -> bool {
    trimmed.starts_with("async fn secure_generate_key(")
        || trimmed.starts_with("pub async fn secure_generate_key(")
}

fn returns_raw_secret_bytes(signature: &str) -> bool {
    signature.contains("Result<Option<Vec<u8>>")
        || signature.contains("Result < Option < Vec < u8 > >")
        || signature.contains("Result<Vec<u8>>")
        || signature.contains("Result < Vec < u8 >")
}

fn returns_generated_secret_material(line: &str, context: &str) -> bool {
    context.contains("secure_generate_key")
        && (line.contains("Ok(Some(material))")
            || line.contains("Ok(Some(key))")
            || line.contains("Ok(Some(key_bytes))"))
}

fn is_peer_bloom_deserialize(line: &str, context: &str) -> bool {
    (line.contains("from_slice::<BloomFilter>")
        || line.contains("serde_json::from_slice::<BloomFilter>")
        || line.contains("binary_deserialize::<BloomFilter>"))
        && !context.contains("validate_wire")
        && !context.contains(".validate()")
}

fn is_frame_canonicality_smell(line: &str, context: &str) -> bool {
    (line.contains("pub payload_length:") && context.contains("pub payload: Vec<u8>"))
        || (line.contains("frame.header.payload_length.to_be_bytes()")
            && !context.contains("payload_length != frame.payload.len()")
            && !context.contains("payload_length == frame.payload.len()"))
        || (line.contains("data.len() < expected_total_size")
            && context.contains("deserialize_frame")
            && !context.contains("data.len() != expected_total_size"))
}

fn is_harness_ingress_env_smell(line: &str, context: &str, rel: &str) -> bool {
    if !rel.starts_with("crates/aura-terminal/src/") {
        return false;
    }
    if line.contains("const COMMAND_SOCKET_ENV")
        || line.contains("const UI_STATE_FILE_ENV")
        || line.contains("const UI_STATE_SOCKET_ENV")
        || line.contains("write_snapshot_file(")
        || line.contains("read_to_end(&mut payload)")
        || line.contains("ensure_harness_command_listener().await")
    {
        return false;
    }
    let reads_tui_harness_env = line.contains("AURA_TUI_COMMAND_SOCKET")
        || line.contains("AURA_TUI_UI_STATE_FILE")
        || line.contains("AURA_TUI_UI_STATE_SOCKET");
    let binds_or_exports = line.contains("UnixListener::bind")
        || line.contains("read_to_end(&mut payload)")
        || line.contains("write_snapshot_file(")
        || line.contains("ensure_harness_command_listener().await");
    (reads_tui_harness_env || binds_or_exports)
        && !context.contains("configured_command_socket")
        && !context.contains("configured_ui_state_file")
        && !context.contains("configured_ui_state_socket")
        && !context.contains("AURA_HARNESS_MODE")
        && !context.contains("HarnessMode")
        && !context.contains("harness_mode_enabled")
        && !context.contains("harness runtime mode")
        && !context.contains("typed harness")
}

fn is_predictable_freshness_id(line: &str, rel: &str) -> bool {
    if !(rel.starts_with("crates/aura-authentication/src/")
        || rel.starts_with("crates/aura-agent/src/")
        || rel.starts_with("crates/aura-core/src/context.rs"))
    {
        return false;
    }
    (line.contains("format!(\"session_")
        || line.contains("format!(\"challenge_")
        || line.contains("format!(\"guardian_req_")
        || line.contains("derive_session_id("))
        && (line.contains("snapshot.epoch")
            || line.contains("snapshot.now_ms")
            || line.contains("authority_id")
            || line.contains("context_id")
            || line.contains("execution_mode")
            || line.contains("session_{}"))
}

fn is_string_prefix_authorization(line: &str, context: &str, rel: &str) -> bool {
    let sensitive_path = rel.starts_with("crates/aura-authorization/src/")
        || rel.starts_with("crates/aura-store/src/")
        || rel.starts_with("crates/aura-sync/src/");
    if !sensitive_path {
        return false;
    }
    if context.contains("fn is_same_or_child_path(") && line.contains("suffix.starts_with('/')") {
        return false;
    }
    let has_prefix_match = line.contains(".starts_with(");
    let has_string_substring_match = line.contains("_str.contains(")
        || line.contains(".as_str().contains(")
        || line.contains("resource.contains(")
        || line.contains("path.contains(")
        || line.contains("scope.contains(")
        || line.contains("namespace.contains(")
        || line.contains("participant.contains(")
        || line.contains("participants.contains(");
    if !(has_prefix_match || has_string_substring_match) {
        return false;
    }
    let lowered = format!("{context}\n{line}").to_ascii_lowercase();
    [
        "auth",
        "authority",
        "capability",
        "content_id",
        "namespace",
        "participant",
        "peer",
        "permission",
        "resource",
        "scope",
        "storage",
        "token",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
}

fn is_deterministic_context_constructor_use(line: &str) -> bool {
    line.contains("EffectContext::deterministic(")
        || line.contains("EffectContext::deterministic_with_default_context(")
        || line.contains("ContextSnapshot::deterministic(")
        || line.contains("HandlerContext::deterministic(")
}

fn known_security_bug_class_violation(rel: &str, line: &str, class: &str) -> bool {
    let _ = (rel, line, class);
    false
}

pub fn run_biscuit_verifier_boundary() -> Result<()> {
    let repo_root = repo_root()?;
    let crates_dir = repo_root.join("crates");
    let mut violations = Vec::new();

    for path in rust_files_under(&crates_dir) {
        let rel = repo_relative(&path);
        let lines = read_lines(&path)?;
        let is_test_path = rel.contains("/tests/") || rel.ends_with("/tests.rs");

        for (idx, line) in lines.iter().enumerate() {
            if is_test_path || line_is_test_scoped(&lines, idx) {
                continue;
            }
            let local_window = lines[idx.saturating_sub(16)..=idx].join("\n");
            if (line.contains("Biscuit::from(") || line.contains("biscuit_auth::Biscuit::from("))
                && !rel.ends_with("crates/aura-authorization/src/biscuit_evaluator.rs")
            {
                violations.push(format!(
                    "{rel}:{} parses raw Biscuit bytes outside VerifiedBiscuitToken::from_bytes",
                    idx + 1
                ));
            }
            if line.contains(".authorizer()")
                && !(rel.ends_with("crates/aura-authorization/src/biscuit_evaluator.rs")
                    && line.contains("self.token.authorizer()"))
                && !local_window.contains("VerifiedBiscuitToken")
                && !local_window.contains("verified_token")
                && !local_window.contains("verified token")
                && !line.contains("verified_token.authorizer()")
                && !line.contains("verified.authorizer()")
            {
                violations.push(format!(
                    "{rel}:{} uses a Biscuit authorizer outside VerifiedBiscuitToken",
                    idx + 1
                ));
            }
            if starts_security_critical_biscuit_api(line) {
                let signature = signature_window(&lines, idx);
                if signature.contains("Biscuit") && !signature.contains("VerifiedBiscuitToken") {
                    violations.push(format!(
                        "{rel}:{} exposes a security-critical Biscuit API without VerifiedBiscuitToken evidence",
                        idx + 1
                    ));
                }
            }
            if line.contains("NoopBiscuitAuthorizationHandler") {
                violations.push(format!(
                    "{rel}:{} reintroduces a production no-op Biscuit authorization handler",
                    idx + 1
                ));
            }
            let lowered = line.to_ascii_lowercase();
            if (lowered.contains("fn new_noop")
                || lowered.contains("fn noop")
                || lowered.contains("diagnostic fallback")
                || lowered.contains("unverified capability")
                || lowered.contains("unverified authorization"))
                && (local_window.contains("Biscuit")
                    || local_window.contains("Authorization")
                    || local_window.contains("capability"))
            {
                violations.push(format!(
                    "{rel}:{} exposes a production fallback/unverified authorization path",
                    idx + 1
                ));
            }
            if line.contains("pub fn new_mock(") || line.contains("pub(crate) fn new_mock(") {
                if rel.contains("crates/aura-testkit/") {
                    continue;
                }
                let cfg_test = lines[..idx]
                    .iter()
                    .rev()
                    .take(3)
                    .any(|prior| prior.contains("#[cfg(test)]"));
                if !cfg_test {
                    violations.push(format!(
                        "{rel}:{} exposes a production mock authorization constructor",
                        idx + 1
                    ));
                }
            }
        }
    }

    if !violations.is_empty() {
        bail!("biscuit-verifier-boundary:\n{}", violations.join("\n"));
    }

    println!("biscuit verifier boundary policy: clean");
    Ok(())
}

fn starts_security_critical_biscuit_api(line: &str) -> bool {
    let trimmed = line.trim_start();
    [
        "pub fn authorize(",
        "pub fn authorize_with_time(",
        "pub async fn authorize(",
        "pub async fn authorize_with_time(",
        "fn authorize(",
        "fn authorize_with_time(",
        "async fn authorize(",
        "async fn authorize_with_time(",
        "pub fn has_capability",
        "fn has_capability",
        "pub fn evaluate_access",
        "fn evaluate_access",
        "pub fn check_access",
        "fn check_access",
        "pub fn verify_token_authority",
        "fn verify_token_authority",
        "fn check_biscuit_authorization",
        "fn authorizer_with_token",
        "pub async fn evaluate_authority_op",
        "async fn evaluate_authority_op",
        "pub async fn evaluate_context_op",
        "async fn evaluate_context_op",
        "fn evaluate_scope",
    ]
    .iter()
    .any(|prefix| trimmed.starts_with(prefix))
}

fn signature_window(lines: &[String], start: usize) -> String {
    let mut signature = String::new();
    for line in lines.iter().skip(start).take(24) {
        signature.push_str(line);
        signature.push('\n');
        if line.contains('{') || line.trim_end().ends_with(';') {
            break;
        }
    }
    signature
}

pub fn run_signed_transcript_boundary() -> Result<()> {
    let repo_root = repo_root()?;
    let crates_dir = repo_root.join("crates");
    let mut violations = Vec::new();

    for path in rust_files_under(&crates_dir) {
        let rel = repo_relative(&path);
        if !is_security_critical_protocol_file(&rel) || is_approved_crypto_boundary(&rel) {
            continue;
        }
        let lines = read_lines(&path)?;
        let is_test_path = rel.contains("/tests/") || rel.ends_with("/tests.rs");

        for (idx, line) in lines.iter().enumerate() {
            if is_test_path || line_is_test_scoped(&lines, idx) {
                continue;
            }
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("//!")
            {
                continue;
            }
            let local_window = lines[idx.saturating_sub(12)..=idx].join("\n");
            let has_transcript_context = local_window.contains("SecurityTranscript")
                || local_window.contains("transcript_bytes")
                || local_window.contains("encode_transcript")
                || local_window.contains("sign_ed25519_transcript")
                || local_window.contains("verify_ed25519_transcript")
                || local_window.contains("verify_frost_transcript")
                || local_window.contains("verify_threshold_signing_context_transcript")
                || local_window.contains("SigningContext");

            if (line.contains(".ed25519_sign(")
                || line.contains(".ed25519_verify(")
                || line.contains(".frost_verify(")
                || line.contains("ed25519_verify(")
                || line.contains("frost_verify("))
                && !has_transcript_context
            {
                violations.push(format!(
                    "{rel}:{} calls a low-level signing/verification primitive without a typed transcript",
                    idx + 1
                ));
            }

            if line.contains(".sign_operation(") && !has_transcript_context {
                violations.push(format!(
                    "{rel}:{} signs raw operation bytes without a typed transcript",
                    idx + 1
                ));
            }

            if line.contains(".verify(")
                && line.contains("signature")
                && !has_transcript_context
                && !line.contains(".verify(evidence)")
            {
                violations.push(format!(
                    "{rel}:{} verifies a signature without typed transcript evidence",
                    idx + 1
                ));
            }

            if starts_security_critical_signature_api(line) {
                let signature = signature_window(&lines, idx);
                if (signature.contains("message: &[u8]")
                    || signature.contains("payload: &[u8]")
                    || signature.contains("bytes: &[u8]")
                    || signature.contains("operation: &[u8]"))
                    && !signature.contains("SecurityTranscript")
                    && !signature.contains("SigningContext")
                {
                    violations.push(format!(
                        "{rel}:{} exposes a signature API over raw bytes instead of a typed transcript",
                        idx + 1
                    ));
                }
            }
        }
    }

    if !violations.is_empty() {
        bail!("signed-transcript-boundary:\n{}", violations.join("\n"));
    }

    println!("signed transcript boundary policy: clean");
    Ok(())
}

fn is_security_critical_protocol_file(rel: &str) -> bool {
    [
        "crates/aura-agent/src/handlers/",
        "crates/aura-authentication/src/",
        "crates/aura-consensus/src/",
        "crates/aura-invitation/src/",
        "crates/aura-protocol/src/",
        "crates/aura-recovery/src/",
        "crates/aura-sync/src/protocols/",
        "crates/aura-sync/src/services/",
    ]
    .iter()
    .any(|prefix| rel.contains(prefix))
}

fn is_approved_crypto_boundary(rel: &str) -> bool {
    [
        "crates/aura-signature/src/transcript.rs",
        "crates/aura-core/src/crypto/",
        "crates/aura-core/src/effects/crypto.rs",
        "crates/aura-core/src/effects/threshold.rs",
        "crates/aura-agent/src/runtime/effects/crypto.rs",
        "crates/aura-consensus/src/frost.rs",
    ]
    .iter()
    .any(|allowed| rel.ends_with(allowed) || rel.contains(allowed))
}

fn starts_security_critical_signature_api(line: &str) -> bool {
    let trimmed = line.trim_start();
    [
        "pub fn sign_",
        "pub async fn sign_",
        "fn sign_",
        "async fn sign_",
        "pub fn verify_",
        "pub async fn verify_",
        "fn verify_",
        "async fn verify_",
        "pub fn validate_",
        "pub async fn validate_",
        "fn validate_",
        "async fn validate_",
    ]
    .iter()
    .any(|prefix| trimmed.starts_with(prefix))
        && trimmed.contains("signature")
}

pub fn run_trusted_key_resolution_boundary() -> Result<()> {
    let repo_root = repo_root()?;
    let crates_dir = repo_root.join("crates");
    let mut violations = Vec::new();

    for path in rust_files_under(&crates_dir) {
        let rel = repo_relative(&path);
        if !is_security_critical_protocol_file(&rel) || is_approved_crypto_boundary(&rel) {
            continue;
        }
        let lines = read_lines(&path)?;
        let is_test_path = rel.contains("/tests/") || rel.ends_with("/tests.rs");

        for (idx, line) in lines.iter().enumerate() {
            if is_test_path || line_is_test_scoped(&lines, idx) {
                continue;
            }
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("//!")
            {
                continue;
            }

            let local_window = lines[idx.saturating_sub(16)..=idx].join("\n");
            if is_signature_verification_call(line)
                && !has_trusted_key_resolution_context(&local_window)
            {
                violations.push(format!(
                    "{rel}:{} verifies a signature without nearby trusted key resolution",
                    idx + 1
                ));
            }

            if starts_signature_verification_api(line) {
                let signature = signature_window(&lines, idx);
                if signature.contains("public_key")
                    && !signature.contains("TrustedKeyResolver")
                    && !signature.contains("TrustedPublicKey")
                    && !signature.contains("key_resolver")
                {
                    violations.push(format!(
                        "{rel}:{} exposes a verification API with raw public-key input instead of trusted key resolution",
                        idx + 1
                    ));
                }
            }

            if line.contains("Deserialize") && line.contains("derive") {
                if let Some((struct_line, block)) = deserializable_item_block(&lines, idx) {
                    if item_carries_identity_and_key_material(&block)
                        && !block
                            .to_ascii_lowercase()
                            .contains("untrusted key material")
                    {
                        violations.push(format!(
                            "{rel}:{} deserializable remote item carries identity and key material without an `untrusted key material` annotation",
                            struct_line + 1
                        ));
                    }
                }
            }
        }
    }

    if !violations.is_empty() {
        bail!(
            "trusted-key-resolution-boundary:\n{}",
            violations.join("\n")
        );
    }

    println!("trusted key resolution boundary policy: clean");
    Ok(())
}

fn is_signature_verification_call(line: &str) -> bool {
    line.contains("verify_ed25519_transcript(")
        || line.contains("verify_frost_transcript(")
        || line.contains("verify_threshold_signing_context_transcript(")
        || line.contains(".ed25519_verify(")
        || line.contains(".frost_verify(")
        || line.contains("ed25519_verify(")
        || line.contains("frost_verify(")
        || line.contains("Ed25519VerifyingKey::from_bytes(")
}

fn has_trusted_key_resolution_context(window: &str) -> bool {
    window.contains("TrustedKeyResolver")
        || window.contains("TrustedPublicKey")
        || window.contains("trusted_key")
        || window.contains("key_resolver")
        || window.contains("resolve_authority_threshold_key")
        || window.contains("resolve_device_key")
        || window.contains("resolve_guardian_key")
        || window.contains("resolve_release_key")
}

fn starts_signature_verification_api(line: &str) -> bool {
    let trimmed = line.trim_start();
    [
        "pub fn verify_",
        "pub async fn verify_",
        "fn verify_",
        "async fn verify_",
        "pub fn validate_",
        "pub async fn validate_",
        "fn validate_",
        "async fn validate_",
    ]
    .iter()
    .any(|prefix| trimmed.starts_with(prefix))
        && trimmed.contains("signature")
}

fn deserializable_item_block(lines: &[String], derive_idx: usize) -> Option<(usize, String)> {
    let mut item_start = None;
    for idx in derive_idx..lines.len().min(derive_idx + 8) {
        let trimmed = lines[idx].trim_start();
        if trimmed.starts_with("pub struct ")
            || trimmed.starts_with("struct ")
            || trimmed.starts_with("pub enum ")
            || trimmed.starts_with("enum ")
        {
            item_start = Some(idx);
            break;
        }
    }
    let item_start = item_start?;
    let mut block = lines[derive_idx..=item_start].join("\n");
    let mut brace_depth = 0i32;
    let mut saw_open = false;
    for line in lines.iter().skip(item_start).take(80) {
        block.push('\n');
        block.push_str(line);
        for ch in line.chars() {
            match ch {
                '{' => {
                    saw_open = true;
                    brace_depth += 1;
                }
                '}' => brace_depth -= 1,
                _ => {}
            }
        }
        if saw_open && brace_depth <= 0 {
            break;
        }
    }
    Some((item_start, block))
}

fn item_carries_identity_and_key_material(block: &str) -> bool {
    let lower = block.to_ascii_lowercase();
    let has_key = [
        "public_key:",
        "verifying_key:",
        "group_public_key:",
        "recovery_public_key:",
        "route_layer_public_key:",
        "public_key_package:",
        "new_group_public_key:",
        "device_public_key:",
        "new_public_key:",
        "ephemeral_public_key:",
    ]
    .iter()
    .any(|needle| lower.contains(needle));

    let has_identity = [
        "authority_id",
        "guardian_id",
        "device_id",
        "account_id",
        "signer",
        "issuer",
        "participant",
        "from_authority",
        "to_authority",
        "proposer",
        "responder",
        "peer_id",
    ]
    .iter()
    .any(|needle| lower.contains(needle));

    has_key && has_identity
}

fn rust_workspace_manifests(repo_root: &Path) -> Result<Vec<std::path::PathBuf>> {
    let mut manifests = vec![repo_root.join("Cargo.toml")];
    let crates_dir = repo_root.join("crates");
    for entry in crates_dir.read_dir().context("reading crates/")? {
        let entry = entry?;
        let manifest = entry.path().join("Cargo.toml");
        if manifest.exists() {
            manifests.push(manifest);
        }
    }
    Ok(manifests)
}

fn default_feature_list(manifest: &str) -> Option<Vec<String>> {
    let mut in_features = false;
    let mut default_expr = String::new();
    let mut collecting = false;

    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if collecting {
                break;
            }
            in_features = trimmed == "[features]";
            continue;
        }
        if !in_features {
            continue;
        }

        if collecting {
            default_expr.push_str(trimmed);
            if trimmed.contains(']') {
                break;
            }
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("default") {
            let rest = rest.trim_start();
            if let Some(expr) = rest.strip_prefix('=') {
                default_expr.push_str(expr.trim());
                collecting = !expr.contains(']');
                if !collecting {
                    break;
                }
            }
        }
    }

    if default_expr.is_empty() {
        return None;
    }

    Some(
        default_expr
            .split(['[', ']', ','])
            .filter_map(|part| {
                let feature = part.trim().trim_matches('"');
                (!feature.is_empty()).then(|| feature.to_string())
            })
            .collect(),
    )
}

pub fn run_remote_ingress_boundary() -> Result<()> {
    let repo_root = repo_root()?;
    let crates_dir = repo_root.join("crates");
    let mut violations = Vec::new();

    for path in rust_files_under(&crates_dir) {
        let rel = repo_relative(&path);
        let lines = read_lines(&path)?;
        let is_test_path = rel.contains("/tests/") || rel.ends_with("/tests.rs");

        for (idx, line) in lines.iter().enumerate() {
            if line.contains("IngressVerificationEvidence::complete")
                && !is_test_path
                && !line_is_test_scoped(&lines, idx)
            {
                violations.push(format!(
                    "{rel}:{} uses IngressVerificationEvidence::complete outside test scope",
                    idx + 1
                ));
            }
        }

        for required in [
            "merge_remote_ops",
            "merge_batch",
            "receive_facts",
            "handle_sync_request",
            "apply_sync_response",
            "verify_receipt",
            "verify_receipt_chain",
            "verify_response",
        ] {
            require_verified_ingress_parameter(&rel, &lines, required, &mut violations);
        }
    }

    if !violations.is_empty() {
        bail!("remote-ingress-boundary:\n{}", violations.join("\n"));
    }

    println!("remote ingress boundary policy: clean");
    Ok(())
}

pub fn run_security_bypass_symbols() -> Result<()> {
    let repo_root = repo_root()?;
    let crates_dir = repo_root.join("crates");
    let mut violations = Vec::new();
    let symbol_re =
        Regex::new(r"\b(Noop|Mock)[A-Za-z0-9_]*|\bnew_mock\b").expect("valid symbol regex");

    for manifest in rust_workspace_manifests(&repo_root)? {
        let rel = repo_relative(&manifest);
        let contents = read(&manifest)?;
        if let Some(default_features) = default_feature_list(&contents) {
            for feature in default_features {
                let feature_lower = feature.to_ascii_lowercase();
                if ["demo", "mock", "test", "harness", "simulation", "insecure"]
                    .iter()
                    .any(|forbidden| feature_lower.contains(forbidden))
                {
                    violations.push(format!(
                        "{rel}: default feature `{feature}` enables a production bypass/test/simulation surface"
                    ));
                }
            }
        }
    }

    for path in rust_files_under(&crates_dir) {
        let rel = repo_relative(&path);
        if rel.contains("crates/aura-testkit/")
            || rel.contains("crates/aura-simulator/")
            || rel.contains("crates/aura-harness/")
            || rel.contains("crates/aura-quint/")
            || rel.contains("crates/aura-macros/")
            || rel.contains("crates/aura-terminal/src/demo/")
            || rel.contains("/tests/")
            || rel.ends_with("/tests.rs")
            || rel.ends_with("/test_support.rs")
        {
            continue;
        }

        let lines = read_lines(&path)?;
        for (idx, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.is_empty()
                || trimmed.starts_with("//")
                || trimmed.starts_with("#[cfg(test)]")
                || line_is_test_scoped(&lines, idx)
            {
                continue;
            }

            let window = lines[idx.saturating_sub(8)..=idx].join("\n");
            let simulation_scoped = window.contains("Simulation")
                || window.contains("simulation")
                || window.contains("for_simulation")
                || window.contains("cfg(test)");

            if let Some(found) = symbol_re.find(line) {
                violations.push(format!(
                    "{rel}:{} contains production bypass/test symbol `{}`",
                    idx + 1,
                    found.as_str()
                ));
            }

            for forbidden in ["demo_crypto", "demo-crypto"] {
                if line.contains(forbidden) {
                    violations.push(format!(
                        "{rel}:{} contains production demo crypto symbol `{forbidden}`",
                        idx + 1
                    ));
                }
            }

            for forbidden in [
                "signature: Vec::new()",
                "signature: vec![]",
                "threshold_signature: Vec::new()",
                "threshold_signature: vec![]",
                "vote_signature: Vec::new()",
                "vote_signature: vec![]",
                "proof: Vec::new()",
                "proof: vec![]",
                "placeholder_signature",
                "placeholder_proof",
                "EvidenceDelta::empty(",
                "verify_signatures: false",
            ] {
                if line.contains(forbidden) && !line.contains("partial_signature") {
                    violations.push(format!(
                        "{rel}:{} contains production unsigned/empty signature pattern `{forbidden}`",
                        idx + 1
                    ));
                }
            }

            for forbidden in [
                "RealCryptoHandler::seeded(",
                "RealCryptoHandler::for_simulation_seed(",
                "CryptoSubsystem::for_simulation_seed(",
                "CryptoRng::deterministic(",
                "StdRng::from_seed(",
            ] {
                let rng_adapter_from_effect_entropy =
                    forbidden == "StdRng::from_seed(" && window.contains("random_bytes_32()");
                if line.contains(forbidden)
                    && !simulation_scoped
                    && !rng_adapter_from_effect_entropy
                {
                    violations.push(format!(
                        "{rel}:{} contains production deterministic crypto pattern `{forbidden}`",
                        idx + 1
                    ));
                }
            }
        }
    }

    if !violations.is_empty() {
        bail!("security-bypass-symbols:\n{}", violations.join("\n"));
    }

    println!("security bypass symbol policy: clean");
    Ok(())
}

pub fn run_secret_field_wrappers() -> Result<()> {
    let repo_root = repo_root()?;
    let crates_dir = repo_root.join("crates");
    let field_re = Regex::new(r"\bpub(?:\s*\([^)]*\))?\s+([A-Za-z_][A-Za-z0-9_]*)\s*:\s*([^,]+)")?;
    let raw_bytes_re = Regex::new(
        r"(^|[<(\s])(?:Vec\s*<\s*u8\s*>|Box\s*<\s*\[\s*u8\s*\]\s*>|\[\s*u8\s*;\s*[A-Za-z0-9_]+\s*\])",
    )?;
    let wrapper_re =
        Regex::new(r"\b(SecretBytes|PrivateKeyBytes|SigningShareBytes|EncryptedSecretBlob)\b")?;
    let mut violations = Vec::new();
    validate_secret_wrapper_contracts(&repo_root, &mut violations)?;

    for path in rust_files_under(&crates_dir) {
        let rel = repo_relative(&path);
        if rel.contains("crates/aura-testkit/")
            || rel.contains("crates/aura-simulator/")
            || rel.contains("crates/aura-harness/")
            || rel.contains("crates/aura-quint/")
            || rel.contains("crates/aura-macros/")
            || rel.contains("crates/aura-terminal/src/demo/")
            || rel.contains("/tests/")
            || rel.ends_with("/tests.rs")
            || rel.ends_with("/test_support.rs")
        {
            continue;
        }

        let lines = read_lines(&path)?;
        for (idx, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.is_empty()
                || trimmed.starts_with("//")
                || trimmed.starts_with("#[cfg(test)]")
                || line_is_test_scoped(&lines, idx)
            {
                continue;
            }

            if line.contains("derive") && line.contains('(') && forbidden_secret_derives(line) {
                if let Some((item_line, block)) = deserializable_item_block(&lines, idx) {
                    let justification_window = lines[idx.saturating_sub(8)..=idx].join("\n");
                    if secret_bearing_type_or_block(&block)
                        && !block.contains("aura-security: raw-secret-field-justified")
                        && !block.contains("aura-security: secret-derive-justified")
                        && !justification_window
                            .contains("aura-security: raw-secret-field-justified")
                        && !justification_window.contains("aura-security: secret-derive-justified")
                    {
                        violations.push(format!(
                            "{rel}:{} secret-bearing type derives logging/cloning/serde/equality traits without an explicit aura-security justification",
                            item_line + 1
                        ));
                    }
                }
            }

            let Some(captures) = field_re.captures(line) else {
                continue;
            };
            let field_name = captures.get(1).map(|m| m.as_str()).unwrap_or_default();
            let field_ty = captures.get(2).map(|m| m.as_str()).unwrap_or_default();
            if wrapper_re.is_match(field_ty)
                || !raw_bytes_re.is_match(field_ty)
                || !secret_field_name_requires_wrapper(field_name)
            {
                continue;
            }

            let window_start = idx.saturating_sub(4);
            let justification_window = lines[window_start..=idx].join("\n");
            if justification_window.contains("aura-security: raw-secret-field-justified") {
                continue;
            }

            violations.push(format!(
                "{rel}:{} public secret-bearing field `{field_name}: {}` uses raw bytes; use SecretBytes, PrivateKeyBytes, SigningShareBytes, or EncryptedSecretBlob, or add an explicit aura-security justification",
                idx + 1,
                field_ty.trim()
            ));
        }
    }

    if !violations.is_empty() {
        bail!("secret-field-wrappers:\n{}", violations.join("\n"));
    }

    println!("secret field wrapper policy: clean");
    Ok(())
}

pub fn run_security_boolean_escape_hatch_boundary() -> Result<()> {
    let repo_root = repo_root()?;
    let crates_dir = repo_root.join("crates");
    let bool_decl_re = Regex::new(r"\b([A-Za-z_][A-Za-z0-9_]*)\s*:\s*bool\b")?;
    let mut violations = Vec::new();

    for path in rust_files_under(&crates_dir) {
        let rel = repo_relative(&path);
        if rel.contains("crates/aura-testkit/")
            || rel.contains("crates/aura-simulator/")
            || rel.contains("crates/aura-harness/")
            || rel.contains("crates/aura-quint/")
            || rel.contains("crates/aura-macros/")
            || rel.contains("crates/aura-terminal/src/demo/")
            || rel.contains("/tests/")
            || rel.ends_with("/tests.rs")
            || rel.ends_with("/test_support.rs")
        {
            continue;
        }

        let lines = read_lines(&path)?;
        for (idx, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.is_empty()
                || trimmed.starts_with("//")
                || trimmed.starts_with("#[cfg(test)]")
                || line_is_test_scoped(&lines, idx)
            {
                continue;
            }

            if let Some(captures) = bool_decl_re.captures(line) {
                let name = captures.get(1).map(|m| m.as_str()).unwrap_or_default();
                if security_boolean_name_requires_policy(name) {
                    let preceding = lines[idx.saturating_sub(8)..=idx].join("\n");
                    let signature_window =
                        lines[idx.saturating_sub(6)..lines.len().min(idx + 7)].join("\n");
                    let public_field = trimmed.starts_with("pub ")
                        || trimmed.starts_with("pub(")
                        || trimmed.starts_with("pub(crate)");
                    let config_field = enclosing_struct_name(&lines, idx)
                        .is_some_and(|name| name.ends_with("Config") || name.ends_with("Options"));
                    let public_parameter = signature_window.contains("pub fn ")
                        || signature_window.contains("pub async fn ");
                    if security_sensitive_context(&rel, &preceding)
                        && ((public_field && config_field) || public_parameter)
                        && !preceding.contains("aura-security: security-boolean-justified")
                    {
                        violations.push(format!(
                            "{rel}:{} security-critical boolean `{name}: bool` must be modeled as an explicit policy enum, typestate, or capability object",
                            idx + 1
                        ));
                    }
                }
            }

            if fail_open_security_evidence_pattern(&lines, idx)
                && !lines[idx.saturating_sub(8)..=idx]
                    .join("\n")
                    .contains("aura-security: fail-open-justified")
            {
                violations.push(format!(
                    "{rel}:{} appears to continue or default-success after missing security evidence; fail closed with a typed error or explicit policy object",
                    idx + 1
                ));
            }
        }
    }

    if !violations.is_empty() {
        bail!(
            "security-boolean-escape-hatch-boundary:\n{}",
            violations.join("\n")
        );
    }

    println!("security boolean escape-hatch boundary: clean");
    Ok(())
}

pub fn run_security_exception_metadata() -> Result<()> {
    let repo_root = repo_root()?;
    let crates_dir = repo_root.join("crates");
    let mut violations = Vec::new();

    for path in rust_files_under(&crates_dir) {
        let rel = repo_relative(&path);
        let lines = read_lines(&path)?;
        for (idx, line) in lines.iter().enumerate() {
            if !line.contains("aura-security:") {
                continue;
            }
            if line.contains("owner=") && line.contains("expires=") && line.contains("remediation=")
            {
                continue;
            }
            violations.push(format!(
                "{rel}:{} aura-security exception must include owner=, expires=, and remediation= metadata",
                idx + 1
            ));
        }
    }

    if !violations.is_empty() {
        bail!("security-exception-metadata:\n{}", violations.join("\n"));
    }

    println!("security exception metadata: clean");
    Ok(())
}

fn validate_secret_wrapper_contracts(repo_root: &Path, violations: &mut Vec<String>) -> Result<()> {
    let rel = "crates/aura-core/src/secrets.rs";
    let path = repo_root.join(rel);
    let contents = read(&path)?;
    for required in [
        "impl fmt::Debug for SecretBytes",
        ".field(\"bytes\", &\"<redacted>\")",
        "impl Zeroize for SecretBytes",
        "impl ZeroizeOnDrop for SecretBytes",
        "impl Drop for SecretBytes",
        "pub fn import(bytes: Vec<u8>) -> Self",
        "pub fn import_from_slice(bytes: &[u8]) -> Self",
        "pub fn export_secret(mut self, _context: SecretExportContext) -> Vec<u8>",
        "export_private_key,",
        "export_signing_share,",
        "export_encrypted_secret_blob,",
    ] {
        if !contents.contains(required) {
            violations.push(format!(
                "{rel}: secret wrapper contract missing required surface `{required}`"
            ));
        }
    }
    Ok(())
}

fn forbidden_secret_derives(line: &str) -> bool {
    [
        "Debug",
        "Clone",
        "Serialize",
        "Deserialize",
        "PartialEq",
        "Eq",
    ]
    .iter()
    .any(|derive| line.contains(derive))
}

fn secret_bearing_type_or_block(block: &str) -> bool {
    let lower = block.to_ascii_lowercase();
    let item_name_is_secret = lower.lines().any(|line| {
        let trimmed = line.trim_start();
        (trimmed.starts_with("pub struct ")
            || trimmed.starts_with("struct ")
            || trimmed.starts_with("pub enum ")
            || trimmed.starts_with("enum "))
            && !trimmed.contains("secretexport")
            && [
                "privatekey",
                "private_key",
                "ed25519signingkey",
                "single_signer_key_package",
                "singlesignerkeypackage",
            ]
            .iter()
            .any(|needle| trimmed.contains(needle))
    });
    if item_name_is_secret {
        return true;
    }

    let field_re =
        Regex::new(r"\b(?:pub(?:\s*\([^)]*\))?\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*:\s*([^,]+)")
            .expect("valid field regex");
    let raw_bytes_re =
        Regex::new(r"(^|[<(\s])(?:Vec\s*<\s*u8\s*>|Box\s*<\s*\[\s*u8\s*\]\s*>|\[\s*u8\s*;\s*[A-Za-z0-9_]+\s*\])")
            .expect("valid raw bytes regex");
    block.lines().any(|line| {
        let Some(captures) = field_re.captures(line) else {
            return false;
        };
        let field_name = captures.get(1).map(|m| m.as_str()).unwrap_or_default();
        let field_ty = captures.get(2).map(|m| m.as_str()).unwrap_or_default();
        raw_bytes_re.is_match(field_ty) && secret_field_name_requires_wrapper(field_name)
    })
}

fn security_boolean_name_requires_policy(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("skip_")
        || lower.starts_with("disable_")
        || lower.starts_with("allow_")
        || lower.starts_with("verify_")
        || lower.starts_with("optional_")
        || (lower.ends_with("_enabled")
            && [
                "auth",
                "capability",
                "crypto",
                "encrypt",
                "guard",
                "key",
                "merkle",
                "proof",
                "receipt",
                "signature",
                "storage",
                "transport",
            ]
            .iter()
            .any(|keyword| lower.contains(keyword)))
}

fn security_sensitive_context(rel: &str, window: &str) -> bool {
    let lower = format!("{}\n{}", rel, window).to_ascii_lowercase();
    [
        "auth",
        "authorization",
        "biscuit",
        "capability",
        "crypto",
        "encrypt",
        "guard",
        "journal",
        "key",
        "merkle",
        "proof",
        "receipt",
        "recovery",
        "signature",
        "sync",
        "transport",
    ]
    .iter()
    .any(|keyword| lower.contains(keyword))
}

fn enclosing_struct_name(lines: &[String], idx: usize) -> Option<String> {
    let struct_re =
        Regex::new(r"\b(?:pub\s+)?struct\s+([A-Za-z_][A-Za-z0-9_]*)").expect("valid struct regex");
    for start in (0..=idx).rev() {
        let line = lines[start].trim_start();
        if line.starts_with('}') {
            return None;
        }
        let Some(captures) = struct_re.captures(line) else {
            continue;
        };
        let block = lines[start..=idx].join("\n");
        let opens = block.matches('{').count();
        let closes = block.matches('}').count();
        if opens > closes {
            return captures.get(1).map(|m| m.as_str().to_string());
        }
    }
    None
}

fn fail_open_security_evidence_pattern(lines: &[String], idx: usize) -> bool {
    let line = lines[idx].trim();
    let lower_window = lines[idx.saturating_sub(8)..lines.len().min(idx + 4)]
        .join("\n")
        .to_ascii_lowercase();
    let security_evidence_context = [
        "missing verification",
        "without verification",
        "missing authorization",
        "missing receipt",
        "missing key",
        "missing proof",
        "missing encryption",
        "verification failed",
        "authorization failed",
    ]
    .iter()
    .any(|needle| lower_window.contains(needle));
    if security_evidence_context
        && (line == "continue;"
            || line.starts_with("return Ok(true")
            || line.starts_with("return None")
            || line.contains("valid: true")
            || line.contains("unwrap_or(true)")
            || line.contains("unwrap_or(Ok(true"))
    {
        return true;
    }

    lower_window.contains("verify")
        && (line.contains("unwrap_or(true)") || line.contains("unwrap_or_else(|_| true)"))
}

pub fn run_raw_transport_send_boundary() -> Result<()> {
    let repo_root = repo_root()?;
    let crates_dir = repo_root.join("crates");
    let mut violations = Vec::new();
    let boundary = read(&repo_root.join("crates/aura-agent/src/runtime/transport_boundary.rs"))?;
    if !boundary.contains("struct GuardChainSendReceipt")
        || !boundary.contains("fn send_raw_transport_envelope")
        || !boundary.contains("send_guarded_transport_envelope")
    {
        violations.push(
            "crates/aura-agent/src/runtime/transport_boundary.rs:1 guarded send boundary must keep GuardChainSendReceipt, private raw send, and guarded send API".to_string(),
        );
    }
    if boundary.contains("pub fn send_raw_transport_envelope")
        || boundary.contains("pub(crate) fn send_raw_transport_envelope")
        || boundary.contains("pub async fn send_raw_transport_envelope")
        || boundary.contains("pub(crate) async fn send_raw_transport_envelope")
    {
        violations.push(
            "crates/aura-agent/src/runtime/transport_boundary.rs:1 raw transport send helper must remain private".to_string(),
        );
    }

    for path in rust_files_under(&crates_dir) {
        let rel = repo_relative(&path);
        if rel.ends_with("crates/aura-agent/src/runtime/transport_boundary.rs")
            || rel.ends_with("crates/aura-agent/src/runtime/effects/transport.rs")
            || rel.contains("crates/aura-effects/src/transport/")
            || rel.contains("crates/aura-testkit/")
            || rel.contains("crates/aura-simulator/")
            || rel.contains("crates/aura-harness/")
            || rel.contains("crates/aura-terminal/src/demo/")
            || rel.contains("examples/")
            || rel.contains("/tests/")
            || rel.ends_with("/tests.rs")
            || rel.ends_with("/test_support.rs")
        {
            continue;
        }

        let lines = read_lines(&path)?;
        for (idx, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.is_empty()
                || trimmed.starts_with("//")
                || trimmed.starts_with("#[cfg(test)]")
                || line_is_test_scoped(&lines, idx)
            {
                continue;
            }

            if line.contains(".send_envelope(") || line.contains("TransportEffects::send_envelope")
            {
                violations.push(format!(
                    "{rel}:{} raw transport send escaped runtime boundary",
                    idx + 1
                ));
            }
        }
    }

    if !violations.is_empty() {
        bail!("raw-transport-send-boundary:\n{}", violations.join("\n"));
    }

    println!("raw transport send boundary: clean");
    Ok(())
}

pub fn run_canonical_remote_apply_boundary() -> Result<()> {
    let repo_root = repo_root()?;
    let crates_dir = repo_root.join("crates");
    let mut violations = Vec::new();

    for path in rust_files_under(&crates_dir) {
        let rel = repo_relative(&path);
        if rel.ends_with("crates/aura-sync/src/protocols/journal_apply.rs")
            || rel.contains("crates/aura-testkit/")
            || rel.contains("crates/aura-simulator/")
            || rel.contains("crates/aura-harness/")
            || rel.contains("/tests/")
            || rel.ends_with("/tests.rs")
            || rel.ends_with("/test_support.rs")
        {
            continue;
        }

        let lines = read_lines(&path)?;
        for (idx, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.is_empty()
                || trimmed.starts_with("//")
                || trimmed.starts_with("#[cfg(test)]")
                || line_is_test_scoped(&lines, idx)
            {
                continue;
            }

            if line.contains(".apply_verified_delta(")
                || line.contains(".accept_verified_relational_facts(")
            {
                let window = lines[idx.saturating_sub(3)..=idx].join("\n");
                if !window.contains("JournalApplyService::new()") {
                    violations.push(format!(
                        "{rel}:{} remote journal apply must go through JournalApplyService",
                        idx + 1
                    ));
                }
            }
        }
    }

    if !violations.is_empty() {
        bail!(
            "canonical-remote-apply-boundary:\n{}",
            violations.join("\n")
        );
    }

    println!("canonical remote apply boundary: clean");
    Ok(())
}

pub fn run_journal_authorization_wiring_boundary() -> Result<()> {
    let repo_root = repo_root()?;
    let runtime_rel = "crates/aura-agent/src/runtime/effects.rs";
    let journal_rel = "crates/aura-journal/src/effects.rs";
    let runtime = read(repo_root.join(runtime_rel))?;
    let journal = read(repo_root.join(journal_rel))?;
    let mut violations = Vec::new();

    for required in [
        "struct JournalBiscuitAuthorizationHandler",
        "VerifiedBiscuitToken::from_bytes",
        "expect(\"journal handler requires a Biscuit authorization policy\")",
        "expect(\"journal authorization policy token must serialize\")",
    ] {
        if !runtime.contains(required) {
            violations.push(format!(
                "{runtime_rel}: journal runtime wiring missing required fail-closed surface `{required}`"
            ));
        }
    }
    for forbidden in [
        "NoopBiscuitAuthorizationHandler",
        "JournalHandlerFactory::create(\n            self.authority_id,\n            self.crypto.handler().clone(),\n            self.storage_handler.clone(),\n            None",
    ] {
        if runtime.contains(forbidden) {
            violations.push(format!(
                "{runtime_rel}: journal runtime wiring contains forbidden optional/no-op authorization pattern `{forbidden}`"
            ));
        }
    }
    if journal.contains("authorization: Option<(Vec<u8>, A)>") {
        violations.push(format!(
            "{journal_rel}: JournalHandlerFactory::create must require authorization instead of accepting Option"
        ));
    }

    if !violations.is_empty() {
        bail!(
            "journal-authorization-wiring-boundary:\n{}",
            violations.join("\n")
        );
    }

    println!("journal authorization wiring boundary: clean");
    Ok(())
}

pub fn run_invitation_legacy_fallback_boundary() -> Result<()> {
    let repo_root = repo_root()?;
    let paths = [
        repo_root.join("crates/aura-agent/src/handlers/invitation"),
        repo_root.join("crates/aura-agent/src/handlers/invitation.rs"),
    ];
    let mut violations = Vec::new();
    let forbidden = [
        "Legacy fallback",
        "legacy imported",
        "legacy-imported",
        "old_format_imported",
        "legacy invite",
        ".split(\"from \")",
    ];

    for path in paths {
        let files = if path.is_dir() {
            rust_files_under(&path)
        } else if path.exists() {
            vec![path]
        } else {
            Vec::new()
        };

        for file in files {
            let rel = repo_relative(&file);
            let lines = read_lines(&file)?;
            for (idx, line) in lines.iter().enumerate() {
                if forbidden.iter().any(|needle| line.contains(needle)) {
                    violations.push(format!(
                        "{rel}:{} reintroduces legacy invitation compatibility fallback",
                        idx + 1
                    ));
                }
            }
        }
    }

    if !violations.is_empty() {
        bail!(
            "invitation-legacy-fallback-boundary:\n{}",
            violations.join("\n")
        );
    }

    println!("invitation legacy fallback boundary: clean");
    Ok(())
}

fn secret_field_name_requires_wrapper(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower.contains("public_key")
        || lower.contains("verifying_key")
        || lower.contains("signature")
        || lower.contains("commitment")
    {
        return false;
    }

    [
        "private",
        "secret",
        "key_material",
        "key_package",
        "key_packages",
        "signing_share",
        "share_data",
        "share_bytes",
        "encrypted_share",
        "threshold_config",
        "route_secret",
        "path_secret",
        "master_key",
        "token_bytes",
        "seed",
        "recovery_material",
    ]
    .iter()
    .any(|keyword| lower.contains(keyword))
}

fn line_is_test_scoped(lines: &[String], idx: usize) -> bool {
    let start = idx.saturating_sub(20);
    lines[..=idx].iter().any(|line| line.contains("mod tests"))
        || lines[start..=idx].iter().any(|line| {
            line.contains("#[cfg(test)]")
                || line.contains("_for_tests")
                || line.contains("fn verified_ops(")
        })
}

fn require_verified_ingress_parameter(
    rel: &str,
    lines: &[String],
    fn_name: &str,
    violations: &mut Vec<String>,
) {
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let is_function = trimmed.starts_with(&format!("fn {fn_name}("))
            || trimmed.starts_with(&format!("fn {fn_name}<"))
            || trimmed.starts_with(&format!("pub fn {fn_name}("))
            || trimmed.starts_with(&format!("pub fn {fn_name}<"))
            || trimmed.starts_with(&format!("async fn {fn_name}("))
            || trimmed.starts_with(&format!("async fn {fn_name}<"))
            || trimmed.starts_with(&format!("pub async fn {fn_name}("))
            || trimmed.starts_with(&format!("pub async fn {fn_name}<"));
        if !is_function {
            continue;
        }
        if line_is_test_scoped(lines, idx) {
            continue;
        }
        let window = lines[idx..lines.len().min(idx + 12)].join("\n");
        if window.contains("VerifiedIngress<")
            || window.contains("&VerifiedIngress<")
            || window.contains("Verified")
        {
            continue;
        }
        let path_is_sync_boundary = rel.contains("aura-sync/src/protocols")
            || rel.contains("aura-anti-entropy/src")
            || rel.contains("aura-agent/src/runtime/effects/sync.rs")
            || rel.contains("aura-agent/src/handlers/auth.rs");
        if path_is_sync_boundary {
            violations.push(format!(
                "{rel}:{} `{fn_name}` must accept VerifiedIngress for peer-originated input",
                idx + 1
            ));
        }
    }
}

fn run_runtime_typed_lifecycle_bridge() -> Result<()> {
    super::runtime_typed_lifecycle_bridge::run()
}

fn run_privacy_onion_quarantine() -> Result<()> {
    let repo_root = repo_root()?;
    let lane_files = [
        ".github/workflows/ci.yml",
        ".github/workflows/harness.yml",
        "justfile",
        "scripts/ci/browser-smoke.sh",
        "scripts/ci/web-matrix.sh",
        "scripts/ci/tui-matrix.sh",
        "scripts/ci/web-semantic.sh",
        "scripts/ci/tui-semantic.sh",
        "scripts/ci/web-conformance.sh",
        "scripts/ci/tui-conformance.sh",
    ];
    let mut lane_args = vec![
        "-n".into(),
        "transparent_onion|--features[ =][^\\n]*transparent_onion".into(),
    ];
    lane_args.extend(
        lane_files
            .iter()
            .map(|path| repo_relative(repo_root.join(path))),
    );
    let lane_hits = rg_lines(&lane_args)?;
    if !lane_hits.is_empty() {
        for hit in lane_hits {
            eprintln!("{hit}");
        }
        bail!(
            "transparent-onion-quarantine: harness and shared-flow lanes must not enable or depend on transparent_onion"
        );
    }
    let source_hits = rg_lines(&vec![
        "-n".into(),
        "TransparentAnonymousSetup|TransparentMoveEnvelope|TransparentMoveTrafficClass|transparent_headers|PathProtectionMode::TransparentDebug|feature *= *\"transparent_onion\"".into(),
        repo_relative(repo_root.join("crates")),
    ])?;
    let allowed: HashSet<_> = [
        "crates/aura-core/src/service.rs",
        "crates/aura-core/src/lib.rs",
        "crates/aura-agent/src/lib.rs",
        "crates/aura-effects/src/lib.rs",
        "crates/aura-protocol/src/lib.rs",
        "crates/aura-social/src/lib.rs",
        "crates/aura-sync/src/lib.rs",
    ]
    .into_iter()
    .collect();
    let source_violations: Vec<_> = source_hits
        .into_iter()
        .filter(|hit| {
            !allowed
                .iter()
                .any(|allowed_path| hit.starts_with(&format!("{allowed_path}:")))
        })
        .collect();
    if !source_violations.is_empty() {
        for hit in source_violations {
            eprintln!("{hit}");
        }
        bail!(
            "transparent-onion-quarantine: transparent debug surfaces must remain quarantined to the explicit allowlist"
        );
    }
    println!("transparent-onion-quarantine: ok");
    Ok(())
}

fn default_diff_range() -> Result<Option<String>> {
    let base_ref = std::env::var("GITHUB_BASE_REF")
        .ok()
        .filter(|value| !value.is_empty());
    if let Some(base_ref) = base_ref {
        let origin = format!("origin/{base_ref}");
        let status = command_stdout(
            "git",
            &["rev-parse".into(), "--verify".into(), origin.clone()],
        )
        .map(|_| ())
        .map(|_| Some(format!("{origin}...HEAD")));
        if let Ok(value) = status {
            return Ok(value);
        }
    }
    let head = command_stdout(
        "git",
        &["rev-parse".into(), "--verify".into(), "HEAD".into()],
    );
    if head.is_ok() {
        return Ok(Some("HEAD".to_string()));
    }
    Ok(None)
}

fn completeness_violations(repo_root: &Path, mode: &str) -> Result<Vec<String>> {
    let attr = match mode {
        "semantic-owner" => "#[aura_macros::semantic_owner",
        "actor-owned" => "#[aura_macros::actor_",
        "capability-boundary" => "#[aura_macros::capability_boundary",
        _ => return Ok(Vec::new()),
    };
    let required_entries: &[(&str, &[&str])] = match mode {
        "semantic-owner" => &[(
            "",
            &[
                "crates/aura-app/src/workflows/account/bootstrap.rs:initialize_runtime_account_owned",
                "crates/aura-app/src/workflows/ceremonies.rs:start_device_enrollment_ceremony_owned",
                "crates/aura-app/src/workflows/context/neighborhood.rs:create_home_owned",
                "crates/aura-app/src/workflows/invitation/accept.rs:accept_invitation_id_owned",
                "crates/aura-app/src/workflows/invitation/accept.rs:accept_imported_invitation_owned",
                "crates/aura-app/src/workflows/invitation/pending_accept.rs:accept_pending_channel_invitation_id_owned",
                "crates/aura-app/src/workflows/invitation/create.rs:create_channel_invitation_owned",
                "crates/aura-app/src/workflows/messaging/channels.rs:join_channel_by_name_owned",
                "crates/aura-app/src/workflows/messaging/send.rs:send_message_ref_owned",
                "crates/aura-app/src/workflows/messaging/invites.rs:invite_user_to_channel_with_context_owned",
            ],
        )],
        "capability-boundary" => &[(
            "",
            &[
                "crates/aura-app/src/workflows/semantic_facts/owner.rs:semantic_lifecycle_publication_capability",
                "crates/aura-app/src/workflows/semantic_facts/owner.rs:semantic_readiness_publication_capability",
                "crates/aura-app/src/workflows/semantic_facts/owner.rs:semantic_postcondition_proof_capability",
                "crates/aura-app/src/workflows/semantic_facts/owner.rs:authorize_readiness_publication",
                "crates/aura-app/src/workflows/semantic_facts/owner.rs:issue_semantic_operation_context",
                "crates/aura-app/src/workflows/semantic_facts/proofs/proof_issuance.rs:issue_home_created_proof",
                "crates/aura-app/src/workflows/semantic_facts/proofs/proof_issuance.rs:issue_account_created_proof",
                "crates/aura-app/src/workflows/semantic_facts/proofs/proof_issuance.rs:issue_channel_membership_ready_proof",
                "crates/aura-app/src/workflows/semantic_facts/proofs/proof_issuance.rs:issue_invitation_created_proof",
                "crates/aura-app/src/workflows/semantic_facts/proofs/proof_issuance.rs:issue_invitation_exported_proof",
                "crates/aura-app/src/workflows/semantic_facts/proofs/proof_issuance.rs:issue_channel_invitation_created_proof",
                "crates/aura-app/src/workflows/semantic_facts/proofs/proof_issuance.rs:issue_invitation_accepted_or_materialized_proof",
                "crates/aura-app/src/workflows/semantic_facts/proofs/proof_issuance.rs:issue_pending_invitation_consumed_proof",
                "crates/aura-app/src/workflows/semantic_facts/proofs/proof_issuance.rs:issue_invitation_declined_proof",
                "crates/aura-app/src/workflows/semantic_facts/proofs/proof_issuance.rs:issue_invitation_revoked_proof",
                "crates/aura-app/src/workflows/semantic_facts/proofs/proof_issuance.rs:issue_device_enrollment_started_proof",
                "crates/aura-app/src/workflows/semantic_facts/proofs/proof_issuance.rs:issue_message_committed_proof",
                "crates/aura-app/src/workflows/semantic_facts/proofs/proof_issuance.rs:issue_device_enrollment_imported_proof",
                "crates/aura-agent/src/runtime_bridge/mod.rs:secure_storage_bootstrap_boundary",
                "crates/aura-agent/src/runtime_bridge/mod.rs:secure_storage_bootstrap_store_capabilities",
                "crates/aura-agent/src/runtime_bridge/identity.rs:get_settings",
                "crates/aura-agent/src/runtime_bridge/identity.rs:list_devices",
                "crates/aura-agent/src/runtime_bridge/identity.rs:list_authorities",
                "crates/aura-agent/src/runtime_bridge/identity.rs:set_nickname_suggestion",
                "crates/aura-agent/src/runtime_bridge/identity.rs:set_mfa_policy",
                "crates/aura-agent/src/runtime_bridge/identity.rs:current_time_ms",
                "crates/aura-agent/src/runtime_bridge/identity.rs:sleep_ms",
                "crates/aura-agent/src/runtime_bridge/identity.rs:authentication_status",
                "crates/aura-agent/src/runtime_bridge/sync.rs:get_sync_status",
                "crates/aura-agent/src/runtime_bridge/sync.rs:is_peer_online",
                "crates/aura-agent/src/runtime_bridge/sync.rs:get_sync_peers",
                "crates/aura-agent/src/runtime_bridge/sync.rs:trigger_sync",
                "crates/aura-agent/src/runtime_bridge/sync.rs:process_ceremony_messages",
                "crates/aura-agent/src/runtime_bridge/sync.rs:sync_with_peer",
                "crates/aura-agent/src/runtime_bridge/sync.rs:ensure_peer_channel",
                "crates/aura-agent/src/reactive/app_signal_projection.rs:map_invitation_type",
                "crates/aura-agent/src/reactive/app_signal_projection.rs:map_channel_metadata",
                "crates/aura-agent/src/reactive/app_signal_projection.rs:collect_moderation_homes",
                "crates/aura-chat/src/guards.rs:plan_local_commit_execution",
                "crates/aura-invitation/src/guards.rs:plan_accept_execution",
                "crates/aura-invitation/src/guards.rs:plan_send_execution",
                "crates/aura-recovery/src/guardian_setup.rs:validate_setup_inputs",
                "crates/aura-recovery/src/guardian_setup.rs:build_setup_completion",
            ],
        )],
        "actor-owned" => &[("", &[])],
        _ => &[],
    };
    let mut violations = Vec::new();
    match mode {
        "actor-owned" => {
            let struct_re = Regex::new(
                r"^(pub )?struct [A-Za-z0-9_]*(Service|Manager|Coordinator|Subsystem|Actor)\b",
            )?;
            for file in rust_files_under(repo_root.join("crates/aura-agent/src/runtime/services")) {
                let contents = read(&file)?;
                if struct_re.is_match(&contents)
                    && !contents.contains("#[aura_macros::actor_owned")
                    && !contents.contains("#[aura_macros::actor_root")
                {
                    violations.push(format!(
                        "{}: runtime service subtree completeness requires #[aura_macros::actor_owned] or #[aura_macros::actor_root]",
                        repo_relative(file)
                    ));
                }
            }
        }
        _ => {
            for (_, entries) in required_entries {
                for entry in *entries {
                    let (file, function_name) = entry.split_once(':').unwrap();
                    if !file_has_attr_for_function(repo_root.join(file), function_name, attr)? {
                        violations.push(format!(
                            "{file}: completeness requires {attr} near fn {function_name}(...)"
                        ));
                    }
                }
            }
        }
    }
    Ok(violations)
}

fn file_has_attr_for_function(
    path: impl AsRef<Path>,
    function_name: &str,
    attr: &str,
) -> Result<bool> {
    let lines = read_lines(path)?;
    for (idx, line) in lines.iter().enumerate() {
        if line.contains(&format!("fn {function_name}("))
            || line.contains(&format!("async fn {function_name}("))
        {
            let start = idx.saturating_sub(16);
            return Ok(lines[start..idx].iter().any(|line| line.contains(attr)));
        }
    }
    Ok(false)
}

fn candidate_requires_attr(mode: &str, current_file: &str, added: &str) -> bool {
    match mode {
        "semantic-owner" => {
            (current_file.starts_with("crates/aura-app/src/workflows/")
                || current_file.starts_with("crates/aura-web/src/")
                || current_file.starts_with("crates/aura-terminal/src/"))
                && Regex::new(
                    r"^\s*(pub(\s*\([^)]*\))?\s+)?async\s+fn\s+[A-Za-z0-9_]+(_owned|_with_terminal_status)\(",
                )
                .unwrap()
                .is_match(added)
        },
        "actor-owned" => current_file.starts_with("crates/aura-agent/src/runtime/services/")
            && Regex::new(
                r".*struct\s+[A-Za-z0-9_]*(Service|Manager|Coordinator|Subsystem|Actor)(\s*[{<]|$)",
            )
            .unwrap()
            .is_match(added),
        "capability-boundary" => {
            (current_file.starts_with("crates/aura-app/src/workflows/")
                || current_file.starts_with("crates/aura-agent/src/runtime_bridge/")
                || current_file.starts_with("crates/aura-agent/src/reactive/")
                || current_file.starts_with("crates/aura-chat/src/")
                || current_file.starts_with("crates/aura-invitation/src/")
                || current_file.starts_with("crates/aura-recovery/src/"))
                && Regex::new(
                    r".*fn\s+(issue_[A-Za-z0-9_]+_(proof|context)|[A-Za-z0-9_]*capability|authorize_[A-Za-z0-9_]+|secure_storage_[A-Za-z0-9_]+|plan_[A-Za-z0-9_]+|validate_setup_inputs|build_setup_completion|map_invitation_type|map_channel_metadata|collect_moderation_homes|get_settings|list_devices|list_authorities|set_nickname_suggestion|set_mfa_policy|current_time_ms|sleep_ms|authentication_status|get_sync_status|is_peer_online|get_sync_peers|trigger_sync|process_ceremony_messages|sync_with_peer|ensure_peer_channel)\(",
                )
                .unwrap()
                .is_match(added)
        },
        _ => false,
    }
}

fn assert_cfg_pair(path: &Path, target: &str) -> Result<()> {
    let lines = read_lines(path)?;
    let matches: Vec<_> = lines
        .iter()
        .enumerate()
        .filter_map(|(idx, line)| (line == target).then_some(idx))
        .collect();
    if matches.len() != 1 {
        bail!(
            "testkit exception boundary: expected exactly one `{target}` entry in {}",
            path.display()
        );
    }
    let idx = matches[0];
    if idx == 0 || lines[idx - 1] != "#[cfg(not(target_arch = \"wasm32\"))]" {
        bail!(
            "testkit exception boundary: `{target}` must be immediately preceded by `#[cfg(not(target_arch = \"wasm32\"))]` in {}",
            path.display()
        );
    }
    Ok(())
}

fn parse_hit_path_line(hit: &str) -> Result<(&str, usize)> {
    let mut parts = hit.splitn(3, ':');
    let file = parts.next().context("missing hit file")?;
    let line = parts
        .next()
        .context("missing hit line")?
        .parse::<usize>()
        .context("parsing hit line")?;
    Ok((file, line))
}

fn is_after_cfg_test(lines: &[String], line_number: usize) -> bool {
    let test_start = lines
        .iter()
        .enumerate()
        .take(line_number)
        .rev()
        .find_map(|(idx, line)| line.contains("#[cfg(test)]").then_some(idx + 1));
    test_start.is_some_and(|start| line_number >= start)
}

fn extract_ts_function(contents: &str, signature: &str) -> Option<String> {
    let start = contents.find(signature)?;
    let body = &contents[start..];
    let next = body
        .match_indices("async function ")
        .map(|(idx, _)| idx)
        .filter(|idx| *idx > 0)
        .next();
    Some(match next {
        Some(idx) => body[..idx].to_string(),
        None => body.to_string(),
    })
}

/// Compute the git diff range for diff-aware policy checks.
/// Priority: env var → GITHUB_BASE_REF PR range → HEAD (unstaged).
fn compute_diff_range(env_var: &str) -> Result<Option<String>> {
    if let Ok(range) = env::var(env_var) {
        if !range.is_empty() {
            return Ok(Some(range));
        }
    }
    if let Ok(base_ref) = env::var("GITHUB_BASE_REF") {
        if !base_ref.is_empty() {
            let origin_ref = format!("origin/{base_ref}");
            let ok = Command::new("git")
                .args(["rev-parse", "--verify", &origin_ref])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if ok {
                return Ok(Some(format!("{origin_ref}...HEAD")));
            }
        }
    }
    let head_ok = Command::new("git")
        .args(["rev-parse", "--verify", "HEAD"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if head_ok {
        return Ok(Some("HEAD".to_owned()));
    }
    Ok(None)
}

/// Return the list of files changed in `diff_range` (via `git diff --name-only`).
fn diff_names(diff_range: &str) -> Result<BTreeSet<String>> {
    let output = Command::new("git")
        .args(["diff", "--name-only", diff_range])
        .output()
        .context("running git diff --name-only")?;
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect())
}

/// Resolve the changed-file list for guidance-sync checks.
/// Honours `AURA_UX_GUIDANCE_CHANGED_FILES` (newline-separated) or falls back to git diff.
fn changed_files_for_guidance() -> Result<BTreeSet<String>> {
    if let Ok(files) = env::var("AURA_UX_GUIDANCE_CHANGED_FILES") {
        return Ok(files
            .lines()
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect());
    }
    let range = match compute_diff_range("AURA_UX_GUIDANCE_DIFF_RANGE")? {
        Some(r) => r,
        None => return Ok(BTreeSet::new()),
    };
    diff_names(&range)
}

fn run_head_shell_script(script: &str, extra_args: &[String]) -> Result<()> {
    let repo_root = repo_root()?;
    let script_path = repo_root.join(script);
    let created = if script_path.exists() {
        false
    } else {
        let contents = command_stdout("git", &["show".into(), format!("HEAD:{script}")])?;
        if let Some(parent) = script_path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        fs::write(&script_path, contents)
            .with_context(|| format!("writing {}", script_path.display()))?;
        true
    };

    let mut args = vec![repo_relative(&script_path)];
    args.extend(extra_args.iter().cloned());
    let result = run_ok("bash", &args);

    if created {
        let _ = fs::remove_file(&script_path);
    }

    result
}

fn browser_driver_dir() -> Result<std::path::PathBuf> {
    Ok(repo_root()?.join("crates/aura-harness/playwright-driver"))
}

fn ensure_browser_toolchain() -> Result<()> {
    run_ok("node", &["--version".into()])
        .context("harness-browser-toolchain: node not found in PATH")?;
    run_ok("npm", &["--version".into()])
        .context("harness-browser-toolchain: npm not found in PATH")?;

    let driver_dir = browser_driver_dir()?;
    let compiler_path = driver_dir.join("node_modules/typescript/bin/tsc");
    let playwright_path = driver_dir.join("node_modules/playwright/package.json");

    if !compiler_path.exists() || !playwright_path.exists() {
        run_ok_in_dir("npm", &["ci".into()], &driver_dir)?;
    }

    if !compiler_path.exists() {
        bail!(
            "harness-browser-toolchain: missing TypeScript compiler after npm ci: {}",
            repo_relative(&compiler_path)
        );
    }
    if !playwright_path.exists() {
        bail!(
            "harness-browser-toolchain: missing Playwright package after npm ci: {}",
            repo_relative(&playwright_path)
        );
    }

    Ok(())
}

fn run_harness_governance_check(check: &str) -> Result<()> {
    run_ok(
        "cargo",
        &[
            "run".into(),
            "-p".into(),
            "aura-harness".into(),
            "--bin".into(),
            "aura-harness".into(),
            "--quiet".into(),
            "--".into(),
            "governance".into(),
            check.into(),
        ],
    )
}

pub fn run_browser_cache_lifecycle() -> Result<()> {
    let repo_root = repo_root()?;
    let ui_contract_dir = repo_root.join("crates/aura-app/src/ui_contract");
    let ui_contract_file = repo_root.join("crates/aura-app/src/ui_contract.rs");

    let search_paths = {
        let mut paths = vec![ui_contract_file.to_string_lossy().into_owned()];
        if ui_contract_dir.exists() {
            paths.push(ui_contract_dir.to_string_lossy().into_owned());
        }
        paths
    };

    let mut args = vec!["pub const BROWSER_CACHE_BOUNDARIES".into()];
    args.extend(search_paths.clone());
    if !rg_exists(&args)? {
        bail!("harness-browser-cache-lifecycle: missing browser cache lifecycle metadata");
    }

    for reason in [
        "session_start",
        "authority_switch",
        "device_import",
        "storage_reset",
        "navigation_recovery",
    ] {
        let mut args = vec![reason.into()];
        args.extend(search_paths.clone());
        if !rg_exists(&args)? {
            bail!(
                "harness-browser-cache-lifecycle: missing browser cache lifecycle reason: {reason}"
            );
        }
    }

    println!("harness browser cache lifecycle: clean");
    Ok(())
}

pub fn run_browser_cache_owner() -> Result<()> {
    let repo_root = repo_root()?;
    let driver = repo_root.join("crates/aura-harness/playwright-driver/src/playwright_driver.ts");

    let start_line: Option<u64> = rg_lines(&[
        "-n".into(),
        "^function resetUiObservationState".into(),
        driver.to_string_lossy().into_owned(),
    ])?
    .first()
    .and_then(|l| l.split(':').next().and_then(|n| n.parse().ok()));

    let end_line: Option<u64> = rg_lines(&[
        "-n".into(),
        "^function resetObservationState".into(),
        driver.to_string_lossy().into_owned(),
    ])?
    .first()
    .and_then(|l| l.split(':').next().and_then(|n| n.parse().ok()));

    let (start, end) = match (start_line, end_line) {
        (Some(s), Some(e)) => (s, e),
        _ => bail!(
            "harness-browser-cache-owner: could not locate browser cache owner function boundaries"
        ),
    };

    let hits = command_stdout(
        "rg",
        &[
            "--no-heading".into(),
            "-n".into(),
            r"session\.uiStateCache = null|session\.uiStateCacheJson = null|session\.uiStateVersion = 0|session\.requiredUiStateRevision = 0".into(),
            driver.to_string_lossy().into_owned(),
        ],
    )
    .unwrap_or_default();

    for hit in hits.lines().filter(|l| !l.is_empty()) {
        let line_num: u64 = hit
            .split(':')
            .next()
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        if line_num < start || line_num >= end {
            eprintln!("{hit}");
            bail!(
                "harness-browser-cache-owner: browser cache reset logic must stay inside resetUiObservationState"
            );
        }
    }

    println!("harness browser cache owner: clean");
    Ok(())
}

pub fn run_browser_driver_contract_sync() -> Result<()> {
    // long-block-exception: multi-stage sync check between Rust and TS driver contract files
    let repo_root = repo_root()?;
    let rust_contract = repo_root.join("crates/aura-web/src/harness/driver_contract.rs");
    let ts_contract =
        repo_root.join("crates/aura-harness/playwright-driver/src/driver_contract.ts");
    let ts_driver =
        repo_root.join("crates/aura-harness/playwright-driver/src/playwright_driver.ts");

    if !rust_contract.exists() {
        bail!(
            "browser-driver-contract-sync: missing Rust contract file: {}",
            repo_relative(&rust_contract)
        );
    }
    if !ts_contract.exists() {
        bail!(
            "browser-driver-contract-sync: missing TS contract file: {}",
            repo_relative(&ts_contract)
        );
    }
    if !ts_driver.exists() {
        bail!(
            "browser-driver-contract-sync: missing TS driver file: {}",
            repo_relative(&ts_driver)
        );
    }

    // Extract constants from Rust contract: `pub(crate) const NAME: &str = "value";`
    let rust_src = read(&rust_contract)?;
    let rust_const_re = Regex::new(r#"pub\(crate\) const (\w+): &str = "([^"]+)";"#)?;
    let mut rust_consts: Vec<String> = rust_const_re
        .captures_iter(&rust_src)
        .map(|c| format!("{}={}", &c[1], &c[2]))
        .collect();
    rust_consts.sort();

    // Extract constants from TS contract: `export const NAME = "value";`
    let ts_src = read(&ts_contract)?;
    let ts_const_re = Regex::new(r#"export const (\w+) = "([^"]+)";"#)?;
    let mut ts_consts: Vec<String> = ts_const_re
        .captures_iter(&ts_src)
        .map(|c| format!("{}={}", &c[1], &c[2]))
        .collect();
    ts_consts.sort();

    if rust_consts.is_empty() {
        bail!("browser-driver-contract-sync: failed to extract Rust contract constants");
    }
    if ts_consts.is_empty() {
        bail!("browser-driver-contract-sync: failed to extract TS contract constants");
    }
    if rust_consts != ts_consts {
        eprintln!("Rust constants: {rust_consts:?}");
        eprintln!("TS constants: {ts_consts:?}");
        bail!("browser-driver-contract-sync: Rust and TS browser-driver contract constants differ");
    }

    // Check SemanticQueuePayload fields
    let rust_semantic_re = Regex::new(r"struct SemanticQueuePayload \{([^}]*)\}")?;
    let rust_semantic_field_re = Regex::new(r"pub\(crate\) (\w+):")?;
    let mut rust_semantic: Vec<String> = rust_semantic_re
        .captures(&rust_src)
        .map(|c| {
            rust_semantic_field_re
                .captures_iter(&c[1])
                .map(|f| f[1].to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    rust_semantic.sort();

    let ts_semantic_re = Regex::new(r"type SemanticQueuePayload = \{([^}]*)\};")?;
    let ts_field_re = Regex::new(r"(\w+):")?;
    let mut ts_semantic: Vec<String> = ts_semantic_re
        .captures(&ts_src)
        .map(|c| {
            ts_field_re
                .captures_iter(&c[1])
                .map(|f| f[1].to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    ts_semantic.sort();

    if rust_semantic != ts_semantic {
        bail!(
            "browser-driver-contract-sync: Semantic queue payload fields differ between Rust and TS contracts"
        );
    }

    // Check RuntimeStageQueuePayload fields
    let rust_runtime_re = Regex::new(r"struct RuntimeStageQueuePayload \{([^}]*)\}")?;
    let mut rust_runtime: Vec<String> = rust_runtime_re
        .captures(&rust_src)
        .map(|c| {
            rust_semantic_field_re
                .captures_iter(&c[1])
                .map(|f| f[1].to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    rust_runtime.sort();

    let ts_runtime_re = Regex::new(r"type RuntimeStageQueuePayload = \{([^}]*)\};")?;
    let mut ts_runtime: Vec<String> = ts_runtime_re
        .captures(&ts_src)
        .map(|c| {
            ts_field_re
                .captures_iter(&c[1])
                .map(|f| f[1].to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    ts_runtime.sort();

    if rust_runtime != ts_runtime {
        bail!(
            "browser-driver-contract-sync: Runtime-stage queue payload fields differ between Rust and TS contracts"
        );
    }

    // Check raw driver names against allowlist
    let driver_src = read(&ts_driver)?;
    let raw_name_re = Regex::new(r#""(__AURA_DRIVER_[A-Z0-9_]+)""#)?;
    let mut raw_names: BTreeSet<String> = raw_name_re
        .captures_iter(&driver_src)
        .map(|c| c[1].to_string())
        .collect();

    let allowed_names: BTreeSet<&str> = [
        "__AURA_DRIVER_OBSERVER_INSTALLED",
        "__AURA_DRIVER_PUSH_CLIPBOARD",
        "__AURA_DRIVER_PUSH_RENDER_HEARTBEAT",
        "__AURA_DRIVER_PUSH_STATE",
        "__AURA_DRIVER_PUSH_UI_STATE",
        "__AURA_DRIVER_RUNTIME_STAGE_ENQUEUE_DISPATCHED__",
        "__AURA_DRIVER_SEMANTIC_ENQUEUE_DISPATCHED__",
    ]
    .into();

    let ts_const_values: BTreeSet<String> = ts_const_re
        .captures_iter(&ts_src)
        .map(|c| c[2].to_string())
        .collect();

    for allowed in &allowed_names {
        raw_names.remove(*allowed);
    }
    for val in &ts_const_values {
        raw_names.remove(val.as_str());
    }

    if !raw_names.is_empty() {
        eprintln!("Unexpected raw TS driver names: {raw_names:?}");
        bail!(
            "browser-driver-contract-sync: TS driver contains raw __AURA_DRIVER_* literals outside the sanctioned contract set"
        );
    }

    // Verify TS driver doesn't hand-build queue payload JSON
    if driver_src
        .contains(r#"JSON.stringify({ command_id: commandId, request_json: requestJson, })"#)
    {
        bail!(
            "browser-driver-contract-sync: TS driver still hand-builds semantic queue payload JSON"
        );
    }
    if driver_src.contains(
        r#"JSON.stringify({ command_id: commandId, runtime_identity_json: runtimeIdentityJson, })"#,
    ) {
        bail!(
            "browser-driver-contract-sync: TS driver still hand-builds runtime-stage queue payload JSON"
        );
    }

    println!("browser-driver-contract-sync: clean");
    Ok(())
}

pub fn run_browser_toolchain() -> Result<()> {
    ensure_browser_toolchain()?;
    println!("harness browser toolchain: clean");
    Ok(())
}

pub fn run_browser_install() -> Result<()> {
    let driver_dir = browser_driver_dir()?;
    let driver_script = driver_dir.join("playwright_driver.mjs");
    if !driver_script.exists() {
        bail!(
            "harness-browser-install: missing Playwright driver script: {}",
            repo_relative(&driver_script)
        );
    }

    ensure_browser_toolchain()?;
    run_ok_in_dir(
        "node",
        &[
            "-e".into(),
            "const { chromium } = require('playwright'); const p = chromium.executablePath(); if (!p) process.exit(2); process.stdout.write(p);".into(),
        ],
        &driver_dir,
    )
    .context(format!(
        "harness-browser-install: Playwright chromium is unavailable; run npm ci and npm run install-browsers in {}",
        repo_relative(&driver_dir)
    ))?;

    println!("harness browser install: clean");
    Ok(())
}

pub fn run_browser_driver_types() -> Result<()> {
    let driver_dir = browser_driver_dir()?;
    ensure_browser_toolchain()?;
    run_ok_in_dir("npm", &["run".into(), "typecheck".into()], &driver_dir)?;

    let driver_src = driver_dir.join("src/playwright_driver.ts");
    let wrapper = driver_dir.join("playwright_driver.mjs");
    if !contains(&driver_src, "./contracts.js")? {
        bail!("harness-browser-driver-types: driver does not import typed contracts");
    }
    if !contains(&driver_src, "./method_sets.js")? {
        bail!("harness-browser-driver-types: driver does not import typed method sets");
    }
    if !contains(&wrapper, "./driver_loader.mjs")? {
        bail!(
            "harness-browser-driver-types: stable wrapper does not delegate to the driver loader"
        );
    }
    if !contains(&wrapper, "ensureCompiledDriverFresh")?
        || !contains(&wrapper, "pathToFileURL(compiledDriver).href")?
    {
        bail!(
            "harness-browser-driver-types: stable wrapper does not load the compiled TS driver through the freshness loader"
        );
    }

    println!("harness-browser-driver-types: clean");
    Ok(())
}

pub fn run_browser_observation_contract() -> Result<()> {
    let driver_dir = browser_driver_dir()?;
    ensure_browser_toolchain()?;
    run_ok_in_dir(
        "node",
        &["./playwright_driver.mjs".into(), "--selftest".into()],
        &driver_dir,
    )?;
    println!("harness-browser-observation-contract: clean");
    Ok(())
}

pub fn run_browser_observation_recovery() -> Result<()> {
    let repo_root = repo_root()?;
    let driver = repo_root.join("crates/aura-harness/playwright-driver/src/playwright_driver.ts");
    let method_sets = repo_root.join("crates/aura-harness/playwright-driver/src/method_sets.ts");
    let observation_module =
        repo_root.join("crates/aura-harness/playwright-driver/src/observation.ts");

    // Check ui_state function body doesn't do implicit recovery
    let driver_src = read(&driver)?;
    // Extract the uiState function body
    let ui_state_body: String = {
        let mut in_fn = false;
        let mut depth = 0i32;
        let mut lines = Vec::new();
        for line in driver_src.lines() {
            if line.starts_with("async function uiState(params)") {
                in_fn = true;
            }
            if in_fn {
                depth += line.chars().filter(|&c| c == '{').count() as i32;
                depth -= line.chars().filter(|&c| c == '}').count() as i32;
                lines.push(line);
                if depth <= 0 && !lines.is_empty() {
                    break;
                }
            }
        }
        lines.join("\n")
    };

    if ui_state_body.contains("readStructuredUiStateWithNavigationRecovery")
        || ui_state_body.contains("resetObservationState(")
    {
        bail!(
            "harness browser observation recovery: ui_state may not perform implicit browser recovery"
        );
    }

    if !contains(
        &method_sets,
        "export const RECOVERY_METHODS: ReadonlySet<DriverMethod> = new Set",
    )? {
        bail!(
            "harness browser observation recovery: driver must declare explicit recovery methods"
        );
    }

    if !contains(&driver, "case \"recover_ui_state\"")? {
        bail!("harness browser observation recovery: driver must expose explicit recover_ui_state");
    }

    if observation_module.exists() {
        let obs_src = read(&observation_module)?;
        if obs_src.contains("recover") || obs_src.contains("retry") || obs_src.contains("fallback")
        {
            bail!(
                "harness browser observation recovery: browser observation module must stay passive and recovery-free"
            );
        }
    }

    for forbidden in [
        "click_button js_fallback_",
        "click_button css fallback_key",
        "click_button nav_label_first",
        "fill_input fallback_done",
        "locator_click_force:",
        "key_press_dom_fallback_",
        "selectorToFallbackLabel",
    ] {
        if driver_src.contains(forbidden) {
            bail!(
                "harness browser observation recovery: legacy implicit browser fallback remains in driver: {forbidden}"
            );
        }
    }

    run_browser_toolchain()?;

    let driver_dir = browser_driver_dir()?;
    run_ok_in_dir(
        "node",
        &["./playwright_driver.mjs".into(), "--selftest".into()],
        &driver_dir,
    )?;

    println!("harness browser observation recovery: clean");
    Ok(())
}

pub fn run_harness_action_preconditions() -> Result<()> {
    let repo_root = repo_root()?;
    let executor = repo_root.join("crates/aura-harness/src/executor.rs");
    let scenario_contract = repo_root.join("crates/aura-app/src/scenario_contract.rs");
    let scenario_contract_dir = repo_root.join("crates/aura-app/src/scenario_contract");

    let mut sc_paths = vec![scenario_contract.to_string_lossy().into_owned()];
    if scenario_contract_dir.exists() {
        sc_paths.push(scenario_contract_dir.to_string_lossy().into_owned());
    }

    let mut args = vec!["ActionPrecondition::Quiescence".into()];
    args.extend(sc_paths);
    if !rg_exists(&args)? {
        bail!(
            "harness-action-preconditions: shared action contracts must declare quiescence preconditions"
        );
    }

    if !rg_exists(&[
        "fn enforce_action_preconditions".into(),
        executor.to_string_lossy().into_owned(),
    ])? {
        bail!(
            "harness-action-preconditions: executor is missing typed action precondition enforcement"
        );
    }
    if !rg_exists(&[
        "-F".into(),
        "enforce_action_preconditions(step, tool_api, context, &intent".into(),
        executor.to_string_lossy().into_owned(),
    ])? {
        bail!(
            "harness-action-preconditions: shared action execution does not enforce preconditions before issue"
        );
    }
    if !rg_exists(&[
        "fn wait_for_contract_barriers".into(),
        executor.to_string_lossy().into_owned(),
    ])? {
        bail!(
            "harness-action-preconditions: executor is missing typed post-operation convergence enforcement"
        );
    }
    if !rg_exists(&[
        "-F".into(),
        "wait_for_contract_barriers(".into(),
        executor.to_string_lossy().into_owned(),
    ])? {
        bail!(
            "harness-action-preconditions: shared action execution does not enforce post-operation convergence before the next intent"
        );
    }

    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "aura-harness".into(),
            "action_preconditions_fail_diagnostically_before_issue".into(),
            "--quiet".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "aura-harness".into(),
            "missing_sync_prerequisites_fail_as_convergence_contract_violations".into(),
            "--quiet".into(),
        ],
    )?;

    println!("harness action preconditions: clean");
    Ok(())
}

pub fn run_harness_backend_contract() -> Result<()> {
    let repo_root = repo_root()?;
    let backend_contract = repo_root.join("crates/aura-harness/src/backend/mod.rs");

    let backend_src = read(&backend_contract)?;

    // Extract InstanceBackend trait body
    let trait_body = {
        let marker = "pub trait InstanceBackend {";
        let start = backend_src.find(marker).ok_or_else(|| {
            anyhow::anyhow!(
                "harness-backend-contract: could not extract InstanceBackend trait body"
            )
        })?;
        let rest = &backend_src[start + marker.len()..];
        let mut depth = 1i32;
        let mut end = 0;
        for (i, c) in rest.char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i;
                        break;
                    }
                }
                _ => {}
            }
        }
        rest[..end].to_string()
    };

    for forbidden in [
        "fn click_button",
        "fn activate_control",
        "fn click_target",
        "fn fill_input",
        "fn fill_field",
        "fn activate_list_item",
        "fn submit_create_account",
        "fn submit_create_home",
        "fn submit_create_contact_invitation",
        "fn submit_accept_contact_invitation",
        "fn submit_invite_actor_to_channel",
        "fn submit_accept_pending_channel_invitation",
        "fn submit_join_channel",
        "fn submit_send_chat_message",
    ] {
        if trait_body.contains(forbidden) {
            bail!(
                "harness-backend-contract: InstanceBackend still carries forbidden surface: {forbidden}"
            );
        }
    }

    for required in [
        "pub trait ObservationBackend",
        "pub trait RawUiBackend",
        "pub trait SharedSemanticBackend",
    ] {
        if !backend_src.contains(required) {
            bail!("harness-backend-contract: missing backend contract surface: {required}");
        }
    }

    if backend_src.contains("impl<T: InstanceBackend + ?Sized> SharedSemanticBackend for T") {
        bail!(
            "harness-backend-contract: blanket SharedSemanticBackend impl keeps fallback-heavy semantic execution alive"
        );
    }

    let local_pty = repo_root.join("crates/aura-harness/src/backend/local_pty.rs");
    if !contains(&local_pty, "impl SharedSemanticBackend for LocalPtyBackend")? {
        bail!(
            "harness-backend-contract: local PTY backend must explicitly implement SharedSemanticBackend"
        );
    }

    let playwright = repo_root.join("crates/aura-harness/src/backend/playwright_browser.rs");
    if !contains(
        &playwright,
        "impl SharedSemanticBackend for PlaywrightBrowserBackend",
    )? {
        bail!(
            "harness-backend-contract: Playwright backend must explicitly implement SharedSemanticBackend"
        );
    }

    println!("harness backend contract: clean");
    Ok(())
}

pub fn run_harness_boundary_policy() -> Result<()> {
    let repo_root = repo_root()?;

    // Check that scenario contracts don't contain frontend mechanics
    let sc_files = {
        let sc = repo_root.join("crates/aura-app/src/scenario_contract.rs");
        let sc_dir = repo_root.join("crates/aura-app/src/scenario_contract");
        let mut paths = vec![sc.to_string_lossy().into_owned()];
        if sc_dir.exists() {
            paths.push(sc_dir.to_string_lossy().into_owned());
        }
        paths
    };

    let forbidden_contract =
        r"send_keys|send_key|click_button|click_target|fill_input|selector|dom_snapshot";
    let mut contract_args: Vec<String> = vec!["-n".into(), forbidden_contract.into()];
    contract_args.extend(sc_files.clone());
    let contract_hits = rg_lines(&contract_args)?;
    let contract_real_hits: Vec<_> = contract_hits
        .into_iter()
        .filter(|l| {
            let trimmed = l.trim_start();
            !trimmed.starts_with("//!") && !trimmed.starts_with("//") && !trimmed.starts_with('*')
        })
        .collect();
    if !contract_real_hits.is_empty() {
        for hit in &contract_real_hits {
            eprintln!("{hit}");
        }
        bail!(
            "harness-boundary-policy: semantic scenario contract contains frontend-specific mechanics"
        );
    }

    // Check that Quint/verification code doesn't reference frontend mechanics
    let forbidden_quint =
        r"aura_terminal|aura_ui|playwright|ToolRequest|send_keys|send_key|click_button|fill_input";
    let quint_hits = rg_lines(&[
        "-n".into(),
        forbidden_quint.into(),
        repo_root
            .join("crates/aura-quint")
            .to_string_lossy()
            .into_owned(),
        repo_root
            .join("verification/quint")
            .to_string_lossy()
            .into_owned(),
    ])?;
    let quint_real_hits: Vec<_> = quint_hits
        .into_iter()
        .filter(|l| {
            let trimmed = l.trim_start();
            !trimmed.starts_with("//!") && !trimmed.starts_with("//") && !trimmed.starts_with('*')
        })
        .collect();
    if !quint_real_hits.is_empty() {
        for hit in &quint_real_hits {
            eprintln!("{hit}");
        }
        bail!(
            "harness-boundary-policy: Quint or verification code references frontend-driving mechanics"
        );
    }

    // Check referenced scenarios are present and not legacy
    let active_ref_roots = [
        repo_root.join("justfile"),
        repo_root.join("scripts/ci"),
        repo_root.join("crates/aura-harness"),
        repo_root.join("docs/804_testing_guide.md"),
    ];
    let scenario_re = Regex::new(r"scenarios/harness/[A-Za-z0-9._/-]+\.toml")?;
    let mut referenced: BTreeSet<String> = BTreeSet::new();
    for root in &active_ref_roots {
        if !root.exists() {
            continue;
        }
        let hits = command_stdout(
            "rg",
            &[
                "-o".into(),
                "--no-filename".into(),
                r"scenarios/harness/[A-Za-z0-9._/-]+\.toml".into(),
                root.to_string_lossy().into_owned(),
            ],
        )
        .unwrap_or_default();
        for line in hits.lines().filter(|l| !l.is_empty()) {
            referenced.insert(line.trim().to_string());
        }
    }
    let legacy_keys = ["schema_version", "execution_mode", "required_capabilities"];
    for scenario in &referenced {
        let path = repo_root.join(scenario);
        if !path.exists() {
            bail!(
                "harness-boundary-policy: active entry point references missing scenario: {scenario}"
            );
        }
        let src = read(&path)?;
        for key in &legacy_keys {
            let pat = format!("{key} =");
            if src.contains(&pat) {
                bail!(
                    "harness-boundary-policy: active entry point references legacy harness scenario: {scenario}"
                );
            }
        }
    }

    // Check semantic scenarios for raw selectors / legacy actions
    let scenario_dir = repo_root.join("scenarios/harness");
    if scenario_dir.exists() {
        for entry in fs::read_dir(&scenario_dir)
            .with_context(|| format!("reading {}", scenario_dir.display()))?
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("toml") {
                continue;
            }
            let src = read(&path)?;
            let is_legacy = legacy_keys.iter().any(|k| src.contains(&format!("{k} =")));
            if is_legacy {
                continue;
            }
            if src.contains("selector =") {
                bail!(
                    "harness-boundary-policy: semantic scenario contains raw selector reference: {}",
                    repo_relative(&path)
                );
            }
            for legacy_action in [
                r#"action = "wait_for""#,
                r#"action = "click_button""#,
                r#"action = "fill_input""#,
                r#"action = "send_keys""#,
                r#"action = "send_key""#,
            ] {
                if src.contains(legacy_action) {
                    bail!(
                        "harness-boundary-policy: semantic scenario contains legacy frontend action: {}",
                        repo_relative(&path)
                    );
                }
            }
        }
    }

    let _ = scenario_re; // used above via command_stdout rg
    println!("harness boundary policy: clean");
    Ok(())
}

pub fn run_harness_bridge_contract() -> Result<()> {
    let repo_root = repo_root()?;
    let ui_contract_file = repo_root.join("crates/aura-app/src/ui_contract.rs");
    let ui_contract_dir = repo_root.join("crates/aura-app/src/ui_contract");

    let mut ui_paths = vec![ui_contract_file.to_string_lossy().into_owned()];
    if ui_contract_dir.exists() {
        ui_paths.push(ui_contract_dir.to_string_lossy().into_owned());
    }

    for (const_name, label) in [
        (
            "pub const BROWSER_HARNESS_BRIDGE_API_VERSION",
            "missing browser harness bridge API version",
        ),
        (
            "pub const BROWSER_HARNESS_BRIDGE_METHODS",
            "missing browser harness bridge method metadata",
        ),
        (
            "pub const BROWSER_OBSERVATION_SURFACE_API_VERSION",
            "missing browser observation surface API version",
        ),
        (
            "pub const BROWSER_OBSERVATION_SURFACE_METHODS",
            "missing browser observation surface method metadata",
        ),
        (
            "pub struct HarnessShellStructureSnapshot",
            "missing HarnessShellStructureSnapshot contract",
        ),
        (
            "pub fn validate_harness_shell_structure",
            "missing harness shell structure validator",
        ),
    ] {
        let mut args = vec![const_name.into()];
        args.extend(ui_paths.clone());
        if !rg_exists(&args)? {
            bail!("harness-bridge-contract: {label}");
        }
    }

    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "hxrts-aura-app".into(),
            "browser_harness_bridge_contract_is_versioned_and_complete".into(),
            "--quiet".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "hxrts-aura-app".into(),
            "browser_harness_bridge_read_methods_are_declared_deterministic".into(),
            "--quiet".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "hxrts-aura-app".into(),
            "browser_observation_surface_contract_is_versioned_and_read_only".into(),
            "--quiet".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "hxrts-aura-app".into(),
            "tui_observation_surface_contract_is_versioned_and_read_only".into(),
            "--quiet".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "hxrts-aura-app".into(),
            "harness_shell_structure_accepts_exactly_one_app_shell".into(),
            "--quiet".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "hxrts-aura-app".into(),
            "harness_shell_structure_accepts_single_onboarding_shell".into(),
            "--quiet".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "hxrts-aura-app".into(),
            "harness_shell_structure_rejects_duplicate_or_ambiguous_roots".into(),
            "--quiet".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "aura-harness".into(),
            "playwright_semantic_bridge_failure_and_projection_contracts_are_explicit".into(),
            "--quiet".into(),
        ],
    )?;

    println!("harness bridge contract: clean");
    Ok(())
}

pub fn run_harness_command_plane_boundary() -> Result<()> {
    let repo_root = repo_root()?;

    let allowed_rust_files: BTreeSet<&str> = [
        "crates/aura-harness/src/backend/mod.rs",
        "crates/aura-harness/src/backend/local_pty.rs",
        "crates/aura-harness/src/backend/playwright_browser.rs",
        "crates/aura-harness/src/tool_api.rs",
        "crates/aura-harness/src/coordinator.rs",
        "crates/aura-harness/src/executor.rs",
        "crates/aura-web/src/harness_bridge.rs",
        "crates/aura-web/src/harness/commands.rs",
    ]
    .into();

    let allowed_ts_files: BTreeSet<&str> = [
        "crates/aura-harness/playwright-driver/src/playwright_driver.ts",
        "crates/aura-harness/playwright-driver/src/contracts.ts",
        "crates/aura-harness/playwright-driver/src/method_sets.ts",
    ]
    .into();

    // Check Rust files for unexpected semantic command handling surfaces.
    let rust_hits: Vec<String> = {
        let output = command_stdout(
            "rg",
            &[
                "-l".into(),
                r"fn submit_semantic_command\(|submit_semantic_command_via_ui\(".into(),
                repo_root
                    .join("crates/aura-harness")
                    .to_string_lossy()
                    .into_owned(),
                repo_root
                    .join("crates/aura-web")
                    .to_string_lossy()
                    .into_owned(),
            ],
        )
        .unwrap_or_default();
        output
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| {
                let p = std::path::Path::new(l);
                p.strip_prefix(&repo_root)
                    .map(|r| r.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| l.to_string())
            })
            .collect()
    };
    for file in &rust_hits {
        if !allowed_rust_files.contains(file.as_str()) {
            bail!(
                "harness-command-plane-boundary: unexpected semantic command handling surface in Rust module: {file}"
            );
        }
    }

    // Check TypeScript files for unexpected semantic command handling surfaces.
    let ts_hits: Vec<String> = {
        let output = command_stdout(
            "rg",
            &[
                "-l".into(),
                "submit_semantic_command".into(),
                repo_root
                    .join("crates/aura-harness/playwright-driver")
                    .to_string_lossy()
                    .into_owned(),
            ],
        )
        .unwrap_or_default();
        output
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| {
                let p = std::path::Path::new(l);
                p.strip_prefix(&repo_root)
                    .map(|r| r.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| l.to_string())
            })
            .collect()
    };
    for file in &ts_hits {
        if !allowed_ts_files.contains(file.as_str()) {
            bail!(
                "harness-command-plane-boundary: unexpected semantic command handling surface in Playwright driver: {file}"
            );
        }
    }

    // Verify executor uses the shared-intent path through ToolApi.
    let executor_path = repo_root.join("crates/aura-harness/src/executor.rs");
    if executor_path.exists() {
        let executor_src = std::fs::read_to_string(&executor_path)?;
        if !executor_src.contains(
            "tool_api.submit_semantic_command(instance_id, SemanticCommandRequest::new(intent))",
        ) {
            bail!(
                "harness-command-plane-boundary: executor must submit shared intents only through ToolApi::submit_semantic_command"
            );
        }
    }

    // Verify per-intent wrappers have not re-appeared in tool_api.rs.
    let per_intent_patterns = [
        "submit_create_account(",
        "submit_create_home(",
        "submit_create_contact_invitation(",
        "submit_accept_contact_invitation(",
        "submit_invite_actor_to_channel(",
        "submit_accept_pending_channel_invitation(",
        "submit_join_channel(",
        "submit_send_chat_message(",
    ];
    let tool_api_path = repo_root.join("crates/aura-harness/src/tool_api.rs");
    if tool_api_path.exists() {
        let tool_api_src = std::fs::read_to_string(&tool_api_path)?;
        for pat in &per_intent_patterns {
            if tool_api_src.contains(pat) {
                bail!(
                    "harness-command-plane-boundary: per-intent semantic command wrappers must not reappear in ToolApi (found: {pat})"
                );
            }
        }
    }

    // Verify per-intent wrappers have not re-appeared in coordinator.rs.
    let per_intent_ui_patterns = [
        "create_account_via_ui(",
        "create_home_via_ui(",
        "create_contact_invitation_via_ui(",
        "accept_contact_invitation_via_ui(",
        "invite_actor_to_channel_via_ui(",
        "accept_pending_channel_invitation_via_ui(",
        "join_channel_via_ui(",
        "send_chat_message_via_ui(",
    ];
    let coordinator_path = repo_root.join("crates/aura-harness/src/coordinator.rs");
    if coordinator_path.exists() {
        let coordinator_src = std::fs::read_to_string(&coordinator_path)?;
        for pat in &per_intent_ui_patterns {
            if coordinator_src.contains(pat) {
                bail!(
                    "harness-command-plane-boundary: per-intent semantic command wrappers must not reappear in HarnessCoordinator (found: {pat})"
                );
            }
        }
    }

    println!("harness command-plane boundary: clean");
    Ok(())
}

pub fn run_harness_conformance_gate() -> Result<()> {
    let repo_root = repo_root()?;
    let workflow = repo_root.join(".github/workflows/conform.yml");
    let legacy_workflow = repo_root.join(".github/workflows/ci.yml");

    let check_triggers = |path: &std::path::Path| -> Result<()> {
        let src = read(path)?;
        if !src.contains("pull_request:") || !src.contains("branches: [main, develop]") {
            bail!(
                "harness-conformance-gate: Conformance gate must run on pull_request for protected branches. Ensure trigger includes 'pull_request' with '[main, develop]' in {}",
                repo_relative(path)
            );
        }
        Ok(())
    };

    if workflow.exists() {
        let src = read(&workflow)?;
        if !src.contains("  conformance:") {
            bail!(
                "harness-conformance-gate: Missing 'conformance' job in {}. Add job 'conformance' that runs 'nix develop --command just ci-conformance'.",
                repo_relative(&workflow)
            );
        }
        if !src.contains("just ci-conformance-policy") {
            bail!(
                "harness-conformance-gate: Conformance workflow must execute 'just ci-conformance-policy'."
            );
        }
        if !src.contains("just ci-conformance") {
            bail!(
                "harness-conformance-gate: Conformance workflow must execute 'just ci-conformance'."
            );
        }
        check_triggers(&workflow)?;
        if !src.contains("upload-artifact@v4") || !src.contains("artifacts/conformance") {
            bail!(
                "harness-conformance-gate: Conformance workflow must upload conformance traces/diffs as artifacts. Add actions/upload-artifact@v4 step for artifacts/conformance."
            );
        }
        println!(
            "[conformance-gate] OK: conformance gate wiring is present in {}",
            repo_relative(&workflow)
        );
        return Ok(());
    }

    if !legacy_workflow.exists() {
        bail!(
            "harness-conformance-gate: Missing {} (or legacy {}). Add a protected-branch conformance workflow that runs 'just ci-conformance'.",
            repo_relative(&workflow),
            repo_relative(&legacy_workflow)
        );
    }

    let legacy_src = read(&legacy_workflow)?;
    if !legacy_src.contains("  conformance-gate:") {
        bail!(
            "harness-conformance-gate: Missing 'conformance-gate' job in {}. Add job 'conformance-gate' that runs 'nix develop --command just ci-conformance'.",
            repo_relative(&legacy_workflow)
        );
    }
    if !legacy_src.contains("just ci-conformance") {
        bail!("harness-conformance-gate: Conformance gate job must execute 'just ci-conformance'.");
    }
    check_triggers(&legacy_workflow)?;
    if !legacy_src.contains("upload-artifact@v4") || !legacy_src.contains("artifacts/conformance") {
        bail!(
            "harness-conformance-gate: Conformance gate must upload conformance traces/diffs as artifacts. Add actions/upload-artifact@v4 step for artifacts/conformance."
        );
    }

    println!(
        "[conformance-gate] OK: conformance gate wiring is present in {}",
        repo_relative(&legacy_workflow)
    );
    Ok(())
}

pub fn run_harness_core_scenario_mechanics() -> Result<()> {
    run_harness_governance_check("core-scenario-mechanics")
}

pub fn run_harness_export_override_policy() -> Result<()> {
    let repo_root = repo_root()?;

    let hits = command_stdout(
        "rg",
        &[
            "--no-heading".into(),
            "-n".into(),
            "publish_.*override".into(),
            repo_root
                .join("crates/aura-terminal/src/tui")
                .to_string_lossy()
                .into_owned(),
            repo_root
                .join("crates/aura-ui/src")
                .to_string_lossy()
                .into_owned(),
            repo_root
                .join("crates/aura-web/src")
                .to_string_lossy()
                .into_owned(),
        ],
    )
    .unwrap_or_default();

    let filtered: Vec<&str> = hits
        .lines()
        .filter(|l| !l.is_empty())
        .filter(|l| !l.contains("crates/aura-terminal/src/tui/harness_state/"))
        .collect();

    if !filtered.is_empty() {
        for hit in &filtered {
            eprintln!("{hit}");
        }
        bail!(
            "harness-export-override-policy: new parity-critical export helpers may not depend on override caches outside the quarantined TUI harness export module"
        );
    }

    println!("harness export override policy: clean");
    Ok(())
}

pub fn run_harness_focus_selection_contract() -> Result<()> {
    let repo_root = repo_root()?;
    let sc = repo_root.join("crates/aura-app/src/scenario_contract.rs");
    let sc_dir = repo_root.join("crates/aura-app/src/scenario_contract");
    let mut sc_paths = vec![sc.to_string_lossy().into_owned()];
    if sc_dir.exists() {
        sc_paths.push(sc_dir.to_string_lossy().into_owned());
    }

    for (pattern, label) in [
        (
            "pub enum FocusSemantics",
            "missing focus semantics contract",
        ),
        (
            "pub enum SelectionSemantics",
            "missing selection semantics contract",
        ),
        (
            "pub struct SharedActionContract",
            "missing shared action contract",
        ),
    ] {
        let mut args = vec![pattern.into()];
        args.extend(sc_paths.clone());
        if !rg_exists(&args)? {
            bail!("harness-focus-selection-contract: {label}");
        }
    }

    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "hxrts-aura-app".into(),
            "every_intent_kind_declares_focus_and_selection_semantics".into(),
            "--quiet".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "hxrts-aura-app".into(),
            "ui_snapshot_parity_detects_focus_semantic_drift".into(),
            "--quiet".into(),
        ],
    )?;

    println!("harness focus selection contract: clean");
    Ok(())
}

pub fn run_harness_matrix_inventory() -> Result<()> {
    let repo_root = repo_root()?;
    let inventory = repo_root.join("scenarios/harness_inventory.toml");
    let matrix_runner = repo_root.join("scripts/harness/run-matrix.sh");

    if !inventory.exists() {
        bail!("harness-matrix-inventory: missing inventory: scenarios/harness_inventory.toml");
    }
    if !matrix_runner.exists() {
        bail!("harness-matrix-inventory: missing matrix runner: scripts/harness/run-matrix.sh");
    }

    // Parse inventory to collect expected scenario ids per lane
    let inv_src = read(&inventory)?;
    let mut expected_tui: BTreeSet<String> = BTreeSet::new();
    let mut expected_web: BTreeSet<String> = BTreeSet::new();
    let mut current_id = String::new();
    let mut current_class = String::new();
    for line in inv_src.lines() {
        let line = line.trim();
        if line == "[[scenario]]" {
            if !current_id.is_empty() {
                match current_class.as_str() {
                    "shared" | "tui_conformance" => {
                        expected_tui.insert(current_id.clone());
                    }
                    _ => {}
                }
                match current_class.as_str() {
                    "shared" | "web_conformance" => {
                        expected_web.insert(current_id.clone());
                    }
                    _ => {}
                }
                current_id.clear();
                current_class.clear();
            }
        } else if let Some(rest) = line.strip_prefix("id = \"") {
            if let Some(id) = rest.strip_suffix('"') {
                current_id = id.to_string();
            }
        } else if let Some(rest) = line.strip_prefix("classification = \"") {
            if let Some(class) = rest.strip_suffix('"') {
                current_class = class.to_string();
            }
        }
    }
    // flush last entry
    if !current_id.is_empty() {
        match current_class.as_str() {
            "shared" | "tui_conformance" => {
                expected_tui.insert(current_id.clone());
            }
            _ => {}
        }
        match current_class.as_str() {
            "shared" | "web_conformance" => {
                expected_web.insert(current_id.clone());
            }
            _ => {}
        }
    }

    // Collect actual scenario ids from matrix runner dry-run
    let collect_actual = |lane: &str| -> Result<BTreeSet<String>> {
        let out = command_stdout(
            "bash",
            &[
                matrix_runner.to_string_lossy().into_owned(),
                "--lane".into(),
                lane.into(),
                "--dry-run".into(),
            ],
        )
        .unwrap_or_default();
        let id_re = Regex::new(r"scenario=([A-Za-z0-9._-]+)")?;
        Ok(id_re
            .captures_iter(&out)
            .map(|c| c[1].to_string())
            .collect())
    };

    let actual_tui = collect_actual("tui")?;
    let actual_web = collect_actual("web")?;

    if expected_tui != actual_tui {
        eprintln!("TUI lane mismatch:");
        eprintln!("  expected: {expected_tui:?}");
        eprintln!("  actual:   {actual_tui:?}");
        bail!("harness-matrix-inventory: lane tui does not match inventory-derived scenario set");
    }
    if expected_web != actual_web {
        eprintln!("Web lane mismatch:");
        eprintln!("  expected: {expected_web:?}");
        eprintln!("  actual:   {actual_web:?}");
        bail!("harness-matrix-inventory: lane web does not match inventory-derived scenario set");
    }

    println!("harness matrix inventory: clean");
    Ok(())
}

pub fn run_harness_mode_allowlist() -> Result<()> {
    let repo_root = repo_root()?;
    let ui_contract_file = repo_root.join("crates/aura-app/src/ui_contract.rs");
    let ui_contract_dir = repo_root.join("crates/aura-app/src/ui_contract");

    let mut ui_paths = vec![ui_contract_file.to_string_lossy().into_owned()];
    if ui_contract_dir.exists() {
        ui_paths.push(ui_contract_dir.to_string_lossy().into_owned());
    }

    {
        let mut args = vec!["pub const HARNESS_MODE_ALLOWLIST".into()];
        args.extend(ui_paths.clone());
        if !rg_exists(&args)? {
            bail!("harness-mode-allowlist: missing harness-mode allowlist metadata");
        }
    }
    {
        let mut args = vec!["enum HarnessModeChangeKind".into()];
        args.extend(ui_paths);
        if !rg_exists(&args)? {
            bail!("harness-mode-allowlist: missing harness-mode change kind metadata");
        }
    }

    // Check frontend modules don't branch on AURA_HARNESS_MODE
    let frontend_hits = command_stdout(
        "rg",
        &[
            "-l".into(),
            "AURA_HARNESS_MODE".into(),
            repo_root
                .join("crates/aura-terminal/src")
                .to_string_lossy()
                .into_owned(),
            repo_root
                .join("crates/aura-web/src")
                .to_string_lossy()
                .into_owned(),
        ],
    )
    .unwrap_or_default();

    let filtered: Vec<&str> = frontend_hits
        .lines()
        .filter(|l| !l.is_empty())
        .filter(|l| {
            !l.ends_with("crates/aura-terminal/src/tui/screens/app/shell/events.rs")
                && !l.ends_with("crates/aura-web/src/shell/maintenance.rs")
        })
        .collect();

    if !filtered.is_empty() {
        bail!(
            "harness-mode-allowlist: frontend product modules must not branch on AURA_HARNESS_MODE: {:?}",
            filtered
        );
    }

    let web_main = repo_root.join("crates/aura-web/src/main.rs");
    if web_main.exists() && contains(&web_main, "reset_harness_bootstrap_storage_once")? {
        bail!(
            "harness-mode-allowlist: web frontend may not carry harness-only bootstrap reset shortcuts"
        );
    }

    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "hxrts-aura-app".into(),
            "harness_mode_allowlist_is_scoped_to_non_semantic_categories".into(),
            "--quiet".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "hxrts-aura-app".into(),
            "connectivity_check_is_harness_mode_neutral".into(),
            "--quiet".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "aura-terminal".into(),
            "invitation_dispatch_uses_product_callbacks_without_harness_shortcuts".into(),
            "--quiet".into(),
        ],
    )?;

    println!("harness mode allowlist: clean");
    Ok(())
}

pub fn run_harness_observation_determinism() -> Result<()> {
    let repo_root = repo_root()?;

    let rust_observation_files = [
        repo_root.join("crates/aura-terminal/src/tui/harness_state/snapshot.rs"),
        repo_root.join("crates/aura-ui/src/model/mod.rs"),
        repo_root.join("crates/aura-web/src/harness_bridge.rs"),
    ];

    let nondeterminism_pattern = r"SystemTime::now|Instant::now|std::time::SystemTime|std::time::Instant|chrono::Utc::now|chrono::Local::now|thread_rng\(\)|rand::thread_rng|rand::random|getrandom::|OsRng|Uuid::new_v4";

    let mut rust_args = vec![
        "--no-heading".into(),
        "-n".into(),
        nondeterminism_pattern.into(),
    ];
    for f in &rust_observation_files {
        if f.exists() {
            rust_args.push(f.to_string_lossy().into_owned());
        }
    }

    let rust_hits = command_stdout("rg", &rust_args).unwrap_or_default();
    if !rust_hits.trim().is_empty() {
        eprintln!("{rust_hits}");
        bail!(
            "harness-observation-determinism: parity-critical observation paths may not read wall clock time, unseeded randomness, or nondeterministic ids"
        );
    }

    let driver_mjs = repo_root.join("crates/aura-harness/playwright-driver/playwright_driver.mjs");
    if driver_mjs.exists() {
        let js_hits = command_stdout(
            "rg",
            &[
                "--no-heading".into(),
                "-n".into(),
                r"Math\.random|randomUUID\(".into(),
                driver_mjs.to_string_lossy().into_owned(),
            ],
        )
        .unwrap_or_default();
        if !js_hits.trim().is_empty() {
            eprintln!("{js_hits}");
            bail!(
                "harness-observation-determinism: browser observation path may not use JS randomness in parity-critical observation code"
            );
        }
    }

    println!("harness observation determinism: clean");
    Ok(())
}

pub fn run_harness_observation_surface() -> Result<()> {
    let repo_root = repo_root()?;
    let backend_mod = repo_root.join("crates/aura-harness/src/backend/mod.rs");

    if !contains(&backend_mod, "pub trait ObservationBackend")? {
        bail!("harness-observation-surface: missing ObservationBackend trait");
    }

    // Extract ObservationBackend trait body and check for action-like methods
    let backend_src = read(&backend_mod)?;
    let trait_body = {
        let marker = "pub trait ObservationBackend";
        if let Some(start) = backend_src.find(marker) {
            let rest = &backend_src[start + marker.len()..];
            let mut depth = 0i32;
            let mut end = 0;
            for (i, c) in rest.char_indices() {
                match c {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            end = i;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            rest[..end].to_string()
        } else {
            String::new()
        }
    };

    let action_method_re = Regex::new(
        r"fn (send_|click_|fill_|create_|accept_|invite_|join_|inject_|restart|start|stop)",
    )?;
    if action_method_re.is_match(&trait_body) {
        bail!("harness-observation-surface: ObservationBackend exports action-like methods");
    }

    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "hxrts-aura-app".into(),
            "observation_surface_methods_do_not_overlap_action_surface".into(),
            "--quiet".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "aura-harness".into(),
            "observation_endpoints_are_side_effect_free".into(),
            "--quiet".into(),
        ],
    )?;

    println!("harness observation surface: clean");
    Ok(())
}

pub fn run_harness_onboarding_contract() -> Result<()> {
    let repo_root = repo_root()?;
    let ui_contract_file = repo_root.join("crates/aura-app/src/ui_contract.rs");
    let ui_contract_dir = repo_root.join("crates/aura-app/src/ui_contract");

    let mut ui_paths = vec![ui_contract_file.to_string_lossy().into_owned()];
    if ui_contract_dir.exists() {
        ui_paths.push(ui_contract_dir.to_string_lossy().into_owned());
    }

    {
        let mut args = vec!["ScreenId::Onboarding".into()];
        args.extend(ui_paths);
        if !rg_exists(&args)? {
            bail!(
                "harness-onboarding-contract: onboarding must be declared in the shared snapshot model"
            );
        }
    }

    let web_main = repo_root.join("crates/aura-web/src/main.rs");
    if web_main.exists() && !contains(&web_main, "controller.set_account_setup_state(")? {
        bail!(
            "harness-onboarding-contract: web onboarding must publish through the canonical controller snapshot pipeline"
        );
    }

    for (dir, label) in [
        (repo_root.join("crates/aura-web/src"), "aura-web"),
        (repo_root.join("crates/aura-harness/src"), "aura-harness"),
    ] {
        if !dir.exists() {
            continue;
        }
        let hits = command_stdout(
            "rg",
            &[
                "--no-heading".into(),
                "-n".into(),
                r"publish_onboarding_snapshot|stale_onboarding_publish|synthetic_onboarding_snapshot".into(),
                dir.to_string_lossy().into_owned(),
            ],
        )
        .unwrap_or_default();
        if !hits.trim().is_empty() {
            eprintln!("{hits}");
            bail!(
                "harness-onboarding-contract: onboarding must not introduce bespoke publication or recovery hooks (found in {label})"
            );
        }
    }

    if web_main.exists() && contains(&web_main, "reset_harness_bootstrap_storage_once")? {
        bail!(
            "harness-onboarding-contract: web onboarding may not carry harness-only bootstrap reset shortcuts"
        );
    }

    println!("harness onboarding contract: clean");
    Ok(())
}

pub fn run_harness_onboarding_publication() -> Result<()> {
    let repo_root = repo_root()?;

    let web_main = repo_root.join("crates/aura-web/src/main.rs");
    if web_main.exists() && contains(&web_main, "publish_onboarding_snapshot")? {
        bail!(
            "harness-onboarding-publication: web onboarding may not publish through a bespoke snapshot path"
        );
    }

    let harness_bridge = repo_root.join("crates/aura-web/src/harness_bridge.rs");
    if harness_bridge.exists() && contains(&harness_bridge, "stale_onboarding_publish")? {
        bail!(
            "harness-onboarding-publication: browser harness bridge may not carry stale-onboarding publication recovery"
        );
    }

    let driver_mjs = repo_root.join("crates/aura-harness/playwright-driver/playwright_driver.mjs");
    if driver_mjs.exists() {
        let src = read(&driver_mjs)?;
        if src.contains("staleOnboardingCache") || src.contains("stale_onboarding_") {
            bail!(
                "harness-onboarding-publication: playwright driver may not carry stale-onboarding recovery heuristics"
            );
        }
    }

    let local_pty = repo_root.join("crates/aura-harness/src/backend/local_pty.rs");
    if local_pty.exists() && contains(&local_pty, "synthetic_onboarding_snapshot")? {
        bail!(
            "harness-onboarding-publication: local PTY backend may not fabricate onboarding snapshots"
        );
    }

    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "hxrts-aura-app".into(),
            "onboarding_is_declared_in_the_shared_snapshot_model".into(),
            "--quiet".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "hxrts-aura-app".into(),
            "onboarding_uses_canonical_snapshot_publication_path".into(),
            "--quiet".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "hxrts-aura-app".into(),
            "onboarding_harness_paths_have_no_bespoke_recovery_logic".into(),
            "--quiet".into(),
        ],
    )?;

    println!("harness onboarding publication: clean");
    Ok(())
}

pub fn run_harness_raw_backend_quarantine() -> Result<()> {
    let repo_root = repo_root()?;
    let backend_dir = repo_root.join("crates/aura-harness/src/backend");

    let raw_impls = command_stdout(
        "rg",
        &[
            "-l".into(),
            "impl RawUiBackend for".into(),
            backend_dir.to_string_lossy().into_owned(),
        ],
    )
    .unwrap_or_default();

    let raw_impl_files: Vec<String> = raw_impls
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| {
            std::path::Path::new(l)
                .strip_prefix(&repo_root)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| l.to_string())
        })
        .collect();

    let expected = [
        "crates/aura-harness/src/backend/local_pty.rs",
        "crates/aura-harness/src/backend/playwright_browser.rs",
    ];

    if raw_impl_files.len() != expected.len() {
        bail!(
            "harness-raw-backend-quarantine: expected exactly {} raw backend impls, found {}: {:?}",
            expected.len(),
            raw_impl_files.len(),
            raw_impl_files
        );
    }

    for exp in &expected {
        if !raw_impl_files.iter().any(|f| f.as_str() == *exp) {
            bail!(
                "harness-raw-backend-quarantine: raw backend impl must stay quarantined to {exp}"
            );
        }
    }

    // Check raw accessor usage is quarantined
    let accessor_hits = command_stdout(
        "rg",
        &[
            "-l".into(),
            r"as_raw_ui_mut\(".into(),
            repo_root
                .join("crates/aura-harness/src")
                .to_string_lossy()
                .into_owned(),
        ],
    )
    .unwrap_or_default();

    for hit in accessor_hits.lines().filter(|l| !l.is_empty()) {
        let rel = std::path::Path::new(hit)
            .strip_prefix(&repo_root)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| hit.to_string());
        match rel.as_str() {
            "crates/aura-harness/src/backend/mod.rs" | "crates/aura-harness/src/coordinator.rs" => {
            }
            _ => bail!(
                "harness-raw-backend-quarantine: raw backend accessor escaped quarantine via {rel}"
            ),
        }
    }

    println!("harness raw backend quarantine: clean");
    Ok(())
}

pub fn run_harness_recovery_contract() -> Result<()> {
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "aura-harness".into(),
            "registered_recoveries_cover_all_paths".into(),
            "--quiet".into(),
        ],
    )?;

    let driver_method_sets =
        repo_root()?.join("crates/aura-harness/playwright-driver/src/method_sets.ts");
    if !contains(&driver_method_sets, "export const RECOVERY_METHODS")? {
        bail!("harness-recovery-contract: missing registered recovery metadata");
    }
    if !contains(&driver_method_sets, "'recover_ui_state'")? {
        bail!(
            "harness-recovery-contract: recover_ui_state must remain registered as an explicit recovery method"
        );
    }

    println!("harness recovery contract: clean");
    Ok(())
}

pub fn run_harness_render_convergence() -> Result<()> {
    let repo_root = repo_root()?;

    let harness_bridge = repo_root.join("crates/aura-web/src/harness_bridge.rs");
    if !rg_exists(&[
        "fn publish_ui_snapshot".into(),
        harness_bridge.to_string_lossy().into_owned(),
    ])? {
        bail!("harness-render-convergence: missing web publish hook");
    }

    let publication = repo_root.join("crates/aura-web/src/harness/publication.rs");
    if !rg_exists(&[
        "requestAnimationFrame".into(),
        publication.to_string_lossy().into_owned(),
    ])? {
        bail!("harness-render-convergence: web publish hook must go through requestAnimationFrame");
    }
    if !rg_exists(&[
        "publish_render_heartbeat".into(),
        publication.to_string_lossy().into_owned(),
    ])? {
        bail!("harness-render-convergence: web publish hook must emit render heartbeat");
    }

    let snapshot = repo_root.join("crates/aura-terminal/src/tui/harness_state/snapshot.rs");
    if !rg_exists(&[
        "next_projection_revision".into(),
        snapshot.to_string_lossy().into_owned(),
    ])? {
        bail!(
            "harness-render-convergence: tui semantic snapshots must publish projection revisions"
        );
    }

    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "hxrts-aura-app".into(),
            "render_convergence_accepts_matching_snapshot_and_heartbeat".into(),
            "--quiet".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "hxrts-aura-app".into(),
            "render_convergence_rejects_semantic_state_published_ahead_of_renderer".into(),
            "--quiet".into(),
        ],
    )?;

    println!("harness render convergence: clean");
    Ok(())
}

pub fn run_harness_revision_contract() -> Result<()> {
    let repo_root = repo_root()?;
    let ui_contract_file = repo_root.join("crates/aura-app/src/ui_contract.rs");
    let ui_contract_dir = repo_root.join("crates/aura-app/src/ui_contract");

    let mut ui_paths = vec![ui_contract_file.to_string_lossy().into_owned()];
    if ui_contract_dir.exists() {
        ui_paths.push(ui_contract_dir.to_string_lossy().into_owned());
    }

    {
        let mut args = vec!["pub revision: ProjectionRevision".into()];
        args.extend(ui_paths.clone());
        if !rg_exists(&args)? {
            bail!("harness-revision-contract: UiSnapshot must carry revision metadata");
        }
    }
    {
        let mut args = vec!["pub quiescence: QuiescenceSnapshot".into()];
        args.extend(ui_paths);
        if !rg_exists(&args)? {
            bail!("harness-revision-contract: UiSnapshot must carry quiescence metadata");
        }
    }

    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "hxrts-aura-app".into(),
            "projection_revision_detects_stale_snapshots_by_revision".into(),
            "--quiet".into(),
        ],
    )?;

    println!("harness revision contract: clean");
    Ok(())
}

pub fn run_harness_row_index_contract() -> Result<()> {
    let repo_root = repo_root()?;
    let inventory = repo_root.join("scenarios/harness_inventory.toml");

    if !inventory.exists() {
        bail!("harness-row-index-contract: missing inventory: scenarios/harness_inventory.toml");
    }

    // Parse inventory to find shared scenarios
    let inv_src = read(&inventory)?;
    let mut entries: Vec<(String, String)> = Vec::new(); // (path, class)
    let mut current_path = String::new();
    let mut current_class = String::new();
    for line in inv_src.lines() {
        let line = line.trim();
        if line == "[[scenario]]" {
            if !current_path.is_empty() {
                entries.push((current_path.clone(), current_class.clone()));
                current_path.clear();
                current_class.clear();
            }
        } else if let Some(rest) = line.strip_prefix("path = \"") {
            if let Some(path) = rest.strip_suffix('"') {
                current_path = path.to_string();
            }
        } else if let Some(rest) = line.strip_prefix("classification = \"") {
            if let Some(class) = rest.strip_suffix('"') {
                current_class = class.to_string();
            }
        }
    }
    if !current_path.is_empty() {
        entries.push((current_path, current_class));
    }

    let row_index_re = Regex::new(
        r#"^\s*item_id\s*=\s*"(row[-_:]?[0-9]+|idx[-_:]?[0-9]+|index[-_:]?[0-9]+|[0-9]+)"\s*$"#,
    )?;

    for (path, class) in &entries {
        if class != "shared" {
            continue;
        }
        let scenario_path = repo_root.join(path);
        if !scenario_path.exists() {
            bail!("harness-row-index-contract: missing shared scenario: {path}");
        }
        let src = read(&scenario_path)?;
        for line in src.lines() {
            if row_index_re.is_match(line) {
                bail!(
                    "harness-row-index-contract: shared scenario targets parity-critical list items by row index: {path}"
                );
            }
        }
    }

    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "hxrts-aura-app".into(),
            "snapshot_invariants_reject_row_index_ids".into(),
            "--quiet".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "hxrts-aura-app".into(),
            "shared_intent_contract_rejects_row_index_item_ids".into(),
            "--quiet".into(),
        ],
    )?;

    println!("harness row-index contract: clean");
    Ok(())
}

pub fn run_harness_runtime_events_authoritative() -> Result<()> {
    let repo_root = repo_root()?;
    let snapshot = repo_root.join("crates/aura-terminal/src/tui/harness_state/snapshot.rs");

    if snapshot.exists() {
        // Read only production source (before #[cfg(test)])
        let full_src = read(&snapshot)?;
        let production_source: String = full_src
            .lines()
            .take_while(|l| !l.trim_start().starts_with("#[cfg(test)]"))
            .collect::<Vec<_>>()
            .join("\n");

        let fact_variant_re = Regex::new(
            r"RuntimeFact::(ContactLinkReady|PendingHomeInvitationReady|ChannelMembershipReady|RecipientPeersResolved|MessageDeliveryReady)",
        )?;
        if fact_variant_re.is_match(&production_source) {
            bail!(
                "harness-runtime-events-authoritative: TUI snapshot export may not synthesize parity-critical runtime facts"
            );
        }

        if production_source.contains("runtime_events.push(RuntimeEventSnapshot") {
            bail!(
                "harness-runtime-events-authoritative: TUI snapshot export may not append runtime events heuristically"
            );
        }
    }

    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "aura-terminal".into(),
            "semantic_snapshot_exports_tui_owned_runtime_facts".into(),
            "--quiet".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "aura-terminal".into(),
            "semantic_snapshot_exporter_does_not_infer_parity_runtime_events".into(),
            "--quiet".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "hxrts-aura-app".into(),
            "ui_snapshot_parity_detects_runtime_event_shape_drift".into(),
            "--quiet".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "hxrts-aura-app".into(),
            "ui_snapshot_parity_detects_toast_drift".into(),
            "--quiet".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "aura-harness".into(),
            "shared_intent_waits_bind_only_to_declared_barriers".into(),
            "--quiet".into(),
        ],
    )?;

    println!("harness runtime events authoritative: clean");
    Ok(())
}

pub fn run_harness_scenario_config_boundary() -> Result<()> {
    let repo_root = repo_root()?;
    let configs_dir = repo_root.join("configs/harness");

    if !configs_dir.exists() {
        println!("harness scenario/config boundary: clean (no configs/harness dir)");
        return Ok(());
    }

    // Check that instance ids don't use frontend-specific names
    let forbidden_ids = command_stdout(
        "rg",
        &[
            "-n".into(),
            r#"^\s*id\s*=\s*"(web|tui|browser|local|playwright|pty)""#.into(),
            configs_dir.to_string_lossy().into_owned(),
            "-g".into(),
            "*.toml".into(),
        ],
    )
    .unwrap_or_default();

    if !forbidden_ids.trim().is_empty() {
        eprintln!("{forbidden_ids}");
        bail!(
            "harness-scenario-config-boundary: config instance ids must remain actor-based and frontend-neutral"
        );
    }

    // Check that mode bindings are declared
    let mode_hits = command_stdout(
        "rg",
        &[
            "-n".into(),
            r#"^\s*mode\s*=\s*"(local|browser|ssh)""#.into(),
            configs_dir.to_string_lossy().into_owned(),
            "-g".into(),
            "*.toml".into(),
        ],
    )
    .unwrap_or_default();

    if mode_hits.trim().is_empty() {
        bail!(
            "harness-scenario-config-boundary: expected config frontend/runtime bindings declared via instance.mode"
        );
    }

    println!("harness scenario/config boundary: clean");
    Ok(())
}

pub fn run_harness_scenario_inventory() -> Result<()> {
    let repo_root = repo_root()?;
    let inventory_path = repo_root.join("scenarios/harness_inventory.toml");
    let scenario_dir = repo_root.join("scenarios/harness");

    if !inventory_path.exists() {
        bail!("harness-scenario-inventory: missing inventory: scenarios/harness_inventory.toml");
    }

    let mut scenario_files: Vec<String> = fs::read_dir(&scenario_dir)
        .context("reading scenarios/harness")?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("toml"))
        .filter_map(|e| {
            e.path()
                .strip_prefix(&repo_root)
                .ok()
                .map(|p| p.to_string_lossy().into_owned())
        })
        .collect();
    scenario_files.sort();

    let inventory_content = read(&inventory_path)?;
    let mut inventory_paths: Vec<String> = Vec::new();
    let mut inventory_classes: Vec<String> = Vec::new();
    for line in inventory_content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("path = \"") {
            if let Some(path) = rest.strip_suffix('"') {
                inventory_paths.push(path.to_owned());
            }
        } else if let Some(rest) = line.strip_prefix("classification = \"") {
            if let Some(class) = rest.strip_suffix('"') {
                inventory_classes.push(class.to_owned());
            }
        }
    }
    inventory_paths.sort();

    if scenario_files.len() != inventory_paths.len() {
        bail!(
            "harness-scenario-inventory: inventory path count ({}) does not match scenario file count ({})",
            inventory_paths.len(),
            scenario_files.len()
        );
    }

    let inventory_set: BTreeSet<&str> = inventory_paths.iter().map(String::as_str).collect();
    for path in &scenario_files {
        if !inventory_set.contains(path.as_str()) {
            bail!("harness-scenario-inventory: scenario missing from inventory: {path}");
        }
    }
    for path in &inventory_paths {
        if !repo_root.join(path).exists() {
            bail!("harness-scenario-inventory: inventory references missing scenario: {path}");
        }
    }

    let valid = ["shared", "web_conformance", "tui_conformance"];
    for class in &inventory_classes {
        if !valid.contains(&class.as_str()) {
            bail!("harness-scenario-inventory: inventory contains invalid classification: {class}");
        }
    }

    println!("harness-scenario-inventory: clean");
    Ok(())
}

pub fn run_harness_scenario_legality() -> Result<()> {
    run_harness_governance_check("scenario-legality")
}

pub fn run_harness_scenario_shape_contract() -> Result<()> {
    run_harness_governance_check("scenario-shape-contract")
}

pub fn run_harness_semantic_primitive_contract() -> Result<()> {
    let repo_root = repo_root()?;
    let sc = repo_root.join("crates/aura-app/src/scenario_contract.rs");
    let sc_dir = repo_root.join("crates/aura-app/src/scenario_contract");
    let mut sc_paths = vec![sc.to_string_lossy().into_owned()];
    if sc_dir.exists() {
        sc_paths.push(sc_dir.to_string_lossy().into_owned());
    }

    for (pattern, label) in [
        (
            "ScenarioAction::Intent",
            "canonical shared scenario model must expose typed intent actions",
        ),
        (
            "validate_shared_intent_contract",
            "shared scenario validation must enforce typed intent actions",
        ),
    ] {
        let mut args = vec![pattern.into()];
        args.extend(sc_paths.clone());
        if !rg_exists(&args)? {
            bail!("harness-semantic-primitive-contract: {label}");
        }
    }

    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "hxrts-aura-app".into(),
            "shared_intent_contract_accepts_intents".into(),
            "--quiet".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "hxrts-aura-app".into(),
            "shared_intent_contract_rejects_ui_actions".into(),
            "--quiet".into(),
        ],
    )?;

    println!("harness semantic primitive contract: clean");
    Ok(())
}

pub fn run_harness_settings_surface_contract() -> Result<()> {
    run_harness_governance_check("settings-surface-contract")
}

pub fn run_harness_shared_scenario_contract() -> Result<()> {
    run_harness_governance_check("shared-scenario-contract")
}

pub fn run_harness_trace_determinism() -> Result<()> {
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "aura-harness".into(),
            "repeated_runs_with_same_seed_share_same_report_shape".into(),
            "--quiet".into(),
        ],
    )
    .context("harness-trace-determinism: same-seed report/trace determinism test failed")?;
    println!("harness trace determinism: clean");
    Ok(())
}

pub fn run_harness_ui_parity_contract() -> Result<()> {
    run_harness_governance_check("ui-parity-contract")
}

pub fn run_harness_ui_state_evented() -> Result<()> {
    for test_name in [
        "wait_contract_refs_cover_all_parity_wait_kinds",
        "semantic_wait_helpers_do_not_use_raw_dom_or_text_fallbacks",
        "raw_text_fallbacks_are_explicitly_diagnostic_only",
    ] {
        run_ok(
            "cargo",
            &[
                "test".into(),
                "-p".into(),
                "aura-harness".into(),
                test_name.into(),
                "--quiet".into(),
            ],
        )?;
    }

    run_browser_observation_contract()?;
    println!("harness ui-state evented policy: clean");
    Ok(())
}

pub fn run_harness_wait_contract() -> Result<()> {
    let repo_root = repo_root()?;
    let executor = repo_root.join("crates/aura-harness/src/executor.rs");
    let executor_path = executor.to_string_lossy().into_owned();

    for (pattern, label) in [
        (
            "enum WaitContractRef",
            "missing typed wait contract reference",
        ),
        (
            "fn ensure_wait_contract_declared",
            "shared semantic waits must validate declared barrier contracts",
        ),
        (
            "WaitContractRef::OperationState",
            "typed wait contracts must include operation wait support",
        ),
    ] {
        if !rg_exists(&[pattern.into(), executor_path.clone()])? {
            bail!("harness-wait-contract: {label}");
        }
    }

    // Modal wait check (either form)
    let has_modal_wait = rg_exists(&["-F".into(), "waits.modal(".into(), executor_path.clone()])?
        || rg_exists(&[
            "-F".into(),
            "fn wait_for_modal(".into(),
            executor_path.clone(),
        ])?;
    if !has_modal_wait {
        bail!(
            "harness-wait-contract: shared semantic execution must route modal waits through the typed wait contract"
        );
    }

    // Runtime event wait check
    let has_runtime_event_wait = rg_exists(&[
        "-F".into(),
        "BarrierDeclaration::RuntimeEvent".into(),
        executor_path.clone(),
    ])? || rg_exists(&[
        "-F".into(),
        "fn wait_for_runtime_event_snapshot(".into(),
        executor_path.clone(),
    ])?;
    if !has_runtime_event_wait {
        bail!(
            "harness-wait-contract: shared semantic execution must route runtime-event waits through the typed wait contract"
        );
    }

    // Semantic state wait check
    let has_semantic_state_wait = rg_exists(&[
        "-F".into(),
        "waits.semantic_state(".into(),
        executor_path.clone(),
    ])? || rg_exists(&[
        "-F".into(),
        "fn wait_for_semantic_state(".into(),
        executor_path.clone(),
    ])?;
    if !has_semantic_state_wait {
        bail!(
            "harness-wait-contract: shared semantic execution must route semantic waits through the typed wait contract"
        );
    }

    if !rg_exists(&[
        "-F".into(),
        "snapshot.operation_state(".into(),
        executor_path,
    ])? {
        bail!(
            "harness-wait-contract: typed wait contracts must read operation state through the shared snapshot surface"
        );
    }

    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "aura-harness".into(),
            "shared_intent_waits_bind_only_to_declared_barriers".into(),
            "--quiet".into(),
        ],
    )?;

    println!("harness wait contract: clean");
    Ok(())
}

pub fn run_ownership_capability_audit() -> Result<()> {
    let repo_root = repo_root()?;
    let mut had_hits = false;

    let run_group = |title: &str, pattern: &str, extra_args: &[String]| -> Result<bool> {
        let mut args = vec![
            "-n".into(),
            "--hidden".into(),
            "--glob".into(),
            "!docs/book/**".into(),
            "--glob".into(),
            "!crates/aura-macros/tests/boundaries/**".into(),
            "--glob".into(),
            "!crates/aura-agent/tests/ui/**".into(),
            pattern.into(),
        ];
        args.extend(extra_args.iter().cloned());

        let output = command_stdout("rg", &args).unwrap_or_default();
        let filtered: Vec<&str> = output
            .lines()
            .filter(|l| !l.contains("docs/809_capability_vocabulary_inventory.md:"))
            .filter(|l| !l.is_empty())
            .collect();

        if !filtered.is_empty() {
            eprintln!("## {title}");
            for line in &filtered {
                eprintln!("{line}");
            }
            eprintln!();
            return Ok(true);
        }
        Ok(false)
    };

    if run_group(
        "Product Choreography Vocabulary Drift",
        r#"guard_capability = "[^"]*,[^"]+""#,
        &[
            "-g".into(),
            "*.tell".into(),
            repo_root.join("crates").to_string_lossy().into_owned(),
        ],
    )? {
        had_hits = true;
    }

    if run_group(
        "Docs, Examples, and .claude Legacy Capability Guidance",
        r#""send_ping"|"send_pong"|guard_capability = "send_request"|guard_capability = "respond"|permission_name|"create_session"|"join_session"|"decline_session"|"activate_session"|"broadcast_message"|"check_status"|"report_status"|"end_session""#,
        &[
            "-g".into(),
            "*.md".into(),
            "-g".into(),
            "*.rs".into(),
            "-g".into(),
            "*.tell".into(),
            repo_root.join("docs").to_string_lossy().into_owned(),
            repo_root.join("examples").to_string_lossy().into_owned(),
            repo_root.join(".claude").to_string_lossy().into_owned(),
        ],
    )? {
        had_hits = true;
    }

    if run_group(
        "Support and Fixture Legacy Capability Vocabulary",
        r#""invitation:create"|capability\("recovery_initiate"\)|capability\("recovery_approve"\)|capability\("threshold_sign"\)"#,
        &[
            repo_root
                .join("crates/aura-core/src/ownership.rs")
                .to_string_lossy()
                .into_owned(),
            repo_root
                .join("crates/aura-testkit/src/fixtures/biscuit.rs")
                .to_string_lossy()
                .into_owned(),
        ],
    )? {
        had_hits = true;
    }

    if had_hits {
        bail!("capability-model-audit: remaining legacy/non-canonical hits detected");
    }

    println!("capability-model-audit: clean");
    Ok(())
}

pub fn run_privacy_tuning_gate() -> Result<()> {
    let repo_root = repo_root()?;
    let artifact_root = std::env::var("AURA_ADAPTIVE_PRIVACY_ARTIFACT_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| repo_root.join("artifacts/adaptive-privacy/phase6"));
    if artifact_root.exists() {
        fs::remove_dir_all(&artifact_root).with_context(|| {
            format!(
                "adaptive-privacy-phase6: removing {}",
                artifact_root.display()
            )
        })?;
    }
    fs::create_dir_all(&artifact_root).with_context(|| {
        format!(
            "adaptive-privacy-phase6: creating {}",
            artifact_root.display()
        )
    })?;

    let output = std::process::Command::new("cargo")
        .env("AURA_ADAPTIVE_PRIVACY_ARTIFACT_ROOT", &artifact_root)
        .args([
            "test",
            "-p",
            "aura-simulator",
            "--test",
            "adaptive_privacy_phase_six",
            "--",
            "--nocapture",
        ])
        .output()
        .context("starting adaptive privacy tuning test")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        bail!(
            "adaptive-privacy-phase6: cargo test failed{}\n{}",
            if stderr.trim().is_empty() { "" } else { ":" },
            if stderr.trim().is_empty() {
                stdout.trim()
            } else {
                stderr.trim()
            }
        );
    }

    for required in [
        "tuning_report.json",
        "matrix_results.json",
        "control-plane/index.json",
        "parity/report.json",
    ] {
        let path = artifact_root.join(required);
        if !path.exists() {
            bail!(
                "adaptive-privacy-phase6: missing expected artifact {}",
                repo_relative(&path)
            );
        }
    }

    println!(
        "adaptive-privacy-phase6: archived artifacts at {}",
        artifact_root.display()
    );
    Ok(())
}

pub fn run_protocol_compat(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return run_head_shell_script("scripts/check/protocol-compat.sh", &[]);
    }
    run_head_shell_script("scripts/check/protocol-compat.sh", args)
}

pub fn run_protocol_device_id_legacy(args: &[String]) -> Result<()> {
    let repo_root = repo_root()?;
    let mode = args.first().map(String::as_str).unwrap_or("legacy");

    let mut violations = 0u32;

    let check_pattern = |description: &str, pattern: &str, extra_args: &[String]| -> Result<bool> {
        let mut rg_args = vec!["-n".into(), "--glob".into(), "*.rs".into()];
        rg_args.extend(extra_args.iter().cloned());
        rg_args.push(pattern.into());
        rg_args.push(repo_root.join("crates").to_string_lossy().into_owned());
        let output = command_stdout("rg", &rg_args).unwrap_or_default();
        if !output.trim().is_empty() {
            eprintln!("✖ {description}");
            eprintln!("{output}");
            return Ok(true);
        }
        Ok(false)
    };

    match mode {
        "legacy" => {
            if check_pattern(
                "removed authority/device helper constructor detected",
                r"AuthorityId::for_device\(|DeviceId::for_authority\(",
                &[],
            )? {
                violations += 1;
            }

            if check_pattern(
                "legacy authority-from-device UUID coercion detected",
                r"AuthorityId::from_uuid\(([^)]*device[^)]*)\)|AuthorityId\([^)]*device[^)]*\)",
                &[],
            )? {
                violations += 1;
            }

            if check_pattern(
                "legacy authority-from-device field coercion detected",
                r"AuthorityId::from_uuid\(([^)]*(device|participant)[^)]*\.0[^)]*)\)",
                &[],
            )? {
                violations += 1;
            }

            if check_pattern(
                "legacy device-from-authority UUID coercion detected",
                r"DeviceId::from_uuid\(([^)]*authority[^)]*)\)|DeviceId\([^)]*authority[^)]*\)",
                &[],
            )? {
                violations += 1;
            }

            if check_pattern(
                "open-coded authority-from-device derivation detected outside canonical bridge",
                r"derived_uuid_with_bytes\(\s*AUTHORITY_FOR_DEVICE_DOMAIN",
                &[
                    "--glob".into(),
                    "!crates/aura-core/src/types/identifiers.rs".into(),
                ],
            )? {
                violations += 1;
            }

            if violations > 0 {
                bail!(
                    "device-id-legacy: found {violations} legacy authority/device coercion pattern(s); use derive_legacy_authority_from_device(...) only, with explicit metadata"
                );
            }
            println!("device-id-legacy: clean");
        }
        "audit-live" => {
            let live_globs: Vec<String> = vec![
                "--glob".into(),
                "!**/tests/**".into(),
                "--glob".into(),
                "!**/test_*.rs".into(),
                "--glob".into(),
                "!**/*_test.rs".into(),
            ];

            if check_pattern(
                "live authority/device helper derivation detected",
                r"^(?!\s*//).*(AuthorityId::for_device\(|DeviceId::for_authority\()",
                &[
                    "-P".into(),
                    live_globs[0].clone(),
                    live_globs[1].clone(),
                    live_globs[2].clone(),
                    live_globs[3].clone(),
                    live_globs[4].clone(),
                    live_globs[5].clone(),
                ],
            )? {
                violations += 1;
            }

            if check_pattern(
                "live bootstrap authority derivation helper detected",
                r"^(?!\s*//).*(derive_authority_id\()",
                &[
                    "-P".into(),
                    live_globs[0].clone(),
                    live_globs[1].clone(),
                    live_globs[2].clone(),
                    live_globs[3].clone(),
                    live_globs[4].clone(),
                    live_globs[5].clone(),
                ],
            )? {
                violations += 1;
            }

            if violations > 0 {
                bail!(
                    "device-id-legacy audit: found {violations} live authority/device derivation pattern(s)"
                );
            }
            println!("device-id-legacy audit: clean");
        }
        "audit-runtime" => {
            let runtime_globs: Vec<String> = vec![
                "--glob".into(),
                "!**/tests/**".into(),
                "--glob".into(),
                "!**/test_*.rs".into(),
                "--glob".into(),
                "!**/*_test.rs".into(),
                "--glob".into(),
                "!crates/aura-agent/src/runtime/effects.rs".into(),
                "--glob".into(),
                "!crates/aura-agent/src/handlers/sessions/coordination.rs".into(),
                "--glob".into(),
                "!crates/aura-simulator/src/choreography_transport.rs".into(),
                "--glob".into(),
                "!crates/aura-simulator/src/testkit_bridge.rs".into(),
            ];

            if check_pattern(
                "runtime authority/device helper derivation detected",
                r"^(?!\s*//).*(AuthorityId::for_device\(|DeviceId::for_authority\()",
                &[
                    "-P".into(),
                    runtime_globs[0].clone(),
                    runtime_globs[1].clone(),
                    runtime_globs[2].clone(),
                    runtime_globs[3].clone(),
                    runtime_globs[4].clone(),
                    runtime_globs[5].clone(),
                    runtime_globs[6].clone(),
                    runtime_globs[7].clone(),
                    runtime_globs[8].clone(),
                    runtime_globs[9].clone(),
                    runtime_globs[10].clone(),
                    runtime_globs[11].clone(),
                    runtime_globs[12].clone(),
                    runtime_globs[13].clone(),
                ],
            )? {
                violations += 1;
            }

            if check_pattern(
                "runtime bootstrap authority derivation helper detected",
                r"^(?!\s*//).*(derive_authority_id\()",
                &[
                    "-P".into(),
                    runtime_globs[0].clone(),
                    runtime_globs[1].clone(),
                    runtime_globs[2].clone(),
                    runtime_globs[3].clone(),
                    runtime_globs[4].clone(),
                    runtime_globs[5].clone(),
                    runtime_globs[6].clone(),
                    runtime_globs[7].clone(),
                    runtime_globs[8].clone(),
                    runtime_globs[9].clone(),
                    runtime_globs[10].clone(),
                    runtime_globs[11].clone(),
                    runtime_globs[12].clone(),
                    runtime_globs[13].clone(),
                ],
            )? {
                violations += 1;
            }

            if violations > 0 {
                bail!(
                    "device-id-legacy runtime audit: found {violations} runtime authority/device derivation pattern(s)"
                );
            }
            println!("device-id-legacy runtime audit: clean");
        }
        _ => {
            bail!("usage: protocol-device-id-legacy [legacy|audit-live|audit-runtime]");
        }
    }

    Ok(())
}

pub fn run_runtime_bootstrap_guardrails() -> Result<()> {
    let repo_root = repo_root()?;
    let mut violations = 0u32;

    let check = |description: &str, pattern: &str, path: &std::path::Path| -> Result<bool> {
        if !path.exists() {
            return Ok(false);
        }
        let hits = command_stdout(
            "rg",
            &[
                "-n".into(),
                pattern.into(),
                path.to_string_lossy().into_owned(),
            ],
        )
        .unwrap_or_default();
        if !hits.trim().is_empty() {
            eprintln!("✖ {description}");
            eprintln!("{hits}");
            return Ok(true);
        }
        Ok(false)
    };

    if check(
        "preset builder authority fallback detected",
        r"self\.authority_id\.(unwrap_or|unwrap_or_else)\(",
        &repo_root.join("crates/aura-agent/src/builder"),
    )? {
        violations += 1;
    }

    if check(
        "preset builder creates authority directly instead of requiring explicit bootstrap identity",
        r"AuthorityId::new_from_entropy\(|new_authority_id\(",
        &repo_root.join("crates/aura-agent/src/builder"),
    )? {
        violations += 1;
    }

    if check(
        "terminal main still hard-codes synthetic startup authority/context",
        r#"ids::authority_id\("cli:main-authority"\)|ids::context_id\("cli:main-context"\)"#,
        &repo_root.join("crates/aura-terminal/src/main.rs"),
    )? {
        violations += 1;
    }

    if violations > 0 {
        bail!("bootstrap-guardrails: found {violations} bootstrap guardrail violation(s)");
    }

    println!("bootstrap-guardrails: clean");
    Ok(())
}

pub fn run_shared_flow_metadata() -> Result<()> {
    let repo_root = repo_root()?;
    let sc = repo_root.join("crates/aura-app/src/scenario_contract.rs");
    let sc_dir = repo_root.join("crates/aura-app/src/scenario_contract");
    let mut sc_paths = vec![sc.to_string_lossy().into_owned()];
    if sc_dir.exists() {
        sc_paths.push(sc_dir.to_string_lossy().into_owned());
    }

    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "hxrts-aura-app".into(),
            "every_intent_kind_has_a_matching_contract".into(),
            "--quiet".into(),
        ],
    )
    .context("harness-shared-flow-metadata: shared intent metadata contract is incomplete")?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "hxrts-aura-app".into(),
            "every_intent_kind_declares_barrier_metadata".into(),
            "--quiet".into(),
        ],
    )
    .context(
        "harness-shared-flow-metadata: shared intent barrier metadata contract is incomplete",
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "hxrts-aura-app".into(),
            "declared_post_operation_convergence_contracts_are_explicit".into(),
            "--quiet".into(),
        ],
    )
    .context(
        "harness-shared-flow-metadata: shared intent convergence metadata contract is incomplete",
    )?;

    for (pattern, label) in [
        (
            "pub struct SharedActionContract",
            "missing SharedActionContract schema",
        ),
        (
            "pub enum ActionPrecondition",
            "missing ActionPrecondition schema",
        ),
        (
            "pub struct SharedActionBarrierMetadata",
            "missing SharedActionBarrierMetadata schema",
        ),
        (
            "pub enum BarrierDeclaration",
            "missing BarrierDeclaration schema",
        ),
        (
            "pub struct PostOperationConvergenceContract",
            "missing PostOperationConvergenceContract schema",
        ),
    ] {
        let mut args = vec![pattern.into()];
        args.extend(sc_paths.clone());
        if !rg_exists(&args)? {
            bail!("harness-shared-flow-metadata: {label}");
        }
    }

    println!("harness shared-flow metadata: clean");
    Ok(())
}

pub fn run_shared_intent_flow() -> Result<()> {
    let repo_root = repo_root()?;
    let backend_contract = repo_root.join("crates/aura-harness/src/backend/mod.rs");

    if backend_contract.exists() {
        let src = read(&backend_contract)?;
        if src.contains("SHARED_INTENT_UI_BYPASS_ALLOWLIST")
            || src.contains("TemporaryHarnessBridgeShortcut")
        {
            bail!(
                "harness-shared-intent-ui-flow: legacy shared intent UI bypass allowlist machinery must be removed"
            );
        }
    }

    let backend_dir = repo_root.join("crates/aura-harness/src/backend");
    let backend_hits = command_stdout(
        "rg",
        &[
            "-n".into(),
            "context_workflows::|invitation_workflows::".into(),
            backend_dir.to_string_lossy().into_owned(),
        ],
    )
    .unwrap_or_default();
    if !backend_hits.trim().is_empty() {
        bail!(
            "harness-shared-intent-ui-flow: backend implementations must not call app-internal workflow shortcuts directly"
        );
    }

    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "aura-harness".into(),
            "local_shared_intent_methods_use_semantic_harness_commands_for_shared_flows".into(),
            "--quiet".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "aura-harness".into(),
            "playwright_shared_intent_methods_use_semantic_bridge".into(),
            "--quiet".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "aura-harness".into(),
            "playwright_shared_semantic_methods_do_not_regress_to_raw_ui_driving".into(),
            "--quiet".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "aura-harness".into(),
            "playwright_shared_semantic_bridge_replaces_shortcut_bypasses".into(),
            "--quiet".into(),
        ],
    )?;

    println!("harness shared-intent ui flow: clean");
    Ok(())
}

pub fn run_shared_raw_quarantine() -> Result<()> {
    let repo_root = repo_root()?;
    let executor = repo_root.join("crates/aura-harness/src/executor.rs");

    if !executor.exists() {
        println!("harness shared raw-ui quarantine: clean (no executor)");
        return Ok(());
    }

    let executor_src = read(&executor)?;

    // Extract execute_semantic_step function body
    let fn_start = "fn execute_semantic_step(";
    let fn_end = "\n\nfn execute_semantic_environment_action";
    let semantic_fn = if let (Some(start), Some(end)) =
        (executor_src.find(fn_start), executor_src.find(fn_end))
    {
        if start < end {
            executor_src[start..end].to_string()
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    if semantic_fn.is_empty() {
        bail!("harness-shared-raw-ui-quarantine: could not extract execute_semantic_step");
    }

    for forbidden in [
        "ToolRequest::ClickButton",
        "ToolRequest::FillInput",
        "ToolRequest::FillField",
        ".click_button(",
        ".fill_input(",
        ".fill_field(",
        ".click_target(",
    ] {
        if semantic_fn.contains(forbidden) {
            bail!(
                "harness-shared-raw-ui-quarantine: shared semantic execution still reaches raw helper: {forbidden}"
            );
        }
    }

    println!("harness shared raw-ui quarantine: clean");
    Ok(())
}

pub fn run_shared_semantic_dedup() -> Result<()> {
    let repo_root = repo_root()?;
    let backend_contract = repo_root.join("crates/aura-harness/src/backend/mod.rs");
    let local_backend = repo_root.join("crates/aura-harness/src/backend/local_pty.rs");
    let browser_backend = repo_root.join("crates/aura-harness/src/backend/playwright_browser.rs");

    for path in [&backend_contract, &local_backend] {
        if path.exists() {
            let src = read(path)?;
            if src.contains("submit_accept_contact_invitation_via_shared_ui")
                || src.contains("submit_invite_actor_to_channel_via_shared_ui")
            {
                bail!(
                    "harness-shared-semantic-dedup: legacy shared semantic UI helper shortcuts must be removed (in {})",
                    repo_relative(path)
                );
            }
        }
    }

    let local_src = if local_backend.exists() {
        read(&local_backend)?
    } else {
        String::new()
    };
    for command in [
        "HarnessUiCommand::OpenSettingsSection",
        "HarnessUiCommand::StartDeviceEnrollment",
        "HarnessUiCommand::ImportDeviceEnrollmentCode",
        "HarnessUiCommand::RemoveSelectedDevice",
        "HarnessUiCommand::CreateContactInvitation",
        "HarnessUiCommand::InviteActorToChannel",
        "HarnessUiCommand::SelectChannel",
    ] {
        if !local_src.contains(command) {
            bail!(
                "harness-shared-semantic-dedup: local backend must route {command} through typed harness commands"
            );
        }
    }

    if backend_contract.exists() {
        let contract_src = read(&backend_contract)?;
        if contract_src.contains("SHARED_INTENT_UI_BYPASS_ALLOWLIST")
            || contract_src.contains("TemporaryHarnessBridgeShortcut")
        {
            bail!(
                "harness-shared-semantic-dedup: shared semantic browser bridge migration should remove the old bypass allowlist machinery"
            );
        }
    }

    if browser_backend.exists() {
        let browser_src = read(&browser_backend)?;
        if !browser_src.contains("fn submit_semantic_command(") {
            bail!(
                "harness-shared-semantic-dedup: browser backend must route supported semantic submissions through the typed bridge"
            );
        }
        if browser_src.contains("submit_accept_contact_invitation_via_shared_ui")
            || browser_src.contains("submit_invite_actor_to_channel_via_shared_ui")
        {
            bail!(
                "harness-shared-semantic-dedup: browser backend should not keep local-only shared UI helper shortcuts"
            );
        }
    }

    println!("harness shared semantic dedup: clean");
    Ok(())
}

pub fn run_tui_observation_channel() -> Result<()> {
    let repo_root = repo_root()?;
    let backend = repo_root.join("crates/aura-harness/src/backend/local_pty.rs");
    let executor = repo_root.join("crates/aura-harness/src/executor.rs");

    if backend.exists() {
        let backend_src = read(&backend)?;

        // Extract ui_snapshot function body
        let ui_snapshot_body = {
            let marker = "fn ui_snapshot(&self) -> Result<UiSnapshot> {";
            backend_src.find(marker).map(|start| {
                let rest = &backend_src[start..];
                // find end by locating the next double-newline + fn pattern
                let end_marker = "\n\n    fn wait_for_ui_snapshot_event(";
                rest.find(end_marker).map(|e| &rest[..e]).unwrap_or(rest)
            })
        };

        if let Some(body) = ui_snapshot_body {
            for forbidden in [
                "fs::read_to_string",
                "thread::sleep",
                "AURA_TUI_UI_STATE_FILE",
                "SNAPSHOT_WAIT_ATTEMPTS",
            ] {
                if body.contains(forbidden) {
                    bail!(
                        "harness-tui-observation-channel: local PTY ui_snapshot may not poll the filesystem or sleep (found: {forbidden})"
                    );
                }
            }
        }

        // Extract wait_for_ui_snapshot_event body
        let wait_body = {
            let marker = "fn wait_for_ui_snapshot_event(";
            backend_src.find(marker).map(|start| {
                let rest = &backend_src[start..];
                let end_marker = "\n\n    fn activate_control(";
                rest.find(end_marker).map(|e| &rest[..e]).unwrap_or(rest)
            })
        };

        if let Some(body) = wait_body {
            if body.contains("thread::sleep") {
                bail!(
                    "harness-tui-observation-channel: local PTY wait_for_ui_snapshot_event must use the event channel, not sleeps"
                );
            }
        }

        if !backend_src.contains("AURA_TUI_UI_STATE_SOCKET") {
            bail!(
                "harness-tui-observation-channel: local PTY backend must provision the TUI snapshot socket"
            );
        }
    }

    if executor.exists() {
        // Extract wait_for_home_bootstrap_ready body
        let executor_src = read(&executor)?;
        let bootstrap_body = {
            let marker = "fn wait_for_home_bootstrap_ready(";
            executor_src.find(marker).map(|start| {
                let rest = &executor_src[start..];
                let end_marker = "\n}\n\nfn home_bootstrap_ready(";
                rest.find(end_marker).map(|e| &rest[..e]).unwrap_or(rest)
            })
        };

        if let Some(body) = bootstrap_body {
            if body.contains("thread::sleep") || body.contains("std::thread::sleep") {
                bail!(
                    "harness-tui-observation-channel: home bootstrap wait must not use raw sleep polling"
                );
            }
        }
    }

    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "aura-harness".into(),
            "local_backend_uses_socket_driven_ui_snapshot_channel".into(),
            "--quiet".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "aura-harness".into(),
            "missing_tui_ui_snapshot_fails_loudly".into(),
            "--quiet".into(),
        ],
    )?;

    println!("harness tui observation channel: clean");
    Ok(())
}

pub fn run_tui_product_path() -> Result<()> {
    let repo_root = repo_root()?;
    let shell = repo_root.join("crates/aura-terminal/src/tui/screens/app/shell.rs");

    if shell.exists() {
        if contains(&shell, "AURA_HARNESS_MODE")? {
            bail!(
                "harness-tui-product-path: TUI product action dispatch may not branch on AURA_HARNESS_MODE"
            );
        }
        let shell_src = read(&shell)?;
        for pattern in [
            "runtime.create_contact_invitation(",
            "runtime.export_invitation(",
            "runtime.import_invitation(",
            "runtime.accept_invitation(",
        ] {
            if shell_src.contains(pattern) {
                bail!(
                    "harness-tui-product-path: TUI product action dispatch may not call runtime invitation shortcuts directly"
                );
            }
        }
    }

    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "aura-terminal".into(),
            "invitation_dispatch_uses_product_callbacks_without_harness_shortcuts".into(),
            "--quiet".into(),
        ],
    )?;

    println!("harness tui product path: clean");
    Ok(())
}

pub fn run_tui_selection_contract() -> Result<()> {
    let repo_root = repo_root()?;
    let events = repo_root.join("crates/aura-terminal/src/tui/screens/app/shell/events.rs");
    let shell = repo_root.join("crates/aura-terminal/src/tui/screens/app/shell.rs");
    let subscriptions = repo_root.join("crates/aura-terminal/src/tui/screens/app/subscriptions.rs");

    if events.exists()
        && !rg_exists(&[
            "fn resolve_committed_selected_channel_id".into(),
            events.to_string_lossy().into_owned(),
        ])?
    {
        bail!("harness-tui-selection-contract: missing committed TUI channel selection helper");
    }

    if shell.exists() {
        let shell_src = read(&shell)?;
        if !shell_src.contains("SharedCommittedChannelSelection")
            && !shell_src.contains("None::<CommittedChannelSelection>")
        {
            bail!(
                "harness-tui-selection-contract: shared TUI channel selection must be tracked by canonical committed channel identity"
            );
        }
    }

    if subscriptions.exists()
        && rg_exists(&[
            r"all_channels\(\)\s*\.next\(".into(),
            subscriptions.to_string_lossy().into_owned(),
        ])?
    {
        bail!(
            "harness-tui-selection-contract: message subscription may not fall back to first channel"
        );
    }

    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "aura-terminal".into(),
            "committed_channel_resolution_requires_authoritative_selection".into(),
            "--quiet".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "aura-terminal".into(),
            "send_dispatch_does_not_background_retry_selection".into(),
            "--quiet".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "aura-terminal".into(),
            "start_chat_dispatch_does_not_optimistically_navigate".into(),
            "--quiet".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "aura-terminal".into(),
            "message_subscription_requires_explicit_selected_channel_identity".into(),
            "--quiet".into(),
        ],
    )?;

    println!("harness tui selection contract: clean");
    Ok(())
}

pub fn run_tui_semantic_snapshot() -> Result<()> {
    let repo_root = repo_root()?;
    let snapshot = repo_root.join("crates/aura-terminal/src/tui/harness_state/snapshot.rs");

    if snapshot.exists() {
        let src = read(&snapshot)?;
        let static_re =
            Regex::new(r"^static (CONTACTS_OVERRIDE|DEVICES_OVERRIDE|MESSAGES_OVERRIDE)")?;
        if src.lines().any(|l| static_re.is_match(l)) {
            bail!(
                "harness-tui-semantic-snapshot: parity-critical TUI exporter may not use contact/device/message override caches"
            );
        }

        let pub_fn_re = Regex::new(
            r"^pub fn (publish_contacts_list_export|publish_devices_list_export|publish_messages_export)",
        )?;
        if src.lines().any(|l| pub_fn_re.is_match(l)) {
            bail!(
                "harness-tui-semantic-snapshot: parity-critical TUI exporter may not declare contact/device/message publish overrides"
            );
        }

        if !src.contains("pub struct TuiSemanticInputs") {
            // Check in commands.rs instead
            let commands = repo_root.join("crates/aura-terminal/src/tui/harness_state/commands.rs");
            if commands.exists() && !contains(&commands, "pub struct TuiSemanticInputs")? {
                bail!(
                    "harness-tui-semantic-snapshot: missing explicit TUI semantic input contract"
                );
            } else if !commands.exists() {
                bail!(
                    "harness-tui-semantic-snapshot: missing explicit TUI semantic input contract"
                );
            }
        }

        if !src.contains("exported_runtime_events") {
            bail!(
                "harness-tui-semantic-snapshot: TUI exporter must consume runtime facts from owned state"
            );
        }
    }

    // Check that screens don't depend on override publish functions
    for dir in [
        repo_root.join("crates/aura-terminal/src/tui/screens"),
        repo_root.join("crates/aura-terminal/src/tui/screens/app/subscriptions.rs"),
    ] {
        if !dir.exists() {
            continue;
        }
        let hits = command_stdout(
            "rg",
            &[
                "publish_contacts_list_export|publish_devices_list_export|publish_messages_export"
                    .into(),
                dir.to_string_lossy().into_owned(),
            ],
        )
        .unwrap_or_default();
        if !hits.trim().is_empty() {
            bail!(
                "harness-tui-semantic-snapshot: parity-critical TUI exporter may not depend on contact/device/message publish overrides"
            );
        }
    }

    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "aura-terminal".into(),
            "semantic_snapshot_does_not_synthesize_placeholder_contact_ids".into(),
            "--quiet".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "aura-terminal".into(),
            "semantic_snapshot_exporter_does_not_depend_on_parity_override_caches".into(),
            "--quiet".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "aura-terminal".into(),
            "semantic_snapshot_ready_state_is_projection_only".into(),
            "--quiet".into(),
        ],
    )?;
    run_ok(
        "cargo",
        &[
            "test".into(),
            "-p".into(),
            "aura-terminal".into(),
            "semantic_snapshot_exports_tui_owned_runtime_facts".into(),
            "--quiet".into(),
        ],
    )?;

    println!("harness tui semantic snapshot: clean");
    Ok(())
}

pub fn run_user_flow_coverage() -> Result<()> {
    run_harness_governance_check("user-flow-coverage")
}

pub fn run_user_flow_guidance_sync() -> Result<()> {
    let changed = changed_files_for_guidance()?;
    if changed.is_empty() {
        println!("user-flow-guidance-sync: no changed files");
        return Ok(());
    }

    // Rule sources: mapping each rule_id to the files that trigger it.
    // Mirrors the `matches_any_rule_source` logic from the original shell script.
    let triggers_testing_guide = changed.iter().any(|f| {
        f == "crates/aura-app/src/ui_contract.rs"
            || f.starts_with("crates/aura-harness/src/")
            || f.starts_with("crates/aura-harness/playwright-driver/")
            || f.starts_with("crates/aura-terminal/src/tui/")
            || f.starts_with("crates/aura-ui/src/")
            || f.starts_with("crates/aura-web/src/")
    });
    let triggers_coverage_report = changed.iter().any(|f| {
        f == "crates/aura-app/src/ui_contract.rs"
            || f.starts_with("scenarios/harness/")
            || f == "scenarios/harness_inventory.toml"
    });
    let triggers_agent_guidance = changed
        .iter()
        .any(|f| f == "toolkit/xtask/src/checks/policy.rs");
    let triggers_skills_guidance = triggers_agent_guidance;

    let mut triggered = 0;
    let mut violations = 0;

    if triggers_testing_guide {
        triggered += 1;
        if !changed.contains("docs/804_testing_guide.md") {
            eprintln!("✖ testing_guide_sync: missing required updates");
            eprintln!(
                "  Shared UX contract, determinism, and parity-surface changes must update the testing guide."
            );
            eprintln!("  - docs/804_testing_guide.md");
            violations += 1;
        } else {
            println!("• testing_guide_sync: required guidance updates present");
        }
    }

    if triggers_coverage_report {
        triggered += 1;
        if !changed.contains("docs/997_flow_coverage.md") {
            eprintln!("✖ coverage_report_sync: missing required updates");
            eprintln!(
                "  Shared-flow coverage, scenario inventory, and parity classification changes must update the user flow coverage report."
            );
            eprintln!("  - docs/997_flow_coverage.md");
            violations += 1;
        } else {
            println!("• coverage_report_sync: required guidance updates present");
        }
    }

    if triggers_agent_guidance {
        triggered += 1;
        if !changed.contains("AGENTS.md") {
            eprintln!("✖ agent_guidance_sync: missing required updates");
            eprintln!("  Changes to shared UX contributor policy must update AGENTS guidance.");
            eprintln!("  - AGENTS.md");
            violations += 1;
        } else {
            println!("• agent_guidance_sync: required guidance updates present");
        }
    }

    if triggers_skills_guidance {
        let skills = [
            ".claude/skills/testing/SKILL.md",
            ".claude/skills/harness-run/SKILL.md",
            ".claude/skills/aura-quick-ref/SKILL.md",
        ];
        let repo_root = repo_root()?;
        let claude_dir = repo_root.join(".claude");
        if claude_dir.exists() {
            // Only require git-tracked files to appear in the diff.
            // Gitignored files (e.g. .claude/**) can never appear in
            // `git diff --name-only` and must be skipped.
            let tracked_skills: Vec<&str> = skills
                .iter()
                .copied()
                .filter(|&s| {
                    let p = repo_root.join(s);
                    if !p.exists() {
                        return false;
                    }
                    Command::new("git")
                        .args(["ls-files", "--error-unmatch", s])
                        .current_dir(&repo_root)
                        .output()
                        .map(|o| o.status.success())
                        .unwrap_or(false)
                })
                .collect();
            if !tracked_skills.is_empty() {
                triggered += 1;
                let missing: Vec<&str> = tracked_skills
                    .iter()
                    .copied()
                    .filter(|&s| !changed.contains(s))
                    .collect();
                if !missing.is_empty() {
                    eprintln!("✖ skills_guidance_sync: missing required updates");
                    eprintln!(
                        "  Changes to shared UX contributor policy must update local skills."
                    );
                    for m in &missing {
                        eprintln!("  - {m}");
                    }
                    violations += 1;
                } else {
                    println!("• skills_guidance_sync: required guidance updates present");
                }
            } else {
                // All skill files are gitignored — cannot enforce via git diff.
                println!(
                    "• skills_guidance_sync: skill files are untracked (gitignored); update manually"
                );
            }
        }
    }

    if triggered == 0 {
        println!("user-flow-guidance-sync: no mapped shared-user-flow guidance changes");
        return Ok(());
    }

    if violations > 0 {
        bail!("user-flow-guidance-sync: {violations} violation(s)");
    }

    println!("user-flow-guidance-sync: clean");
    Ok(())
}

// long-block-exception: direct port of user-flow-policy-guardrails.sh diff parser;
// each check maps one-to-one to a named policy invariant
pub fn run_user_flow_policy_guardrails() -> Result<()> {
    // Static metadata check — always runs, independent of diff.
    let repo_root = repo_root()?;
    let allowlisted: &[(&str, &str, &str, &str)] = &[
        (
            "crates/aura-app/src/workflows/runtime.rs",
            "aura-app-runtime",
            "runtime harness toggles and deterministic instrumentation only",
            "docs/804_testing_guide.md",
        ),
        (
            "crates/aura-app/src/workflows/invitation.rs",
            "aura-app-invitation",
            "invitation harness instrumentation only; no parity-critical flow bypass",
            "docs/804_testing_guide.md",
        ),
        (
            "crates/aura-agent/src/handlers/invitation.rs",
            "aura-agent-invitation",
            "runtime-owned invitation handler instrumentation only",
            "docs/804_testing_guide.md",
        ),
        (
            "crates/aura-agent/src/runtime/effects.rs",
            "aura-agent-runtime-effects",
            "effect wiring for deterministic harness-mode runtime assembly only",
            "docs/804_testing_guide.md",
        ),
        (
            "crates/aura-agent/src/runtime_bridge/mod.rs",
            "aura-agent-runtime-bridge",
            "runtime bridge instrumentation and environment binding only",
            "docs/804_testing_guide.md",
        ),
        (
            "crates/aura-terminal/src/tui/context/io_context.rs",
            "aura-terminal-tui-context",
            "TUI IO instrumentation and deterministic harness plumbing only",
            "crates/aura-terminal/ARCHITECTURE.md",
        ),
        (
            "crates/aura-web/src/main.rs",
            "aura-web-main",
            "web harness instrumentation and snapshot publication only",
            "docs/804_testing_guide.md",
        ),
    ];

    let mut violations = 0u32;
    for (file, owner, justification, design_ref) in allowlisted {
        if owner.is_empty() {
            eprintln!("✖ {file}: missing allowlisted harness-mode owner metadata");
            violations += 1;
        }
        if justification.is_empty() {
            eprintln!("✖ {file}: missing allowlisted harness-mode justification metadata");
            violations += 1;
        }
        if design_ref.is_empty() {
            eprintln!("✖ {file}: missing allowlisted harness-mode design-note reference");
            violations += 1;
        } else if !repo_root.join(design_ref).exists() {
            eprintln!(
                "✖ {file}: allowlisted harness-mode design-note reference does not exist: {design_ref}"
            );
            violations += 1;
        }
    }

    // Diff-aware checks.
    let diff_range = match compute_diff_range("AURA_UX_POLICY_DIFF_RANGE")? {
        Some(r) => r,
        None => {
            println!(
                "user-flow-policy-guardrails: unable to compute diff range; skipping diff checks"
            );
            if violations > 0 {
                bail!("user-flow-policy-guardrails: {violations} violation(s)");
            }
            println!("user-flow-policy-guardrails: clean");
            return Ok(());
        }
    };

    // Browser bridge compat: check if bridge files changed without doc updates.
    let diff_names = diff_names(&diff_range)?;
    let bridge_changed = diff_names.iter().any(|f| {
        f == "crates/aura-web/src/harness_bridge.rs"
            || f == "crates/aura-web/src/main.rs"
            || f == "crates/aura-harness/playwright-driver/playwright_driver.mjs"
    });
    if bridge_changed {
        if !diff_names.contains("crates/aura-web/ARCHITECTURE.md") {
            eprintln!(
                "✖ browser harness bridge compatibility changes require crates/aura-web/ARCHITECTURE.md updates"
            );
            violations += 1;
        }
        if !diff_names.contains("docs/804_testing_guide.md") {
            eprintln!(
                "✖ browser harness bridge compatibility changes require docs/804_testing_guide.md updates"
            );
            violations += 1;
        }
    }

    // Per-line diff checks.
    let allowlisted_harness_mode_files: BTreeSet<&str> =
        allowlisted.iter().map(|(f, ..)| *f).collect();
    let sleep_guard_paths: BTreeSet<&str> = [
        "crates/aura-harness/src/coordinator.rs",
        "crates/aura-harness/src/executor.rs",
        "crates/aura-harness/playwright-driver/playwright_driver.mjs",
        "crates/aura-terminal/src/tui/harness_state/snapshot.rs",
        "crates/aura-web/src/harness_bridge.rs",
    ]
    .into();
    let row_index_guard_paths: BTreeSet<&str> = [
        "crates/aura-app/src/ui_contract.rs",
        "crates/aura-terminal/src/tui/harness_state/snapshot.rs",
    ]
    .into();
    let harness_entrypoint_allowlist: BTreeSet<&str> = [
        "crates/aura-harness/src/backend/playwright_browser.rs",
        "crates/aura-harness/playwright-driver/src/playwright_driver.ts",
        "justfile",
        ".github/workflows/ci.yml",
        ".github/workflows/harness.yml",
        "toolkit/xtask/src/checks/policy.rs",
        "scripts/harness/run-matrix.sh",
        "docs/804_testing_guide.md",
        ".claude/skills/testing/SKILL.md",
        ".claude/skills/aura-quick-ref/SKILL.md",
        ".claude/skills/harness-run/SKILL.md",
    ]
    .into();
    let _shared_scenario_allowed_actions: BTreeSet<&str> = [
        "launch_actors",
        "screen_is",
        "create_account",
        "readiness_is",
        "open_screen",
        "start_device_enrollment",
        "runtime_event_occurred",
        "import_device_enrollment_code",
        "open_settings_section",
        "selection_is",
        "list_count_is",
        "remove_selected_device",
        "capture_current_authority_id",
        "create_contact_invitation",
        "accept_contact_invitation",
        "join_channel",
        "invite_actor_to_channel",
        "accept_pending_channel_invitation",
        "send_chat_message",
        "parity_with_actor",
    ]
    .into();

    let diff_output = command_stdout(
        "git",
        &[
            "diff".into(),
            "--unified=0".into(),
            "--no-color".into(),
            diff_range.clone(),
        ],
    )?;

    let mut current_file = String::new();
    let mut new_line: u64 = 0;

    for raw_line in diff_output.lines() {
        if let Some(rest) = raw_line.strip_prefix("+++ b/") {
            current_file = rest.to_owned();
            continue;
        }
        if raw_line == "+++ /dev/null" {
            current_file.clear();
            continue;
        }
        if let Some(rest) = raw_line.strip_prefix("@@ ") {
            // Parse `@@ -a,b +c[,d] @@`
            if let Some(plus_part) = rest.split_whitespace().nth(1) {
                let num_str = plus_part
                    .trim_start_matches('+')
                    .split(',')
                    .next()
                    .unwrap_or("0");
                new_line = num_str.parse().unwrap_or(0);
            }
            continue;
        }
        if current_file.is_empty() || !raw_line.starts_with('+') || raw_line.starts_with("+++") {
            continue;
        }

        let text = &raw_line[1..];

        // AURA_HARNESS_MODE outside allowlist
        if text.contains("AURA_HARNESS_MODE")
            && current_file.starts_with("crates/")
            && !current_file.starts_with("crates/aura-harness/")
            && !text.contains(r#"contains("AURA_HARNESS_MODE")"#)
            && !text.contains("assert!(!")
            && !allowlisted_harness_mode_files.contains(current_file.as_str())
        {
            eprintln!(
                "✖ {current_file}:{new_line}: new AURA_HARNESS_MODE branch outside allowlisted instrumentation surface"
            );
            violations += 1;
        }

        // Sleep/polling in parity-critical harness paths
        if sleep_guard_paths.contains(current_file.as_str())
            && (text.contains("thread::sleep")
                || text.contains("std::thread::sleep")
                || text.contains("tokio::time::sleep")
                || text.contains("recv_timeout")
                || text.contains("POLL_INTERVAL")
                || text.contains("poll_interval"))
        {
            eprintln!(
                "✖ {current_file}:{new_line}: new sleep/polling helper in parity-critical harness or export path"
            );
            violations += 1;
        }

        // Parity remap/normalization helpers
        if current_file.starts_with("crates/")
            && (text.contains("normalize_parity_")
                || text.contains("parity_normalize")
                || text.contains("parity_remap")
                || text.contains("normalize_parity")
                || text.contains("remap_parity"))
        {
            eprintln!("✖ {current_file}:{new_line}: new parity remap/normalization helper");
            violations += 1;
        }

        // Stringly-typed parity identifiers outside ui_contract
        if current_file.starts_with("crates/")
            && current_file != "crates/aura-app/src/ui_contract.rs"
            && (text.contains(r#"ScreenId(""#)
                || text.contains(r#"ModalId(""#)
                || text.contains(r#"ControlId(""#)
                || text.contains(r#"FieldId(""#)
                || text.contains(r#"ListId(""#)
                || text.contains(r#"OperationId(""#)
                || text.contains(r#"RuntimeEventId(""#)
                || text.contains(r#"ToastId(""#))
        {
            eprintln!(
                "✖ {current_file}:{new_line}: new stringly-typed parity identifier outside aura-app::ui_contract"
            );
            violations += 1;
        }

        // Row-index selection in parity paths
        if row_index_guard_paths.contains(current_file.as_str())
            && (text.contains("selected_idx")
                || text.contains("selected_by_index")
                || text.contains("selected_channel_index"))
        {
            eprintln!(
                "✖ {current_file}:{new_line}: new row-index selection/addressing in parity-critical export or contract code"
            );
            violations += 1;
        }

        // Legacy scenario dialect fields
        if current_file.starts_with("scenarios/harness/") && current_file.ends_with(".toml") {
            let trimmed = text.trim_start();
            if trimmed.starts_with("schema_version ")
                || trimmed.starts_with("execution_mode ")
                || trimmed.starts_with("required_capabilities ")
            {
                eprintln!(
                    "✖ {current_file}:{new_line}: new legacy scenario-dialect field in scenarios/harness"
                );
                violations += 1;
            }
        }

        // Non-allowlisted harness entrypoints
        if !harness_entrypoint_allowlist.contains(current_file.as_str())
            && !current_file.starts_with("scripts/check/")
            && (text.contains("just harness-run")
                || text.contains("just harness-run-browser")
                || text.contains("aura-harness -- run")
                || text.contains("cargo run -p aura-harness --bin aura-harness")
                || text.contains("window.__AURA_HARNESS__"))
        {
            eprintln!(
                "✖ {current_file}:{new_line}: new frontend-driving harness entry point outside approved owner files"
            );
            violations += 1;
        }

        // Raw String parity identifiers in TUI
        if (current_file.starts_with("crates/aura-terminal/src/tui/")
            || current_file == "crates/aura-terminal/src/handlers/tui.rs")
            && (text.contains("screen_id: String")
                || text.contains("modal_id: String")
                || text.contains("control_id: String")
                || text.contains("field_id: String")
                || text.contains("list_id: String")
                || text.contains("operation_id: String"))
        {
            eprintln!(
                "✖ {current_file}:{new_line}: new parity-critical TUI surface uses raw String identifier"
            );
            violations += 1;
        }

        new_line += 1;
    }

    if violations > 0 {
        bail!("user-flow-policy-guardrails: {violations} violation(s)");
    }

    println!("user-flow-policy-guardrails: clean");
    Ok(())
}

pub fn run_verification_coverage() -> Result<()> {
    // long-block-exception: multi-section verification coverage check comparing documented vs actual counts
    let repo_root = repo_root()?;
    let doc = repo_root.join("docs/998_verification_coverage.md");

    if !doc.exists() {
        bail!("verification-coverage: docs/998_verification_coverage.md not found");
    }

    let doc_src = read(&doc)?;
    let mut mismatches = 0u32;

    // Helper: extract documented value from summary table "| Metric | Count |"
    let get_documented = |metric: &str| -> Option<u64> {
        let pat = format!("| {metric} |");
        doc_src.lines().find(|l| l.contains(&pat)).and_then(|l| {
            let parts: Vec<&str> = l.split('|').collect();
            parts.get(2).and_then(|s| s.trim().parse().ok())
        })
    };

    // Helper: count files under a path matching extension
    let count_files = |path: &std::path::Path, ext: &str| -> u64 {
        if !path.exists() {
            return 0;
        }
        walkdir_count(path, ext)
    };

    fn walkdir_count(path: &std::path::Path, ext: &str) -> u64 {
        let Ok(entries) = std::fs::read_dir(path) else {
            return 0;
        };
        let mut count = 0u64;
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                count += walkdir_count(&p, ext);
            } else if p.extension().and_then(|s| s.to_str()) == Some(ext) {
                count += 1;
            }
        }
        count
    }

    let check_metric =
        |name: &str, documented: Option<u64>, actual: u64, mismatches: &mut u32| match documented {
            None => {
                eprintln!("  ? {name:<30} {actual:>4} (not documented)");
                *mismatches += 1;
            }
            Some(d) if d == actual => {
                println!("  ✓ {name:<30} {actual:>4}");
            }
            Some(d) => {
                let diff = actual as i64 - d as i64;
                eprintln!("  ✗ {name:<30} {actual:>4} (doc: {d}, diff: {diff:+})");
                *mismatches += 1;
            }
        };

    println!("Verification Coverage Check");
    println!("============================");
    println!();
    println!("Summary Metrics");
    println!("---------------");

    let quint_dir = repo_root.join("verification/quint");
    let lean_dir = repo_root.join("verification/lean");

    let quint_specs = count_files(&quint_dir, "qnt");
    check_metric(
        "Quint Specifications",
        get_documented("Quint Specifications"),
        quint_specs,
        &mut mismatches,
    );

    let quint_invariants = {
        command_stdout(
            "rg",
            &[
                "-rh".into(),
                r"^\s*val [A-Za-z]*[Ii]nvariant".into(),
                quint_dir.to_string_lossy().into_owned(),
            ],
        )
        .unwrap_or_default()
        .lines()
        .count() as u64
    };
    check_metric(
        "Quint Invariants",
        get_documented("Quint Invariants"),
        quint_invariants,
        &mut mismatches,
    );

    let quint_temporal = {
        command_stdout(
            "rg",
            &[
                "-rh".into(),
                r"^\s*temporal [a-zA-Z]".into(),
                quint_dir.to_string_lossy().into_owned(),
            ],
        )
        .unwrap_or_default()
        .lines()
        .count() as u64
    };
    check_metric(
        "Quint Temporal Properties",
        get_documented("Quint Temporal Properties"),
        quint_temporal,
        &mut mismatches,
    );

    let quint_types = {
        command_stdout(
            "rg",
            &[
                "-rh".into(),
                r"^\s*type ".into(),
                quint_dir.to_string_lossy().into_owned(),
            ],
        )
        .unwrap_or_default()
        .lines()
        .count() as u64
    };
    check_metric(
        "Quint Type Definitions",
        get_documented("Quint Type Definitions"),
        quint_types,
        &mut mismatches,
    );

    let lean_files = count_files(&lean_dir, "lean");
    check_metric(
        "Lean Source Files",
        get_documented("Lean Source Files"),
        lean_files,
        &mut mismatches,
    );

    let lean_theorems = {
        command_stdout(
            "rg",
            &[
                "-rh".into(),
                r"^(theorem|lemma) ".into(),
                lean_dir.to_string_lossy().into_owned(),
            ],
        )
        .unwrap_or_default()
        .lines()
        .count() as u64
    };
    check_metric(
        "Lean Theorems",
        get_documented("Lean Theorems"),
        lean_theorems,
        &mut mismatches,
    );

    let conformance_fixtures = count_files(
        &repo_root.join("crates/aura-testkit/fixtures/conformance"),
        "json",
    );
    check_metric(
        "Conformance Fixtures",
        get_documented("Conformance Fixtures"),
        conformance_fixtures,
        &mut mismatches,
    );

    let itf_harnesses = count_files(&quint_dir.join("harness"), "qnt");
    check_metric(
        "ITF Trace Harnesses",
        get_documented("ITF Trace Harnesses"),
        itf_harnesses,
        &mut mismatches,
    );

    let testkit_tests = {
        let testkit_src = repo_root.join("crates/aura-testkit/src");
        let testkit_tests_dir = repo_root.join("crates/aura-testkit/tests");
        let mut count = 0u64;
        for dir in [&testkit_src, &testkit_tests_dir] {
            count += command_stdout(
                "rg",
                &[
                    "-rh".into(),
                    r"^#\[test\]".into(),
                    dir.to_string_lossy().into_owned(),
                ],
            )
            .unwrap_or_default()
            .lines()
            .count() as u64;
        }
        count
    };
    check_metric(
        "Testkit Tests",
        get_documented("Testkit Tests"),
        testkit_tests,
        &mut mismatches,
    );

    // Only count bridge_*.rs files
    let bridge_modules = {
        let dir = repo_root.join("crates/aura-quint/src");
        if dir.exists() {
            std::fs::read_dir(&dir)
                .ok()
                .map(|entries| {
                    entries
                        .flatten()
                        .filter(|e| {
                            e.file_name().to_string_lossy().starts_with("bridge_")
                                && e.path().extension().and_then(|s| s.to_str()) == Some("rs")
                        })
                        .count() as u64
                })
                .unwrap_or(0)
        } else {
            0
        }
    };
    let _ = bridge_modules; // suppress unused warning from first binding
    check_metric(
        "Bridge Modules",
        get_documented("Bridge Modules"),
        bridge_modules,
        &mut mismatches,
    );

    let telltale_parity_modules = {
        let dir = repo_root.join("crates/aura-simulator/src");
        if dir.exists() {
            std::fs::read_dir(&dir)
                .ok()
                .map(|e| {
                    e.flatten()
                        .filter(|e| e.file_name() == "telltale_parity.rs")
                        .count() as u64
                })
                .unwrap_or(0)
        } else {
            0
        }
    };
    check_metric(
        "Telltale Parity Modules",
        get_documented("Telltale Parity Modules"),
        telltale_parity_modules,
        &mut mismatches,
    );

    let bridge_pipeline_fixtures = count_files(
        &repo_root.join("crates/aura-quint/tests/fixtures/bridge"),
        "json",
    );
    check_metric(
        "Bridge Pipeline Fixtures",
        get_documented("Bridge Pipeline Fixtures"),
        bridge_pipeline_fixtures,
        &mut mismatches,
    );

    // CI gates check
    let justfile = repo_root.join("justfile");
    let ci_gate_names = [
        "ci-property-monitor:",
        "ci-simulator-telltale-parity:",
        "ci-choreo-parity:",
        "ci-quint-typecheck:",
        "ci-conformance-policy:",
        "ci-conformance-contracts:",
        "ci-lean-build:",
        "ci-lean-check-sorry:",
        "ci-telltale-bridge:",
        "ci-kani:",
    ];
    let justfile_src = if justfile.exists() {
        read(&justfile)?
    } else {
        String::new()
    };
    let ci_gate_count = ci_gate_names
        .iter()
        .filter(|&&g| justfile_src.contains(g))
        .count() as u64
        + 1;
    check_metric(
        "CI Verification Gates",
        get_documented("CI Verification Gates"),
        ci_gate_count,
        &mut mismatches,
    );

    println!();

    // Bridge modules existence check
    let bridge_ok: Vec<&str> = [
        "bridge_export",
        "bridge_import",
        "bridge_format",
        "bridge_validate",
    ]
    .iter()
    .filter(|&&m| {
        repo_root
            .join(format!("crates/aura-quint/src/{m}.rs"))
            .exists()
    })
    .copied()
    .collect();
    let bridge_missing = 4 - bridge_ok.len();
    if bridge_missing > 0 {
        eprintln!("  ✗ bridge modules: {}/{} found", bridge_ok.len(), 4);
        mismatches += 1;
    } else {
        println!("  ✓ All {} bridge modules found", bridge_ok.len());
    }

    // Schema references check
    let check_schema =
        |schema: &str, source_file: &std::path::Path, label: &str, mismatches: &mut u32| {
            let in_doc = doc_src.contains(schema);
            let in_source = source_file.exists()
                && read(source_file)
                    .map(|s| s.contains(schema))
                    .unwrap_or(false);
            if in_doc && in_source {
                println!("  ✓ {label}");
            } else {
                eprintln!("  ✗ {label} (doc={in_doc}, source={in_source})");
                *mismatches += 1;
            }
        };

    check_schema(
        "aura.telltale-parity.report.v1",
        &repo_root.join("crates/aura-simulator/src/telltale_parity.rs"),
        "Telltale parity report schema documented and implemented",
        &mut mismatches,
    );
    check_schema(
        "aura.telltale-bridge.discrepancy.v1",
        &repo_root.join("crates/aura-quint/tests/bridge_pipeline.rs"),
        "Bridge discrepancy schema documented and implemented",
        &mut mismatches,
    );

    println!();
    if mismatches > 0 {
        bail!(
            "verification-coverage: {mismatches} check(s) failed. Update docs/998_verification_coverage.md to match actual counts."
        );
    }

    println!("verification-coverage: PASSED");
    Ok(())
}

pub fn run_shared_flow_policy() -> Result<()> {
    run_harness_core_scenario_mechanics()?;
    run_harness_ui_state_evented()?;
    run_harness_ui_parity_contract()?;
    run_harness_shared_scenario_contract()?;
    run_harness_scenario_legality()?;
    run_harness_scenario_shape_contract()?;
    run_harness_trace_determinism()?;
    run_harness_recovery_contract()?;
    run_harness_settings_surface_contract()?;
    run_browser_observation_contract()?;
    run_browser_driver_types()?;
    run_harness_scenario_inventory()?;
    run_harness_command_plane_boundary()?;
    run_harness_row_index_contract()?;
    run_harness_scenario_config_boundary()?;
    run_harness_observation_determinism()?;
    run_harness_observation_surface()?;
    run_harness_action_preconditions()?;
    run_harness_mode_allowlist()?;
    run_harness_render_convergence()?;
    run_harness_focus_selection_contract()?;
    run_harness_revision_contract()?;
    run_harness_wait_contract()?;
    run_harness_semantic_primitive_contract()?;
    run_harness_backend_contract()?;
    run_harness_raw_backend_quarantine()?;
    run_harness_onboarding_publication()?;
    run_harness_runtime_events_authoritative()?;
    run_harness_export_override_policy()?;
    run_harness_bridge_contract()?;
    run_browser_cache_owner()?;
    run_browser_cache_lifecycle()?;
    run_privacy_runtime_locality()?;
    run_privacy_legacy_sweep()?;
    run_shared_flow_metadata()?;
    run_shared_intent_flow()?;
    run_shared_raw_quarantine()?;
    run_tui_semantic_snapshot()?;
    run_shared_semantic_dedup()?;
    run_tui_product_path()?;
    run_tui_observation_channel()?;
    run_tui_selection_contract()?;
    println!("shared flow policy: clean");
    Ok(())
}
