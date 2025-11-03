//! Time effects for scheduling and temporal coordination

use crate::AuraError;
use std::future::Future;
use std::time::Duration;

/// Wake conditions for cooperative yielding
#[derive(Debug, Clone)]
pub enum WakeCondition {
    /// Wake when new events are available
    NewEvents,
    /// Wake when a specific epoch/timestamp is reached
    EpochReached(u64),
    /// Wake after a timeout at specific timestamp
    TimeoutAt(u64),
    /// Wake when an event matching criteria is received
    EventMatching(String),
    /// Wake when threshold number of events received
    ThresholdEvents {
        /// Number of events to wait for before waking
        threshold: usize,
        /// Maximum time to wait in milliseconds
        timeout_ms: u64,
    },
}

/// Time source abstraction for testing
pub trait TimeSource {
    /// Get the current timestamp/epoch
    fn now(&self) -> u64;

    /// Sleep for a duration
    fn sleep(&self, duration: Duration) -> impl Future<Output = ()> + Send;
}

/// Time effects interface
pub trait TimeEffects {
    /// Get the current timestamp/epoch
    fn current_timestamp(&self) -> u64;

    /// Delay execution for a specified duration
    fn delay(&self, duration: Duration) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + '_>>;

    /// Yield execution until a condition is met
    fn yield_until(
        &self,
        condition: WakeCondition,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<(), AuraError>> + Send + '_>>;
}

/// Production time effects using system time
pub struct ProductionTimeEffects {
    /// Starting time reference for elapsed time calculations
    start_time: std::time::Instant,
}

impl ProductionTimeEffects {
    /// Create a new production time effects handler
    pub fn new() -> Self {
        #[allow(clippy::disallowed_methods)]
        Self {
            start_time: std::time::Instant::now(),
        }
    }
}

impl TimeSource for ProductionTimeEffects {
    fn now(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }

    fn sleep(&self, duration: Duration) -> impl Future<Output = ()> + Send {
        async move {
            // Placeholder - would use tokio::time::sleep in production
            std::thread::sleep(duration);
        }
    }
}

impl TimeEffects for ProductionTimeEffects {
    fn current_timestamp(&self) -> u64 {
        self.now()
    }

    fn delay(&self, duration: Duration) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(self.sleep(duration))
    }

    fn yield_until(
        &self,
        condition: WakeCondition,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<(), AuraError>> + Send + '_>> {
        let current_time = self.current_timestamp();
        Box::pin(async move {
            match condition {
                WakeCondition::NewEvents => {
                    // Just yield control in production (placeholder)
                    Ok(())
                }
                WakeCondition::EpochReached(target) => {
                    if target > current_time {
                        let duration = Duration::from_secs(target - current_time);
                        std::thread::sleep(duration); // Placeholder
                    }
                    Ok(())
                }
                WakeCondition::TimeoutAt(target) => {
                    if target > current_time {
                        let duration = Duration::from_secs(target - current_time);
                        std::thread::sleep(duration); // Placeholder
                    }
                    Ok(())
                }
                WakeCondition::EventMatching(_) => {
                    // For production, just yield - would need event system integration
                    Ok(())
                }
                WakeCondition::ThresholdEvents { timeout_ms, .. } => {
                    // For production, just wait for timeout
                    let duration = Duration::from_millis(timeout_ms);
                    std::thread::sleep(duration); // Placeholder
                    Ok(())
                }
            }
        })
    }
}

/// Test time effects with controllable time progression
pub struct TestTimeEffects {
    current_time: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl TestTimeEffects {
    /// Create a new test time effects instance with initial time set to seed
    ///
    /// # Arguments
    /// * `seed` - The initial time value (in seconds since epoch)
    pub fn new(seed: u64) -> Self {
        Self {
            current_time: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(seed)),
        }
    }

    /// Manually advance time by the specified amount
    pub fn advance_time(&self, seconds: u64) {
        self.current_time
            .fetch_add(seconds, std::sync::atomic::Ordering::SeqCst);
    }

    /// Set the current time to a specific value
    pub fn set_time(&self, timestamp: u64) {
        self.current_time
            .store(timestamp, std::sync::atomic::Ordering::SeqCst);
    }
}

impl TimeSource for TestTimeEffects {
    fn now(&self) -> u64 {
        self.current_time.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn sleep(&self, duration: Duration) -> impl Future<Output = ()> + Send {
        // In test mode, immediately advance time instead of actually sleeping
        let current_time = self.current_time.clone();
        async move {
            current_time.fetch_add(duration.as_secs(), std::sync::atomic::Ordering::SeqCst);
        }
    }
}

impl TimeEffects for TestTimeEffects {
    fn current_timestamp(&self) -> u64 {
        self.now()
    }

    fn delay(&self, duration: Duration) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(self.sleep(duration))
    }

    fn yield_until(
        &self,
        condition: WakeCondition,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<(), AuraError>> + Send + '_>> {
        let current_time = self.current_time.clone();
        Box::pin(async move {
            match condition {
                WakeCondition::NewEvents => {
                    // In test mode, just advance time slightly
                    current_time.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(())
                }
                WakeCondition::EpochReached(target) => {
                    let current = current_time.load(std::sync::atomic::Ordering::SeqCst);
                    if target > current {
                        current_time.store(target, std::sync::atomic::Ordering::SeqCst);
                    }
                    Ok(())
                }
                WakeCondition::TimeoutAt(target) => {
                    let current = current_time.load(std::sync::atomic::Ordering::SeqCst);
                    if target > current {
                        current_time.store(target, std::sync::atomic::Ordering::SeqCst);
                    }
                    Ok(())
                }
                WakeCondition::EventMatching(_) => {
                    // In test mode, just advance time slightly
                    current_time.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(())
                }
                WakeCondition::ThresholdEvents { timeout_ms, .. } => {
                    // In test mode, advance by timeout duration
                    current_time.fetch_add(timeout_ms / 1000, std::sync::atomic::Ordering::SeqCst);
                    Ok(())
                }
            }
        })
    }
}
