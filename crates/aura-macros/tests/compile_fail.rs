//! Compile-fail boundary tests for aura-macros proc-macro crate.
//!
//! Valid inputs must compile. Invalid inputs must produce clear errors.
//! If a valid choreography is rejected or an invalid one is silently
//! accepted, the DSL contract is broken.

struct TrybuildLock {
    path: std::path::PathBuf,
}

impl Drop for TrybuildLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn acquire_trybuild_lock() -> TrybuildLock {
    let workspace_root = match std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
    {
        Some(path) => path,
        None => panic!("workspace root"),
    };
    let lock_root = workspace_root.join("target/tests");
    if let Err(error) = std::fs::create_dir_all(&lock_root) {
        panic!(
            "failed to create trybuild lock root {}: {error}",
            lock_root.display()
        );
    }
    let lock_path = lock_root.join("trybuild-lock");
    loop {
        match std::fs::create_dir(&lock_path) {
            Ok(()) => return TrybuildLock { path: lock_path },
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(error) => panic!("failed to acquire trybuild lock: {error}"),
        }
    }
}

/// Choreography annotations: valid inputs compile, invalid inputs produce
/// clear errors with guidance toward the fix.
#[test]
fn choreography_annotation_validation() {
    let _lock = acquire_trybuild_lock();
    let t = trybuild::TestCases::new();
    // Valid choreographies must compile
    t.pass("tests/boundaries/valid_annotations.rs");
    t.pass("tests/boundaries/ceremony_facts_valid.rs");
    // Ownership macro valid cases
    t.pass("tests/boundaries/semantic_owner_valid.rs");
    t.pass("tests/boundaries/actor_owned_valid.rs");
    t.pass("tests/boundaries/capability_boundary_valid.rs");
    t.pass("tests/boundaries/ownership_lifecycle_valid.rs");
    // Invalid choreographies must produce clear errors
    t.compile_fail("tests/boundaries/parameterized_roles_and_parallel.rs");
    t.compile_fail("tests/boundaries/invalid_flow_cost.rs");
    t.compile_fail("tests/boundaries/invalid_guard_capability.rs");
    t.compile_fail("tests/boundaries/incoherent_self_send.rs");
    t.compile_fail("tests/boundaries/missing_namespace.rs");
    // Ownership macro rejection cases
    t.compile_fail("tests/boundaries/semantic_owner_missing_context.rs");
    t.compile_fail("tests/boundaries/semantic_owner_missing_owner.rs");
    t.compile_fail("tests/boundaries/semantic_owner_missing_proof.rs");
    t.compile_fail("tests/boundaries/semantic_owner_missing_authoritative_inputs.rs");
    t.compile_fail("tests/boundaries/semantic_owner_missing_category.rs");
    t.compile_fail("tests/boundaries/semantic_owner_missing_terminal_path.rs");
    t.compile_fail("tests/boundaries/actor_owned_missing_capacity.rs");
    t.compile_fail("tests/boundaries/actor_owned_missing_gate.rs");
    t.compile_fail("tests/boundaries/actor_owned_bypass_without_macro.rs");
    t.compile_fail("tests/boundaries/capability_boundary_missing_category.rs");
    t.compile_fail("tests/boundaries/ownership_lifecycle_invalid_variant.rs");
}
