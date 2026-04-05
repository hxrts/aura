//! Compile-fail tests for the builder typestate pattern.
//!
//! These tests verify that the CustomPresetBuilder correctly enforces
//! at compile-time that all required effects must be provided before
//! calling `build()`.

struct TrybuildLock {
    path: std::path::PathBuf,
}

impl Drop for TrybuildLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn trybuild_available() -> bool {
    std::env::var_os("CARGO").is_some()
        || std::process::Command::new("cargo")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok()
}

fn trybuild_root() -> std::path::PathBuf {
    let workspace_root = match std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
    {
        Some(path) => path.to_path_buf(),
        None => panic!("workspace root"),
    };
    let preferred = workspace_root.join("target/tests");
    if std::fs::create_dir_all(&preferred).is_ok() {
        return preferred;
    }

    let fallback = std::env::temp_dir()
        .join("aura-trybuild")
        .join(env!("CARGO_PKG_NAME"));
    std::fs::create_dir_all(&fallback).unwrap_or_else(|error| {
        panic!(
            "failed to create fallback trybuild root {}: {error}",
            fallback.display()
        )
    });
    fallback
}

fn acquire_trybuild_lock() -> TrybuildLock {
    let lock_root = trybuild_root();
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

#[test]
fn ui() {
    if !trybuild_available() {
        eprintln!("skipping trybuild ui guards: cargo is unavailable");
        return;
    }
    let _lock = acquire_trybuild_lock();
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
