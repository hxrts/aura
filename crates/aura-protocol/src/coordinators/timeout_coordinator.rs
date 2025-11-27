//! Timeout Coordinator - Multi-Context Timeout Management
//!
//! **Layer 4 (aura-protocol)**: Stateful multi-context coordination handler.
//!
//! This module was extracted from aura-effects/src/time.rs RealTimeHandler because it violates
//! the Layer 3 principle of "stateless, single-party, context-free" handlers. The timeout
//! coordination logic maintains shared state across multiple contexts, making it multi-party
//! coordination logic that belongs in the orchestration layer.
//!
//! Key violations that required the extraction:
//! - Maintains global timeout registry (`Arc<RwLock<HashMap<Uuid, JoinHandle>>>`)
//! - Maintains global context registry (`Arc<RwLock<HashMap<Uuid, broadcast::Sender>>>`)
//! - Manages timeouts across multiple contexts (multi-party coordination)
//! - Broadcasts events to all registered contexts
//! - Tracks timeout tasks globally for cancellation

use async_lock::RwLock;
use async_trait::async_trait;
use aura_core::effects::{PhysicalTimeEffects, RandomEffects, TimeError};
use futures::channel::mpsc::UnboundedSender;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/// Context registry for managing time contexts and timeouts
#[derive(Debug, Default)]
struct ContextRegistry {
    contexts: HashMap<Uuid, UnboundedSender<()>>,
}

/// Timeout coordinator that adds multi-context coordination to a base TimeEffects handler
#[derive(Debug, Clone)]
pub struct TimeoutCoordinator<T, R> {
    /// Base time handler for stateless operations
    inner: T,
    /// Random effects for UUID generation
    random: R,
    /// Shared registry for coordinating timeouts and contexts
    registry: Arc<RwLock<ContextRegistry>>,
}

impl<T: PhysicalTimeEffects + Clone, R: RandomEffects + Clone> TimeoutCoordinator<T, R> {
    /// Create a new timeout coordinator wrapping a base time handler and random effects
    pub fn new(inner: T, random: R) -> Self {
        Self {
            inner,
            random,
            registry: Arc::new(RwLock::new(ContextRegistry::default())),
        }
    }
}

#[async_trait]
impl<T: PhysicalTimeEffects + Clone, R: RandomEffects + Clone> PhysicalTimeEffects
    for TimeoutCoordinator<T, R>
{
    async fn physical_time(&self) -> Result<aura_core::time::PhysicalTime, TimeError> {
        self.inner.physical_time().await
    }

    async fn sleep_ms(&self, ms: u64) -> Result<(), TimeError> {
        self.inner.sleep_ms(ms).await
    }
}
