//! Real-time watchdog for waits driven by virtual time.

use std::time::{Duration, Instant};

/// Measures real time spent in one virtual-time wait. Under a paused tokio
/// clock, virtual time advances only when every task is idle, so a wait that
/// burns real time without reaching its virtual deadline is a livelock: a
/// busy loop, or a re-check that does unbounded work.
#[derive(Debug, Clone, Copy)]
pub struct VirtualTimeStallWatchdog {
    started: Instant,
    limit: Duration,
}

impl VirtualTimeStallWatchdog {
    /// Start a watchdog that reports a stall after `limit` of real time.
    pub fn start(limit: Duration) -> Self {
        Self {
            started: Instant::now(),
            limit,
        }
    }

    /// The real time spent so far, if it exceeds the limit.
    pub fn stalled(&self) -> Option<Duration> {
        let elapsed = self.started.elapsed();
        (elapsed > self.limit).then_some(elapsed)
    }
}
