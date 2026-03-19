//! Protocol-level coherence and orphan-free checks for consensus choreography patterns.

use aura_mpst::telltale_choreography::{
    ast::local_to_local_r,
    compiler::{parse_choreography_str, project},
};
use std::collections::BTreeMap;
use telltale_theory::orphan_free;

fn strip_aura_annotations_for_parser(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    #[allow(clippy::while_let_on_iterator)]
    while let Some(ch) = chars.next() {
        if ch != '[' {
            out.push(ch);
            continue;
        }

        let mut depth = 1usize;
        let mut buf = String::new();
        let mut has_equals = false;

        while let Some(next) = chars.next() {
            if next == '[' {
                depth += 1;
            } else if next == ']' {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    break;
                }
            }
            if next == '=' {
                has_equals = true;
            }
            buf.push(next);
        }

        if !has_equals {
            out.push('[');
            out.push_str(&buf);
            out.push(']');
        }
    }

    out
}

fn orphan_free_status_for_all_roles(source: &str) -> BTreeMap<String, bool> {
    let parser_source = strip_aura_annotations_for_parser(source);
    let choreography = parse_choreography_str(&parser_source)
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
    let source = include_str!("../../src/protocol/choreography.choreo");
    let orphan_free = orphan_free_status_for_all_roles(source);
    assert!(
        orphan_free.values().any(|ok| !ok),
        "expected at least one non-orphan-free role in consensus choreography"
    );
}
