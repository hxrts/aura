//! Compile-fail guards for strong command API invariants.

#![cfg(not(target_arch = "wasm32"))]

use aura_testkit::process_lock::TrybuildProcessLock;

fn require_cargo() {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = std::process::Command::new(cargo)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap_or_else(|source| panic!("required compile-fail Cargo invocation: {source:?}"));
    assert!(
        status.success(),
        "required compile-fail Cargo invocation failed: {status}"
    );
}

fn acquire_trybuild_lock() -> TrybuildProcessLock {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .unwrap_or_else(|| panic!("compile-fail crate must reside under the workspace root"));
    TrybuildProcessLock::acquire_workspace(workspace, std::time::Duration::from_secs(900))
        .unwrap_or_else(|error| panic!("required compile-fail workspace lock: {error:?}"))
}

#[test]
fn strong_command_compile_fail_guards() {
    require_cargo();
    let _lock = acquire_trybuild_lock();
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
