#[test]
fn choreography_annotation_validation() {
    let t = trybuild::TestCases::new();
    t.pass("tests/trybuild/valid_annotations.rs");
    t.pass("tests/trybuild/ceremony_facts_valid.rs");
    t.compile_fail("tests/trybuild/invalid_flow_cost.rs");
    t.compile_fail("tests/trybuild/invalid_guard_capability.rs");
    t.compile_fail("tests/trybuild/missing_namespace.rs");
}
