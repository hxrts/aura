use std::{
    collections::{BTreeSet, HashSet},
    fs,
    path::Path,
};

use anyhow::{bail, Context, Result};
use regex::Regex;

use super::support::{
    command_stdout, contains, first_match_line, git_diff, read, read_lines, repo_relative,
    repo_root, rg_exists, rg_lines, rg_non_comment_lines, run_ok, run_ok_in_dir,
    rust_files_under,
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
        bail!("service-registry-ownership: legacy duplicate rendezvous cache ownership paths are still present");
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
        bail!("service-registry-ownership: duplicate runtime descriptor stores detected outside service_registry/rendezvous_manager");
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
        bail!("typed-error-boundary: parity-critical workflow/runtime paths still use stringly primary error construction");
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
        bail!("ownership-category-declarations: crates are missing required ARCHITECTURE.md ownership declarations");
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
        println!("ownership-annotation-ratchet({mode}): no diff in scope; completeness clean (0 named exclusions)");
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
        bail!("runtime-shutdown-order: reactive pipeline must shut down before task tree cancellation");
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
        bail!("adaptive-privacy-runtime-locality: LocalSelectionProfile must remain runtime-local, not an authoritative service-surface object");
    }
    if !rg_exists(&vec![
        "-n".into(),
        r#"authoritative = """#.into(),
        repo_relative(&selection_manager),
    ])? {
        bail!("adaptive-privacy-runtime-locality: selection_manager service_surface must declare an empty authoritative set");
    }
    if !rg_exists(&vec![
        "-n".into(),
        r#"runtime_local = ".*selection_profiles.*""#.into(),
        repo_relative(&selection_manager),
    ])? {
        bail!("adaptive-privacy-runtime-locality: selection_manager service_surface must declare selection profiles as runtime-local state");
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
        bail!("adaptive-privacy-runtime-locality: LocalSelectionProfile must not escape the runtime-owned selection service surface");
    }
    if !rg_exists(&vec![
        "-n".into(),
        "SelectionState".into(),
        repo_relative(&registry),
    ])? {
        bail!("adaptive-privacy-runtime-locality: service_registry must store SelectionState snapshots for sanctioned runtime-local queries");
    }
    let agent_arch_contents = read(&agent_arch)?;
    if !agent_arch_contents.contains(
        "Adaptive privacy runtime-owned services include `SelectionManager`, `LocalHealthObserver`, `CoverTrafficGenerator`, and `AnonymousPathManager`",
    ) {
        bail!("adaptive-privacy-runtime-locality: aura-agent ARCHITECTURE.md must document the adaptive privacy runtime-owned service set");
    }
    if !agent_arch_contents.contains("`LocalSelectionProfile` is runtime-local") {
        bail!("adaptive-privacy-runtime-locality: aura-agent ARCHITECTURE.md must state that LocalSelectionProfile is runtime-local");
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
        bail!("adaptive-privacy-phase5-legacy-sweep: legacy non-runtime selection ownership paths must be removed");
    }

    if rg_exists(&vec![
        "-n".into(),
        "upcoming runtime/app integration|upcoming.*land".into(),
        repo_relative(repo_root.join("crates/aura-agent/src/runtime/services/mod.rs")),
    ])? {
        bail!("adaptive-privacy-phase5-legacy-sweep: transitional transparent-envelope scaffolding comments must be removed from runtime service exports");
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
        bail!("adaptive-privacy-phase5-legacy-sweep: transparent anonymous setup objects must stay scoped to aura-core service types and the runtime path manager");
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
            bail!("adaptive-privacy-phase5-legacy-sweep: shared transparent move envelope must carry {traffic_class} traffic");
        }
    }

    let cover = repo_root.join("crates/aura-agent/src/runtime/services/cover_traffic_generator.rs");
    if !contains(&cover, "MoveEnvelope::opaque")? {
        bail!("adaptive-privacy-phase5-legacy-sweep: cover traffic planning must stay on the shared Move envelope substrate");
    }
    if contains(&cover, "TransportEnvelope")? {
        bail!("adaptive-privacy-phase5-legacy-sweep: cover traffic planning must not bypass the shared Move envelope substrate");
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
        bail!("adaptive-privacy-phase5-legacy-sweep: runtime adaptive-privacy services must not reintroduce implicit route setup or direct transport fallback");
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
            bail!("adaptive-privacy-phase5-legacy-sweep: runtime adaptive-privacy services still reference legacy transport assumption: {legacy_pattern}");
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
        bail!("harness-typed-semantic-errors: parity-critical shared semantic paths still rely on stringly error construction");
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
        bail!("harness-typed-json-boundary: shared semantic core still relies on raw serde_json::Value plumbing");
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
        bail!("harness-authoritative-fact-boundary: frontend-facing modules are handling authoritative semantic facts outside approved boundaries");
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
        bail!("observed-layer-authorship: observed UI modules may not author authoritative semantic truth");
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
        bail!("browser semantic restart boundary: legacy restart seed payload plumbing is still present in the Playwright driver");
    }
    let submit_body = extract_ts_function(&contents, "async function submitSemanticCommand(params)")
        .context("browser semantic restart boundary: could not locate submitSemanticCommand in Playwright driver")?;
    let runtime_body = extract_ts_function(&contents, "async function stageRuntimeIdentity(params)")
        .context("browser semantic restart boundary: could not locate stageRuntimeIdentity in Playwright driver")?;
    if submit_body.contains("restartPageSession(") {
        bail!("browser semantic restart boundary: submitSemanticCommand must fail closed instead of replaying through restartPageSession");
    }
    if !submit_body.contains("submit_semantic_command_enqueue_failed_closed") {
        bail!("browser semantic restart boundary: submitSemanticCommand no longer exposes an explicit fail-closed semantic enqueue path");
    }
    if runtime_body.contains("restartPageSession(") {
        bail!("browser semantic restart boundary: stageRuntimeIdentity must fail closed instead of replaying through restartPageSession");
    }
    if !runtime_body.contains("stage_runtime_identity_enqueue_failed_closed") {
        bail!("browser semantic restart boundary: stageRuntimeIdentity no longer exposes an explicit fail-closed runtime-stage enqueue path");
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
    run_harness_ownership_policy()?;
    run_browser_restart_boundary()?;
    run_testing_exception_boundary()?;
    println!("ownership policy: clean");
    Ok(())
}

fn run_runtime_typed_lifecycle_bridge() -> Result<()> {
    super::runtime_typed_lifecycle_bridge::run()
}

fn run_privacy_onion_quarantine() -> Result<()> {
    let repo_root = repo_root()?;
    let lane_files = [
        ".github/workflows/ci.yml",
        ".github/workflows/harness.yml",
        "Justfile",
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
        bail!("transparent-onion-quarantine: harness and shared-flow lanes must not enable or depend on transparent_onion");
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
        bail!("transparent-onion-quarantine: transparent debug surfaces must remain quarantined to the explicit allowlist");
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

fn run_head_shell_script(script: &str, extra_args: &[String]) -> Result<()> {
    let repo_root = repo_root()?;
    let script_path = repo_root.join(script);
    let created = if script_path.exists() {
        false
    } else {
        let contents = command_stdout("git", &["show".into(), format!("HEAD:{script}")])?;
        if let Some(parent) = script_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
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
    run_ok("node", &["--version".into()]).context("harness-browser-toolchain: node not found in PATH")?;
    run_ok("npm", &["--version".into()]).context("harness-browser-toolchain: npm not found in PATH")?;

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
    run_head_shell_script("scripts/check/browser-cache-lifecycle.sh", &[])
}

pub fn run_browser_cache_owner() -> Result<()> {
    run_head_shell_script("scripts/check/browser-cache-owner.sh", &[])
}

pub fn run_browser_driver_contract_sync() -> Result<()> {
    run_head_shell_script("scripts/check/browser-driver-contract.sh", &[])
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
        bail!("harness-browser-driver-types: stable wrapper does not delegate to the driver loader");
    }
    if !contains(&wrapper, "ensureCompiledDriverFresh")?
        || !contains(&wrapper, "pathToFileURL(compiledDriver).href")?
    {
        bail!("harness-browser-driver-types: stable wrapper does not load the compiled TS driver through the freshness loader");
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
    run_head_shell_script("scripts/check/browser-observation-recovery.sh", &[])
}

pub fn run_harness_action_preconditions() -> Result<()> {
    run_head_shell_script("scripts/check/harness-action-preconditions.sh", &[])
}

pub fn run_harness_backend_contract() -> Result<()> {
    run_head_shell_script("scripts/check/harness-backend-contract.sh", &[])
}

pub fn run_harness_boundary_policy() -> Result<()> {
    run_head_shell_script("scripts/check/harness-boundary-policy.sh", &[])
}

pub fn run_harness_bridge_contract() -> Result<()> {
    run_head_shell_script("scripts/check/harness-bridge-contract.sh", &[])
}

pub fn run_harness_command_plane_boundary() -> Result<()> {
    run_head_shell_script("scripts/check/harness-command-plane-boundary.sh", &[])
}

pub fn run_harness_conformance_gate() -> Result<()> {
    run_head_shell_script("scripts/check/harness-conformance-gate.sh", &[])
}

pub fn run_harness_core_scenario_mechanics() -> Result<()> {
    run_harness_governance_check("core-scenario-mechanics")
}

pub fn run_harness_export_override_policy() -> Result<()> {
    run_head_shell_script("scripts/check/harness-export-override-policy.sh", &[])
}

pub fn run_harness_focus_selection_contract() -> Result<()> {
    run_head_shell_script("scripts/check/harness-focus-selection-contract.sh", &[])
}

pub fn run_harness_matrix_inventory() -> Result<()> {
    run_head_shell_script("scripts/check/harness-matrix-inventory.sh", &[])
}

pub fn run_harness_mode_allowlist() -> Result<()> {
    run_head_shell_script("scripts/check/harness-mode-allowlist.sh", &[])
}

pub fn run_harness_observation_determinism() -> Result<()> {
    run_head_shell_script("scripts/check/harness-observation-determinism.sh", &[])
}

pub fn run_harness_observation_surface() -> Result<()> {
    run_head_shell_script("scripts/check/harness-observation-surface.sh", &[])
}

pub fn run_harness_onboarding_contract() -> Result<()> {
    run_head_shell_script("scripts/check/harness-onboarding-contract.sh", &[])
}

pub fn run_harness_onboarding_publication() -> Result<()> {
    run_head_shell_script("scripts/check/harness-onboarding-publication.sh", &[])
}

pub fn run_harness_raw_backend_quarantine() -> Result<()> {
    run_head_shell_script("scripts/check/harness-raw-backend-quarantine.sh", &[])
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
    run_head_shell_script("scripts/check/harness-render-convergence.sh", &[])
}

pub fn run_harness_revision_contract() -> Result<()> {
    run_head_shell_script("scripts/check/harness-revision-contract.sh", &[])
}

pub fn run_harness_row_index_contract() -> Result<()> {
    run_head_shell_script("scripts/check/harness-row-index-contract.sh", &[])
}

pub fn run_harness_runtime_events_authoritative() -> Result<()> {
    run_head_shell_script("scripts/check/harness-runtime-events-authoritative.sh", &[])
}

pub fn run_harness_scenario_config_boundary() -> Result<()> {
    run_head_shell_script("scripts/check/harness-scenario-config-boundary.sh", &[])
}

pub fn run_harness_scenario_inventory() -> Result<()> {
    run_head_shell_script("scripts/check/harness-scenario-inventory.sh", &[])
}

pub fn run_harness_scenario_legality() -> Result<()> {
    run_harness_governance_check("scenario-legality")
}

pub fn run_harness_scenario_shape_contract() -> Result<()> {
    run_harness_governance_check("scenario-shape-contract")
}

pub fn run_harness_semantic_primitive_contract() -> Result<()> {
    run_head_shell_script("scripts/check/harness-semantic-primitive-contract.sh", &[])
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
    run_head_shell_script("scripts/check/harness-wait-contract.sh", &[])
}

pub fn run_ownership_capability_audit() -> Result<()> {
    run_head_shell_script("scripts/check/ownership-capability-audit.sh", &[])
}

pub fn run_privacy_tuning_gate() -> Result<()> {
    let repo_root = repo_root()?;
    let artifact_root = std::env::var("AURA_ADAPTIVE_PRIVACY_ARTIFACT_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| repo_root.join("artifacts/adaptive-privacy/phase6"));
    if artifact_root.exists() {
        fs::remove_dir_all(&artifact_root)
            .with_context(|| format!("adaptive-privacy-phase6: removing {}", artifact_root.display()))?;
    }
    fs::create_dir_all(&artifact_root)
        .with_context(|| format!("adaptive-privacy-phase6: creating {}", artifact_root.display()))?;

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
    run_head_shell_script("scripts/check/protocol-device-id-legacy.sh", args)
}

pub fn run_runtime_bootstrap_guardrails() -> Result<()> {
    run_head_shell_script("scripts/check/runtime-bootstrap-guardrails.sh", &[])
}

pub fn run_shared_flow_metadata() -> Result<()> {
    run_head_shell_script("scripts/check/shared-flow-metadata.sh", &[])
}

pub fn run_shared_intent_flow() -> Result<()> {
    run_head_shell_script("scripts/check/shared-intent-flow.sh", &[])
}

pub fn run_shared_raw_quarantine() -> Result<()> {
    run_head_shell_script("scripts/check/shared-raw-quarantine.sh", &[])
}

pub fn run_shared_semantic_dedup() -> Result<()> {
    run_head_shell_script("scripts/check/shared-semantic-dedup.sh", &[])
}

pub fn run_tui_observation_channel() -> Result<()> {
    run_head_shell_script("scripts/check/tui-observation-channel.sh", &[])
}

pub fn run_tui_product_path() -> Result<()> {
    run_head_shell_script("scripts/check/tui-product-path.sh", &[])
}

pub fn run_tui_selection_contract() -> Result<()> {
    run_head_shell_script("scripts/check/tui-selection-contract.sh", &[])
}

pub fn run_tui_semantic_snapshot() -> Result<()> {
    run_head_shell_script("scripts/check/tui-semantic-snapshot.sh", &[])
}

pub fn run_user_flow_coverage() -> Result<()> {
    run_harness_governance_check("user-flow-coverage")
}

pub fn run_user_flow_guidance_sync() -> Result<()> {
    run_head_shell_script("scripts/check/user-flow-guidance-sync.sh", &[])
}

pub fn run_user_flow_policy_guardrails() -> Result<()> {
    run_head_shell_script("scripts/check/user-flow-policy-guardrails.sh", &[])
}

pub fn run_verification_coverage() -> Result<()> {
    run_head_shell_script("scripts/check/verification-coverage.sh", &[])
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
