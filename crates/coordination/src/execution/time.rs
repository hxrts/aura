//! Simulation-First Time and Event Architecture
//!
//! This module implements cooperative yielding and event-driven protocol execution
//! that works identically in both simulation and production environments.
//!
//! Key principles:
//! - Zero polling: All waiting is event-driven with specific wake conditions
//! - Deterministic simulation: Time advancement is controlled by simulation engine
//! - Clean architecture: Single execution model for all environments

use super::types::{EventFilter, ProtocolError};
use uuid::Uuid;

/// Wake conditions for cooperative yielding in protocol execution
#[derive(Debug, Clone)]
pub enum WakeCondition {
    /// Wake when new events are available for this context
    NewEvents,
    /// Wake when simulated time reaches this epoch
    EpochReached(u64),
    /// Wake when timeout expires
    TimeoutAt(u64),
    /// Wake when specific event pattern appears
    EventMatching(EventFilter),
    /// Wake when threshold number of matching events received
    ThresholdEvents { count: usize, filter: EventFilter },
}

/// Time source abstraction for simulation and production environments
#[async_trait::async_trait]
pub trait TimeSource: Send + Sync + dyn_clone::DynClone {
    /// Get current epoch (simulation tick or wall clock)
    fn current_epoch(&self) -> u64;

    /// Yield execution until condition is met
    async fn yield_until(&self, condition: WakeCondition) -> Result<(), ProtocolError>;

    /// Check if we're running in simulation mode
    fn is_simulated(&self) -> bool;

    /// Register a context for wake condition notifications
    fn register_context(&self, context_id: Uuid);

    /// Unregister a context (cleanup on completion)
    fn unregister_context(&self, context_id: Uuid);

    /// Notify that new events are available (for immediate wake-up)
    async fn notify_events_available(&self);
}

dyn_clone::clone_trait_object!(TimeSource);

/// Simulation-based time source with cooperative scheduling
#[derive(Clone)]
pub struct SimulatedTimeSource {
    context_id: Uuid,
    scheduler: std::sync::Arc<tokio::sync::RwLock<SimulationScheduler>>,
}

impl SimulatedTimeSource {
    pub fn new(
        context_id: Uuid,
        scheduler: std::sync::Arc<tokio::sync::RwLock<SimulationScheduler>>,
    ) -> Self {
        Self {
            context_id,
            scheduler,
        }
    }
}

#[async_trait::async_trait]
impl TimeSource for SimulatedTimeSource {
    fn current_epoch(&self) -> u64 {
        // Get current simulation tick
        let scheduler = futures::executor::block_on(self.scheduler.read());
        scheduler.current_tick()
    }

    async fn yield_until(&self, condition: WakeCondition) -> Result<(), ProtocolError> {
        // Register condition and obtain receiver without holding the lock across await
        let receiver = {
            let mut scheduler = self.scheduler.write().await;
            match scheduler.prepare_wake_condition(self.context_id, condition.clone()) {
                PrepareResult::AlreadySatisfied => return Ok(()),
                PrepareResult::Receiver(rx) => rx,
            }
        };

        // Await outside of lock to avoid deadlocks
        let _ = receiver.await;

        // Clean up any lingering state
        let mut scheduler = self.scheduler.write().await;
        scheduler.complete_wake(self.context_id);

        Ok(())
    }

    fn is_simulated(&self) -> bool {
        true
    }

    fn register_context(&self, context_id: Uuid) {
        let mut scheduler = futures::executor::block_on(self.scheduler.write());
        scheduler.register_context(context_id);
    }

    fn unregister_context(&self, context_id: Uuid) {
        let mut scheduler = futures::executor::block_on(self.scheduler.write());
        scheduler.unregister_context(context_id);
    }

    async fn notify_events_available(&self) {
        let mut scheduler = self.scheduler.write().await;
        scheduler.notify_events_available_globally();
    }
}

/// Production time source using wall clock and async notifications
#[derive(Clone)]
pub struct ProductionTimeSource {
    event_notifier: std::sync::Arc<tokio::sync::Notify>,
    start_time: std::time::Instant,
}

impl ProductionTimeSource {
    pub fn new() -> Self {
        Self {
            event_notifier: std::sync::Arc::new(tokio::sync::Notify::new()),
            start_time: std::time::Instant::now(),
        }
    }

    /// Notify waiting contexts that new events are available
    pub fn notify_new_events(&self) {
        self.event_notifier.notify_waiters();
    }
}

impl Default for ProductionTimeSource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl TimeSource for ProductionTimeSource {
    fn current_epoch(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }

    async fn yield_until(&self, condition: WakeCondition) -> Result<(), ProtocolError> {
        match condition {
            WakeCondition::NewEvents => {
                // Use async notification for new events
                self.event_notifier.notified().await;
                Ok(())
            }
            WakeCondition::EpochReached(target) => {
                let current = self.current_epoch();
                if target > current {
                    let duration = std::time::Duration::from_secs(target - current);
                    tokio::time::sleep(duration).await;
                }
                Ok(())
            }
            WakeCondition::TimeoutAt(target) => {
                let current = self.current_epoch();
                if target > current {
                    let duration = std::time::Duration::from_secs(target - current);
                    tokio::time::sleep(duration).await;
                }
                Ok(())
            }
            WakeCondition::EventMatching(_) => {
                // For production, fall back to event notification
                self.event_notifier.notified().await;
                Ok(())
            }
            WakeCondition::ThresholdEvents { .. } => {
                // For production, fall back to event notification
                self.event_notifier.notified().await;
                Ok(())
            }
        }
    }

    fn is_simulated(&self) -> bool {
        false
    }

    fn register_context(&self, _context_id: Uuid) {
        // No-op for production
    }

    fn unregister_context(&self, _context_id: Uuid) {
        // No-op for production
    }

    async fn notify_events_available(&self) {
        self.event_notifier.notify_waiters();
    }
}

/// Simulation scheduler that manages wake conditions and context coordination
pub struct SimulationScheduler {
    current_tick: u64,
    waiting_contexts: std::collections::HashMap<Uuid, WakeCondition>,
    context_wakers: std::collections::HashMap<Uuid, tokio::sync::oneshot::Sender<()>>,
    active_contexts: std::collections::HashSet<Uuid>,
}

/// Result of registering a wake condition with the scheduler
pub enum PrepareResult {
    AlreadySatisfied,
    Receiver(tokio::sync::oneshot::Receiver<()>),
}

impl SimulationScheduler {
    pub fn new() -> Self {
        Self {
            current_tick: 0,
            waiting_contexts: std::collections::HashMap::new(),
            context_wakers: std::collections::HashMap::new(),
            active_contexts: std::collections::HashSet::new(),
        }
    }

    pub fn current_tick(&self) -> u64 {
        self.current_tick
    }

    pub fn register_context(&mut self, context_id: Uuid) {
        self.active_contexts.insert(context_id);
    }

    pub fn unregister_context(&mut self, context_id: Uuid) {
        self.active_contexts.remove(&context_id);
        self.waiting_contexts.remove(&context_id);
        self.context_wakers.remove(&context_id);
    }

    /// Check if there are any contexts waiting on conditions
    pub fn has_waiting_contexts(&self) -> bool {
        !self.waiting_contexts.is_empty()
    }

    /// Check if there are any active contexts that might still be running
    pub fn has_active_contexts(&self) -> bool {
        !self.active_contexts.is_empty()
    }

    /// Get the number of active contexts
    pub fn active_context_count(&self) -> usize {
        self.active_contexts.len()
    }

    /// Get the number of waiting contexts
    pub fn waiting_context_count(&self) -> usize {
        self.waiting_contexts.len()
    }

    /// Prepare a wake condition, returning a receiver that can be awaited
    pub fn prepare_wake_condition(
        &mut self,
        context_id: Uuid,
        condition: WakeCondition,
    ) -> PrepareResult {
        if self.condition_satisfied(&condition) {
            return PrepareResult::AlreadySatisfied;
        }
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.waiting_contexts.insert(context_id, condition);
        self.context_wakers.insert(context_id, sender);
        PrepareResult::Receiver(receiver)
    }

    /// Complete a wake condition registration, cleaning up state
    pub fn complete_wake(&mut self, context_id: Uuid) {
        self.waiting_contexts.remove(&context_id);
        self.context_wakers.remove(&context_id);
    }

    pub fn advance_time(&mut self, ticks: u64) {
        self.current_tick += ticks;

        // Wake contexts waiting for time conditions
        let mut contexts_to_wake = Vec::new();

        for (context_id, condition) in &self.waiting_contexts {
            if self.condition_satisfied(condition) {
                contexts_to_wake.push(*context_id);
            }
        }

        for context_id in contexts_to_wake {
            self.wake_context(context_id);
        }
    }

    pub fn notify_events_available(&mut self, _context_id: Uuid) {
        // Wake ALL contexts waiting for events (since we don't know which specific events were added)
        let contexts_to_wake: Vec<Uuid> = self
            .waiting_contexts
            .iter()
            .filter_map(|(context_id, condition)| match condition {
                WakeCondition::NewEvents
                | WakeCondition::EventMatching(_)
                | WakeCondition::ThresholdEvents { .. } => Some(*context_id),
                _ => None,
            })
            .collect();

        for context_id in contexts_to_wake {
            self.wake_context(context_id);
        }
    }

    /// Notify that events are available globally (wake all waiting contexts)
    pub fn notify_events_available_globally(&mut self) {
        let contexts_to_wake: Vec<Uuid> = self
            .waiting_contexts
            .iter()
            .filter_map(|(context_id, condition)| match condition {
                WakeCondition::NewEvents
                | WakeCondition::EventMatching(_)
                | WakeCondition::ThresholdEvents { .. } => Some(*context_id),
                _ => None,
            })
            .collect();

        for context_id in contexts_to_wake {
            self.wake_context(context_id);
        }
    }

    pub fn condition_satisfied(&self, condition: &WakeCondition) -> bool {
        match condition {
            WakeCondition::EpochReached(target) => self.current_tick >= *target,
            WakeCondition::TimeoutAt(target) => self.current_tick >= *target,
            // Events conditions are satisfied externally via notify_events_available
            // We return false here to ensure they go through the notification system
            WakeCondition::NewEvents
            | WakeCondition::EventMatching(_)
            | WakeCondition::ThresholdEvents { .. } => false,
        }
    }

    fn wake_context(&mut self, context_id: Uuid) {
        self.waiting_contexts.remove(&context_id);
        if let Some(waker) = self.context_wakers.remove(&context_id) {
            let _ = waker.send(()); // Wake the waiting context
        }
    }
}

impl Default for SimulationScheduler {
    fn default() -> Self {
        Self::new()
    }
}
