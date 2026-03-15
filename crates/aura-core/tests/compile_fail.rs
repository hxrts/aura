//! Compile-fail guards for ownership capability boundaries.

#[test]
fn ownership_compile_fail_guards() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
