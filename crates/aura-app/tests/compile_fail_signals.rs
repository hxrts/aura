//! Compile-fail guards for signals-gated workflow privacy invariants.

#![cfg(not(target_arch = "wasm32"))]

#[cfg(feature = "signals")]
use aura_testkit::process_lock::TrybuildProcessLock;

#[cfg(feature = "signals")]
fn require_cargo() {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = std::process::Command::new(cargo)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap_or_else(|source| panic!("required signals Cargo invocation: {source:?}"));
    assert!(
        status.success(),
        "required signals Cargo invocation failed: {status}"
    );
}

#[cfg(feature = "signals")]
fn acquire_trybuild_lock() -> TrybuildProcessLock {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .unwrap_or_else(|| panic!("required signals compile-fail workspace root is missing"));
    TrybuildProcessLock::acquire_workspace(workspace, std::time::Duration::from_secs(900))
        .unwrap_or_else(|error| panic!("required signals compile-fail workspace lock: {error:?}"))
}

#[cfg(feature = "signals")]
#[test]
fn signals_compile_fail_guards() {
    require_cargo();
    let _lock = acquire_trybuild_lock();
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui_signals/*.rs");
}
