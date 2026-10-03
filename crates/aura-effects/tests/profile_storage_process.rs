//! Infrastructure tests: real processes contend on the same actual profile.
#![cfg(unix)]
use aura_core::effects::profile_storage::{ProfileStorageEffects, ProfileStorageError};
use aura_effects::profile_storage::FilesystemProfileStorageHandler;
use std::io::{BufRead, Write};

#[test]
fn child_profile_owner() {
    let Some(path) = std::env::var_os("AURA_PROFILE_OWNER_CHILD") else {
        return;
    };
    let handler = FilesystemProfileStorageHandler::new(path.into());
    let _lease = futures::executor::block_on(handler.acquire_profile_lease())
        .expect("child acquires actual OS lease");
    println!("PROFILE_OWNER_READY");
    std::io::stdout().flush().unwrap();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap();
}

#[test]
fn os_process_contention_crash_and_restart_release_exact_profile() {
    let directory = tempfile::tempdir().unwrap();
    let handler = FilesystemProfileStorageHandler::new(directory.path().into());
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_profile_owner", "--nocapture"])
        .env("AURA_PROFILE_OWNER_CHILD", directory.path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let output = child.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in std::io::BufReader::new(output).lines() {
            match line {
                Ok(line) if line == "PROFILE_OWNER_READY" => {
                    let _ = tx.send(());
                    return;
                }
                Err(_) => return,
                _ => {}
            }
        }
    });
    if rx.recv_timeout(std::time::Duration::from_secs(10)).is_err() {
        let _ = child.kill();
        let _ = child.wait();
        let _ = reader.join();
        panic!("child did not acknowledge acquired lease");
    }
    reader.join().unwrap();
    assert!(matches!(
        futures::executor::block_on(handler.acquire_profile_lease()),
        Err(ProfileStorageError::Busy)
    ));
    child.kill().unwrap();
    child.wait().unwrap();
    let lease = futures::executor::block_on(handler.acquire_profile_lease())
        .expect("OS crash releases lease");
    assert!(matches!(
        futures::executor::block_on(handler.acquire_profile_lease()),
        Err(ProfileStorageError::Busy)
    ));
    drop(lease);
    let _restarted = futures::executor::block_on(handler.acquire_profile_lease())
        .expect("cancellation/drop releases lease");
    assert!(
        directory.path().join(".aura-profile-owner.lock").exists(),
        "stable lock inode is retained"
    );
}

#[test]
fn profile_symlink_and_replaced_lock_inode_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let real = root.path().join("real");
    std::fs::create_dir(&real).unwrap();
    let alias = root.path().join("alias");
    std::os::unix::fs::symlink(&real, &alias).unwrap();
    let handler = FilesystemProfileStorageHandler::new(alias);
    assert!(matches!(
        futures::executor::block_on(handler.acquire_profile_lease()),
        Err(ProfileStorageError::Invalid(_))
    ));
    std::os::unix::fs::symlink(
        root.path().join("foreign"),
        real.join(".aura-profile-owner.lock"),
    )
    .unwrap();
    let handler = FilesystemProfileStorageHandler::new(real);
    assert!(matches!(
        futures::executor::block_on(handler.acquire_profile_lease()),
        Err(ProfileStorageError::Io { .. })
    ));
}

#[tokio::test]
async fn exposed_raw_writer_clone_retains_owner_and_clear_preserves_lock_inode() {
    use aura_core::effects::StorageExtendedEffects;
    let directory = tempfile::tempdir().unwrap();
    let factory = FilesystemProfileStorageHandler::new(directory.path().into());
    let owner = std::sync::Arc::new(factory.acquire_owned_native().unwrap());
    let writer = aura_effects::FilesystemStorageHandler::new(directory.path().into())
        .retain_profile_owner(owner)
        .unwrap();
    let cloned = writer.clone();
    drop(writer);
    cloned.clear_all().await.unwrap();
    assert!(matches!(
        factory.acquire_owned_native(),
        Err(ProfileStorageError::Busy)
    ));
    assert!(directory.path().join(".aura-profile-owner.lock").exists());
    drop(cloned);
    let _owner = factory
        .acquire_owned_native()
        .expect("last writer clone releases actual lease");
}

#[tokio::test]
async fn clear_failure_keeps_actual_native_io_error() {
    use aura_core::effects::StorageExtendedEffects;
    use std::error::Error;
    let directory = tempfile::tempdir().unwrap();
    let invalid = directory.path().join("not_a_directory");
    std::fs::write(&invalid, b"file").unwrap();
    let writer = aura_effects::FilesystemStorageHandler::new(invalid);
    let error = writer
        .clear_all()
        .await
        .expect_err("actual filesystem type failure");
    assert!(error
        .source()
        .and_then(|e| e.source())
        .and_then(|e| e.downcast_ref::<std::io::Error>())
        .is_some());
}
