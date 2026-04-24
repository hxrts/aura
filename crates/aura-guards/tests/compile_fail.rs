//! Compile-fail guard operation API boundary tests.

#[test]
fn guard_operation_compile_fail_guards() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/boundaries/*.rs");
}
