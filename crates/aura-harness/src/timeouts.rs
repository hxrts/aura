use std::time::{Duration, Instant};

/// Central blocking sleep shim for synchronous harness polling paths.
///
/// This keeps wall-clock polling localized while the broader harness timeout
/// rollout removes or replaces remaining blocking loops.
pub fn blocking_sleep(duration: Duration) {
    #[allow(clippy::disallowed_methods)]
    let deadline = Instant::now() + duration;
    loop {
        #[allow(clippy::disallowed_methods)]
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        std::thread::park_timeout(deadline.saturating_duration_since(now));
    }
}

/// Shared blocking poll helper for synchronous harness paths and tests.
///
/// The caller owns the local timeout budget and supplies a poll interval. The
/// closure returns `Some(T)` when the condition is satisfied.
pub fn blocking_wait_until<T, F>(
    timeout: Duration,
    poll_interval: Duration,
    mut poll: F,
) -> Option<T>
where
    F: FnMut() -> Option<T>,
{
    #[allow(clippy::disallowed_methods)]
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(value) = poll() {
            return Some(value);
        }
        #[allow(clippy::disallowed_methods)]
        let now = Instant::now();
        if now >= deadline {
            return None;
        }
        blocking_sleep(poll_interval.min(deadline.saturating_duration_since(now)));
    }
}
