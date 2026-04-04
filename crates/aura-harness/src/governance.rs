use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};
use aura_app::scenario_contract::{
    ActorId, BarrierDeclaration, Expectation, IntentAction, ScenarioAction as SemanticAction,
    ScenarioDefinition, ScenarioStep,
};
use aura_app::ui_contract::{
    SharedFlowId, PARITY_EXCEPTION_METADATA, SHARED_FLOW_SCENARIO_COVERAGE,
    SHARED_FLOW_SOURCE_AREAS,
};

use crate::config::{
    load_scenario_inventory, load_semantic_scenario_definition, ScenarioClassification,
    ScenarioConfig, ScenarioInventoryEntry,
};

const COVERAGE_DOC: &str = "docs/997_flow_coverage.md";
const CORE_SHARED_SCENARIO_IDS: &[&str] = &[
    "scenario12-mixed-device-enrollment-removal-e2e",
    "scenario13-mixed-contact-channel-message-e2e",
];

fn read_contract_family(root: &str, module_dir: &str) -> Result<String> {
    let mut combined =
        fs::read_to_string(root).with_context(|| format!("failed to read {root}"))?;

    let module_path = Path::new(module_dir);
    if module_path.exists() {
        let mut files = fs::read_dir(module_path)
            .with_context(|| format!("failed to read directory {module_dir}"))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .with_context(|| format!("failed to enumerate directory {module_dir}"))?;
        files.sort_by_key(|entry| entry.path());
        for entry in files {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                continue;
            }
            combined.push('\n');
            combined.push_str(
                &fs::read_to_string(&path)
                    .with_context(|| format!("failed to read {}", path.display()))?,
            );
        }
    }

    Ok(combined)
}

pub enum GovernanceCheck {
    SharedScenarioContract,
    ScenarioLegality,
    CoreScenarioMechanics,
    UserFlowCoverage,
    UiParityContract,
    SettingsSurfaceContract,
    ScenarioShapeContract,
    GovernanceWrappers,
}

pub fn run(check: GovernanceCheck) -> Result<()> {
    match check {
        GovernanceCheck::SharedScenarioContract => validate_shared_scenario_contract(),
        GovernanceCheck::ScenarioLegality => validate_scenario_legality(),
        GovernanceCheck::CoreScenarioMechanics => validate_core_scenario_mechanics(),
        GovernanceCheck::UserFlowCoverage => validate_ux_flow_coverage(),
        GovernanceCheck::UiParityContract => validate_ui_parity_contract(),
        GovernanceCheck::SettingsSurfaceContract => validate_settings_surface_contract(),
        GovernanceCheck::ScenarioShapeContract => validate_scenario_shape_contract(),
        GovernanceCheck::GovernanceWrappers => validate_governance_wrappers(),
    }
}

pub fn validate_shared_scenario_contract() -> Result<()> {
    let inventory = load_scenario_inventory(None)?;
    let shared = inventory
        .iter()
        .filter(|entry| entry.classification == ScenarioClassification::Shared)
        .collect::<Vec<_>>();
    if shared.is_empty() {
        bail!("no shared scenarios found in inventory");
    }

    for entry in shared {
        let definition = load_semantic_scenario_definition(&entry.path)?;
        definition
            .validate_shared_intent_contract()
            .map_err(|error| {
                anyhow!(
                    "shared scenario {} violates intent contract: {error}",
                    entry.path.display()
                )
            })?;
        let scenario = ScenarioConfig::try_from(definition.clone()).with_context(|| {
            format!(
                "shared scenario {} failed strict canonical lowering",
                entry.path.display()
            )
        })?;
        ensure_shared_execution_is_strict(&scenario, &entry.path)?;
        validate_declared_barriers(&definition).with_context(|| {
            format!(
                "shared scenario {} is missing declared convergence barriers",
                entry.path.display()
            )
        })?;
    }

    println!("harness shared scenario contract: clean");
    Ok(())
}

pub fn validate_scenario_legality() -> Result<()> {
    let inventory = load_scenario_inventory(None)?;
    if inventory.is_empty() {
        bail!("no inventory entries found");
    }

    let mut ids = BTreeSet::new();
    let mut paths = BTreeSet::new();

    for entry in &inventory {
        if !ids.insert(entry.id.clone()) {
            bail!("duplicate scenario id in inventory: {}", entry.id);
        }
        let normalized_path = normalize_rel_path(&entry.path);
        if !paths.insert(normalized_path.clone()) {
            bail!(
                "duplicate scenario path in inventory: {}",
                entry.path.display()
            );
        }
        if !entry.path.exists() {
            bail!("missing scenario file: {}", entry.path.display());
        }

        let definition = load_semantic_scenario_definition(&entry.path)?;
        ensure_converted_frontend_mechanics_are_classified(entry, &definition)?;
        if entry.classification == ScenarioClassification::Shared {
            definition
                .validate_shared_intent_contract()
                .map_err(|error| {
                    anyhow!(
                        "shared scenario {} violates intent contract: {error}",
                        entry.path.display()
                    )
                })?;
        }
    }

    println!("harness scenario legality: clean");
    Ok(())
}

pub fn validate_core_scenario_mechanics() -> Result<()> {
    let inventory = load_scenario_inventory(None)?;
    for required_id in CORE_SHARED_SCENARIO_IDS {
        let entry = inventory
            .iter()
            .find(|entry| entry.id == *required_id)
            .ok_or_else(|| {
                anyhow!("missing core shared scenario inventory entry: {required_id}")
            })?;
        if entry.classification != ScenarioClassification::Shared {
            bail!(
                "core shared scenario {} must be classified as shared",
                entry.path.display()
            );
        }
        let definition = load_semantic_scenario_definition(&entry.path)?;
        definition
            .validate_shared_intent_contract()
            .map_err(|error| {
                anyhow!(
                    "core shared scenario {} violates intent contract: {error}",
                    entry.path.display()
                )
            })?;
        let scenario = ScenarioConfig::try_from(definition.clone()).with_context(|| {
            format!(
                "core shared scenario {} failed strict canonical lowering",
                entry.path.display()
            )
        })?;
        ensure_shared_execution_is_strict(&scenario, &entry.path)?;
        validate_declared_barriers(&definition).with_context(|| {
            format!(
                "core shared scenario {} is missing declared convergence barriers",
                entry.path.display()
            )
        })?;
    }

    println!("harness core scenario mechanics: clean");
    Ok(())
}

pub fn validate_ux_flow_coverage() -> Result<()> {
    if std::env::var("AURA_ALLOW_FLOW_COVERAGE_SKIP").as_deref() == Ok("1") {
        if is_ci() {
            bail!("AURA_ALLOW_FLOW_COVERAGE_SKIP=1 is not allowed in CI");
        }
        println!("user-flow-coverage: skipped via AURA_ALLOW_FLOW_COVERAGE_SKIP=1");
        return Ok(());
    }

    let coverage_doc = Path::new(COVERAGE_DOC);
    if !coverage_doc.exists() {
        bail!("missing coverage doc: {}", coverage_doc.display());
    }
    let changed_files = changed_files()?;
    if changed_files.is_empty() {
        println!("user-flow-coverage: no changed files");
        return Ok(());
    }

    let changed_set = changed_files.iter().cloned().collect::<BTreeSet<_>>();
    let doc_touched = changed_set.contains(COVERAGE_DOC);
    let coverage_metadata_touched = changed_set.contains("crates/aura-app/src/ui_contract.rs");
    let coverage_doc_body = fs::read_to_string(coverage_doc)
        .with_context(|| format!("failed to read {COVERAGE_DOC}"))?;

    let inventory = load_scenario_inventory(None)?;
    let inventory_ids = inventory
        .iter()
        .map(|entry| entry.id.as_str())
        .collect::<BTreeSet<_>>();

    let mut flow_to_scenarios: HashMap<SharedFlowId, Vec<&'static str>> = HashMap::new();
    for coverage in SHARED_FLOW_SCENARIO_COVERAGE {
        if !inventory_ids.contains(coverage.scenario_id) {
            bail!(
                "shared flow {:?} maps to unknown scenario_id {}",
                coverage.flow,
                coverage.scenario_id
            );
        }
        flow_to_scenarios
            .entry(coverage.flow)
            .or_default()
            .push(coverage.scenario_id);
    }

    let mut affected_flows = HashSet::new();
    for changed in &changed_files {
        for area in SHARED_FLOW_SOURCE_AREAS {
            if area.path == changed {
                affected_flows.insert(area.flow);
            }
        }
    }

    if affected_flows.is_empty() {
        println!("user-flow-coverage: no typed shared-flow source mappings for changed files");
        return Ok(());
    }

    let mut violations = Vec::new();
    for flow in affected_flows {
        let scenario_ids = flow_to_scenarios
            .get(&flow)
            .ok_or_else(|| anyhow!("no scenario coverage mapping for shared flow {flow:?}"))?;
        let scenario_paths = scenario_ids
            .iter()
            .map(|scenario_id| format!("scenarios/harness/{scenario_id}.toml"))
            .collect::<Vec<_>>();

        for scenario_path in &scenario_paths {
            if !coverage_doc_body.contains(scenario_path) {
                violations.push(format!(
                    "docs mapping missing for flow={flow:?} scenario={scenario_path}"
                ));
            }
        }

        let scenarios_changed = scenario_paths
            .iter()
            .any(|scenario_path| changed_set.contains(scenario_path.as_str()));
        if !scenarios_changed && !coverage_metadata_touched {
            violations.push(format!(
                "flow-relevant changes detected for {flow:?} without scenario or shared coverage metadata update"
            ));
        }
    }

    if doc_touched && !coverage_metadata_touched {
        println!("user-flow-coverage: coverage doc updated for traceability");
    }
    if !violations.is_empty() {
        bail!(violations.join(" | "));
    }

    println!("user-flow-coverage: clean");
    Ok(())
}

pub fn validate_ui_parity_contract() -> Result<()> {
    let contract_path = Path::new("crates/aura-app/src/ui_contract.rs");
    if !contract_path.exists() {
        bail!(
            "missing parity contract source: {}",
            contract_path.display()
        );
    }

    for metadata in PARITY_EXCEPTION_METADATA {
        if metadata.reason_code.trim().is_empty() {
            bail!(
                "parity exception {:?} must declare reason_code",
                metadata.exception
            );
        }
        if metadata.scope.trim().is_empty() {
            bail!(
                "parity exception {:?} must declare scope",
                metadata.exception
            );
        }
        if metadata.affected_surface.trim().is_empty() {
            bail!(
                "parity exception {:?} must declare affected_surface",
                metadata.exception
            );
        }
        if metadata.doc_reference.trim().is_empty() {
            bail!(
                "parity exception {:?} must declare doc_reference",
                metadata.exception
            );
        }
        let doc_path = Path::new(metadata.doc_reference);
        if !doc_path.exists() {
            bail!(
                "parity exception {:?} references missing doc {}",
                metadata.exception,
                metadata.doc_reference
            );
        }
    }

    run_cargo_test("aura-app", "shared_flow_support_contract_is_consistent")?;
    run_cargo_test(
        "aura-app",
        "shared_flow_scenario_coverage_points_to_existing_scenarios",
    )?;
    run_cargo_test(
        "aura-app",
        "shared_screen_modal_and_list_support_is_unique_and_addressable",
    )?;
    run_cargo_test(
        "aura-app",
        "shared_screen_module_map_uses_canonical_screen_names",
    )?;
    run_cargo_test(
        "aura-app",
        "parity_module_map_points_to_existing_frontend_symbols",
    )?;
    run_cargo_test(
        "aura-app",
        "parity_exception_metadata_is_complete_and_documented",
    )?;
    run_cargo_test(
        "aura-app",
        "ui_snapshot_parity_ignores_occurrence_ids_but_catches_state_drift",
    )?;
    run_cargo_test(
        "aura-app",
        "ui_snapshot_parity_detects_focus_semantic_drift",
    )?;
    run_cargo_test(
        "aura-app",
        "ui_snapshot_parity_detects_runtime_event_shape_drift",
    )?;
    run_cargo_test("aura-app", "parity_ui_identity_helpers_match_contract_ids")?;
    run_cargo_test(
        "aura-app",
        "frontend_sources_reference_shared_identity_helpers",
    )?;

    println!("ui parity contract: clean");
    Ok(())
}

pub fn validate_settings_surface_contract() -> Result<()> {
    let contract = read_contract_family(
        "crates/aura-app/src/ui_contract.rs",
        "crates/aura-app/src/ui_contract",
    )?;
    let web_model = fs::read_to_string("crates/aura-ui/src/model/settings.rs")
        .context("failed to read crates/aura-ui/src/model/settings.rs")?;
    let tui_types = fs::read_to_string("crates/aura-terminal/src/tui/types/settings.rs")
        .context("failed to read crates/aura-terminal/src/tui/types/settings.rs")?;
    let tui_export =
        fs::read_to_string("crates/aura-terminal/src/tui/harness_state/snapshot.rs")
            .context("failed to read crates/aura-terminal/src/tui/harness_state/snapshot.rs")?;

    for needle in [
        "pub enum SharedSettingsSectionId",
        "pub enum FrontendSpecificSettingsSectionId",
        "pub const PARITY_CRITICAL_SETTINGS_SECTIONS",
        "pub const FRONTEND_SPECIFIC_SETTINGS_SECTIONS",
        "pub const fn settings_section_item_id",
        "pub fn classify_settings_section_item_id",
    ] {
        if !contract.contains(needle) {
            bail!("settings contract missing required declaration: {needle}");
        }
    }

    for needle in [
        "settings_section_item_id(",
        "SharedSettingsSectionId::GuardianThreshold",
        "SharedSettingsSectionId::RequestRecovery",
        "FrontendSpecificSettingsSectionId::Appearance",
        "FrontendSpecificSettingsSectionId::Info",
    ] {
        if !web_model.contains(needle) {
            bail!("web settings surface missing canonical settings reference: {needle}");
        }
    }

    for needle in [
        "settings_section_item_id(",
        "SharedSettingsSectionId::GuardianThreshold",
        "SharedSettingsSectionId::RequestRecovery",
        "FrontendSpecificSettingsSectionId::Observability",
    ] {
        if !tui_types.contains(needle) {
            bail!("tui settings surface missing canonical settings reference: {needle}");
        }
    }

    if !tui_export.contains("section.parity_item_id().to_string()") {
        bail!("tui settings export must use the canonical parity item id");
    }
    if tui_export.contains("to_ascii_lowercase().replace(' ', \"_\")") {
        bail!("tui settings export may not derive parity ids from section titles");
    }

    run_cargo_test("aura-app", "shared_settings_section_surface_is_explicit")?;
    run_cargo_test(
        "aura-app",
        "frontend_settings_sources_use_shared_section_ids",
    )?;

    println!("harness settings surface contract: clean");
    Ok(())
}

pub fn validate_scenario_shape_contract() -> Result<()> {
    let definition = ScenarioDefinition {
        id: "canonical-shared-check".to_string(),
        goal: "shared scenario shape contract".to_string(),
        steps: vec![
            ScenarioStep {
                id: "create".to_string(),
                actor: Some(ActorId("alice".to_string())),
                timeout_ms: Some(1000),
                action: SemanticAction::Intent(IntentAction::CreateAccount {
                    account_name: "alice".to_string(),
                }),
            },
            ScenarioStep {
                id: "wait".to_string(),
                actor: Some(ActorId("alice".to_string())),
                timeout_ms: Some(1000),
                action: SemanticAction::Expect(Expectation::ReadinessIs(
                    aura_app::ui::contract::UiReadiness::Ready,
                )),
            },
        ],
    };
    let config = ScenarioConfig::try_from(definition)?;
    if !config.compatibility_steps.is_empty() {
        bail!("semantic shared scenarios must not store mirrored frontend-conformance execution steps");
    }
    if config
        .semantic_steps()
        .map_or(true, |steps| steps.is_empty())
    {
        bail!("semantic shared scenarios must retain semantic steps");
    }

    run_cargo_test(
        "aura-harness",
        "semantic_scenarios_keep_canonical_steps_directly",
    )?;
    run_cargo_test(
        "aura-harness",
        "semantic_definition_translates_into_execution_scenario",
    )?;
    run_cargo_test(
        "aura-harness",
        "semantic_parity_expectation_translates_into_execution_scenario",
    )?;
    println!("harness-scenario-shape-contract: clean");
    Ok(())
}

pub fn validate_governance_wrappers() -> Result<()> {
    const WRAPPERS: &[(&str, &str)] = &[
        (
            "scripts/check/harness-shared-scenario-contract.sh",
            "shared-scenario-contract",
        ),
        (
            "scripts/check/harness-scenario-legality.sh",
            "scenario-legality",
        ),
        (
            "scripts/check/harness-core-scenario-mechanics.sh",
            "core-scenario-mechanics",
        ),
        ("scripts/check/user-flow-coverage.sh", "user-flow-coverage"),
        ("scripts/check/ui-parity-contract.sh", "ui-parity-contract"),
        (
            "scripts/check/harness-settings-surface-contract.sh",
            "settings-surface-contract",
        ),
        (
            "scripts/check/harness-scenario-shape-contract.sh",
            "scenario-shape-contract",
        ),
        (
            "scripts/check/harness-governance-wrappers.sh",
            "governance-wrappers",
        ),
    ];

    for (path, check) in WRAPPERS {
        let body =
            fs::read_to_string(path).with_context(|| format!("failed to read wrapper {path}"))?;
        let expected =
            format!("cargo run -p aura-harness --bin aura-harness --quiet -- governance {check}");
        if !body.contains(&expected) {
            bail!("wrapper does not call typed governance entry point: {path}");
        }
        for forbidden in [
            "rg ",
            "awk ",
            "git diff",
            "allowed_actions=",
            "shared_paths=",
            "entries=",
            "core_shared_scenarios=",
            "AURA_ALLOW_FLOW_COVERAGE_SKIP",
        ] {
            if body.contains(forbidden) {
                bail!("wrapper contains standalone governance logic: {path}");
            }
        }
    }

    println!("harness-governance-wrappers: clean");
    Ok(())
}
fn ensure_converted_frontend_mechanics_are_classified(
    entry: &ScenarioInventoryEntry,
    definition: &ScenarioDefinition,
) -> Result<()> {
    if !uses_frontend_ui_mechanics(definition) {
        return Ok(());
    }
    if entry.classification.is_frontend_conformance() {
        return Ok(());
    }
    bail!(
        "scenario {} uses frontend-local ui mechanics and must be classified as tui_conformance or web_conformance",
        entry.path.display()
    );
}

fn uses_frontend_ui_mechanics(definition: &ScenarioDefinition) -> bool {
    definition
        .steps
        .iter()
        .any(|step| matches!(step.action, SemanticAction::Ui(_)))
}

fn ensure_shared_execution_is_strict(scenario: &ScenarioConfig, path: &Path) -> Result<()> {
    if !scenario.is_semantic_scenario() {
        bail!(
            "shared scenario {} must use the semantic shared-flow model",
            path.display()
        );
    }
    for step in scenario.semantic_steps().unwrap_or(&[]).iter() {
        if matches!(step.action, SemanticAction::Ui(_)) {
            bail!(
                "shared scenario {} contains raw ui mechanic action {:?}",
                path.display(),
                step.action
            );
        }
    }
    Ok(())
}

fn validate_declared_barriers(definition: &ScenarioDefinition) -> Result<()> {
    let mut pending = Vec::new();

    for step in &definition.steps {
        pending.retain(|barrier| !action_satisfies_barrier(&step.action, barrier));

        if let SemanticAction::Intent(intent) = &step.action {
            let Some(convergence) = intent.contract().post_operation_convergence else {
                continue;
            };
            if !convergence.required_before_next_intent.is_empty() {
                pending.extend(convergence.required_before_next_intent);
            }
        }
    }

    if pending.is_empty() {
        Ok(())
    } else {
        bail!(
            "scenario is missing convergence barriers: {}",
            pending
                .into_iter()
                .map(barrier_label)
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

fn action_satisfies_barrier(action: &SemanticAction, barrier: &BarrierDeclaration) -> bool {
    match (action, barrier) {
        (
            SemanticAction::Expect(Expectation::ScreenIs(actual)),
            BarrierDeclaration::Screen(expected),
        ) => actual == expected,
        (
            SemanticAction::Expect(Expectation::ReadinessIs(actual)),
            BarrierDeclaration::Readiness(expected),
        ) => actual == expected,
        (
            SemanticAction::Expect(Expectation::RuntimeEventOccurred { kind, .. }),
            BarrierDeclaration::RuntimeEvent(expected),
        ) => kind == expected,
        (
            SemanticAction::Expect(Expectation::OperationStateIs {
                operation_id,
                state,
            }),
            BarrierDeclaration::OperationState {
                operation_id: expected_id,
                state: expected_state,
            },
        ) => operation_id == expected_id && state == expected_state,
        _ => false,
    }
}

fn barrier_label(barrier: BarrierDeclaration) -> String {
    match barrier {
        BarrierDeclaration::Modal(modal) => format!("modal:{modal:?}"),
        BarrierDeclaration::RuntimeEvent(kind) => format!("runtime_event:{kind:?}"),
        BarrierDeclaration::Screen(screen) => format!("screen:{screen:?}"),
        BarrierDeclaration::Readiness(readiness) => format!("readiness:{readiness:?}"),
        BarrierDeclaration::Quiescence(quiescence) => format!("quiescence:{quiescence:?}"),
        BarrierDeclaration::OperationState {
            operation_id,
            state,
        } => {
            format!("operation:{operation_id:?}:{state:?}")
        }
    }
}

fn changed_files() -> Result<Vec<String>> {
    if let Ok(raw) = std::env::var("AURA_FLOW_COVERAGE_CHANGED_FILES") {
        return Ok(split_lines(&raw));
    }

    let diff_range = if let Ok(raw) = std::env::var("AURA_FLOW_COVERAGE_DIFF_RANGE") {
        raw
    } else if let Ok(base_ref) = std::env::var("GITHUB_BASE_REF") {
        let candidate = format!("origin/{base_ref}");
        if git_success(["rev-parse", "--verify", &candidate])? {
            format!("{candidate}...HEAD")
        } else if git_success(["rev-parse", "--verify", "HEAD"])? {
            "HEAD".to_string()
        } else {
            return Ok(Vec::new());
        }
    } else if git_success(["rev-parse", "--verify", "HEAD"])? {
        "HEAD".to_string()
    } else {
        return Ok(Vec::new());
    };

    let output = Command::new("git")
        .args(["diff", "--name-only", &diff_range])
        .output()
        .with_context(|| format!("failed to run git diff for range {diff_range}"))?;
    if !output.status.success() {
        bail!("git diff --name-only {diff_range} failed");
    }
    Ok(split_lines(&String::from_utf8_lossy(&output.stdout)))
}

fn split_lines(raw: &str) -> Vec<String> {
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(normalize_rel_path)
        .collect()
}

fn git_success<const N: usize>(args: [&str; N]) -> Result<bool> {
    Ok(Command::new("git")
        .args(args)
        .status()
        .context("failed to run git")?
        .success())
}

fn run_cargo_test(crate_name: &str, test_name: &str) -> Result<()> {
    let status = Command::new("cargo")
        .args(["test", "-p", crate_name, test_name, "--quiet"])
        .status()
        .with_context(|| format!("failed to run cargo test -p {crate_name} {test_name}"))?;
    if !status.success() {
        bail!("cargo test -p {crate_name} {test_name} failed");
    }
    Ok(())
}

fn normalize_rel_path(path: impl AsRef<Path>) -> String {
    let mut normalized = path.as_ref().to_string_lossy().replace('\\', "/");
    while normalized.contains("//") {
        normalized = normalized.replace("//", "/");
    }
    normalized
}

fn is_ci() -> bool {
    std::env::var("CI").as_deref() == Ok("true")
        || std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_app::scenario_contract::{ActorId, ScenarioStep};
    use aura_app::ui::contract::{
        FieldId, OperationId, OperationState, RuntimeEventKind, ScreenId, UiReadiness,
    };
    use aura_app::ui_contract::QuiescenceState;
    use std::path::PathBuf;

    #[test]
    fn barrier_validator_allows_accept_pending_channel_without_membership_wait() {
        let definition = ScenarioDefinition {
            id: "barriers".to_string(),
            goal: "test barriers".to_string(),
            steps: vec![
                ScenarioStep {
                    id: "join".to_string(),
                    actor: Some(ActorId("alice".to_string())),
                    timeout_ms: Some(1000),
                    action: SemanticAction::Intent(IntentAction::AcceptPendingChannelInvitation),
                },
                ScenarioStep {
                    id: "next-intent".to_string(),
                    actor: Some(ActorId("alice".to_string())),
                    timeout_ms: Some(1000),
                    action: SemanticAction::Intent(IntentAction::OpenScreen {
                        screen: ScreenId::Chat,
                        channel_id: None,
                        context_id: None,
                    }),
                },
            ],
        };

        assert!(validate_declared_barriers(&definition).is_ok());
    }

    #[test]
    fn barrier_validator_accepts_extra_runtime_wait_after_accept_pending_channel() {
        let definition = ScenarioDefinition {
            id: "barriers-ok".to_string(),
            goal: "test barriers".to_string(),
            steps: vec![
                ScenarioStep {
                    id: "join".to_string(),
                    actor: Some(ActorId("alice".to_string())),
                    timeout_ms: Some(1000),
                    action: SemanticAction::Intent(IntentAction::AcceptPendingChannelInvitation),
                },
                ScenarioStep {
                    id: "wait".to_string(),
                    actor: Some(ActorId("alice".to_string())),
                    timeout_ms: Some(1000),
                    action: SemanticAction::Expect(Expectation::RuntimeEventOccurred {
                        kind: RuntimeEventKind::ChannelMembershipReady,
                        detail_contains: None,
                        capture_name: None,
                    }),
                },
                ScenarioStep {
                    id: "next-intent".to_string(),
                    actor: Some(ActorId("alice".to_string())),
                    timeout_ms: Some(1000),
                    action: SemanticAction::Intent(IntentAction::JoinChannel {
                        channel_name: "shared".to_string(),
                    }),
                },
                ScenarioStep {
                    id: "join-wait".to_string(),
                    actor: Some(ActorId("alice".to_string())),
                    timeout_ms: Some(1000),
                    action: SemanticAction::Expect(Expectation::RuntimeEventOccurred {
                        kind: RuntimeEventKind::ChannelMembershipReady,
                        detail_contains: None,
                        capture_name: None,
                    }),
                },
            ],
        };

        assert!(validate_declared_barriers(&definition).is_ok());
    }

    #[test]
    fn barrier_validator_requires_join_channel_convergence_before_next_intent() {
        let definition = ScenarioDefinition {
            id: "join-barriers".to_string(),
            goal: "test join channel barriers".to_string(),
            steps: vec![
                ScenarioStep {
                    id: "join".to_string(),
                    actor: Some(ActorId("alice".to_string())),
                    timeout_ms: Some(1000),
                    action: SemanticAction::Intent(IntentAction::JoinChannel {
                        channel_name: "shared".to_string(),
                    }),
                },
                ScenarioStep {
                    id: "next-intent".to_string(),
                    actor: Some(ActorId("alice".to_string())),
                    timeout_ms: Some(1000),
                    action: SemanticAction::Intent(IntentAction::SendChatMessage {
                        message: "hello".to_string(),
                        channel_id: None,
                        context_id: None,
                    }),
                },
            ],
        };

        let error = validate_declared_barriers(&definition)
            .unwrap_err()
            .to_string();
        assert!(error.contains("ChannelMembershipReady"));
    }

    #[test]
    fn changed_files_parser_normalizes_env_lines() {
        let files = split_lines(
            "crates\\\\aura-ui\\\\src\\\\app.rs\n\nscenarios/harness/shared-settings-parity.toml\n",
        );
        assert_eq!(
            files,
            vec![
                "crates/aura-ui/src/app.rs".to_string(),
                "scenarios/harness/shared-settings-parity.toml".to_string()
            ]
        );
    }

    #[test]
    fn action_satisfies_barrier_matches_typed_expectations() {
        assert!(action_satisfies_barrier(
            &SemanticAction::Expect(Expectation::ScreenIs(ScreenId::Chat)),
            &BarrierDeclaration::Screen(ScreenId::Chat)
        ));
        assert!(action_satisfies_barrier(
            &SemanticAction::Expect(Expectation::ReadinessIs(UiReadiness::Ready)),
            &BarrierDeclaration::Readiness(UiReadiness::Ready)
        ));
        assert!(action_satisfies_barrier(
            &SemanticAction::Expect(Expectation::RuntimeEventOccurred {
                kind: RuntimeEventKind::MessageCommitted,
                detail_contains: None,
                capture_name: None,
            }),
            &BarrierDeclaration::RuntimeEvent(RuntimeEventKind::MessageCommitted)
        ));
        assert!(action_satisfies_barrier(
            &SemanticAction::Expect(Expectation::OperationStateIs {
                operation_id: OperationId::invitation_accept_contact(),
                state: OperationState::Succeeded,
            }),
            &BarrierDeclaration::OperationState {
                operation_id: OperationId::invitation_accept_contact(),
                state: OperationState::Succeeded,
            }
        ));
        assert!(!action_satisfies_barrier(
            &SemanticAction::Expect(Expectation::ReadinessIs(UiReadiness::Ready)),
            &BarrierDeclaration::Quiescence(QuiescenceState::Settled)
        ));
    }

    #[test]
    fn converted_frontend_ui_mechanics_require_conformance_classification() {
        let entry = ScenarioInventoryEntry {
            id: "ui-conformance".to_string(),
            path: PathBuf::from("tests/fixtures/ui-conformance.toml"),
            classification: ScenarioClassification::Shared,
            runtime_substrate: "real_runtime".to_string(),
            notes: "test".to_string(),
        };
        let definition = ScenarioDefinition {
            id: "ui-conformance".to_string(),
            goal: "frontend-local ui action".to_string(),
            steps: vec![ScenarioStep {
                id: "fill".to_string(),
                actor: Some(ActorId("alice".to_string())),
                timeout_ms: Some(1000),
                action: SemanticAction::Ui(aura_app::scenario_contract::UiAction::Fill(
                    FieldId::Nickname,
                    "ops".to_string(),
                )),
            }],
        };

        let error = ensure_converted_frontend_mechanics_are_classified(&entry, &definition)
            .err()
            .unwrap_or_else(|| panic!("shared classification must reject frontend-local ui"));
        assert!(
            error
                .to_string()
                .contains("tui_conformance or web_conformance"),
            "expected conformance classification requirement, got {error:#}"
        );
    }

    #[test]
    fn converted_frontend_ui_mechanics_allow_conformance_classification() {
        let entry = ScenarioInventoryEntry {
            id: "ui-conformance".to_string(),
            path: PathBuf::from("tests/fixtures/ui-conformance.toml"),
            classification: ScenarioClassification::TuiConformance,
            runtime_substrate: "real_runtime".to_string(),
            notes: "test".to_string(),
        };
        let definition = ScenarioDefinition {
            id: "ui-conformance".to_string(),
            goal: "frontend-local ui action".to_string(),
            steps: vec![ScenarioStep {
                id: "fill".to_string(),
                actor: Some(ActorId("alice".to_string())),
                timeout_ms: Some(1000),
                action: SemanticAction::Ui(aura_app::scenario_contract::UiAction::Fill(
                    FieldId::Nickname,
                    "ops".to_string(),
                )),
            }],
        };

        assert!(ensure_converted_frontend_mechanics_are_classified(&entry, &definition).is_ok());
        assert!(uses_frontend_ui_mechanics(&definition));
    }
}
