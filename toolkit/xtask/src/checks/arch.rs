use std::{collections::BTreeMap, path::Path};

use anyhow::{bail, Context, Result};
use regex::Regex;
use serde_json::Value;
use syn::{spanned::Spanned, visit::Visit};

use super::support::{
    command_stdout, dirs_under, read, read_lines, repo_relative, repo_root, rg_exists, rg_lines,
    rg_non_comment_lines, rust_files_under,
};

pub fn run(args: &[String]) -> Result<()> {
    let config = ArchConfig::parse(args)?;
    let repo_root = repo_root()?;
    let mut audit = ArchAudit::default();

    if config.run_all || config.layers {
        check_layers(&repo_root, &mut audit)?;
    }
    if config.run_all || config.effects {
        check_effects(&repo_root, &mut audit)?;
    }
    if config.run_all || config.invariants {
        check_invariants(&repo_root, &mut audit)?;
    }
    if config.run_all || config.reactive {
        check_reactive(&repo_root, &mut audit)?;
    }
    if config.run_all || config.ceremonies {
        check_ceremonies(&repo_root, &mut audit)?;
    }
    if config.run_all || config.ui {
        check_ui(&repo_root, &mut audit)?;
    }
    if config.run_all || config.workflows {
        check_workflows(&repo_root, &mut audit)?;
    }
    if config.run_all || config.serialization {
        check_serialization(&repo_root, &mut audit)?;
    }
    if config.run_all || config.style {
        check_style(&repo_root, &mut audit)?;
    }
    if config.run_all || config.test_seeds {
        check_test_seeds(&repo_root, &mut audit)?;
    }
    if config.run_all || config.todos {
        check_todos(&repo_root, &mut audit)?;
    }

    if audit.violations.is_empty() {
        println!("arch: clean");
        return Ok(());
    }

    for violation in &audit.violations {
        eprintln!("✖ {violation}");
    }
    bail!("arch: {} violation(s)", audit.violations.len())
}

#[derive(Default)]
struct ArchConfig {
    run_all: bool,
    layers: bool,
    effects: bool,
    invariants: bool,
    reactive: bool,
    ceremonies: bool,
    ui: bool,
    workflows: bool,
    serialization: bool,
    style: bool,
    test_seeds: bool,
    todos: bool,
}

impl ArchConfig {
    fn parse(args: &[String]) -> Result<Self> {
        let mut config = Self {
            run_all: true,
            ..Self::default()
        };

        let mut idx = 0;
        while idx < args.len() {
            match args[idx].as_str() {
                "--layers" | "--deps" => {
                    config.run_all = false;
                    config.layers = true;
                }
                "--effects" | "--crypto" | "--concurrency" => {
                    config.run_all = false;
                    config.effects = true;
                }
                "--invariants" => {
                    config.run_all = false;
                    config.invariants = true;
                }
                "--reactive" => {
                    config.run_all = false;
                    config.reactive = true;
                }
                "--ceremonies" => {
                    config.run_all = false;
                    config.ceremonies = true;
                }
                "--ui" => {
                    config.run_all = false;
                    config.ui = true;
                }
                "--workflows" => {
                    config.run_all = false;
                    config.workflows = true;
                }
                "--serialization" => {
                    config.run_all = false;
                    config.serialization = true;
                }
                "--style" => {
                    config.run_all = false;
                    config.style = true;
                }
                "--test-seeds" => {
                    config.run_all = false;
                    config.test_seeds = true;
                }
                "--todos" => {
                    config.run_all = false;
                    config.todos = true;
                }
                "--quick" => {
                    config.run_all = false;
                    config.layers = true;
                    config.effects = true;
                    config.invariants = true;
                    config.reactive = true;
                    config.ceremonies = true;
                    config.workflows = true;
                    config.serialization = true;
                    config.style = true;
                    config.test_seeds = true;
                }
                "-v" | "--verbose" => {}
                "--layer" => {
                    idx += 1;
                    if idx >= args.len() {
                        bail!("arch: --layer requires an argument")
                    }
                }
                flag => bail!("arch: unknown flag {flag}"),
            }
            idx += 1;
        }

        Ok(config)
    }
}

#[derive(Default)]
struct ArchAudit {
    violations: Vec<String>,
}

impl ArchAudit {
    fn push(&mut self, message: impl Into<String>) {
        self.violations.push(message.into());
    }

    fn push_matches(&mut self, label: &str, hits: impl IntoIterator<Item = String>) {
        for hit in hits {
            self.push(format!("{label}: {hit}"));
        }
    }
}

fn check_layers(repo_root: &Path, audit: &mut ArchAudit) -> Result<()> {
    let aura_core_impls = rg_non_comment_lines(&[
        "-n".into(),
        r"\bimpl\b.*Effects".into(),
        repo_relative(repo_root.join("crates/aura-core/src")),
        "-g".into(),
        "*.rs".into(),
    ])?;
    for hit in filtered_test_module_hits(repo_root, aura_core_impls)? {
        if hit.contains("trait") || hit.contains("impl<") || hit.contains("ScriptedTimeEffects") {
            continue;
        }
        audit.push(format!(
            "arch(layers): aura-core contains effect implementation syntax: {hit}"
        ));
    }

    for crate_name in [
        "aura-authentication",
        "aura-app",
        "aura-chat",
        "aura-invitation",
        "aura-recovery",
        "aura-relational",
        "aura-rendezvous",
        "aura-sync",
    ] {
        let cargo_toml = repo_root.join("crates").join(crate_name).join("Cargo.toml");
        if !cargo_toml.exists() {
            continue;
        }
        let contents = read(&cargo_toml)?;
        if contents.contains("aura-agent")
            || contents.contains("aura-simulator")
            || contents.contains("aura-terminal")
        {
            audit.push(format!(
                "arch(layers): {crate_name} depends on runtime/UI layers ({})",
                cargo_toml.display()
            ));
        }
    }

    check_dependency_direction(audit)?;

    for crate_name in [
        "aura-protocol",
        "aura-guards",
        "aura-consensus",
        "aura-amp",
        "aura-anti-entropy",
    ] {
        let cargo_toml = repo_root.join("crates").join(crate_name).join("Cargo.toml");
        if !cargo_toml.exists() {
            continue;
        }
        let contents = read(&cargo_toml)?;
        for forbidden in [
            "aura-agent",
            "aura-simulator",
            "aura-app",
            "aura-terminal",
            "aura-testkit",
        ] {
            if contents.contains(forbidden) {
                audit.push(format!(
                    "arch(layers): {crate_name} depends on forbidden L6+ crate {forbidden}"
                ));
            }
        }
    }

    Ok(())
}

fn check_dependency_direction(audit: &mut ArchAudit) -> Result<()> {
    let stdout = command_stdout(
        "cargo",
        &[
            "metadata".into(),
            "--format-version".into(),
            "1".into(),
            "--no-deps".into(),
        ],
    )?;
    let metadata: Value = serde_json::from_str(&stdout).context("parsing cargo metadata")?;

    let mut layers = BTreeMap::new();
    let mut deps = Vec::new();

    for package in metadata["packages"]
        .as_array()
        .context("cargo metadata missing packages")?
    {
        let name = package["name"].as_str().unwrap_or_default();
        if !name.starts_with("aura-") {
            continue;
        }
        let canonical = name.to_string();
        let layer = layer_of(&canonical);
        if layer == 0 {
            continue;
        }
        layers.insert(canonical.clone(), layer);
        if let Some(package_deps) = package["dependencies"].as_array() {
            for dep in package_deps {
                let dep_name = dep["name"].as_str().unwrap_or_default();
                if !dep_name.starts_with("aura-") {
                    continue;
                }
                let dep_canonical = dep_name.to_string();
                if layer_of(&dep_canonical) > 0 {
                    deps.push((canonical.clone(), dep_canonical));
                }
            }
        }
    }

    for (src, dst) in deps {
        let src_layer = *layers.get(&src).unwrap_or(&0);
        let dst_layer = layer_of(&dst);
        let allowlisted = matches!(
            (src.as_str(), dst.as_str()),
            ("aura-simulator", "aura-quint")
                | ("aura-simulator", "aura-testkit")
                | ("aura-terminal", "aura-testkit")
        );
        if src_layer > 0 && dst_layer > src_layer {
            if allowlisted {
                continue;
            }
            audit.push(format!(
                "arch(layers): {src} (L{src_layer}) depends upward on {dst} (L{dst_layer})"
            ));
        }
    }

    Ok(())
}

fn check_effects(repo_root: &Path, audit: &mut ArchAudit) -> Result<()> {
    audit.push_matches(
        "arch(effects): ad hoc VM bridge queue/state storage outside approved implementations",
        filtered_test_module_hits(
            repo_root,
            rg_non_comment_lines(&["-n".into(),
                "Mutex<.*VmBridgePendingSend|Mutex<.*VmBridgeBlockedEdge|Mutex<.*VmBridgeSchedulerSignals|VecDeque<.*VmBridgePendingSend|VecDeque<.*VmBridgeBlockedEdge|VecDeque<.*VmBridgeSchedulerSignals".into(),
                repo_relative(repo_root.join("crates/aura-agent/src")),
                repo_relative(repo_root.join("crates/aura-testkit/src")),
                "-g".into(),
                "*.rs".into()])?,
        )?
        .into_iter()
        .filter(|hit| {
            !hit.contains("crates/aura-agent/src/runtime/subsystems/vm_bridge.rs")
                && !hit.contains("crates/aura-testkit/src/stateful_effects/vm_bridge.rs")
        }),
    );

    audit.push_matches(
        "arch(effects): threaded/envelope runtime selection outside hardening/engine paths",
        filtered_test_module_hits(
            repo_root,
            rg_non_comment_lines(&["-n".into(),
                "ThreadedVM::with_workers|AuraVmRuntimeMode::ThreadedReplayDeterministic|AuraVmRuntimeMode::ThreadedEnvelopeBounded".into(),
                repo_relative(repo_root.join("crates/aura-agent/src")),
                "-g".into(),
                "*.rs".into()])?,
        )?
        .into_iter()
        .filter(|hit| {
            !hit.contains("crates/aura-agent/src/runtime/choreo_engine.rs")
                && !hit.contains("crates/aura-agent/src/runtime/vm_hardening.rs")
        }),
    );

    audit.push_matches(
        "arch(effects): OTA code assumes a network-wide authoritative cutover model",
        filtered_test_module_hits(
            repo_root,
            rg_non_comment_lines(
                &{
                    let mut args = vec![
                        "-n".into(),
                        "GlobalNetwork|NetworkWide|network-wide authoritative cutover|global cutover|whole Aura network.*cutover".into(),
                    ];
                    for path in [
                        repo_root.join("crates/aura-maintenance/src"),
                        repo_root.join("crates/aura-sync/src/services"),
                        repo_root.join("crates/aura-agent/src/runtime/services/ota_manager.rs"),
                    ] {
                        if path.exists() {
                            args.push(repo_relative(path));
                        }
                    }
                    args.push("-g".into());
                    args.push("*.rs".into());
                    args
                },
            )?,
        )?,
    );

    let registry_file = repo_root.join("crates/aura-core/src/conformance.rs");
    if !registry_file.exists() {
        audit.push(
            "arch(effects): missing conformance registry file crates/aura-core/src/conformance.rs",
        );
    } else {
        let registry = read(&registry_file)?;
        let classification_re = Regex::new(r#"^\s*\("([^"]+)",\s*AuraEnvelopeLawClass::"#).unwrap();
        let mut kinds = Vec::new();
        for line in registry.lines() {
            if let Some(caps) = classification_re.captures(line) {
                kinds.push(caps[1].to_string());
            }
        }
        if kinds.is_empty() {
            audit.push(
                "arch(effects): no classified effect envelope kinds found in conformance registry",
            );
        }
        kinds.sort();
        for window in kinds.windows(2) {
            if window[0] == window[1] {
                audit.push(format!(
                    "arch(effects): duplicate effect envelope classification {}",
                    window[0]
                ));
            }
        }
    }

    audit.push_matches(
        "arch(effects): synchronous guard/effect bridge remains",
        filtered_test_module_hits(
            repo_root,
            rg_non_comment_lines(&[
                "-n".into(),
                "GuardEffectSystem|futures::executor::block_on".into(),
                repo_relative(repo_root.join("crates")),
                "-g".into(),
                "*.rs".into(),
            ])?,
        )?
        .into_iter()
        .filter(|hit| {
            !hit.contains("crates/aura-app/src/frontend_primitives/submitted_operation.rs")
                && !hit.contains("crates/aura-terminal/src/tui/semantic_lifecycle.rs")
                && !hit.contains("crates/aura-ui/src/semantic_lifecycle.rs")
        }),
    );

    Ok(())
}

fn check_invariants(repo_root: &Path, audit: &mut ArchAudit) -> Result<()> {
    let arch_files = rg_lines(&[
        "--files".into(),
        repo_relative(repo_root.join("crates")),
        "-g".into(),
        "ARCHITECTURE.md".into(),
    ])?;
    if arch_files.is_empty() {
        audit.push("arch(invariants): no crate ARCHITECTURE.md files found");
        return Ok(());
    }

    let mut with_invariants = 0usize;
    for file in arch_files {
        let path = repo_root.join(&file);
        let contents = read(&path)?;
        if contents.contains("## Invariants") {
            with_invariants += 1;
        }
        if contents.contains("### Detailed Specifications")
            || contents.contains("## Detailed Invariant Specifications")
            || contents.contains("### Invariant")
        {
            for field in ["Enforcement locus:", "Failure mode:", "Verification hooks:"] {
                if !contents.contains(field) {
                    audit.push(format!(
                        "arch(invariants): missing detailed invariant field `{field}` in {file}"
                    ));
                }
            }
        }
    }
    if with_invariants == 0 {
        audit.push("arch(invariants): no crate ARCHITECTURE.md includes an Invariants section");
    }
    Ok(())
}

fn check_reactive(repo_root: &Path, audit: &mut ArchAudit) -> Result<()> {
    audit.push_matches(
        "arch(reactive): domain data marker remains in TUI props",
        rg_lines(&[
            "-l".into(),
            "// === Domain data".into(),
            repo_relative(repo_root.join("crates/aura-terminal/src/tui/screens")),
            "-g".into(),
            "*.rs".into(),
        ])?,
    );

    let screens_dir = repo_root.join("crates/aura-terminal/src/tui/screens");
    for file in rust_files_under(&screens_dir)
        .into_iter()
        .filter(|path| path.file_name().and_then(|name| name.to_str()) == Some("screen.rs"))
    {
        let contents = read(&file)?;
        if !contents.contains("subscribe_signal_with_retry") && !contents.contains("SIGNAL") {
            audit.push(format!(
                "arch(reactive): screen without signal subscription: {}",
                repo_relative(file)
            ));
        }
    }

    let commit_files = rg_lines(&[
        "-l".into(),
        "commit_generic_fact_bytes".into(),
        repo_relative(repo_root.join("crates")),
        "-g".into(),
        "*.rs".into(),
    ])?;
    for file in commit_files {
        let path = repo_root.join(&file);
        if path
            .components()
            .any(|component| component.as_os_str() == "tests")
        {
            continue;
        }
        for line in unacknowledged_semantic_commits(&read(&path)?)? {
            audit.push(format!(
                "arch(reactive): direct generic fact commit in semantic owner at {file}:{line}; use required commit and processing capability"
            ));
        }
    }
    Ok(())
}

/// Local declaration boundary fence. Durable actor publication is not terminal
/// view evidence. Interprocedural ownership and capability consumption remain
/// governed by the declaration ratchet and typed ownership API gates.
fn unacknowledged_semantic_commits(source: &str) -> Result<Vec<usize>> {
    struct Commits<'source> {
        source: &'source str,
        owner: bool,
        lines: Vec<usize>,
        failure: Option<anyhow::Error>,
    }
    fn declares_owner(attributes: &[syn::Attribute]) -> bool {
        attributes.iter().any(|attribute| {
            attribute
                .path()
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "semantic_owner")
        })
    }
    impl Commits<'_> {
        fn record(&mut self, identifier: &syn::Ident) {
            if !self.owner || identifier != "commit_generic_fact_bytes" {
                return;
            }
            let line = identifier.span().start().line;
            match test_scope_contains(self.source, line) {
                Ok(false) => self.lines.push(line),
                Ok(true) => {}
                Err(error) => self.failure = Some(error),
            }
        }
    }
    impl<'ast> Visit<'ast> for Commits<'_> {
        fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
            let original = self.owner;
            self.owner = declares_owner(&item.attrs);
            syn::visit::visit_item_fn(self, item);
            self.owner = original;
        }
        fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
            let original = self.owner;
            self.owner = declares_owner(&item.attrs);
            syn::visit::visit_impl_item_fn(self, item);
            self.owner = original;
        }
        fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
            self.record(&call.method);
            syn::visit::visit_expr_method_call(self, call);
        }
        fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
            if let syn::Expr::Path(path) = call.func.as_ref() {
                if let Some(segment) = path.path.segments.last() {
                    self.record(&segment.ident);
                }
            }
            syn::visit::visit_expr_call(self, call);
        }
    }
    let syntax = syn::parse_file(source).context("parse reactive semantic owner boundary")?;
    let mut commits = Commits {
        source,
        owner: false,
        lines: Vec::new(),
        failure: None,
    };
    commits.visit_file(&syntax);
    if let Some(error) = commits.failure {
        return Err(error);
    }
    Ok(commits.lines)
}

#[cfg(test)]
mod reactive_commit_tests {
    use super::unacknowledged_semantic_commits;

    #[test]
    fn actor_publication_comments_and_issuer_are_not_terminal_proofs() {
        let source = "// commit_generic_fact_bytes is our durable issuer\nimpl Effects { async fn commit_generic_fact_bytes(&self) {} }\nasync fn actor(effects: Effects) { effects.commit_generic_fact_bytes().await; }\n";
        assert!(unacknowledged_semantic_commits(source).unwrap().is_empty());
    }

    #[test]
    fn declared_owner_cannot_substitute_marker_comments_for_processing() {
        let source = "#[aura_macros::semantic_owner(owner = \"test\")]\nasync fn terminal(effects: Effects) {\n // FactCommitResult await_next_view_update fire_and_forget\n effects.commit_generic_fact_bytes().await;\n}\n";
        assert_eq!(unacknowledged_semantic_commits(source).unwrap(), vec![4]);
    }

    #[test]
    fn mixed_test_configuration_preserves_terminal_fence() {
        let source = "#[cfg(all(test, unix))]\n#[semantic_owner]\nasync fn fixture(effects: Effects) { effects.commit_generic_fact_bytes().await; }\n#[cfg(any(test, feature = \"production\"))]\n#[semantic_owner]\nasync fn terminal(effects: Effects) { effects.commit_generic_fact_bytes().await; }\n";
        assert_eq!(unacknowledged_semantic_commits(source).unwrap(), vec![6]);
    }

    #[test]
    fn owner_methods_and_associated_calls_are_checked() {
        let source = "impl Owner {\n #[semantic_owner]\n async fn terminal(effects: Effects) {\n  Effects::commit_generic_fact_bytes(&effects).await;\n }\n}\n";
        assert_eq!(unacknowledged_semantic_commits(source).unwrap(), vec![4]);
    }
}

fn check_ceremonies(repo_root: &Path, audit: &mut ArchAudit) -> Result<()> {
    let ceremony_files = rg_lines(&[
        "-l".into(),
        "ceremony.*complete|GuardianBinding|invitation.*accept".into(),
        repo_relative(repo_root.join("crates/aura-agent/src/runtime_bridge")),
        "-g".into(),
        "*.rs".into(),
    ])?;
    let ceremony_re = Regex::new(r"ceremony.*complet|guardian.*accept|Committed.*Binding")?;
    for file in ceremony_files {
        if file.contains("/tests/") || file.contains("_test.rs") || file.contains("aura-simulator")
        {
            continue;
        }
        let contents = read(repo_root.join(&file))?;
        if ceremony_re.is_match(&contents)
            && !contents.contains("commit_relational_facts")
            && !contents.contains("RelationalFact::")
        {
            audit.push(format!(
                "arch(ceremonies): ceremony completion without fact commit in {file}"
            ));
        }
    }

    let handler_files = rg_lines(&[
        "-l".into(),
        "async fn.*ceremony|execute.*ceremony".into(),
        repo_relative(repo_root.join("crates/aura-agent/src/handlers")),
        "-g".into(),
        "*.rs".into(),
    ])?;
    let handler_re = Regex::new(r"ceremony.*complet|Ok\(CeremonyResult")?;
    for file in handler_files {
        if file.contains("/tests/") || file.contains("_test.rs") || file.contains("aura-simulator")
        {
            continue;
        }
        let contents = read(repo_root.join(&file))?;
        if !contents.contains("commit_relational_facts")
            && !contents.contains("runtime_bridge")
            && !contents.contains("RelationalFact::")
            && handler_re.is_match(&contents)
        {
            audit.push(format!(
                "arch(ceremonies): ceremony handler without fact commit or delegation in {file}"
            ));
        }
    }
    Ok(())
}

fn check_ui(repo_root: &Path, audit: &mut ArchAudit) -> Result<()> {
    audit.push_matches(
        "arch(ui): direct aura_app module access in aura-terminal",
        rg_non_comment_lines(&[
            "-n".into(),
            "aura_app::(workflows|signal_defs|views|runtime_bridge|authorization)".into(),
            repo_relative(repo_root.join("crates/aura-terminal/src")),
            "-g".into(),
            "*.rs".into(),
        ])?,
    );
    audit.push_matches(
        "arch(ui): direct ViewState access in aura-terminal",
        rg_non_comment_lines(&[
            "-n".into(),
            r"\.views\(".into(),
            repo_relative(repo_root.join("crates/aura-terminal/src")),
            "-g".into(),
            "*.rs".into(),
        ])?,
    );
    audit.push_matches(
        "arch(ui): direct journal/protocol mutation in aura-terminal",
        rg_non_comment_lines(&["-n".into(),
            "FactRegistry|FactReducer|RelationalFact|JournalEffects|commit_.*facts|RuntimeBridge::commit".into(),
            repo_relative(repo_root.join("crates/aura-terminal/src")),
            "-g".into(),
            "*.rs".into()])?
        .into_iter()
        .filter(|hit| !hit.contains("crates/aura-terminal/src/demo/")),
    );
    audit.push_matches(
        "arch(ui): direct protocol/domain crate usage in aura-terminal",
        rg_non_comment_lines(&["-n".into(),
            "aura_(journal|protocol|consensus|guards|amp|anti_entropy|transport|recovery|sync|invitation|authentication|relational|chat)::".into(),
            repo_relative(repo_root.join("crates/aura-terminal/src")),
            "-g".into(),
            "*.rs".into()])?
        .into_iter()
        .filter(|hit| !hit.contains("/demo/") && !hit.contains("/scenarios/")),
    );
    audit.push_matches(
        "arch(ui): local domain state in terminal handlers",
        rg_non_comment_lines(&[
            "-n".into(),
            "HashSet<.*Id>|HashMap<.*Id,".into(),
            repo_relative(repo_root.join("crates/aura-terminal/src/handlers")),
            "-g".into(),
            "*.rs".into(),
        ])?
        .into_iter()
        .filter(|hit| {
            !hit.contains("// temporary")
                && !hit.contains("// local cache")
                && !hit.contains("/tests/")
        }),
    );
    Ok(())
}

fn check_workflows(repo_root: &Path, audit: &mut ArchAudit) -> Result<()> {
    for (pattern, root, label, exclusions) in [
        (
            "Runtime bridge not available",
            "crates/aura-app/src/workflows",
            "arch(workflows): direct runtime error string",
            vec![
                "crates/aura-app/src/workflows/runtime.rs",
                "crates/aura-app/src/workflows/error.rs",
            ],
        ),
        (
            "parse::<AuthorityId>",
            "crates/aura-app/src/workflows",
            "arch(workflows): direct AuthorityId parsing",
            vec!["crates/aura-app/src/workflows/parse.rs"],
        ),
        (
            "parse::<ContextId>",
            "crates/aura-app/src/workflows",
            "arch(workflows): direct ContextId parsing",
            vec!["crates/aura-app/src/workflows/parse.rs"],
        ),
        (
            r"\.(read|emit)\(&\*.*_SIGNAL",
            "crates/aura-app/src/workflows",
            "arch(workflows): direct signal access",
            vec!["crates/aura-app/src/workflows/signals.rs"],
        ),
    ] {
        audit.push_matches(
            label,
            rg_non_comment_lines(&[
                "-n".into(),
                pattern.into(),
                repo_relative(repo_root.join(root)),
                "-g".into(),
                "*.rs".into(),
            ])?
            .into_iter()
            .filter(|hit| {
                !(exclusions.iter().any(|excluded| hit.contains(excluded))
                    || (label == "arch(workflows): direct runtime error string"
                        && hit.contains(".contains(")))
            }),
        );
    }

    audit.push_matches(
        "arch(workflows): direct init_signals call outside approved app core",
        rg_non_comment_lines(&[
            "-n".into(),
            "init_signals\\(".into(),
            repo_relative(repo_root.join("crates/aura-app/src")),
            "-g".into(),
            "*.rs".into(),
        ])?
        .into_iter()
        .filter(|hit| {
            !hit.contains("crates/aura-app/src/core/app.rs")
                && !hit.contains("crates/aura-app/src/core/app/legacy.rs")
                && !hit.contains("init_signals_with_hooks")
        }),
    );

    for (pattern, root, label, exclusions) in [
        (
            "CommandDispatcher::|CapabilityPolicy::|dispatcher\\.dispatch\\(",
            "crates/aura-terminal/src/tui/callbacks/factories",
            "arch(workflows): forbidden slash command dispatcher usage",
            vec![],
        ),
        (
            "parse_command\\(",
            "crates/aura-terminal/src/tui/callbacks/factories",
            "arch(workflows): forbidden slash parse helper usage in callbacks/factories",
            vec![],
        ),
        (
            "parse_command\\(",
            "crates/aura-terminal/src/tui/state/handlers/input.rs",
            "arch(workflows): forbidden slash parse helper usage in input handler",
            vec![],
        ),
        (
            "CommandDispatcher|CapabilityPolicy",
            "crates/aura-terminal/src",
            "arch(workflows): forbidden dispatcher references outside dispatcher module",
            vec![
                "crates/aura-terminal/src/tui/effects/dispatcher.rs",
                "crates/aura-terminal/src/tui/effects/mod.rs",
            ],
        ),
        (
            "parse_command\\(",
            "crates/aura-terminal/src",
            "arch(workflows): forbidden parse helper references outside commands module",
            vec!["crates/aura-terminal/src/tui/commands.rs"],
        ),
    ] {
        audit.push_matches(
            label,
            rg_non_comment_lines(&[
                "-n".into(),
                pattern.into(),
                repo_relative(repo_root.join(root)),
                "-g".into(),
                "*.rs".into(),
            ])?
            .into_iter()
            .filter(|hit| !exclusions.iter().any(|excluded| hit.contains(excluded))),
        );
    }

    if !rg_exists(&[
        "-n".into(),
        "workflows::slash_commands::prepare_and_execute".into(),
        repo_relative(repo_root.join("crates/aura-terminal/src/tui/callbacks/factories")),
        "-g".into(),
        "*.rs".into(),
    ])? {
        audit.push(
            "arch(workflows): callbacks/factories must delegate to the shared slash-command workflow",
        );
    }

    let shared_plan_hits = rg_lines(&[
        "-n".into(),
        "\\.plan\\(".into(),
        repo_relative(repo_root.join("crates/aura-app/src/workflows/slash_commands.rs")),
        "-g".into(),
        "*.rs".into(),
    ])?;
    if shared_plan_hits.is_empty() {
        audit.push(
            "arch(workflows): shared slash-command workflow must plan resolved commands before execution",
        );
    }

    if !rg_exists(&[
        "-n".into(),
        "execute_planned\\(".into(),
        repo_relative(repo_root.join("crates/aura-app/src/workflows/slash_commands.rs")),
    ])? {
        audit.push("arch(workflows): shared slash-command workflow must execute its plan");
    }

    audit.push_matches(
        "arch(workflows): untyped workflow Result<_, String>",
        rg_non_comment_lines(&[
            "-n".into(),
            r"Result<[^>]*,\s*String>".into(),
            repo_relative(repo_root.join("crates/aura-app/src/workflows")),
            "-g".into(),
            "*.rs".into(),
        ])?
        .into_iter()
        .filter(|hit| {
            !hit.contains("crates/aura-app/src/workflows/authority.rs")
                && !hit.contains("crates/aura-app/src/workflows/budget.rs")
                && !hit.contains("crates/aura-app/src/workflows/chat_commands.rs")
        }),
    );

    audit.push_matches(
        "arch(workflows): serde_json::Value in workflow surface",
        filtered_test_module_hits(
            repo_root,
            rg_non_comment_lines(&[
                "-n".into(),
                "serde_json::Value".into(),
                repo_relative(repo_root.join("crates/aura-app/src/workflows")),
                "-g".into(),
                "*.rs".into(),
            ])?,
        )?
        .into_iter()
        .filter(|hit| !hit.contains("crates/aura-app/src/workflows/recovery_cli.rs")),
    );

    Ok(())
}

fn check_serialization(repo_root: &Path, audit: &mut ArchAudit) -> Result<()> {
    for file in rust_files_under(repo_root.join("crates"))
        .into_iter()
        .filter(|path| {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            name == "wire.rs" || name.ends_with("_wire.rs")
        })
    {
        let contents = read(&file)?;
        if (contents.contains("serde_json::to_vec")
            || contents.contains("serde_json::from_slice")
            || contents.contains("bincode::"))
            && !contents.contains("aura_core::util::serialization")
            && !contents.contains("crate::util::serialization")
        {
            audit.push(format!(
                "arch(serialization): wire protocol without canonical DAG-CBOR helper: {}",
                repo_relative(file)
            ));
        }
    }

    for file in rust_files_under(repo_root.join("crates"))
        .into_iter()
        .filter(|path| repo_relative(path).ends_with("/src/facts.rs"))
        .filter(|path| !repo_relative(path).contains("aura-core"))
    {
        let contents = read(&file)?;
        if (contents.contains("Serialize") || contents.contains("Deserialize"))
            && !contents.contains("aura_core::util::serialization")
            && !contents.contains("Versioned")
            && !contents.contains("Fact")
            && !contents.contains("from_slice")
            && !contents.contains("to_vec")
        {
            audit.push(format!(
                "arch(serialization): facts file without versioned serialization helper: {}",
                repo_relative(file)
            ));
        }
    }

    for file in rg_lines(&[
        "-l".into(),
        "#\\[derive.*Serialize.*Deserialize|#\\[derive.*Deserialize.*Serialize".into(),
        repo_relative(repo_root.join("crates")),
        "-g".into(),
        "protocol.rs".into(),
    ])? {
        if file.contains("/tests/") || file.contains("/benches/") || file.contains("/examples/") {
            continue;
        }
        let contents = read(repo_root.join(&file))?;
        if (contents.contains("serde_json::to_vec")
            || contents.contains("serde_json::from_slice")
            || contents.contains("serde_json::to_string")
            || contents.contains("serde_json::from_str"))
            && !contents.contains("aura_core::util::serialization")
            && !contents.contains("serde_ipld_dagcbor")
        {
            audit.push(format!(
                "arch(serialization): protocol file uses serde_json where DAG-CBOR is required: {file}"
            ));
        }
    }

    audit.push_matches(
        "arch(serialization): stateful handler under aura-agent/src/handlers",
        filtered_test_module_hits(
            repo_root,
            rg_non_comment_lines(&[
                "-n".into(),
                "Arc<.*(RwLock|Mutex)|RwLock<|Mutex<".into(),
                repo_relative(repo_root.join("crates/aura-agent/src/handlers")),
                "-g".into(),
                "*.rs".into(),
            ])?,
        )?
        .into_iter()
        .filter(|hit| !hit.contains("ota_activation_service") && !hit.contains("recovery_service")),
    );

    for bridge_file in rg_lines(&[
        "--files".into(),
        repo_relative(repo_root.join("crates/aura-agent/src/handlers")),
        "-g".into(),
        "*bridge*.rs".into(),
    ])? {
        audit.push(format!(
            "arch(serialization): handler bridge module present: {bridge_file}"
        ));
    }

    Ok(())
}

fn check_style(repo_root: &Path, audit: &mut ArchAudit) -> Result<()> {
    for file in rust_files_under(repo_root.join("crates"))
        .into_iter()
        .filter(|path| path.file_name().and_then(|name| name.to_str()) == Some("mod.rs"))
    {
        let parent = file.parent().context("mod.rs missing parent")?;
        let sibling_rs = rust_files_under(parent)
            .into_iter()
            .filter(|path| path.file_name().and_then(|name| name.to_str()) != Some("mod.rs"))
            .count();
        let subdirs = parent
            .read_dir()
            .with_context(|| format!("reading {}", parent.display()))?
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_dir())
            .count();
        if sibling_rs == 0 && subdirs == 0 {
            audit.push(format!(
                "arch(style): lonely mod.rs (convert to single file): {}",
                repo_relative(file)
            ));
        }
    }

    for dir in dirs_under(repo_root.join("crates")) {
        let display = repo_relative(&dir);
        if display.contains(".git") || display.contains("target") || display.contains("/artifacts/")
        {
            continue;
        }
        let mut file_count = 0usize;
        let mut dir_count = 0usize;
        for entry in dir
            .read_dir()
            .with_context(|| format!("reading {display}"))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                file_count += 1;
            } else if path.is_dir() {
                dir_count += 1;
            }
        }
        if file_count == 0 && dir_count == 0 {
            audit.push(format!(
                "arch(style): empty directory (delete or add .gitkeep): {display}"
            ));
        }
    }

    Ok(())
}

fn check_test_seeds(repo_root: &Path, audit: &mut ArchAudit) -> Result<()> {
    let banned_pattern = Regex::new(
        r"AuraEffectSystem::(testing\(|testing_for_authority\(|testing_with_shared_transport\(|simulation\(|simulation_for_authority\(|simulation_with_shared_transport_for_authority\()",
    )?;
    let helper_pattern = Regex::new(
        r"AuraEffectSystem::(simulation_for_test\(|simulation_for_test_with_salt\(|simulation_for_named_test\(|simulation_for_named_test_with_salt\(|simulation_for_test_for_authority\(|simulation_for_test_for_authority_with_salt\(|simulation_for_test_with_shared_transport\(|simulation_for_test_with_shared_transport_for_authority\()",
    )?;

    let mut helper_count = 0usize;
    for file in rust_files_under(repo_root.join("crates")) {
        let lines = read_lines(&file)?;
        for (idx, line) in lines.iter().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            if helper_pattern.is_match(line) && is_test_context(&file, idx + 1)? {
                helper_count += 1;
            }
            if !banned_pattern.is_match(line) {
                continue;
            }
            if is_test_context(&file, idx + 1)? {
                audit.push(format!(
                    "arch(test-seeds): banned AuraEffectSystem constructor in test context: {}:{}:{}",
                    repo_relative(&file),
                    idx + 1,
                    line.trim()
                ));
            } else if !has_allow_annotation(&lines, idx + 1)? {
                audit.push(format!(
                    "arch(test-seeds): banned AuraEffectSystem constructor outside test context without allow annotation: {}:{}:{}",
                    repo_relative(&file),
                    idx + 1,
                    line.trim()
                ));
            }
        }
    }
    if helper_count == 0 {
        audit.push("arch(test-seeds): no simulation_for_test* helper calls found in test contexts");
    }
    Ok(())
}

fn check_todos(repo_root: &Path, audit: &mut ArchAudit) -> Result<()> {
    for (pattern, label, exclusions) in [
        (
            "uuid::nil\\(\\)|placeholder implementation",
            "arch(todos): placeholder ID",
            vec!["/tests/", "/benches/", "/examples/", "/scenarios/", "/demo/", "crates/aura-simulator/"],
        ),
        (
            "deterministic algorithm",
            "arch(todos): deterministic algorithm stub",
            vec!["/tests/", "/benches/", "/examples/"],
        ),
        (
            "temporary context|temp context",
            "arch(todos): temporary context",
            vec!["/tests/", "/benches/", "/examples/"],
        ),
        (
            "TODO|FIXME",
            "arch(todos): TODO/FIXME",
            vec![
                "/benches/",
                "crates/aura-agent/src/builder/android.rs",
                "crates/aura-agent/src/builder/ios.rs",
                "crates/aura-agent/src/builder/web.rs",
                "Implement channel deletion callback",
                "Implement contact removal callback",
                "Implement invitation revocation callback",
                "Pass actual channel",
                "tree_chaos.rs",
            ],
        ),
        (
            "in production[^\\n]*(would|should|not)|in a full implementation|stub|not implemented|unimplemented|temporary|workaround|hacky|\\bWIP\\b|\\bTBD\\b|prototype|future work|to be implemented",
            "arch(todos): incomplete/WIP marker",
            vec![
                "/tests/",
                "/benches/",
                "/examples/",
                "/bin/",
                "biscuit_capability_stub",
                "effects/dispatcher.rs",
            ],
        ),
    ] {
        let hits = rg_non_comment_lines(&["-n".into(),
            "-i".into(),
            pattern.into(),
            repo_relative(repo_root.join("crates")),
            "-g".into(),
            "*.rs".into()])?;
        audit.push_matches(
            label,
            hits.into_iter()
                .filter(|hit| !exclusions.iter().any(|excluded| hit.contains(excluded))),
        );
    }
    Ok(())
}

fn filtered_test_module_hits(repo_root: &Path, hits: Vec<String>) -> Result<Vec<String>> {
    let mut filtered = Vec::new();
    for hit in hits {
        let mut parts = hit.splitn(3, ':');
        let file = parts.next().unwrap_or_default();
        let line = parts
            .next()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or_default();
        let path = repo_root.join(file);
        if is_test_context(&path, line)? {
            continue;
        }
        filtered.push(hit);
    }
    Ok(filtered)
}

fn is_test_context(path: &Path, line_number: usize) -> Result<bool> {
    // Cargo integration-test targets are test-only. A source filename such as
    // latest.rs or *_test.rs does not by itself establish a test configuration.
    if path
        .components()
        .any(|component| component.as_os_str() == "tests")
    {
        return Ok(true);
    }
    test_scope_contains(&read(path)?, line_number)
        .with_context(|| format!("classifying Rust test scope in {}", path.display()))
}

fn test_scope_contains(source: &str, line: usize) -> Result<bool> {
    attribute_scope_contains(source, line, super::policy::is_rust_test_only)
}

fn attribute_scope_contains(
    source: &str,
    line: usize,
    predicate: fn(&[syn::Attribute]) -> bool,
) -> Result<bool> {
    struct TestScopes {
        ranges: Vec<(usize, usize)>,
        predicate: fn(&[syn::Attribute]) -> bool,
    }
    impl TestScopes {
        fn record(&mut self, attributes: &[syn::Attribute], span: proc_macro2::Span) {
            if (self.predicate)(attributes) {
                self.ranges.push((span.start().line, span.end().line));
            }
        }
    }
    impl<'ast> Visit<'ast> for TestScopes {
        fn visit_item(&mut self, item: &'ast syn::Item) {
            let attributes = match item {
                syn::Item::Const(item) => &item.attrs,
                syn::Item::Enum(item) => &item.attrs,
                syn::Item::ExternCrate(item) => &item.attrs,
                syn::Item::Fn(item) => &item.attrs,
                syn::Item::ForeignMod(item) => &item.attrs,
                syn::Item::Impl(item) => &item.attrs,
                syn::Item::Macro(item) => &item.attrs,
                syn::Item::Mod(item) => &item.attrs,
                syn::Item::Static(item) => &item.attrs,
                syn::Item::Struct(item) => &item.attrs,
                syn::Item::Trait(item) => &item.attrs,
                syn::Item::TraitAlias(item) => &item.attrs,
                syn::Item::Type(item) => &item.attrs,
                syn::Item::Union(item) => &item.attrs,
                syn::Item::Use(item) => &item.attrs,
                _ => return,
            };
            self.record(attributes, item.span());
            syn::visit::visit_item(self, item);
        }
        fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
            self.record(&item.attrs, item.span());
            syn::visit::visit_impl_item_fn(self, item);
        }
        fn visit_impl_item_const(&mut self, item: &'ast syn::ImplItemConst) {
            self.record(&item.attrs, item.span());
            syn::visit::visit_impl_item_const(self, item);
        }
        fn visit_local(&mut self, item: &'ast syn::Local) {
            self.record(&item.attrs, item.span());
            syn::visit::visit_local(self, item);
        }
    }
    let syntax = syn::parse_file(source).context("parse architecture test scope")?;
    let mut scopes = TestScopes {
        ranges: Vec::new(),
        predicate,
    };
    scopes.record(&syntax.attrs, syntax.span());
    scopes.visit_file(&syntax);
    Ok(scopes
        .ranges
        .iter()
        .any(|(start, end)| *start <= line && line <= *end))
}

#[cfg(test)]
mod test_scope_tests {
    use super::test_scope_contains;

    #[test]
    fn lexical_test_scopes_do_not_hide_following_production() {
        let source = "// #[cfg(test)] is only a comment\n#[cfg(test)]\nmod fixtures {\n impl Effects for Fixture {}\n}\nimpl Effects for Production {}\n";
        assert!(test_scope_contains(source, 4).unwrap());
        assert!(!test_scope_contains(source, 1).unwrap());
        assert!(!test_scope_contains(source, 6).unwrap());
    }

    #[test]
    fn positive_test_predicates_preserve_mixed_production_branches() {
        let source = "#[cfg(all(test, not(target_arch = \"wasm32\")))]\nfn fixture() {}\n#[cfg(any(test, feature = \"production\"))]\nfn production() {}\n#[cfg(any(all(test, unix), all(test, windows)))]\nfn alternate_fixture() {}\n#[cfg(not(test))]\nfn native() {}\n";
        assert!(test_scope_contains(source, 2).unwrap());
        assert!(!test_scope_contains(source, 4).unwrap());
        assert!(test_scope_contains(source, 6).unwrap());
        assert!(!test_scope_contains(source, 8).unwrap());
    }

    #[test]
    fn test_method_and_nested_module_have_only_their_own_custody() {
        let source = "impl Owner {\n #[cfg(test)]\n fn fixture() { forbidden(); }\n fn production() { forbidden(); }\n}\nmod outer {\n #[cfg(test)]\n mod fixtures { fn fixture() {} }\n fn production() {}\n}\n";
        assert!(test_scope_contains(source, 3).unwrap());
        assert!(!test_scope_contains(source, 4).unwrap());
        assert!(test_scope_contains(source, 8).unwrap());
        assert!(!test_scope_contains(source, 9).unwrap());
    }

    #[test]
    fn malformed_rust_fails_closed() {
        assert!(test_scope_contains("#[cfg(test)] fn broken( {", 1).is_err());
    }

    #[test]
    fn constructor_allowance_is_lexical_not_nearby_text() {
        let source = "// #[allow(clippy::disallowed_methods)]\nfn outside() { forbidden(); }\nfn owner() {\n #[allow(clippy::disallowed_methods)]\n let system = {\n  forbidden();\n };\n forbidden();\n}\n";
        let lines = source.lines().map(str::to_owned).collect::<Vec<_>>();
        assert!(!super::has_allow_annotation(&lines, 2).unwrap());
        assert!(super::has_allow_annotation(&lines, 6).unwrap());
        assert!(!super::has_allow_annotation(&lines, 8).unwrap());
    }
}

fn has_allow_annotation(lines: &[String], line_number: usize) -> Result<bool> {
    fn allows_disallowed_methods(attributes: &[syn::Attribute]) -> bool {
        attributes.iter().any(|attribute| {
            attribute.path().is_ident("allow")
                && attribute
                    .parse_args_with(
                        syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
                    )
                    .is_ok_and(|paths| {
                        paths.iter().any(|path| {
                            path.segments.len() == 2
                                && path.segments[0].ident == "clippy"
                                && path.segments[1].ident == "disallowed_methods"
                        })
                    })
        })
    }
    attribute_scope_contains(&lines.join("\n"), line_number, allows_disallowed_methods)
}

fn layer_of(name: &str) -> usize {
    match name {
        "aura-core" => 1,
        "aura-journal" | "aura-authorization" | "aura-signature" | "aura-store"
        | "aura-transport" | "aura-maintenance" | "aura-mpst" | "aura-macros" => 2,
        "aura-effects" | "aura-composition" => 3,
        "aura-protocol" | "aura-guards" | "aura-consensus" | "aura-amp" | "aura-anti-entropy" => 4,
        "aura-authentication"
        | "aura-chat"
        | "aura-invitation"
        | "aura-recovery"
        | "aura-relational"
        | "aura-rendezvous"
        | "aura-social"
        | "aura-sync"
        | "aura-app" => 5,
        "aura-agent" | "aura-simulator" => 6,
        "aura-terminal" | "aura-ui" | "aura-web" => 7,
        "aura-testkit" | "aura-quint" | "aura-harness" => 8,
        _ => 0,
    }
}
