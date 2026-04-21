//! Compile-fail boundary tests for aura-macros proc-macro crate.
//!
//! Valid inputs must compile. Invalid inputs must produce clear errors.
//! If a valid choreography is rejected or an invalid one is silently
//! accepted, the DSL contract is broken.

#[path = "support/trybuild.rs"]
mod trybuild_support;

/// Choreography annotations: valid inputs compile, invalid inputs produce
/// clear errors with guidance toward the fix.
#[test]
fn choreography_annotation_validation() {
    if !trybuild_support::trybuild_available() {
        eprintln!("skipping trybuild macro guards: cargo is unavailable");
        return;
    }
    let _lock = trybuild_support::acquire_trybuild_lock("trybuild-lock");
    let t = trybuild::TestCases::new();
    // Valid choreographies must compile
    t.pass("tests/boundaries/valid_annotations.rs");
    t.pass("tests/boundaries/ceremony_facts_valid.rs");
    t.pass("tests/boundaries/capability_family_valid.rs");
    // Ownership macro valid cases
    t.pass("tests/boundaries/semantic_owner_valid.rs");
    t.pass("tests/boundaries/actor_owned_valid.rs");
    t.pass("tests/boundaries/actor_root_valid.rs");
    t.pass("tests/boundaries/capability_boundary_valid.rs");
    t.pass("tests/boundaries/ownership_lifecycle_valid.rs");
    // Invalid choreographies must produce clear errors
    t.compile_fail("tests/boundaries/parameterized_roles_and_parallel.rs");
    t.compile_fail("tests/boundaries/invalid_flow_cost.rs");
    t.compile_fail("tests/boundaries/invalid_guard_capability.rs");
    t.compile_fail("tests/boundaries/legacy_guard_capability_name.rs");
    t.compile_fail("tests/boundaries/comma_joined_guard_capability_name.rs");
    t.compile_fail("tests/boundaries/incoherent_self_send.rs");
    t.compile_fail("tests/boundaries/missing_namespace.rs");
    t.compile_fail("tests/boundaries/capability_family_duplicate_local_name.rs");
    t.compile_fail("tests/boundaries/capability_family_invalid_generated_name.rs");
    t.compile_fail("tests/boundaries/capability_family_invalid_namespace.rs");
    t.compile_fail("tests/boundaries/capability_family_missing_local_name.rs");
    t.compile_fail("tests/boundaries/choreography_namespace_mismatch.rs");
    // Ownership macro rejection cases
    t.compile_fail("tests/boundaries/semantic_owner_missing_context.rs");
    t.compile_fail("tests/boundaries/semantic_owner_missing_owner.rs");
    t.compile_fail("tests/boundaries/semantic_owner_missing_wrapper.rs");
    t.compile_fail("tests/boundaries/semantic_owner_missing_proof.rs");
    t.compile_fail("tests/boundaries/semantic_owner_missing_authoritative_inputs.rs");
    t.compile_fail("tests/boundaries/semantic_owner_missing_category.rs");
    t.compile_fail("tests/boundaries/semantic_owner_missing_terminal_path.rs");
    t.compile_fail("tests/boundaries/semantic_owner_await_before_handoff.rs");
    t.compile_fail("tests/boundaries/actor_owned_missing_capacity.rs");
    t.compile_fail("tests/boundaries/actor_owned_missing_gate.rs");
    t.compile_fail("tests/boundaries/actor_owned_bypass_without_macro.rs");
    t.compile_fail("tests/boundaries/actor_owned_invalid_name.rs");
    t.compile_fail("tests/boundaries/actor_owned_forbidden_field.rs");
    t.compile_fail("tests/boundaries/actor_root_missing_supervision.rs");
    t.compile_fail("tests/boundaries/actor_root_invalid_name.rs");
    t.compile_fail("tests/boundaries/actor_root_forbidden_field.rs");
    t.compile_fail("tests/boundaries/actor_root_unit_struct.rs");
    t.compile_fail("tests/boundaries/actor_root_on_function.rs");
    t.compile_fail("tests/boundaries/capability_boundary_missing_category.rs");
    t.compile_fail("tests/boundaries/capability_boundary_missing_capability.rs");
    t.compile_fail("tests/boundaries/capability_boundary_missing_family.rs");
    t.compile_fail("tests/boundaries/capability_boundary_invalid_family.rs");
    t.compile_fail("tests/boundaries/capability_boundary_non_capability_helper.rs");
    t.compile_fail("tests/boundaries/capability_boundary_proof_issuer_missing_source.rs");
    t.compile_fail("tests/boundaries/ownership_lifecycle_invalid_variant.rs");
}
