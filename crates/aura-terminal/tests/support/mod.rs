//! Integration-test shared helpers.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Create a unique, empty directory under the OS temp directory.
///
/// Avoids disallowed randomness (`Uuid::new_v4`) while still remaining safe for
/// parallel test execution by using `create_dir` collision avoidance.
pub fn unique_test_dir(prefix: &str) -> PathBuf {
    let root = std::env::temp_dir();
    let mut attempt = COUNTER.fetch_add(1, Ordering::Relaxed);

    for _ in 0..1024 {
        let dir = root.join(format!("{prefix}-{attempt}"));
        match std::fs::create_dir(&dir) {
            Ok(()) => return dir,
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                attempt = attempt.wrapping_add(1);
                continue;
            }
            Err(_) => {
                attempt = attempt.wrapping_add(1);
                continue;
            }
        }
    }

    root.join(prefix)
}
