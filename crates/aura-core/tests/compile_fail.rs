//! Compile-fail guards for ownership capability boundaries.
use aura_build_support as process_lock;

#[test]
fn ownership_compile_fail_guards() {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    assert!(
        std::process::Command::new(cargo)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("required Cargo must be invocable")
            .success(),
        "required Cargo must succeed"
    );
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root");
    let _lock = process_lock::TrybuildProcessLock::acquire_workspace(
        workspace,
        std::time::Duration::from_secs(900),
    )
    .expect("bounded shared compile-fail lock");
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/boundaries/*.rs");
}
