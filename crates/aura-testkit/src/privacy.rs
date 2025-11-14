//! Privacy-oriented test helpers.
//!
//! These utilities simulate the perspective of an external observer
//! watching rendezvous or search traffic. They let us assert that
//! observables remain indistinguishable when contexts are permuted,
//! which is the informal requirement behind τ[κ₁↔κ₂] ≈ₑₓₜ τ.

use std::time::Duration;

/// Minimal observable event recorded by the privacy observer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObserverEvent {
    /// Length of the actual payload
    pub payload_len: usize,
    /// Length of padding added
    pub pad_len: u16,
    /// Time between message arrivals
    pub inter_arrival: Duration,
}

/// A simple observer that records envelope metadata.
#[derive(Debug, Default)]
pub struct PrivacyObserver {
    events: Vec<ObserverEvent>,
}

impl PrivacyObserver {
    /// Create a new privacy observer
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    /// Record an observed event
    pub fn observe(&mut self, event: ObserverEvent) {
        self.events.push(event);
    }

    /// Get the recorded trace of events
    pub fn trace(&self) -> &[ObserverEvent] {
        &self.events
    }
}

/// Simulate a rendezvous trace from the point-of-view of an observer.
///
/// The trace intentionally depends only on protocol-level parameters
/// (number of envelopes, fixed padding schedule) so that swapping the
/// logical context produces the same observable sequence.
pub fn simulate_rendezvous_trace(envelope_count: usize) -> Vec<ObserverEvent> {
    (0..envelope_count)
        .map(|_| ObserverEvent {
            payload_len: 2048,
            pad_len: 32,
            inter_arrival: Duration::from_millis(250),
        })
        .collect()
}

/// Utility to compare two traces and return true if they are
/// indistinguishable to an external observer.
pub fn traces_equivalent(trace_a: &[ObserverEvent], trace_b: &[ObserverEvent]) -> bool {
    trace_a == trace_b
}
