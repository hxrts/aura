//! Builder bootstrap-required contract tests.

#![allow(clippy::expect_used)]

use aura_agent::AgentBuilder;

#[test]
fn cli_builder_requires_explicit_authority() {
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let result = AgentBuilder::cli()
        .data_dir(temp_dir.path())
        .testing_mode()
        .build_sync();

    let error = match result {
        Ok(_) => panic!("cli builder should reject missing authority"),
        Err(error) => error,
    };

    assert!(
        error.to_string().contains("bootstrap required"),
        "unexpected error: {error}"
    );
}
