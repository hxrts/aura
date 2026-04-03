//! Protocol-level validation helpers for choreography tests.
//!
//! These assertions are intended for test code in Layer 5+ crates to enforce
//! coherence and orphan-free properties on choreography sources.

use aura_mpst::upstream::language::{
    ast::{choreography_to_global, local_to_local_r, LocalTypeR},
    parse_choreography_str, project,
};
use std::collections::BTreeMap;
use telltale_theory::coherence::check_coherent;
use telltale_theory::subtyping::{async_subtype, orphan_free};

fn project_locals_by_role(source: &str, label: &str) -> BTreeMap<String, LocalTypeR> {
    let choreography = parse_choreography_str(source)
        .unwrap_or_else(|err| panic!("{label}: failed to parse choreography source: {err}"));

    let mut locals = BTreeMap::new();
    for role in &choreography.roles {
        let local = project(&choreography, role).unwrap_or_else(|err| {
            panic!("{label}: projection failed for role {}: {err}", role.name())
        });
        let local_r = local_to_local_r(&local).unwrap_or_else(|err| {
            panic!(
                "{label}: local conversion failed for role {}: {err}",
                role.name()
            )
        });
        locals.insert(role.name().to_string(), local_r);
    }
    locals
}

/// Assert that a choreography source is coherent under telltale-theory checks.
pub fn assert_protocol_coherent(source: &str) {
    let choreography = parse_choreography_str(source)
        .unwrap_or_else(|err| panic!("coherence: failed to parse choreography source: {err}"));
    let global = choreography_to_global(&choreography).unwrap_or_else(|err| {
        panic!("coherence: failed to convert choreography to theory global: {err}")
    });

    let bundle = check_coherent(&global);
    assert!(
        bundle.is_coherent(),
        "coherence failed: size={}, action={}, uniq_labels={}, projectable={}, good={}",
        bundle.size,
        bundle.action,
        bundle.uniq_labels,
        bundle.projectable,
        bundle.good
    );
}

/// Assert that every role projection in a choreography is orphan-free.
pub fn assert_orphan_free_for_all_roles(source: &str) {
    for (role, local) in orphan_free_status_for_all_roles(source) {
        assert!(local, "orphan-free failed for role `{role}`");
    }
}

/// Compute orphan-free status for each projected role in a choreography.
pub fn orphan_free_status_for_all_roles(source: &str) -> BTreeMap<String, bool> {
    let locals = project_locals_by_role(source, "orphan-free");
    locals
        .into_iter()
        .map(|(role, local)| (role, orphan_free(&local)))
        .collect()
}

/// Assert protocol evolution compatibility (`new` is an async subtype of `old`)
/// for all roles present in both protocol versions.
pub fn assert_async_subtype_for_shared_roles(old_source: &str, new_source: &str) {
    check_async_subtype_for_shared_roles(old_source, new_source)
        .unwrap_or_else(|err| panic!("{err}"));
}

/// Check protocol evolution compatibility (`new` is an async subtype of `old`)
/// for all roles present in both protocol versions.
pub fn check_async_subtype_for_shared_roles(
    old_source: &str,
    new_source: &str,
) -> Result<(), String> {
    let old_locals = project_locals_by_role(old_source, "old protocol");
    let new_locals = project_locals_by_role(new_source, "new protocol");

    for (role, old_local) in &old_locals {
        if let Some(new_local) = new_locals.get(role) {
            async_subtype(new_local, old_local).map_err(|err| {
                format!(
                    "async subtype failed for role `{role}`: new is not a subtype of old: {err}"
                )
            })?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use aura_mpst::upstream::language::parse_choreography_str;

    #[test]
    fn parses_telltale_ten_sender_record_annotations() {
        let source = r#"
protocol Demo =
  roles A, B

  A { guard_capability : "demo:start" } -> B : Msg of crate.demo.Payload
"#;

        parse_choreography_str(source).expect("sender-record annotations should parse directly");
    }

    #[test]
    fn parses_telltale_ten_choice_surface_directly() {
        let source = r#"
protocol Demo =
  roles A, B

  choice A at
    | accept =>
      A -> B : Msg of crate.demo.Payload
"#;

        parse_choreography_str(source).expect("choice surface should parse directly");
    }
}
