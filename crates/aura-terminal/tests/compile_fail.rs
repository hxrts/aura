#![allow(missing_docs)]

#[test]
fn callback_owner_shapes_compile_fail() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
