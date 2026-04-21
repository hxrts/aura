#[path = "support/trybuild.rs"]
mod trybuild_support;

#[test]
fn marker_attribute_validation() {
    let _lock = trybuild_support::acquire_trybuild_lock("trybuild-lock-marker-attrs");
    let t = trybuild::TestCases::new();
    t.pass("tests/boundaries/authoritative_source_valid.rs");
    t.pass("tests/boundaries/strong_reference_valid.rs");
    t.pass("tests/boundaries/weak_identifier_valid.rs");
    t.compile_fail("tests/boundaries/authoritative_source_missing_kind.rs");
    t.compile_fail("tests/boundaries/authoritative_source_invalid_kind.rs");
    t.compile_fail("tests/boundaries/authoritative_source_on_struct.rs");
    t.compile_fail("tests/boundaries/strong_reference_missing_domain.rs");
    t.compile_fail("tests/boundaries/strong_reference_invalid_domain.rs");
    t.compile_fail("tests/boundaries/strong_reference_on_function.rs");
    t.compile_fail("tests/boundaries/weak_identifier_missing_domain.rs");
    t.compile_fail("tests/boundaries/weak_identifier_invalid_domain.rs");
    t.compile_fail("tests/boundaries/weak_identifier_on_function.rs");
}
