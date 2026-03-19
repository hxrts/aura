//! Compile-fail guards for signals-gated workflow privacy invariants.

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
    let lock_path = lock_root.join("trybuild-lock-signals");
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

#[cfg(feature = "signals")]
#[test]
fn signals_compile_fail_guards() {
    let _lock = acquire_trybuild_lock();
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui_signals/*.rs");
}

#[cfg(not(feature = "signals"))]
#[test]
fn signals_compile_fail_guards() {}
