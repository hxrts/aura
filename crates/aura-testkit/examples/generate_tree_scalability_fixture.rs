//! Regenerate structural benchmark input using the canonical journal reducer.
//!
//! Run from the workspace root:
//! AURA_BUILD_PACKAGES=aura-testkit scripts/dev/build-budget.sh --lane tree-scalability-generate --no-prune -- \
//!   bash -c 'cargo run -p aura-testkit --example generate_tree_scalability_fixture > /tmp/tree-scalability.json'
//! Compare the output before replacing tests/fixtures/tree_scalability_history.json.

#[path = "../tests/fixtures/tree_scalability_history.rs"]
mod tree_scalability_history;

fn main() {
    let fixture = tree_scalability_history::connected_tree_history(100, 1000);
    println!(
        "{}",
        serde_json::to_string(&fixture)
            .unwrap_or_else(|error| panic!("serialize deterministic structural history: {error}"))
    );
}
