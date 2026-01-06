//! Protocol Observer for Event Tracing
//!
//! This module provides a `ProtocolObserver` implementation for simulation
//! environments, enabling detailed event tracing during protocol execution.
//!
//! ## Features
//!
//! - Phase transition tracking (start/end with timing)
//! - Message send/receive events with metadata
//! - Choice/branch tracking for protocol decisions
//! - Error and timeout event recording
//! - Full trace export for analysis
//!
//! ## Example
//!
//! ```ignore
//! use aura_simulator::choreography_observer::{SimulatorObserver, ProtocolEvent};
//!
//! let observer = SimulatorObserver::new("AuraConsensus");
//!
//! // Events are recorded during protocol execution
//! observer.on_phase_start("Coordinator", "broadcast_execute");
//! observer.on_send("Coordinator", "Witness[0]", "ConsensusMessage", 256);
//! observer.on_phase_end("Coordinator", "broadcast_execute");
//!
//! // Export trace for analysis
//! let events = observer.take_events();
//! ```

use parking_lot::RwLock;
use std::time::{Duration, Instant};

/// Events that can occur during protocol execution
#[derive(Debug, Clone)]
pub enum ProtocolEvent {
    /// A protocol phase has started
    PhaseStart {
        /// Name of the role executing
        role: String,
        /// Phase identifier
        phase: String,
        /// Timestamp relative to protocol start
        timestamp: Duration,
    },

    /// A protocol phase has completed
    PhaseEnd {
        /// Name of the role executing
        role: String,
        /// Phase identifier
        phase: String,
        /// Timestamp relative to protocol start
        timestamp: Duration,
        /// Duration of the phase
        duration: Duration,
    },

    /// A message was sent
    Send {
        /// Sender role
        from: String,
        /// Receiver role
        to: String,
        /// Message type name
        message_type: String,
        /// Serialized message size in bytes
        size: usize,
        /// Timestamp relative to protocol start
        timestamp: Duration,
    },

    /// A message was received
    Receive {
        /// Sender role
        from: String,
        /// Receiver role
        to: String,
        /// Message type name
        message_type: String,
        /// Serialized message size in bytes
        size: usize,
        /// Timestamp relative to protocol start
        timestamp: Duration,
    },

    /// A choice/branch was selected
    Choice {
        /// Role making the choice
        role: String,
        /// Branch that was selected
        branch: String,
        /// Timestamp relative to protocol start
        timestamp: Duration,
    },

    /// An error occurred during execution
    Error {
        /// Role where error occurred
        role: String,
        /// Error message
        message: String,
        /// Timestamp relative to protocol start
        timestamp: Duration,
    },

    /// A timeout occurred
    Timeout {
        /// Role that timed out
        role: String,
        /// What operation timed out
        operation: String,
        /// Timestamp relative to protocol start
        timestamp: Duration,
    },

    /// Protocol completed successfully
    Complete {
        /// Timestamp relative to protocol start
        timestamp: Duration,
        /// Total protocol duration
        total_duration: Duration,
    },
}

/// Observer for tracking protocol execution events
pub struct SimulatorObserver {
    /// Protocol name being observed
    protocol_name: String,
    /// Start time of the protocol
    start_time: Instant,
    /// Collected events
    events: RwLock<Vec<ProtocolEvent>>,
    /// Phase start times for duration calculation
    phase_starts: RwLock<std::collections::HashMap<(String, String), Instant>>,
}

impl SimulatorObserver {
    /// Create a new observer for the given protocol
    pub fn new(protocol_name: impl Into<String>) -> Self {
        Self {
            protocol_name: protocol_name.into(),
            start_time: Instant::now(),
            events: RwLock::new(Vec::new()),
            phase_starts: RwLock::new(std::collections::HashMap::new()),
        }
    }

    /// Get the protocol name
    pub fn protocol_name(&self) -> &str {
        &self.protocol_name
    }

    /// Record the start of a protocol phase
    pub fn on_phase_start(&self, role: &str, phase: &str) {
        let timestamp = self.start_time.elapsed();
        let key = (role.to_string(), phase.to_string());

        self.phase_starts.write().insert(key, Instant::now());

        self.events.write().push(ProtocolEvent::PhaseStart {
            role: role.to_string(),
            phase: phase.to_string(),
            timestamp,
        });
    }

    /// Record the end of a protocol phase
    pub fn on_phase_end(&self, role: &str, phase: &str) {
        let timestamp = self.start_time.elapsed();
        let key = (role.to_string(), phase.to_string());

        let duration = self
            .phase_starts
            .write()
            .remove(&key)
            .map(|start| start.elapsed())
            .unwrap_or(Duration::ZERO);

        self.events.write().push(ProtocolEvent::PhaseEnd {
            role: role.to_string(),
            phase: phase.to_string(),
            timestamp,
            duration,
        });
    }

    /// Record a message send event
    pub fn on_send(&self, from: &str, to: &str, message_type: &str, size: usize) {
        let timestamp = self.start_time.elapsed();

        self.events.write().push(ProtocolEvent::Send {
            from: from.to_string(),
            to: to.to_string(),
            message_type: message_type.to_string(),
            size,
            timestamp,
        });
    }

    /// Record a message receive event
    pub fn on_receive(&self, from: &str, to: &str, message_type: &str, size: usize) {
        let timestamp = self.start_time.elapsed();

        self.events.write().push(ProtocolEvent::Receive {
            from: from.to_string(),
            to: to.to_string(),
            message_type: message_type.to_string(),
            size,
            timestamp,
        });
    }

    /// Record a choice/branch selection
    pub fn on_choice(&self, role: &str, branch: &str) {
        let timestamp = self.start_time.elapsed();

        self.events.write().push(ProtocolEvent::Choice {
            role: role.to_string(),
            branch: branch.to_string(),
            timestamp,
        });
    }

    /// Record an error event
    pub fn on_error(&self, role: &str, message: &str) {
        let timestamp = self.start_time.elapsed();

        self.events.write().push(ProtocolEvent::Error {
            role: role.to_string(),
            message: message.to_string(),
            timestamp,
        });
    }

    /// Record a timeout event
    pub fn on_timeout(&self, role: &str, operation: &str) {
        let timestamp = self.start_time.elapsed();

        self.events.write().push(ProtocolEvent::Timeout {
            role: role.to_string(),
            operation: operation.to_string(),
            timestamp,
        });
    }

    /// Record protocol completion
    pub fn on_complete(&self) {
        let timestamp = self.start_time.elapsed();

        self.events.write().push(ProtocolEvent::Complete {
            timestamp,
            total_duration: timestamp,
        });
    }

    /// Get a copy of all collected events
    pub fn events(&self) -> Vec<ProtocolEvent> {
        self.events.read().clone()
    }

    /// Take all collected events, leaving the observer empty
    pub fn take_events(&self) -> Vec<ProtocolEvent> {
        std::mem::take(&mut *self.events.write())
    }

    /// Get the number of events collected
    pub fn event_count(&self) -> usize {
        self.events.read().len()
    }

    /// Clear all collected events
    pub fn clear(&self) {
        self.events.write().clear();
        self.phase_starts.write().clear();
    }

    /// Reset the observer for a new protocol execution
    pub fn reset(&mut self) {
        self.clear();
        self.start_time = Instant::now();
    }

    /// Get statistics about the protocol execution
    pub fn statistics(&self) -> ObserverStatistics {
        let events = self.events.read();

        let mut stats = ObserverStatistics::default();

        for event in events.iter() {
            match event {
                ProtocolEvent::Send { size, .. } => {
                    stats.messages_sent += 1;
                    stats.bytes_sent += size;
                }
                ProtocolEvent::Receive { size, .. } => {
                    stats.messages_received += 1;
                    stats.bytes_received += size;
                }
                ProtocolEvent::PhaseStart { .. } => {
                    stats.phases_started += 1;
                }
                ProtocolEvent::PhaseEnd { duration, .. } => {
                    stats.phases_completed += 1;
                    stats.total_phase_time += *duration;
                }
                ProtocolEvent::Choice { .. } => {
                    stats.choices_made += 1;
                }
                ProtocolEvent::Error { .. } => {
                    stats.errors += 1;
                }
                ProtocolEvent::Timeout { .. } => {
                    stats.timeouts += 1;
                }
                ProtocolEvent::Complete { total_duration, .. } => {
                    stats.total_duration = Some(*total_duration);
                }
            }
        }

        stats
    }
}

impl Default for SimulatorObserver {
    fn default() -> Self {
        Self::new("unknown")
    }
}

/// Statistics about protocol execution
#[derive(Debug, Clone, Default)]
pub struct ObserverStatistics {
    /// Number of messages sent
    pub messages_sent: usize,
    /// Number of messages received
    pub messages_received: usize,
    /// Total bytes sent
    pub bytes_sent: usize,
    /// Total bytes received
    pub bytes_received: usize,
    /// Number of phases started
    pub phases_started: usize,
    /// Number of phases completed
    pub phases_completed: usize,
    /// Total time spent in phases
    pub total_phase_time: Duration,
    /// Number of choices made
    pub choices_made: usize,
    /// Number of errors
    pub errors: usize,
    /// Number of timeouts
    pub timeouts: usize,
    /// Total protocol duration (if completed)
    pub total_duration: Option<Duration>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_observer_phase_tracking() {
        let observer = SimulatorObserver::new("TestProtocol");

        observer.on_phase_start("Alice", "handshake");
        std::thread::sleep(Duration::from_millis(10));
        observer.on_phase_end("Alice", "handshake");

        let events = observer.events();
        assert_eq!(events.len(), 2);

        if let ProtocolEvent::PhaseStart { role, phase, .. } = &events[0] {
            assert_eq!(role, "Alice");
            assert_eq!(phase, "handshake");
        } else {
            panic!("Expected PhaseStart");
        }

        if let ProtocolEvent::PhaseEnd { duration, .. } = &events[1] {
            assert!(duration.as_millis() >= 10);
        } else {
            panic!("Expected PhaseEnd");
        }
    }

    #[test]
    fn test_observer_message_tracking() {
        let observer = SimulatorObserver::new("TestProtocol");

        observer.on_send("Alice", "Bob", "Ping", 128);
        observer.on_receive("Alice", "Bob", "Ping", 128);

        let stats = observer.statistics();
        assert_eq!(stats.messages_sent, 1);
        assert_eq!(stats.messages_received, 1);
        assert_eq!(stats.bytes_sent, 128);
        assert_eq!(stats.bytes_received, 128);
    }

    #[test]
    fn test_observer_statistics() {
        let observer = SimulatorObserver::new("TestProtocol");

        observer.on_phase_start("Alice", "init");
        observer.on_send("Alice", "Bob", "Hello", 64);
        observer.on_choice("Alice", "Continue");
        observer.on_phase_end("Alice", "init");
        observer.on_complete();

        let stats = observer.statistics();
        assert_eq!(stats.phases_started, 1);
        assert_eq!(stats.phases_completed, 1);
        assert_eq!(stats.messages_sent, 1);
        assert_eq!(stats.choices_made, 1);
        assert!(stats.total_duration.is_some());
    }

    #[test]
    fn test_observer_reset() {
        let mut observer = SimulatorObserver::new("TestProtocol");

        observer.on_send("Alice", "Bob", "Test", 32);
        assert_eq!(observer.event_count(), 1);

        observer.reset();
        assert_eq!(observer.event_count(), 0);
    }
}
