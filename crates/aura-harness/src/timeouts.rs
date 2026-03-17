use std::time::{Duration, Instant};

/// Central blocking sleep shim for synchronous harness polling paths.
///
/// This keeps wall-clock polling localized while the broader harness timeout
/// rollout removes or replaces remaining blocking loops.
pub fn blocking_sleep(duration: Duration) {
    let deadline = Instant::now() + duration;
    loop {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        std::thread::park_timeout(deadline.saturating_duration_since(now));
    }
}
