//! Acknowledgment routing service for fact delivery confirmation.
//!
//! This module provides the `AckRouter` service that bridges the transport layer
//! (where acks are received) with the journal layer (where acks are stored).
//!
//! # Architecture
//!
//! ```text
//! Transport Layer                 Ack Router                    Journal Layer
//! ┌─────────────┐    FactAck     ┌──────────┐    AckStorage    ┌─────────────┐
//! │ Anti-Entropy│ ──────────────>│ AckRouter│ ────────────────>│   Journal   │
//! └─────────────┘                └──────────┘                  └─────────────┘
//!                                     │
//!                                     │ Signals
//!                                     ▼
//!                               ┌───────────────┐
//!                               │ App/TUI Layer │
//!                               └───────────────┘
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use aura_sync::services::ack_routing::{AckRouter, AckSignal};
//! use aura_anti_entropy::FactAck;
//!
//! // Create router with signal callback
//! let router = AckRouter::new(|signal| {
//!     match signal {
//!         AckSignal::AckReceived { fact_id, from } => {
//!             println!("Ack received for {:?} from {:?}", fact_id, from);
//!         }
//!         AckSignal::FullyAcked { fact_id } => {
//!             println!("Fact {:?} fully acked", fact_id);
//!         }
//!     }
//! });
//!
//! // Route incoming ack to journal
//! router.route_ack(&mut journal.ack_storage, &fact_ack, &expected_peers)?;
//! ```

use aura_anti_entropy::FactAck;
use aura_core::identifiers::AuthorityId;
use aura_core::time::OrderTime;
use aura_core::AuraError;
use aura_journal::AckStorage;
use std::sync::Arc;

// =============================================================================
// Ack Signals
// =============================================================================

/// Signals emitted when acks are routed.
///
/// These signals can be used by higher layers (app, TUI) to update
/// optimistic UI state in real-time.
#[derive(Debug, Clone)]
pub enum AckSignal {
    /// An acknowledgment was received from a peer
    AckReceived {
        /// The fact that was acknowledged
        fact_id: OrderTime,
        /// The peer that sent the ack
        from: AuthorityId,
    },

    /// A fact is now fully acknowledged by all expected peers
    FullyAcked {
        /// The fact that is fully acked
        fact_id: OrderTime,
    },

    /// An ack was received but routing failed
    RoutingFailed {
        /// The fact that failed
        fact_id: OrderTime,
        /// Error description
        error: String,
    },
}

// =============================================================================
// Ack Signal Callback
// =============================================================================

/// Callback trait for receiving ack signals.
pub trait AckSignalCallback: Send + Sync {
    /// Called when an ack signal is emitted
    fn on_ack_signal(&self, signal: AckSignal);
}

/// No-op implementation
pub struct NoOpAckSignalCallback;

impl AckSignalCallback for NoOpAckSignalCallback {
    fn on_ack_signal(&self, _signal: AckSignal) {
        // Intentionally empty
    }
}

/// Logging implementation
pub struct LoggingAckSignalCallback;

impl AckSignalCallback for LoggingAckSignalCallback {
    fn on_ack_signal(&self, signal: AckSignal) {
        match &signal {
            AckSignal::AckReceived { fact_id, from } => {
                tracing::debug!("Ack received for {:?} from {:?}", fact_id, from);
            }
            AckSignal::FullyAcked { fact_id } => {
                tracing::info!("Fact {:?} fully acknowledged", fact_id);
            }
            AckSignal::RoutingFailed { fact_id, error } => {
                tracing::warn!("Ack routing failed for {:?}: {}", fact_id, error);
            }
        }
    }
}

/// Function-based callback wrapper
pub struct FnAckSignalCallback<F>(F);

impl<F> FnAckSignalCallback<F>
where
    F: Fn(AckSignal) + Send + Sync,
{
    /// Create a new function-based callback
    pub fn new(f: F) -> Self {
        Self(f)
    }
}

impl<F> AckSignalCallback for FnAckSignalCallback<F>
where
    F: Fn(AckSignal) + Send + Sync,
{
    fn on_ack_signal(&self, signal: AckSignal) {
        (self.0)(signal);
    }
}

// =============================================================================
// Ack Router
// =============================================================================

/// Routes acknowledgments from transport to journal storage.
///
/// The `AckRouter` is responsible for:
/// 1. Receiving `FactAck` messages from the transport layer
/// 2. Storing acks in the journal's `AckStorage`
/// 3. Emitting signals for real-time UI updates
/// 4. Detecting when a fact is fully acknowledged
pub struct AckRouter {
    /// Signal callback for ack events
    callback: Arc<dyn AckSignalCallback>,
}

impl AckRouter {
    /// Create a new ack router with the given signal callback
    pub fn new(callback: impl AckSignalCallback + 'static) -> Self {
        Self {
            callback: Arc::new(callback),
        }
    }

    /// Create a new ack router with no callbacks
    pub fn no_op() -> Self {
        Self::new(NoOpAckSignalCallback)
    }

    /// Create a new ack router with logging
    pub fn with_logging() -> Self {
        Self::new(LoggingAckSignalCallback)
    }

    /// Route a received ack to journal storage.
    ///
    /// # Arguments
    ///
    /// * `ack_storage` - The journal's ack storage
    /// * `ack` - The received ack from transport
    /// * `expected_peers` - Peers expected to ack this fact (for full ack detection)
    ///
    /// # Returns
    ///
    /// `Ok(true)` if the fact is now fully acked, `Ok(false)` otherwise.
    pub fn route_ack(
        &self,
        ack_storage: &mut AckStorage,
        ack: &FactAck,
        expected_peers: &[AuthorityId],
    ) -> Result<bool, AuraError> {
        // Record the ack in storage
        let result = ack_storage.record_ack(&ack.fact_id, ack.acknowledger, ack.acked_at.clone());

        match result {
            Ok(()) => {
                // Emit AckReceived signal
                self.callback.on_ack_signal(AckSignal::AckReceived {
                    fact_id: ack.fact_id.clone(),
                    from: ack.acknowledger,
                });

                // Check if fully acked
                let is_fully_acked = ack_storage.all_acked(&ack.fact_id, expected_peers);

                if is_fully_acked {
                    self.callback.on_ack_signal(AckSignal::FullyAcked {
                        fact_id: ack.fact_id.clone(),
                    });
                }

                Ok(is_fully_acked)
            }
            Err(e) => {
                self.callback.on_ack_signal(AckSignal::RoutingFailed {
                    fact_id: ack.fact_id.clone(),
                    error: e.to_string(),
                });
                Err(e)
            }
        }
    }

    /// Route multiple acks in batch.
    ///
    /// # Returns
    ///
    /// Number of facts that became fully acked.
    pub fn route_acks(
        &self,
        ack_storage: &mut AckStorage,
        acks: &[FactAck],
        expected_peers: &[AuthorityId],
    ) -> Result<usize, AuraError> {
        let mut fully_acked_count = 0;

        for ack in acks {
            if self.route_ack(ack_storage, ack, expected_peers)? {
                fully_acked_count += 1;
            }
        }

        Ok(fully_acked_count)
    }

    /// Get a reference to the signal callback (for testing)
    pub fn callback(&self) -> &Arc<dyn AckSignalCallback> {
        &self.callback
    }
}

impl Default for AckRouter {
    fn default() -> Self {
        Self::no_op()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::time::PhysicalTime;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn test_authority(n: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([n; 32])
    }

    fn test_order_time(n: u8) -> OrderTime {
        OrderTime([n; 32])
    }

    fn test_physical_time(ts_ms: u64) -> PhysicalTime {
        PhysicalTime {
            ts_ms,
            uncertainty: None,
        }
    }

    #[test]
    fn test_route_single_ack() {
        let signal_count = Arc::new(AtomicUsize::new(0));
        let signal_count_clone = signal_count.clone();

        let router = AckRouter::new(FnAckSignalCallback::new(move |_| {
            signal_count_clone.fetch_add(1, Ordering::SeqCst);
        }));

        let mut ack_storage = AckStorage::new();
        let expected_peers = vec![test_authority(1), test_authority(2)];

        let ack = FactAck::new(
            test_order_time(1),
            test_authority(1),
            test_physical_time(1000),
        );

        let is_fully_acked = router
            .route_ack(&mut ack_storage, &ack, &expected_peers)
            .unwrap();

        assert!(!is_fully_acked);
        assert_eq!(signal_count.load(Ordering::SeqCst), 1); // AckReceived
    }

    #[test]
    fn test_route_ack_fully_acked() {
        let fully_acked = Arc::new(AtomicUsize::new(0));
        let fully_acked_clone = fully_acked.clone();

        let router = AckRouter::new(FnAckSignalCallback::new(move |signal| {
            if matches!(signal, AckSignal::FullyAcked { .. }) {
                fully_acked_clone.fetch_add(1, Ordering::SeqCst);
            }
        }));

        let mut ack_storage = AckStorage::new();
        let expected_peers = vec![test_authority(1), test_authority(2)];
        let fact_id = test_order_time(1);

        // First ack
        let ack1 = FactAck::new(fact_id.clone(), test_authority(1), test_physical_time(1000));
        let is_fully_acked = router
            .route_ack(&mut ack_storage, &ack1, &expected_peers)
            .unwrap();
        assert!(!is_fully_acked);

        // Second ack - should be fully acked now
        let ack2 = FactAck::new(fact_id, test_authority(2), test_physical_time(2000));
        let is_fully_acked = router
            .route_ack(&mut ack_storage, &ack2, &expected_peers)
            .unwrap();
        assert!(is_fully_acked);
        assert_eq!(fully_acked.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_route_batch() {
        let router = AckRouter::no_op();
        let mut ack_storage = AckStorage::new();

        let expected_peers = vec![test_authority(1)];

        let acks = vec![
            FactAck::new(
                test_order_time(1),
                test_authority(1),
                test_physical_time(1000),
            ),
            FactAck::new(
                test_order_time(2),
                test_authority(1),
                test_physical_time(2000),
            ),
        ];

        let fully_acked_count = router
            .route_acks(&mut ack_storage, &acks, &expected_peers)
            .unwrap();

        assert_eq!(fully_acked_count, 2);
    }
}
