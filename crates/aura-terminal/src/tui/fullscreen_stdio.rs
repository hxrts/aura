//! TUI stdio control.
//!
//! Writing to stdout/stderr while iocraft is in fullscreen mode can scroll the
//! terminal buffer and leave stale UI rows behind (e.g., a "duplicated" nav bar).
//!
//! This module provides an RAII guard that redirects stdout/stderr to `/dev/null`
//! for the duration of fullscreen rendering.

#[cfg(unix)]
mod imp {
    use std::fs::OpenOptions;
    use std::io;
    use std::io::Write;
    use std::os::unix::io::AsRawFd;

    use nix::libc;
    use nix::unistd::{close, dup, dup2};

    /// Redirects stderr to `/dev/null` while alive.
    ///
    /// IMPORTANT: We intentionally do **not** redirect stdout because iocraft
    /// renders to stdout.
    ///
    /// Use `AURA_TUI_ALLOW_STDIO=1` to disable redirection for debugging.
    pub struct FullscreenStdioGuard {
        saved_stderr: i32,
    }

    impl FullscreenStdioGuard {
        pub fn redirect_stderr_to_null() -> io::Result<Self> {
            // Flush before we steal the fds.
            let _ = io::stdout().lock().flush();
            let _ = io::stderr().lock().flush();

            let null = OpenOptions::new().write(true).open("/dev/null")?;
            let null_fd = null.as_raw_fd();

            // Duplicate current stderr so we can restore later.
            let saved_stderr = dup(libc::STDERR_FILENO).map_err(nix_err)?;

            // Redirect stderr to /dev/null.
            if let Err(e) = dup2(null_fd, libc::STDERR_FILENO) {
                let _ = dup2(saved_stderr, libc::STDERR_FILENO);
                let _ = close(saved_stderr);
                return Err(nix_err(e));
            }

            // Keep `null` alive until after dup2 calls by holding it in scope.
            drop(null);

            Ok(Self { saved_stderr })
        }
    }

    impl Drop for FullscreenStdioGuard {
        fn drop(&mut self) {
            // Restore stderr even if callers panicked.
            let _ = dup2(self.saved_stderr, libc::STDERR_FILENO);
            let _ = close(self.saved_stderr);

            let _ = io::stdout().lock().flush();
            let _ = io::stderr().lock().flush();
        }
    }

    fn nix_err(err: nix::Error) -> io::Error {
        // nix 0.26 uses `nix::Error` as an errno-like value.
        io::Error::from_raw_os_error(err as i32)
    }
}

#[cfg(not(unix))]
mod imp {
    use std::io;

    /// No-op on non-Unix platforms.
    pub struct FullscreenStdioGuard;

    impl FullscreenStdioGuard {
        pub fn redirect_stderr_to_null() -> io::Result<Self> {
            Ok(Self)
        }
    }
}

pub use imp::FullscreenStdioGuard;
