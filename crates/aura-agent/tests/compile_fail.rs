//! Compile-fail tests for the builder typestate pattern.
//!
//! These tests verify that the CustomPresetBuilder correctly enforces
//! at compile-time that all required effects must be provided before
//! calling `build()`.

#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
