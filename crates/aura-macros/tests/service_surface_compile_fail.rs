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
    let lock_path = lock_root.join("trybuild-lock-service-surface");
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

#[test]
fn service_surface_validation() {
    let _lock = acquire_trybuild_lock();
    let t = trybuild::TestCases::new();
    t.pass("tests/boundaries/service_surface_valid.rs");
    t.compile_fail("tests/boundaries/service_surface_missing_select.rs");
    t.compile_fail("tests/boundaries/service_surface_invalid_family.rs");
    t.compile_fail("tests/boundaries/service_surface_authoritative_cache.rs");
    t.compile_fail("tests/boundaries/service_surface_authoritative_social_role.rs");
    t.compile_fail("tests/boundaries/service_surface_runtime_local_social_role.rs");
}
