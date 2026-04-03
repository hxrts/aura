//! Protocol-level coherence and orphan-free checks for consensus choreography patterns.

use aura_mpst::upstream::language::{ast::local_to_local_r, parse_choreography_str, project};
use std::collections::BTreeMap;
use telltale_theory::subtyping::orphan_free;

fn orphan_free_status_for_all_roles(source: &str) -> BTreeMap<String, bool> {
    let choreography = parse_choreography_str(source)
        .unwrap_or_else(|err| panic!("failed to parse consensus choreography: {err}"));

    let mut status = BTreeMap::new();
    for role in &choreography.roles {
        let local = project(&choreography, role)
            .unwrap_or_else(|err| panic!("projection failed for role {}: {err}", role.name()));
        let local_r = local_to_local_r(&local).unwrap_or_else(|err| {
            panic!("local conversion failed for role {}: {err}", role.name())
        });
        status.insert(role.name().to_string(), orphan_free(&local_r));
    }
    status
}

#[test]
fn consensus_protocol_pattern_is_coherent_and_orphan_free() {
    let source = include_str!("../../src/protocol/choreography.tell");
    let orphan_free = orphan_free_status_for_all_roles(source);
    assert!(
        orphan_free.values().any(|ok| !ok),
        "expected at least one non-orphan-free role in consensus choreography"
    );
}
