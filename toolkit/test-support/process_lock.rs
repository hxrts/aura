//! Native process-scoped locks for compile-fail test infrastructure.
//!
//! The persistent lock file is never removed: replacing its inode would allow
//! two processes to believe they own the same shared trybuild workspace.
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::time::Instant;

/// Every Aura compile-fail harness uses this one workspace lock namespace.
pub const TRYBUILD_LOCK_FILE: &str = ".aura-trybuild.lock";

/// Required lock failures retain the actual operating-system cause.
#[derive(Debug, thiserror::Error)]
pub enum ProcessLockError {
    /// Opening the shared lock or acquiring it failed.
    #[error("{operation} shared trybuild lock {path}: {source}")]
    Io {
        /// Operation that failed.
        operation: &'static str,
        /// Actual shared lock path.
        path: PathBuf,
        /// Original operating-system error.
        #[source]
        source: std::io::Error,
    },
    /// Another process held the lock through the caller's bounded wait.
    #[error("shared trybuild lock {path} remained busy for {waited:?}: {source}")]
    TimedOut {
        /// Actual shared lock path.
        path: PathBuf,
        /// Elapsed acquisition time.
        waited: Duration,
        /// Actual last lock-contention error.
        #[source]
        source: std::io::Error,
    },
}

/// A sole descriptor owner; closing it or terminating its process releases the lock.
#[derive(Debug)]
pub struct TrybuildProcessLock {
    _file: File,
}
impl TrybuildProcessLock {
    /// Acquire the shared workspace lock with a bounded contention wait.
    ///
    /// All suites sharing a trybuild workspace must supply the same root. The
    /// lock file persists across test runs and interrupted processes.
    pub fn acquire_workspace(
        workspace: &Path,
        timeout: Duration,
    ) -> Result<Self, ProcessLockError> {
        let root = workspace.join("target/tests");
        std::fs::create_dir_all(&root).map_err(|source| ProcessLockError::Io {
            operation: "create lock parent",
            path: root.clone(),
            source,
        })?;
        let path = root.join(TRYBUILD_LOCK_FILE);
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|source| ProcessLockError::Io {
                operation: "open",
                path: path.clone(),
                source,
            })?;
        // Host test infrastructure cannot import Aura effect implementations
        // into foundation crates. This clock bounds only lock contention.
        #[allow(clippy::disallowed_methods)]
        let started = Instant::now();
        loop {
            match fs2::FileExt::try_lock_exclusive(&file) {
                Ok(()) => return Ok(Self { _file: file }),
                Err(source)
                    if source.raw_os_error() == fs2::lock_contended_error().raw_os_error() =>
                {
                    let waited = started.elapsed();
                    if waited >= timeout {
                        return Err(ProcessLockError::TimedOut {
                            path,
                            waited,
                            source,
                        });
                    }
                    std::thread::sleep(Duration::from_millis(10).min(timeout - waited));
                }
                Err(source) => {
                    return Err(ProcessLockError::Io {
                        operation: "lock",
                        path,
                        source,
                    })
                }
            }
        }
    }
}
