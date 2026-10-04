//! Actual process termination and namespace regressions for compile-fail locks.
#![cfg(not(target_arch = "wasm32"))]
use aura_effects::time::monotonic_now;
use aura_testkit::process_lock::{ProcessLockError, TrybuildProcessLock, TRYBUILD_LOCK_FILE};
use std::error::Error;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const CHILD_ROOT: &str = "AURA_TEST_TRYBUILD_LOCK_CHILD_ROOT";

#[test]
fn process_lock_child() {
    let Some(root) = std::env::var_os(CHILD_ROOT) else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let _owner = TrybuildProcessLock::acquire_workspace(&root, Duration::from_secs(5)).unwrap();
    std::fs::write(root.join("holder-ready"), b"ready").unwrap();
    loop {
        std::thread::park();
    }
}

struct ChildOwner(Child);
impl Drop for ChildOwner {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn wait_for_child_publication(child: &mut Child, root: &Path) -> std::io::Result<()> {
    let started = monotonic_now();
    loop {
        match std::fs::read(root.join("holder-ready")) {
            Ok(bytes) => {
                assert_eq!(bytes, b"ready");
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        assert!(
            child.try_wait()?.is_none(),
            "child exited before holding lock"
        );
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "child did not publish readiness"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn forced_process_termination_releases_same_persistent_lock_file() {
    let root = tempfile::tempdir().unwrap();
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "process_lock_child", "--nocapture"])
        .env(CHILD_ROOT, root.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let mut child = ChildOwner(child);
    wait_for_child_publication(&mut child.0, root.path())
        .expect("actual child lock publication and process status");
    let busy = TrybuildProcessLock::acquire_workspace(root.path(), Duration::from_millis(40))
        .expect_err("actual second process cannot acquire the held descriptor lock");
    assert!(matches!(busy, ProcessLockError::TimedOut { .. }));
    assert!(busy
        .source()
        .unwrap()
        .downcast_ref::<std::io::Error>()
        .is_some());
    let path = root.path().join("target/tests").join(TRYBUILD_LOCK_FILE);
    #[cfg(unix)]
    let original_inode = {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(&path).unwrap().ino()
    };
    child.0.kill().unwrap();
    let started = monotonic_now();
    while child.0.try_wait().unwrap().is_none() {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "forced child termination did not complete"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let owner = TrybuildProcessLock::acquire_workspace(root.path(), Duration::from_secs(2))
        .expect("OS releases the lock even when Rust Drop never runs");
    assert!(path.exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(std::fs::metadata(&path).unwrap().ino(), original_inode);
    }
    drop(owner);
    assert!(path.exists(), "releasing a lock must not remove its inode");
}

#[test]
fn all_compile_fail_suites_share_the_workspace_namespace() {
    let root = tempfile::tempdir().unwrap();
    // Legacy interrupted directories no longer control acquisition.
    std::fs::create_dir_all(root.path().join("target/tests/trybuild-lock")).unwrap();
    std::fs::create_dir_all(root.path().join("target/tests/trybuild-lock-signals")).unwrap();
    let app_owner = TrybuildProcessLock::acquire_workspace(root.path(), Duration::ZERO).unwrap();
    for suite in ["agent", "signals"] {
        let error =
            TrybuildProcessLock::acquire_workspace(root.path(), Duration::ZERO).expect_err(suite);
        assert!(matches!(error, ProcessLockError::TimedOut { .. }));
    }
    drop(app_owner);
    TrybuildProcessLock::acquire_workspace(root.path(), Duration::ZERO).unwrap();
}

#[test]
fn invalid_lock_parent_retains_actual_io_cause() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("not-a-directory");
    std::fs::write(&file, b"fixture").unwrap();
    let error = TrybuildProcessLock::acquire_workspace(&file, Duration::ZERO).unwrap_err();
    assert!(matches!(error, ProcessLockError::Io { .. }));
    assert!(error
        .source()
        .unwrap()
        .downcast_ref::<std::io::Error>()
        .is_some());
}
