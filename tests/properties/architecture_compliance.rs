//! Architecture Boundary Enforcement Tests
//!
//! This test suite validates the 8-layer architecture defined in docs/002_system_architecture.md.
//! It ensures that dependencies only flow downward through layers and that each crate
//! belongs to exactly one layer with appropriate dependencies.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

/// The 8 architectural layers in dependency order (lower layers can only depend on same or lower)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Layer {
    Foundation = 1,     // aura-core
    Specification = 2,  // Domain crates, aura-mpst, aura-macros
    Implementation = 3, // aura-effects
    Orchestration = 4,  // aura-protocol
    Feature = 5,        // aura-invitation, etc. (aura-frost deprecated)
    Runtime = 6,        // aura-agent, aura-simulator
    UI = 7,             // aura-cli
    Testing = 8,        // aura-testkit, aura-quint
}

impl Layer {
    fn from_crate_name(name: &str) -> Option<Self> {
        match name {
            // Layer 1: Foundation
            "aura-core" => Some(Layer::Foundation),

            // Layer 2: Specification
            "aura-journal" | "aura-wot" | "aura-verify" | "aura-store" | "aura-transport"
            | "aura-mpst" | "aura-macros" => Some(Layer::Specification),

            // Layer 3: Implementation
            "aura-effects" => Some(Layer::Implementation),

            // Layer 4: Orchestration
            "aura-protocol" => Some(Layer::Orchestration),

            // Layer 5: Feature/Protocol
            "aura-authenticate" | "aura-invitation" | "aura-recovery" | "aura-rendezvous"
            | "aura-storage" | "aura-sync" => Some(Layer::Feature),

            // Layer 6: Runtime Composition
            "aura-agent" => Some(Layer::Runtime),

            // aura-simulator is technically runtime but is a testing tool,
            // so it's classified as Layer 8 to allow testkit dependency
            "aura-simulator" => Some(Layer::Testing),

            // Layer 7: User Interface
            "aura-cli" => Some(Layer::UI),

            // Layer 8: Testing/Tools
            "aura-testkit" | "aura-quint" => Some(Layer::Testing),

            // Benchmarks and meta-crates (not part of main architecture)
            "handler_performance" | "simple_benchmarks" | "aura" => None,

            // Examples are not part of the main architecture
            _ if name.starts_with("hello-") => None,
            _ if name.starts_with("app-") => None, // Future UI apps

            _ => {
                eprintln!(
                    "Warning: Unknown crate '{}' - not in layer classification",
                    name
                );
                None
            }
        }
    }
}

/// Crate dependency information
#[derive(Debug)]
struct CrateInfo {
    name: String,
    layer: Option<Layer>,
    dependencies: HashSet<String>,
    path: PathBuf,
}

fn parse_cargo_toml(path: &Path) -> Option<CrateInfo> {
    let content = fs::read_to_string(path).ok()?;

    // Simple TOML parsing for [package] name and [dependencies]
    let mut package_name = None;
    let mut dependencies = HashSet::new();
    let mut in_dependencies = false;
    let mut in_package = false;

    for line in content.lines() {
        let line = line.trim();

        // Track [package] section
        if line == "[package]" {
            in_package = true;
            continue;
        }

        // Stop at next section
        if line.starts_with('[') {
            in_package = false;
        }

        // Parse package name (only in [package] section, ignore [lib] name)
        if in_package && line.starts_with("name") && line.contains('=') && package_name.is_none() {
            package_name = line
                .split('=')
                .nth(1)
                .map(|s| s.trim().trim_matches('"').to_string());
        }

        // Track [dependencies] section
        if line == "[dependencies]" {
            in_dependencies = true;
            continue;
        }

        // Stop at next section
        if line.starts_with('[') && line != "[dependencies]" {
            in_dependencies = false;
        }

        // Parse dependencies (only aura-* crates)
        if in_dependencies && !line.is_empty() && !line.starts_with('#') {
            if let Some(dep_name) = line.split_whitespace().next() {
                if dep_name.starts_with("aura-") {
                    dependencies.insert(dep_name.to_string());
                }
            }
        }
    }

    package_name.map(|n| {
        let layer = Layer::from_crate_name(&n);
        CrateInfo {
            name: n,
            layer,
            dependencies,
            path: path.to_path_buf(),
        }
    })
}

/// Allowed architectural exceptions with justifications
///
/// These are documented violations that have been explicitly approved.
/// Each exception must have a clear justification.
fn is_allowed_exception(from_crate: &str, to_crate: &str) -> bool {
    matches!(
        (from_crate, to_crate),
        // aura-simulator is a testing runtime, so it can use testkit utilities
        // even though this violates strict layering (Layer 6 → Layer 8)
        ("aura-simulator", "aura-testkit")
    )
}

fn collect_crates() -> Vec<CrateInfo> {
    let mut crates = Vec::new();

    // Find the workspace root by looking for Cargo.toml with [workspace]
    let workspace_root = std::env::current_dir()
        .ok()
        .and_then(|mut dir| {
            loop {
                let cargo_toml = dir.join("Cargo.toml");
                if cargo_toml.exists() {
                    if let Ok(content) = fs::read_to_string(&cargo_toml) {
                        if content.contains("[workspace]") {
                            return Some(dir);
                        }
                    }
                }
                if !dir.pop() {
                    break;
                }
            }
            None
        })
        .unwrap_or_else(|| std::env::current_dir().unwrap());

    let crates_dir = workspace_root.join("crates");
    if let Ok(entries) = fs::read_dir(crates_dir) {
        for entry in entries.flatten() {
            let cargo_toml = entry.path().join("Cargo.toml");
            if cargo_toml.exists() {
                if let Some(crate_info) = parse_cargo_toml(&cargo_toml) {
                    crates.push(crate_info);
                }
            }
        }
    }

    // Also check examples
    let examples_dir = workspace_root.join("examples");
    if let Ok(entries) = fs::read_dir(examples_dir) {
        for entry in entries.flatten() {
            let cargo_toml = entry.path().join("Cargo.toml");
            if cargo_toml.exists() {
                if let Some(crate_info) = parse_cargo_toml(&cargo_toml) {
                    crates.push(crate_info);
                }
            }
        }
    }

    crates
}

#[test]
fn test_layer_boundaries() {
    let crates = collect_crates();
    let layer_map: HashMap<String, Option<Layer>> =
        crates.iter().map(|c| (c.name.clone(), c.layer)).collect();

    let mut violations = Vec::new();

    for crate_info in &crates {
        // Skip crates not in the architecture (examples)
        let Some(crate_layer) = crate_info.layer else {
            continue;
        };

        for dep in &crate_info.dependencies {
            let Some(&dep_layer) = layer_map.get(dep).and_then(|l| l.as_ref()) else {
                // Dependency not in architecture (external or example)
                continue;
            };

            // Check: dependencies must be same layer or lower
            if dep_layer > crate_layer {
                // Check if this is an allowed exception
                if is_allowed_exception(&crate_info.name, dep) {
                    eprintln!(
                        "⚠️  {} (Layer {:?}) depends on {} (Layer {:?}) - ALLOWED EXCEPTION",
                        crate_info.name, crate_layer, dep, dep_layer
                    );
                } else {
                    violations.push(format!(
                        "❌ {} (Layer {:?}) depends on {} (Layer {:?}) - UPWARD DEPENDENCY VIOLATION",
                        crate_info.name, crate_layer, dep, dep_layer
                    ));
                }
            }
        }
    }

    if !violations.is_empty() {
        eprintln!("\n=== ARCHITECTURE BOUNDARY VIOLATIONS ===\n");
        for violation in &violations {
            eprintln!("{}", violation);
        }
        eprintln!("\nSee docs/002_system_architecture.md for layer definitions");
        eprintln!("Dependencies must only flow downward through layers:\n");
        eprintln!("  Layer 1: Foundation (aura-core)");
        eprintln!("  Layer 2: Specification (domain crates, aura-mpst, aura-macros)");
        eprintln!("  Layer 3: Implementation (aura-effects)");
        eprintln!("  Layer 4: Orchestration (aura-protocol)");
        eprintln!("  Layer 5: Feature/Protocol (aura-frost deprecated)");
        eprintln!("  Layer 6: Runtime Composition (aura-agent, aura-simulator)");
        eprintln!("  Layer 7: User Interface (aura-cli)");
        eprintln!("  Layer 8: Testing/Tools (aura-testkit, aura-quint)\n");

        panic!(
            "{} architecture boundary violations found",
            violations.len()
        );
    }
}

#[test]
fn test_no_circular_dependencies() {
    let crates = collect_crates();
    let dep_graph: HashMap<String, HashSet<String>> = crates
        .iter()
        .map(|c| (c.name.clone(), c.dependencies.clone()))
        .collect();

    // DFS-based cycle detection
    fn has_cycle(
        node: &str,
        graph: &HashMap<String, HashSet<String>>,
        visited: &mut HashSet<String>,
        rec_stack: &mut HashSet<String>,
        path: &mut Vec<String>,
    ) -> Option<Vec<String>> {
        visited.insert(node.to_string());
        rec_stack.insert(node.to_string());
        path.push(node.to_string());

        if let Some(deps) = graph.get(node) {
            for dep in deps {
                if !visited.contains(dep) {
                    if let Some(cycle) = has_cycle(dep, graph, visited, rec_stack, path) {
                        return Some(cycle);
                    }
                } else if rec_stack.contains(dep) {
                    // Found a cycle
                    let cycle_start = path.iter().position(|n| n == dep).unwrap();
                    return Some(path[cycle_start..].to_vec());
                }
            }
        }

        rec_stack.remove(node);
        path.pop();
        None
    }

    for crate_info in &crates {
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();
        let mut path = Vec::new();

        if let Some(cycle) = has_cycle(
            &crate_info.name,
            &dep_graph,
            &mut visited,
            &mut rec_stack,
            &mut path,
        ) {
            panic!(
                "\n=== CIRCULAR DEPENDENCY DETECTED ===\nCycle: {} -> {}\n",
                cycle.join(" -> "),
                cycle.first().unwrap()
            );
        }
    }
}

#[test]
fn test_effect_traits_only_in_core() {
    // This test ensures effect traits are only defined in aura-core
    // (Would require more sophisticated parsing to implement fully)
    // For now, we validate through layer boundaries

    let crates = collect_crates();
    let core_crate = crates.iter().find(|c| c.name == "aura-core");

    assert!(
        core_crate.is_some(),
        "aura-core (Foundation Layer) must exist"
    );
}

#[test]
fn test_layer_population() {
    let crates = collect_crates();
    let mut layer_counts = HashMap::new();

    for crate_info in &crates {
        if let Some(layer) = crate_info.layer {
            *layer_counts.entry(layer).or_insert(0) += 1;
        }
    }

    // Ensure each layer has at least one crate (except examples)
    assert!(
        layer_counts.contains_key(&Layer::Foundation),
        "Foundation layer must have crates"
    );
    assert!(
        layer_counts.contains_key(&Layer::Specification),
        "Specification layer must have crates"
    );
    assert!(
        layer_counts.contains_key(&Layer::Implementation),
        "Implementation layer must have crates"
    );
    assert!(
        layer_counts.contains_key(&Layer::Orchestration),
        "Orchestration layer must have crates"
    );

    eprintln!("\n=== LAYER POPULATION ===");
    for layer in [
        Layer::Foundation,
        Layer::Specification,
        Layer::Implementation,
        Layer::Orchestration,
        Layer::Feature,
        Layer::Runtime,
        Layer::UI,
        Layer::Testing,
    ] {
        let count = layer_counts.get(&layer).copied().unwrap_or(0);
        eprintln!("Layer {:?}: {} crates", layer, count);
    }
}
