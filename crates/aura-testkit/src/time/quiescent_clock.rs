//! Virtual physical time for multi-runtime tests, advanced only at quiescence.
//!
//! Every runtime of a test shares one [`ManualPhysicalClock`]. A driver task
//! advances it in fixed steps, each step waiting on a tokio sleep. Under a
//! paused tokio clock (`#[tokio::test(start_paused = true)]`) tokio advances
//! its own time only when every task is idle, so the virtual clock moves only
//! once all runtimes are quiescent: timeouts, retries and periodic work fire
//! at the same logical point regardless of host load.
use super::ManualPhysicalClock;
use aura_core::effects::PhysicalTimeEffects;
use std::sync::Arc;
use std::time::Duration;

/// Virtual time origin, far from zero so no runtime sees an epoch-0 clock.
const ORIGIN_MS: u64 = 1_700_000_000_000;
/// Virtual milliseconds added per quiescent step.
const STEP_MS: u64 = 10;

/// One virtual clock shared by every runtime in a test, with its driver.
#[derive(Debug)]
pub struct QuiescentClock {
    clock: Arc<ManualPhysicalClock>,
    driver: tokio::task::JoinHandle<()>,
}

impl QuiescentClock {
    /// Start the clock and its driver on the current (paused) tokio runtime.
    #[must_use]
    pub fn start() -> Self {
        let clock = Arc::new(ManualPhysicalClock::new(ORIGIN_MS));
        let driven = clock.clone();
        let driver = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(STEP_MS)).await;
                driven.advance(STEP_MS);
            }
        });
        Self { clock, driver }
    }

    /// The provider to inject into each runtime.
    #[must_use]
    pub fn provider(&self) -> Arc<dyn PhysicalTimeEffects> {
        self.clock.clone()
    }

    /// Virtual milliseconds elapsed since the clock started.
    #[must_use]
    pub fn elapsed_ms(&self) -> u64 {
        self.clock.now_ms() - ORIGIN_MS
    }
}

impl Drop for QuiescentClock {
    fn drop(&mut self) {
        self.driver.abort();
    }
}
