//! Compile-fail guards for strong command API invariants.

#[test]
fn strong_command_compile_fail_guards() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
